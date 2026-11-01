//! Player extraction from the decompressed `game_db.dat`.
//!
//! The database is one long serialised object graph without a published schema,
//! so players are found by an anchored scan rather than by walking the object tree:
//!
//! 1. **Anchor:** the person UID stored twice (`r-15`, `r-11`), followed by a
//!    plausible ability block: CA 1..200, PA -10..200, 15 position ratings in 1..20,
//!    54 attributes in 1..100, and valid dates at the two date slots after the
//!    attributes. Every candidate must pass all checks, which is far stricter
//!    than the null-date marker scan earlier tools used (that scan also misses the
//!    ~1–2 % of players whose second date is set).
//! 2. **Name block:** between the end of the player block and the next player, the
//!    first offset where the person name block validates:
//!    ```text
//!    u32 first-name id, u8 0, u32 surname id, u8 0, u32 common-name id (or FFFFFFFF), u8 0,
//!    u32 len, full name (UTF-8), date of birth (u16 day-of-year, u16 year) ...
//!    ```
//!    ids must be inside the respective name tables and the birth date must be real.

use crate::bytes::{i16_at, u16_at, u32_at, u8_at, FmDate};
use crate::layout::{
    self, PlayerLayout, ATTRIBUTE_COUNT, ATTRIBUTE_NAMES, PERSONALITY_COUNT, PERSONALITY_NAMES,
    POSITION_COUNT, POSITION_NAMES, REPUTATION_OFFSET, UID_OFFSET,
};
use crate::clubs::ClubTable;
use crate::contracts::{self, Contract, LastTransfer, ParentContract};
use crate::names::NameTables;
use crate::nations::{NameSource, Nations};
use serde::ser::{Serialize, SerializeMap, Serializer};
use serde::Serialize as DeriveSerialize;

/// How far past a player block the name block is searched for. Observed maximum
/// distance: ~9 KB (FM24), ~14 KB (FM26); 99 % lie within 2 KB.
const NAME_SEARCH_LIMIT: usize = 32 * 1024;
const MAX_FULL_NAME: u32 = 160;
const MAX_NATIONS: u16 = 512;

/// Save-wide limits that reject false name blocks: a nationality outside the
/// nation table, or a birth date after the in-game date. Without them, some person
/// objects contain byte runs that pass every structural check (FM27: ~850 players
/// got a stranger's name, all with nation index >= 251 and a birth year like 2048).
#[derive(Debug, Clone)]
pub struct NameLimits {
    /// Number of nations in the nation table (indexes must be below it).
    pub nations: u16,
    /// In-game date (ISO); no one can be born after it.
    pub latest_birth: Option<String>,
}

impl Default for NameLimits {
    fn default() -> Self {
        NameLimits { nations: MAX_NATIONS, latest_birth: None }
    }
}

/// Values serialised as an ordered `{name: value}` object.
#[derive(Debug, Clone)]
pub struct Named {
    pub names: &'static [&'static str],
    pub values: Vec<u8>,
}

impl Named {
    pub fn get(&self, name: &str) -> Option<u8> {
        self.names.iter().position(|n| *n == name).map(|i| self.values[i])
    }
}

impl Serialize for Named {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.values.len()))?;
        for (n, v) in self.names.iter().zip(&self.values) {
            m.serialize_entry(n, v)?;
        }
        m.end()
    }
}

#[derive(Debug, Clone, DeriveSerialize)]
pub struct Reputation {
    pub home: u16,
    pub current: u16,
    pub world: u16,
}

/// The club owning the player's team (exact mapping, see `clubs.rs`).
#[derive(Debug, Clone, DeriveSerialize)]
pub struct ClubRef {
    pub uid: u32,
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub nation_index: Option<u32>,
    /// Position of the team in the club's team list: 0 = first team; higher slots
    /// are second/youth teams (slot 1 median age 19, slot 2 median age 17).
    pub team_slot: u8,
}

