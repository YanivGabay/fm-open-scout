//! Version detection.
//!
//! Every file inside the container starts with an 8-byte chunk header:
//! `03 01`, a reversed four-character extension (`"tad."` for `.dat`, `"tmc."` for
//! `.cmt`, ...) and a `u16` per-file format version. Two files identify the game:
//!
//! * `game_info.dat`: after the chunk header, a `u32`-length string holding the game
//!   build, e.g. `"24.3.0+0"` (FM24) or `"26.0.0+0"` (FM26).
//! * `game_db.dat`: a `"DB"` block with a `u16` database schema version
//!   (FM24: 3569, FM26: 3717) and a 4-digit tag (`"2430"`, `"2600"`).
//!
//! The major version selects the record layout (see `layout.rs`).

use crate::bytes::{u16_at, u32_at, Cursor, FmDate};
use crate::container::FmContainer;
use crate::error::{FmError, Result};
use serde::Serialize;

pub const CHUNK_MAGIC: [u8; 2] = [0x03, 0x01];
pub const CHUNK_HEADER_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChunkHeader {
    /// Extension as stored, un-reversed (e.g. `".dat"`).
    pub kind: String,
    pub version: u16,
}

pub fn chunk_header(d: &[u8]) -> Option<ChunkHeader> {
    if d.len() < CHUNK_HEADER_LEN || d[..2] != CHUNK_MAGIC {
        return None;
    }
    let kind: String = d[2..6].iter().rev().map(|&b| b as char).collect();
    Some(ChunkHeader { kind, version: u16_at(d, 6)? })
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveVersion {
    /// Build string from `game_info.dat`, e.g. `"24.3.0+0"`.
    pub game_version: Option<String>,
    /// FM release number: 24 = FM24, 26 = FM26, ...
    pub major: u16,
    pub minor: Option<u16>,
    pub patch: Option<u16>,
    /// Chunk-header version of `game_info.dat`.
    pub game_info_format: Option<u16>,
    /// Chunk-header version of `game_db.dat`.
    pub game_db_format: Option<u16>,
    /// Database schema version from the `"DB"` block.
    pub db_version: Option<u16>,
    /// Four-digit database tag, e.g. `"2430"`.
    pub db_tag: Option<String>,
    /// In-game current date (ISO), from the `"DB"` block (tag offset + 16).
    pub current_date: Option<String>,
    /// Build that last wrote the save, from `save_game_summary.dat` (e.g. `"24.4.2"`);
    /// `game_version` is the build that created it.
    pub saved_with: Option<String>,
}

impl SaveVersion {
    /// Release whose record layout `game_db.dat` uses: the newer of the creating
    /// build and the last writer. FM27 re-saves FM24 careers in its own format
    /// while keeping the FM24 version string and DB tag.
    pub fn layout_major(&self) -> u16 {
        let saved = self.saved_with.as_deref().and_then(|s| parse_numbers(s).0);
        saved.map_or(self.major, |s| s.max(self.major))
    }
}

/// Parses `game_info.dat`: chunk header + `u32` length + version string.
pub fn parse_game_info(d: &[u8]) -> Option<(u16, String)> {
    let hdr = chunk_header(d)?;
    let mut cur = Cursor::new(d);
    cur.pos = CHUNK_HEADER_LEN;
    let len = cur.u32()? as usize;
    if len == 0 || len > 64 {
        return None;
    }
    let s = cur.bytes(len)?;
    if !s.iter().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    Some((hdr.version, String::from_utf8_lossy(s).into_owned()))
}

pub struct DbHeader {
    pub format: u16,
    pub db_version: u16,
    pub tag: Option<String>,
    /// In-game date stored 16 bytes after the tag string.
    pub current_date: Option<FmDate>,
    /// Offset of the `"DB"` marker; `None` when the save has none (see below).
    pub marker_offset: Option<usize>,
}

fn tag_at(d: &[u8], range: std::ops::Range<usize>) -> Option<usize> {
    range.into_iter().find(|&o| u32_at(d, o) == Some(4) && d.get(o + 4..o + 8).is_some_and(|s| s.iter().all(u8::is_ascii_digit)))
}

/// Parses the start of `game_db.dat`. The `"DB"` marker sits a few bytes after the
/// chunk header (FM24: +2, FM26: +6); the tag follows as a `u32`-length string of
/// four ASCII digits (FM26 inserts one extra `u16` before it). Some FM24 saves have
/// `u32 8` instead of `"DB" u16 0x0f`; there the schema version is the `u16` just
/// before the tag.
pub fn parse_db_header(d: &[u8]) -> Option<DbHeader> {
    let hdr = chunk_header(d)?;
    let window = d.get(CHUNK_HEADER_LEN..64.min(d.len()))?;
    let marker_offset = window.windows(2).position(|w| w == b"DB").map(|rel| CHUNK_HEADER_LEN + rel);
    let (db_version, tag_at) = match marker_offset {
        Some(m) => (u16_at(d, m + 4)?, tag_at(d, m + 6..m + 16)),
        None => {
            let t = tag_at(d, CHUNK_HEADER_LEN..48)?;
            (u16_at(d, t - 2)?, Some(t))
        }
    };
    let tag = tag_at.map(|o| String::from_utf8_lossy(&d[o + 4..o + 8]).into_owned());
    // After the tag: u16, u16 year, u32, [8 bytes, FM27+], u16 year, u16, date.
    // FM24: tag at 20, date at 36; FM26: tag at 26, date at 42; FM27: tag at 26, date at 50.
    let current_date = tag_at.and_then(|o| {
        let end = o + 8;
        let year = u16_at(d, end + 2)?;
        [8usize, 16].into_iter().find_map(|gap| {
            let y = u16_at(d, end + gap)?;
            let date = FmDate::read(d, end + gap + 4)?;
            (y.abs_diff(year) <= 3 && date.is_plausible() && !date.is_null()).then_some(date)
        })
    });
    Some(DbHeader { format: hdr.version, db_version, tag, current_date, marker_offset })
}

fn parse_numbers(v: &str) -> (Option<u16>, Option<u16>, Option<u16>) {
    let core = v.split(['+', ' ']).next().unwrap_or("");
    let mut it = core.split('.').map(|p| p.parse::<u16>().ok());
    (it.next().flatten(), it.next().flatten(), it.next().flatten())
}

/// First `u32`-length string in `save_game_summary.dat` that looks like a build
/// number (`NN.N.N`, optionally `+build`).
pub fn parse_saved_with(summary: &[u8]) -> Option<String> {
    (8..summary.len().saturating_sub(8)).find_map(|p| {
        let len = u32_at(summary, p)? as usize;
        if !(5..=24).contains(&len) {
            return None;
        }
        let s = std::str::from_utf8(summary.get(p + 4..p + 4 + len)?).ok()?;
        let core = s.split('+').next()?;
        let parts: Vec<&str> = core.split('.').collect();
        let ok = parts.len() == 3 && parts.iter().all(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit())) && parts[0].len() == 2;
        ok.then(|| s.to_owned())
    })
}

