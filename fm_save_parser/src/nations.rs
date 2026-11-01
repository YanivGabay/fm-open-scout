//! Nations: the nation table in `game_db.dat` and nation names.
//!
//! The nation table is a run of objects starting `u32 index, u32 UID, u32 UID`
//! with indexes 0..N (N = 250 in FM24 and FM26). Players and clubs refer to
//! nations by index. Nation **names are not stored in `game_db.dat`**: FM reads
//! them from its own database files. A save contains them only inside national-
//! team records in match/scout files (`*.apm`, `*.scm`, ...):
//!
//! ```text
//! 02, u32 team id, 02, u32 UID, 3 × 00, u32 nation index, u32 UID, ...
//! str name, str short name, 3 bytes, u32 UID
//! ```
//!
//! A name is accepted only when the record's (index, UID) pair matches the nation
//! table. Names for other nations can be supplied by the user as a `uid,name` file
//! keyed by FM's stable nation UID.

use crate::bytes::u32_at;
use crate::container::FmContainer;
use serde::Serialize;
use std::collections::HashMap;

/// England is nation index 139 with UID 765 in FM24 and FM26 (UIDs are FM
/// database constants); the table is located from this anchor.
const ANCHOR: (u32, u32) = (139, 765);
const MAX_OBJECT: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct TeamRecord {
    pub team_id: u32,
    pub uid: u32,
    pub nation_index: u32,
    pub name: String,
    pub short_name: String,
    /// Container entry the record was read from.
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NameSource {
    /// National-team record inside the save, cross-checked with the nation table.
    Save,
    /// User-supplied `uid,name` file.
    UserFile,
}

#[derive(Debug, Default)]
pub struct Nations {
    /// UID per nation index.
    pub uids: Vec<u32>,
    names: HashMap<u32, (String, NameSource)>,
    /// National-team names by team id (exact, from the same records).
    team_names: HashMap<u32, String>,
}

#[derive(Debug, Serialize)]
pub struct NationStats {
    pub nations: usize,
    pub named_from_save: usize,
    pub named_from_file: usize,
    pub national_teams_named: usize,
}

fn triple_at(d: &[u8], k: usize, index: u32) -> bool {
    u32_at(d, k) == Some(index) && matches!((u32_at(d, k + 4), u32_at(d, k + 8)), (Some(u), Some(v)) if u != 0 && u == v)
}

/// Locates the nation table: the anchor triple, walked back to index 0 and forward
/// to the last index, with consecutive objects at most 64 KB apart.
pub fn locate_table(d: &[u8]) -> Option<Vec<u32>> {
    let mut pat = [0u8; 12];
    pat[..4].copy_from_slice(&ANCHOR.0.to_le_bytes());
    pat[4..8].copy_from_slice(&ANCHOR.1.to_le_bytes());
    pat[8..].copy_from_slice(&ANCHOR.1.to_le_bytes());
    let mut from = 0;
    while let Some(rel) = d.get(from..).and_then(|w| w.windows(12).position(|w| w == pat)) {
        let hit = from + rel;
        from = hit + 1;
        // Walk back to index 0.
        let mut pos = vec![hit];
        let mut idx = ANCHOR.0;
        while idx > 0 {
            let cur = pos[pos.len() - 1];
            let lo = cur.saturating_sub(MAX_OBJECT);
            match (lo..cur.saturating_sub(11)).rev().find(|&k| triple_at(d, k, idx - 1)) {
                Some(k) => pos.push(k),
                None => break,
            }
            idx -= 1;
        }
        if idx != 0 {
            continue;
        }
        pos.reverse();
        // Walk forward from the anchor.
        let mut idx = ANCHOR.0;
        loop {
            let cur = pos[pos.len() - 1];
            match (cur + 12..(cur + MAX_OBJECT).min(d.len().saturating_sub(12))).find(|&k| triple_at(d, k, idx + 1)) {
                Some(k) => {
                    pos.push(k);
                    idx += 1;
                }
                None => break,
            }
        }
        if pos.len() >= 200 {
            return Some(pos.iter().map(|&k| u32_at(d, k + 4).unwrap()).collect());
        }
    }
    None
}

fn str_at(b: &[u8], q: usize) -> Option<(String, usize)> {
    let l = u32_at(b, q)? as usize;
    if !(1..=80).contains(&l) {
        return None;
    }
    let s = std::str::from_utf8(b.get(q + 4..q + 4 + l)?).ok()?;
    (!s.chars().any(char::is_control)).then(|| (s.to_owned(), q + 4 + l))
}

/// National-team records in one decompressed file.
pub fn team_records(b: &[u8], source: &str) -> Vec<TeamRecord> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 40 < b.len() {
        if b[p] != 2 || b[p + 5] != 2 || b[p + 10..p + 13] != [0, 0, 0] {
            p += 1;
            continue;
        }
        let (tid, uid, nidx) = (u32_at(b, p + 1).unwrap(), u32_at(b, p + 6).unwrap(), u32_at(b, p + 13).unwrap());
        if uid == 0 || u32_at(b, p + 17) != Some(uid) || nidx > 400 || tid > 500_000 {
            p += 1;
            continue;
        }
        let found = (p + 21..(p + 400).min(b.len())).find_map(|q| {
            let (name, r) = str_at(b, q)?;
            let (short, e) = str_at(b, r)?;
            (u32_at(b, e + 3)? == uid).then_some((name, short))
        });
        if let Some((name, short_name)) = found {
            out.push(TeamRecord { team_id: tid, uid, nation_index: nidx, name, short_name, source: source.to_owned() });
        }
        p += 1;
    }
    out
}

