---
contract: adr/v1
kind: decision
verb: change
capability: [cli, attend]
amends:
  - ADR-154#2
  - ADR-154#4
basis:
  - operator: aaronsb
    level: directed
    said: I'd like to look over the ways cli and see if we can consolidate some of the commands. for example, settings could be more of a tree, and honestly, a univseral module or crate for interacting in a tui (as an option to setting properties etc)
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: directed
    said: seems like we're using rust and a maintained library would be better.
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: authored
    said: let's make it so apply(save), discard settings is per tab. we can add a guard to exit, so someone doesn't loose their settings (from any tab)
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: directed
    said: we don't need to integrate with statusline, instead just lift some themes. we can keep our own toml file or resource (external) that can create/manage themes
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: authored
    said: theme works a little different than settings changer - it's a preview right in the ui, and saves indpendently from the application config
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: directed
    said: attend could also use the same system - so let's see if we can re-use the same config model and ux conventions across both (they have quite a bit of shared code already)
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: authored
    said: one app
    via: chat, session 02e97f86, 2026-10-01, asked whether ways and attend share one settings app or each get their own on the same engine
  - operator: aaronsb
    level: directed
    said: ideally, attend and ways would follow this theme. ways binary has tools such as 'rethink' to see an agent's disclosure over time. I'd like to make sure these themes are consistent across all those other interactive systems too
    via: chat, session 02e97f86, 2026-10-01
  - evidence: 'no shared palette across ways and attend: ok/warn/err exist as four copies with three different codes, selection is drawn three ways, and only agent-identity honours NO_COLOR'
  - evidence: rethink is about 2,900 lines of hand-rolled crossterm plus about 1,000 lines of compositor and render code; attend-chat uses iocraft
  - evidence: the dry-run spike tools/spikes/settings-tui (branch spike/settings-tui) loaded the real ways files into one typed tree with queued actions, masked key entry, per-tab review and apply, guided flows and a theme tab, under 107 ratatui TestBackend tests; frames reviewed as PNGs
  - evidence: 'roguemap''s TUI test loop: headless snapshot, PNG review, golden frames in cargo test'
  - precedent: ADR-154
  - operator: aaronsb
    level: directed
    said: attend-chat needs to respect themes too
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: authored
    said: so technically that's three apps
    via: chat, session 02e97f86, 2026-10-01, on the settings app, the ways interactive modes and attend-chat sharing one theme
  - operator: aaronsb
    level: guided
    said: and attend will likely get thinner as we expand the ways mcp
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: directed
    said: the interactive modes had some legacy paths to get to them, let's just make sure we've retired the legacy modes
    via: chat, session 02e97f86, 2026-10-01
  - precedent: ADR-189
  - operator: aaronsb
    level: directed
    said: I think attend-chat could move to the same ux framework .
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: guided
    said: this would allow more code reuse
    via: chat, session 02e97f86, 2026-10-01, the reason for moving attend-chat to the same ux framework
  - operator: aaronsb
    level: directed
    said: I want to address items 1-4 now and the two other items
    via: chat, session 02e97f86, 2026-10-01
  - evidence: 'Claude projects and transcripts are located ad hoc: tools/claude-projects (Python, 1,422 lines), ways-core paths.rs:278 and transcript.rs:33, ways-cli session.rs, context.rs, corpus.rs, memory_seed.rs, list.rs and rethink/frames.rs, attend util.rs:35 and cmd/tune.rs, sensor-peers lib.rs:141, and the spike picker (ways.rs:686, :726); the project-path encoder is copied in ten files under three rules'
  - evidence: terminal capability detection exists twice, agent-identity TermCaps::detect (palette.rs:33) and the spike ColorDepth::from_env (themes/depth.rs:20), and they disagree on TERM=dumb and on 256-colour terminals
  - evidence: 'two categorical palettes: ways show PIN_COLORS (ways-cli render.rs:30-41) and agent-identity RICH_PALETTE (palette.rs:84-105), with near pairs red 255,118,117 / coral ff6b6b, teal 78,205,196 / 4ec9b0, sky 116,185,255 / 5ac8fa'
  - evidence: 'raw SGR literals on 236 lines in 24 files: ways-cli 138, ways-audit 56, agent-fmt 26, agent-identity 13, attend-identity-view 2, attend 1; the traffic light is written out in ways-cli render.rs:238-242 and :358-362, context.rs:237-242, ways-audit report.rs:39-44 and agent-fmt permissions.rs:207-209'
  - evidence: hooks run ways scan prompt|state|command|file|task|messages, show way and response-topics-path on prompts, tool calls and stops, ways disable --list and status --json on subagent start, and attend inbox --drain on stop (settings.json hooks; hooks/ways/check-*.sh, inject-subagent.sh, attend-drain-stop.sh)
  - operator: aaronsb
    level: directed
    said: let's settle on rust
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: guided
    said: lightweight hooks allows us to begin to be compatible for an exciting new system for claude code https://claude.com/blog/claude-code-mods
    via: chat, session 02e97f86, 2026-10-01
  - evidence: 'non-Rust tools on these paths: tools/claude-projects (Python, linked onto PATH by scripts/install.sh:224) and the hook adapters hooks/ways/*.sh, of which check-post.sh:49-89 finds and runs postcheck.sh files and decides what to inject in bash; the spike reviewed frames through a Python/PIL script kept outside the repo; tools/scripts/fire-panel.py and probe-measure.py measure matching calibration and sit outside these paths'
  - upstream: 'Claude Code mods: TypeScript functions shipped in plugins, hot-reloaded, that run in-process before, after, instead of or around tool calls, permission requests and prompts, and can draw parts of the UI (https://claude.com/blog/claude-code-mods)'
  - evidence: ADR-505
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "introspect, settings, plain ways is one application. attend-chat is another application, and attend is the next. the reason for this is to keep seperation of purose. on the horizon is moving the ways embedding searcher into the daemon (right now it's just the judge) and stripping down attend binary to an mcp server (or at least, an mcp server mode). attend-chat remains an indpendent application for human tui interaction, and in the future may have a json object mode which would facilitate other programmatic interaction."
    via: chat, session 02e97f86, 2026-10-01, answering the ADR probes
    covers: [one look]
  - operator: aaronsb
    said: "agent ways can vend it's own themes, but let's use the same structure as the dotfiles themes do. if the dotfiles themes are insufficient in properties or configs, we can deviate from dotfiles"
    via: chat, session 02e97f86, 2026-10-01, answering the ADR probes
    covers: [own themes]