#[derive(Debug, Clone, DeriveSerialize)]
pub struct Player {
    pub uid: u32,
    /// Offset of the Current Ability field in `game_db.dat` (`r`).
    pub db_offset: usize,
    /// Display name: common name if set, else "first last", else the inline full name.
    pub name: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub common_name: Option<String>,
    pub full_name: Option<String>,
    pub date_of_birth: Option<String>,
    /// Index into the save's nation table (e.g. 139 = England in FM24/FM26).
    pub nation_index: Option<u16>,
    /// FM's stable nation UID (from the nation table).
    pub nation_uid: Option<u32>,
    /// Nation name, only when exact (see `nations.rs`).
    pub nation_name: Option<String>,
    pub nation_name_source: Option<NameSource>,
    pub current_ability: u16,
    pub potential_ability: i16,
    pub reputation: Reputation,
    /// Team id (index into FM's team list) of the player's club team.
    pub club_id: Option<u16>,
    /// Club owning `club_id`; `None` when the id is not in a parsed club team list.
    pub club: Option<ClubRef>,
    pub international_team_id: Option<u16>,
    /// National-team name for `international_team_id`, when the save contains it.
    pub international_team_name: Option<String>,
    pub joined_club: Option<String>,
    pub second_date: Option<String>,
    /// Current contract (wage, dates); `None` when no contract structure is found.
    /// For a player on loan this is the loan deal (`contract.loan`).
    pub contract: Option<Contract>,
    /// For a player on loan: his contract with the parent club.
    pub loan_from: Option<ParentContract>,
    /// Fee and signing wage of the transfer that brought the player to his club.
    pub last_transfer: Option<LastTransfer>,
    /// GBP amount stored right after the attributes. Correlates with CA (r = 0.68 in
    /// FM24) and is capped at 300 000 000, but whether it is market value, asking
    /// price or something else is not established. `FFFFFFFF` (no value; ~5 % of
    /// FM26/FM27 players, all low-CA free agents) is `None`. The cap itself is shared
    /// by stars and by ~1 000 contracted CA 80-100 players, so 300 000 000 means
    /// "very high or not for sale" rather than a price.
    pub valuation_unverified: Option<u32>,
    pub height_cm: Option<u8>,
    pub weight_kg: Option<u8>,
    pub preferred_foot: &'static str,
    pub best_position: &'static str,
    pub positions: Named,
    /// Attributes on FM's 1..20 display scale (`round(raw / 5)`, clamped to 1..20).
    pub attributes: Named,
    /// Attributes as stored (1..100).
    pub attributes_raw: Named,
    pub personality: Option<Named>,
}

fn rel(r: usize, off: isize) -> Option<usize> {
    r.checked_add_signed(off)
}

/// Checks the ability block at `r` (the CA position) for `lay`.
fn ability_block_ok(d: &[u8], r: usize, lay: &PlayerLayout) -> bool {
    let (Some(ca), Some(pa)) = (u16_at(d, r), i16_at(d, r + 2)) else { return false };
    if !(1..=200).contains(&ca) || !(-10..=200).contains(&pa) {
        return false;
    }
    let p = r + lay.positions();
    let Some(pos) = d.get(p..p + POSITION_COUNT) else { return false };
    if !pos.iter().all(|v| (1..=20).contains(v)) {
        return false;
    }
    let a = r + lay.attributes();
    let Some(attrs) = d.get(a..a + ATTRIBUTE_COUNT) else { return false };
    if !attrs.iter().all(|v| (1..=100).contains(v)) {
        return false;
    }
    let date_ok = |o: usize| FmDate::read(d, r + o).is_some_and(|dt| dt.is_null() || dt.is_plausible());
    date_ok(lay.joined_date()) && date_ok(lay.second_date())
}

/// All anchor positions `r` (CA offsets), ascending.
pub fn find_player_blocks(d: &[u8], lay: &PlayerLayout) -> Vec<usize> {
    let mut out = Vec::new();
    let min_r = (-UID_OFFSET) as usize;
    let end = d.len().saturating_sub(lay.tail() + 32);
    let mut u = 0usize;
    while u + min_r < end {
        // UID repeated at u and u+4 (cheap byte compare first).
        if d[u] == d[u + 4] && d[u + 1] == d[u + 5] && d[u + 2] == d[u + 6] && d[u + 3] == d[u + 7] {
            let uid = u32::from_le_bytes([d[u], d[u + 1], d[u + 2], d[u + 3]]);
            let r = u + min_r;
            if uid != 0 && uid != u32::MAX && ability_block_ok(d, r, lay) {
                out.push(r);
                u = r + lay.tail();
                continue;
            }
        }
        u += 1;
    }
    out
}