/// Detects the version from the decompressed `game_info.dat` and the first bytes of
/// `game_db.dat`. Either may be `None`; at least one must identify the major version.
pub fn detect_from_parts(game_info: Option<&[u8]>, game_db_head: Option<&[u8]>) -> Result<SaveVersion> {
    let info = game_info.and_then(parse_game_info);
    let db = game_db_head.and_then(parse_db_header);
    let (mut major, minor, patch) = match &info {
        Some((_, s)) => parse_numbers(s),
        None => (None, None, None),
    };
    if major.is_none() {
        // The DB tag's first two digits are the release number ("2430" -> 24).
        major = db.as_ref().and_then(|h| h.tag.as_ref()).and_then(|t| t[..2].parse().ok());
    }
    let major = major.ok_or_else(|| FmError::UnsupportedVersion("no version string or DB tag found".into()))?;
    Ok(SaveVersion {
        game_version: info.as_ref().map(|(_, s)| s.clone()),
        major,
        minor,
        patch,
        game_info_format: info.map(|(v, _)| v),
        game_db_format: db.as_ref().map(|h| h.format),
        db_version: db.as_ref().map(|h| h.db_version),
        current_date: db.as_ref().and_then(|h| h.current_date).and_then(|x| x.iso()),
        db_tag: db.and_then(|h| h.tag),
        saved_with: None,
    })
}

