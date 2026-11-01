// Finds player records in an FM save and prints the top players by Current Ability.
// Usage: node find_player_records.js [save.fm] [top]
const { openContainer, detectVersion, locateNameTables, readPlayers, LAYOUTS } = require('./lib/fmsave');

const inputFile = process.argv[2] || './saves/fm24.fm';
const top = Number(process.argv[3] || 25);

const c = openContainer(inputFile);
const db = c.read('game_db.dat');
const v = detectVersion(c, db);
const layout = LAYOUTS[v.layoutMajor];
if (!layout) {
  console.error(`FM${v.major}: no layout registered (the Rust parser can probe unknown versions)`);
  process.exit(1);
}
const tables = locateNameTables(db);
const players = readPlayers(db, layout, tables);
const named = players.filter((p) => p.name).length;
console.log(`FM${v.major} (${v.gameVersion}): ${players.length} players, ${named} named`);

players.sort((a, b) => b.ca - a.ca);
for (const p of players.slice(0, top)) {
  const dob = p.dob ? `${p.dob.year}/${p.dob.day}` : '?';
  console.log(`${(p.name || '?').padEnd(32)} CA ${String(p.ca).padStart(3)} PA ${String(p.pa).padStart(3)}  born ${dob.padEnd(9)} nat ${p.nation} club ${p.clubId} uid ${p.uid}`);
}
