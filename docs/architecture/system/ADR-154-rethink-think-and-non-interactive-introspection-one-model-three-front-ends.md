---
status: Draft
date: 2026-07-02
deciders:
  - aaronsb
  - claude
related:
  - ADR-153
  - ADR-111
---

# ADR-154: rethink, think, and non-interactive introspection — one model, three front-ends

## Context

ADR-153 defines a shared `SessionIntrospection` model. This ADR decides the
surfaces over it. There are three, plus a drill-down, and the user wants them to
feel like one tool a user can move between:

- **`rethink`** — post-hoc replay of a finished session (exists today).
- **`think`** — a *live* monitor of the current session, same UI, refreshing as
  new ways fire.
- **non-interactive dump** — structured output an autonomous agent reads to
  investigate a session (a JSON dump exists today via `rethink --json`).
- **the why-fired drill-down** — from a list of fired ways, enter one and read the
  way, its trigger criteria, and the clip of session that matched.

The research pass established the current surface and its gaps:

- `rethink` is raw **crossterm + hand-rolled ANSI strings** (not ratatui), two
  sequential blocking loops (session picker → frame player), sharing the
  `cmd/render` table writer with `ways list`. Single-panel, full-clear-redraw.
- The **model→(render | serialize) split already half-exists**: `reconstruct_frames`
  (pure data) feeds both the TUI and `rethink_dump`'s JSON — the exact factoring
  the three front-ends want.
- Two bugs (owned here as command semantics): `rethink` **silently globalizes**
  when current-project detection returns `None` (run manually, `CLAUDE_PROJECT_DIR`
  is unset), and there is **no `--list --json`** for an agent to enumerate sessions
  before dumping one. (The events-log discovery bug is fixed in ADR-153.)
- `attend-chat` uses **iocraft** (a declarative TUI) with an async runtime for its
  rich interface — the maintainer's rich-TUI precedent, but it drags `smol`/
  `async-channel` that `ways-cli` (fully synchronous) deliberately lacks.

## Decision

### 1. One `cmd/introspect/` module; front-ends differ only in their loop

Generalize the proven triplet into a module (mirroring the `cmd/scan`,
`cmd/settings`, `cmd/lint` subdir convention):

- **model** — build `SessionIntrospection` (ADR-153).
- **render** — extend `cmd/render`'s ANSI-`String` contract with
  `render_way_detail` / `render_clip`; no new rendering paradigm.
- **front-ends**, each a thin loop over the same model:
  - one-shot render → print (like `list`);
  - **`think`** — the existing `rethink::tui_loop` poll skeleton, re-reading on a
    tick (see §3);
  - **JSON** — serde-serialize (extend `rethink_dump`);
  - **drill-down** — a selection/tab loop compositing model panels.

### 2. Keep crossterm; grow a small micro-compositor — do not adopt ratatui

Build a ~200–400-line internal compositor over the existing ANSI-`String` panels:
`panel = Vec<String> + width`, a side-by-side placer, a scroll-window helper, a
tab-bar helper. This preserves the lean `opt-level=z` binary (~3.3 MB), adds **zero
dependencies**, and reuses `render.rs`'s output verbatim as the "matched clip"
content.

Rejected for now: **ratatui** — ~+300–600 KB and 15–30 crates against a size-tuned
binary, a real compile hit under `lto`/`codegen-units=1`, and — decisively — its
immediate-mode `Line`/`Span` model breaks the ANSI-`String` contract that
`cmd/render` shares with `ways list`, forcing a rewrite of both. **Escape hatch,
stated up front:** if the drill-down is meant to become a genuine multi-pane,
mouse-driven, resizable, text-selectable inspector, adopt ratatui *at that point*
rather than grow a poor reimplementation and migrate later. The micro-compositor
is right for "live table + status bar" and "list-left / read-only-detail-right
with independent scroll"; ratatui is right for a true windowed inspector.

### 3. Live refresh: poll-with-timeout + mtime/size stat gate — not `notify`

`think` reuses crossterm `poll(Duration)`: the timeout is the refresh interval
(~100–250 ms, imperceptible), on timeout re-read the *tail* of the (append-only)
events log + transcript and re-render, on key event handle input. Skip the
re-parse when the files' mtime/length are unchanged. This stays synchronous, adds
zero dependencies, and matches the project's existing poll style. Rejected:
`notify` (event-driven) — it forces a background thread + channel + a blocking
select that `attend-chat` solved only by bringing in an async runtime; its
large-tree advantage does not apply to two append-only files.

### 4. Command surface and scoping semantics

- **`ways rethink`** — post-hoc. Default to the **current Claude Code project**;
  add `--all` for every project and keep `--project <path>` for a specific one.
  When current-project detection returns `None`, **fail loud** (name the missing
  marker) or fall back to cwd — never silently globalize. Compare on the encoded
  project slug / normalized path, not a loose substring.
- **`ways rethink --list --json`** — enumerate candidate sessions as structured
  data so an agent can pick one before dumping it (closes the non-interactive
  enumeration gap).
- **`ways think`** — live monitor of the current session; same scoping default.
- **the drill-down** — a tab in `rethink`/`think`: a fired-ways list; entering a
  way opens a read-as-a-human panel with the way body, its trigger criteria
  (ADR-153 `MatchCriteria`), and the matched session clip (precise once ADR-153 §3
  enrichment lands; heuristic-labelled before).

Consolidation (ADR-111 single-surface spirit): whether these live under `ways
rethink`/`ways think` or a unified `ways introspect <mode>` is an open naming
question for the implementation — the *model* is shared regardless.

## Consequences

### Positive

- Three surfaces + a drill-down from one model — no drift between what an agent
  reads as JSON and what a human sees in the TUI.
- Zero new dependencies; the lean binary and the shared `render.rs` contract both
  survive.
- `rethink` stops silently globalizing and gains machine-listable sessions.

### Negative

- The micro-compositor is hand-built layout code (panes, scroll, tabs) — bounded
  (~200–400 lines) but genuinely new, and less capable than ratatui's widgets.
- A live `think` loop re-reading files is more moving parts than a one-shot dump;
  the stat-gate must be correct to avoid needless re-parse flicker.

### Neutral

- If the inspector's ambitions grow, the escape hatch to ratatui is deliberate and
  documented — this decision is reversible, not a dead end.
- Final command naming (`rethink`/`think` vs. `introspect`) is left to
  implementation.

## Alternatives Considered

- **Adopt ratatui now** for real layout/widget primitives. Rejected for the
  current scope on binary-size, compile-cost, and the `render.rs`-rewrite grounds
  above — with the explicit escape hatch if scope grows.
- **Reuse `attend-chat`'s iocraft + async stack.** Rejected: it would drag an
  async runtime into a deliberately synchronous CLI — a larger intrusion than
  ratatui for less fit.
- **`notify` filesystem watcher for `think`.** Rejected: event-driven latency is
  not needed for two append-only files, and it forces the async architecture
  ways-cli avoids; poll + stat-gate is the lean match.
- **Keep three independent implementations** (rethink as-is, a separate think, a
  separate dumper). Rejected: guarantees drift; the whole point is one model.

## References

- **ADR-153** — the `SessionIntrospection` substrate these front-ends render.
- **ADR-111** — the single-tool-surface consolidation spirit informing the command
  naming question.
- Research pass 2026-07-02 (TUI stack, crossterm-vs-ratatui trade study, refresh
  mechanism, shared-model factoring) — the ground truth this ADR is built on.
