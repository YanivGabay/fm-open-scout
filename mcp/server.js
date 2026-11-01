#!/usr/bin/env node
// fm-open-scout MCP server: scouting tools over your own Football Manager saves.
// Saves are only read; parsed results are cached under ~/.cache/fm-open-scout.
const { McpServer } = require('@modelcontextprotocol/sdk/server/mcp.js');
const { StdioServerTransport } = require('@modelcontextprotocol/sdk/server/stdio.js');
const { z } = require('zod');
const scout = require('./scout');
const { project } = require('./project');
const { listSaves, loadSave } = require('./saves');
const pkg = require('../package.json');

const QUIET = process.env.FM_OPEN_SCOUT_QUIET === '1';
const DONATE = 'Enjoying fm-open-scout? buymeacoffee.com/yanivga (hide with FM_OPEN_SCOUT_QUIET=1)';

let current = null; // { data, path, modified }

const money = (v) => (v == null ? '-' : v >= 1e6 ? `£${(v / 1e6).toFixed(1)}m` : `£${Math.round(v / 1e3)}k`);
const price = (p) => (p.notForSale ? '£300m cap' : money(p.value));
const wage = (w) => (w == null ? '?' : `£${(w / 1000).toFixed(w < 10000 ? 1 : 0)}k`);
const year = (d) => (d ? d.slice(0, 4) : '?');
const local = (t) => { const d = new Date(t - t.getTimezoneOffset() * 60000); return d.toISOString().slice(0, 16).replace('T', ' '); };
const text = (s) => ({ content: [{ type: 'text', text: s }] });
const fail = (s) => ({ content: [{ type: 'text', text: s }], isError: true });

function table(headers, rows) {
  return [`| ${headers.join(' | ')} |`, `|${headers.map(() => '---').join('|')}|`, ...rows.map((r) => `| ${r.join(' | ')} |`)].join('\n');
}

function playerRow(data, p, role) {
  const idx = scout.priceIndex(data, p);
  const roles = scout.rolesOf(p).slice(0, 2).map((r) => `${r.role} ${r.score.toFixed(1)}`).join(', ');
  return [p.name, `${p.ca}/${p.pa}`, p.age == null ? '?' : Math.floor(p.age), p.best,
    role ? scout.roleScore(p, role).toFixed(1) : roles, p.club || (scout.isFreeAgent(p) ? 'free agent' : 'unknown club'), price(p),
    idx == null ? '-' : `x${idx.toFixed(2)}`, `${wage(p.wage)} to ${year(p.contractEnd)}`];
}
const PLAYER_HEADERS = (role) => ['Player', 'CA/PA', 'Age', 'Pos', role ? `${role} score` : 'Best roles', 'Club', 'Value', 'Price vs usual', 'Wage'];

// Loads the requested save, or the newest one; reloads when the file changed.
async function ensure(save) {
  const saves = listSaves();
  let target;
  if (save) {
    const q = scout.fold(save);
    target = saves.find((s) => s.path === save) || saves.find((s) => scout.fold(s.name) === q) || saves.find((s) => scout.fold(s.path).includes(q));
    if (!target && require('fs').existsSync(save)) target = { path: save, modified: require('fs').statSync(save).mtime };
    if (!target) throw new Error(`no save matching "${save}". Use list_saves.`);
  } else if (current) {
    target = saves.find((s) => s.path === current.path) || current;
  } else {
    target = saves[0];
    if (!target) throw new Error('no .fm saves found. Set FM_SAVE_DIRS or pass a save path.');
  }
  if (!current || current.path !== target.path || +current.modified !== +target.modified) {
    current = { data: await loadSave(target.path), path: target.path, modified: target.modified };
  }
  const newer = saves.find((s) => s.modified > current.modified && s.path !== current.path);
  return { data: current.data, note: newer ? `\n\nNote: a newer save exists: "${newer.name}" (${newer.game}, ${local(newer.modified)}). Use load_save to switch.` : '' };
}

