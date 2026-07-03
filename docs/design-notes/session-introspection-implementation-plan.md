## Session Introspection — Implementation Plan

> **Type:** Design note (not an ADR)
> **Status:** Working draft — sequencing for ADR-153 + ADR-154, deferred implementation
> **Cites:** ADR-153 (introspection substrate), ADR-154 (three front-ends), ADR-201 (shared finding evidence), ADR-134 (near-miss telemetry)
> **Motivates:** the `ways introspect <replay|live|dump>` surface (`ways rethink` fixes + a live monitor + non-interactive dump) and the why-fired drill-down

## What this note is

The two ADRs decide *what* and *why*; this note sequences the *how* into
independently-shippable increments, so the work can land incrementally without a
big-bang PR. Grounded in the 2026-07-02 research pass (three probes: rethink
internals, correlation data model, TUI trade study).

## Guardrails (carry into every increment)

- **Join honesty (ADR-153).** The fired-way ↔ turn link is *keyed* only after
  enrichment (increment 3); before that it is a `(session, token_position, ts≈)`
  heuristic bucket. The model must *label* each edge's confidence; never render a
  proximity guess as a foreign key.
- **No fabricated semantic "why."** One embedding per way — there is no matched
  vocabulary term to recover. Present semantic fires as way-level cosine
  (`fire_score` ≥ `embed_threshold`), never a highlighted term.
- **Lean binary.** Zero new deps is the target (crossterm + micro-compositor, poll
  + stat-gate). Adopting ratatui is the documented escape hatch *only* if the
  inspector grows into a true multi-pane/mouse/resizable tool — decide before, not
  during.
- **Transcripts are read-only** (Claude-Code-owned) and may be truncated to a
  spilled `tool-results/…-additionalContext.txt`; tolerate absence and truncation.

## Increments

### 1 — Correctness fixes (fast, independently shippable) — ADR-153 §1, ADR-154 §4

The bugs that make `rethink` wrong today. Land first; delivers value alone.

- **Events-log single-writer.** Add `ways events-log-path`; rewire
  `hooks/.../clear-markers.sh` and `inject-subagent.sh` to resolve it from the
  binary (precedent: `ways response-topics-path`). Optional transitional union-read
  in `resolve_events`. *This is the fix for "rethink doesn't see post-1.0.0
  sessions."*
- **Project scoping.** `rethink`/`rethink_dump`: default to current project,
  add `--all`, fail-loud (or cwd-fallback) when detection is `None`, compare on
  slug/normalized path not substring. Insertion points: `rethink.rs:123-134` +
  `665-668`; `rethink_dump.rs:94-102` + `277-281`.
- **`ways rethink --list --json`** — machine-listable session enumeration. (Becomes
  `ways introspect list --json` at increment 4; the alias preserves this form.)
- Key files: `session.rs` (log_event / new path cmd), the two shell hooks,
  `cmd/rethink.rs`, `cmd/rethink_dump.rs`, `ways-core/src/paths.rs`.

### 2 — The `SessionIntrospection` model — ADR-153 §2

- New `ways-core` module (or `cmd/introspect/model.rs`): pure serde structs
  `Session → Turn → FiredWay{criteria, match?}` with confidence-labelled joins.
- Generalize `rethink::reconstruct_frames` into it (keep the existing frame data;
  add `MatchCriteria` from frontmatter fire-bearing fields, read via
  `cmd/scan/candidates.rs`'s raw extraction).
- No UI yet — model + unit tests over sample events + a fixture transcript.

### 3 — Precise "why" — ADR-153 §3 (revised: split by source)

Investigation (2026-07-03) corrected §3: no message uuid is available at fire time
(`UserPromptSubmit` carries none), but the transcript records each hook injection
with a `parentUuid` chain back to its user message. So the two halves are sourced
differently and land as two independently-shippable sub-increments:

- **3a — `transcript_uuid`, post-hoc in the ways-core model (no hot path).** Read
  the transcript, index `UserPromptSubmit` `attachment`s (their `parentUuid` walks
  to the triggering `user` message), match each turn to its attachment by
  session + fire-timestamp, follow to the user-message uuid, and flip that turn's
  join `Heuristic`→`Keyed`. Absent/unmatched transcript → stays `Heuristic`. Reuse
  the transcript-reading pattern from `context.rs`/`rethink::build_token_timeline`.
- **3b — `matched_span`, fire-time enrichment (hot path).** The injected content is
  way-anonymous, so *what text matched a way* must be captured when it fires. Add
  `matched_span` for the keyword/command/file channels at the fire sites
  (`cmd/show/mod.rs` `way_scored` log block ~155-186, via `scan/scoring.rs:179`;
  `cmd/scan/state.rs`). Plumb the regex/glob match through `PromptMatch::Fired` →
  `scoring.rs` → `way_scored`. Cheap, line-atomic. Semantic stays `fire_score` only.
- Model reads each field when present, falls back to the heuristic grain when
  absent.

### 4 — `ways introspect` surface + non-interactive dump — ADR-154 §1, §4 (agent-facing MVP)

- Introduce the `ways introspect <replay|live|dump|list>` command; wire `ways
  rethink` as a deprecated alias → `introspect replay` (stderr notice, still works).
- Re-point `rethink_dump` at the increment-2 model as `ways introspect dump`. This
  is the autonomous-investigation surface — ship it before the TUIs.

### 5 — Micro-compositor + why-fired drill-down — ADR-154 §2

- ~200–400-line compositor over `render.rs`'s ANSI-`String` panels: panel
  (`Vec<String>`+width), side-by-side placer, scroll-window, tab-bar.
- Drill-down tab in `rethink`: fired-ways list → enter → way body + `MatchCriteria`
  + matched clip (precise post-enrichment, heuristic-labelled before).

### 6 — `ways introspect live` monitor — ADR-154 §3

- New mode; reuse `rethink::tui_loop`'s `poll(Duration)` skeleton with the
  refresh interval as timeout; re-read tail of events-log + transcript on tick;
  mtime/size stat-gate to skip unchanged re-parse. Same compositor + model.

## Resolved decisions (settled 2026-07-02)

- **Command naming — DECIDED:** one unified `ways introspect <replay|live|dump>`
  surface (ADR-111 single-surface spirit), *not* the `ways rethink`/`ways think`
  verb pair (rejected as too cute). `ways rethink` is kept as a **deprecated alias**
  → `introspect replay` (and `rethink --json` → `introspect dump`) so existing
  muscle memory keeps working. The `introspect` CLI surface + alias is a thin step
  introduced at **increment 4** (the first new command); increments 1–3 fix and
  factor the *underlying* code that `introspect replay` will call.
- **Shared substrate with ADR-201 — DECIDED:** the `SessionIntrospection` model
  lives in **`ways-core`** (not `ways-cli`) and is the finding pipeline's evidence
  source, so introspection and findings read one correlation, not two drifting
  re-derivations.
- **ratatui escape-hatch trigger — DECIDED:** the concrete criterion is the
  drill-down needing **text selection or resizable / mouse-driven panes**. Short of
  that, the micro-compositor stays. It's a criterion now, not a vibe.
