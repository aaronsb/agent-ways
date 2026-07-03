---
status: Accepted
date: 2026-07-02
deciders:
  - aaronsb
  - claude
related:
  - ADR-142
  - ADR-134
  - ADR-201
---

# ADR-153: Session-introspection substrate — correlating fired ways to turns

## Context

We want to answer, for any past or live session: **which ways were injected into
context, on which turn, and *why* — what caused the hook to fire.** Three
front-ends want this (post-hoc `rethink`, live `think`, a non-interactive dump
for autonomous agents — ADR-154), so the correlation belongs in one shared
substrate below them, not re-derived per front-end.

A research pass (2026-07-02) mapped exactly what data exists. The findings define
what the substrate can join today and what it cannot:

**The firing-event log** (`$XDG_STATE/agent-ways/events.jsonl`, append-only JSONL;
written by `session::log_event`, read by `firing::load_events`). A `way_fired`
record carries `way`, `domain`, `trigger`, `scope`, `project`, `session`,
`token_position`, and — for semantic fires only — `fire_score`. Sibling event
types: `way_nearmiss` (with `score_en/multi`, `thr_en/multi`, `margin`,
`query_tokens` *count*), `session_start` (`{ts, project, session}` only),
`check_fired`, `way_redisclosed`.

**The `trigger` field records the match *channel*, not the matched term** —
`keyword` / `semantic:embedding:en|multi` / `bash` / `file` / `state`. It tells
you the mechanism, never which vocabulary word or regex substring hit.

**Session transcripts** live at `~/.claude/projects/<slug>/<session>.jsonl`
(Claude-Code-owned, read-only; ms-precision timestamps, `uuid`/`parentUuid`
threading). Injected way guidance appears as an `attachment` line
(`hook_additional_context`) whose `content` is the **concatenated way bodies for
one prompt** — way-*anonymous*, one blob per prompt, and sometimes truncated to a
spilled `tool-results/…-additionalContext.txt` file.

**Two correctness problems block the substrate before it starts:**

1. **The events log is split-brained.** `session_start` — the only event that
   defines a session for `rethink` — is written by shell hooks
   (`clear-markers.sh`, `inject-subagent.sh`) that **hardcode the legacy
   `~/.claude/stats/events.jsonl`**, while every reader resolves through
   `paths::events_log()`, which *prefers* the migrated `$XDG_STATE` file
   post-ADR-142. New sessions' `session_start` lines land in the orphaned file and
   are invisible. Any introspection over an incomplete log is wrong.

2. **The join to a specific turn is heuristic, not keyed.** `way_fired` carries no
   transcript message `uuid` and no turn index; the finest available link is
   `(session, token_position, ts≈)` → a prompt bucket → the one attachment → its
   `user` turn via `parentUuid`. And "what text matched" is persisted for
   *nothing* — keyword matches are re-runnable against the prompt (if the
   transcript is present), semantic matches are only a way-level cosine.

## Decision

### 1. Single-writer the events log (correctness prerequisite)

Add a `ways events-log-path` subcommand (precedent: `ways response-topics-path`,
which shell hooks already consult instead of hardcoding). Rewire
`clear-markers.sh` and `inject-subagent.sh` to resolve the path from the binary,
so every `session_start` / `way_fired` writer and every reader agree on one file.
A migration-time union read can bridge existing orphaned logs, but the durable fix
is one writer path.

### 2. A typed introspection model in `ways-core`

Factor a `SessionIntrospection` model that joins the three sources into pure,
serde-serializable data — no ANSI, no terminal. It generalizes the already-proven
`reconstruct_frames` → `render`/`serialize` triplet (ADR-154). Shape:

```
Session { id, project, window_k, summary }
  └─ Turn { epoch, token_position, ts, transcript_uuid? }
       └─ FiredWay { way_id, trigger_channel, fire_score?, way_path,
                     criteria: MatchCriteria,        // from frontmatter
                     match: MatchDetail? }           // what hit (see §3)
```

`MatchCriteria` surfaces the fire-bearing frontmatter (`pattern`, `vocabulary`,
`commands`, `files`, `trigger`, `embed_threshold`). The join is honest about its
grain: keyed where a key exists, heuristic (time/token bucket) where it does not,
and every heuristic edge is labelled as such in the model so a consumer never
mistakes a proximity guess for a foreign key.

### 3. Enrich `way_fired` at fire time to make "why" precise

The precise "what caused this hook to fire" is unanswerable from today's log. Add,
at the fire sites (`cmd/show/mod.rs`, `cmd/scan/mod.rs`):

- **`transcript_uuid`** — the triggering message id (the hook receives it on
  stdin). Converts the heuristic time-join into a foreign key.
- **`matched_span`** — for keyword/command/file channels, the regex/glob match
  text. The only way to show the exact clip without transcript replay.
- Semantic stays **way-level**: `fire_score` ≥ `embed_threshold` is the honest
  grain; per-vocabulary-term attribution is impossible (one embedding per way) and
  must not be faked.

Enrichment is **additive and forward-only** — old records lack the fields, the
model marks their join heuristic, and no backfill is claimed.

### 4. Share the substrate with the compliance finding pipeline (ADR-201)

ADR-201's finding assembler needs exactly this: a way's firing evidence tied to
transcript pointers. The `SessionIntrospection` join *is* that evidence substrate.
Building it once, in `ways-core`, means findings and introspection read the same
correlation rather than two drifting re-derivations.

## Consequences

### Positive

- One honest correlation, shared by three front-ends and the finding pipeline.
- The split-brain fix repairs `rethink` (and any events-log reader) for
  post-migration installs — a real bug, not just a feature enabler.
- "Why fired" becomes precise for keyword/command/file once enrichment lands, and
  honestly way-level for semantic — no fabricated term-level attribution.

### Negative

- Fire-time enrichment touches the hot fire path; the added fields must be cheap
  and must never break the log's append-only, line-atomic contract.
- The model must encode *degrees* of join confidence (keyed vs. heuristic), which
  is more complex than pretending every link is exact — but the honesty is the
  point.

### Neutral

- Enrichment is forward-only; historical sessions keep the coarse heuristic join.
- Transcripts remain Claude-Code-owned and read-only; the substrate depends on
  their availability and tolerates truncation-to-spill-file.

## Alternatives Considered

- **Union-read both event-log paths, leave the hooks hardcoded.** Rejected as the
  durable fix: it papers over the split-brain and re-breaks the next time a path
  moves. A single resolved writer path is the real correction (a bridging union
  read on top is fine as a transition).
- **Reconstruct "why" purely by transcript replay, persist nothing new.** Works
  for keyword/command (re-run the regex on the stored prompt) but fails when the
  transcript is absent/truncated and gives no foreign key; semantic stays
  way-level regardless. Enrichment is the only path to a precise, transcript-
  independent "why."
- **Fake semantic term-level attribution** (highlight the "matching" vocabulary
  word). Rejected: the corpus stores one vector per way; there is no matched term
  to recover. Presenting one would be a confabulated explanation — the exact
  epistemic error the compliance work (ADR-200) exists to avoid.

## References

- **ADR-142** — the XDG projection whose migration created the events-log split.
- **ADR-134** — the near-miss telemetry stream this model also surfaces.
- **ADR-201** — the finding pipeline that shares this transcript-evidence substrate.
- Research pass 2026-07-02 (events-log schema, transcript shape, frontmatter match
  fields, join feasibility) — the ground truth this ADR is built on.
