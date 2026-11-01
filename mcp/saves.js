// Finding saves, running the Rust parser on them (read-only) and caching the result.
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { execFile } = require('child_process');
const { slim } = require('./scout');

const pkg = require('../package.json');

const REPO = path.resolve(__dirname, '..');
const EXE = process.platform === 'win32' ? 'fm_save_parser.exe' : 'fm_save_parser';
const CACHE_DIR = process.env.FM_OPEN_SCOUT_CACHE || path.join(os.homedir(), '.cache', 'fm-open-scout');
// FM_SAVE_PARSER, a release binary put in bin/, or a local cargo build.
const LOCAL_PARSER = process.env.FM_SAVE_PARSER
  || [path.join(REPO, 'bin', EXE), path.join(REPO, 'fm_save_parser', 'target', 'release', EXE)].find((p) => fs.existsSync(p));

// Release asset per platform (see .github/workflows/release.yml).
const RELEASE_ASSETS = {
  'linux-x64': 'fm_save_parser-x86_64-unknown-linux-gnu',
  'win32-x64': 'fm_save_parser-x86_64-pc-windows-msvc.exe',
  'darwin-arm64': 'fm_save_parser-aarch64-apple-darwin',
  'darwin-x64': 'fm_save_parser-x86_64-apple-darwin',
};
const RELEASES = process.env.FM_OPEN_SCOUT_RELEASES || `https://github.com/YanivGabay/fm-open-scout/releases/download/v${pkg.version}`;

async function fetchOk(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`download failed: ${url} (HTTP ${res.status})`);
  return Buffer.from(await res.arrayBuffer());
}

// Downloads a release asset to `dest` after checking it against the release's
// SHA256SUMS. Nothing is written when the checksum does not match.
async function downloadParser(base, asset, dest) {
  const sums = (await fetchOk(`${base}/SHA256SUMS`)).toString('utf8');
  const line = sums.split('\n').find((l) => l.trim().split(/\s+/)[1] === asset);
  if (!line) throw new Error(`${asset} is not listed in ${base}/SHA256SUMS`);
  const bytes = await fetchOk(`${base}/${asset}`);
  const got = crypto.createHash('sha256').update(bytes).digest('hex');
  if (got !== line.trim().split(/\s+/)[0]) throw new Error(`checksum mismatch for ${asset}: refusing to use it`);
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  const tmp = `${dest}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, bytes, { mode: 0o755 });
  fs.renameSync(tmp, dest);
  return dest;
}

// The parser to run: a local one, or the release binary for this platform,
// downloaded once into the cache.
async function parserPath() {
  if (LOCAL_PARSER) return LOCAL_PARSER;
  const asset = RELEASE_ASSETS[`${process.platform}-${process.arch}`];
  const dest = path.join(CACHE_DIR, 'bin', `v${pkg.version}`, EXE);
  if (fs.existsSync(dest)) return dest;
  if (!asset) throw new Error(`no prebuilt parser for ${process.platform}-${process.arch}. Build it with "cargo build --release" in fm_save_parser/ and set FM_SAVE_PARSER.`);
  return downloadParser(RELEASES, asset, dest);
}
const CACHE_FORMAT = 5;

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

async function runParser(file, out) {
  const parser = await parserPath();
  return new Promise((resolve, reject) => {
    execFile(parser, ['players', file, '--top', '0', '--json', out], { maxBuffer: 16 * 1024 * 1024 }, (err, stdout, stderr) => {
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

module.exports = { CACHE_DIR, RELEASE_ASSETS, defaultSaveDirs, listSaves, loadSave, parserPath, downloadParser };