function header(data) {
  const v = data.version;
  const resaved = v.saved_with && parseInt(v.saved_with, 10) > v.major ? ` (re-saved with ${v.saved_with})` : '';
  return `**${data.title}**: FM${v.major}${resaved}, in-game date ${v.current_date || '?'}, ${data.players.length} named players (layout ${data.layout.name}, ${data.layout.confidence}).`;
}

const server = new McpServer({ name: 'fm-open-scout', version: pkg.version });

const tool = (name, description, inputSchema, run) => server.registerTool(name, { description, inputSchema }, async (args) => {
  try { return await run(args || {}); } catch (e) { return fail(String(e.message || e)); }
});

const saveArg = z.string().optional().describe('Save name or path; default: the loaded save, or the newest save');

tool('list_saves', 'List Football Manager saves found on this machine, newest first.', {}, async () => {
  const saves = listSaves();
  if (!saves.length) return text('No .fm saves found. Set FM_SAVE_DIRS to your saves folder.');
  return text(table(['Save', 'Game', 'Modified', 'Size'], saves.map((s) => [s.name, s.game, local(s.modified), `${(s.size / 1e6).toFixed(0)} MB`])));
});

tool('load_save', 'Parse a save (the newest by default) and make it the current save. First load of a save takes 10-30 s; later loads use the cache.', { save: saveArg }, async ({ save }) => {
  const { data, note } = await ensure(save);
  const warn = data.warnings && data.warnings.length ? `\nWarnings: ${data.warnings.join('; ')}` : '';
  return text(`${header(data)}${warn}${note}${QUIET ? '' : `\n\n${DONATE}`}`);
});

const roleEnum = z.enum(Object.keys(scout.ROLES));
tool('search_players',
  'Search players with filters and ranking. Roles: GK, CB, FB, DM, CM, AM, W, ST (role score = attribute fit for the role, 1-20). '
  + 'Sort by ca, pa, role, value (cheapest first), bargain (lowest price relative to players of the same CA) or potential_gap (PA-CA). Values are the save\'s own estimate in GBP.',
  {
    save: saveArg,
    role: roleEnum.optional(),
    positions: z.array(z.enum(['GK', 'SW', 'DL', 'DC', 'DR', 'DM', 'ML', 'MC', 'MR', 'AML', 'AMC', 'AMR', 'ST', 'WBL', 'WBR'])).optional(),
    min_rating: z.number().int().min(1).max(20).optional().describe('Minimum position rating for role/positions (default 15; 13-14 = can be retrained)'),
    min_ca: z.number().int().optional(), max_ca: z.number().int().optional(), min_pa: z.number().int().optional(),
    min_age: z.number().optional(), max_age: z.number().optional(),
    max_value: z.number().optional().describe('GBP, e.g. 23000000'), min_value: z.number().optional(),
    max_wage: z.number().optional().describe('GBP per week'),
    max_price_index: z.number().optional().describe('Only players priced at most this share of the usual price for their CA (0.35 = undervalued)'),
    free_agents_only: z.boolean().optional(),
    club: z.string().optional(), exclude_club: z.string().optional(), nation: z.string().optional(),
    sort: z.enum(['ca', 'pa', 'role', 'value', 'bargain', 'potential_gap']).optional(),
    limit: z.number().int().min(1).max(100).optional(),
  },
  async (args) => {
    const { data, note } = await ensure(args.save);
    const r = scout.search(data, args);
    if (args.club && !r.club) return fail(`no club matching "${args.club}"`);
    return text(`${header(data)}\n${r.total} match, showing ${r.rows.length}.\n\n${table(PLAYER_HEADERS(args.role), r.rows.map((p) => playerRow(data, p, args.role)))}${note}`);
  });

function pick(data, name) {
  const found = scout.findPlayers(data, name);
  if (!found.length) throw new Error(`no player matching "${name}"`);
  return { p: found[0], others: found.slice(1, 6) };
}

