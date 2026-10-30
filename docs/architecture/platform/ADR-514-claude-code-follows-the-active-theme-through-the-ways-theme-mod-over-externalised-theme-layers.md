---
contract: adr/v1
kind: decision
verb: add
capability: cli
extends: [ADR-513]
basis:
  - precedent: ADR-513
  - precedent: ADR-504
  - precedent: ADR-512
  - precedent: ADR-406
  - operator: aaronsb
    level: guided
    said: "you know the natural extension of this is to write a claude mod that themes claude code the same way, for consistency"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "a miami nights claude code would look pretty rad"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "would it make sense to externalize the themes to afford common elements for both, mapped for specific ones, and the custom ones for custom?"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "let's externalie themes too"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "we can use the mod manager in ways settings to activate/deactivate the claude theme"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "to keep it straightforward, the theme is managed in one place, and active/inactive/uninstalled/installed for claude code just follows the configured theme. if the mod is installed but ways is broken or unavailable, it should just basically no-op"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "the mod can't recolor things?"
    via: "operator, in chat in the commissioning session; a question, which led to the frame layer"
  - operator: aaronsb
    level: directed
    said: "remember it's tui and the web mode"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "focusing mostly on tui first"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: guided
    said: "let's search for 'claude skins' - maybe thats what we we need"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "let me try the plugin here, and if it seems to make a difference, then we can copy it and adjust it so it's more integrated as a private plugin"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "well, it colors the text in the response. there's several options around. I activated it for this project scope only"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "see what happens. something new is it puts a box around my replies which honestly is kinda helpful"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "but there's a bunch of options, and they're extremely inconvenient to navigate - they're ina  weird strip in a ui in claude code - I'd rather just have a mod system that's more of a hook and applies stuff (headless) and is configurable through ways. the miami-nights skin looks pretty nice"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "do you think the gradient style could be applied to the 'box' that my statements show inside?"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "I vote we strip out a lot of the ui stuff that makes it run inside claude code and move that over to a claude theme controller in ways"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "'ui stuff' -> the menu options for it etc."
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "I think we can get our new idea built here"
    via: "operator, in chat in the commissioning session"
  - operator: aaronsb
    level: directed
    said: "can we make the copy commands a toggle"
    via: "operator, in chat in the commissioning session"
  - evidence: "the live refusal in Claude Code 2.1.312, from the mod's debug log in a scratch config: `ways-theme: theme custom:ways-miami-nights refused: takes one of: auto, dark, light, light-daltonized, dark-daltonized, light-ansi, dark-ansi`; `/theme` selects the same custom theme"
  - upstream: "anthropics/claude-code#105739, filed 2026-10-28: a mod's `$.config.set` refuses a custom theme that `/theme` accepts (https://github.com/anthropics/claude-code/issues/105739)"
  - upstream: "Claude Code custom themes: a JSON file in the config directory's `themes/` folder with `name`, `base` (a built-in preset) and `overrides` by colour token, selected as `custom:<file>` in settings.json's `theme`; Claude Code watches the folder and reloads a changed file (code.claude.com, terminal-config, \"Create a custom theme\"; the token list is held at tools/agent-theme/src/target.rs:13-28)"
  - upstream: "Claude Code mods: `ui.render` lets a mod replace or wrap the element the engine draws for a transcript entry, on the surface it reports"
  - upstream: "Claude Code's mod API: `$.fs` reads and writes files and offers no watch, and `$.clock.every` runs a callback on an interval (plugins/ways-theme/.claude-plugin/types/claude-code/index.d.ts)"
  - evidence: "the code on feature/claude-code-theme-mod at dafbc023: the target mapping and Claude Code export (tools/agent-theme/src/target.rs), roles lifted onto extra grounds (`Roles::derive_on`, tools/agent-theme/src/derive.rs:180), the export command (tools/ways-cli/src/cmd/settings/themes.rs:165), the settings merge's `theme` key (tools/ways-cli/src/cmd/settings_merge.rs), the default theme file laid down by repair (tools/ways-cli/src/cmd/reconcile.rs), the mod (plugins/ways-theme/hooks/register.ts, lib.ts), and `ways-theme` in the mod manager (tools/ways-cli/src/cmd/attend_mod.rs)"
  - evidence: "the code on feature/claude-code-theme-mod at 661a9be6: the ported renderer and its configuration (plugins/ways-theme/hooks/skins/hub.tsx, config.ts, prompt-box.tsx, LICENSE), the mod export (`export_mod`, tools/ways-cli/src/cmd/settings/themes.rs:282), the `mods.ways-theme.*` keys (tools/ways-core/src/settings.rs:560-680), the theme tab's Claude Code section (tools/agent-tui/src/app/themecc.rs), and the `[words]` table (tools/agent-theme/src/text.rs:592)"
  - evidence: "Claude Code paints no ground under the transcript, and a mod has no call that writes OSC 11, so exported text colours sit on whatever background the terminal has"
  - upstream: "prior art: the looks Claude Code theme project (MIT), read for its approach only; no code is borrowed from it"
  - upstream: "hellosverre/claude-skins (MIT), a Claude Code mod that redraws the transcript through `ui.render` and is configured inside Claude Code through `/skin`, a settings pane and a gallery; its renderer is ported from commit 985ad63 (https://github.com/hellosverre/claude-skins)"
  - upstream: "beautiful-mermaid 1.1.3 (MIT), bundled with skins' renderer for chart layout (plugins/ways-theme/hooks/skins/vendor/mermaid-ascii.js)"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Ship it now"
    via: "operator, multiple-choice prompt in the commissioning session; question and option labels written by the agent, option chosen by the operator; question: \"The Claude Code mod can also wrap each reply and tool group in the theme's background with a border (the second screenshot: violet background, lavender border). It costs 4 columns per row, and it still needs ctrl+o expansion handled and the tool-group hook added. Ship it with this mod?\"; options not chosen: \"Later, own PR\" and \"Drop it\". Decision item 6"
  - operator: aaronsb
    said: "Switch on install"
    via: "operator, multiple-choice prompt in the commissioning session; question and option text written by the agent, option chosen by the operator; question: \"If you already have a Claude Code theme set when you install the mod, should installing it switch you to the ways theme?\"; option text: \"Installing the mod selects the ways theme even over a theme you chose; uninstalling gives yours back.\"; option not chosen: \"Keep theirs\". Decision item 5"
  - operator: aaronsb
    said: "File it"
    via: "operator, multiple-choice prompt in the commissioning session; question written by the agent, option chosen by the operator; question: \"Claude Code refuses to let a mod select a custom theme through config.set, though /theme can. File the drafted issue on anthropics/claude-code?\"; filed as https://github.com/anthropics/claude-code/issues/105739"
  - operator: aaronsb
    said: "Yes"
    via: "operator, multiple-choice prompt in the commissioning session; question: 'ADR-514 check: you pick the theme once, in ways settings, and Claude Code follows it. The mod has no theme setting of its own; installing or uninstalling it through the mod manager is its only switch. Right?'"
    covers: [one place]
  - operator: aaronsb
    said: "Yes, terminal first"
    via: "operator, multiple-choice prompt in the commissioning session; question: 'ADR-514 check: in web and desktop mode Claude Code gets only what its custom theme file gives it (if those surfaces read it at all), with no frames, until the terminal side settles. Is that the right holding position for now?'; option text: 'Web and desktop get no frames for now and nothing breaks there; revisit later.'"
    covers: [web later]
  - operator: aaronsb
    said: "Fold into this release"
    via: "operator, multiple-choice prompt in the commissioning session; question and option text written by the agent, option chosen by the operator; question: \"When should the skins-style rendering (tool rail, tables, spinner words, etc., coloured from the ways theme, settings in `ways settings`) go into the ways-theme mod?\"; option text: \"Hold the release until the skins-derived rendering is in the mod too; one bigger release.\"; option not chosen: \"Next increment\". Decision item 9"
  - operator: aaronsb
    said: "Horizontal across"
    via: "operator, multiple-choice prompt in the commissioning session; question and option text written by the agent, option chosen by the operator; question: \"In the prototype, your one-line messages show the vertical gradient as three stripes: a padding row, the text row, a padding row. How should the gradient box look?\"; option text: \"Short messages sweep the gradient left to right across the line, which is smooth even on one line; long messages (4+ lines) run it top to bottom.\"; options not chosen: \"Vertical, no padding\" and \"Keep as shown\". Decision item 13"
  - operator: aaronsb
    said: "Keep them"
    via: "operator, multiple-choice prompt in the commissioning session; question and option labels written by the agent, option chosen by the operator; question: \"skins adds inline 'Copy', 'as text' and 'copy reply' labels on cards and replies. Keep them?\"; options not chosen: \"Drop them\" and \"Setting, off by default\". The operator later asked \"can we make the copy commands a toggle\", which led to `mods.ways-theme.copy`, default on. Decision item 15"
  - operator: aaronsb
    said: "It's fine to have it interact with desktop app"
    via: "operator, in chat in the commissioning session, answering: 'On the desktop app, the mod now draws skins' cards for tool calls and shell output; everything else there stays Claude Code's own. Is that fine for now?'"
