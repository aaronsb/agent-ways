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
  - evidence: "the live refusal in Claude Code 2.1.312, from the mod's debug log in a scratch config: `ways-theme: theme custom:ways-miami-nights refused: takes one of: auto, dark, light, light-daltonized, dark-daltonized, light-ansi, dark-ansi`; `/theme` selects the same custom theme"
  - upstream: "anthropics/claude-code#105739, filed 2026-10-28: a mod's `$.config.set` refuses a custom theme that `/theme` accepts (https://github.com/anthropics/claude-code/issues/105739)"
  - upstream: "Claude Code custom themes: a JSON file in the config directory's `themes/` folder with `name`, `base` (a built-in preset) and `overrides` by colour token, selected as `custom:<file>` in settings.json's `theme`; Claude Code watches the folder and reloads a changed file (code.claude.com, terminal-config, \"Create a custom theme\"; the token list is held at tools/agent-theme/src/target.rs:13-28)"
  - upstream: "Claude Code mods: `ui.render` lets a mod replace or wrap the element the engine draws for a transcript entry, on the surface it reports"
  - evidence: "the code on feature/claude-code-theme-mod at dafbc023: the target mapping and Claude Code export (tools/agent-theme/src/target.rs), roles lifted onto extra grounds (`Roles::derive_on`, tools/agent-theme/src/derive.rs:180), the export command (tools/ways-cli/src/cmd/settings/themes.rs:165), the settings merge's `theme` key (tools/ways-cli/src/cmd/settings_merge.rs), the default theme file laid down by repair (tools/ways-cli/src/cmd/reconcile.rs), the mod (plugins/ways-theme/hooks/register.ts, lib.ts), and `ways-theme` in the mod manager (tools/ways-cli/src/cmd/attend_mod.rs)"
  - evidence: "Claude Code paints no ground under the transcript, and a mod has no call that writes OSC 11, so exported text colours sit on whatever background the terminal has"
  - upstream: "prior art: the looks and skins Claude Code theme projects (MIT), read for their approach only; no code is borrowed"
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
observable:
  - 'try: with Miami Nights active in ways settings, install the mod from the mod tab and start a Claude Code session; the prompt border is neon pink, Claude''s text and the spinner are in the theme''s accent, and a reply sits in a violet frame with a lavender border'
  - 'try: change `theme.active` to Tampa and start another session; the open session and the new one both redraw in Tampa''s colours without a restart'
  - 'run: `ways settings theme export miami-nights --roles` prints every role with the floors applied; `--claude-code` prints a custom theme whose every text token reads at 4.5:1, and every muted token at 3:1, on the theme''s bg and on the ground band'
  - 'try: with `/theme` set to light, install the mod: Claude Code shows the ways theme; uninstall it: Claude Code is back on light and `themes/ways.json` is gone'
  - 'try: with `ways` removed from PATH, start a session with the mod installed; Claude Code starts with the theme file as it was, and the debug log has one ways-theme line'
  - 'see: with the frame layer on, ctrl+o on a framed tool group shows the engine''s own expanded view, unframed'
  - 'run: on a Claude Code below 2.1.309, `ways manage mod install ways-theme` reports the install pending and no theme change; with `--commit`, settings.json keeps its theme and no `themes/ways.json` is written'
status: proposed
date: 2026-10-28
deciders:
  - aaronsb
related: [ADR-513, ADR-504, ADR-512, ADR-406, ADR-500]
---

# ADR-514: Claude Code follows the active theme through the ways-theme mod, over externalised theme layers

## Summary

- **Decided:** A Claude Code mod, `ways-theme`, makes Claude Code follow the theme chosen in ways settings. The theme is chosen in one place, `theme.active`; the mod has no setting of its own and is installed or removed through the mod manager, off by default. Themes are split into three layers: the palette (the `.theme` file), the roles derived from it with the contrast floors applied, and per-target mappings from roles to another program's colour keys, which a theme file can pin. Claude Code gets one fixed custom theme, `custom:ways`, whose file the mod rewrites at session start; installing the mod selects it over the person's theme and uninstalling gives theirs back. On the terminal surface the mod also wraps replies and tool groups in the theme's ground and a border. A missing, failing or slow `ways` does nothing.
- **Trades away:** Four columns of every framed row on the terminal. The person's own Claude Code theme while the mod is installed, though it comes back on uninstall. The theme's gradient, border and rule glyphs, marks and ornament, which Claude Code's chrome cannot take. Exact colours: text roles are lifted to read on terminal backgrounds the theme does not paint, so they can sit further from the file's values than on the theme's own screens. Selection through settings.json and repair, where a supported mod call would be simpler, until Claude Code lets a mod select a custom theme.
- **One-way?** No. Reversible: uninstalling the mod restores the person's theme and removes the theme file, and the theme layers are additions to the theme file format that a theme need not use.
- **Probes:** *Confident (one place):* you want the theme picked once, in ways settings, and Claude Code to follow it with no theme setting of its own in the mod. *Not confident (web later):* you are content that the web surface gets no frame and only what Claude Code's custom theme gives it, until the terminal surface has settled.
- **Inversion:** Between a Claude Code theme maintained by hand beside each agent-ways theme, and Claude Code drawn entirely by agent-ways. The record sits between them: agent-ways supplies colours through Claude Code's own theme file and wraps only the transcript entries a mod may render.

