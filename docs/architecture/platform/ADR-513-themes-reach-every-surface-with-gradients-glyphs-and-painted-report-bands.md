---
contract: adr/v1
kind: decision
verb: add
capability: cli
extends: [ADR-504]
basis:
  - precedent: ADR-504
  - operator: aaronsb
    level: guided
    said: "let's look over the 'themes' for ways and attend. we have one light mode, I'd like to add a new one called Florida. let's think about what that could look like"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: guided
    said: "if it's possible to include some other fun decoration customizations like border shape glyphs and corners etc, that would be helpful"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: guided
    said: "most of the themes are pretty much just 'looks good' but as an experiment, maybe we could include palm trees or beach stuff or something"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: directed
    said: "I think rich vibrant colors, a Miami and a Tampa theme. and Miami nights (so three in total) (miami nights is sort of retro future palm trees. we can take advantage of background colors and gradients too"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: directed
    said: "vertical and horizontal gradients"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: guided
    said: "south beach ideas can be incorporated as appropriate too - don't want to loose that"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: directed
    said: "let's make sure that the crate is updated to fulfill the theme contract so the others can get the capabilites (we just don't have to implement them in the other themes yet)"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: guided
    said: "I know Hiroshi Nagai is more of a west coast scene painter but I think about those colors"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: directed
    said: "tampa needs some color differentiation too - whats it known for?"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: directed
    said: "'ways session live' (the session viewer) should also get the themes too"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: directed
    said: "and reports etc"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: directed
    said: "for instance you can 'view' a way and read it. the reader should follow the theme"
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: directed
    said: "we use a glyph identifier for events in the timeline view. let's get the glpyh set out of a hard coded set and allow us to substitute a different one, per theme."
    via: "operator, in the session that commissioned ADR-513"
  - operator: aaronsb
    level: directed
    said: "the only ones we want to change would be the florida theme ones for now"
    via: "operator, in the session that commissioned ADR-513"
  - evidence: "the glyphs a theme cannot change, at af0b8bfe: panes are drawn with ratatui's plain border (tools/agent-tui/src/app/render.rs:19-20) and attend-chat's with the rounded one (tools/attend-chat/src/app/view.rs:263, :289); the rule and selected rule are `─` and `━` (tools/agent-tui/src/feed.rs:213, timeline.rs:227-229); the selected-row mark is `▌` (tools/agent-tui/src/app/theme.rs:297); presence is `●` and `○` (tools/attend-chat/src/app/sidebar.rs:403-404, :478, :507)"
  - evidence: "at 613dc9ea nothing outside agent-theme calls `agent_theme::install` (tools/agent-theme/src/paint.rs:511), so plain output paints with `Painter::detect`, the default theme at the environment's depth (paint.rs:505-507); the chosen theme reaches only the screens, which read `theme.active` themselves (tools/ways-cli/src/cmd/screen_host.rs:54, tools/attend-chat/src/theme.rs:46)"
  - evidence: "at 613dc9ea the event glyphs are hard-coded where they are drawn: the timeline's head and mark (tools/agent-tui/src/timeline.rs:221-229), each fate's mark in the introspect table (tools/ways-cli/src/cmd/introspect/table.rs:55-64) and the suppressed and re-disclosed notes (table.rs:296-303)"
  - evidence: "at af0b8bfe the contrast derivation lifts text roles against two grounds, bg and the selection (tools/agent-theme/src/derive.rs:178-180), and the parser rejects any key it does not know (tools/agent-theme/src/text.rs:191-200)"
  - evidence: "while the bundled themes were drawn, a saturated blue sky stop for Miami was tried; lifting every status colour to 4.5:1 across it darkened them all to dull values, so the stop was dropped for a pale sky (relayed by the calling session's brief, 2026-10-28)"
  - standard: "WCAG 2.2, success criteria 1.4.3 (text contrast 4.5:1) and 1.4.11 (non-text contrast 3:1): the floors ADR-504 §4 holds text and muted roles to"
  - standard: "OKLab (Björn Ottosson, 2020): a perceptual colour space in which a straight line between two colours changes lightness evenly; agent-theme already works in OKLab and OKLCH (tools/agent-theme/src/oklab.rs)"
  - standard: "Unicode UAX #29 (grapheme clusters) and UAX #11 (East Asian Width): what one user-perceived character is, and how many terminal cells it takes"
  - upstream: "ratatui 0.30 (ratatui-core 0.1.2): a border is a `symbols::border::Set` of eight strings, the four corners and the top, bottom, left and right edges, and `BorderType` names Plain, Rounded, Double and Thick among its sets"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Keep glyphs"
    via: "operator, multiple-choice prompt in the commissioning session; option label and text written by the agent, chosen by the operator; question: \"On a 16-colour terminal, or with NO_COLOR set, today's rule (ADR-504) swaps any theme for the plain default theme, so Tampa's waves, ⚡ and other decoration would disappear along with its colours. Should the decoration glyphs survive there?\"; option text: \"Borders, rules, marks and ornament still draw on 16-colour or NO_COLOR terminals. Colours fall back as they do now. Glyphs don't depend on colour depth.\" The record was revised for it (Summary, Decision items 2 and 9, Neutral consequences)"
    covers: [lower depths]
  - operator: aaronsb
    said: "Yes, unchanged"
    via: "operator, multiple-choice prompt in the commissioning session; option label written by the agent, chosen by the operator; question: \"A theme that doesn't set the new keys (a bundled one or your own) is meant to look exactly as it does today on every screen. Is that right?\""
    covers: [looks unchanged]
  - operator: aaronsb
    said: "Paint report blocks"
    via: "operator, multiple-choice prompt in the commissioning session; question and option text written by the agent, option chosen by the operator; not a Summary probe; question: \"Plain-text reports (`ways status`, the tables from agent-fmt, ways-audit output) never pick up the chosen theme today: nothing installs it, so they always print in the default colours. Miami, Tampa, Miami Nights and Paper paint their own background, but plain output prints on your terminal's background, where a light theme's text colours may not read on a dark terminal. How should plain reports use these themes?\"; option text: \"When output goes to a terminal, each report line is painted with the theme's background (and its horizontal gradient) out to the line's end, so a report shows as a band of the theme. The colours read because they sit on the ground they were tuned for. Piped output stays uncoloured, as now.\"; options not chosen: \"Colours only\" (the theme's text colours on the terminal's own background) and \"Terminal themes only\" (only themes on the terminal background apply to reports). The record was revised for it (Decision items 11 and 12)"
  - operator: aaronsb
    said: "Yes, rules and marks"
    via: "operator, multiple-choice prompt in the commissioning session; question and option text written by the agent, option chosen by the operator; not a Summary probe; question: \"Should plain reports also take the theme's glyphs (for example Tampa's ~ table rules)?\"; option text: \"Table rules use the theme's rule glyph, and live/idle markers use its presence glyphs. Borders and the ornament stay TUI-only.\" The record was revised for it (Decision item 13)"
  - operator: aaronsb
    said: "we can adjust paper since it defines a custom background"
    via: "operator, in chat in the commissioning session; answering the coordinator's question, not a Summary probe: \"Under Paper, the one existing theme that paints its own background, `ways status` and the other reports will print as a cream band instead of default colours. Under the other existing themes, which leave the terminal's background alone, reports change colour only. Piped output stays as it is everywhere. Is that what you want for Paper, or should reports under Paper stay as they are today?\" The record was revised for it (Consequences, Positive)"
  - operator: aaronsb
    said: "let's accept the adr and the get through making the improvements, review and build, then get through publishing too"
    via: operator, in chat in the commissioning session, after every probe was answered