/// Tries ability-header lengths 12..=32 on a sample of `game_db.dat` and returns the
/// one producing the most validated blocks. Used when no layout is known for a
/// version. Only core fields (UID, CA/PA, positions, attributes, names) are read
/// with a probed layout.
pub fn probe_layout(d: &[u8]) -> Option<PlayerLayout> {
    // People live in the second half of the database in FM24/FM26; sample widely.
    let sample_len = 24 * 1024 * 1024;
    let starts = [d.len() / 2, d.len() / 3, d.len() * 2 / 3];
    let mut best: Option<(usize, usize)> = None;
    for hl in 12..=32 {
        let lay = layout::probed(hl);
        let n: usize = starts
            .iter()
            .map(|&s| find_player_blocks(&d[s.min(d.len())..(s + sample_len).min(d.len())], &lay).len())
            .sum();
        if best.map_or(true, |(_, bn)| n > bn) {
            best = Some((hl, n));
        }
    }
    match best {
        Some((hl, n)) if n >= 100 => Some(layout::probed(hl)),
        _ => None,
    }
}

pub struct NameBlock {
    /// Offset of the block (first-name id).
    pub start: usize,
    pub first: Option<String>,
    pub last: Option<String>,
    pub common: Option<String>,
    pub full: Option<String>,
    pub dob: FmDate,
    /// Offset right after the full-name string (= date of birth).
    pub after: usize,
}

fn name_block_at(d: &[u8], q: usize, names: &NameTables, nation_after: Option<usize>, limits: &NameLimits, first_as_common: bool) -> Option<NameBlock> {
    if d.get(q + 4)? | d.get(q + 9)? | d.get(q + 14)? != 0 {
        return None;
    }
    let f = u32_at(d, q)?;
    let s = u32_at(d, q + 5)?;
    let c = u32_at(d, q + 10)?;
    if f as usize >= names.first.len() || s as usize >= names.last.len() {
        return None;
    }
    // Zero ids are zero-filled bytes elsewhere in the person object (the "dates"
    // there are recent, not births). Real blocks use FFFFFFFF for "no common
    // name", so common id 0 is rejected too; this costs the single person whose
    // common name is entry 0 of the table.
    if c == 0 || (f == 0 && s == 0) {
        return None;
    }
    // Players known by their first name only (single-name newgens, mostly Brazilian
    // and Spanish) store the first-name id as their common name.
    let common_is_first = first_as_common && c == f && c as usize >= names.common.len();
    if c != u32::MAX && c as usize >= names.common.len() && !common_is_first {
        return None;
    }
    let len = u32_at(d, q + 15)?;
    if len > MAX_FULL_NAME {
        return None;
    }
    let raw = d.get(q + 19..q + 19 + len as usize)?;
    let full = std::str::from_utf8(raw).ok()?;
    if full.chars().any(char::is_control) {
        return None;
    }
    let after = q + 19 + len as usize;
    let dob = FmDate::read(d, after)?;
    if dob.ymd().is_none() || !(1900..=2100).contains(&dob.year) {
        return None;
    }
    if let (Some(latest), Some(born)) = (&limits.latest_birth, dob.iso()) {
        if born > *latest {
            return None;
        }
    }
    // Every person has a nationality; an index outside the nation table means
    // this is not a name block (when the layout knows where the nation sits).
    if let Some(o) = nation_after {
        if u16_at(d, after + o)? >= limits.nations {
            return None;
        }
    }
    let nonempty = |s: Option<&str>| s.filter(|s| !s.is_empty()).map(str::to_owned);
    let first = nonempty(names.first.get(f));
    let last = nonempty(names.last.get(s));
    let common = if c == u32::MAX {
        None
    } else if common_is_first {
        first.clone()
    } else {
        nonempty(names.common.get(c))
    };
    if first.is_none() && last.is_none() && common.is_none() && full.is_empty() {
        return None;
    }
    Some(NameBlock { start: q, first, last, common, full: nonempty(Some(full)), dob, after })
}

pub fn find_name_block(d: &[u8], from: usize, to: usize, names: &NameTables, nation_after: Option<usize>, limits: &NameLimits) -> Option<NameBlock> {
    let to = to.min(from + NAME_SEARCH_LIMIT).min(d.len());
    // Single-name blocks (common id = first-name id) are accepted only when the range
    // has no ordinary block: a neighbouring newgen's single-name block can sit before
    // a player's own block, and taking it would rename him.
    (from..to)
        .find_map(|q| name_block_at(d, q, names, nation_after, limits, false))
        .or_else(|| (from..to).find_map(|q| name_block_at(d, q, names, nation_after, limits, true)))
}

