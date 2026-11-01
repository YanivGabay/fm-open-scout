// Minimal JavaScript reader for Football Manager .fm saves (mirrors the Rust
// parser in fm_save_parser/; see FORMAT.md for the format and evidence).
const fs = require('fs');
const fzstd = require('fzstd');

const SIGNATURE = Buffer.from([0x02, 0x01, 0x66, 0x6d, 0x66, 0x2e, 0x08, 0x00, 0x00]);
const UNSET_TIMESTAMP = -62135596800n;

// Player layouts keyed by FM major version (offsets relative to the CA field).
const LAYOUTS = {
  24: { name: 'FM24', headerLen: 19, club: 15, nationAfterName: 9 },
  26: { name: 'FM26', headerLen: 20, club: 16, nationAfterName: 13 },
  27: { name: 'FM27', headerLen: 20, club: 16, nationAfterName: 14 },
};

// First u32-length string in save_game_summary.dat shaped like a build ("27.0.0+2486695").
function savedWith(summary) {
  for (let p = 8; p + 8 < summary.length; p++) {
    const len = summary.readUInt32LE(p);
    if (len < 5 || len > 24 || p + 4 + len > summary.length) continue;
    const s = summary.toString('latin1', p + 4, p + 4 + len);
    if (/^\d\d\.\d+\.\d+(\+\d+)?$/.test(s)) return s;
  }
  return null;
}

function openContainer(path) {
  const data = fs.readFileSync(path);
  if (!data.subarray(0, 9).equals(SIGNATURE)) throw new Error(`${path}: not an .fm save (bad signature)`);
  const bodyLength = Number(data.readBigUInt64LE(9));
  const dataStart = 9 + Number(data.readBigUInt64LE(17));
  const dirOffset = 9 + bodyLength;
  if (!data.subarray(dirOffset, dirOffset + 9).equals(SIGNATURE)) throw new Error('directory signature missing');
  const dir = Buffer.from(fzstd.decompress(data.subarray(dirOffset + 9)));

  let p = 0;
  const str = () => { const n = dir.readUInt32LE(p); p += 4; const s = dir.toString('utf8', p, p + n); p += n; return s; };
  const u64 = () => { const v = dir.readBigUInt64LE(p); p += 8; return Number(v); };
  const i64 = () => { const v = dir.readBigInt64LE(p); p += 8; return v === UNSET_TIMESTAMP ? null : Number(v); };
  const entries = [];
  function node(parent) {
    const name = str();
    const path = parent ? `${parent}/${name}` : name;
    const files = dir.readUInt32LE(p); p += 4;
    for (let i = 0; i < files; i++) {
      const fname = str(), ext = str();
      entries.push({ path: `${path}/${fname}${ext}`, fileName: fname + ext, offset: dataStart + u64(),
        compressedSize: u64(), size: u64(), created: i64(), modified: i64() });
    }
    const subdirs = dir.readUInt32LE(p); p += 4;
    for (let i = 0; i < subdirs; i++) node(path);
    return name;
  }
  const title = node('');

  const find = (name) => entries.find((e) => e.path === name || e.fileName === name);
  const read = (name) => {
    const e = typeof name === 'string' ? find(name) : name;
    if (!e) throw new Error(`entry not found: ${name}`);
    return Buffer.from(fzstd.decompress(data.subarray(e.offset, e.offset + e.compressedSize)));
  };
  return { path, size: data.length, header: { bodyLength, dataStart, dirOffset }, title, entries, find, read };
}

// Per-file chunk header: 03 01, reversed extension ("tad." = .dat), u16 format version.
function chunkHeader(buf) {
  if (buf[0] !== 3 || buf[1] !== 1) return null;
  return { kind: [...buf.subarray(2, 6)].reverse().map((c) => String.fromCharCode(c)).join(''), version: buf.readUInt16LE(6) };
}

function detectVersion(container, gameDb) {
  const info = container.read('game_info.dat');
  const len = info.readUInt32LE(8);
  const gameVersion = info.toString('ascii', 12, 12 + len);
  const db = gameDb || container.read('game_db.dat');
  const m = db.indexOf('DB', 8);
  const dbVersion = m >= 0 && m < 64 ? db.readUInt16LE(m + 4) : null;
  let dbTag = null;
  for (let o = m + 6; m >= 0 && o < m + 16; o++) {
    if (db.readUInt32LE(o) === 4 && /^\d{4}$/.test(db.toString('ascii', o + 4, o + 8))) { dbTag = db.toString('ascii', o + 4, o + 8); break; }
  }
  const major = parseInt(gameVersion, 10) || (dbTag ? parseInt(dbTag.slice(0, 2), 10) : null);
  // FM27 re-saves FM24 careers in its own layout but keeps the FM24 version and tag.
  const summary = container.find('save_game_summary.dat') ? container.read('save_game_summary.dat') : null;
  const saved = summary && savedWith(summary);
  const layoutMajor = Math.max(major || 0, saved ? parseInt(saved, 10) : 0) || null;
  return { gameVersion, savedWith: saved, major, layoutMajor, dbVersion, dbTag, gameInfoFormat: chunkHeader(info).version, gameDbFormat: chunkHeader(db).version };
}

