# ways-theme

Claude Code follows the active agent-ways theme (ADR-513). The theme is chosen in one place, `theme.active` in ways settings, and the mod adds no setting of its own.

`ways manage mod install ways-theme --commit`, or the mod tab in `ways settings`, installs it. `mods.ways-theme.install` defaults to false.

## What it does

At an interactive session's start the `session.start` hook:

1. reads `ways settings get theme.active`;
2. for an agent-ways theme, runs `ways settings theme export <name> --claude-code`, writes the result to `${CLAUDE_CONFIG_DIR:-~/.claude}/themes/ways-<name>.json` when the file differs, and selects `custom:ways-<name>` when Claude Code's theme row differs;
3. for `terminal`, moves a `custom:ways-*` theme back to Claude Code's `dark` and leaves any other theme alone.

Each `ways` call is bounded at 1.5 s. A missing `ways`, a non-zero exit, output that does not parse, or a call past its bound changes nothing and leaves one debug-log line. A theme the person picks in `/theme` stays until the next session start. A headless (`-p`) session changes nothing.

Claude Code draws on the terminal's own background, so the exporter lifts every text colour to its contrast floor on the theme's bg and on a band of common terminal grounds of the theme's kind. A theme's gradient, border and rule glyphs, marks, presence and event glyphs and ornament do not reach Claude Code's chrome. A theme file's `[target.claude-code]` table pins any Claude Code colour token over the mapping.

## Versions

The plugin carries the `ways` version, since it calls `ways settings` verbs: `scripts/release.sh bump ways` moves both, and `make test-plugins` checks them.
