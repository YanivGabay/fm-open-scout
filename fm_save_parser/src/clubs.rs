//! Club table and the team-id → club mapping.
//!
//! The u16 "club id" in a player record is a **team id**: an index into FM's team
//! list. Club objects in `game_db.dat` do not sit at that index; instead each club
//! object ends with an explicit list of the team ids it owns, first team first
//! (e.g. Man City `[540, 35767, 35768]`: first team, then two youth teams).
//! Parsing that list gives the mapping exactly, without interpolation.
//!
//! Club object (verified FM24 + FM26, see FORMAT.md §4.4):
//!
//! ```text
//! u32 E (sequential index), u32 UID, u32 UID, u8 kind (0 = club, 0x0C = nameless stub),
//! 4 × i32 nation fields (-1 or a nation index), ... str long name, str short name,
//! ... tail:
//!   u8 m, m × dated entry (marker, date, date, ...; FM24 24 B / FM26 25 B),
//!   [u8 0], u8 n, n × reference (01, type, UID, UID, u16),
//!   u32, u32, u8 flag (0|1), u8 count, count × u32 team id
//! ```
//!
//! "Basic" clubs (most of the database) have a fixed layout between the names and
//! the tail, so the tail is read at a fixed offset. "Full" clubs (playable leagues)
//! have a long variable section; there the tail is found by searching for the only
//! position where the whole tail grammar validates. When several positions
//! validate, the candidate is kept only if it is the single one consistent with the
//! strictly increasing first-team ids of the neighbouring clubs.

use crate::bytes::{i32_at, u32_at, FmDate};
use crate::layout::{ClubEntryShape, CLUB_ENTRY_FM24, CLUB_ENTRY_FM26};
use serde::Serialize;
use std::collections::HashMap;

const MAX_TEAM_ID: u32 = 500_000;
const MAX_NATION: i32 = 400;
const CHAIN_SEARCH: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Resolution {
    /// The only position in the object where the tail grammar validates.
    Structural,
    /// Several positions validate; exactly one fits the neighbours' id order.
    OrderChecked,
    /// The neighbours fix this club's first-team id exactly (their id gap equals
    /// the number of clubs between them) and a structural candidate has that id.
    Pinned,
    /// No team list found (the club's teams stay unmapped).
    Unresolved,
}

#[derive(Debug, Clone, Serialize)]
pub struct Club {
    /// Sequential index stored in the object (not the team id).
    pub index: u32,
    pub uid: u32,
    pub name: Option<String>,
    pub short_name: Option<String>,
    /// First non-negative of the four nation fields.
    pub nation_index: Option<u32>,
    /// Team ids owned by the club, first team first (empty = club has no team).
    pub teams: Vec<u32>,
    pub resolution: Resolution,
    #[serde(skip)]
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClubStats {
    pub clubs: usize,
    pub structural: usize,
    pub order_checked: usize,
    pub pinned: usize,
    pub unresolved: usize,
    pub teams: usize,
}

pub struct ClubTable {
    pub clubs: Vec<Club>,
    pub entry_shape: ClubEntryShape,
    team_owner: HashMap<u32, (usize, u8)>,
}

impl ClubTable {
    /// Club owning `team_id`, and the team's position in its list (0 = first team).
    pub fn team(&self, team_id: u32) -> Option<(&Club, u8)> {
        self.team_owner.get(&team_id).map(|&(i, slot)| (&self.clubs[i], slot))
    }

