// Finding saves, running the Rust parser on them (read-only) and caching the result.
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { execFile } = require('child_process');
const { slim } = require('./scout');

const REPO = path.resolve(__dirname, '..');
const PARSER = process.env.FM_SAVE_PARSER
  || path.join(REPO, 'fm_save_parser', 'target', 'release', process.platform === 'win32' ? 'fm_save_parser.exe' : 'fm_save_parser');
const CACHE_DIR = process.env.FM_OPEN_SCOUT_CACHE || path.join(os.homedir(), '.cache', 'fm-open-scout');
const CACHE_FORMAT = 3;

function listDir(dir) {
  try { return fs.readdirSync(dir); } catch { return []; }
}

// Football Manager <year>/games and .../cloud/games under every known root.
function defaultSaveDirs() {
  if (process.env.FM_SAVE_DIRS) return process.env.FM_SAVE_DIRS.split(path.delimiter).filter(Boolean);
  const roots = [];
  const home = os.homedir();
  if (process.platform === 'win32') {
    roots.push(path.join(home, 'Documents', 'Sports Interactive'));
    if (process.env.LOCALAPPDATA) roots.push(path.join(process.env.LOCALAPPDATA, 'Sports Interactive'));
  } else if (process.platform === 'darwin') {
    roots.push(path.join(home, 'Library', 'Application Support', 'Sports Interactive'));
  } else {
    roots.push(path.join(home, '.local', 'share', 'Sports Interactive'));
    // WSL: the Windows user folders.
    for (const user of listDir('/mnt/c/Users')) {
      roots.push(`/mnt/c/Users/${user}/Documents/Sports Interactive`, `/mnt/c/Users/${user}/AppData/Local/Sports Interactive`);
    }
  }
  const dirs = [];
  for (const root of roots) {
    for (const game of listDir(root).filter((d) => d.startsWith('Football Manager'))) {
      dirs.push(path.join(root, game, 'games'), path.join(root, game, 'cloud', 'games'));
    }
  }
  return dirs.filter((d) => fs.existsSync(d));
}

// Every .fm file in the save directories, newest first.
function listSaves(dirs = defaultSaveDirs()) {
  const saves = [];
  for (const dir of dirs) {
    for (const f of listDir(dir).filter((f) => f.endsWith('.fm'))) {
      const file = path.join(dir, f);
      const st = fs.statSync(file);
      const game = file.split(/[\\/]/).find((s) => s.startsWith('Football Manager')) || '';
      saves.push({ path: file, name: f.replace(/\.fm$/, ''), game, modified: st.mtime, size: st.size });
    }
  }
  return saves.sort((a, b) => b.modified - a.modified);
}

function cacheKey(file) {
  const st = fs.statSync(file);
  return crypto.createHash('sha1').update(`${CACHE_FORMAT}|${path.resolve(file)}|${st.size}|${st.mtimeMs}`).digest('hex');
}

function runParser(file, out) {
  return new Promise((resolve, reject) => {
    if (!fs.existsSync(PARSER)) {
      reject(new Error(`parser not found at ${PARSER}. Build it with "cargo build --release" in fm_save_parser/, or set FM_SAVE_PARSER.`));
      return;
    }
    execFile(PARSER, ['players', file, '--top', '0', '--json', out], { maxBuffer: 16 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err) reject(new Error(`parser failed: ${stderr || err.message}`));
      else resolve(stderr);
    });
  });
}

// Parses a save (or reads the cached result). The save file itself is only read.
async function loadSave(file) {
  fs.mkdirSync(CACHE_DIR, { recursive: true });
  const cached = path.join(CACHE_DIR, `${cacheKey(file)}.json`);
  if (fs.existsSync(cached)) return JSON.parse(fs.readFileSync(cached, 'utf8'));
  const raw = path.join(CACHE_DIR, `raw-${process.pid}-${Date.now()}.json`);
  try {
    const warnings = await runParser(file, raw);
    const data = slim(JSON.parse(fs.readFileSync(raw, 'utf8')), file);
    data.parserLog = warnings.trim();
    fs.writeFileSync(cached, JSON.stringify(data));
    return data;
  } finally {
    fs.rmSync(raw, { force: true });
  }
}

module.exports = { PARSER, CACHE_DIR, defaultSaveDirs, listSaves, loadSave };
