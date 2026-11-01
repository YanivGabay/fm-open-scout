// Lists the files inside an FM save and extracts one of them (default game_db.dat).
// Usage: node decompress_fm.js [save.fm] [entry] [out]
const fs = require('fs');
const { openContainer } = require('./lib/fmsave');

const inputFile = process.argv[2] || './saves/fm24.fm';
const entryName = process.argv[3] || 'game_db.dat';
const outFile = process.argv[4] || './fm_main_data.dat';

const c = openContainer(inputFile);
console.log(`Save: ${c.title} (${c.size} bytes, ${c.entries.length} files, directory at ${c.header.dirOffset})`);
const byDir = {};
for (const e of c.entries) {
  const dir = e.path.split('/').slice(0, -1).join('/');
  byDir[dir] = (byDir[dir] || 0) + 1;
}
console.log('Directories:', byDir);
const largest = [...c.entries].sort((a, b) => b.size - a.size).slice(0, 8);
for (const e of largest) console.log(`  ${e.path.padEnd(60)} ${String(e.compressedSize).padStart(10)} -> ${String(e.size).padStart(10)}`);

const data = c.read(entryName);
fs.writeFileSync(outFile, data);
console.log(`Extracted ${entryName}: ${data.length} bytes -> ${outFile}`);
