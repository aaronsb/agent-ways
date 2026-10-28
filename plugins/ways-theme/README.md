# ways-theme

Claude Code follows the active agent-ways theme (ADR-513). The theme is chosen in one place, `theme.active` in ways settings, and the mod adds no setting of its own.

`ways manage mod install ways-theme --commit`, or the mod tab in `ways settings`, installs it. `mods.ways-theme.install` defaults to false.

## The theme file

Claude Code keeps one custom theme for agent-ways, `custom:ways`, in `${CLAUDE_CONFIG_DIR:-~/.claude}/themes/ways.json`.

- **Selection.** Installing the mod claims `"theme": "custom:ways"` in the target's settings.json, through the settings merge that `ways manage repair --commit` and the mod manager run, and records the theme it replaced in the merge base. Uninstalling restores that theme, or removes the key when there was none, and removes `themes/ways.json`. A theme the person picks after the claim is kept through both. `ways manage repair --what-if` names the change. The mod selects nothing itself, because `$.config.set` refuses a custom theme in Claude Code 2.1.312 (anthropics/claude-code#105739).
- **Colours.** At an interactive session's start the mod reads `ways settings get theme.active` and rewrites `ways.json` when its content changed: `ways settings theme export <name> --claude-code` for a theme, Claude Code's own dark for `terminal`. Claude Code watches its themes folder, so the change applies live.

Claude Code draws on the terminal's own background, so the exporter lifts every text colour to its contrast floor on the theme's bg and on a band of common terminal grounds of the theme's kind. A theme file's `[target.claude-code]` table pins any Claude Code colour token over the mapping.

## Frames

On the terminal, under an agent-ways theme, each reply sits inside a round border on the theme's ground, and each tool call and tool group inside a round border, wrapped around Claude Code's own drawing. A dark theme pairs with Claude Code's dark base and a dark terminal, a light theme with the light base and a light terminal, whose default foreground is dark; Claude Code draws a reply's body in that foreground. A light theme in a dark terminal is a mismatched setup, and its replies read poorly. The colours are Claude Code theme keys (`userMessageBackground`, `claude`, `suggestion`, `error`), so the frames follow `custom:ways`. Claude Code draws its own row, unframed, while the transcript is expanded with ctrl+o, for a row expanded on its own, under `verbose`, for a reply longer than 20,000 characters, for a block that does not open a reply, and on every surface other than the terminal. A frame costs four columns of width.

## Failures

Each `ways` call is bounded at 1.5 s. A missing `ways`, a non-zero exit, output that does not parse, or a call past its bound leaves the file as it was, draws no frame, and leaves one debug-log line. A headless (`-p`) session changes nothing.

A theme's gradient, border and rule glyphs, marks, presence and event glyphs and ornament do not reach Claude Code's chrome.

## Versions

The plugin carries the `ways` version, since it calls `ways settings` verbs: `scripts/release.sh bump ways` moves both, and `make test-plugins` checks them.