fn opt_id(v: Option<u16>) -> Option<u16> {
    v.filter(|&x| x != u16::MAX)
}

fn display_attr(raw: u8) -> u8 {
    ((raw as u16 + 2) / 5).clamp(1, 20) as u8
}

pub fn read_player(
    d: &[u8],
    r: usize,
    next_r: Option<usize>,
    lay: &PlayerLayout,
    names: Option<&NameTables>,
    clubs: Option<&ClubTable>,
    limits: &NameLimits,
) -> Player {
    let uid = rel(r, UID_OFFSET).and_then(|o| u32_at(d, o)).unwrap_or(0);
    let rep = |i: isize| rel(r, REPUTATION_OFFSET + 2 * i).and_then(|o| u16_at(d, o)).unwrap_or(0);
    let pos = d[r + lay.positions()..r + lay.positions() + POSITION_COUNT].to_vec();
    let raw = d[r + lay.attributes()..r + lay.attributes() + ATTRIBUTE_COUNT].to_vec();
    let tail = r + lay.tail();

    let attributes_raw = Named { names: &ATTRIBUTE_NAMES, values: raw.clone() };
    let attributes = Named { names: &ATTRIBUTE_NAMES, values: raw.iter().map(|&v| display_attr(v)).collect() };
    let (lf, rf) = (attributes_raw.get("left_foot").unwrap_or(0), attributes_raw.get("right_foot").unwrap_or(0));
    let preferred_foot = match lf.cmp(&rf) {
        std::cmp::Ordering::Greater => "left",
        std::cmp::Ordering::Less => "right",
        std::cmp::Ordering::Equal => "either",
    };
    let best = pos.iter().enumerate().max_by_key(|(i, v)| (**v, std::cmp::Reverse(*i))).map(|(i, _)| i).unwrap_or(0);

    let block_end = next_r.map(|n| n.saturating_sub(19)).unwrap_or(d.len());
    let nb = names.and_then(|n| find_name_block(d, tail, block_end, n, lay.nation_after_name, limits));
    let after = nb.as_ref().map(|b| b.after);
    let nation_index = after.zip(lay.nation_after_name).and_then(|(a, o)| u16_at(d, a + o));
    let personality = after.zip(lay.personality_after_name).and_then(|(a, o)| {
        let v = d.get(a + o..a + o + PERSONALITY_COUNT)?.to_vec();
        v.iter().all(|x| (1..=20).contains(x)).then_some(Named { names: &PERSONALITY_NAMES, values: v })
    });
    let date_iso = |o: usize| FmDate::read(d, r + o).and_then(|dt| dt.iso());

    let name_start = nb.as_ref().map(|b| b.start);
    let club_id_raw = opt_id(lay.club.and_then(|o| u16_at(d, r + o)));
    let contract = club_id_raw.and_then(|t| contracts::find_contract(d, tail, block_end, t as u32, clubs));
    let loan_from = contract
        .as_ref()
        .filter(|c| c.loan)
        .and_then(|c| contracts::find_parent_contract(d, tail, block_end, c.team_id, clubs, limits.latest_birth.as_deref()));
    let transfer_to = name_start.unwrap_or(block_end).min(block_end);
    let extras: &[usize] = lay.transfer_extra.as_ref().map_or(&[0, 1], std::slice::from_ref);
    let gaps: &[usize] = lay.transfer_gap.as_ref().map_or(&[0, 2], std::slice::from_ref);
    let last_transfer = extras
        .iter()
        .flat_map(|&x| gaps.iter().map(move |&g| (x, g)))
        .find_map(|(x, g)| contracts::find_last_transfer(d, tail, transfer_to, x, g));
    let (first_name, last_name, common_name, full_name, dob) = match nb {
        Some(b) => (b.first, b.last, b.common, b.full, b.dob.iso()),
        None => (None, None, None, None, None),
    };
    let name = common_name.clone().or_else(|| match (&first_name, &last_name) {
        (Some(f), Some(l)) => Some(format!("{f} {l}")),
        (None, Some(l)) => Some(l.clone()),
        (Some(f), None) => Some(f.clone()),
        (None, None) => full_name.clone(),
    });

    let club_id = opt_id(lay.club.and_then(|o| u16_at(d, r + o)));
    let club = club_id.zip(clubs).and_then(|(id, t)| t.team(id as u32)).map(|(c, slot)| ClubRef {
        uid: c.uid,
        name: c.name.clone(),
        short_name: c.short_name.clone(),
        nation_index: c.nation_index,
        team_slot: slot,
    });

    Player {
        uid,
        db_offset: r,
        name,
        first_name,
        last_name,
        common_name,
        full_name,
        date_of_birth: dob,
        nation_index,
        nation_uid: None,
        nation_name: None,
        nation_name_source: None,
        current_ability: u16_at(d, r).unwrap_or(0),
        potential_ability: i16_at(d, r + 2).unwrap_or(0),
        reputation: Reputation { home: rep(0), current: rep(1), world: rep(2) },
        club_id,
        club,
        international_team_id: opt_id(lay.intl_team.and_then(|o| u16_at(d, r + o))),
        international_team_name: None,
        joined_club: date_iso(lay.joined_date()),
        second_date: date_iso(lay.second_date()),
        contract,
        loan_from,
        last_transfer,
        valuation_unverified: u32_at(d, r + lay.valuation()).filter(|&v| v != u32::MAX),
        height_cm: lay.height_in_tail.and_then(|o| u8_at(d, tail + o)).filter(|&h| (120..=230).contains(&h)),
        weight_kg: lay.weight_in_tail.and_then(|o| u8_at(d, tail + o)).filter(|&w| (35..=150).contains(&w)),
        preferred_foot,
        best_position: POSITION_NAMES[best],
        positions: Named { names: &POSITION_NAMES, values: pos },
        attributes,
        attributes_raw,
        personality,
    }
}