observable:
  - 'try: with Miami Nights active in ways settings, install the mod from the mod tab and start a Claude Code session; the prompt border is neon pink, Claude''s text and the spinner are in the theme''s accent, and a reply sits in a violet frame with a lavender border'
  - 'try: change `theme.active` to Tampa and start another session; the open session and the new one both redraw in Tampa''s colours without a restart'
  - 'run: `ways settings theme export miami-nights --roles` prints every role with the floors applied; `--claude-code` prints a custom theme whose every text token reads at 4.5:1, and every muted token at 3:1, on the theme''s bg and on the ground band'
  - 'try: with `/theme` set to light, install the mod: Claude Code shows the ways theme; uninstall it: Claude Code is back on light and `themes/ways.json` is gone'
  - 'try: with `ways` removed from PATH, start a session with the mod installed; Claude Code starts with the theme file as it was, and the debug log has one ways-theme line'
  - 'see: with the frame layer on, ctrl+o on a framed tool group shows the engine''s own expanded view, unframed'
  - 'run: on a Claude Code below 2.1.309, `ways manage mod install ways-theme` reports the install pending and no theme change; with `--commit`, settings.json keeps its theme and no `themes/ways.json` is written'
  - 'run: `ways settings theme export --mod` prints one line of JSON with the keys theme, kind, roles, gradient, words and render; under `terminal`, roles and gradient are null'
  - 'try: with a session open, run `ways settings set mods.ways-theme.copy false`; within 5 seconds the Copy, `as text` and `copy reply` labels are gone, with no restart'
  - 'see: under Miami Nights a one-line message sweeps the gradient left to right, a message of 4 lines or more runs it top to bottom, and a message with a pasted block takes the outline'
  - 'see: with the mod installed, Claude Code lists no `/skin` command and has no skins settings or gallery pane'