observable:
  - 'see: the seven bundled themes that predate this record draw the frames they drew before; their golden frames do not change'
  - 'see: Miami on a truecolor terminal paints a pale sky at the top falling to blush stucco at the bottom, with double borders and the flamingo before each pane title; under TERM=xterm-256color with COLORTERM unset it keeps the borders and the flamingo on the flat THEME_BG'
  - 'see: Tampa under NO_COLOR=1, and on a 16-colour terminal, draws its wave border, wavy rules, `›` mark and lightning bolt in the default theme''s colours'
  - 'run: cargo test -p agent-theme refuses a theme file with a gradient and THEME_BACKGROUND="terminal", with one stop or five, with THEME_BORDER and THEME_BORDER_GLYPHS both, with a two-cell glyph among the border glyphs, and with three presence glyphs, each naming its line'
  - 'run: for every bundled theme with a gradient, each text role meets 4.5:1 and each muted role 3:1 against every stop and every sample between stops'
  - 'see: with Tampa active, `ways status` on a truecolor terminal prints as a band running from bay blue at the left to sunset at the right, its table rules drawn with `~`; `ways status | cat` prints as it did before'
  - 'see: with Tampa active, the session viewer (`ways session live`) and a way opened to read draw on Tampa''s gradient'
  - 'see: with Miami active, the session timeline marks the head with `☀` and an injected way with `❀`, and a blocked way still with `⊘`; with Nord active, the timeline draws the glyphs it draws today'
  - 'try: copy Tampa in the theme tab, change one colour and save; the saved file keeps its gradient, wave border, rule glyphs, mark, presence glyphs and ornament'
