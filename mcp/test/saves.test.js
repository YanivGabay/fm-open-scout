// Save discovery on a fake home folder: node --test mcp/test
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const os = require('os');
const path = require('path');

test('finds saves in Steam Proton prefixes, newest first', { skip: process.platform === 'win32' || process.platform === 'darwin' }, () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'fmos-home-'));
  const old = { HOME: process.env.HOME, FM_SAVE_DIRS: process.env.FM_SAVE_DIRS };
  try {
    const games = path.join(home, '.local', 'share', 'Steam', 'steamapps', 'compatdata', '123', 'pfx', 'drive_c', 'users', 'steamuser',
      'Documents', 'Sports Interactive', 'Football Manager 27', 'games');
    fs.mkdirSync(games, { recursive: true });
    fs.writeFileSync(path.join(games, 'Old.fm'), 'x');
    fs.writeFileSync(path.join(games, 'New.fm'), 'x');
    fs.writeFileSync(path.join(games, 'notes.txt'), 'x');
    fs.utimesSync(path.join(games, 'Old.fm'), new Date(2020, 0, 1), new Date(2020, 0, 1));
    process.env.HOME = home;
    delete process.env.FM_SAVE_DIRS;
    const { listSaves } = require('../saves');
    const found = listSaves().filter((s) => s.path.startsWith(home));
    assert.deepEqual(found.map((s) => s.name), ['New', 'Old']);
    assert.equal(found[0].game, 'Football Manager 27');
  } finally {
    for (const [k, v] of Object.entries(old)) if (v === undefined) delete process.env[k]; else process.env[k] = v;
    fs.rmSync(home, { recursive: true, force: true });
  }
});