status: accepted
date: 2026-10-28
deciders:
  - aaronsb
related: [ADR-513, ADR-504, ADR-512, ADR-406, ADR-500]
---

# ADR-514: Claude Code follows the active theme through the ways-theme mod, over externalised theme layers

## Summary

- **Decided:** A Claude Code mod, `ways-theme`, makes Claude Code follow the theme chosen in ways settings and draws its transcript in that theme. The theme is chosen in one place, `theme.active`; the mod is installed or removed through the mod manager, off by default, and every setting it reads is a ways setting. Themes are split into three layers: the palette (the `.theme` file), the roles derived from it with the contrast floors applied, and per-target mappings from roles to another program's colour keys, which a theme file can pin. Claude Code gets one fixed custom theme, `custom:ways`, whose file the mod rewrites at session start; installing the mod selects it over the person's theme and uninstalling gives theirs back. On the terminal surface the mod frames replies and tool groups in the theme's ground and a border, redraws the transcript headless with a port of skins' renderer, and sets the person's message on the theme's gradient. One `ways` export, read every 5 s, configures the drawing, and none of skins' menus, slash commands or settings store is ported. A missing, failing or slow `ways` does nothing.
- **Trades away:** Four columns of every framed row on the terminal. The person's own Claude Code theme while the mod is installed, though it comes back on uninstall. The theme's border and rule glyphs, marks and ornament, which Claude Code's chrome cannot take. Exact colours: text roles are lifted to read on terminal backgrounds the theme does not paint, so they can sit further from the file's values than on the theme's own screens. Selection through settings.json and repair, where a supported mod call would be simpler, until Claude Code lets a mod select a custom theme. About 14k vendored lines that this project maintains and skins' later releases do not reach. A `ways` process every 5 s in each open session.
- **One-way?** No. Reversible: uninstalling the mod restores the person's theme, removes the theme file and stops the drawing, and the theme layers are additions to the theme file format that a theme need not use.
- **Probes:** *Confident (one place):* you want the theme picked once, in ways settings, and Claude Code to follow it with no theme setting of its own in the mod. *Not confident (web later):* you are content that the web surface gets no frame and only what Claude Code's custom theme gives it, until the terminal surface has settled.
- **Inversion:** Between a Claude Code theme maintained by hand beside each agent-ways theme, and Claude Code drawn entirely by agent-ways. The record sits between them: agent-ways supplies colours through Claude Code's own theme file and redraws only the components a mod may render, while Claude Code keeps its chrome, its menus and every surface but the terminal.

