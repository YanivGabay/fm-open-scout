//! Layout discovery for versions without a registered layout (FM20–FM23, future
//! releases). Every offset is chosen by a statistic over thousands of player
//! blocks, never by a single example, and each must pass a threshold or stay
//! `None`. Run against FM24 and FM26 the probe reproduces their verified layouts
//! (integration test `probe_reproduces_verified_layouts`).

use crate::bytes::{u16_at, u8_at};
use crate::clubs::ClubTable;
use crate::layout::{self, PlayerLayout};
use crate::names::NameTables;
use crate::players::{find_name_block, find_player_blocks, probe_layout, NameLimits};
use serde::Serialize;

const SAMPLE: usize = 6000;

#[derive(Debug, Clone, Serialize)]
pub struct ProbeReport {
    pub header_len: usize,
    pub sample: usize,
    /// Share of the sample supporting each chosen offset (0..1).
    pub club: Option<(usize, f32)>,
    pub nation_after_name: Option<(usize, f32)>,
    pub personality_after_name: Option<(usize, f32)>,
    pub height_in_tail: Option<(usize, f32)>,
    pub weight_in_tail: Option<(usize, f32)>,
}

fn best<F: Fn(usize) -> f32>(range: impl Iterator<Item = usize>, score: F, min: f32) -> Option<(usize, f32)> {
    range.map(|o| (o, score(o))).filter(|&(_, s)| s >= min).max_by(|a, b| a.1.total_cmp(&b.1))
}

fn share(n: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        n as f32 / total as f32
    }
}

/// Full probe: core layout plus every optional offset that clears its threshold.
pub fn probe(d: &[u8], names: Option<&NameTables>, clubs: Option<&ClubTable>) -> Option<(PlayerLayout, ProbeReport)> {
    let core = probe_layout(d)?;
    let mut lay = layout::probed(core.header_len);
    let blocks = find_player_blocks(d, &lay);
    let step = (blocks.len() / SAMPLE).max(1);
    let sample: Vec<usize> = blocks.iter().step_by(step).copied().collect();

    // Club id: the u16 in the ability header whose values are team ids of parsed clubs.
    // A real club field spreads over thousands of teams; constant bytes that happen
    // to form a valid team id do not.
    let club = clubs.and_then(|t| {
        best(
            4..4 + core.header_len - 1,
            |o| {
                let vals: Vec<u16> = sample.iter().filter_map(|&r| u16_at(d, r + o)).filter(|&v| t.team(v as u32).is_some()).collect();
                let distinct = vals.iter().collect::<std::collections::HashSet<_>>().len();
                if distinct * 10 < sample.len() {
                    0.0
                } else {
                    share(vals.len(), sample.len())
                }
            },
            0.5,
        )
    });
    lay.club = club.map(|c| c.0);

    // Name blocks for the sample (needed for nationality and personality).
    let tails: Vec<(usize, Option<usize>)> = sample
        .iter()
        .map(|&r| {
            let after = names.and_then(|n| find_name_block(d, r + lay.tail(), r + lay.tail() + 32 * 1024, n, None, &NameLimits::default())).map(|b| b.after);
            (r, after)
        })
        .collect();
    let named: Vec<(usize, usize)> = tails.iter().filter_map(|&(r, a)| a.map(|a| (r, a))).collect();

    // Nationality: the u16 after the name block that most often equals the
    // nation of the player's club (players mostly play in their own country).
    let nation = clubs.zip(lay.club).and_then(|(t, co)| {
        let rows: Vec<(usize, u32)> = named
            .iter()
            .filter_map(|&(r, a)| t.team(u16_at(d, r + co)? as u32).and_then(|(c, _)| c.nation_index).map(|n| (a, n)))
            .collect();
        best(4..32, |o| share(rows.iter().filter(|&&(a, n)| u16_at(d, a + o) == Some(n as u16)).count(), rows.len()), 0.4)
    });
    lay.nation_after_name = nation.map(|n| n.0);

    // Personality: 8 consecutive bytes in 1..20, after the nationality.
    let personality = lay.nation_after_name.and_then(|n0| {
        best(n0 + 2..n0 + 24, |o| share(named.iter().filter(|&&(_, a)| d.get(a + o..a + o + 8).is_some_and(|b| b.iter().all(|v| (1..=20).contains(v)))).count(), named.len()), 0.8)
    });
    lay.personality_after_name = personality.map(|p| p.0);

    // Height / weight in the tail: bytes in human ranges with the right means.
    let tail_off = lay.tail();
    let tail_stat = |o: usize, lo: u8, hi: u8, mean_lo: f32, mean_hi: f32| {
        let vals: Vec<u8> = sample.iter().filter_map(|&r| u8_at(d, r + tail_off + o)).collect();
        let inr: Vec<u8> = vals.iter().copied().filter(|v| (lo..=hi).contains(v)).collect();
        let mean = inr.iter().map(|&v| v as f32).sum::<f32>() / inr.len().max(1) as f32;
        if (mean_lo..=mean_hi).contains(&mean) {
            share(inr.len(), vals.len())
        } else {
            0.0
        }
    };
    let height = best(0..32, |o| tail_stat(o, 150, 215, 176.0, 188.0), 0.95);
    lay.height_in_tail = height.map(|h| h.0);
    let height_off = height.map(|h| h.0);
    let weight = best((0..32).filter(|&o| Some(o) != height_off), |o| tail_stat(o, 50, 110, 68.0, 82.0), 0.95);
    lay.weight_in_tail = weight.map(|w| w.0);

    let report = ProbeReport {
        header_len: core.header_len,
        sample: sample.len(),
        club,
        nation_after_name: nation,
        personality_after_name: personality,
        height_in_tail: height,
        weight_in_tail: weight,
    };
    Some((lay, report))
}
