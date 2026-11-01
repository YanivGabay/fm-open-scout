// Scouting logic over parsed players: role scores, search, squads, similar players.
// Pure functions on the data returned by `slim`, so they can be tested without saves.

// Attributes that matter for each role: key ones count double. Eligibility is the
// position rating (1..20) for the listed positions.
const ROLES = {
  GK: { positions: ['GK'], key: ['handling', 'reflexes', 'one_on_ones', 'positioning', 'aerial_reach', 'command_of_area'], sec: ['communication', 'kicking', 'concentration', 'decisions', 'agility', 'anticipation', 'composure'] },
  CB: { positions: ['DC'], key: ['tackling', 'marking', 'heading', 'positioning', 'jumping_reach', 'anticipation', 'concentration', 'strength'], sec: ['decisions', 'composure', 'bravery', 'pace', 'acceleration', 'passing'] },
  FB: { positions: ['DL', 'DR', 'WBL', 'WBR'], key: ['pace', 'acceleration', 'stamina', 'work_rate', 'tackling', 'positioning'], sec: ['crossing', 'dribbling', 'teamwork', 'decisions', 'marking', 'anticipation', 'concentration', 'passing'] },
  DM: { positions: ['DM'], key: ['tackling', 'anticipation', 'positioning', 'concentration', 'decisions', 'teamwork'], sec: ['work_rate', 'marking', 'passing', 'composure', 'stamina', 'strength'] },
  CM: { positions: ['MC'], key: ['passing', 'decisions', 'first_touch', 'teamwork', 'work_rate', 'vision'], sec: ['technique', 'anticipation', 'stamina', 'composure', 'off_the_ball', 'tackling'] },
  AM: { positions: ['AMC'], key: ['passing', 'vision', 'technique', 'first_touch', 'dribbling', 'decisions'], sec: ['off_the_ball', 'composure', 'flair', 'acceleration', 'agility', 'long_shots', 'finishing', 'anticipation'] },
  W: { positions: ['AML', 'AMR', 'ML', 'MR'], key: ['acceleration', 'pace', 'dribbling', 'crossing', 'technique', 'flair'], sec: ['first_touch', 'off_the_ball', 'agility', 'work_rate', 'decisions', 'passing'] },
  ST: { positions: ['ST'], key: ['finishing', 'composure', 'off_the_ball', 'first_touch', 'anticipation', 'acceleration'], sec: ['pace', 'dribbling', 'heading', 'decisions', 'technique', 'strength', 'jumping_reach'] },
};

// Best-XI slots used to compare squads (display order).
const XI = [['GK', 1], ['FB', 2], ['CB', 2], ['DM', 1], ['CM', 1], ['AM', 1], ['W', 2], ['ST', 1]];
// Fill order: specialist slots first, so a natural striker is not used up on the
// wing before the striker slot is filled.
const FILL_ORDER = ['GK', 'CB', 'FB', 'ST', 'DM', 'CM', 'AM', 'W'];

const YEAR_MS = 365.25 * 24 * 3600 * 1000;

const fold = (s) => (s || '').normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().trim();

// Keeps the fields the tools use and computes ages; drops unnamed records.
function slim(parsed, file) {
  const date = parsed.version.current_date;
  const now = date ? new Date(date) : new Date();
  const players = [];
  for (const p of parsed.players) {
    if (!p.name || !p.attributes || !p.current_ability) continue;
    players.push({
      uid: p.uid,
      name: p.name,
      age: p.date_of_birth ? Math.floor(((now - new Date(p.date_of_birth)) / YEAR_MS) * 10) / 10 : null,
      dob: p.date_of_birth,
      ca: p.current_ability,
      pa: p.potential_ability,
      nation: p.nation_name || null,
      nationUid: p.nation_uid,
      club: p.club ? p.club.name || p.club.short_name : null,
      clubShort: p.club ? p.club.short_name || null : null,
      value: p.valuation_unverified || 0,
      wage: p.contract ? p.contract.weekly_wage : null,
      contractEnd: p.contract ? p.contract.ends : null,
      height: p.height_cm,
      foot: p.preferred_foot,
      best: p.best_position,
      positions: p.positions,
      attributes: p.attributes,
      personality: p.personality || null,
      reputation: p.reputation || null,
      lastFee: p.last_transfer ? p.last_transfer.fee : null,
    });
  }
  return {
    file,
    title: parsed.title,
    version: parsed.version,
    layout: { name: parsed.layout.name, confidence: parsed.layout.confidence },
    warnings: parsed.warnings,
    players,
  };
}