observable:
  - 'see: switching the active theme changes the colours of the settings screens, introspect, attend-chat and ways list output'
  - 'see: NO_COLOR=1 removes colour from every ways and attend surface'
  - 'run: cargo test in agent-tui passes its golden frames at 80x25 and the reference size'
  - 'see: ways rethink exits with an unknown-command error, and no doc outside docs/architecture names it'
  - 'see: a theme previewed in the theme tab reverts on Esc and is written only on Enter or Ctrl-S'
  - 'run: a palette file copied unchanged from ~/.dotfiles/theme/palettes/ into the themes directory appears in the theme tab and previews'
  - 'run: grep finds one project-path encoder in tools/, in claude-sessions, and no TermCaps or PIN_COLORS'
  - 'run: hook latency for ways scan prompt, scan command, scan file and attend inbox --drain stays within the budget of §11 against the commit before the change'
  - 'run: ways projects list, search, show, stats, cleanup --dry-run and relocate --dry-run report the same projects, sessions and sizes claude-projects reported on the same ~/.claude, and git ls-files shows no tools/claude-projects'
  - 'run: cargo test -p agent-theme fails on a raw SGR literal planted in a crate outside agent-theme, and passes once it is removed'
status: proposed
date: 2026-10-01
deciders:
  - aaronsb
