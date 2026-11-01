//! The `.fm` container: a small virtual filesystem of zstd-compressed files.
//!
//! Layout (verified on FM24 and FM26 saves, see FORMAT.md §1):
//!
//! ```text
//! 0x00  9 bytes  signature  02 01 'f' 'm' 'f' '.' 08 00 00
//! 0x09  u64      body length L: the directory block starts at 9 + L
//! 0x11  u64      17 = size of this header after the signature (data starts at 9 + 17)
//! 0x19  u8       3 (observed constant; compression id is the likeliest meaning)
//! 0x1a  ...      file data: one zstd frame per file, back to back
//! 9+L   9 bytes  the same signature again
//! 9+L+9 ...      one zstd frame holding the directory tree (to end of file)
//! ```
//!
//! Directory node: `str name, u32 file_count, file_count × entry, u32 subdir_count,
//! subdir_count × node`. Entry: `str name, str extension, u64 offset (relative to the
//! data start), u64 compressed size, u64 uncompressed size, i64 created, i64 modified`
//! (Unix seconds; -62135596800 = 0001-01-01 means "unset"). `str` = u32 length + UTF-8.

use crate::bytes::{u64_at, u8_at, Cursor};
use crate::error::{FmError, Result};
use memmap2::Mmap;
use serde::Serialize;
use std::fs::File;
use std::path::Path;

/// Full 9-byte signature of FM24/FM26 saves. Only the first 6 bytes (`MAGIC`) are
/// required; bytes 6..9 are kept as `container_version` (08 00 00 in FM24/FM26;
/// other values are accepted but unverified).
pub const SIGNATURE: [u8; 9] = [0x02, 0x01, b'f', b'm', b'f', b'.', 0x08, 0x00, 0x00];
pub const MAGIC: [u8; 6] = [0x02, 0x01, b'f', b'm', b'f', b'.'];
/// Compression id at offset 0x19 observed in all saves (zstd).
pub const COMPRESSION_ZSTD: u8 = 3;
pub const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// Timestamp FM writes for "never set" (0001-01-01T00:00:00Z in Unix seconds).
pub const UNSET_TIMESTAMP: i64 = -62_135_596_800;

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    /// Full path inside the save, e.g. `"<save title>/game_db.dat"`.
    pub path: String,
    pub name: String,
    pub extension: String,
    /// Absolute file offset of the entry's zstd frame.
    pub offset: u64,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub created: Option<i64>,
    pub modified: Option<i64>,
}

impl DirEntry {
    /// `name + extension`, e.g. `game_db.dat`.
    pub fn file_name(&self) -> String {
        format!("{}{}", self.name, self.extension)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerHeader {
    /// Bytes 6..9 of the signature (08 00 00 in FM24/FM26).
    pub container_version: [u8; 3],
    pub body_length: u64,
    pub header_length: u64,
    pub header_flag: u8,
    pub data_start: u64,
    pub directory_offset: u64,
}

/// Raw bytes of a save, owned (tests, in-memory data) or memory-mapped.
enum Backing {
    Mapped(Mmap),
    Owned(Vec<u8>),
}

impl std::ops::Deref for Backing {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Backing::Mapped(m) => m,
            Backing::Owned(v) => v,
        }
    }
}

pub struct FmContainer {
    data: Backing,
    pub header: ContainerHeader,
    /// Title of the root directory node (the save's display name).
    pub title: String,
    pub entries: Vec<DirEntry>,
}

