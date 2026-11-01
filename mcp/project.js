// Development estimate: what a player's attributes may look like at a higher CA.
//
// FM does not store future attributes; it develops players at runtime from PA, age,
// personality, training and match time. This is an independent estimate built from
// the save itself: across all players of the same position, each attribute's trend
// against CA (least-squares slope) says how much it typically rises per CA point.
// That rise, from the current CA to the target, is applied to the player and scaled
// by age for each attribute group.

const GROUPS = {
  physical: ['acceleration', 'agility', 'balance', 'jumping_reach', 'natural_fitness', 'pace', 'stamina', 'strength'],
  technical: ['corners', 'crossing', 'dribbling', 'finishing', 'first_touch', 'free_kicks', 'heading', 'long_shots', 'long_throws', 'marking', 'passing', 'penalty_taking', 'tackling', 'technique'],
  mental: ['aggression', 'anticipation', 'bravery', 'composure', 'concentration', 'decisions', 'determination', 'flair', 'leadership', 'off_the_ball', 'positioning', 'teamwork', 'vision', 'work_rate'],
  goalkeeping: ['aerial_reach', 'command_of_area', 'communication', 'eccentricity', 'handling', 'kicking', 'one_on_ones', 'reflexes', 'rushing_out', 'tendency_to_punch', 'throwing'],
};

// Share of the typical improvement an attribute group still makes at a given age.
function ageFactor(group, age) {
  if (age == null) return 0.5;
  if (group === 'physical') return age < 21 ? 1 : age < 24 ? 0.8 : age < 27 ? 0.4 : 0;
  if (group === 'technical') return age < 24 ? 1 : age < 28 ? 0.7 : 0.3;
  if (group === 'mental') return age < 32 ? 1 : 0.5;
  return age < 30 ? 1 : 0.5;
}

// Negative PA is a range ("-9" = 150-180 and so on); use its midpoint.
const PA_RANGES = { '-10': [170, 200], '-9': [150, 180], '-8': [130, 160], '-7': [110, 140], '-6': [90, 120], '-5': [70, 100], '-4': [50, 80], '-3': [30, 60], '-2': [10, 40], '-1': [1, 30] };
function effectivePa(pa) {
  if (pa > 0) return { pa, range: null };
  const r = PA_RANGES[String(pa)];
  return r ? { pa: (r[0] + r[1]) / 2, range: r } : { pa: null, range: null };
}

// Share of the CA-PA gap a player is likely to close, from determination,
// professionalism and age (young players have more time).
function realisation(p) {
  const det = p.attributes.determination || 10;
  const pro = (p.personality && p.personality.professionalism) || 10;
  const time = p.age == null ? 0 : p.age <= 18 ? 0.1 : p.age <= 21 ? 0.05 : p.age >= 26 ? -0.15 : 0;
  return Math.min(1, Math.max(0.15, 0.45 + 0.035 * (det - 10) + 0.03 * (pro - 10) + time));
}

const positionGroup = (best) => ({ DL: 'FB', DR: 'FB', WBL: 'FB', WBR: 'FB', SW: 'DC', ML: 'W', MR: 'W', AML: 'W', AMR: 'W' }[best] || best);

const MIN_PEERS = 30;

// Per-attribute slope against CA over every player of a position group (CA >= 80),
// cached on the data set.
function trends(data, group) {
  if (!data.trendCache) Object.defineProperty(data, 'trendCache', { value: new Map(), enumerable: false });
  if (data.trendCache.has(group)) return data.trendCache.get(group);
  const peers = data.players.filter((q) => positionGroup(q.best) === group && q.ca >= 80);
  let result = null;
  if (peers.length >= MIN_PEERS) {
    const mx = peers.reduce((s, q) => s + q.ca, 0) / peers.length;
    const sxx = peers.reduce((s, q) => s + (q.ca - mx) ** 2, 0);
    const slope = {};
    for (const k of Object.keys(peers[0].attributes)) {
      const my = peers.reduce((s, q) => s + q.attributes[k], 0) / peers.length;
      slope[k] = peers.reduce((s, q) => s + (q.ca - mx) * (q.attributes[k] - my), 0) / sxx;
    }
    result = { slope, peers: peers.length };
  }
  data.trendCache.set(group, result);
  return result;
}

function project(data, p) {
  const { pa, range } = effectivePa(p.pa);
  if (pa == null) return { error: 'unknown potential' };
  const gap = Math.max(0, pa - p.ca);
  const share = realisation(p);
  const targets = { realistic: Math.round(p.ca + gap * share), ceiling: Math.round(pa) };
  const t = trends(data, positionGroup(p.best));
  const out = { name: p.name, age: p.age, ca: p.ca, pa: p.pa, paRange: range, position: p.best, realisation: share, targets, attributes: {} };
  if (!t || gap === 0) {
    for (const k of Object.keys(p.attributes)) out.attributes[k] = { now: p.attributes[k], realistic: p.attributes[k], ceiling: p.attributes[k] };
    out.note = gap === 0 ? 'already at potential' : 'not enough comparable players';
    return out;
  }
  const groupOf = {};
  for (const [g, keys] of Object.entries(GROUPS)) for (const k of keys) groupOf[k] = g;
  for (const k of Object.keys(p.attributes)) {
    const now = p.attributes[k];
    const row = { now };
    for (const [label, ca] of Object.entries(targets)) {
      const g = groupOf[k];
      // Hidden attributes and feet do not develop with CA in this model.
      const delta = g ? Math.max(0, t.slope[k]) * (ca - p.ca) * ageFactor(g, p.age) : 0;
      row[label] = Math.min(20, Math.round(now + delta));
    }
    out.attributes[k] = row;
  }
  out.peers = t.peers;
  return out;
}

module.exports = { GROUPS, ageFactor, effectivePa, realisation, project };
