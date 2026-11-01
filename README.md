# fm-open-scout

Open-source scouting for your own Football Manager saves. Ask your AI assistant
"find me a DM better than mine for under £25m" and get an answer from your actual save.

[![CI](https://github.com/YanivGabay/fm-open-scout/actions/workflows/ci.yml/badge.svg)](https://github.com/YanivGabay/fm-open-scout/actions/workflows/ci.yml)
[![Buy Me a Coffee](https://img.shields.io/badge/Buy%20me%20a%20coffee-yanivga-FFDD00?logo=buymeacoffee&logoColor=black)](https://buymeacoffee.com/yanivga)

> Independent project, not affiliated with or endorsed by Sports Interactive or SEGA.
> Football Manager is a trademark of Sports Interactive. fm-open-scout only reads save
> files on your own machine; it does not modify saves, the game, or any game data.

## What it does

- **Reads `.fm` saves** from FM24, FM26 and FM27, including FM24 careers re-saved by FM27.
  Other versions are read with a statistical layout probe and clearly marked as unverified.
- **Every player in the database:** CA and PA, all 54 attributes (including hidden ones),
  personality, positions, club, contract, wage, value and last transfer fee.
- **Scouting tools for AI assistants** through an [MCP](https://modelcontextprotocol.io) server:

  | Tool | What it answers |
  |---|---|
  | `list_saves` / `load_save` | Which saves you have; loads the newest by default and warns when a newer one appears |
  | `search_players` | Filter by role, position, CA, PA, age, value, wage, club, nation, free agents; sort by CA, PA, role fit, price or "bargain" (cheap for their CA) |
  | `player` | Full profile, including hidden attributes and personality |
  | `squad` / `compare_squads` | Your squad, best XI by slot, and where it falls short of your rivals |
  | `similar_players` | Players like X, by CA/PA/age or by playing style |
  | `project_player` | Estimate of how a player's attributes may develop (realistic and ceiling) |

- **A skill** (`skills/fm-scout`) that teaches the assistant a good scouting workflow:
  newest save first, find weaknesses, search with several lenses (ready now, role fit,
  undervalued, potential, free agents) and check determination before trusting a PA.
- **A command-line parser** (Rust) that exports a save to JSON, and a small JavaScript reader.

## Quick start

Requirements: Node.js 20+.

```bash
git clone https://github.com/YanivGabay/fm-open-scout.git && cd fm-open-scout
npm install
```

Then get the save parser, either:

- **Download it:** from the [latest release](https://github.com/YanivGabay/fm-open-scout/releases/latest),
  take the file for your system and save it as `bin/fm_save_parser` (`bin\fm_save_parser.exe`
  on Windows; on macOS and Linux run `chmod +x bin/fm_save_parser`), or
- **Build it** with [Rust](https://rustup.rs): `(cd fm_save_parser && cargo build --release)`.

### Use it from Claude Code

```bash
claude mcp add fm-open-scout -- node /path/to/fm-open-scout/mcp/server.js
```

Optional: copy the skill so Claude follows the scouting workflow.

```bash
mkdir -p ~/.claude/skills && cp -r skills/fm-scout ~/.claude/skills/
```

Then ask: *"load my newest FM save and tell me what my squad lacks"*.

### Use it from Claude Desktop or other MCP clients

Add to the client's MCP configuration (for Claude Desktop: `claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "fm-open-scout": {
      "command": "node",
      "args": ["/path/to/fm-open-scout/mcp/server.js"]
    }
  }
}
```

### Settings (environment variables)

| Variable | Default |
|---|---|
| `FM_SAVE_DIRS` | Auto-detected `Sports Interactive/Football Manager */games` folders (Windows, macOS, Linux, WSL). Separate several with `:` (`;` on Windows). |
| `FM_SAVE_PARSER` | `fm_save_parser/target/release/fm_save_parser` |
| `FM_OPEN_SCOUT_CACHE` | `~/.cache/fm-open-scout` (parsed saves; delete any time) |
| `FM_OPEN_SCOUT_QUIET` | Set to `1` to hide the one-line donation note shown on `load_save` |

### Command line

```bash
fm_save_parser/target/release/fm_save_parser info "My Save.fm"
fm_save_parser/target/release/fm_save_parser players "My Save.fm" --top 30 --json players.json
```

## Troubleshooting

| Problem | Fix |
|---|---|
| "no .fm saves found" | Saves are looked for in `Sports Interactive/Football Manager */games` (and `cloud/games`) under Documents, OneDrive\Documents, AppData\Local, macOS Application Support and Steam Proton prefixes. If yours are elsewhere, set `FM_SAVE_DIRS`. |
| "parser not found" | Put the release binary in `bin/` or build it (see Quick start), or set `FM_SAVE_PARSER`. |
| Results look out of date | Save the game again; the tools always use the newest save and say when a newer one appears. |
| "bad signature" | The file is not an FM24-FM27 save (or is still being written). Older versions: see FORMAT.md §9. |
| A target "doesn't want to sign" | Players compare their reputation with your club's. Search lower-reputation leagues and use promotion clauses or loans. |

## How accurate is it?

- The save format, the evidence for every field and its confidence level are documented
  in [FORMAT.md](FORMAT.md). Fields are checked against known real players and statistics
  over the whole database; `scripts/verify_fields.js` reproduces the statistics.
- **Values** are the save's own stored estimate, not the asking price a club will want.
- **Role scores** are weighted attribute averages (1-20) chosen by this project, a guide
  rather than the game's rating.
- **`project_player` is an estimate.** FM does not store future attributes; it develops
  players while you play. The estimate uses how each attribute rises with CA across all
  players of the same position, scaled by age, determination and professionalism.

## Privacy

Everything runs locally. Saves are only read, never modified or uploaded. Parsed data is
cached in your user folder. Please don't publish exported player databases.

## Contributing

New FM release? The parser's probe usually reads it on day one. To verify a version,
see FORMAT.md §9: one new-game save and one save a few months in, plus screenshots of a
couple of player profiles, are enough to register a verified layout.

Run the tests with `npm test`. The Rust integration tests read your own saves from the
git-ignored `saves/` folder (`fm24.fm`, `fm26.fm`, `fm27.fm`, `fm27_resaved.fm`, `fm26/`),
or from `FM24_SAVE`, `FM26_SAVE`, `FM27_SAVE`, `FM27_RESAVED`, `FM26_SAVE_DIR`; tests
without a save are skipped.

## Support the project

fm-open-scout is free and will stay free, with every feature open. If it helped your
save, you can [buy me a coffee](https://buymeacoffee.com/yanivga).

## License

[MIT](LICENSE)
