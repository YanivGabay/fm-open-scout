// Finding saves, running the Rust parser on them (read-only) and caching the result.
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { execFile } = require('child_process');
const { slim } = require('./scout');

const REPO = path.resolve(__dirname, '..');
const EXE = process.platform === 'win32' ? 'fm_save_parser.exe' : 'fm_save_parser';
// FM_SAVE_PARSER, a downloaded release binary in bin/, or a local cargo build.
const PARSER = process.env.FM_SAVE_PARSER
  || [path.join(REPO, 'bin', EXE), path.join(REPO, 'fm_save_parser', 'target', 'release', EXE)].find((p) => fs.existsSync(p))
  || path.join(REPO, 'fm_save_parser', 'target', 'release', EXE);
const CACHE_DIR = process.env.FM_OPEN_SCOUT_CACHE || path.join(os.homedir(), '.cache', 'fm-open-scout');
const CACHE_FORMAT = 4;

function listDir(dir) {
  try { return fs.readdirSync(dir); } catch { return []; }
}

// Football Manager <year>/games and .../cloud/games under every known root.
function defaultSaveDirs() {
  if (process.env.FM_SAVE_DIRS) return process.env.FM_SAVE_DIRS.split(path.delimiter).filter(Boolean);
  const roots = [];
  const home = os.homedir();
  // Windows user folder: Documents (possibly redirected to OneDrive) and AppData.
  const windowsUser = (userDir, localAppData) => {
    roots.push(path.join(userDir, 'Documents', 'Sports Interactive'), path.join(userDir, 'OneDrive', 'Documents', 'Sports Interactive'));
    roots.push(path.join(localAppData || path.join(userDir, 'AppData', 'Local'), 'Sports Interactive'));
  };
  if (process.platform === 'win32') {
    windowsUser(home, process.env.LOCALAPPDATA);
    if (process.env.OneDrive) roots.push(path.join(process.env.OneDrive, 'Documents', 'Sports Interactive'));
  } else if (process.platform === 'darwin') {
    roots.push(path.join(home, 'Library', 'Application Support', 'Sports Interactive'));
  } else {
    roots.push(path.join(home, '.local', 'share', 'Sports Interactive'));
    // Steam Proton (Linux, Steam Deck): each game's Windows prefix.
    for (const steam of [path.join(home, '.steam', 'steam'), path.join(home, '.local', 'share', 'Steam')]) {
      const compat = path.join(steam, 'steamapps', 'compatdata');
      for (const app of listDir(compat)) windowsUser(path.join(compat, app, 'pfx', 'drive_c', 'users', 'steamuser'));
    }
    // WSL: the Windows user folders.
    for (const user of listDir('/mnt/c/Users')) windowsUser(`/mnt/c/Users/${user}`);
  }
  const dirs = [];
  for (const root of roots) {
    for (const game of listDir(root).filter((d) => d.startsWith('Football Manager'))) {
      dirs.push(path.join(root, game, 'games'), path.join(root, game, 'cloud', 'games'));
    }
  }
  return [...new Set(dirs)].filter((d) => fs.existsSync(d));
}

// Every .fm file in the save directories, newest first.
function listSaves(dirs = defaultSaveDirs()) {
  const saves = [];
  for (const dir of dirs) {
    for (const f of listDir(dir).filter((f) => f.endsWith('.fm'))) {
      const file = path.join(dir, f);
      let st;
      try { st = fs.statSync(file); } catch { continue; } // removed while listing (FM rewriting it)
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
      reject(new Error(`parser not found at ${PARSER}. Download it from the GitHub releases into bin/ (as ${EXE}), build it with "cargo build --release" in fm_save_parser/, or set FM_SAVE_PARSER.`));
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
