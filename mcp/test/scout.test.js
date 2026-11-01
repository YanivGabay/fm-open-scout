// Unit tests on synthetic players (no saves needed): node --test mcp/test
const test = require('node:test');
const assert = require('node:assert/strict');
const scout = require('../scout');
const { project, effectivePa, realisation, ageFactor } = require('../project');

const ATTRS = ['crossing', 'dribbling', 'finishing', 'heading', 'long_shots', 'marking', 'off_the_ball', 'passing', 'penalty_taking', 'tackling', 'vision',
  'handling', 'aerial_reach', 'command_of_area', 'communication', 'kicking', 'throwing', 'anticipation', 'decisions', 'one_on_ones', 'positioning', 'reflexes',
  'first_touch', 'technique', 'left_foot', 'right_foot', 'flair', 'corners', 'teamwork', 'work_rate', 'long_throws', 'eccentricity', 'rushing_out',
  'tendency_to_punch', 'acceleration', 'free_kicks', 'strength', 'stamina', 'pace', 'jumping_reach', 'leadership', 'dirtiness', 'balance', 'bravery',
  'consistency', 'aggression', 'agility', 'important_matches', 'injury_proneness', 'versatility', 'natural_fitness', 'determination', 'composure', 'concentration'];

let uid = 1;
function player(o = {}) {
  const level = o.level ?? 10;
  const attributes = Object.fromEntries(ATTRS.map((k) => [k, level]));
  Object.assign(attributes, o.attrs);
  return {
    uid: uid++, name: o.name || `Player ${uid}`, age: o.age ?? 24, ca: o.ca ?? 120, pa: o.pa ?? 140, club: o.club === undefined ? 'Club A' : o.club, clubShort: o.clubShort || null,
    value: o.value ?? 5e6, wage: o.free ? null : 20000, contractEnd: o.free ? null : '2028-06-30', best: o.best || 'DC', nation: o.nation || 'England',
    positions: { GK: 1, DC: 1, DL: 1, DR: 1, DM: 1, MC: 1, AMC: 1, ST: 1, ...o.positions }, attributes, personality: o.personality || null,
  };
}

test('role score weights key attributes double', () => {
  const p = player({ attrs: { tackling: 20 } });
  const base = scout.roleScore(player(), 'CB');
  assert.ok(scout.roleScore(p, 'CB') > base);
  assert.equal(scout.roleScore(p, 'ST'), scout.roleScore(player(), 'ST'));
});

test('search filters by role rating, age, value and sorts', () => {
  const data = { players: [
    player({ name: 'Cheap CB', positions: { DC: 18 }, ca: 140, value: 2e6, attrs: { tackling: 18 } }),
    player({ name: 'Dear CB', positions: { DC: 20 }, ca: 150, value: 60e6 }),
    player({ name: 'Old CB', positions: { DC: 20 }, ca: 145, age: 33 }),
    player({ name: 'Striker', positions: { ST: 20 }, ca: 160 }),
    player({ name: 'Free CB', positions: { DC: 16 }, ca: 130, club: null, value: 0, free: true }),
    player({ name: 'Unknown club CB', positions: { DC: 16 }, ca: 125, club: null, value: 9e6 }),
  ] };
  const r = scout.search(data, { role: 'CB', max_value: 23e6, max_age: 30 });
  assert.deepEqual(r.rows.map((p) => p.name), ['Cheap CB', 'Free CB', 'Unknown club CB']);
  assert.deepEqual(scout.search(data, { role: 'CB', sort: 'ca' }).rows.map((p) => p.name), ['Dear CB', 'Old CB', 'Cheap CB', 'Free CB', 'Unknown club CB']);
  // No club but a contract = an unresolved club, not a free agent.
  assert.deepEqual(scout.search(data, { free_agents_only: true }).rows.map((p) => p.name), ['Free CB']);
});

test('budget filters need a known price; capped values are not prices', () => {
  const raw = (name, v, club = 'Club A') => ({ name, uid: name.length, current_ability: 130, potential_ability: 140, attributes: { determination: 10 },
    valuation_unverified: v, club: club ? { name: club } : null, contract: club ? { weekly_wage: 1000, ends: '2028-06-30' } : null, positions: {} });
  const data = scout.slim({ version: {}, layout: {}, players: [raw('Priced', 3e6), raw('Unknown', null), raw('Capped', 3e8), raw('Free', null, null)] });
  assert.deepEqual(scout.search(data, { max_value: 5e6 }).rows.map((p) => p.name).sort(), ['Free', 'Priced']);
  const capped = data.players.find((p) => p.name === 'Capped');
  assert.equal(capped.value, null);
  assert.ok(capped.notForSale);
});

test('max_age includes the whole year of that age', () => {
  const data = { players: [player({ name: 'A', age: 25.9 }), player({ name: 'B', age: 26.0 })] };
  assert.deepEqual(scout.search(data, { max_age: 25 }).rows.map((p) => p.name), ['A']);
});

