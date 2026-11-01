// Prints version information and the structure of game_db.dat for an FM save.
// Usage: node analyze_fm_data.js [save.fm]
const { openContainer, detectVersion, chunkHeader, locateNameTables, LAYOUTS } = require('./lib/fmsave');

const inputFile = process.argv[2] || './saves/fm24.fm';
const c = openContainer(inputFile);
const db = c.read('game_db.dat');
const v = detectVersion(c, db);

console.log(`Save:          ${c.title}`);
console.log(`Game version:  ${v.gameVersion} (FM${v.major})`);
console.log(`DB version:    ${v.dbVersion}, tag ${v.dbTag}, game_db format ${v.gameDbFormat}`);
console.log(`Layout:        ${LAYOUTS[v.layoutMajor] ? LAYOUTS[v.layoutMajor].name : 'none registered'}`);

const kinds = {};
for (const e of c.entries) {
  const h = chunkHeader(c.read(e).subarray(0, 8));
  const k = h ? h.kind : '(no chunk header)';
  kinds[k] = (kinds[k] || 0) + 1;
}
console.log('Chunk kinds:  ', kinds);

const t = locateNameTables(db);
if (t) {
  console.log(`Name tables at ${t.first.offset}: ${t.first.names.length} first, ${t.last.names.length} surnames, ${t.common.names.length} common`);
} else {
  console.log('Name tables not found');
}