    pub fn stats(&self) -> ClubStats {
        let count = |r: Resolution| self.clubs.iter().filter(|c| c.resolution == r).count();
        ClubStats {
            clubs: self.clubs.len(),
            structural: count(Resolution::Structural),
            order_checked: count(Resolution::OrderChecked),
            pinned: count(Resolution::Pinned),
            unresolved: count(Resolution::Unresolved),
            teams: self.team_owner.len(),
        }
    }
}

fn valid_text(b: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(b).ok()?;
    (!s.chars().any(char::is_control)).then(|| s.to_owned())
}

/// Long and short name; returns them and the offset right after the short name.
fn names_at(d: &[u8], s: usize) -> Option<(String, String, usize)> {
    for q in s + 29..s + 60 {
        let l = u32_at(d, q)? as usize;
        if !(1..=100).contains(&l) {
            continue;
        }
        let Some(long) = d.get(q + 4..q + 4 + l).and_then(valid_text) else { continue };
        let l2 = u32_at(d, q + 4 + l)? as usize;
        if l2 > 100 {
            continue;
        }
        let Some(short) = d.get(q + 8 + l..q + 8 + l + l2).and_then(valid_text) else { continue };
        return Some((long, short, q + 8 + l + l2));
    }
    None
}

/// 1 = club header with names, 2 = nameless 0x0C stub, 0 = not a club object.
fn header_kind(d: &[u8], s: usize) -> u8 {
    let (Some(uid), Some(uid2)) = (u32_at(d, s + 4), u32_at(d, s + 8)) else { return 0 };
    if uid == 0 || uid != uid2 {
        return 0;
    }
    match d.get(s + 12) {
        Some(0) => {
            for j in 0..4 {
                match i32_at(d, s + 13 + 4 * j) {
                    Some(-1) => {}
                    Some(v) if (0..=MAX_NATION).contains(&v) => {}
                    _ => return 0,
                }
            }
            if names_at(d, s).is_some() {
                1
            } else {
                0
            }
        }
        Some(0x0c) => 2,
        _ => 0,
    }
}

fn find_index(d: &[u8], index: u32, from: usize, to: usize) -> Option<usize> {
    let pat = index.to_le_bytes();
    let to = to.min(d.len().saturating_sub(40));
    let mut k = from;
    while k < to {
        let rel = d[k..to].windows(4).position(|w| w == pat)?;
        let at = k + rel;
        if header_kind(d, at) != 0 {
            return Some(at);
        }
        k = at + 1;
    }
    None
}

/// Offsets of every club object, in index order. The chain starts at the first
/// valid index-0 club header that is followed by a valid index-1 header.
pub fn find_chain(d: &[u8]) -> Vec<usize> {
    let mut start = None;
    let mut k = 0;
    while let Some(rel) = d.get(k..).and_then(|w| w.windows(4).position(|w| w == [0, 0, 0, 0])) {
        let s = k + rel;
        if header_kind(d, s) == 1 && find_index(d, 1, s + 29, s + 100_000).is_some() {
            start = Some(s);
            break;
        }
        k = s + 1;
        if k + 40 > d.len() {
            break;
        }
    }
    let Some(start) = start else { return Vec::new() };
    let mut chain = vec![start];
    let mut e = 1u32;
    while let Some(next) = find_index(d, e, chain[chain.len() - 1] + 29, chain[chain.len() - 1] + CHAIN_SEARCH) {
        chain.push(next);
        e += 1;
    }
    chain
}

fn date_ok(d: &[u8], o: usize) -> bool {
    FmDate::read(d, o).is_some_and(|dt| dt.is_null() || ((1..=366).contains(&dt.day_of_year) && (1850..=2200).contains(&dt.year)))
}

fn team_list_after_refs(d: &[u8], q: usize) -> Option<Vec<u32>> {
    let n = *d.get(q)? as usize;
    if n > 60 {
        return None;
    }
    for j in 0..n {
        let r = q + 1 + 12 * j;
        if *d.get(r)? != 1 || u32_at(d, r + 2)? != u32_at(d, r + 6)? {
            return None;
        }
    }
    let l = q + 1 + 12 * n + 8;
    if *d.get(l)? > 1 {
        return None;
    }
    let count = *d.get(l + 1)? as usize;
    if count > 40 {
        return None;
    }
    (0..count)
        .map(|j| u32_at(d, l + 2 + 4 * j).filter(|&v| v <= MAX_TEAM_ID))
        .collect()
}

/// Parses the tail grammar starting at the entry count `k`.
fn parse_tail(d: &[u8], k: usize, shape: ClubEntryShape) -> Option<Vec<u32>> {
    let m = *d.get(k)? as usize;
    if m > 60 {
        return None;
    }
    let mut q = k + 1;
    for _ in 0..m {
        if *d.get(q)? != shape.marker || !date_ok(d, q + 2) || !date_ok(d, q + 6) {
            return None;
        }
        q += shape.len;
    }
    team_list_after_refs(d, q).or_else(|| (*d.get(q)? == 0).then(|| team_list_after_refs(d, q + 1)).flatten())
}

fn is_basic(d: &[u8], a: usize) -> bool {
    d.get(a + 1..a + 3) == Some(&[0, 0]) && i32_at(d, a + 3) == Some(-1) && d.get(a + 7..a + 10) == Some(&[0, 0, 0])
}

/// All tail candidates of a "full" club. The tail must be preceded by a
/// plausible float and, without dated entries, carry at least one reference.
fn full_candidates(d: &[u8], a: usize, end: usize, shape: ClubEntryShape) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    for k in a + 4..end.saturating_sub(30) {
        let f = f32::from_le_bytes(d[k - 4..k].try_into().unwrap());
        if !(f > 0.001 && f < 1000.0) {
            continue;
        }
        if d[k] == 0 && d[k + 1] == 0 && d[k + 2] == 0 {
            continue;
        }
        if let Some(ids) = parse_tail(d, k, shape) {
            out.push(ids);
        }
    }
    out
}