## Context

ADR-513 has every agent-ways surface follow `theme.active`. Claude Code, the program those surfaces sit beside, keeps its own theme, so a session under Miami Nights draws agent-ways in neon violet and Claude Code in its default dark.

Claude Code takes custom themes: a JSON file in `themes/` naming a built-in base and overriding colour tokens, selected in settings.json as `custom:<name>`. It watches the folder and reloads a changed file. A mod's `$.config.set` refuses a custom theme, which `/theme` accepts (anthropics/claude-code#105739). Claude Code paints no ground under the transcript, and a mod cannot set the terminal's background, so its text lands on whatever background the terminal has. A mod's `ui.render` can wrap the element the engine draws for a transcript entry.

An agent-ways theme holds slots; the roles every surface draws are derived from them with the contrast floors applied (ADR-504 §4). A second program needs those roles mapped to its own keys, and a theme may want a key the mapping does not give.

## Decision

1. **Three theme layers.**
   - *Palette:* the `.theme` file's slots and keys (ADR-504 §4, ADR-513).
   - *Roles:* derived once from the palette with the floors applied, the same roles every agent-ways surface draws. `ways settings theme export <name> --roles` prints them as JSON, so another program can read them without agent-theme.
   - *Targets:* a default mapping from roles to each target program's keys, held in agent-theme (`target.rs`), with each key's contrast floor. A theme file may carry a `[target.<name>]` table of `key = "#rrggbb"`, and an entry there wins over the mapping. A key the target does not list is a parse error. Claude Code is the one target; `ways settings theme export <name> --claude-code` prints its custom theme.

2. **Roles on a ground agent-ways does not paint.** For a target that draws on the terminal's own background, the roles are derived again (`Roles::derive_on`) with every text role lifted to its floor on the theme's bg, its selection ground, and a band of common terminal grounds of the theme's kind: black and `#2a2a2a` for dark, white and `#e8e8e8` for light. The gradient is dropped, since the target does not paint it. A background token, such as a diff or a message background, is shaded only as far as the body text still reads on it.

3. **One fixed Claude Code theme.** Claude Code has one custom theme for agent-ways, `custom:ways`, in `<config dir>/themes/ways.json`. At the start of an interactive session the mod reads `theme.active` through `ways settings get`, exports that theme with `--claude-code`, or Claude Code's own dark for `terminal`, and rewrites the file when its content changed. Claude Code reloads it live, so every open session follows. A headless session changes nothing.

4. **The mod manager installs it.** `ways-theme` is a mod under `mods.ways-theme` (ADR-512), installed from the mod tab in ways settings or with `ways manage mod install ways-theme --commit`. It defaults to not installed. The mod adds no setting: its theme is `theme.active`. Its Claude Code floor is 2.1.309, the release it was seen running on, whose engine wrote the type declarations that carry every call it makes. The floor is checked at install, so a Claude Code downgraded afterwards is not caught.

5. **Selection through the settings merge.** While `mods.ways-theme.install` holds and Claude Code lists the mod installed in that target, the settings merge that repair and the mod manager run (ADR-500) owns settings.json's `theme` key there. A wanted mod still pending, on a Claude Code below its floor or with no `claude` on PATH, claims nothing.
   - Installing claims the key for `custom:ways` and keeps the value it held. The claim is made even over a theme the person chose. A live `custom:ways` is never kept as that value.
   - A value the person sets after the claim is their later choice: kept, and neither claimed again nor replaced on uninstall.
   - Uninstalling gives back the kept value, or removes the key when it held none, and removes `ways.json`. A live `custom:ways` with the merge's record lost is the mod's own and is removed the same way.
   - Each install, uninstall and repair what-if names the key's value before and after, and `ways.json` written or removed.
   - Repair lays down a default `ways.json` before it selects it, so the folder watch holds from the first session.
   - The mod itself selects nothing, because `$.config.set` refuses a custom theme. When Claude Code lets a mod select one, selection moves into the mod and the merge gives up the key.