## Context

ADR-513 has every agent-ways surface follow `theme.active`. Claude Code, the program those surfaces sit beside, keeps its own theme, so a session under Miami Nights draws agent-ways in neon violet and Claude Code in its default dark.

Claude Code takes custom themes: a JSON file in `themes/` naming a built-in base and overriding colour tokens, selected in settings.json as `custom:<name>`. It watches the folder and reloads a changed file. A mod's `$.config.set` refuses a custom theme, which `/theme` accepts (anthropics/claude-code#105739). Claude Code paints no ground under the transcript, and a mod cannot set the terminal's background, so its text lands on whatever background the terminal has. A mod's `ui.render` can wrap the element the engine draws for a transcript entry.

An agent-ways theme holds slots; the roles every surface draws are derived from them with the contrast floors applied (ADR-504 §4). A second program needs those roles mapped to its own keys, and a theme may want a key the mapping does not give.

Colours alone leave Claude Code's layout as it is. skins (hellosverre/claude-skins, MIT) is a mod that redraws the transcript through `ui.render`: tool rows on a rail, shell output as cards, tables, code, charts, the spinner and a box around the person's message. It keeps its own themes and settings store and is configured inside Claude Code, through `/skin`, a settings pane and a gallery. Tried in one project, its drawing helped, and its menus were hard to navigate.

## Decision

1. **Three theme layers.**
   - *Palette:* the `.theme` file's slots and keys (ADR-504 §4, ADR-513).
   - *Roles:* derived once from the palette with the floors applied, the same roles every agent-ways surface draws. `ways settings theme export <name> --roles` prints them as JSON, so another program can read them without agent-theme.
   - *Targets:* a default mapping from roles to each target program's keys, held in agent-theme (`target.rs`), with each key's contrast floor. A theme file may carry a `[target.<name>]` table of `key = "#rrggbb"`, and an entry there wins over the mapping. A key the target does not list is a parse error. Claude Code is the one target; `ways settings theme export <name> --claude-code` prints its custom theme.

