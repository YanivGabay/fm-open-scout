// Field-order evidence from a parsed save, using statistics only (no names assumed):
//   node scripts/verify_fields.js players.json
// players.json comes from `fm_save_parser players <save> --json players.json`.
// Prints, for every raw attribute slot, the mean by position group and by age band,
// then reputation and personality checks. FORMAT.md §4.3 records what this shows.
const j = require(require('path').resolve(process.argv[2]));
const now = new Date(j.version.current_date || '2025-01-01');
const age = (p) => (now - new Date(p.date_of_birth)) / 3.15576e10;
const P = j.players.filter((p) => p.attributes_raw && p.date_of_birth && p.current_ability >= 80);
const GROUPS = ['GK', 'DC', 'FB', 'DM', 'MC', 'W', 'AMC', 'ST'];
const group = (p) => {
  const b = p.best_position;
  if (b === 'GK') return 'GK';
  if (b === 'DC' || b === 'SW') return 'DC';
  if (['DL', 'DR', 'WBL', 'WBR'].includes(b)) return 'FB';
  if (b === 'DM' || b === 'MC') return b;
  if (['ML', 'MR', 'AML', 'AMR'].includes(b)) return 'W';
  return b === 'AMC' ? 'AMC' : 'ST';
};
const mean = (xs) => xs.reduce((a, b) => a + b, 0) / Math.max(xs.length, 1);
const corr = (xs, ys) => {
  const mx = mean(xs), my = mean(ys);
  let sxy = 0, sx = 0, sy = 0;
  xs.forEach((x, i) => { sxy += (x - mx) * (ys[i] - my); sx += (x - mx) ** 2; sy += (ys[i] - my) ** 2; });
  return sxy / Math.sqrt(sx * sy);
};

console.log(`${P.length} players, in-game date ${j.version.current_date}\n`);
console.log('slot name               ' + GROUPS.map((g) => g.padStart(5)).join('') + '  top   age 17-21 / 24-28 / 31-36 (outfield, CA 120-140)');
const band = P.filter((p) => group(p) !== 'GK' && p.current_ability >= 120 && p.current_ability <= 140);
Object.keys(P[0].attributes_raw).forEach((name, slot) => {
  const byGroup = GROUPS.map((g) => mean(P.filter((p) => group(p) === g).map((p) => p.attributes_raw[name])));
  const top = GROUPS[byGroup.indexOf(Math.max(...byGroup))];
  const byAge = [[17, 21], [24, 28], [31, 36]].map(([lo, hi]) => mean(band.filter((p) => age(p) >= lo && age(p) < hi).map((p) => p.attributes_raw[name])));
  console.log(`${String(slot).padEnd(4)} ${name.padEnd(18)} ${byGroup.map((x) => x.toFixed(0).padStart(5)).join('')}  ${top.padEnd(5)} ${byAge.map((x) => x.toFixed(0)).join(' / ')}`);
});

const R = P.filter((p) => p.reputation);
console.log('\nreputation slots (mean): ' + ['home', 'current', 'world'].map((k) => `${k} ${mean(R.map((p) => p.reputation[k])).toFixed(0)}`).join(', '));
console.log(`share with slot 0 >= slot 2: ${(R.filter((p) => p.reputation.home >= p.reputation.world).length / R.length).toFixed(3)}`);

const Q = P.filter((p) => p.personality && p.attributes);
const ATTRS = ['determination', 'dirtiness', 'aggression', 'leadership', 'important_matches'];
console.log('\npersonality slot vs attribute correlation\n' + ''.padEnd(16) + ATTRS.map((a) => a.slice(0, 9).padStart(10)).join(''));
for (const k of Object.keys(Q[0].personality)) {
  console.log(k.padEnd(16) + ATTRS.map((a) => corr(Q.map((p) => p.personality[k]), Q.map((p) => p.attributes[a])).toFixed(2).padStart(10)).join(''));
}