6. **The frame layer.** On the terminal surface, the mod's `ui.render` wraps the element the engine draws for an assistant message (`AssistantMessage`), a tool use (`ToolUse`) and a tool group (`ToolGroup`) in the theme's ground with a border, which costs four columns per row. It hands back the engine's own element, unframed, in the ctrl+o transcript, for a tool row expanded on its own, in verbose mode, or for a very long reply. The ctrl+o transcript is read from the line that closes a turn (`TurnDuration`), the one row whose `isExpanded` describes the whole view. On any other surface, the web surface included, it does nothing.

> Note, 2026-10-28, after review of the mod branch: the claim waited only on `mods.ways-theme.install`, so a Claude Code below the floor kept no mod and still lost its theme; item 5 now waits on the mod being installed. The floor was 2.1.312, resting on `$.config.set`, which the mod does not call to select a theme; item 4 now sets it at 2.1.309. ctrl+o was read from a person's composer row, whose `isExpanded` is that row's own drawing under a speaker label; item 6 now reads the turn line. A pin in a theme's `[target.claude-code]` table is exported as given and is now checked at its token's floor, as the mapping's colours are.

7. **Fail-safe.** Each `ways` call is bounded at 1.5 s. A missing `ways`, a non-zero exit, output that does not parse, a call past its bound, or a refused write leaves `ways.json` as it was and writes one line to the debug log. A failure in the frame layer hands back the engine's element.

8. **What Claude Code cannot take.** A theme's gradient, borders, rule glyphs, marks, presence, event glyphs, waypoints and ornament reach agent-ways' own surfaces only. Claude Code takes colours through its tokens, and the frame layer's ground and border. Its chrome outside the transcript entries keeps its own glyphs and layout.

The figure for this record is two screenshots of Claude Code 2.1.312 under Miami Nights, taken during the work and described here because the corpus keeps figures as SVG. The first shows the welcome block and a typed prompt: the prompt box's border is neon pink, the header and status text are lavender, and the transcript sits on the terminal's own near-black. The second shows the frame layer's prototype: the user's prompt on a violet bar, each reply in a violet block inside a rounded lavender border, and a collapsed tool line ("Listed 1 directory (ctrl+o to expand)") left unframed between them.

## Consequences

### Positive

- One choice in ways settings themes agent-ways and Claude Code together, and a change reaches open sessions live.
- Another program can follow agent-ways themes by reading the roles export or adding a target mapping, without its own copy of the palettes.
- A theme can pin any Claude Code token where the mapping falls short, in the same file as its palette.
- Text stays readable on common terminal backgrounds the theme does not paint.
- A broken or missing `ways` never breaks Claude Code.

### Negative

- Framed rows lose four columns, which narrow terminals feel first.
- The frame layer depends on the element names and props Claude Code's engine passes to `ui.render`; a Claude Code release that changes them leaves entries unframed until the mod follows.
- Selection runs through settings.json and repair, so a person who sets `/theme` by hand after the claim keeps their choice, and the mod's colours apply only while `custom:ways` is selected.
- Installing the mod replaces the person's theme until uninstall, which a person who chose their own theme may not expect.
- Lifting onto the ground band moves text colours further from a theme's own values, most for themes whose bg is far from black or white.
- Each session start runs up to two `ways` calls, bounded at 1.5 s each, before the theme file is current.

### Neutral

- The web surface gets whatever Claude Code's custom theme gives it, and no frame.
- `[target.<name>]` is a table in the theme file, as `[[waypoints]]` is (ADR-513 item 15), so a theme file that uses it no longer sources as shell.
- The mod carries the `ways` version, since it calls `ways settings` verbs.
- The prior art read for this work, the looks and skins projects (MIT), supplied ideas only; no code is borrowed.

## Alternatives Considered

- **A hand-written Claude Code theme beside each agent-ways theme.** Two files per theme drift apart, and a user theme would need its Claude Code twin written by hand. A mapping derives both from one palette.
- **The mod selects its theme with `$.config.set`.** Claude Code 2.1.312 refuses a custom theme there. Filed as anthropics/claude-code#105739; selection moves into the mod once the call accepts one.
- **One custom theme file per agent-ways theme.** Switching themes would change the selected name in settings.json, which the mod cannot write. One fixed name whose content changes needs settings.json touched only on install and uninstall.
- **Keep the person's theme on install** ("Keep theirs", declined by the operator). The mod would do nothing visible for anyone with a theme already chosen.
- **Ship the frame layer in a later change** ("Later, own PR", declined by the operator), **or drop it** ("Drop it", declined by the operator). The colours alone leave replies on the terminal's own background, which is the look the operator asked the mod to change.
- **Set the terminal's background with OSC 11.** A mod has no call that writes to the terminal outside the engine's rendering, and a background changed under the person's terminal would outlive the session.
- **Export only the roles and let each target map them.** Every target would carry its own copy of the floors. The mapping in agent-theme applies them once, and the roles export stays for programs agent-ways does not map.