2. **Roles on a ground agent-ways does not paint.** For a target that draws on the terminal's own background, the roles are derived again (`Roles::derive_on`) with every text role lifted to its floor on the theme's bg, its selection ground, and a band of common terminal grounds of the theme's kind: black and `#2a2a2a` for dark, white and `#e8e8e8` for light. The gradient is dropped, since the target does not paint it. A background token, such as a diff or a message background, is shaded only as far as the body text still reads on it.

3. **One fixed Claude Code theme.** Claude Code has one custom theme for agent-ways, `custom:ways`, in `<config dir>/themes/ways.json`. At the start of an interactive session the mod reads `theme.active` through `ways settings get`, exports that theme with `--claude-code`, or Claude Code's own dark for `terminal`, and rewrites the file when its content changed. It does the same mid-session whenever the export of item 10 changes, on that export's 5 s read, so a switch of theme or an edit to its colours reaches the file, and a rewrite that failed is tried again on each later read. Claude Code reloads it live, so every open session follows. A headless session changes nothing.

4. **The mod manager installs it.** `ways-theme` is a mod under `mods.ways-theme` (ADR-512), installed from the mod tab in ways settings or with `ways manage mod install ways-theme --commit`. It defaults to not installed. The mod adds no theme setting: its theme is `theme.active`, and its render settings are ways settings (item 11). Its Claude Code floor is 2.1.309, the release it was seen running on, whose engine wrote the type declarations that carry every call it makes. The floor is checked at install, so a Claude Code downgraded afterwards is not caught.

5. **Selection through the settings merge.** While `mods.ways-theme.install` holds and Claude Code lists the mod installed in that target, the settings merge that repair and the mod manager run (ADR-500) owns settings.json's `theme` key there. A wanted mod still pending, on a Claude Code below its floor or with no `claude` on PATH, claims nothing.
   - Installing claims the key for `custom:ways` and keeps the value it held. The claim is made even over a theme the person chose. A live `custom:ways` is never kept as that value.
   - A value the person sets after the claim is their later choice: kept, and neither claimed again nor replaced on uninstall.
   - Uninstalling gives back the kept value, or removes the key when it held none, and removes `ways.json`. A live `custom:ways` with the merge's record lost is the mod's own and is removed the same way.
   - Each install, uninstall and repair what-if names the key's value before and after, and `ways.json` written or removed.
   - Repair lays down a default `ways.json` before it selects it, so the folder watch holds from the first session.
   - The mod itself selects nothing, because `$.config.set` refuses a custom theme. When Claude Code lets a mod select one, selection moves into the mod and the merge gives up the key.

6. **The frame layer.** On the terminal surface, the mod's `ui.render` wraps the element the engine draws for an assistant message (`AssistantMessage`), a tool use (`ToolUse`) and a tool group (`ToolGroup`) in the theme's ground with a border, which costs four columns per row. The frame's colours are Claude Code theme keys, so it draws only while Claude Code's selected theme is `custom:ways`, read from the `theme` row of `/config` at session start, on each `config.set` of the row, which the mod observes and passes on unchanged, and on the 5 s read of item 10. Under another theme the person picked in `/theme` or `/config`, or a row that cannot be read, the frames and the person's message box (item 13) step aside for Claude Code's own drawing, and selecting `custom:ways` again brings them back. It hands back the engine's own element, unframed, in the ctrl+o transcript, for a tool row expanded on its own, or for a very long reply. The ctrl+o transcript is read from the line that closes a turn (`TurnDuration`), the one row whose `isExpanded` describes the whole view. Verbose mode is a standing preference, not a view: under it every row reports `isExpanded`, so the mod reads `isExpanded` as false and frames as it does with verbose off, the ctrl+o transcript included. One predicate decides framing for this layer and the skins layer (item 9), from one state. On any other surface, the web surface included, it does nothing.