/// Detects the version of an opened save. Only the first frame bytes of
/// `game_db.dat` are needed, so this decompresses a small prefix of it.
pub fn detect(container: &FmContainer) -> Result<SaveVersion> {
    let info = container.find("game_info.dat").map(|e| container.read_entry(e)).transpose()?;
    let db_head = match container.find("game_db.dat") {
        Some(e) => {
            use std::io::Read;
            let mut head = vec![0u8; 64];
            let mut dec = zstd::stream::read::Decoder::new(container.compressed(e))?;
            let n = dec.read(&mut head)?;
            head.truncate(n);
            Some(head)
        }
        None => None,
    };
    let mut v = detect_from_parts(info.as_deref(), db_head.as_deref())?;
    if let Some(e) = container.find("save_game_summary.dat") {
        v.saved_with = container.read_entry(e).ok().as_deref().and_then(parse_saved_with);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FM24_INFO: &[u8] = b"\x03\x01tad.\x2a\x00\x08\x00\x00\x0024.3.0+0\x00";
    const FM24_DB: &[u8] = b"\x03\x01tad.\xf1\x0d\x00\x00DB\x0f\x00\xf1\x0d\x04\x00\x00\x002430K2";
    const FM26_INFO: &[u8] = b"\x03\x01tad.\x2e\x00\x08\x00\x00\x0026.0.0+0\x00";
    const FM26_DB: &[u8] = b"\x03\x01tad.\xa0\x0f\x8d\x0e\x07\x00\x00\x00DB\x0f\x00\x85\x0e\x06\x00\x04\x00\x00\x002600 5";

    #[test]
    fn chunk_header_kind_is_unreversed() {
        let h = chunk_header(FM24_INFO).unwrap();
        assert_eq!(h.kind, ".dat");
        assert_eq!(h.version, 0x2a);
    }

    #[test]
    fn detects_fm24() {
        let v = detect_from_parts(Some(FM24_INFO), Some(FM24_DB)).unwrap();
        assert_eq!(v.major, 24);
        assert_eq!((v.minor, v.patch), (Some(3), Some(0)));
        assert_eq!(v.db_version, Some(3569));
        assert_eq!(v.db_tag.as_deref(), Some("2430"));
    }

    #[test]
    fn current_date_and_saved_with() {
        let mut db = FM24_DB.to_vec();
        db.extend([0xe8, 0x07, 0x97, 0x94, 0x04, 0x00, 0xe7, 0x07, 0xf2, 0x11, 0xd9, 0x00, 0xe9, 0x07]);
        let v = detect_from_parts(Some(FM24_INFO), Some(&db)).unwrap();
        assert_eq!(v.current_date.as_deref(), Some("2025-08-05"));
        let mut summary = b"\x03\x01tad.\x16\x00junk".to_vec();
        summary.extend(6u32.to_le_bytes());
        summary.extend(b"24.4.2");
        summary.extend([0u8; 8]);
        assert_eq!(parse_saved_with(&summary).as_deref(), Some("24.4.2"));
    }

    #[test]
    fn fm27_header_date_and_layout_major() {
        // FM27 inserts 8 bytes before the second year; bytes from a 27.0.0 save.
        const FM27_DB: &[u8] = b"\x03\x01tad.\xa0\x0f\xaa\x0e\x09\x00\x00\x00DB\x0f\x00\xa9\x0e\x09\x00\x04\x00\x00\x002700\x1f\x37\xea\x07\x79\x46\x08\x00\x00\x7e\x4a\x4c\x24\x3a\x00\x00\xea\x07\x38\x16\x6d\x23\xe9\x07";
        let mut v = detect_from_parts(None, Some(FM27_DB)).unwrap();
        assert_eq!((v.major, v.current_date.as_deref()), (27, Some("2025-12-31")));
        assert_eq!(v.layout_major(), 27);
        // An FM24 career re-saved by FM27 uses the FM27 layout.
        let mut old = detect_from_parts(Some(FM24_INFO), Some(FM24_DB)).unwrap();
        assert_eq!(old.layout_major(), 24);
        old.saved_with = Some("27.0.0+2486695".into());
        assert_eq!(old.layout_major(), 27);
        v.saved_with = Some("26.1.0".into());
        assert_eq!(v.layout_major(), 27);
    }

    #[test]
    fn db_header_without_marker() {
        // FM24 save (Sporting CP, 24.4.2) with u32 8 in place of "DB" 0x0f.
        const DB: &[u8] = b"\x03\x01tad.\xf1\x0d\x00\x00\x08\x00\x00\x00\xf1\x0d\x04\x00\x00\x002430K2\xe8\x07\x97\x94\x04\x00\xe7\x07\x30\x0d\xce\x80\xeb\x07";
        let v = detect_from_parts(None, Some(DB)).unwrap();
        assert_eq!((v.major, v.db_version, v.db_tag.as_deref()), (24, Some(3569), Some("2430")));
        assert_eq!(v.current_date.as_deref(), Some("2027-07-25")); // day 206
    }

    #[test]
    fn detects_fm26() {
        let v = detect_from_parts(Some(FM26_INFO), Some(FM26_DB)).unwrap();
        assert_eq!(v.major, 26);
        assert_eq!(v.db_version, Some(3717));
        assert_eq!(v.db_tag.as_deref(), Some("2600"));
        assert_eq!(v.game_db_format, Some(4000));
    }

    #[test]
    fn falls_back_to_db_tag() {
        let v = detect_from_parts(None, Some(FM26_DB)).unwrap();
        assert_eq!(v.major, 26);
        assert!(v.game_version.is_none());
        assert!(detect_from_parts(None, None).is_err());
    }
}