related: [ADR-503, ADR-153, ADR-189, ADR-501, ADR-502, ADR-184, ADR-505]
---

# ADR-504: One TUI engine on ratatui and one theme for every interactive surface

## Summary

- **Decided:** agent-ways has three applications with separate purposes, sharing one theme. `ways` is one application for settings, `introspect` and the plain ways output. `attend-chat` is an independent application and binary for people. Both run on `agent-tui`, an app shell on ratatui, so `attend-chat` is ported off iocraft for the code reuse. `attend` is being reduced toward an MCP server and has no TUI. Every surface that prints colour, interactive or not, draws it from `agent-theme`, a theme engine with no ratatui dependency and outputs for ratatui and plain ANSI. A theme file has the structure of the operator's dotfiles palettes, which is valid TOML, so a palette file loads unchanged; agent-ways adds keys in that style only where the structure is insufficient. Themes are bundled and user-owned, previewed live in a theme tab and saved apart from settings. Shared code has one copy each: a `claude-sessions` crate locates Claude directories, projects and transcripts, and the Python `claude-projects` is retired for a Rust `ways projects`; `agent-theme` alone detects terminal colour; agent-identity's colours are the one categorical palette; a raw colour escape outside `agent-theme` fails a test. New tooling on these paths is Rust. Hook scripts are thin adapters over decisions the binaries make; the commands they run never touch the engine or read a theme file, within a stated latency budget. `ways rethink` and its flags are removed. This amends ADR-154 §2 and §4.
- **Trades away:** The zero-dependency compositor ADR-154 §2 chose, with its binary-size margin, and the hand-rolled `rethink` code, which is rewritten. A theme schema shaped to agent-ways alone: its theme keys follow the dotfiles palettes. `attend-chat`'s iocraft UI, which is rewritten. Scripts and muscle memory that still call `ways rethink` stop working. The `claude-projects` command name: its commands move to `ways projects`. The names of attend's signal directories for project paths holding characters other than letters, digits, `/`, `_` and `.`.
- **One-way?** Not strictly, but undoing it after `rethink`, the settings screens and `attend-chat` move means rewriting them again. The theme file format, the dotfiles palette structure plus agent-ways' added keys, becomes a contract with every user theme. Removing `ways rethink` can be undone by restoring the alias, but a caller that has broken has already broken.
- **Probes:** *Confident (one look):* you want the `introspect` screens, the settings app, `attend-chat` and the plain `ways` and `attend` tables to read as one product, and changing the theme to change all of them. *Not confident (own themes):* you are content that agent-ways' themes live only in its own directory, so changing your dotfiles theme changes nothing here until you copy or edit a theme in the theme tab.
- **Inversion:** Between each surface choosing its own library and colours, and one framework every application must render through. The theme is shared by all three applications, and the engine by the two that draw screens; each application keeps its own binary and purpose.

## Context

The workspace has two TUIs on two foundations. `rethink` is about 2,900 lines of hand-rolled crossterm plus about 1,000 lines of compositor and render code; `attend-chat` uses iocraft. ADR-154 §2 kept crossterm and a small compositor for `introspect`, rejecting ratatui on binary size and on its break with the ANSI-`String` contract `cmd/render` shares with `ways list`, and named its own escape hatch: adopt ratatui once a surface needs multiple panes, mouse, resizing and text selection.

Colour has no shared source. Ok, warn and err exist as four separate copies using three different codes; selection is drawn three ways; only `agent-identity` honours `NO_COLOR`. Raw SGR escapes appear on 236 lines in 24 files across six crates, 138 of them in `ways-cli` and 56 in `ways-audit`. Terminal detection exists twice, `agent-identity`'s `TermCaps` and the spike's `ColorDepth`, and the two disagree on `TERM=dumb` and on 256-colour terminals. Categorical colour exists twice: `ways show` pins clusters with ten truecolor values of its own, three of them near agent-identity's coral, teal and sky.