impl FmContainer {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        // SAFETY: the map is read-only; the caller must not modify the save
        // while it is open (we never write to it).
        let mmap = unsafe { Mmap::map(&file)? };
        Self::parse(Backing::Mapped(mmap))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::parse(Backing::Owned(bytes))
    }

    /// Cheap check used by version detection and CLI: does this look like an `.fm` container?
    pub fn has_signature(bytes: &[u8]) -> bool {
        bytes.len() >= SIGNATURE.len() && bytes[..MAGIC.len()] == MAGIC
    }

    fn parse(data: Backing) -> Result<Self> {
        let d: &[u8] = &data;
        if !Self::has_signature(d) {
            let got: Vec<u8> = d.iter().take(9).copied().collect();
            return Err(FmError::InvalidFile(format!(
                "bad signature {:02x?}: not an 'fmf.' container. Only the FM24/FM26 \
                 container is verified; see FORMAT.md §9 for older versions",
                got
            )));
        }
        let invalid = |m: &str| FmError::InvalidFile(m.to_string());
        let body_length = u64_at(d, 9).ok_or_else(|| invalid("truncated header"))?;
        let header_length = u64_at(d, 17).ok_or_else(|| invalid("truncated header"))?;
        let header_flag = u8_at(d, 25).ok_or_else(|| invalid("truncated header"))?;
        if header_flag != COMPRESSION_ZSTD {
            return Err(FmError::InvalidFile(format!(
                "compression id {header_flag} at 0x19 (only 3 = zstd is known); see FORMAT.md §9"
            )));
        }
        let signature = &d[..SIGNATURE.len()];
        let container_version = [d[6], d[7], d[8]];
        let data_start = 9 + header_length;
        let directory_offset = 9 + body_length;
        let dir_sig_end = directory_offset as usize + SIGNATURE.len();
        if dir_sig_end > d.len() || &d[directory_offset as usize..dir_sig_end] != signature {
            return Err(invalid("directory signature not found at 9 + body length"));
        }
        let header = ContainerHeader { container_version, body_length, header_length, header_flag, data_start, directory_offset };

        let dir_bytes = zstd::stream::decode_all(&d[dir_sig_end..])
            .map_err(|e| FmError::DecompressionError(format!("directory: {e}")))?;
        let mut cur = Cursor::new(&dir_bytes);
        let mut entries = Vec::new();
        let title = parse_node(&mut cur, "", data_start, &mut entries, 0)
            .ok_or_else(|| invalid("malformed directory tree"))?;
        if cur.remaining() != 0 {
            return Err(FmError::InvalidFile(format!(
                "{} trailing bytes after directory tree",
                cur.remaining()
            )));
        }
        for e in &entries {
            let end = e.offset.checked_add(e.compressed_size);
            if end.map_or(true, |end| end > directory_offset) {
                return Err(FmError::InvalidFile(format!("entry {} points outside the data area", e.path)));
            }
        }
        Ok(Self { data, header, title, entries })
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn raw(&self) -> &[u8] {
        &self.data
    }

    /// First entry whose file name (`name + ext`) or full path equals `name`.
    pub fn find(&self, name: &str) -> Option<&DirEntry> {
        self.entries.iter().find(|e| e.path == name || e.file_name() == name)
    }

    /// The compressed bytes of an entry (one zstd frame).
    pub fn compressed(&self, entry: &DirEntry) -> &[u8] {
        &self.data[entry.offset as usize..(entry.offset + entry.compressed_size) as usize]
    }

    pub fn read_entry(&self, entry: &DirEntry) -> Result<Vec<u8>> {
        let out = zstd::bulk::decompress(self.compressed(entry), entry.uncompressed_size as usize)
            .map_err(|e| FmError::DecompressionError(format!("{}: {e}", entry.path)))?;
        if out.len() as u64 != entry.uncompressed_size {
            return Err(FmError::DecompressionError(format!(
                "{}: expected {} bytes, got {}",
                entry.path,
                entry.uncompressed_size,
                out.len()
            )));
        }
        Ok(out)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let entry = self.find(name).ok_or_else(|| FmError::MissingEntry(name.to_string()))?;
        self.read_entry(entry)
    }
}

fn opt_timestamp(t: i64) -> Option<i64> {
    (t != UNSET_TIMESTAMP).then_some(t)
}

fn parse_node(cur: &mut Cursor, parent: &str, data_start: u64, out: &mut Vec<DirEntry>, depth: u32) -> Option<String> {
    if depth > 32 {
        return None;
    }
    let name = cur.string()?;
    let dir_path = if parent.is_empty() { name.clone() } else { format!("{parent}/{name}") };
    let files = cur.u32()?;
    for _ in 0..files {
        let fname = cur.string()?;
        let ext = cur.string()?;
        let offset = cur.u64()?;
        let compressed_size = cur.u64()?;
        let uncompressed_size = cur.u64()?;
        let created = cur.i64()?;
        let modified = cur.i64()?;
        out.push(DirEntry {
            path: format!("{dir_path}/{fname}{ext}"),
            name: fname,
            extension: ext,
            offset: data_start.checked_add(offset)?,
            compressed_size,
            uncompressed_size,
            created: opt_timestamp(created),
            modified: opt_timestamp(modified),
        });
    }
    let subdirs = cur.u32()?;
    for _ in 0..subdirs {
        parse_node(cur, &dir_path, data_start, out, depth + 1)?;
    }
    Some(name)
}

