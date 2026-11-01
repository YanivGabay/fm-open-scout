# Football Manager `.fm` save format

Part of **fm-open-scout**, an independent, open-source project. It is not affiliated
with or endorsed by Sports Interactive or SEGA. Football Manager is their trademark.

This file documents the save format as reverse-engineered for interoperability,
meaning reading your own saves. Everything here comes from the saves themselves:
byte layouts, statistics over tens of thousands of players, and known real players
as ground truth. The parsers are `fm_save_parser/` (Rust, the
reference implementation) and `lib/fmsave.js` (JavaScript).

**Confidence labels**
- **verified**: checked byte by byte on local saves, with known real players as ground truth.
- **inferred**: backed by evidence (statistics, other versions), but not checked field by field.
- **unknown**: not known.

**Local evidence**

| Version | Save | Build | DB schema | DB tag |
|---|---|---|---|---|
| FM24 | a Crystal Palace career (210 MB) | 24.3.0+0 | 3569 | `2430` |
| FM26 | 4 saves of one career | 26.0.0+0 | 3717 | `2600` |
| FM27 | a Man Utd career | 27.0.0+0 (saved with 27.0.0+2486695) | 3753 | `2700` |
| FM24 → FM27 | a Chelsea career started in FM24 and re-saved by FM27 | 24.3.0+0 (saved with 27.0.0+2486695) | 3569 | `2430` |

There are no local saves for FM20–FM23. FM25 was never released.

**Prior work**

FootballSaveScout, a community save reader, scans heuristically for the `01 00 6c 07`
marker and fixed offsets. Its model was re-checked against the saves here, and several
points were corrected (§6). No code from it or from any other tool is used.

## 1. Container (verified, FM24 + FM26)

The file is a small virtual filesystem. Each file is stored as one zstd frame.

```
off   size  field
0x00  9     signature 02 01 66 6d 66 2e 08 00 00   ("fmf." = ".fmf" reversed)
0x09  u64   L: directory block starts at 9 + L
0x11  u64   17: header length after the signature (data area starts at 9 + 17 = 26)
0x19  u8    3   (constant in all saves; probably the compression id = zstd) [inferred]
0x1a  ...   data area: one zstd frame per file, packed with no gaps
9+L   9     signature again
9+L+9 ...   one zstd frame with the directory tree, running to end of file
```

All integers are little-endian. A `str` is a `u32` byte length followed by UTF-8.

**Directory tree** (recursive):

```
node  = str name, u32 n_files, n_files × entry, u32 n_subdirs, n_subdirs × node
entry = str name, str ext, u64 offset (relative to data start 26),
        u64 compressed_size, u64 uncompressed_size, i64 created, i64 modified
```

- Timestamps are Unix seconds. `-62135596800` (0001-01-01) means unset.
- The root node's name is the save title, e.g. "Manager Name - Crystal Palace".

**Verification**
- The directory decodes with 0 bytes left over.
- The entries tile the data area exactly, with no gaps: FM24 has 1334 entries ending at `9+L`; FM26 has 1253.
- Every entry starts with the zstd magic `28 b5 2f fd`.

**Contents**
- Root: `game_info.dat`, `memory_pools.dat`, `save_game_summary.dat`, `game_db.dat` (FM24: 234 MB uncompressed), `pl_hist_ls.dat`, `news.dat`, `transfer_man.dat`, and so on.
- Subdirectory `rgman/`: rule groups (`.dat`), plus `.cmt`, `.apm` and `.tsm` files.

The older approach of scanning for the zstd magic happened to work, because the frames are contiguous. It cannot name files, though, and it treated the directory frame as data.

## 2. File chunk header (verified)

Every decompressed file begins with an 8-byte chunk header:

```
03 01, 4 bytes = extension reversed ("tad." = .dat, "tmc." = .cmt, "mpa." = .apm, "mst." = .tsm), u16 format version
```

| file | FM24 version | FM26 version |
|---|---|---|
| `game_info.dat` | 0x2a | 0x2e |
| `game_db.dat` | 3569 | 4000 |

The trailing directory frame has no chunk header.

## 3. Version detection (verified)