Locating Claude Code's data has no shared source either. `tools/claude-projects` is a 1,422-line Python script that lists, searches, cleans and relocates `~/.claude/projects`. Transcript paths are built separately in `ways-core`, six `ways-cli` modules, `attend`, `sensor-peers` and the spike's project picker. The project-path encoder is copied into ten files under three rules: `/` and `.` become `-`; `/`, `_` and `.` do; every character outside `[A-Za-z0-9]` does. `attend`, `attend-chat` and `sensor-peers` do not depend on `ways-core`, so its `transcripts_root` was out of their reach. `introspect`'s session picker lists sessions from the ways event log and opens their transcripts through this path code.

Hooks run `ways` on every prompt, tool call and stop. On those paths `ways` loads its config files and, for the relevance gate, `agent.yaml` (ADR-503). The hook scripts are bash; `check-post.sh` finds and runs each way's `postcheck.sh` and decides what to inject itself. Claude Code mods are TypeScript functions in plugins that run in-process before, after, instead of or around tool calls, permission requests and prompts.

`ways rethink`, `rethink --list` and `rethink --json` have been deprecated aliases for `ways introspect replay`, `list` and `dump` since ADR-154 §4 and ADR-189 increment 4. Docs still cite `ways rethink --json`.

`attend` is expected to get thinner as the ways MCP server (ADR-501) takes on more of what it does. The ways agent daemon (ADR-502) runs the judge, and embedding search is to move into it next (#668).

The operator asked for a maintained library over hand-rolled code, for themes agent-ways owns as its own TOML resource without linking to the status line, for a theme tab that previews in place and saves independently of settings, and for the same themes across `ways`, `attend` and tools such as `rethink`. The operator answered "one app" for the settings of `ways` and `attend`, and drew the application boundaries by purpose: `introspect`, settings and the plain `ways` output are one application; `attend-chat` is another, for people, respects the themes and moves to the same UX framework for more code reuse; `attend` is being stripped toward an MCP server. Themes are agent-ways' own, in the structure of the operator's dotfiles palettes, deviating where that structure is insufficient. The operator also asked that the legacy paths into the interactive modes be retired, and that the duplicated foundations and the hook paths be settled in this decision. The operator settled on Rust for these tools, and named lightweight hooks as the start of compatibility with Claude Code mods.

A dry-run spike (`tools/spikes/settings-tui` on `spike/settings-tui`) built the shell and theme engine on ratatui: tabs, typed values, queued actions, masked entry, per-tab review and apply, an exit guard, guided flows, mouse, and a theme tab with live preview and its own save. It runs 107 tests on ratatui's `TestBackend`, and its frames were reviewed as PNGs. Its frames were turned into PNGs by a Python script kept outside the repository. roguemap's TUI loop (headless snapshot, PNG review, golden frames) was the model for testing.

## Decision

1. **Three applications, one theme.** Each keeps a separate purpose.
   - `ways`: the settings screens (§8), `introspect replay` and `live` first among the interactive modes, then list, query, document and flow screens as they are built, and the plain ways output. One application on `agent-tui`.
   - `attend-chat`: an independent application and binary for people. It is ported from iocraft to `agent-tui`, sharing the tabs, lists, input, mouse, theme and test kit with `ways`; only the engine and theme are shared, and its screens and purpose stay its own. A JSON object mode for programmatic use may follow in a later decision.
   - `attend`: being reduced toward an MCP server, or an MCP server mode (#538). It has no TUI, and its settings are edited from `ways` (ADR-503).

   The ways agent daemon takes on embedding search beside the judge (ADR-502, #668), which keeps `ways` thin as a client.

2. **ratatui.** `ways` and `attend-chat` use ratatui with the crossterm backend. This invokes ADR-154 §2's escape hatch: the settings screens need multiple panes, mouse, resizing and guided flows, and `introspect`'s drill-down is the inspector §2 described. ratatui's `TestBackend` renders frames in `cargo test`. The ANSI-`String` panels in `cmd/render` stay for non-interactive output and take their colours from `agent-theme`.

3. **`agent-tui`, the app shell.** It provides a tab bar with pending badges, a bottom bar, the modes browse, review and apply, an exit guard across tabs, mouse with a capture toggle, confirm and masked entry, and guided flows (pick, preview, confirm). Screen shapes are tree and detail, list and detail, query and ranked results, timeline, and document. Review is a read-only mode of the browsing layout. Apply and discard act on one tab: value writes first, then queued commands in order, stopping at the first failure. The crate knows nothing about `ways` or `attend`; an adapter supplies content, actions and previews.

4. **`agent-theme`, the theme engine.** It has no ratatui dependency.
   - A theme file has the structure of the operator's dotfiles palettes (`~/.dotfiles/theme/palettes/*.theme`): `THEME_NAME`, `THEME_LABEL`, `THEME_KIND` (`light` or `dark`) and ten slots, `THEME_BG`, `THEME_FG`, `THEME_DIM`, `THEME_SUBTLE`, `THEME_ACCENT`, `THEME_INFO`, `THEME_OK`, `THEME_WARN`, `THEME_ERR` and `THEME_ALT`. Bare uppercase keys, quoted values and `#` comments make such a file valid TOML as written, so agent-ways parses theme files as TOML with these keys, and a dotfiles palette file loads unchanged with no import step. The tool bindings `THEME_VIVID`, `THEME_BAT` and `THEME_DELTA` are accepted and ignored.
   - Where that structure is insufficient, agent-ways adds keys in the same style: `THEME_BACKGROUND = "terminal"` keeps the terminal's ground and `"fill"` paints bg behind every cell, defaulting to `terminal`; `THEME_HOT`, `THEME_RULE`, `THEME_FADED` and `THEME_SELECTION` optionally pin the derived roles.
   - Derived roles (hot, rule, faded text, selection) are computed unless pinned.
   - Text roles are lifted to 4.5:1 contrast, and muted roles to 3:1, against bg and against the selection, by moving lightness in OKLCH and keeping hue and chroma as far as sRGB allows.
   - The status roles (accent, err, ok, warn, info, hot) stay at least 0.08 apart in ΔE OK, each moved the least that clears the ones before it.
   - It detects colour depth and honours `NO_COLOR`, under which nothing is coloured and emphasis is reverse video.
   - Outputs map roles onto ratatui styles and ANSI SGR strings.
   - Identity colours and the banner gradient are not theme roles.

5. **Where themes live.** Bundled themes ship in the binary in the same format: a default on the agent-ways identity palette, and a few taken from the operator's dotfiles palettes. User themes live in `$XDG_CONFIG_HOME/agent-ways/themes/`, with the active choice in a file beside them; a dotfiles palette copied there is a user theme as it stands, and a user file with a bundled theme's name overrides it. Nothing reads another tool's theme at runtime.

6. **Shared foundations.** Code that every application needs has one copy.
   - *Claude directories, projects and sessions.* A leaf crate, `claude-sessions`, locates Claude Code's data: the config directory (`~/.claude` or one given, such as an ADR-184 target), its projects with their sessions, a project path's directory name and the path a name resolves to on disk, and a session's transcript by id. It depends on the standard library and `serde_json` only. It is a new crate because `attend`, `attend-chat`, `attend-groups`, `attend-instances` and `sensor-peers` need it and none of them depends on `ways-core`; `ways-core`'s `transcripts_root` and transcript lookup delegate to it. Its one encoder follows Claude Code's rule, every character outside `[A-Za-z0-9]` becoming `-`, pinned by a test against directory names Claude Code wrote, and replaces the ten copies. `attend` names its signal directories with the encoder too; for one release it also reads directories named under its old rule. `introspect`, the settings screens' project and target pickers, the `ways` commands that open transcripts (`context`, `corpus`, `memory-seed`, `list`) and `attend` use it.
   - *`ways projects`.* The commands of `tools/claude-projects` are reimplemented in Rust over `claude-sessions` as the `ways projects` subcommand group: `list` (with `--active`, `--memory`, `--stale`), `search`, `show`, `stats`, `cleanup` (with `--dry-run`) and `relocate` (a preview by default, `--execute` to move). A group in `ways` over a binary of its own because `ways` already links `claude-sessions` and `agent-theme`, and a separate binary would need its own release, download script and install link. The change that lands the port deletes the Python file and its link in `scripts/install.sh`, and updates `docs/migration-1.0.md`. The command regrouping (ADR-503 §14) may move the group under `ways session`. `cleanup` and `relocate` keep their confirmation; they are the only writers into `~/.claude/projects`, which `claude-sessions` itself only reads.
   - *Terminal capability.* `agent-theme` detects colour depth (truecolor, 256, 16, none) and `NO_COLOR`. `TERM=dumb`, an empty `TERM` and a non-empty `NO_COLOR` mean no colour. agent-identity's `TermCaps` is removed; its callers in `agent-identity`, `attend`, `attend-chat` and `attend-identity-view` take the depth from `agent-theme`, and agent-identity chooses between its rich and basic palettes from it.
   - *Categorical colour.* agent-identity's identity colours are the one categorical palette. `ways show` pins its clusters from it, and its ten `PIN_COLORS` are removed. Categorical colours stay outside themes (§4).
   - *Every coloured output goes through `agent-theme`.* Status colours are theme roles; categorical colours and the banner gradient are written by the same outputs. Plain CLIs use the ANSI output and screens the ratatui output. `ways-cli`, `agent-fmt` (tables, banner and permissions), `ways-audit`, agent-identity's ANSI rendering, `attend-identity-view` and `attend` move to it. A raw SGR literal (`\x1b[`, `\033[`, `\u{1b}[`) in a Rust source under `tools/` outside `agent-theme` and `tools/spikes` is a finding: a test in `agent-theme`, run by `cargo test` and `make test`, scans the workspace sources and fails naming each one with its file and line. A second test plants a literal in a fixture file and requires the scan to report it. Until the plain outputs move (§9), the scan reads an allowlist of the files that still carry literals; each step of §9 removes its files, and a file not on the list fails from the first change.
   - *Rust.* New tooling on these paths is Rust: the crates above, the `ways projects` port, the SGR check and the test kit's PNG renderer (§12). Once the port lands no Python remains on them, and no shim keeps the `claude-projects` name. Existing non-Rust tools on these paths are follow-ups outside this decision: the bash hook adapters in `hooks/ways/`, whose remaining logic moves into the binaries under §11.

7. **The theme tab.** It follows the settings tabs. Moving the cursor previews a theme across the whole UI; leaving the tab returns to the active theme; Enter makes the one under the cursor active. New, copy, rename, delete and edit are actions; editing a bundled theme starts a named copy. The editor shows each slot as a swatch with its hex, sliders and hex entry for the selected slot, and the contrast and distinctness results, naming any failing role. A failing check does not block saving. Theme saves are written at once or on Ctrl-S, outside the settings review; the exit guard lists unsaved theme edits beside the settings tabs.

8. **One set of settings screens.** `ways settings` opens them with `attend`'s tabs beside `ways`' (ADR-503), and `ways settings <tab>` opens on that tab. `attend` has no command that opens a settings UI.

9. **Adoption order.** One step per change:
   1. `claude-sessions`, with the path code in `ways-core`, `ways-cli`, `attend` and `sensor-peers` moved onto it, and `ways projects` replacing `tools/claude-projects`.
   2. `agent-theme`'s detection and ANSI output, with `TermCaps` removed and its callers moved, and the SGR test with its allowlist.
   3. The settings screens.
   4. `introspect` replay and live.
   5. The `attend-chat` port.
   6. The plain outputs: `ways-cli`'s, with `ways show` on the identity palette; `agent-fmt`'s tables, banner and permissions; `ways-audit`; then what remains of `attend`'s. The allowlist is empty when this step lands.

   The foundations go first because the settings screens' pickers and every later step use them. `introspect` goes before `attend-chat` so the shell's list, timeline and document shapes are settled on screens inside `ways` before a second binary depends on them. iocraft leaves the workspace when the port lands, and `attend-chat` follows the theme from then on.

10. **Every mode keeps its CLI form.** Nothing is reachable only through a TUI. Hook and script plumbing stays CLI-only and gets no screen.

11. **Hooks are thin and stay off the engine.** A hook script is a transport adapter: it passes the event to a binary and returns what the binary says. Every decision a hook needs (which ways fire, what to inject, the gate's verdict) is reached through one interface the binaries own, the `ways` command's JSON output and the ways agent daemon's protocol (ADR-502). The same logic can then be called from a Claude Code mod in place of a per-event shell hook without being written again; no mod work is decided here. Logic still held in a hook script, such as `check-post.sh`'s postcheck loop, moves into `ways` as a follow-up. The commands hooks run are `ways scan prompt`, `scan state`, `scan command`, `scan file`, `scan task`, `scan messages`, `show way` and `response-topics-path` on prompts, tool calls and stops; `ways disable --list` and `ways status --json` on each subagent start; and `attend inbox --drain` on stop. These code paths never initialise a terminal, call into `agent-tui` or read a theme file. Colour on them, if any, is `agent-theme`'s ANSI output at built-in defaults. Each change in §9 that touches a crate these commands use is measured against the commit before it:
    - *Budget:* the median of each command rises by no more than 5% and by no more than 2 ms.
    - *Method:* `hyperfine` with 20 warm-up runs and 200 measured runs per command, on one machine, each command fed a fixed fixture input (a prompt, a Bash command, a file edit, a stop), with `HOME` and every `XDG_*` directory pointing at a temporary fixture install so the operator's event log and settings are untouched, and with no agent daemon running so the relevance gate takes its local path.
    - A change over budget is fixed before it merges or carries the measurement and the reason in its PR.

12. **A testing method ships with the engine.**
   - The crate's test half: a headless `render(state, w, h)` entry point and a `--snap` flag; one shot list that drives both snapshots and golden tests; a frame format of glyph, colours and modifiers; a comparator that reports what drifted, exact by default, with record and dump switches; a PNG renderer for review, written in Rust in `agent-tui`'s test support, in place of the spike's Python script.
   - A way, `softwaredev/code/testing/tui`: render headless from the start, judge the look from an image, iterate before pinning, treat golden frames as a reviewed baseline whose re-record names the frames and the reason, cover 80x25 and a reference size, drive interaction through the real key handler.
   - Skills shipped to every project: `tui-snap` renders shots to PNGs and reviews them; `tui-golden` runs the golden check and, on an intended change, re-records and drafts the commit text.

13. **Legacy entry points are retired.** `ways rethink`, with its `--list` and `--json` flags, is removed when `introspect replay` and `live` move to the engine, with no hidden alias. Its deprecation window, opened by ADR-154 §4, has run. The one-release hidden aliases of a command regrouping cover names that are current; this one is not. The same change moves the docs that still name it: `docs/explanation/how-ways-works/reading-the-session-data.md`, `docs/explanation/how-ways-works/how-ways-works-the-model.md` and `docs/attend-and-monitor/salience.md`.

## Consequences

### Positive

- One theme change reaches every surface that prints colour, and `NO_COLOR` is honoured everywhere.
- Status colours stay legible and distinct on any theme, including user themes that would fail the checks as written.
- TUI changes are reviewed as frames and pinned by golden tests.
- New screens reuse the shell's tabs, review, apply and flows.
- Project paths encode one way everywhere, so `attend`'s liveness check and the transcript lookups find the same directory for every path.
- A new colour escape fails a test instead of landing.
- The TUI test loop and the project commands need no Python.
- Moving hook logic behind the binaries' interface lets a mod call it later.
- Hook latency has a budget each change is measured against.

### Negative

- ratatui adds crates and binary size to `ways`; ADR-154 estimated 300 to 600 KB and 15 to 30 crates.
- `rethink` is rewritten on the new engine.
- `attend-chat`'s UI is rewritten on the new engine, and its tests move to the `TestBackend` and golden-frame method of §12.
- Until the port lands, `attend-chat` keeps its own colours and two TUI libraries remain.
- A script or habit that still calls `ways rethink` fails once the alias is removed.
- The theme format is a contract with user files and follows the dotfiles palettes; a slot added there needs adding here, and an added agent-ways key must not collide with a later dotfiles key.
- `claude-sessions` tracks a layout Claude Code owns and can change; its encoder test fails when the rule moves.
- `attend` reads signal directories under two names for one release.
- The latency measurement adds a step to every change that touches a hook path's crates.

### Neutral

- `agent-fmt` takes its colours from `agent-theme` and keeps its own output form.
- `agent-identity` depends on `agent-theme` for terminal depth; `agent-theme` does not depend on `agent-identity`.
- `claude-projects` users run `ways projects` with the same verbs and flags; the old name stops working.
- ADR-154 §1 and §3 are unchanged. §4's migration ends: the `rethink` alias is removed.
- `agent-tui` and `agent-theme` take no dependency on `attend` and assume nothing about its shape; its tabs in the settings screens are an adapter that can shrink or move with its settings.

## Alternatives Considered

- **Keep `attend-chat` on iocraft with a theme adapter.** It avoids rewriting `attend-chat`, but leaves two TUI stacks whose look, input handling and tests must be kept consistent by hand, and `attend-chat` could not reuse the shell's widgets or test kit.
- **Keep `ways rethink` as a hidden alias.** It has been deprecated since ADR-154, and an alias kept through the port is one more entry point to maintain on the new engine.
- **Keep the hand-rolled compositor (ADR-154 §2).** It meets the cases §2 scoped it to. The settings screens and the drill-down exceed them, and §2 itself named ratatui as the step at that point.
- **iocraft everywhere.** `attend-chat` already uses it. It brings the async runtime ADR-154 kept out of the synchronous `ways` CLI, and the spike's test method rests on ratatui's `TestBackend`.
- **A third-party colour picker or theme crate.** `tui_color_picker` (about 80 downloads) and `tui-rgb-picker` (0.1.0, about 170) are early and single-purpose; `ratatui-themes` ships fixed palettes with no editing and no contrast checks. The slot editor and the checks are small and needed regardless.
- **Read the operator's dotfiles theme tool at runtime.** Declined by the operator. It would tie agent-ways to one machine's setup; sharing the file structure lets a palette be copied in instead.
- **A theme schema of agent-ways' own.** Declined by the operator. It would need an import step for every dotfiles palette and diverge from it over time.
- **A theme per tool.** The present state, with four copies of the status colours.
- **Put the Claude locator in `ways-core`.** `ways-core` already has `transcripts_root`, but `attend` and its crates would take on its YAML, markdown and corpus dependencies to reach it.
- **Keep `claude-projects` in Python, or wrap it.** Declined by the operator in favour of Rust. It keeps a second implementation of the locator and a Python dependency.
- **A Rust `claude-projects` binary.** It keeps the old name but adds a release artifact, a download script and an install link for what a `ways` subcommand group serves.
- **A Clippy lint, a per-crate test or a grep script for SGR literals.** Clippy has no such lint and a custom one needs a driver; a test per crate has to be added to each; a shell script is new non-Rust tooling. One workspace-scanning test covers every crate, including new ones.