> Note, 2026-10-28, after review of the mod branch: the claim waited only on `mods.ways-theme.install`, so a Claude Code below the floor kept no mod and still lost its theme; item 5 now waits on the mod being installed. The floor was 2.1.312, resting on `$.config.set`, which the mod does not call to select a theme; item 4 now sets it at 2.1.309. ctrl+o was read from a person's composer row, whose `isExpanded` is that row's own drawing under a speaker label; item 6 now reads the turn line. A pin in a theme's `[target.claude-code]` table is exported as given and is now checked at its token's floor, as the mapping's colours are.

> Note, 2026-10-28, after a live run under the operator's own settings: item 6 handed back the engine's element in verbose mode, and Claude Code reports every row as expanded under verbose, so a person running with `verbose: true` saw no frames and no gradient box. Item 6 now treats verbose as a preference and frames under it; the cost is that the ctrl+o transcript under verbose is framed too, since a hook cannot tell it from the main view. The operator runs with verbose on and asked for the frames and the box.

> Note, 2026-10-28, after the operator picked Claude Code's built-in `dark` in `/theme`: the frames drew whenever the ways theme had colours, without checking Claude Code's selection, so every reply drew in dark's orange `claude` outline on its gray `userMessageBackground`, whatever the ways theme. Item 6 now frames only while `custom:ways` is selected, and item 13's box steps aside with the frames. In the same session, `theme.active` moved to Miami Nights while `ways.json` kept the earlier theme until a new session, so the frames named Claude Code keys still holding the old colours. Item 3 now rewrites the file mid-session whenever the live export changes, and retries a rewrite that failed.

7. **Fail-safe.** Each `ways` call is bounded at 1.5 s. A missing `ways`, a non-zero exit, output that does not parse, a call past its bound, or a refused write leaves `ways.json` as it was and writes one line to the debug log. A failure in the frame layer hands back the engine's element.

8. **What Claude Code cannot take.** A theme's borders, rule glyphs, marks, presence, event glyphs, waypoints and ornament reach agent-ways' own surfaces only. Its gradient reaches Claude Code in the person's message alone (item 13). Claude Code takes colours through its tokens and the renderer's mapping from the roles. Its chrome outside the components a mod may render keeps its own glyphs and layout.

9. **skins' renderer, headless.** The renderer of skins (hellosverre/claude-skins, MIT, commit 985ad63) is ported into the mod, with its licence kept at `hooks/skins/LICENSE` and a header on each ported file naming its source. Kept: the tool rows on a rail with their fold, open and copy controls, shell output as a card, tables, highlighted code and diff fences, math, Mermaid and data charts, markdown with alerts and task lists, the spinner, the turn footer, the usage band above the prompt, and the outline around the person's message. Removed: `/skin` and every other slash command, the settings and gallery panes, the design tool and its skill, the settings store, skins' theme files, the update check, and the Compact button. The renderer's colours come from the theme's roles through a fixed mapping; under `terminal` it takes skins' own terminal look, the terminal's sixteen colours.

10. **One source of configuration.** `ways settings theme export --mod` prints one JSON object: the theme's name and kind, its roles as Claude Code's grounds take them, its gradient, the words in effect, and the render settings. The mod reads it at an interactive session's start and every 5 s after, each read bounded at 1.5 s, and holds it to the contract key for key. An unchanged export redraws nothing. An export that fails or breaks the contract keeps the last good look, or, before there is one, leaves every row Claude Code's own, and logs one debug line per distinct reason.

11. **The theme controller in ways.** The render settings are ways settings under `mods.ways-theme`: `rail`, `tables` (on, text, off), `shimmer`, `charts`, `markdown`, `icons` (unicode, ascii), `frames`, `prompt_box` (off, frame, gradient), `copy`, `copy_as` (content, markdown; added 2026-10-29), `paths` (wrap, truncate), `spinner_words` and `done_words`. They are edited in the Claude Code section of the `ways settings` theme tab, beside the palette the theme maps to in Claude Code and the words in effect, or with `ways settings set`. A value a project's config sets is shown with that file, and a change made in the tab writes the user config, under the project value. Claude Code offers no control over the drawing.