function roleScore(p, role) {
  const r = ROLES[role];
  let sum = 0, n = 0;
  for (const k of r.key) { sum += 2 * (p.attributes[k] || 0); n += 2; }
  for (const k of r.sec) { sum += p.attributes[k] || 0; n += 1; }
  return sum / n;
}

const roleRating = (p, role) => Math.max(...ROLES[role].positions.map((x) => (p.positions && p.positions[x]) || 0));

// Roles a player can play (rating >= min), best score first.
function rolesOf(p, min = 15) {
  return Object.keys(ROLES).filter((r) => roleRating(p, r) >= min)
    .map((r) => ({ role: r, score: roleScore(p, r) })).sort((a, b) => b.score - a.score);
}

// Median value for each CA (window +-2), to judge whether a price is high or low.
function valueMedians(players) {
  const byCa = new Map();
  for (const p of players) if (p.value > 0) (byCa.get(p.ca) || byCa.set(p.ca, []).get(p.ca)).push(p.value);
  const med = {};
  for (let ca = 1; ca <= 200; ca++) {
    const vals = [];
    for (let c = ca - 2; c <= ca + 2; c++) if (byCa.has(c)) vals.push(...byCa.get(c));
    if (vals.length) { vals.sort((a, b) => a - b); med[ca] = vals[vals.length >> 1]; }
  }
  return med;
}

function context(data) {
  if (!data.medians) Object.defineProperty(data, 'medians', { value: valueMedians(data.players), enumerable: false });
  return data;
}

// Price relative to the usual price for the player's CA (1 = typical, 0.3 = cheap).
function priceIndex(data, p) {
  const m = context(data).medians[p.ca];
  return p.value > 0 && m ? p.value / m : null;
}

// No club and no contract. A player with a contract but no club name plays for a
// club the parser could not resolve; he is not free.
const isFreeAgent = (p) => !p.club && p.wage == null && !p.contractEnd;

function sameClub(p, club) {
  return p.club && fold(p.club) === fold(club);
}

// Club by full or short name ("Man Utd"), exact match first, then partial.
function findClubName(data, query) {
  const q = fold(query);
  const clubs = new Map();
  for (const p of data.players) if (p.club && !clubs.has(p.club)) clubs.set(p.club, p.clubShort);
  const entries = [...clubs.entries()];
  const hit = entries.find(([n, s]) => fold(n) === q || fold(s) === q) || entries.find(([n, s]) => fold(n).includes(q) || fold(s).includes(q));
  return hit ? hit[0] : null;
}

function findPlayers(data, query) {
  const q = fold(query);
  if (/^\d+$/.test(q)) return data.players.filter((p) => String(p.uid) === q);
  const exact = data.players.filter((p) => fold(p.name) === q);
  if (exact.length) return exact.sort((a, b) => b.ca - a.ca);
  return data.players.filter((p) => fold(p.name).includes(q)).sort((a, b) => b.ca - a.ca);
}

const SORTS = {
  ca: (data, p) => p.ca,
  pa: (data, p) => p.pa,
  value: (data, p) => -p.value,
  potential_gap: (data, p) => p.pa - p.ca,
  bargain: (data, p) => { const i = priceIndex(data, p); return i == null ? (p.club ? -Infinity : 1) : -i; },
};

// Filters and ranks players. `role` restricts to players rated >= min_rating there
// and makes role score available for sorting.
function search(data, f = {}) {
  const role = f.role && ROLES[f.role] ? f.role : null;
  const minRating = f.min_rating ?? 15;
  const club = f.club ? findClubName(data, f.club) : null;
  const exclude = f.exclude_club ? findClubName(data, f.exclude_club) : null;
  const nation = f.nation ? fold(f.nation) : null;
  let rows = data.players.filter((p) => {
    if (role && roleRating(p, role) < minRating) return false;
    if (f.positions && !f.positions.some((x) => (p.positions[x] || 0) >= minRating)) return false;
    if (f.min_ca != null && p.ca < f.min_ca) return false;
    if (f.max_ca != null && p.ca > f.max_ca) return false;
    if (f.min_pa != null && p.pa < f.min_pa) return false;
    if (f.min_age != null && (p.age == null || p.age < f.min_age)) return false;
    if (f.max_age != null && (p.age == null || p.age >= f.max_age + 1)) return false;
    if (f.max_value != null && p.value > f.max_value) return false;
    if (f.min_value != null && p.value < f.min_value) return false;
    if (f.max_wage != null && p.wage != null && p.wage > f.max_wage) return false;
    if (f.max_price_index != null) { const i = priceIndex(data, p); if (i == null || i > f.max_price_index) return false; }
    if (f.free_agents_only && !isFreeAgent(p)) return false;
    if (club && p.club !== club) return false;
    if (exclude && p.club === exclude) return false;
    if (nation && fold(p.nation) !== nation) return false;
    return true;
  });
  const sort = f.sort || (role ? 'role' : 'ca');
  const key = sort === 'role' && role ? (p) => roleScore(p, role) : (p) => (SORTS[sort] || SORTS.ca)(data, p);
  rows = rows.map((p) => ({ p, k: key(p) })).sort((a, b) => b.k - a.k || b.p.ca - a.p.ca).map((x) => x.p);
  return { total: rows.length, club, exclude, rows: rows.slice(0, Math.min(f.limit || 20, 100)) };
}