const ATTR_GROUPS = {
  Technical: ['corners', 'crossing', 'dribbling', 'finishing', 'first_touch', 'free_kicks', 'heading', 'long_shots', 'long_throws', 'marking', 'passing', 'penalty_taking', 'tackling', 'technique'],
  Mental: ['aggression', 'anticipation', 'bravery', 'composure', 'concentration', 'decisions', 'determination', 'flair', 'leadership', 'off_the_ball', 'positioning', 'teamwork', 'vision', 'work_rate'],
  Physical: ['acceleration', 'agility', 'balance', 'jumping_reach', 'natural_fitness', 'pace', 'stamina', 'strength'],
  Goalkeeping: ['aerial_reach', 'command_of_area', 'communication', 'eccentricity', 'handling', 'kicking', 'one_on_ones', 'reflexes', 'rushing_out', 'tendency_to_punch', 'throwing'],
  Hidden: ['consistency', 'dirtiness', 'important_matches', 'injury_proneness', 'versatility'],
};
const label = (k) => k.replace(/_/g, ' ');

tool('player', 'Full profile of one player: attributes (1-20), hidden attributes, personality, positions, contract, role scores and price vs usual.', { name: z.string().describe('Player name (accents optional) or UID'), save: saveArg }, async ({ name, save }) => {
  const { data, note } = await ensure(save);
  const { p, others } = pick(data, name);
  const idx = scout.priceIndex(data, p);
  const groups = Object.entries(ATTR_GROUPS)
    .filter(([g]) => g !== 'Goalkeeping' || p.best === 'GK')
    .map(([g, keys]) => `**${g}:** ${keys.map((k) => `${label(k)} ${p.attributes[k]}`).join(', ')}`).join('\n');
  const pers = p.personality ? `**Personality (hidden):** ${Object.entries(p.personality).map(([k, v]) => `${k} ${v}`).join(', ')}` : '';
  const pos = Object.entries(p.positions).filter(([, v]) => v >= 10).sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k} ${v}`).join(', ');
  const roles = scout.rolesOf(p, 10).slice(0, 4).map((r) => `${r.role} ${r.score.toFixed(1)}`).join(', ');
  return text([
    `## ${p.name}`,
    `${p.age == null ? '?' : Math.floor(p.age)} years (born ${p.dob || '?'})${p.nation ? `, ${p.nation}` : ''}${p.height ? `, ${p.height} cm` : ''}, ${p.foot || '?'} foot`,
    `CA ${p.ca} / PA ${p.pa}. Club: ${p.club || (scout.isFreeAgent(p) ? 'free agent' : 'unknown club')}. Wage ${wage(p.wage)}/week to ${p.contractEnd || '?'}. Value ${p.notForSale ? 'at the £300m cap (very high or not for sale)' : money(p.value)}${idx == null ? '' : ` (x${idx.toFixed(2)} of the usual price for CA ${p.ca})`}.`,
    `Positions: ${pos}. Role scores: ${roles}.`,
    groups, pers,
    others.length ? `\nOther matches: ${others.map((o) => `${o.name} (${o.club || 'free'}, ${o.ca})`).join(', ')}` : '',
  ].filter(Boolean).join('\n') + note);
});

tool('squad', 'A club\'s squad sorted by CA, with best roles and contracts, plus its best XI by slot.', { club: z.string(), save: saveArg }, async ({ club, save }) => {
  const { data, note } = await ensure(save);
  const name = scout.findClubName(data, club);
  if (!name) return fail(`no club matching "${club}"`);
  const players = data.players.filter((p) => p.club === name).sort((a, b) => b.ca - a.ca);
  const xi = scout.bestXI(data, name);
  const slots = xi.slots.map((s) => `${s.role}: ${s.players.map((p) => `${p.name} ${p.ca}`).join(', ') || '(none rated 15+)'}`).join('\n');
  return text(`${header(data)}\n\n## ${name}: ${players.length} players, best XI average CA ${xi.average.toFixed(1)}\n${slots}\n\n${table(PLAYER_HEADERS(), players.map((p) => playerRow(data, p)))}${note}`);
});