status: accepted
date: 2026-10-28
deciders:
  - aaronsb
related: [ADR-504, ADR-503]
---

# ADR-513: Themes reach every surface, with gradients, glyphs and painted report bands

## Summary

- **Decided:** A theme file may set a ground gradient across the whole frame, the border, the rule, mark and presence glyphs, and an ornament before pane titles. Each key is optional; a key left out keeps the surface's own default, so every existing theme and dotfiles palette draws as it does now. The gradient is painted only at truecolor depth, and every text role must meet its contrast floor on every colour the gradient paints. The glyphs and the ornament draw at every colour depth; below 256 colours they take the default theme's colours. Three bundled themes use the keys: Miami, Tampa and Miami Nights. Every surface that shows content follows the active theme, plain reports included: on a terminal, a report under a theme that paints its own ground prints as a band of that ground, with the theme's rule and presence glyphs. A theme may also replace each event glyph of the session timeline and the introspect screens, and the three Florida themes replace six of them. This extends ADR-504 §4 and §5 and completes §9's move of the plain outputs.
- **Trades away:** A theme file that only sets colours. Decoration is part of the theme a user chooses, so a user who likes Tampa's colours and not its waves edits a copy; no setting overrides a theme's decoration yet. A theme file with the new keys fails to load in a build older than this change. Status colours on a gradient theme may be lifted further from the values written in the file than on a flat ground. Plain reports printed to a terminal change under every chosen theme, including ones without the new keys.
- **One-way?** No. Reversible now; expensive once user themes carry the keys, because removing a key makes those files fail to parse.
- **Probes:** *Confident (looks unchanged):* you want a theme without the new keys, bundled or your own, to look exactly as it does today on every surface. *Not confident (lower depths):* on a 256-colour terminal a gradient theme shows its flat THEME_BG and keeps its borders, marks and ornament; on a 16-colour terminal, or under NO_COLOR, the plain default theme is used in its place, so Tampa's waves and lightning bolt disappear along with its colours.
- **Inversion:** Between decoration as part of a theme's identity, one file and one choice, and decoration as a user preference set apart from the palette, as the status-line segment shape is. This record takes the first end and leaves room for the second as an override.

## Context

ADR-504 §4 fixed the theme file at the dotfiles palette's keys plus agent-ways' own: ten colour slots, the background mode, and four optional pinned roles. A theme changes colour and nothing else. Every border, rule, selected-row mark and presence dot is a glyph the surface picks, and the surfaces do not agree: `agent-tui` panes use the plain border and attend-chat's use the rounded one.

Three bundled themes were drawn to carry a place, Miami, Tampa and Miami Nights, and each needs a ground that changes across the frame and glyphs of its own.

A gradient changes the ground every text role is read against. ADR-504 §4 lifts each text role to its contrast floor against two grounds, bg and the selection. On a gradient those two no longer cover what is under the text.

The chosen theme reaches the screens only. The plain outputs moved onto `agent-theme` under ADR-504 §9, but no binary installs the chosen theme as the process painter, so `ways status`, agent-fmt's tables and ways-audit print in the default theme's colours whatever is active. Printing a light theme's text colours on a dark terminal's own background would leave them below their contrast floors.