// Three consecutive tables of [u32 count][count x (u32 id == index, u32 len, utf8)].
function locateNameTables(d) {
  const parse = (off) => {
    const count = d.readUInt32LE(off);
    if (count === 0 || count > (d.length - off) / 8) return null;
    const names = new Array(count);
    let p = off + 4;
    for (let i = 0; i < count; i++) {
      if (p + 8 > d.length || d.readUInt32LE(p) !== i) return null;
      const len = d.readUInt32LE(p + 4);
      if (len > 200) return null;
      names[i] = d.toString('utf8', p + 8, p + 8 + len);
      p += 8 + len;
    }
    return { offset: off, names, end: p };
  };
  for (let p = 4; p + 16 < d.length; p++) {
    if (d.readUInt32LE(p) !== 0) continue;
    const len = d.readUInt32LE(p + 4);
    if (len < 1 || len > 60 || d.readUInt32LE(p + 8 + len) !== 1 || d.readUInt32LE(p - 4) < 1000) continue;
    const first = parse(p - 4); if (!first) continue;
    const last = parse(first.end); if (!last) continue;
    const common = parse(last.end); if (!common) continue;
    return { first, last, common };
  }
  return null;
}

const plausibleDate = (d, o) => {
  const day = d.readUInt16LE(o) & 0x1ff, y = d.readUInt16LE(o + 2);
  return (day === 1 && y === 1900) || (day >= 1 && day <= 366 && y >= 1900 && y <= 2200);
};

// UID-anchored scan for player ability blocks; returns CA offsets.
function findPlayerBlocks(d, layout) {
  const pos = 4 + layout.headerLen, attrs = pos + 15, joined = attrs + 59, tail = joined + 8;
  const out = [];
  for (let u = 0; u + 15 + tail + 32 < d.length; u++) {
    if (d[u] !== d[u + 4] || d[u + 1] !== d[u + 5] || d[u + 2] !== d[u + 6] || d[u + 3] !== d[u + 7]) continue;
    const uid = d.readUInt32LE(u);
    if (uid === 0 || uid === 0xffffffff) continue;
    const r = u + 15;
    const ca = d.readUInt16LE(r), pa = d.readInt16LE(r + 2);
    if (ca < 1 || ca > 200 || pa < -10 || pa > 200) continue;
    let ok = true;
    for (let i = 0; i < 15 && ok; i++) { const v = d[r + pos + i]; ok = v >= 1 && v <= 20; }
    for (let i = 0; i < 54 && ok; i++) { const v = d[r + attrs + i]; ok = v >= 1 && v <= 100; }
    if (!ok || !plausibleDate(d, r + joined) || !plausibleDate(d, r + joined + 4)) continue;
    out.push(r);
    u = r + tail - 1;
  }
  return out;
}

// Person name block: first id,0,surname id,0,common id|FFFFFFFF,0,u32 len,full name,DOB.
function nameBlockAt(d, q, t, nationAfter) {
  if (d[q + 4] | d[q + 9] | d[q + 14]) return null;
  const f = d.readUInt32LE(q), s = d.readUInt32LE(q + 5), c = d.readUInt32LE(q + 10);
  if (f >= t.first.names.length || s >= t.last.names.length) return null;
  if (c === 0 || (f === 0 && s === 0)) return null; // zero-filled bytes, not a name
  if (c !== 0xffffffff && c >= t.common.names.length) return null;
  const len = d.readUInt32LE(q + 15);
  if (len > 160 || q + 23 + len > d.length) return null;
  const full = d.toString('utf8', q + 19, q + 19 + len);
  if (/[\u0000-\u001f�]/.test(full)) return null;
  const after = q + 19 + len;
  const day = d.readUInt16LE(after) & 0x1ff, year = d.readUInt16LE(after + 2);
  if (day < 1 || day > 366 || year < 1900 || year > 2100 || (day === 1 && year === 1900)) return null;
  if (nationAfter !== undefined && d.readUInt16LE(after + nationAfter) >= 512) return null; // not a real nationality
  return { first: t.first.names[f] || null, last: t.last.names[s] || null,
    common: c === 0xffffffff ? null : t.common.names[c] || null, full: full || null, dob: { day, year }, after };
}

function readPlayers(d, layout, tables) {
  const blocks = findPlayerBlocks(d, layout);
  const tailOff = 4 + layout.headerLen + 15 + 54 + 13;
  return blocks.map((r, i) => {
    const end = Math.min(i + 1 < blocks.length ? blocks[i + 1] - 19 : d.length, r + tailOff + 32768);
    let nb = null;
    if (tables) for (let q = r + tailOff; q < end && !nb; q++) nb = nameBlockAt(d, q, tables, layout.nationAfterName);
    const name = nb && (nb.common || [nb.first, nb.last].filter(Boolean).join(' ') || nb.full);
    const club = d.readUInt16LE(r + layout.club);
    return { uid: d.readUInt32LE(r - 15), offset: r, ca: d.readUInt16LE(r), pa: d.readInt16LE(r + 2), name,
      dob: nb ? nb.dob : null, nation: nb ? d.readUInt16LE(nb.after + layout.nationAfterName) : null,
      clubId: club === 0xffff ? null : club };
  });
}

module.exports = { SIGNATURE, LAYOUTS, openContainer, chunkHeader, detectVersion, locateNameTables, findPlayerBlocks, readPlayers };