1. **`game_info.dat`**: after the chunk header comes a `u32` length and the build string, e.g. `"24.3.0+0"` or `"26.0.0+0"`. The major version is the number before the first dot.
2. **`game_db.dat`**: the `"DB"` marker sits a few bytes after the chunk header (FM24 at +10, FM26 at +14).
   - It is followed by `u16 0x0f` and the **u16 DB schema version** (FM24 3569, FM26 3717).
   - FM26 then inserts an extra `u16` (6).
   - Next comes a `u32 4` + 4-digit tag (`"2430"`, `"2600"`); FM24 follows it with `"K2"`, FM26 with `" 5"`.
   - The first two digits of the tag are the fallback major version.
   - Some FM24 saves (2 of 14 local FM24 saves, both last saved with 24.4.2) have `u32 8` where `"DB" u16 0x0f` normally is. There the schema version is the `u16` just before the tag, and the rest of the header is unchanged.

3. **In-game current date** (verified FM24, inferred FM26/FM27): after the tag string come `u16, u16 year, u32`, then (FM27 only) 8 more bytes, then `u16 year, u16, date`. So the date is 16 bytes after the tag string in FM24/FM26 (`game_db` +36 / +42) and 24 bytes after it in FM27 (+50). The parser picks the gap (8 or 16) at which the second year is within 3 of the first.
   - FM27: 2025-12-31 (Man Utd save), the same value as in `save_game_summary.dat`; the latest contract start in the save is 2025-12-29.
   - FM24: 2025-08-05. The latest "joined club" date of any player is 2025-08-04, and `save_game_summary.dat` holds the same date twice.
   - FM26: 2025-08-31, also found twice in `save_game_summary.dat`. The value increases across the four local saves in their write order (08-17 → 08-24 → 08-24 → 08-31). However, 1 253 players have a joined-club date after it (up to 2025-10-15). Those are most likely pre-agreed future moves, but that is unconfirmed.
4. **Last-saved-with build**: `save_game_summary.dat` holds a second build string, the build that last wrote the save (FM24 `"24.4.2"`, FM26 `"26.1.0+2245540"`). `game_info.dat` holds the creating build.

The Rust code is in `version.rs`. Dispatch is `layout::layout_for(major)`. An unregistered major version falls back to the statistical probe (`probe.rs`, §9).

## 4. `game_db.dat`: database

`game_db.dat` is one serialised object graph without a schema. These areas were located (FM24 offsets):

| range | content |
|---|---|
| ~2.7–16.5 MB | clubs: 33 842 objects (FM26: 36 147 at 2.9–12.4 MB), see §4.4 |
| ~23.66 MB | nations: 251 objects (FM26: 21.0 MB), see §4.7 |
| 80.9–95.2 MB | name tables (§4.1) |
| ~96–235 MB | people: players, non-players, histories |

### 4.1 Name tables (verified)

There are three tables back to back: first names, surnames, common names. Each has the form:

```
u32 count, count × (u32 id, u32 len, UTF-8 bytes)      with id == index (0..count-1)
```

| | first names | surnames | common names | starts at |
|---|---|---|---|---|
| FM24 | 247 847 | 543 952 | 85 879 | 80 938 953 |
| FM26 | 292 513 | 597 008 | 92 864 | 79 207 566 |

- Strings are not de-duplicated: "Kevin" appears about 200 times.
- The parser locates the tables structurally: a count ≥ 1000, then (id 0, str)(id 1, …), and the full table must parse, followed by two more tables.
- FootballSaveScout read these tables one field out of phase, as `len, name, id`. That is why its lookups needed `ref + 1`. With the correct entry framing, ids map directly.

### 4.2 Player ability block (verified FM24/FM26)

Offsets are relative to **r**, the position of the Current Ability field.