test('price index compares with players of the same CA', () => {
  const players = [5, 10, 10, 10, 20].map((m) => player({ ca: 140, value: m * 1e6 }));
  const data = { players };
  assert.equal(scout.priceIndex(data, players[0]), 0.5);
  assert.equal(scout.priceIndex(data, players[4]), 2);
  assert.equal(scout.search(data, { max_price_index: 0.6 }).rows.length, 1);
});

test('names match without accents; clubs by partial name', () => {
  const data = { players: [player({ name: 'Benjamin Šeško', club: 'Manchester United', clubShort: 'Man Utd' }), player({ name: 'Other', club: 'Manchester City' })] };
  assert.equal(scout.findClubName(data, 'man utd'), 'Manchester United');
  assert.equal(scout.findPlayers(data, 'benjamin sesko')[0].name, 'Benjamin Šeško');
  assert.equal(scout.findClubName(data, 'manchester united'), 'Manchester United');
  assert.equal(scout.findClubName(data, 'city'), 'Manchester City');
});

test('best XI uses each player once', () => {
  const both = player({ name: 'Both', positions: { DC: 20, DL: 20 }, ca: 160 });
  const data = { players: [both, player({ name: 'CB2', positions: { DC: 18 }, ca: 140 })] };
  const xi = scout.bestXI(data, 'Club A');
  const all = xi.slots.flatMap((s) => s.players.map((p) => p.name));
  assert.equal(all.filter((n) => n === 'Both').length, 1);
});

test('best XI keeps a natural striker up front', () => {
  const data = { players: [
    player({ name: 'Winger', positions: { AML: 20, ST: 15 }, ca: 136 }),
    player({ name: 'Striker', positions: { ST: 20, AML: 15 }, ca: 127 }),
    player({ name: 'Winger 2', positions: { AMR: 18 }, ca: 125 }),
    player({ name: 'Winger 3', positions: { AMR: 18 }, ca: 124 }),
  ] };
  const xi = scout.bestXI(data, 'Club A');
  assert.deepEqual(xi.slots.find((s) => s.role === 'ST').players.map((p) => p.name), ['Striker']);
  assert.deepEqual(xi.slots.find((s) => s.role === 'W').players.map((p) => p.name), ['Winger', 'Winger 2']);
});

test('similar by attributes ranks the same style first, among similar levels', () => {
  const ref = player({ name: 'Ref', best: 'ST', ca: 150, attrs: { finishing: 18, pace: 17 } });
  const twin = player({ name: 'Twin', best: 'ST', ca: 145, attrs: { finishing: 17, pace: 16 } });
  const other = player({ name: 'Other', best: 'DC', ca: 150, attrs: { tackling: 18, marking: 18 } });
  const weak = player({ name: 'Weak twin', best: 'ST', ca: 90, level: 5, attrs: { finishing: 13, pace: 12 } });
  const data = { players: [ref, other, weak, twin] };
  const r = scout.similar(data, ref, { mode: 'attributes' });
  assert.deepEqual(r.map((p) => p.name), ['Twin', 'Other']);
});

test('range PA uses its midpoint', () => {
  assert.deepEqual(effectivePa(-9), { pa: 165, range: [150, 180] });
  assert.equal(effectivePa(170).pa, 170);
});

test('determination and age drive realisation', () => {
  const keen = player({ age: 18, attrs: { determination: 18 }, personality: { professionalism: 18 } });
  const lazy = player({ age: 18, attrs: { determination: 3 }, personality: { professionalism: 5 } });
  assert.ok(realisation(keen) > 0.9);
  assert.ok(realisation(lazy) < 0.4);
  assert.equal(ageFactor('physical', 29), 0);
  assert.equal(ageFactor('mental', 29), 1);
});

test('projection grows attributes towards stronger peers, never down', () => {
  const players = [];
  for (let i = 0; i < 40; i++) players.push(player({ best: 'ST', ca: 120, level: 10 }));
  for (let i = 0; i < 40; i++) players.push(player({ best: 'ST', ca: 160, level: 15 }));
  const kid = player({ name: 'Kid', best: 'ST', ca: 120, pa: 160, age: 18, level: 10, attrs: { determination: 20 }, personality: { professionalism: 20 } });
  players.push(kid);
  const r = project({ players }, kid);
  assert.equal(r.targets.ceiling, 160);
  assert.equal(r.attributes.finishing.ceiling, 15); // +5 over 40 CA, as the peers show
  assert.equal(r.attributes.consistency.ceiling, 10); // hidden: unchanged
  // A target above every peer extends the trend instead of stopping at the peers.
  const star = player({ best: 'ST', ca: 120, pa: 184, age: 18, level: 10, attrs: { determination: 20 } });
  assert.equal(project({ players }, star).attributes.finishing.ceiling, 18);
  const vet = player({ best: 'ST', ca: 120, pa: 160, age: 30, level: 10 });
  const v = project({ players }, vet);
  assert.equal(v.attributes.pace.ceiling, 10); // physical does not grow at 30
  assert.ok(v.attributes.decisions.ceiling > 10);
});