fn build(d: &[u8], chain: &[usize], shape: ClubEntryShape) -> ClubTable {
    let mut clubs = Vec::with_capacity(chain.len());
    let mut multi: Vec<(usize, Vec<Vec<u32>>)> = Vec::new();
    let mut all_cands: HashMap<usize, Vec<Vec<u32>>> = HashMap::new();
    for (i, &s) in chain.iter().enumerate() {
        let end = chain.get(i + 1).copied().unwrap_or((s + 64 * 1024).min(d.len()));
        let index = u32_at(d, s).unwrap_or(0);
        let uid = u32_at(d, s + 4).unwrap_or(0);
        let nation_index = (0..4).filter_map(|j| i32_at(d, s + 13 + 4 * j)).find(|&v| v >= 0).map(|v| v as u32);
        let mut club = Club {
            index,
            uid,
            name: None,
            short_name: None,
            nation_index: if d.get(s + 12) == Some(&0) { nation_index } else { None },
            teams: Vec::new(),
            resolution: Resolution::Unresolved,
            offset: s,
        };
        if let Some((long, short, a)) = (d.get(s + 12) == Some(&0)).then(|| names_at(d, s)).flatten() {
            club.name = Some(long);
            club.short_name = Some(short);
            let cands = if is_basic(d, a) {
                parse_tail(d, a + 55, shape).into_iter().collect()
            } else {
                full_candidates(d, a, end, shape)
            };
            if cands.len() != 1 {
                all_cands.insert(i, cands.clone());
            }
            match cands.len() {
                0 => {}
                1 => {
                    club.teams = cands.into_iter().next().unwrap();
                    club.resolution = Resolution::Structural;
                }
                _ => multi.push((i, cands)),
            }
        }
        clubs.push(club);
    }
    // First-team ids increase strictly with the club index; use that to pick the
    // single order-consistent candidate where the grammar matched more than once.
    for (i, cands) in multi {
        let lo = clubs[..i].iter().rev().find(|c| !c.teams.is_empty()).map_or(-1i64, |c| c.teams[0] as i64);
        let hi = clubs[i + 1..].iter().find(|c| !c.teams.is_empty()).map_or(i64::MAX, |c| c.teams[0] as i64);
        let fit: Vec<_> = cands.into_iter().filter(|c| !c.is_empty() && (c[0] as i64) > lo && (c[0] as i64) < hi).collect();
        if fit.len() == 1 {
            clubs[i].teams = fit.into_iter().next().unwrap();
            clubs[i].resolution = Resolution::OrderChecked;
        }
    }
    // Pinning: between two resolved clubs with first-team ids a < b, if every club in
    // between is unresolved and there are exactly b - a - 1 of them, their first-team
    // ids must be a+1, a+2, ... Accept only candidates carrying exactly that id.
    let resolved: Vec<usize> = (0..clubs.len()).filter(|&i| !clubs[i].teams.is_empty()).collect();
    for w in resolved.windows(2) {
        let (i, j) = (w[0], w[1]);
        let between: Vec<usize> = (i + 1..j).filter(|&k| clubs[k].resolution == Resolution::Unresolved).collect();
        if between.is_empty() || between.len() != j - i - 1 {
            continue;
        }
        let (a, b) = (clubs[i].teams[0], clubs[j].teams[0]);
        if (b - a - 1) as usize != between.len() {
            continue;
        }
        for (n, &k) in between.iter().enumerate() {
            let want = a + 1 + n as u32;
            if let Some(c) = all_cands.get(&k).and_then(|cs| cs.iter().find(|c| c.first() == Some(&want))) {
                clubs[k].teams = c.clone();
                clubs[k].resolution = Resolution::Pinned;
            }
        }
    }
    let mut team_owner = HashMap::new();
    let mut prev = -1i64;
    for (i, c) in clubs.iter_mut().enumerate() {
        if let Some(&first) = c.teams.first() {
            // Safety net: a structural match that breaks the order is dropped.
            if (first as i64) <= prev || c.teams.iter().any(|t| team_owner.contains_key(t)) {
                c.teams.clear();
                c.resolution = Resolution::Unresolved;
                continue;
            }
            prev = first as i64;
            for (slot, &t) in c.teams.iter().enumerate() {
                team_owner.insert(t, (i, slot as u8));
            }
        }
    }
    ClubTable { clubs, entry_shape: shape, team_owner }
}