## Decision

1. **Optional keys.** Each key below is optional. A theme that leaves one out gets the surface's own default for it, so a theme with none of them draws exactly as before. The keys follow ADR-504 §4's style: bare uppercase `THEME_*` names with double-quoted values.

2. **`THEME_GRADIENT`.** The value is a direction, `vertical` or `horizontal`, then two to four `#rrggbb` stops separated by spaces, for example `THEME_GRADIENT="vertical #d4efff #fff4ee #ffe6ee"`.
   - The stops are spaced evenly from the frame's first row to its last (vertical) or its first column to its last (horizontal), and a cell's ground is the colour interpolated in OKLab at its row or column.
   - A cell with a ground of its own, such as the selection, a lozenge or a button, keeps it. Every other cell shows the gradient at its position. Dialogs and popups have no flat ground of their own, so the gradient shows behind them.
   - The gradient requires `THEME_BACKGROUND="fill"`; a gradient with `terminal` is a parse error.
   - It is painted only at truecolor depth. At 256 colours the frame is filled with `THEME_BG`, which stays required. Below 256 colours item 9 applies.
   - The ANSI output for plain commands paints no gradient.

3. **Contrast on a gradient.** The grounds the derivation lifts against are bg, the selection, each gradient stop, and seven points evenly spaced between each pair of neighbouring stops. Each text role is lifted to 4.5:1 and each muted role to 3:1 against all of them, by the method of ADR-504 §4. The theme editor's contrast results cover the same grounds and name the ground a failing role fails on.

4. **The border.** `THEME_BORDER` names a set: `plain`, `rounded`, `double` or `thick`. `THEME_BORDER_GLYPHS` gives eight glyphs in this order: top-left, top-right, bottom-left and bottom-right corners, then the top, bottom, left and right edges. Tampa's `╭╮╰╯~·││` draws a rounded box with waves along the top and a dotted bottom. Setting both keys is a parse error. The theme's border is the one every pane and dialog draws; a dialog that names a border of its own (ADR-504, note of 2026-10-13) keeps it.

5. **Rule, mark and presence glyphs.**
   - `THEME_RULE_GLYPHS` is two glyphs, the rule and the selected rule. The default is `─━`.
   - `THEME_MARK` is the selected-row mark. The default is `▌`.
   - `THEME_PRESENCE` is two glyphs, live and idle. The default is `●○`.

6. **The ornament.** `THEME_ORNAMENT` is one grapheme of display width 1 or 2, an emoji included. It is drawn before each pane title, followed by one space. When a title is cut to fit, the ornament is dropped before any of the title's text.

