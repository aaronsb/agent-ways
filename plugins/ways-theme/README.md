# ways-theme

Claude Code follows the active agent-ways theme (ADR-513). The theme is chosen in one place, `theme.active` in ways settings, and the mod adds no setting of its own.

`ways manage mod install ways-theme --commit`, or the mod tab in `ways settings`, installs it. `mods.ways-theme.install` defaults to false.

## How it works

Claude Code keeps one custom theme for agent-ways, `custom:ways`, in `${CLAUDE_CONFIG_DIR:-~/.claude}/themes/ways.json`.

- **Selection.** While the mod is wanted, the settings merge that `ways manage repair --commit` and the mod manager run owns `"theme": "custom:ways"` in that target's settings.json, and lays down a default `ways.json` if none is there. Uninstalling removes the key through the merge base. A theme the person chose, before or after, is never claimed or removed.
- **Colours.** At an interactive session's start the mod reads `ways settings get theme.active` and rewrites `ways.json` when its content changed: `ways settings theme export <name> --claude-code` for a theme, Claude Code's own dark for `terminal`. Claude Code watches its themes folder, so the change applies live.

The mod selects nothing itself: in Claude Code 2.1.312 `$.config.set` refuses a `custom:` theme. Each `ways` call is bounded at 1.5 s. A missing `ways`, a non-zero exit, output that does not parse, or a call past its bound leaves the file as it was and one debug-log line. A headless (`-p`) session changes nothing.

Claude Code draws on the terminal's own background, so the exporter lifts every text colour to its contrast floor on the theme's bg and on a band of common terminal grounds of the theme's kind. A theme's gradient, border and rule glyphs, marks, presence and event glyphs and ornament do not reach Claude Code's chrome. A theme file's `[target.claude-code]` table pins any Claude Code colour token over the mapping.

## Versions

The plugin carries the `ways` version, since it calls `ways settings` verbs: `scripts/release.sh bump ways` moves both, and `make test-plugins` checks them.