/// Builds a container in memory. Used by unit tests to exercise the parser
/// without a real save, and documents the layout from the writer's side.
#[cfg(test)]
pub(crate) fn build_container(title: &str, files: &[(&str, &str, &[u8])], subdir: Option<(&str, &[(&str, &str, &[u8])])>) -> Vec<u8> {
    fn put_str(v: &mut Vec<u8>, s: &str) {
        v.extend_from_slice(&(s.len() as u32).to_le_bytes());
        v.extend_from_slice(s.as_bytes());
    }
    let mut body = Vec::new();
    let mut dir = Vec::new();
    let write_node = |dir: &mut Vec<u8>, body: &mut Vec<u8>, name: &str, files: &[(&str, &str, &[u8])], nsub: u32| {
        put_str(dir, name);
        dir.extend_from_slice(&(files.len() as u32).to_le_bytes());
        for (n, ext, content) in files {
            let frame = zstd::bulk::compress(content, 3).unwrap();
            put_str(dir, n);
            put_str(dir, ext);
            dir.extend_from_slice(&(body.len() as u64).to_le_bytes());
            dir.extend_from_slice(&(frame.len() as u64).to_le_bytes());
            dir.extend_from_slice(&(content.len() as u64).to_le_bytes());
            dir.extend_from_slice(&UNSET_TIMESTAMP.to_le_bytes());
            dir.extend_from_slice(&UNSET_TIMESTAMP.to_le_bytes());
            body.extend_from_slice(&frame);
        }
        dir.extend_from_slice(&nsub.to_le_bytes());
    };
    write_node(&mut dir, &mut body, title, files, subdir.is_some() as u32);
    if let Some((name, sfiles)) = subdir {
        write_node(&mut dir, &mut body, name, sfiles, 0);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&SIGNATURE);
    out.extend_from_slice(&((17 + body.len()) as u64).to_le_bytes());
    out.extend_from_slice(&17u64.to_le_bytes());
    out.push(3);
    out.extend_from_slice(&body);
    out.extend_from_slice(&SIGNATURE);
    out.extend_from_slice(&zstd::bulk::compress(&dir, 3).unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_synthetic_container() {
        let bytes = build_container(
            "my save",
            &[("game_info", ".dat", b"hello"), ("game_db", ".dat", &[7u8; 1000])],
            Some(("rgman", &[("rule_group", ".dat", b"xyz")])),
        );
        let c = FmContainer::from_bytes(bytes).unwrap();
        assert_eq!(c.title, "my save");
        assert_eq!(c.header.data_start, 26);
        assert_eq!(c.entries.len(), 3);
        assert_eq!(c.entries[0].offset, 26);
        assert_eq!(c.entries[2].path, "my save/rgman/rule_group.dat");
        assert_eq!(c.read("game_info.dat").unwrap(), b"hello");
        assert_eq!(c.read("game_db.dat").unwrap(), vec![7u8; 1000]);
        assert_eq!(c.read("my save/rgman/rule_group.dat").unwrap(), b"xyz");
        assert!(c.entries[0].created.is_none());
        assert!(matches!(c.read("nope.dat"), Err(FmError::MissingEntry(_))));
    }

    #[test]
    fn rejects_bad_signature() {
        let mut bytes = build_container("x", &[("a", ".dat", b"1")], None);
        bytes[2] = b'X';
        assert!(matches!(FmContainer::from_bytes(bytes), Err(FmError::InvalidFile(_))));
    }

    #[test]
    fn accepts_other_container_version_but_not_other_compression() {
        let mut bytes = build_container("x", &[("a", ".dat", b"1")], None);
        let dir = 9 + u64::from_le_bytes(bytes[9..17].try_into().unwrap()) as usize;
        bytes[6] = 7;
        bytes[dir + 6] = 7;
        let c = FmContainer::from_bytes(bytes.clone()).unwrap();
        assert_eq!(c.header.container_version, [7, 0, 0]);
        bytes[25] = 9;
        assert!(FmContainer::from_bytes(bytes).is_err());
    }

    #[test]
    fn rejects_bad_directory_offset() {
        let mut bytes = build_container("x", &[("a", ".dat", b"1")], None);
        bytes[9] ^= 1;
        assert!(FmContainer::from_bytes(bytes).is_err());
    }
}