tool('compare_squads', 'Compare clubs\' best XI slot by slot (CA) to find where a squad is weak.', { clubs: z.array(z.string()).min(1).max(12), save: saveArg }, async ({ clubs, save }) => {
  const { data, note } = await ensure(save);
  const rows = [];
  for (const c of clubs) {
    const name = scout.findClubName(data, c);
    if (!name) { rows.push([c, 'not found']); continue; }
    const xi = scout.bestXI(data, name);
    rows.push([name, xi.average.toFixed(1), ...xi.slots.map((s) => s.players.map((p) => p.ca).join('/') || '-')]);
  }
  return text(table(['Club', 'XI avg', ...scout.XI.map(([r, n]) => (n > 1 ? `${r} x${n}` : r))], rows) + note);
});

tool('similar_players', 'Find players similar to one player: mode "profile" matches CA, PA and age; mode "attributes" matches the attribute shape (playing style).', {
  name: z.string(), mode: z.enum(['profile', 'attributes']).optional(),
  min_ca: z.number().int().optional().describe('attributes mode default: the player\'s CA - 20'),
  max_value: z.number().optional(), max_age: z.number().optional(), exclude_club: z.string().optional(),
  limit: z.number().int().min(1).max(50).optional(), save: saveArg,
}, async (args) => {
  const { data, note } = await ensure(args.save);
  const { p } = pick(data, args.name);
  const rows = scout.similar(data, p, args);
  const headers = [...PLAYER_HEADERS(), ...(args.mode === 'attributes' ? ['Similarity'] : [])];
  return text(`Players like **${p.name}** (${p.ca}/${p.pa}, ${Math.floor(p.age)}, ${p.best}, ${p.club || 'free'}):\n\n${table(headers, rows.map((q) => [...playerRow(data, q), ...(args.mode === 'attributes' ? [q.similarity.toFixed(3)] : [])]))}${note}`);
});

tool('project_player',
  'Estimate how a player\'s attributes may develop: a realistic target (adjusted for determination, professionalism and age) and a ceiling (full PA). '
  + 'This is fm-open-scout\'s own estimate from players already at that level in the same position; FM does not store future attributes.',
  { name: z.string(), save: saveArg },
  async ({ name, save }) => {
    const { data, note } = await ensure(save);
    const { p } = pick(data, name);
    const r = project(data, p);
    if (r.error) return fail(r.error);
    const keys = Object.entries(ATTR_GROUPS).filter(([g]) => g !== 'Hidden' && (g !== 'Goalkeeping' || p.best === 'GK')).flatMap(([, k]) => k);
    const rows = keys.map((k) => { const a = r.attributes[k]; return [label(k), a.now, a.realistic === a.now ? a.now : `**${a.realistic}**`, a.ceiling === a.now ? a.now : `**${a.ceiling}**`]; });
    return text([
      `## ${p.name}: development estimate`,
      `CA ${r.ca} → realistic **${r.targets.realistic}** → ceiling **${r.targets.ceiling}**${r.paRange ? ` (PA is a range, ${r.paRange.join('-')})` : ''}. Likely to realise ${(r.realisation * 100).toFixed(0)}% of the gap (determination ${p.attributes.determination}${p.personality ? `, professionalism ${p.personality.professionalism}` : ''}, age ${Math.floor(p.age)}).`,
      r.note ? `Note: ${r.note}.` : `Based on how each attribute rises with CA across ${r.peers} players of the same position.`,
      '',
      table(['Attribute', 'Now', 'Realistic', 'Ceiling'], rows),
      '',
      '_Estimate only: actual development depends on training, match time, injuries and the game\'s randomness._',
    ].join('\n') + note);
  });

async function main() {
  await server.connect(new StdioServerTransport());
}

if (require.main === module) main().catch((e) => { console.error(e); process.exit(1); });

module.exports = { server, ensure };