impl Nations {
    /// Builds the nation list from `game_db.dat` and harvests national-team names
    /// from every other file of the save.
    pub fn load(container: &FmContainer, game_db: &[u8]) -> Nations {
        let uids = locate_table(game_db).unwrap_or_default();
        let mut n = Nations { uids, ..Default::default() };
        for e in &container.entries {
            if e.extension != ".apm" && e.extension != ".scm" && e.extension != ".tsm" && e.extension != ".dat" {
                continue;
            }
            if e.file_name() == "game_db.dat" {
                continue;
            }
            let Ok(b) = container.read_entry(e) else { continue };
            for r in team_records(&b, &e.path) {
                n.add_record(&r);
            }
        }
        n
    }

    pub fn add_record(&mut self, r: &TeamRecord) {
        if (r.nation_index as usize) >= self.uids.len() {
            return;
        }
        // Every self-consistent record names its team (senior and youth national
        // teams alike); only a senior team, whose UID is the nation's, names the nation.
        self.team_names.entry(r.team_id).or_insert_with(|| r.name.clone());
        if self.uids[r.nation_index as usize] == r.uid {
            self.names.entry(r.nation_index).or_insert_with(|| (r.name.clone(), NameSource::Save));
        }
    }

    /// Adds names from a `uid,name` text file (lines starting with `#` ignored).
    /// Names already read from the save take precedence.
    pub fn add_user_names(&mut self, text: &str) -> usize {
        let by_uid: HashMap<u32, u32> = self.uids.iter().enumerate().map(|(i, &u)| (u, i as u32)).collect();
        let mut added = 0;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let Some((uid, name)) = line.split_once(',') else { continue };
            let (Ok(uid), name) = (uid.trim().parse::<u32>(), name.trim()) else { continue };
            if let Some(&idx) = by_uid.get(&uid) {
                if !self.names.contains_key(&idx) && !name.is_empty() {
                    self.names.insert(idx, (name.to_owned(), NameSource::UserFile));
                    added += 1;
                }
            }
        }
        added
    }

    pub fn uid(&self, index: u32) -> Option<u32> {
        self.uids.get(index as usize).copied()
    }

    pub fn name(&self, index: u32) -> Option<(&str, NameSource)> {
        self.names.get(&index).map(|(n, s)| (n.as_str(), *s))
    }

    pub fn team_name(&self, team_id: u32) -> Option<&str> {
        self.team_names.get(&team_id).map(String::as_str)
    }

    pub fn stats(&self) -> NationStats {
        let count = |s: NameSource| self.names.values().filter(|(_, src)| *src == s).count();
        NationStats {
            nations: self.uids.len(),
            named_from_save: count(NameSource::Save),
            named_from_file: count(NameSource::UserFile),
            national_teams_named: self.team_names.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(tid: u32, uid: u32, nidx: u32, name: &str) -> Vec<u8> {
        let mut v = vec![0x55u8; 7];
        v.push(2);
        v.extend(tid.to_le_bytes());
        v.push(2);
        v.extend(uid.to_le_bytes());
        v.extend([0, 0, 0]);
        v.extend(nidx.to_le_bytes());
        v.extend(uid.to_le_bytes());
        v.extend([0u8; 30]);
        for s in [name, name] {
            v.extend((s.len() as u32).to_le_bytes());
            v.extend(s.as_bytes());
        }
        v.extend([0x30, 0x1b, 0x00]);
        v.extend(uid.to_le_bytes());
        v.extend([0u8; 40]);
        v
    }

    #[test]
    fn parses_national_team_record() {
        let b = record(621, 765, 139, "England");
        let r = team_records(&b, "x.apm");
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].team_id, r[0].uid, r[0].nation_index, r[0].name.as_str()), (621, 765, 139, "England"));
    }

    #[test]
    fn names_require_table_agreement_and_user_file_fills_gaps() {
        let mut n = Nations { uids: (0..251).map(|i| if i == 139 { 765 } else { 10_000 + i }).collect(), ..Default::default() };
        n.add_record(&team_records(&record(621, 765, 139, "England"), "a")[0]);
        // Mismatching UID for index 143: rejected.
        n.add_record(&team_records(&record(625, 999, 143, "Nope"), "b")[0]);
        assert_eq!(n.name(139), Some(("England", NameSource::Save)));
        assert_eq!(n.name(143), None);
        assert_eq!(n.team_name(621), Some("England"));
        // ...but the team name of a self-consistent record is kept (youth teams).
        assert_eq!(n.team_name(625), Some("Nope"));
        let added = n.add_user_names("# uid,name\n10143, France\n765,Not England\n42,Unknown uid\n");
        assert_eq!(added, 1);
        assert_eq!(n.name(143), Some(("France", NameSource::UserFile)));
        assert_eq!(n.name(139).unwrap().0, "England");
    }

    #[test]
    fn locates_table_from_anchor() {
        let mut d = vec![0xAAu8; 16];
        for i in 0..251u32 {
            let uid = if i == 139 { 765 } else { 5000 + i };
            d.extend(i.to_le_bytes());
            d.extend(uid.to_le_bytes());
            d.extend(uid.to_le_bytes());
            d.extend([0x11u8; 37]);
        }
        let t = locate_table(&d).unwrap();
        assert_eq!(t.len(), 251);
        assert_eq!((t[0], t[139], t[250]), (5000, 765, 5250));
    }
}
