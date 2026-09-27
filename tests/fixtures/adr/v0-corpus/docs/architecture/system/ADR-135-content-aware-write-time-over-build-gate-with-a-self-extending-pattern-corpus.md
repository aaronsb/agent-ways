---
status: Accepted
date: 2026-06-12
deciders:
  - aaronsb
  - claude
related:
  - ADR-134
  - ADR-123
  - ADR-130
---

# ADR-135: Content-aware write-time over-build gate with a self-extending pattern corpus

## Context

A write-time check now exists: `softwaredev/code/code.check.md` fires at `PreToolUse(Edit|Write)` on source files and surfaces a short anchor plus verification questions — does this need to exist, is this the codepath that actually runs, is this the minimal change. It ships with no engine change because it needs none: the `ways scan file` path matches a check to a file by its `files:` regex and never inspects what is about to be written. That is its ceiling. A question can ask "did you reach for the stdlib first?"; it cannot *see* the hand-rolled LRU cache, the reinvented email-regex validator, or the new dependency in the diff and name them.

Naming them requires the candidate content, which today never reaches the matcher. `check-file-pre.sh` forwards `tool_input.file_path` only, and `ways scan file` accepts `--path`, `--session`, `--project` — no content channel. Adding one is a genuine new capability, not a new file.

Three forces make it worth building, and one makes it dangerous:

1. **The friction is empirically at write-time.** An external, model-graded usage report (2026-05 to 2026-06, 92 sessions) shows the dominant friction buckets are Buggy Code (40), Wrong Approach (37), and Excessive Changes (7) — every one of them landing at the moment code is written, the exact moment this gate would fire.
2. **The minimalism content is well-understood.** The "ponytail" plugin's ladder (YAGNI → stdlib → native → installed dep → one line) and its review tags (`stdlib:`, `native:`, `yagni:`, `shrink:`) are a serviceable taxonomy for what over-build *looks like* in concrete code.
3. **The matcher already learns elsewhere.** ADR-134 established empirical tuning from fire and near-miss telemetry — but at the threshold and cadence level. Pattern *recognition* is the next level up and has no mechanism.

The danger is the obvious implementation. The ponytail model bakes one experienced developer's opinion into a fixed catalog. A fixed catalog nags at its edges: code that falls outside it is misjudged, and legitimately novel approaches read as "you're doing it wrong." A matcher that suppresses innovation because it has not seen it before is a failure we will not ship. The catalog cannot be the deliverable.

## Decision

Build the content channel, but make the thing it feeds a **learning matcher, not a static catalog**. The pattern corpus self-extends from comprehended encounters, bounded by the precision discipline already in force, so it grows understanding without growing dogma.

### 1. Content channel

`check-file-pre.sh` forwards `tool_input.content` (Write) / `tool_input.new_string` (Edit). `ways scan file` gains a `--content` input — or a sibling `ways scan write` subcommand — that pattern-scans the candidate code. Content is budget-reduced before scanning, consistent with ADR-130's uniform hook budget.

### 2. The gate, and what runs on the hot path

The gate emits ponytail-style `stdlib:` / `native:` findings plus a new-file line-count signal, and is silent otherwise. The hot path is cheap and does no inference:

- **Recognized pattern** → emit the finding (`stdlib: hand-rolled LRU — functools.lru_cache covers it`).
- **Unrecognized code** → **silence**, plus an *encounter* entry to the `events.jsonl` near-miss stream (ADR-134). Silence is the correct output, not a fallback.

There is no inline comprehension on the hot path. "Spend cycles to understand it" never means deep-analyze every keystroke — that would destroy the ambient, near-zero-cost value the firing engine exists to provide.

### 3. The loop — corpus growth happens out of band, and runs both ways

Comprehension and recording happen in a deliberate authoring/tuning pass modeled on `ways tune`, consuming the encounter telemetry the hot path emitted:

1. Read the encounter stream — the unrecognized writes the gate stayed silent on.
2. Comprehend the genuinely ambiguous cases (the cost is paid here, off the hot path, on a bounded sample).
3. **Record a finding — or, usually, do not.** A finding is either a new **match** (a real, generalizable reinvention) or an **exemption** (a legitimate or novel shape that must never be flagged). Most encounters are neither and stay silent and unrecorded.
4. **Prune** patterns whose telemetry shows them over-firing.

The loop is **bidirectional by design**. A loop that only adds matches becomes ponytail-by-accretion — a catalog that has "seen everything" and therefore nags at everything. That is the named anti-pattern. Recognition is provisional and revisable; a pattern earns its place from observed behavior and loses it the same way.

### 4. Precision discipline (inherited, not invented)

The gate inherits ADR-134's precision-first, zero-false-positive **hard constraint**. The corpus gets less ignorant over time *only through comprehension*, never through speculative up-front enumeration. Comprehensiveness is explicitly a non-goal: a small, high-confidence corpus that is silent on the unfamiliar beats a large one that is confidently wrong. The bootstrap seed is deliberately tiny — on the order of three or four patterns (hand-rolled LRU/TTL cache, email-regex validation, manual retry around an idempotent call, new-file line count) — and the seed is a starting point for the loop, not a specification of coverage.

This makes ADR-135 the first *content-level* and *pattern-level* application of ADR-134's machinery: 134 tunes thresholds and cadence from telemetry; 135 tunes recognition itself from the same substrate.

## Amendment — 2026-06-12: mechanism (PostToolUse postcheck, pattern-as-way)