// Best XI by slot: each player is used once; specialist slots are filled first.
function bestXI(data, club) {
  const squad = data.players.filter((p) => p.club === club).sort((a, b) => b.ca - a.ca);
  const used = new Set();
  const picked = {};
  const count = Object.fromEntries(XI);
  for (const role of FILL_ORDER) {
    const picks = [];
    // Natural players for the slot (rating 18+) first, then the rest, each by CA.
    const natural = (p) => (roleRating(p, role) >= 18 ? 1 : 0);
    for (const p of [...squad].sort((a, b) => natural(b) - natural(a) || b.ca - a.ca)) {
      if (picks.length >= count[role]) break;
      if (!used.has(p.uid) && roleRating(p, role) >= 15) { used.add(p.uid); picks.push(p); }
    }
    picked[role] = picks;
  }
  const slots = XI.map(([role]) => ({ role, players: picked[role] }));
  const all = slots.flatMap((s) => s.players);
  return { club, slots, average: all.length ? all.reduce((s, p) => s + p.ca, 0) / all.length : 0 };
}

const GK_ONLY = ['handling', 'aerial_reach', 'command_of_area', 'communication', 'kicking', 'throwing', 'one_on_ones', 'reflexes', 'eccentricity', 'rushing_out', 'tendency_to_punch'];
const NOT_STYLE = ['left_foot', 'right_foot', 'consistency', 'dirtiness', 'important_matches', 'injury_proneness', 'versatility'];

// Players like `ref`: by CA/PA/age profile, or by playing style (correlation of the
// attribute profiles, which ignores overall level) among players of a similar level.
function similar(data, ref, f = {}) {
  const mode = f.mode || 'profile';
  const keys = Object.keys(ref.attributes).filter((k) => !NOT_STYLE.includes(k) && (ref.best === 'GK' || !GK_ONLY.includes(k)));
  const centred = (p) => { const v = keys.map((k) => p.attributes[k] || 0); const m = v.reduce((a, b) => a + b, 0) / v.length; return v.map((x) => x - m); };
  const rv = centred(ref);
  const norm = (v) => Math.sqrt(v.reduce((s, x) => s + x * x, 0)) || 1;
  const corr = (v) => v.reduce((s, x, i) => s + x * rv[i], 0) / (norm(v) * norm(rv));
  const minCa = f.min_ca ?? (mode === 'attributes' ? ref.ca - 20 : null);
  const dist = (p) => (mode === 'attributes'
    ? 1 - corr(centred(p))
    : Math.abs(p.ca - ref.ca) / 10 + Math.abs(p.pa - ref.pa) / 10 + Math.abs((p.age ?? 99) - (ref.age ?? 99)) / 2);
  const rows = data.players.filter((p) => p.uid !== ref.uid
    && (f.max_value == null || p.value <= f.max_value)
    && (f.max_age == null || (p.age != null && p.age < f.max_age + 1))
    && (minCa == null || p.ca >= minCa)
    && (f.exclude_club == null || !sameClub(p, f.exclude_club))
    && (mode !== 'attributes' || (ref.best === 'GK') === (p.best === 'GK')))
    .map((p) => ({ p, d: dist(p) })).sort((a, b) => a.d - b.d).slice(0, Math.min(f.limit || 15, 50));
  return rows.map((r) => ({ ...r.p, similarity: mode === 'attributes' ? 1 - r.d : null }));
}

module.exports = { ROLES, XI, fold, slim, isFreeAgent, roleScore, roleRating, rolesOf, valueMedians, priceIndex, findClubName, findPlayers, search, bestXI, similar };