12. **The `[words]` table.** A theme file may carry `[words]`: `spinner`, shown while a turn runs, and `done`, the verbs of a turn's done line, each 1 to 12 entries of 1 to 24 characters with no control character. Miami, Miami Nights and Tampa carry theirs. `mods.ways-theme.spinner_words` and `done_words` win over the theme's list, list by list, and a slot neither sets keeps Claude Code's own words.

13. **The gradient prompt box.** With `prompt_box` at `gradient`, the default, the person's message on the terminal is drawn on the theme's gradient, mixed in OKLab, in the theme's body colour. A message under 4 lines sweeps the gradient across each line, padding rows included; one of 4 lines or more runs it down the lines, whatever the theme's own axis. Text is laid out by grapheme, so an emoji sequence is never split, and a line keeps its own leading indentation. A message with an image or a pasted block, the ctrl+o transcript or an expanded row (read as in item 6, so verbose does not count), a view under 12 columns, a theme with no gradient, and the `terminal` theme take the outline instead. A message holding a control character or longer than 4,000 characters keeps Claude Code's own drawing, as every surface but the terminal does, and so does every message while another Claude Code theme than `custom:ways` is selected (item 6). `frame` draws the outline every time under `custom:ways`, and `off` leaves Claude Code's own.

14. **One frame per row.** The frames of item 6 and skins' outline are one set: while `custom:ways` is selected, each reply, tool row and tool group sits in one frame, and `frames` switches them off. A shell call's card is that call's frame, so its row is not framed again, and inside a frame the rail draws no connector.

15. **The copy labels.** skins' inline Copy, `as text` and `copy reply` controls are kept, and `mods.ways-theme.copy`, default on, switches them off. `mods.ways-theme.copy_as` (added 2026-10-29) sets what a block's Copy takes: its content, the default, or the markdown it was written as; `as text` and `copy reply` are unchanged by it.

16. **Surfaces.** The terminal is the surface this record designs for. Off the terminal, a surface that draws SVG keeps the cards skins draws there, the desktop's among them; the person's message, the frames, the gradient and every other row are Claude Code's own.

> Note, 2026-10-28, after the skins trial: the record had the mod add no setting of its own (item 4), kept the gradient out of Claude Code (item 8), and borrowed no code from the prior art. The operator said "I vote we strip out a lot of the ui stuff that makes it run inside claude code and move that over to a claude theme controller in ways", and items 9 to 16 port skins' renderer without its menus and put its settings in ways.

> Note, 2026-10-29, after the copy blocks and their review: on the terminal, text a reply hands the person to copy draws as a block of its own outside every frame, so a selection over it takes no border glyph, padding space or wrap break, and its Copy takes the text whole. There are five forms. A `!` shell suggestion. A `/` slash command: the bare name on a line of its own, or with its arguments in a code span or a one-line fence; a sentence that opens on a command name and a top-level directory such as `/tmp` stay prose. A fence's code, less the indentation of the list item it sits in. A code span wider than 60% of its row, less that indentation. A table, which Copy takes as CSV with its header row under `copy_as: content`, each code span in a cell kept as written. The operator also said to "remove the folding content. it's just obscuring the output". An opened tool row had stopped at 20 lines behind a `show all` control, which a row printed into the terminal's scrollback cannot press, and the desktop's code and diff cards had stopped at 80 and 30 lines with "N more lines". Every line now draws and a long one wraps. A block past what one drawing or one Svg document holds keeps Claude Code's own drawing, as a reply past the Markdown limit does. The chevron that opens a tool row (item 9) stays.

The figure for this record is two screenshots of Claude Code 2.1.312 under Miami Nights, taken during the work and described here because the corpus keeps figures as SVG. The first shows the welcome block and a typed prompt: the prompt box's border is neon pink, the header and status text are lavender, and the transcript sits on the terminal's own near-black. The second shows the frame layer's prototype: the user's prompt on a violet bar, each reply in a violet block inside a rounded lavender border, and a collapsed tool line ("Listed 1 directory (ctrl+o to expand)") left unframed between them.