The Decision above specified the content channel as *PreToolUse* plus a new `ways scan --content` channel in the binary. Implementation grounding revised the mechanism on two points. The intent above is unchanged — detect concrete over-build in candidate code, as a learning corpus governed by precision-first silence. Only the delivery changes, and it changes toward less code and existing infrastructure.

**1. PostToolUse via per-way `postcheck.sh`, not PreToolUse via the binary.** The repo already has the mechanism this needs: ADR-123 Decision 5's reactive-firing path (`check-post.sh` walks `**/postcheck.sh`, pipes the PostToolUse input to each, and treats exit 0 as a fire request gated through `ways show way`). The gate is advisory — it emits, never denies — so PreToolUse's interception power buys nothing; PostToolUse is both the better behavioral fit (let the write land, then surface the fix for the next turn, as the usage report itself suggested) and free of any binary or core-hook change. The PreToolUse/binary channel was the right *first* design on paper; the existing postcheck path is the lazier one that works.

**2. Patterns live as pattern-ways, not as a bespoke data structure.** A postcheck's stdout is discarded — only its exit code is read, and the content injected on a fire is the *way's own body*. So a postcheck cannot emit a dynamic finding line; the body carries the named replacement(s). This collapses the "self-extending pattern corpus" into the corpus itself:

- **Recognition** = a postcheck predicate (stimulus → match?).
- **Learned response** = the way's body (the named replacement).
- **Learning a pattern** = authoring a pattern-way, through the existing `knowledge/authoring` loop — so the #7 loop's "record a finding" is literally `ways`-authoring from encounter telemetry.
- **Pruning** = deleting the way; **exemption** = the absence of one.

The pattern library is therefore not new infrastructure; it is a small, prunable domain of pattern-ways. This deepens, rather than alters, the learning-corpus thesis: the recognition set is the corpus, and it grows and shrinks by gaining and losing ways. Encounter telemetry and the loop (#7) remain gated on ADR-134.

**Granularity is a YAGNI call, decided by the hot path.** `check-post.sh` runs *every* `postcheck.sh` on *every* `Edit`/`Write`/`Bash`/`Task` — so one-way-per-pattern means one subprocess spawn per pattern per tool event. v1 is therefore a **single `softwaredev/code/overbuild` way** whose one postcheck holds the seed detectors and whose body lists their replacements; Claude self-selects the relevant one. Splitting into per-pattern ways (`overbuild/lru`, `overbuild/email`, …) is the loop's later refinement, justified only when per-pattern pruning or telemetry earns the extra spawns. The conceptual mapping above holds at either granularity — one way with N detectors, or N ways with one each.

Implementation note: the over-build way fires *only* via its postcheck, not via prompt/file embedding — its frontmatter is kept trigger-inert (no `description`/`vocabulary`/`pattern`) so it never pollutes semantic matching, and needs no locale companion.

## Consequences

### Positive

- The write-time over-build signal becomes concrete: it names the specific reinvention in the candidate code, not just a generic "did you consider…". This targets the usage report's measured #1 friction at the point it occurs.
- The corpus is anti-fragile to novelty. The encounter ponytail would misjudge is exactly the encounter that, once comprehended, teaches the system — or is recorded as a permanent exemption. Unfamiliarity routes to silence-then-maybe-learn, never to a false flag.
- ADR-134's near-miss substrate gets its first concrete consumer beyond threshold tuning, validating that design end to end.

### Negative

- A new input channel widens the hot-path surface: content must be forwarded, budget-reduced, and scanned within the PreToolUse latency envelope. Pattern scanning must stay cheap (literal/regex shape matching, no inference).
- The loop requires human/agent attention on a cadence. Without the out-of-band pass, the corpus ossifies at its seed — functional, but not the learning system this ADR justifies.
- `events.jsonl` gains an encounter stream on top of 134's near-miss volume; the encounter margin needs a cap and the log needs rotation (134's concern, compounded).

### Neutral

- Tier 1 (`code.check.md`) is unaffected and remains valuable on its own; this ADR is strictly additive. If 135 is never implemented, Tier 1 still ships the questions.
- The ponytail review *tags* survive (`stdlib:`/`native:`); the ponytail *philosophy* (fixed, comprehensive, opinionated) is explicitly rejected. Only the finding shape is borrowed.
- Pattern entries become ordinary versioned artifacts — reviewable, revertible git diffs — like the frontmatter 134's `--apply` rewrites.

## Alternatives Considered

- **Static curated catalog (the ponytail model).** Rejected as the deliverable: it nags at its edges and cannot grow from experience. Its taxonomy is borrowed; its fixedness is the thing this ADR exists to avoid.
- **Model-graded per-write judgment (an LLM decides "is this over-built?" on every Write).** Highest fidelity, rejected for the hot path for the same reason ADR-134 rejected model-graded relevance: it adds inference cost to a system whose value is being cheap and ambient. It is admissible only in the out-of-band comprehension step, on a bounded sample.
- **PostToolUse lint instead of PreToolUse gate** (the shape the usage report literally suggested: `PostToolUse(Edit|Write)` running a formatter/type-checker). Complementary, not a substitute — it catches mechanical defects *after* the code lands, whereas over-build is a *before-you-write* decision. Worth adopting separately; it does not address this signal.
- **Append-only learning loop.** Rejected: monotonic growth reconstitutes the ponytail dogma by accretion. Pruning and exemptions are load-bearing, not optional.
