---
name: fm-scout
description: Scout players in the user's own Football Manager save with the fm-open-scout MCP tools. Use when the user asks about their squad, transfer targets, budget buys, wonderkids, free agents, similar players, player development or squad weaknesses in an FM save.
---

# FM scouting with fm-open-scout

The fm-open-scout MCP server reads the user's own `.fm` saves (read-only) and exposes
`list_saves`, `load_save`, `search_players`, `player`, `squad`, `compare_squads`,
`similar_players` and `project_player`.

## Always start from the newest save

1. Call `list_saves`. Saves are newest first; the user is almost always playing the top one.
2. Call `load_save` (no argument = newest) and say which save and in-game date you use.
3. If a tool result says a newer save exists, switch to it before answering. A stale save
   gives wrong squads, prices and contracts. If the user says something contradicts the
   data ("I already sold him"), ask them to save in-game and reload.

## Know the user's situation first

- Which club they manage (the save title usually says it, e.g. "Manager Name - Man Utd").
- Budget (transfer and wage), age limits, and positions they are happy with.
- The in-game date: January and summer are transfer windows.

## Find weaknesses before suggesting players

`compare_squads` with the user's club and its rivals (league leaders, same-level clubs).
The slot-by-slot CA table shows the gaps. Count depth too (`squad`): a position with one
good player and no backup is a weakness even if the starter is strong.

## Search with several lenses, not just CA

For each weak role run several `search_players` queries:

| Lens | Query |
|---|---|
| Ready now | `role`, `max_value`, `sort: "ca"` |
| Best fit for the role | `role`, `sort: "role"` (attribute score, compare with the user's starter) |
| Hidden quality | `role`, `max_ca` ~142, `sort: "role"`: good attributes, low CA |
| Undervalued | `max_price_index: 0.35`, `min_ca`, `sort: "ca"` |
| Potential | `min_pa`, `max_age` 21-23, `sort: "pa"` |
| Free | `free_agents_only: true` |
| Retrainable | `min_rating: 13` (a player who can learn the position) |

Then open the shortlisted players with `player` and check:
- **Determination and professionalism**: low values (< 10) mean the player will likely
  not reach his PA. Say so.
- Age curve: physical attributes decline after ~29.
- Contract end and wage, not only the transfer value.

Use `project_player` for young targets to show realistic vs ceiling development.

## Present clearly

- Tables: CA/PA, age, club, price, wage, why this player.
- Put the user's current starter next to a target so the upgrade is visible.
- Give a concrete plan that fits the budget, including who to sell (`squad` shows the
  price of surplus players).
- Mention that values are the save's own estimate, not the asking price, and that
  `project_player` is an estimate, not the game's future.

## Limits to mention when relevant

- Some saves have no nation names (they come from match data in the save).
- Rival clubs may refuse to sell, and players may refuse to move.