/// Fills nation UID/name and national-team names from the nation data.
pub fn attach_nations(players: &mut [Player], nations: &Nations) {
    for p in players {
        if let Some(i) = p.nation_index {
            p.nation_uid = nations.uid(i as u32);
            if let Some((name, src)) = nations.name(i as u32) {
                p.nation_name = Some(name.to_owned());
                p.nation_name_source = Some(src);
            }
        }
        p.international_team_name = p.international_team_id.and_then(|t| nations.team_name(t as u32)).map(str::to_owned);
    }
}

pub fn extract_players(d: &[u8], lay: &PlayerLayout, names: Option<&NameTables>, clubs: Option<&ClubTable>, limits: &NameLimits) -> Vec<Player> {
    let blocks = find_player_blocks(d, lay);
    blocks
        .iter()
        .enumerate()
        .map(|(i, &r)| read_player(d, r, blocks.get(i + 1).copied(), lay, names, clubs, limits))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::layout::{FM24, FM26};
    use crate::names::{build_table, locate, strs};

    /// Writes a synthetic player block for `lay` and returns (bytes, r).
    pub(crate) fn synth_player(lay: &PlayerLayout, uid: u32, ca: u16, pa: i16) -> (Vec<u8>, usize) {
        let mut v = vec![0x77u8; 4]; // row index
        v.extend(uid.to_le_bytes());
        v.extend(uid.to_le_bytes());
        v.push(2);
        for rep in [8761u16, 9406, 9829] {
            v.extend(rep.to_le_bytes());
        }
        let r = v.len();
        v.extend(ca.to_le_bytes());
        v.extend(pa.to_le_bytes());
        let mut header = vec![0u8; lay.header_len];
        header[lay.club.unwrap() - 4..lay.club.unwrap() - 2].copy_from_slice(&540u16.to_le_bytes());
        header[lay.intl_team.unwrap() - 4..lay.intl_team.unwrap() - 2].copy_from_slice(&0xFFFFu16.to_le_bytes());
        v.extend(header);
        let mut pos = [1u8; 15];
        pos[12] = 20;
        v.extend(pos);
        let mut attrs = [50u8; 54];
        attrs[2] = 91; // finishing
        attrs[24] = 100; // left foot
        attrs[25] = 55; // right foot
        v.extend(attrs);
        v.extend([0u8; 5]);
        v.extend([0xb6, 0x00, 0xe6, 0x07]); // 2022-07-01
        v.extend([0x01, 0x00, 0x6c, 0x07]); // null date
        let mut tail = vec![0u8; 24];
        if let Some(w) = lay.weight_in_tail {
            tail[w] = 94;
        }
        tail[lay.height_in_tail.unwrap()] = 195;
        v.extend(tail);
        (v, r)
    }

    fn synth_name_block(lay: &PlayerLayout, first: u32, last: u32, full: &str) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(first.to_le_bytes());
        v.push(0);
        v.extend(last.to_le_bytes());
        v.push(0);
        v.extend(u32::MAX.to_le_bytes());
        v.push(0);
        v.extend((full.len() as u32).to_le_bytes());
        v.extend(full.as_bytes());
        v.extend([203, 0, 0xd0, 0x07]); // 2000-07-21
        let mut rest = vec![0u8; 40];
        rest[lay.nation_after_name.unwrap() - 4..lay.nation_after_name.unwrap() - 2].copy_from_slice(&160u16.to_le_bytes());
        let p = lay.personality_after_name.unwrap() - 4;
        rest[p..p + 8].copy_from_slice(&[16, 20, 15, 18, 17, 10, 12, 11]);
        v.extend(rest);
        v
    }

    fn roundtrip(lay: &PlayerLayout) {
        let firsts: Vec<String> = (0..1000).map(|i| if i == 7 { "Erling".into() } else { format!("F{i}") }).collect();
        let lasts: Vec<String> = (0..1000).map(|i| if i == 9 { "Haaland".into() } else { format!("L{i}") }).collect();
        let mut d = Vec::new();
        d.extend(build_table(&strs(&firsts)));
        d.extend(build_table(&strs(&lasts)));
        d.extend(build_table(&["Common"]));
        d.extend([0x11u8; 300]);
        let (pb, r) = synth_player(lay, 29179063, 194, 195);
        let base = d.len();
        d.extend(pb);
        d.extend([0x33u8; 123]);
        d.extend(synth_name_block(lay, 7, 9, "Erling Braut Haaland"));
        d.extend([0x44u8; 200]);

        let names = locate(&d).expect("tables");
        let players = extract_players(&d, lay, Some(&names), None, &NameLimits::default());
        assert_eq!(players.len(), 1, "{}", lay.name);
        let p = &players[0];
        assert_eq!(p.db_offset, base + r);
        assert_eq!(p.uid, 29179063);
        assert_eq!((p.current_ability, p.potential_ability), (194, 195));
        assert_eq!(p.name.as_deref(), Some("Erling Haaland"));
        assert_eq!(p.full_name.as_deref(), Some("Erling Braut Haaland"));
        assert_eq!(p.date_of_birth.as_deref(), Some("2000-07-21"));
        assert_eq!(p.nation_index, Some(160));
        assert_eq!(p.club_id, Some(540));
        assert_eq!(p.international_team_id, None);
        assert_eq!(p.joined_club.as_deref(), Some("2022-07-01"));
        assert_eq!(p.second_date, None);
        assert_eq!(p.height_cm, Some(195));
        assert_eq!(p.weight_kg, lay.weight_in_tail.map(|_| 94));
        assert_eq!(p.best_position, "ST");
        assert_eq!(p.preferred_foot, "left");
        assert_eq!(p.attributes.get("finishing"), Some(18));
        assert_eq!(p.reputation.world, 9829);
        assert_eq!(p.personality.as_ref().unwrap().get("ambition"), Some(20));
    }

    #[test]
    fn fm24_synthetic_roundtrip() {
        roundtrip(&FM24);
    }

    #[test]
    fn fm26_synthetic_roundtrip() {
        roundtrip(&FM26);
    }

    #[test]
    fn layouts_do_not_cross_match() {
        // An FM26 block must not validate under the FM24 layout and vice versa
        // (positions/attributes shift by one byte).
        let (b26, _) = synth_player(&FM26, 5, 100, 120);
        assert!(find_player_blocks(&b26, &FM24).is_empty());
        let (b24, _) = synth_player(&FM24, 5, 100, 120);
        assert!(find_player_blocks(&b24, &FM26).is_empty());
    }

    #[test]
    fn display_scale() {
        assert_eq!(display_attr(91), 18);
        assert_eq!(display_attr(100), 20);
        assert_eq!(display_attr(1), 1);
        assert_eq!(display_attr(55), 11);
    }
}