## Consequences

### Positive

- One choice in ways settings themes agent-ways and Claude Code together, and a change reaches open sessions live.
- Another program can follow agent-ways themes by reading the roles export or adding a target mapping, without its own copy of the palettes.
- A theme can pin any Claude Code token where the mapping falls short, in the same file as its palette.
- Text stays readable on common terminal backgrounds the theme does not paint.
- A broken or missing `ways` never breaks Claude Code.
- The transcript takes the theme's colours, words and gradient in its tool rows, cards, tables, charts and the person's message.
- Every setting the drawing reads is edited where the theme is chosen, and Claude Code carries no menu for it.

### Negative

- Framed rows lose four columns, which narrow terminals feel first.
- The frame layer and the renderer depend on the element names and props Claude Code's engine passes to `ui.render`; a Claude Code release that changes them leaves rows unframed or misdrawn until the mod follows.
- Selection runs through settings.json and repair, so a person who sets `/theme` by hand after the claim keeps their choice, and the mod's colours apply only while `custom:ways` is selected.
- Installing the mod replaces the person's theme until uninstall, which a person who chose their own theme may not expect.
- Lifting onto the ground band moves text colours further from a theme's own values, most for themes whose bg is far from black or white.
- Each session start runs up to two `ways` calls, bounded at 1.5 s each, before the theme file is current.
- About 14k lines are vendored, an 84 KB bundle of beautiful-mermaid's layout among them. This project maintains them, and a fix in skins arrives only when ported by hand.
- Each open session starts a `ways` process every 5 s.
- The bundled Miami, Miami Nights and Tampa files carry `[words]`, so they no longer source as shell.
- A shell card in a resumed session shows "Output" in place of its command.

### Neutral

- The web surface gets whatever Claude Code's custom theme gives it, and no frame.
- `[target.<name>]` and `[words]` are tables in the theme file, as `[[waypoints]]` is (ADR-513 item 15), so a theme file that uses one no longer sources as shell.
- The mod carries the `ways` version, since it calls `ways settings` verbs.
- The looks project (MIT) supplied ideas only. skins' code is ported under its licence.

## Alternatives Considered

- **A hand-written Claude Code theme beside each agent-ways theme.** Two files per theme drift apart, and a user theme would need its Claude Code twin written by hand. A mapping derives both from one palette.
- **The mod selects its theme with `$.config.set`.** Claude Code 2.1.312 refuses a custom theme there. Filed as anthropics/claude-code#105739; selection moves into the mod once the call accepts one.
- **One custom theme file per agent-ways theme.** Switching themes would change the selected name in settings.json, which the mod cannot write. One fixed name whose content changes needs settings.json touched only on install and uninstall.
- **Keep the person's theme on install** ("Keep theirs", declined by the operator). The mod would do nothing visible for anyone with a theme already chosen.
- **Ship the frame layer in a later change** ("Later, own PR", declined by the operator), **or drop it** ("Drop it", declined by the operator). The colours alone leave replies on the terminal's own background, which is the look the operator asked the mod to change.
- **Set the terminal's background with OSC 11.** A mod has no call that writes to the terminal outside the engine's rendering, and a background changed under the person's terminal would outlive the session.
- **Export only the roles and let each target map them.** Every target would carry its own copy of the floors. The mapping in agent-theme applies them once, and the roles export stays for programs agent-ways does not map.
- **Install skins as it is.** Its drawing is configured through menus inside Claude Code, which the operator found inconvenient, and it keeps a second set of themes beside agent-ways'.
- **Port the renderer in a later release** ("Next increment", declined by the operator). The operator chose one release carrying both the theme and the drawing.
- **Pick up a settings change through a file watch.** The mod API reads and writes files and offers no watch, so the mod polls the export.