| offset | type | field | confidence |
|---|---|---|---|
| r-19 | u32 | row index (sequential per person table) | inferred |
| r-15 | u32 | person UID | verified (Haaland 29179063, Mbappé 85138874; same UID in FM24 and FM26) |
| r-11 | u32 | person UID again; **scan anchor** | verified |
| r-7 | u8 | 2 in every record | unknown |
| r-6 | u16×3 | reputations 0..10000: home, current, world | inferred: the third is world (lowest for 99.8 % of players, mean 2 453 vs ~4 800); the order of the first two (home, current) is not separable by statistics |
| r+0 | u16 | Current Ability 1..200 | verified |
| r+2 | i16 | Potential Ability (negative = range PA; none present in these played saves) | verified |
| r+4 | header | FM24 19 bytes, FM26 20 bytes (FM26 inserts 1 byte at r+9) | verified (length) |
| r+11 / r+12 | u16 | international team id: the team id of the national squad the player is in (FFFF = none) | verified as national-squad membership (exactly 23 ENG players carry 621, the id of the "England" team record; FRA 625, NOR 642) |
| r+15 / r+16 | u16 | **team id** of the player's club team (FFFF = none); resolved to a club via §4.4 | verified (exact mapping, §4.4) |
| p = 4+hdr | u8×15 | position ratings 1..20: GK SW DL DC DR DM ML MC MR AML AMC AMR ST WBL WBR | verified (Haaland: ST 20 only; Mbappé: AML/ST) |
| a = p+15 | u8×54 | attributes, stored on a 1..100 scale (display ≈ raw/5) | verified, see §4.3 |
| a+54 | u32 | GBP amount, exposed as `valuation_unverified`. It correlates with CA (r = 0.68 FM24) and is capped at 300 000 000. FM26 values look like market values (Yamal 265.7m, Haaland 181.4m; women's players around 1–2m). But FM24 has CA-105 players at the cap, so it may be an asking price instead. Two special values: `FFFFFFFF` means no value (≈5 % of FM26/FM27 players, nearly all low-CA free agents; reported as null), and exactly 300 000 000 is shared by stars (Yamal, Kane, Messi) and ~1 000 contracted CA 80–100 players, so it reads as "very high or not for sale" rather than a price. | unknown meaning |
| a+58 | u8 | unknown | unknown |
| a+59 | date | date joined current club | verified (Haaland 2022-07-01; Mbappé FM26 2024-07-01) |
| a+63 | date | second date: null in ≥98 % of records; when set, it holds one save-wide date (FM24 2023-11-17, FM26 2025-08-25) | unknown meaning |
| t = a+67 | tail | t+12/t+13: CA/PA byte copies; FM24 t+15 = weight kg, t+17 = height cm; FM26 t+15 = height (no weight byte) | verified (Haaland 195 cm / 94 kg, Courtois 200 cm) |

**Dates.** A date is a `u16` day field plus a `u16` year.
- The low 9 bits of the day field are the 1-based day of the year. The upper 7 bits are flags, set on many "joined" dates.
- `01 00 6c 07` (day 1, year 1900) is the null date. FootballSaveScout's "tail marker" is just this null date.
- `02 00 6c 07` (day 2, year 1900) is the end date of every non-contract deal (amateurs on no wage, ~9 % of FM27 players); it is reported as no end date.

**Scan.** The parser checks every offset `u` where `u32[u] == u32[u+4]` (UID not 0 or FFFFFFFF). For that anchor, all of the following must hold:
- CA 1..200 and PA -10..200
- all 15 positions in 1..20
- all 54 attributes in 1..100
- both date slots are null or plausible

Results:

| | null-marker scan (FootballSaveScout style) | UID-anchored scan |
|---|---|---|
| FM24 players | 87 148 | **88 297** |
| FM26 players | 107 979 | **109 782** |

The extra players are the ones whose second date is set. Probing every header length from 12 to 32 recovers exactly 19 (FM24) and 20 (FM26), so the anchor is not tuned to one layout. The full probe reproduces every other offset too (§9).

### 4.3 Attribute order (54 bytes)

```
 0 crossing        1 dribbling       2 finishing       3 heading         4 long_shots     5 marking
 6 off_the_ball    7 passing         8 penalty_taking  9 tackling       10 vision        11 handling
12 aerial_reach   13 command_of_area 14 communication 15 kicking        16 throwing      17 anticipation
18 decisions      19 one_on_ones    20 positioning    21 reflexes       22 first_touch   23 technique
24 left_foot      25 right_foot     26 flair          27 corners        28 teamwork      29 work_rate
30 long_throws    31 eccentricity   32 rushing_out    33 tendency_to_punch 34 acceleration 35 free_kicks
36 strength       37 stamina        38 pace           39 jumping_reach  40 leadership    41 dirtiness
42 balance        43 bravery        44 consistency    45 aggression     46 agility       47 important_matches
48 injury_proneness 49 versatility  50 natural_fitness 51 determination 52 composure     53 concentration
```

**Evidence.** `scripts/verify_fields.js` reproduces every statistic below from a parsed save (numbers: FM27, 85 k players; FM24 and FM26 agree).

- **Keeper slots (verified):** slots 11–16, 19, 21 and 31–33 average 37–64 (raw) for keepers and about 10 for every outfield group. No other slot behaves like this.
- **Slots that peak in one position group (verified):**

  | slot | top group | reading |
  |---|---|---|
  | 0 | full-backs, wingers | crossing |
  | 2, 6, 8 | strikers | finishing, off the ball, penalty taking |
  | 3, 5, 9, 20 | centre-backs | heading, marking, tackling, positioning |
  | 7, 10, 22, 23, 26 | central/attacking midfield | passing, vision, first touch, technique, flair |
  | 30 | full-backs | long throws |
  | 36, 39 | centre-backs (and keepers for 39) | strength, jumping reach |
  | 24 / 25 | full-backs vs the rest | left foot / right foot (one of the two is ~20 for almost every player; slot 25 is the high one for ~80 %) |

- **Age curves (verified, outfield CA 120–140):** slots 34, 38, 46 fall from ~68 at 17–21 to ~57–60 at 31–36 (acceleration, pace, agility: physical decline); slot 40 rises 42 → 61 (leadership); slot 48 rises 39 → 48 (injury proneness); slots 17, 18, 53 rise (anticipation, decisions, concentration).
- **Pairs statistics cannot separate:** pace / acceleration, anticipation / decisions, first touch / technique, teamwork / work rate, corners / free kicks, composure / concentration, and the order of the keeper slots among themselves. These are pinned by in-game ground truth: one known player per pair whose two values differ (see the verification table in §9).
- **Verified on saves:**
  - Haaland: left foot 20, right foot 11, finishing 18, pace 19, determination 20, GK attributes ≤ 3.
  - Courtois: handling, reflexes, one-on-ones and aerial reach ≥ 14; finishing ≤ 5.
- **Statistics:** FootballSaveScout labels slots 11/14/19/33 as 1v1/TTP/handling/communication. Correlations over all keepers settle slots 14 and 33:

  | slot | correlation with CA | correlation with command of area | reading |
  |---|---|---|---|
  | 14 | 0.44 | 0.31 | communication |
  | 33 | 0.12 | -0.02 | tendency to punch: a preference, not an ability |

  Samples: 1 445 keepers (FM24), 2 328 (FM26). Slots 11 vs 19 (handling vs one-on-ones) fit Courtois (handling 16, 1v1 19) but are **inferred** until checked against an in-game keeper profile.

**Display value:** `round(raw / 5)`, clamped to 1..20. Inferred: raw values are multiples of 5 in fresh databases; the exact rounding of in-between values is not confirmed.

### 4.4 Clubs and the team-id mapping (verified FM24 + FM26)

The player's u16 "club id" is a **team id**, an index into FM's team list. Club objects carry their own team ids explicitly, so the mapping is **read, not approximated**. Each club object ends with the list of team ids it owns, first team first. Example: Man City `[540, 35767, 35768]` = first team, then two youth teams.

**Club object** (`clubs.rs`):

```
u32 E (sequential index), u32 UID, u32 UID,
u8 kind (0 = club, 0x0C = nameless stub),
4 × i32 nation fields (each -1 or a nation index; e.g. Newport [139, -1, 175, 139]),
... str long name, str short name,
--- "basic" clubs: fixed section, name_end + 55 = tail ---
type u8, 00 00, FFFFFFFF, 00 00 00, u32, u32, 3 dates, u32 0, u8 0, 5 × f32,
--- tail ---
u8 m, m × dated entry,
[00], u8 n, n × reference (01, type, UID, UID, u16),
u32, u32, u8 flag (0|1), u8 count, count × u32 team id
```

| | FM24 | FM26 |
|---|---|---|
| dated entry | 24 B, first byte 01 | 25 B, first byte 02 |
| club objects | 33 842 | 36 147 |
| team ids | 75 334 | 78 447 |

Each dated entry is `marker, type, date, date, …`.

**Reading the tail**
- **Basic clubs** (~95 %) are read at the fixed offset.
- **Full clubs** (playable leagues) have a long variable section, so the tail is searched for. A candidate must be preceded by a plausible float, and without dated entries it needs at least one reference.
- **Order rules** (first-team ids strictly increase with E):
  - When the grammar matches at several positions, the candidate is kept only if exactly one fits the neighbours' id order (*OrderChecked*).
  - When the id gap to the neighbours equals the number of clubs between them, those ids are fixed, and a structural candidate must carry exactly that id (*Pinned*).
- **Safety net:** a structural match that breaks the order or reuses a team id is dropped.

| | structural | order-checked | pinned | unresolved clubs | players with a club id resolved |
|---|---|---|---|---|---|
| FM24 | 32 943 | 623 | 11 | 265 | **99.84 %** |
| FM26 | 35 090 | 518 | 34 | 505 | **99.10 %** |

Across all resolved lists, no team id appears twice, and first-team ids are strictly increasing in E order.

**Ground truth**

| save | player | resolved club |
|---|---|---|
| FM24 | Haaland, Rodri, Foden | Manchester City |
| FM24 | Mbappé, Vinícius, Bellingham | R. Madrid |
| FM24 | Messi | Inter Miami CF |
| FM24 | Lamine Yamal | Barcelona |
| FM24 | Gyökeres, Eze | Arsenal (2025 moves in the updated database) |
| FM24 | Guéhi | Crystal Palace (the human manager's club) |
| FM26 | Bonmatí, Graham Hansen | Barcelona: the women's club, a **separate club object**, UID 2000167448, vs. 1707 for the men's |
| FM26 | Kerr, James | Chelsea |
| FM26 | Shaw, Hemp | Manchester City |
| FM26 | Kane | FC Bayern München |

The mapping is cross-checked against national-team ids in match-file team records (§4.7). Those ids fall exactly into gaps that no club list uses.

**Team slots.** Slot 0 is the first team. Higher slots are the club's other teams. Median player age by slot (FM24):

| slot | median age | inferred meaning |
|---|---|---|
| 0 | 26 | first team |
| 1 | 19 | reserve/U21–U23 |
| 2 | 17 | U18–U19 |

The team type itself is **not decoded**, so the parser outputs only the slot number.

**What the old hypothesis got wrong.** Team ids are *not* the UID rank. Other clubs' second teams sit between them in the id order: Austrian "1b" teams, e.g. id 12857 owned by club 3506 sitting between Wolfurt (12856) and the next club (12858). National teams are also interleaved.

**Unresolved:** 73 FM26 ids (scattered small and women's clubs) and some full clubs whose tail grammar matches ambiguously. Their players keep the raw `club_id` and get no name.

### 4.5 Person name block (verified)

The block follows the player block. Its distance varies: median ~800 B (FM24) / ~680 B (FM26), 99th percentile ~1.7 KB / 1.3 KB, maximum ~9 KB / 14 KB.

```
q+0   u32  first-name id      q+4  u8 0
q+5   u32  surname id         q+9  u8 0
q+10  u32  common-name id or FFFFFFFF      q+14 u8 0
q+15  u32  len, then full legal name UTF-8 (may be empty)  e.g. "Erling Braut Haaland"
after       date of birth (day field + year)                e.g. 203/2000 = 2000-07-21
```

Offsets relative to `after`:

| | FM24 | FM26 | confidence |
|---|---|---|---|
| bytes +4..+8 | 5 bytes | same 5 bytes | unknown |
| inserted bytes | — | +9..+12 (`00 00 00 03` typical) | unknown |
| u16 nationality index | +9 | +13 | verified (Haaland 160 NOR, Kane 139 ENG, Salah 11 EGY, Mbappé 143 FRA) |
| personality, 8 bytes on 1..20 scale (not ×5) | +17 | +21 | inferred: offsets verified (values identical between FM24 and FM26 for Haaland), order inferred |

Personality order: adaptability, ambition, loyalty, pressure, professionalism, sportsmanship, temperament, controversy.
- **Evidence (inferred):** correlations with attributes are weak but consistent with this order across FM27's 85 k players: professionalism and ambition correlate most with determination (0.26, 0.24); sportsmanship correlates negatively with dirtiness (−0.17); temperament negatively with aggression (−0.14); pressure positively with important matches (0.14).
- These values are hidden in the game UI, so field-level ground truth needs FM's own pre-game editor, which shows them for database players.

**Validation used:**
- ids are inside their tables
- the separator bytes are 0
- the date of birth is real and not null
- the nationality index is < 512
- reject first = surname = 0 and common = 0. Zero-filled regions inside person objects otherwise produced about 14 k false "DJ Buffonge" (FM24) / "Stiven Rua" (FM26) matches; these are entry 0 of the common-name table.

- reject a nationality index outside the save's nation table, and a birth date after the in-game date. Some person objects contain byte runs that pass every other check; before this rule they gave 846 players of an FM27 save (67 in FM24) a stranger's name, always with nation index ≥ 251 and a birth year such as 2048. With it, all of them get their real name block.

- single-name players (mostly Brazilian and Spanish newgens) store the first-name id in the common-name field, an id beyond the common-name table. Such a block is accepted, with the first name as the common name, only when the search range holds no ordinary block, because a neighbouring newgen's single-name block can sit before a player's own block. Before this rule 1–4 % of players were unnamed, growing with career length (2 803 in an FM24 save in 2027); with it, 17. No previously named player changes name.

**Coverage:** FM24 Crystal Palace 13 unnamed of 88 297 (was 1 577), FM24 Sporting CP (season 2027) 17 of 72 379 (was 2 803), FM26 2 of 109 782 (was 472), FM27 1 of 81 789 (was 230).

### 4.6 Contracts, wages, transfers (`contracts.rs`)

All money is **GBP**: Bellingham's €103m fee is stored as 88 318 856.

**Current contract** (verified FM24 + FM26):

```
k+0   FF×8
k+8   date  contract end
k+12  date  contract start
k+16  6 bytes
k+22  u32 n = clause count
k+26  n × 29-byte clause entries (first byte 07; content not decoded)
...   20 bytes
w     date, u8 kind, u32 ?, u32 team id, u32 0, u32 weekly wage, 01, u8, 00, FF FF FF ..
```

- **Anchor:** the wage pattern `w`. Its team id must be the player's team, or another team of the same club (youth players are contracted to the club).
- **Header:** accepted only at the one offset where the clause count and the 07 markers line up.
- **`kind`:** 0, 1 or 2. The value 2 shows up on loans, e.g. FM26 Nicolas Jackson at Bayern (2025-07-07 → 2026-06-30) and El Bilal Touré. It also shows up on non-loans such as FM26 Bellingham, so its meaning is **unknown**.

Verified examples:

| player | contract | weekly wage |
|---|---|---|
| Haaland | 2022-07-01 → 2034-06-30 | FM24 £340 000, FM26 £450 000 |
| Rodri | starts 2019-07-04 (his real signing date) | |
| Kane (FM26) | starts 2023-08-12 (real signing date) | |
| Mbappé (FM26) | 2024-07-01 → 2029-06-30 | |
| Ødegaard | starts 2021-08-20 | |
| Camavinga | starts 2021-08-31 | |
| Lamine Yamal | 2014-07-01 (joined La Masia) → 2031-06-30 | |

**Coverage** (players with a club id):

| | contract found | of which with dates | CA ≥ 160 |
|---|---|---|---|
| FM24 | 61 % | 41 735 | 95 % |
| FM26 | 63 % | 55 190 | 95 % |

Lower-league players in non-playable nations mostly have no contract structure of this form. Clause content, bonuses and release clauses are **not decoded**.

**Last transfer** (verified FM24 and FM27 against public fees; FM26 has the same fields):

```
date, date, date   (all null in observed records)
                   FM26/FM27 insert one byte after the first date
                   FM27 inserts two bytes here (FF, u8)
str                (u32 length + UTF-8; FM27 sometimes fills it, e.g. "Bellingham")
u32 ?              (unknown; −1 in FM26)
i32 fee            (−1 = none: academy, free, unknown)
u32 wage at signing
01
```

The search is restricted to the region before the person's name block.

| player | stored fee | public fee |
|---|---|---|
| Rice | 100 000 000 | £100m+ |
| Caicedo | 100 000 000 | £100m+ |
| Bellingham | 88 318 856 | €103m ≈ £88.5m |
| Kane | 85 746 464 | £86.4m |
| Gvardiol | 77 171 816 | £77.6m |
| Haaland | 51 000 000 | £51.2m |
| Ødegaard | 30 079 556 | £30m |
| Guéhi | 18 000 000 | £18m |
| Saka, Foden (academy) | −1 | — |

- The signing wage is often lower than the current wage. For example, Rice signed at £250 000 and now earns £292 805 in FM24, two years later. This fits yearly wage rises, which is **inferred**.
- Coverage: FM24 63 056 records, FM26 83 410. Before the string was read as a string (it was matched as `u32 0`), 5 335 FM24 / 8 700 FM26 records with a non-empty string were missed, among them Salah £34.3m, Van Dijk £75m, Alisson £63.5m and Rúben Dias £65m.
- FM27 (Man Utd save): 57 005 records. Bellingham 89 580 488, Cunha £62.5m, Mbeumo £65m, Šeško 65 520 980; all match public fees.

**Value:** FM appears to compute market value at runtime. The only stored candidate is `valuation_unverified` (§4.2, a+54).

### 4.7 Nations (`nations.rs`)

**Nation table** (verified FM24 + FM26):
- Objects are `u32 index, u32 UID, u32 UID, …`, with indexes 0..250 (251 nations); FM26 objects are up to 17 KB apart.
- It is located from the England anchor, index 139 / UID 765. FM UIDs are database constants and were observed in both versions.
- Player nationality and club nation fields are indexes into this table.

**Nation names are not stored in `game_db.dat`** (verified). Exact length-prefixed searches for "Norway", "Netherlands" and the like find only people. FM reads names from its own database files.

Names do occur inside **national-team records** in match/scout files (`.apm`, `.scm`, …):

```
02, u32 team id, 02, u32 UID, 3 × 00, u32 nation index, u32 UID, ...
str name, str short name, 3 bytes, u32 UID
```

- A nation name is accepted only when (index, UID) matches the nation table.
- Team names are kept for every self-consistent record, giving `international_team_name`.

| | nations named from the save |
|---|---|
| FM24 | 24 of 251 (England, France, Spain, Norway, USA, Mexico, …) |
| FM26 (career save with no international matches) | 0 |

Youth national teams ("England Under 19s") use a different record shape and are not parsed.

**Other names:** `--nation-names <file>` adds names from a `uid,name` file keyed by FM's stable nation UID. Each name carries its source (`Save` or `UserFile`), and names from the save win.

## 5. Per-version differences

| item | FM24 | FM26 | FM27 |
|---|---|---|---|
| build string / DB tag / schema | 24.3.0+0 / 2430 / 3569 | 26.0.0+0 / 2600 / 3717 | 27.0.0+0 / 2700 / 3753 |
| `game_db.dat` chunk version | 3569 | 4000 (4 extra header bytes before `"DB"`) | 4000 |
| ability header length | 19 | 20 (+1 byte at r+9) | 20 |
| positions / attributes / second date | r+23 / r+38 / r+101 | r+24 / r+39 / r+102 | as FM26 |
| club id / international team id | r+15 / r+11 | r+16 / r+12 | r+16 / r+12 |
| weight / height (from tail) | +15 / +17 | none / +15 | none / +15 |
| nationality / personality (from `after`) | +9 / +17 | +13 / +21 | +14 / +22 |
| women's football | no | yes (UIDs ≥ 2 000 000 000 are common for women's players; women's clubs are separate club objects) | not checked |
| club dated-entry shape | 24 B, marker 01 | 25 B, marker 02 | as FM26 |
| last-transfer record | 3 dates contiguous | 1 extra byte after the first date | as FM26, plus 2 bytes before the string |
| in-game date | `game_db` +36 | `game_db` +42 (tag + 16) | `game_db` +50 (tag + 24) |
| nation objects | ≤ 4 KB apart | up to 17 KB apart | not measured |

**FM27 verification:** Bruno Fernandes (born 1994-09-08, nation UID 788 Portugal, Man Utd, contract to 2027-06-30, £300k/week); Chelsea players' nationalities (Caicedo Ecuador, Enzo Argentina, Cucurella Spain, Lavia Belgium, Hato Netherlands); Kane, Saka and Palmer share one international team id, Mbappé has another; transfer fees as in §4.6. The FM27 probe reproduces every probed offset (header 20, club 16, nationality 14, personality 22, height 15, no weight), and with the FM27 layout or the probe the players, clubs and contracts are identical.

**Re-saved careers:** FM27 converts an FM24 career when it loads it. The result keeps the FM24 build string in `game_info.dat` and the FM24 DB tag and schema (`2430`, 3569), but the records and the DB header are FM27's. Only `save_game_summary.dat` shows the newer build. The parser therefore picks the layout from `layout_major()`: the newer of the creating build and the last-saved-with build. Before this, such a save was read with the FM24 layout and returned 0 players.

FM20–FM23 are **unverified**:
- There are no local saves.
- No public layout evidence for them was found.

`layout_for` returns `None` for these versions, and the statistical probe (§9) recovers the layout. The CLI and the JSON `warnings` mark the output as unverified.

FM25 does not exist. Whether FM20–23 use this same zstd container is **unknown**; older saves may use another compression. The parser reports a clear error when the signature or compression id does not match.

**Adding a version:** add one `PlayerLayout` constant in `layout.rs` (header length plus optional offsets), one `layout_for` arm, and a test.

## 6. Corrections to FootballSaveScout's model

| FootballSaveScout | Actual |
|---|---|
| `01 00 6c 07` is a record "tail marker" | It is the null date. Anchor on the repeated UID instead (+1.3 % / +1.7 % players). |
| Name tables are `len, name, id`; look up `ref + 1` | Tables are `id, len, name`; ids map directly. |
| Attribute slots 11/14/19/33 = 1v1/TTP/handling/communication | They are handling/communication/one-on-ones/TTP. |
| Personality ends controversy, temperament | It ends temperament, controversy. |
| FM26 `after+7` is gender | Not supported: it is 3 for Haaland in FM26 and 1 in FM24. **Unknown.** |
| Version detection by searching for `"26."` in the first 200 bytes | Parse the length-prefixed build string and the DB block. |
| Frame 3 = database, frames found by magic scan | Use the directory: `game_db.dat` is named. |

## 7. Field verification against the game UI

Statistics fix most fields (§4.3); the rest need one in-game screenshot per pair. Players
chosen so that every ambiguous pair has two different values (FM27 Man Utd career,
2026-01-14):

| player | role | pairs it settles |
|---|---|---|
| Yuel Helafu (Man Utd youth, DR) | outfield | pace 14 / acceleration 15, anticipation 10 / decisions 8, first touch 12 / technique 13, teamwork 12 / work rate 13, corners 5 / free kicks 4, composure 11 / concentration 9, flair 9 / dribbling 12, bravery 11 / aggression 14 |
| Dermot Mee (Man Utd, GK) | keeper | handling 12 / one-on-ones 9 / reflexes 13, aerial reach 14 / command of area 10, kicking 10 / throwing 7, communication 11 / eccentricity 5, rushing out 8 / punching 6 |

Status: **pending** (values above are what the parser reads; they become verified when the in-game profile shows the same numbers).

## 8. Open questions

- **Team types:** which slot is U21/U18/B. Not decoded; only the slot number is output.
- **Unresolved clubs:** 265 FM24 / 505 FM26 clubs; 0.2 % / 0.9 % of players.
- **Nation names:** only in match/scout records, otherwise a user file. Youth national-team records are not parsed.
- **Valuation:** what `valuation_unverified` (a+54) means.
- **Contracts:** the contract `kind` byte, clause contents, release clauses, and contracts of lower-league players.
- **FM26 date:** the in-game date versus joined-club dates up to 2025-10-15.
- **Unknown bytes:** the second date slot, the byte after the valuation, ability-header bytes r+4..r+10, name-block bytes +4..+8.
- **FM26 fields:** the weight field and the gender flag. Women's clubs are separate objects, but no per-person flag has been found.
- **Older versions:** the FM20–FM23 container and layouts (§9).

## 9. Unverified versions and the saves that would verify them

**Probe** (`probe.rs`). For a major version without a registered layout, every offset is chosen by a statistic over about 6 000 player blocks and must clear its threshold, otherwise it stays unset:

| offset | how it is chosen | threshold |
|---|---|---|
| header length | the length that validates the most UID-anchored ability blocks (12..32 tried) | — |
| club id | the header u16 whose values are parsed team ids, with ≥ 10 % distinct values | 50 % |
| nationality | the u16 after the name block that matches the club's nation most often | 40 % |
| personality | 8 bytes in 1..20 after the nationality | 80 % |
| height | a tail byte with ≥ 95 % in human range and the right mean | 95 % |
| weight | a tail byte with ≥ 95 % in human range and the right mean | 95 % |

Club entry shape and transfer-record shape are tried in both known forms.

- **Check against known versions:** with `--force-probe`, the probe reproduces the verified FM24, FM26 and FM27 layouts exactly (both FM27 saves) (test `probe_reproduces_verified_layouts`).
- **Output for probed layouts:** `layout.confidence = "Probed"`, a `probe` report with the support share of each offset, and a warning printed by the CLI and stored in JSON `warnings`.
- **Not probed:** the international team id.

**Saves that would verify each version.** For each version, ideally two saves:
- (a) a **new game saved on day 1, before any match**: the database is pristine, and attributes are mostly exact multiples of 5.
- (b) a save **a few months in**: dates, contracts and transfers are populated.

Saves live in `Documents/Sports Interactive/Football Manager <year>/games/*.fm`. A screenshot of one known player's profile (attributes, contract, wage, club) from the same save gives field-level ground truth.

| version | what is needed | why |
|---|---|---|
| FM20 (20.4.x) | (a) + (b) | Unknown whether the zstd `fmf.` container is used at all. If it is not, the container itself has to be reverse-engineered first. |
| FM21 (21.4.x) | (a) + (b) | Container, header length and offsets unknown. |
| FM22 (22.4.x) | (a) + (b) | Same as FM21. |
| FM23 (23.4.x) | (a) + (b) | The closest predecessor of FM24; likely the probe's best case. |
| FM24 other patches (24.0–24.2, 24.4) | (b) | Confirms the layout is per release, not per patch. This save was created with 24.3 and last saved with 24.4.2. |
| FM26 later patches (26.1+) | (a) | The FM26 save was created with 26.0 and last saved with 26.1.0; a fresh 26.1+ game confirms stability. |
| Console / Game Pass / Touch saves of any version | (a) | Separate builds; the save format may differ. |

**Verifying a new save:**
1. Run `fm_save_parser players <save> --json out.json`.
2. Compare the `probe` report and three or four known players with the in-game screenshots.
3. If they match, add a `PlayerLayout` constant (layout.rs) with the probed offsets and a test like `fm24_players`.