/// Locates the club table. With `shape == None` (unregistered versions) both
/// known entry shapes are tried and the one resolving more clubs wins.
pub fn locate(d: &[u8], shape: Option<ClubEntryShape>) -> Option<ClubTable> {
    let chain = find_chain(d);
    if chain.is_empty() {
        return None;
    }
    let shapes = match shape {
        Some(s) => vec![s],
        None => vec![CLUB_ENTRY_FM24, CLUB_ENTRY_FM26],
    };
    shapes
        .into_iter()
        .map(|s| build(d, &chain, s))
        .max_by_key(|t| t.stats().teams)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_str(v: &mut Vec<u8>, s: &str) {
        v.extend((s.len() as u32).to_le_bytes());
        v.extend(s.as_bytes());
    }

    /// A "basic" club object as found in FM24 saves (layout of Albanian clubs).
    fn basic_club(index: u32, uid: u32, name: &str, teams: &[u32], shape: ClubEntryShape) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(index.to_le_bytes());
        v.extend(uid.to_le_bytes());
        v.extend(uid.to_le_bytes());
        v.push(0);
        for n in [126i32, -1, 126, 126] {
            v.extend(n.to_le_bytes());
        }
        v.extend([0u8; 10]);
        put_str(&mut v, name);
        put_str(&mut v, name);
        // fixed basic section: type, 00 00, FFFFFFFF, 3 × 00, u32, u32, 3 null dates,
        // u32 0, u8 0, 5 floats
        v.push(2);
        v.extend([0, 0]);
        v.extend((-1i32).to_le_bytes());
        v.extend([0, 0, 0]);
        v.extend([0u8; 8]);
        for _ in 0..3 {
            v.extend([0x01, 0x00, 0x6c, 0x07]);
        }
        v.extend([0u8; 5]);
        for f in [0.0f32, 0.0, 1.0, 1.0, 1.0] {
            v.extend(f.to_le_bytes());
        }
        // one dated entry
        v.push(1);
        let mut e = vec![0u8; shape.len];
        e[0] = shape.marker;
        e[1] = 2;
        e[2..6].copy_from_slice(&[0x01, 0x00, 0xe7, 0x07]);
        e[6..10].copy_from_slice(&[0xb6, 0x00, 0xe8, 0x07]);
        v.extend(e);
        // one reference, then X, Y, flag, team list
        v.push(1);
        v.extend([1, 2]);
        v.extend(1234u32.to_le_bytes());
        v.extend(1234u32.to_le_bytes());
        v.extend([0, 0]);
        v.extend([0u8; 8]);
        v.push(0);
        v.push(teams.len() as u8);
        for t in teams {
            v.extend(t.to_le_bytes());
        }
        v.extend([0u8; 12]);
        v
    }

    fn roundtrip(shape: ClubEntryShape) {
        let mut d = vec![0xEEu8; 50];
        d.extend(basic_club(0, 59, "Klubi i Futbollit Tirana", &[53], shape));
        d.extend(basic_club(1, 61, "Flamurtari", &[54, 40001, 40002], shape));
        d.extend(basic_club(2, 63, "Teuta", &[], shape));
        d.extend(basic_club(3, 64, "Vllaznia", &[56], shape));
        let t = locate(&d, Some(shape)).expect("club table");
        assert_eq!(t.clubs.len(), 4);
        assert_eq!(t.clubs[0].name.as_deref(), Some("Klubi i Futbollit Tirana"));
        assert_eq!(t.clubs[0].nation_index, Some(126));
        let (c, slot) = t.team(40002).unwrap();
        assert_eq!((c.uid, slot), (61, 2));
        assert_eq!(t.team(56).unwrap().0.short_name.as_deref(), Some("Vllaznia"));
        assert!(t.clubs[2].teams.is_empty());
        assert_eq!(t.clubs[2].resolution, Resolution::Structural);
        assert!(t.team(55).is_none());
    }

    #[test]
    fn basic_clubs_fm24_shape() {
        roundtrip(CLUB_ENTRY_FM24);
    }

    #[test]
    fn basic_clubs_fm26_shape() {
        roundtrip(CLUB_ENTRY_FM26);
    }

    #[test]
    fn unknown_shape_is_detected() {
        let mut d = vec![0xEEu8; 8];
        d.extend(basic_club(0, 59, "A", &[53], CLUB_ENTRY_FM26));
        d.extend(basic_club(1, 61, "B", &[54], CLUB_ENTRY_FM26));
        let t = locate(&d, None).unwrap();
        assert_eq!(t.entry_shape, CLUB_ENTRY_FM26);
        assert_eq!(t.stats().teams, 2);
    }
}