7. **Glyph checks.** Every glyph except the ornament is one grapheme cluster (UAX #29) of display width 1, measured by the width rule ratatui uses. A glyph of any other width, a glyph that is more than one grapheme, or a key with the wrong count of glyphs is a parse error naming its line.

8. **Writing back.** `to_text` writes every decoration key the theme carries, after the pinned roles, so a theme saved from the theme editor keeps its decoration. Editing the decoration in the theme editor is outside this decision.

9. **Decoration at every colour depth.** Below 256 colours, on a 16-colour terminal or under `NO_COLOR`, the default theme's colours are used in place of the chosen theme's, as ADR-504's note of 2026-10-01 says, and the chosen theme's border, rule, mark and presence glyphs and its ornament keep drawing. That note put the whole default theme in place of the chosen one; for decoration this record narrows it to colours.

10. **Bundled themes.** ADR-504 §5's bundled set gains three themes original to agent-ways, each with `THEME_BACKGROUND="fill"`. The figure below shows each as a pane, drawn from the files with the slot colours as written, before lifting.
   - **Miami** (light): a vertical gradient from a pale Hiroshi Nagai sky to South Beach blush stucco, flamingo accent, pool-turquoise info, double borders, `🦩`.
   - **Tampa** (light): a horizontal gradient from the bay at the left (`#e8f3fb`) through a warm cream (`#fff6e8`) to the sunset at the right (`#ffe4d2`); Lightning blue accent (`#0057b8`), Ybor tile teal info (`#00818a`), creamsicle orange warn (`#e35200`), Buccaneers red err (`#c8102e`), Gasparilla purple alt (`#6c2c91`), Skyway-cable gold subtle (`#f5d98a`), and a pinned pale-blue selection (`#d8e8f8`); the wave-topped rounded border `╭╮╰╯~·││`, `⚡`.
   - **Miami Nights** (dark, retro-future): a vertical violet-to-magenta horizon gradient, neon pink, cyan and mint, thick borders, `🌴`.

![Three terminal panes side by side. Miami: a light pane whose ground runs from pale blue at the top to pink at the bottom, double-line borders, a flamingo before the title, diamond presence marks. Tampa: a light pane whose ground runs from pale bay blue on the left through cream to sunset peach on the right, a rounded border with tildes along the top and dots along the bottom, a lightning bolt before the title, a pale-blue selected row, a blue selected rule of `≈`. Miami Nights: a dark pane running from near-black violet at the top to magenta at the bottom, thick borders, a palm before the title, neon pink, cyan and mint text.](figures/ADR-513-decorated-themes.svg)

11. **Every surface follows the active theme.** The settings screens, the session viewer (`ways session live`) and the session screens, a way opened to read, attend-chat, and every plain report draw from the theme named by `theme.active`. Each binary that prints plain output installs that theme as the process painter (`agent_theme::install`) at the start of `main`, after it has found that output is styled. It reads `theme.active` only then, so piped output and the hook commands of ADR-504 §11, whose output is never styled, read no theme file.

12. **Report blocks.** Under a theme with `THEME_BACKGROUND="fill"`, a plain report goes through an `agent-theme` `block()` that paints the theme's ground behind every line out to the terminal's width.
   - A vertical gradient spans the block's lines, top to bottom, and a horizontal one spans the terminal's width, left to right.
   - The depth rules are the screens' rules: the gradient at truecolor, the flat `THEME_BG` at 256 colours, and below 256 the default theme's colours, which paint no ground.
   - A theme with `THEME_BACKGROUND="terminal"` paints no ground, and its colours print on the terminal's own background.
   - Unstyled output, such as a pipe or a file, is unchanged.

13. **Report glyphs.** A report's table rules use the theme's rule glyph, and its live and idle markers use the theme's presence glyphs, at every colour depth where output is styled. Borders and the ornament stay on the screens. Unstyled output keeps the default glyphs.

14. **Event glyphs.** Each glyph the session timeline and the introspect screens use to mark an event is an optional theme key, one glyph of display width 1 under item 7's checks. Its default is the glyph drawn today, held in `agent-theme`, so a theme without the key draws as before.
   - `THEME_EVENT_HEAD` `●`, `THEME_EVENT_MARK` `┼`, `THEME_EVENT_MATCHED` `◆`, `THEME_EVENT_INJECTED` `◇`, `THEME_EVENT_SUPPRESSED` `⊝`, `THEME_EVENT_REDISCLOSED` `↩`.
   - `THEME_EVENT_WOULD_BLOCK` `◌`, `THEME_EVENT_BLOCKED` `⊘`, `THEME_EVENT_REFIRE_HELD` `◷`, `THEME_EVENT_CAP_HELD` `⊟`.
   - `THEME_EVENT_DUE` takes two glyphs, due and not due, `●○`.
   - Each event has its own key, so a theme names the events it changes and leaves the rest at their defaults.
   - The three Florida themes set the six keys of the first line and leave the warning glyphs (would-block, blocked, the two held, due) at their defaults, so a warning reads the same under every theme. Head, mark, matched, injected, suppressed and re-disclosed are Miami's `☀ ┊ ✿ ❀ ☂ ↻`, Tampa's `↯ ┊ ⚔ ✦ ☈ ⟲`, and Miami Nights' `◓ ┊ ✦ ✧ ☾ ↻`.

## Consequences

### Positive

- A theme can carry a place or a mood in its ground and glyphs, and the bundled set shows three.
- Every existing theme, bundled or user-owned, and every dotfiles palette loads and draws its screens unchanged. That holds for the screens only: plain reports follow item 11 under every theme, and under Paper, which paints its own ground, a report prints as a band of it.
- Readability on a gradient is held by the same derivation as on a flat ground, so a user theme with a gradient cannot leave text below its floor.
- A bad glyph is refused when the file is read, so a frame never misaligns from a theme.
- A report reads in a light theme on a dark terminal, because its text sits on the ground the theme was tuned for.
- One theme choice reaches the screens and the plain reports alike.

### Negative

- A build older than this change refuses a theme file with the new keys as an unknown key. A user theme shared between machines on different versions loads on one and fails on the other.
- Status colours on a gradient are lifted against more grounds, so they can sit further from the colours the file names. A stop far from the text roles in lightness makes this worse.
- `agent-theme` measures grapheme clusters and display width, which adds Unicode segmentation and width to a crate with no ratatui dependency.
- Glyphs of ambiguous East Asian width, such as Miami's `◆◇` and Tampa's `≈`, measure width 1 and pass; a terminal set to treat ambiguous characters as wide draws them two cells across and misaligns the frame.
- Several of the Florida event glyphs, such as `☀`, `☂` and `⚔`, have text presentation by default but render as colour emoji in some fonts, where they lose the role colour and can take two cells.
- An emoji ornament depends on the terminal's font. A terminal that draws a two-cell emoji in one cell, or with no colour glyph, shifts or blanks the title.
- Golden frames for surfaces drawn under a decorated theme need recording, and the theme editor's contrast view grows a row per ground.
- A plain report on a terminal now changes with the active theme, including under bundled themes that predate this record, such as Paper. Report tests that compare styled output pin the painter.
- A report under a vertical gradient is painted only once its line count is known, so the block holds its lines until the report ends. A report under a horizontal gradient or a flat ground can print line by line.
- A report painted to the terminal's width leaves a band that a terminal resized later does not repaint.
- Each binary with plain output reads the settings and the theme directory once when its output is styled, which adds that read to the start of an interactive command.

### Neutral

- A surface reads the chosen theme's decoration at every depth, including where its colours are unused, so below 256 colours a frame takes its glyphs and its colours from two themes.
- The new keys are agent-ways' own in the dotfiles style. ADR-504's warning stands: a later dotfiles key of the same name would collide.
- A named glyph set that themes refer to, shared between themes, can be added later in the same way as a user decoration setting; the event keys stay as they are.
- A user decoration setting that overrides the theme's, as `theme.shape` does for the lozenge caps (ADR-504, note of 2026-10-01), can be added later without changing these keys.

## Alternatives Considered

- **Decoration as a user setting apart from the palette,** as the segment shape is. It lets a user pair any palette with any borders. A theme could then not carry a place in one choice, and the bundled themes would ship as palettes with advice. Deferred: an override setting can come later and leave the theme keys as they are.
- **A saturated Nagai-blue sky stop in Miami.** Lifting every status colour to 4.5:1 across it darkened them all to dull values. Miami's sky stop is pale instead.
- **Interpolating in sRGB.** A straight line in sRGB between two hues passes through muddier, darker midpoints, and agent-theme already measures and lifts in OKLab and OKLCH.
- **Quantizing the gradient to 256 colours.** Neighbouring rows would snap to the same palette entry and band, and the contrast checks would run against colours the theme never named. The flat `THEME_BG` is a ground the derivation already handles.
- **Reports take the theme's text colours on the terminal's own background** ("Colours only", declined by the operator). A light theme's colours would print on a dark terminal below their contrast floors, and the reverse on a light one.
- **Only themes on the terminal's background apply to reports** ("Terminal themes only", declined by the operator). Reports would ignore every theme that paints its own ground, the three in this record among them.
- **One positional string for every event glyph,** as `THEME_BORDER_GLYPHS` is for the border. A position names no event, a theme must restate every default to change one glyph, and adding an event later shifts or extends every theme's string. One key per event names its event and lets a theme change a subset.
- **A registry of named glyph sets that themes refer to.** Deferred, like the user decoration setting: the Florida themes are the only ones that change event glyphs, and a registry can sit over the keys later.
- **Free-width glyphs, measured at draw time.** A two-cell border glyph breaks every box it sits in, and catching it at draw time means each surface handles a bad theme. One check when the file is read covers every surface.
