# ways-theme

Claude Code follows the active agent-ways theme (ADR-514). The theme is chosen in one place, `theme.active` in ways settings, and the mod adds no setting of its own.

`ways manage mod install ways-theme --commit`, or the mod tab in `ways settings`, installs it. `mods.ways-theme.install` defaults to false.

## The theme file

Claude Code keeps one custom theme for agent-ways, `custom:ways`, in `${CLAUDE_CONFIG_DIR:-~/.claude}/themes/ways.json`.

- **Selection.** Installing the mod claims `"theme": "custom:ways"` in the target's settings.json, through the settings merge that `ways manage repair --commit` and the mod manager run, and records the theme it replaced in the merge base. Uninstalling restores that theme, or removes the key when there was none, and removes `themes/ways.json`. A theme the person picks after the claim is kept through both. `ways manage repair --what-if` names the change. The mod selects nothing itself, because `$.config.set` refuses a custom theme in Claude Code 2.1.312 (anthropics/claude-code#105739).
- **Colours.** At an interactive session's start the mod reads `ways settings get theme.active` and rewrites `ways.json` when its content changed: `ways settings theme export <name> --claude-code` for a theme, Claude Code's own dark for `terminal`. Claude Code watches its themes folder, so the change applies live.

Claude Code draws on the terminal's own background, so the exporter lifts every text colour to its contrast floor on the theme's bg and on a band of common terminal grounds of the theme's kind. A theme file's `[target.claude-code]` table pins any Claude Code colour token over the mapping.

## Frames

On the terminal, under an agent-ways theme, each reply sits inside a round border on the theme's ground, and each tool call and tool group inside a round border, wrapped around Claude Code's own drawing. A dark theme pairs with Claude Code's dark base and a dark terminal, a light theme with the light base and a light terminal, whose default foreground is dark; Claude Code draws a reply's body in that foreground. A light theme in a dark terminal is a mismatched setup, and its replies read poorly. The colours are Claude Code theme keys (`userMessageBackground`, `claude`, `suggestion`, `error`), so the frames follow `custom:ways`. Claude Code draws its own row, unframed, in the ctrl+o transcript, which the mod reads from the line that closes a turn, for a tool row expanded on its own, for a reply longer than 20,000 characters, for a block that does not open a reply, and on every surface other than the terminal. Under `verbose` Claude Code reports every row expanded, the main view included, so the mod frames rows as it does with `verbose` off, and the ctrl+o transcript, which then looks the same to the mod, is framed too. A frame costs four columns of width.

## The transcript drawing

The mod also redraws the transcript, headless, with the renderer of [skins](https://github.com/hellosverre/claude-skins) by hellosverre (MIT, ported from commit 985ad63; the licence is at `hooks/skins/LICENSE`). On the terminal it draws tool rows on a rail with their fold, open and copy controls, shell output as a card, tables, highlighted code, diff fences, math, Mermaid charts (laid out by the vendored beautiful-mermaid, MIT), alerts and task lists, the spinner with the theme's words, the turn footer, the usage band above the prompt, and the person's message in a box. Skins' `/skin` command, its settings pane, its design tool and skill, its themes, its settings store, its update check and its Compact button are not part of the port. Nothing in Claude Code configures it.

One call configures it: `ways settings theme export --mod`, at an interactive session's start and every 5 seconds after, bounded at 1.5 s. The export names the theme, its colour roles, its gradient, its spinner and footer words, and the `render` switches: `rail`, `tables` (`on`, `text`, `off`), `shimmer`, `charts`, `markdown`, `icons` (`unicode`, `ascii`), `frames`, `prompt_box` (`off`, `frame`, `gradient`) and `copy`. A change in `ways settings` reaches an open session within 5 seconds, and an unchanged export redraws nothing. An export that fails, or that does not match the contract key for key, leaves the drawing as it was: Claude Code's own at a session's start, the last good look afterwards.

The skin's colours come from the theme's roles: `accent` for the person's marks and the rail, `body` for text, `muted` for secondary text, `ok`, `err` and `warn` for outcomes, `info` for reads, `hot` for writes, `warn` for shell runs, `alt` for searches, `accent_dim` for web calls, `muted` for MCP calls and `rule` for the rest. A table's header band and its zebra rows are steps from `bg` toward `subtle`. Under the `terminal` theme the drawing takes skins' own terminal look, the terminal's sixteen colours.

Each reply, tool row and tool group sits in one frame, the one described above, and `render.frames` switches them off. A shell call's card is that call's frame, so its row goes unframed. Inside a frame the rail draws no connector.

The person's message, with `prompt_box` at `gradient`, sits on the theme's gradient in `body` text, mixed in OKLab. A message under 4 lines runs the gradient across each line, padding rows included; one of 4 lines or more runs it down the lines. A message with an image or a pasted block, a row in the ctrl+o transcript (under `verbose`, as for the frames, no row counts as one), a view under 12 columns and the `terminal` theme take a plain outline instead. So does every message under a theme with no gradient: `ways` exports its `prompt_box` of `gradient` as `frame`. A message holding a control character is Claude Code's own. With `copy` off, the Copy, `as text` and `copy reply` controls are not drawn.

On the desktop, VS Code and mobile the rows skins draws there are its own (the desktop's cards), and the person's message, the frames and the gradient are Claude Code's own drawing.

## Failures

Each `ways` call is bounded at 1.5 s. A missing `ways`, a non-zero exit, output that does not parse, or a call past its bound leaves the file as it was, draws no frame, and leaves one debug-log line. A headless (`-p`) session changes nothing.

A theme's gradient, border and rule glyphs, marks, presence and event glyphs and ornament do not reach Claude Code's chrome.

## Versions

The plugin carries the `ways` version, since it calls `ways settings` verbs: `scripts/release.sh bump ways` moves both, and `make test-plugins` checks them.
