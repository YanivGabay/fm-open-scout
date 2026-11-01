//! Person name tables in `game_db.dat`.
//!
//! Three tables are stored back to back: first names, surnames, common names
//! (FM24: 247 847 / 543 952 / 85 879 entries, FM26: 292 513 / 597 008 / 92 864).
//! Each is `u32 count` followed by `count × (u32 id, u32 byte_len, UTF-8 bytes)`
//! with `id == index`. Strings are not de-duplicated. Person records refer to
//! entries by id.

use crate::bytes::u32_at;
use serde::Serialize;

#[derive(Debug, Default)]
pub struct NameTable {
    /// Offset of the table's `count` field.
    pub offset: usize,
    pub names: Vec<String>,
}

impl NameTable {
    pub fn get(&self, id: u32) -> Option<&str> {
        self.names.get(id as usize).map(String::as_str)
    }
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct NameTables {
    pub first: NameTable,
    pub last: NameTable,
    pub common: NameTable,
}

#[derive(Debug, Serialize)]
pub struct NameTableStats {
    pub first_offset: usize,
    pub first_count: usize,
    pub last_count: usize,
    pub common_count: usize,
}

impl NameTables {
    pub fn stats(&self) -> NameTableStats {
        NameTableStats {
            first_offset: self.first.offset,
            first_count: self.first.len(),
            last_count: self.last.len(),
            common_count: self.common.len(),
        }
    }
}

const MAX_NAME_LEN: u32 = 200;

/// Parses a full table at `off` if every entry has the expected sequential id.
fn parse_table(d: &[u8], off: usize) -> Option<(NameTable, usize)> {
    let count = u32_at(d, off)? as usize;
    // Cheapest possible rejection before allocating: each entry is >= 8 bytes.
    if count == 0 || count > (d.len() - off) / 8 {
        return None;
    }
    let mut names = Vec::with_capacity(count);
    let mut p = off + 4;
    for i in 0..count {
        if u32_at(d, p)? as usize != i {
            return None;
        }
        let len = u32_at(d, p + 4)?;
        if len > MAX_NAME_LEN {
            return None;
        }
        let s = d.get(p + 8..p + 8 + len as usize)?;
        names.push(String::from_utf8_lossy(s).into_owned());
        p += 8 + len as usize;
    }
    Some((NameTable { offset: off, names }, p))
}

/// Finds the three consecutive tables. Version independent: the search keys on
/// the first two entries `(id 0, name)(id 1, ...)` of a table with >= 1000 entries.
pub fn locate(d: &[u8]) -> Option<NameTables> {
    let mut p = 4;
    while p + 16 < d.len() {
        // id 0
        if d[p..p + 4] != [0, 0, 0, 0] {
            p += 1;
            continue;
        }
        let ok = (|| {
            let len = u32_at(d, p + 4)?;
            if !(1..=60).contains(&len) {
                return None;
            }
            if u32_at(d, p + 8 + len as usize)? != 1 {
                return None;
            }
            let count = u32_at(d, p - 4)?;
            if count < 1000 {
                return None;
            }
            let (first, end1) = parse_table(d, p - 4)?;
            let (last, end2) = parse_table(d, end1)?;
            let (common, _) = parse_table(d, end2)?;
            Some(NameTables { first, last, common })
        })();
        if ok.is_some() {
            return ok;
        }
        p += 1;
    }
    None
}

#[cfg(test)]
pub(crate) fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[cfg(test)]
pub(crate) fn build_table(names: &[&str]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for (i, n) in names.iter().enumerate() {
        v.extend_from_slice(&(i as u32).to_le_bytes());
        v.extend_from_slice(&(n.len() as u32).to_le_bytes());
        v.extend_from_slice(n.as_bytes());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_three_tables() {
        let first: Vec<String> = (0..1200).map(|i| format!("First{i}")).collect();
        let last: Vec<String> = (0..1500).map(|i| format!("Last{i}")).collect();
        let common: Vec<String> = (0..10).map(|i| format!("Common{i}")).collect();
        let mut d = vec![0xAAu8; 37];
        d.extend(build_table(&strs(&first)));
        d.extend(build_table(&strs(&last)));
        d.extend(build_table(&strs(&common)));
        d.extend([0xBB; 20]);
        let t = locate(&d).unwrap();
        assert_eq!(t.first.offset, 37);
        assert_eq!(t.first.get(1199), Some("First1199"));
        assert_eq!(t.last.get(0), Some("Last0"));
        assert_eq!(t.common.len(), 10);
    }

    #[test]
    fn rejects_non_sequential_ids() {
        let mut d = vec![0u8; 4];
        let mut t = build_table(&(0..1000).map(|_| "x").collect::<Vec<_>>());
        // corrupt id of entry 500
        let off = 4 + 500 * 9;
        t[off] = 0xFF;
        d.extend(t);
        assert!(locate(&d).is_none());
    }
}
