---
status: Superseded
superseded_by:
  - "ADR-144"
date: 2026-06-30
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-144]]"
  - "[[ADR-142]]"
  - "[[ADR-143]]"
---

# ADR-145: Explicit three-source convergence manifest

> **ARCHIVED — 2026-08-13.** No longer part of the active architecture set. Kept for history
> and so existing references still resolve.
>
> **Why:** ADR-144 shipped the two-source reconciler the day before this refinement was written, and solved the problem without a pinned Claude-Code baseline. The three-source manifest is absent from manifest.rs and reconcile.rs.
> **Superseded by:** ADR-144
>
> Nothing below this line has been edited.

## Context

This is a child of ADR-144 (install / repair / migrate as one manifest reconciler),
which is itself a child of ADR-142 (agent-ways 1.0). ADR-144 unified install, update,
repair, and migrate into **one idempotent reconciler** that "converges `~/.claude`
toward a git-derived manifest," and framed the four from-states as **one convergence
with different entry conditions**: "the four from-states are one convergence … they
differ only in *starting actual* and *trust posture*." Fresh is "actual is empty →
materialize all entries"; the others re-materialize a delta against a live tree.

ADR-144 names the desired side of that convergence — the manifest, derived from
`git ls-files` in `$XDG_DATA/agent-ways` — and the engine that drives toward it. But it
left the **actual** side, and one whole leg of the desired side, implicit:

- The **agent-ways leg is explicit and exists today.** `ways manifest`
  (`tools/ways-cli/src/cmd/manifest.rs`) emits exactly what agent-ways projects, derived
  from the git-tracked file set: `PROJECTED_TREES` (`skills`, `agents`, `commands`,
  `hooks/ways`), `PROJECTED_FILES` (the two named hooks), and `PROJECTED_BINS` (`ways`,
  `attend`, `attend-chat`, `way-embed`). It runs `git ls-files` over the tracked trees and
  allowlists the built binaries by name. This is already one of the three legs the
  convergence needs.
- There is **no model of what Claude Code itself owns** in `~/.claude`. ADR-142's layout
  table calls `~/.claude` the "irreducible **Claude-Code-owned floor**," but nothing
  enumerates that floor. The reconciler converges toward *(agent-ways manifest)* and
  treats everything else as a single undifferentiated "don't touch" region.
- "**What is the user's own customization**" is therefore decided heuristically — by the
  same weak membership test ADR-144 already flagged in today's `build_manifest`:
  *"did a prior projection write it,"* which "can't classify a file the user dropped into
  a shared dir that happens to match a name we later ship."

Three concrete consequences of that gap motivate this ADR:

1. **The fresh-install path was never wired.** agent-ways 1.0.0's documented installer
   (`git clone … ~/.claude && make setup`) still produces the **pre-1.0 in-place shape**,
   not the projection. A brand-new 1.0 user lands in exactly the topology 1.0 replaced.
   ADR-144 says fresh install should "fall out" of the engine as *materialize the manifest
   from nothing* — but with no explicit target for the engine to materialize *into an empty
   tree*, the native projection installer was never built. This is a real, current bug.

2. **Classification is heuristic and error-prone, and four consumers need it.** Deciding
   whether any `~/.claude` file is Claude-Code-owned, agent-ways-owned, or user-owned is
   needed by install, repair, migrate, **and** cleanup — and each currently guesses.
   ADR-144's own sharpest Negative ("the migrator must detect and rescue hand-edited core
   files … or it silently destroys the customization it was meant to preserve") is a
   symptom of having no authoritative classifier.

3. **The settings.json three-way merge already solves a version of this — for one file.**
   `tools/ways-cli/src/cmd/settings_merge.rs` does a kubectl-style three-way merge of
   `settings.json`. It tracks a stored **last-applied base** — the slice agent-ways itself
   last wrote, persisted to `$XDG_STATE/agent-ways/settings-applied.json` — and computes, per
   owned slice, `result = (theirs − base − ours) ++ ours`. **Field ownership** is explicit:
   agent-ways owns the `hooks` events and the `permissions.allow` entries matching
   `WAYS_PERMS`; *everything else* — `model`, `theme`, `plugins`, `env`, credentials, the
   user's own hooks, **and Claude Code's own default values** — is treated as the unmanaged
   remainder. This is precisely the "tell app from user" computation, made idempotent by the
   stored base. But note its shape: it is a **two-party** split (agent-ways-owned vs.
   everything-else), it does *not* separately model the Claude-Code floor, and it is scoped to
   a single JSON document.

The realization: **the reconciler's convergence target is implicit and ad-hoc, and the
pattern that would make it explicit already exists — at the granularity of one file, and in
two-party form.** This ADR lifts the owned-vs-remainder partition from that one JSON document
to the whole `~/.claude` tree, and additionally **splits the remainder into its two real
owners** — the Claude-Code floor and the user — which the single-file merge currently
conflates.

## Decision

**Make the convergence reconciler's target an explicit, versioned, three-source manifest:
the union of a pinned Claude-Code baseline and the agent-ways manifest, with the user
remainder defined by construction as everything in neither set.** The reconciler converges
`actual → (CC-baseline ∪ agent-ways-manifest)` and, because the user remainder is outside
that target, never touches it.

This is the **tree-wide generalization of the settings.json three-way merge**.
`settings_merge.rs` already proves the core move: carve a stable agent-ways-owned layer out
of an unmanaged remainder, idempotently, by tracking what we last applied. This ADR lifts
that move from one file to the whole tree — and, where the single-file merge stops at
two parties (ours vs. everything-else), the tree model **splits the remainder into the
Claude-Code floor and the user**, the distinction the file merge currently leaves implicit.

### 1. Three manifest legs

| Leg | What it is | Source of truth | Status today |
|---|---|---|---|
| **Claude Code baseline** | Files/dirs vanilla Claude Code owns or creates in `~/.claude`, **pinned to a real CC release tag** (e.g. `2.1.196`). | An empirical clean-room snapshot (see §2), supplemented by package introspection. | **New** — does not exist. |
| **agent-ways manifest** | What agent-ways projects: `PROJECTED_TREES`, `PROJECTED_FILES`, `PROJECTED_BINS`. | `git ls-files` in `$XDG_DATA/agent-ways`, via `ways manifest`. | **Exists** (`cmd/manifest.rs`). |
| **User remainder** | Everything in neither set. | Defined by construction — the complement of the union. | **Implicit today** (heuristic). |

The target the reconciler converges toward is **CC-baseline ∪ agent-ways-manifest**. The
user remainder is *not in the target set at all*. This is the principled replacement for
heuristic "don't clobber the user's stuff" guards: the user's files are not protected by a
special case — they are simply **not in the manifest**, so idempotent convergence has no
entry that would write over them.

This maps onto `settings_merge.rs`, with one deliberate refinement:

- **agent-ways-manifest = the owned layer** — the file merge's `ours` (the `hooks` +
  `WAYS_PERMS` slice). At the tree level, the **git-derived `ways manifest` *is* the record
  of what we own**, so the tree leg needs no separately stored base the way the file merge
  keeps `settings-applied.json` — git tracking plays that role.
- **user remainder = the unmanaged content** — the file merge's `(theirs − base − ours)`,
  present and left alone (ours-by-absence).
- **CC-baseline = the new third leg.** The file merge has no separate notion of the
  Claude-Code floor — it folds CC's default values into the unmanaged remainder. The tree
  model promotes that floor to its own leg, because at tree scale the difference between
  "Claude Code created this" and "the user created this" is load-bearing for cleanup and
  migration in a way it is not for a single settings field.

### 2. Capturing the CC baseline — empirical clean-room snapshot, pinned to a tag

The key technical fact: **Claude Code's `~/.claude` footprint is mostly runtime-emergent,
not unpacked from the npm package.** `settings.json` defaults, `projects/`,
`history.jsonl`, `sessions/`, and `file-history/` are created **when Claude Code runs**,
not when it installs. A baseline built only from package introspection would miss most of
the floor it is meant to describe.

So the authoritative capture method is an **empirical clean-room snapshot**: run vanilla
Claude Code with `HOME` pointed at an empty sandbox directory, exercise it minimally, and
record the resulting `~/.claude` tree. Static files that *do* ship in the package are
captured by package introspection and merged in. The result is **pinned to a specific CC
release tag**, which makes the baseline **versioned and reproducible** — a baseline for
`2.1.196` is a fact about `2.1.196`, regenerable by anyone with that tag and a sandbox.

**Decision on storage and shipping:** the baseline ships as a **committed snapshot file
per CC tag** in `$XDG_DATA/agent-ways` (versioned alongside the code that consumes it),
**generated by a `ways` subcommand** that performs the clean-room run. The committed
snapshot is what the reconciler reads at convergence time; the generator is what the
maintainer runs to refresh it when a new CC release moves the floor. This keeps the
runtime path offline and deterministic (no sandbox spin-up during a user's SessionStart)
while keeping the snapshot honestly reproducible. **Refresh cadence is maintainer-driven,
not per-release** — the baseline is refreshed when CC's footprint actually changes, and
skew between refreshes is absorbed by §3.

### 3. Tolerating CC version skew — the baseline is an allow-pattern set, not an exact list

The user's installed CC version will rarely equal the pinned baseline tag. If the baseline
were an exact file list, every patch-level CC release would produce spurious "unclassified"
files and risk the reconciler treating a genuine CC runtime file as user remainder (or vice
versa).

**Decision:** the CC baseline is consumed as an **allow-pattern set** (path globs /
prefixes — `projects/`, `sessions/`, `history.jsonl`, `file-history/`, `settings.json`,
`statsig/`, `todos/`, …), not an exact inventory. Classification asks *"does this path
match a known CC-owned pattern?"* rather than *"is this path byte-identical to the
snapshot?"* Patterns are far more skew-tolerant than file lists: a new session file under
`sessions/` is still obviously CC-owned. The per-tag exact snapshot remains the **evidence**
from which the pattern set is derived and audited, but the pattern set is the runtime
contract. Skew within the pattern set's tolerance is a non-event; skew that introduces a
*new top-level CC artifact* is the signal that the maintainer should regenerate (§2).

### 4. The `ways` surface — a new `ways classify`

**Decision:** add a new **`ways classify`** subcommand that emits the three-way
classification of a real `~/.claude` — for each path, which leg it belongs to
(cc-baseline / agent-ways / user-remainder) — rather than overloading `ways manifest` or
`ways status`.

Rationale, by single-responsibility:

- `ways manifest` answers *"what does agent-ways project?"* — one leg, derived purely from
  git, with no reference to a live `~/.claude`. Folding CC-baseline and live-tree
  classification into it would give it two reasons to change.
- `ways status` is a health/observability summary for the operator, not a per-path
  classification emitter.
- `ways classify` is the natural home for the **set operation over a live tree**: it
  consumes the agent-ways manifest (leg 2) and the CC baseline pattern set (leg 1), reads
  the actual `~/.claude`, and emits the three-way partition that install, repair, migrate,
  and cleanup all consume.

`ways classify` is the read-only classifier; the reconciler (`cmd/reconcile.rs`) is the
mutating consumer that acts on the partition.

### 5. Precedence when a path is claimed by both CC baseline and agent-ways

Some paths are claimed by **both** legs — `settings.json` is the canonical case: it is part
of CC's baseline floor *and* a file agent-ways writes into. **Decision:** the agent-ways
manifest takes precedence for *projection* (agent-ways is allowed to write the path), but
the **already-specialized three-way merge (`settings_merge.rs`) governs that write** — the
whole-tree convergence delegates any path that is both CC-baseline and agent-ways-managed to
the per-file merge that already knows how to combine a CC-default base, the agent-ways layer,
and user fields without clobbering the user. In other words: tree-level classification routes
`settings.json` to file-level classification; the coarse leg precedence (agent-ways > CC for
projection) selects *who may write*, and the fine-grained merge decides *what to write*. No
other current path is expected to be doubly-claimed; if more emerge, the same rule applies —
overlap routes to a path-specific reconciler, defaulting to "agent-ways may write, user
fields preserved."

### 6. Consumers — all four ADR-144 from-states, plus cleanup

The single explicit manifest is consumed by every entry condition ADR-144 defined, plus
cleanup:

- **fresh** — materialize `(CC-baseline ∪ agent-ways-manifest)` into an empty tree. This is
  what makes the **native projection installer fall out as "bootstrap shim + reconcile in
  fresh state,"** with almost no new code: there is finally an explicit target to materialize
  *into nothing*. Wiring this closes the 1.0.0 fresh-install bug (Context #1).
- **drifted** — re-materialize missing/broken entries of the union (repair).
- **out-of-date** — re-derive the agent-ways leg from the advanced `$XDG_DATA` HEAD,
  materialize the delta, prune orphans (update).
- **legacy-in-place** — the migrator classifies the old clone against all three legs:
  agent-ways files relocate to `$XDG_DATA`, the **user remainder** lifts to
  `$XDG_CONFIG`/`$XDG_STATE`, and CC-baseline files stay. The classifier is exactly the
  rescue mechanism ADR-144's sharpest Negative demanded.
- **cleanup** — the **user remainder is precisely the set that is safe to prune or flag**;
  conversely, agent-ways orphans (in the manifest's history but no longer git-tracked) are
  safe to remove outright. This is the principled form of the by-hand cleanup done in the
  session that motivated this ADR.

## Consequences

### Positive

- **The convergence target becomes explicit and testable.** ADR-144's "converge toward the
  manifest" gains a concrete, three-legged, versioned definition of *the manifest* — install
  / repair / migrate / cleanup all read one artifact instead of each guessing.
- **The 1.0.0 fresh-install bug has a principled fix.** Fresh install stops being a missing
  feature and becomes "reconcile in the fresh from-state against the explicit target,"
  exactly as ADR-144 promised it would fall out.
- **"Don't clobber the user" stops being a guard and becomes a set property.** The user
  remainder is untouched not because of a special case, but because it is not in the target —
  the most robust form of the protection, and the one least likely to regress.
- **One pattern, two granularities.** The settings.json three-way merge stops being a
  one-off; it is now the file-level instance of the same model the whole tree uses, which
  makes both easier to reason about.
- **Migration's rescue problem gets a real classifier.** ADR-144's "detect and rescue
  hand-edited core files" becomes a concrete `ways classify` output rather than an
  aspiration.

### Negative

- **A new external dependency surface: the CC baseline must track Claude Code.** agent-ways
  now maintains a model of a tree it does not own and that changes on Anthropic's clock. The
  pattern-set design (§3) absorbs most skew, but a CC release that introduces a new top-level
  artifact requires a maintainer refresh; until then that artifact classifies as user
  remainder, which is the *safe* failure direction (we leave it alone) but a misclassification
  nonetheless.
- **The clean-room snapshot is real machinery to build and keep honest.** A generator that
  spins up sandboxed CC, exercises it enough to emit its runtime files, and diffs the result
  is non-trivial and itself version-sensitive — and "exercise it minimally" is a fuzzy
  contract (which CC features must run to materialize which files?).
- **`settings.json` remains the doubly-claimed seam** (ADR-142 / ADR-144 already flagged it);
  this ADR routes it correctly but does not remove the shared-write risk — it just states
  precisely where the tree model hands off to the file model.
- **Pattern sets can be wrong in both directions.** Too broad, and a genuine user file under a
  CC-shaped path is misclassified as CC-owned and skipped by cleanup; too narrow, and a CC
  runtime file is treated as user remainder. The exact per-tag snapshot is the audit evidence,
  but tuning the pattern breadth is an ongoing judgment.

### Neutral

- `ways manifest` is unchanged; this ADR adds `ways classify` beside it rather than altering
  the existing leg.
- The CC baseline snapshot is versioned in `$XDG_DATA/agent-ways` and so is **replaced
  wholesale on update** like the rest of the application (ADR-142's `$XDG_DATA` durability
  contract) — losing it is a re-derive, not data loss.
- This ADR sharpens, but does not resolve, ADR-142's open `$XDG_STATE` ↔ Claude-Code-owned
  boundary (see Open Questions); it gives that boundary a *mechanism* (the CC baseline pattern
  set) without fixing where the line sits.

## Alternatives Considered

- **Leave the target implicit; keep heuristic classification.** The status quo. Rejected for
  the three consequences in Context — the fresh-install path stays unwired, and migrate /
  cleanup keep guessing. ADR-144 already rejected the weaker "did a prior sync write it"
  membership test for the agent-ways leg; this ADR extends the same reasoning to the CC and
  user legs.
- **Model the CC baseline by package introspection alone (no clean-room run).** Rejected:
  most of CC's `~/.claude` footprint is runtime-emergent, so an install-time inventory would
  miss `projects/`, `sessions/`, `history.jsonl`, `file-history/`, and the defaulted
  `settings.json` — i.e. most of the floor. Introspection is kept only as a *supplement* for
  the genuinely static files.
- **Pin the CC baseline as an exact per-tag file list (no pattern set).** Rejected: it is
  brittle under the inevitable version skew between the pinned tag and the user's installed
  CC — every patch release would manufacture spurious unclassified paths. The exact snapshot
  is retained as evidence; the *runtime contract* is the skew-tolerant pattern set (§3).
- **Extend `ways manifest` to emit all three legs instead of adding `ways classify`.**
  Rejected on single-responsibility grounds: `ways manifest` answers "what does agent-ways
  ship," derived purely from git with no live-tree or CC dependency. Bolting CC-baseline and
  live `~/.claude` classification onto it gives one command two reasons to change and couples
  a pure git derivation to an external-dependency model.
- **Generate the CC baseline live at each SessionStart (no committed snapshot).** Rejected:
  spinning up a sandboxed CC run on the user's machine at session start is slow, non-
  deterministic, and fragile; the committed-per-tag snapshot keeps the runtime path offline
  and reproducible, with regeneration a deliberate maintainer act.

## Open Questions

These are recorded deliberately undecided; they refine, and partly inherit, ADR-142/144's
open questions.

- **The ADR-142 `$XDG_STATE` ↔ Claude-Code-owned boundary.** The CC baseline pattern set is
  the natural place to *draw* this line — runtime state CC creates (`projects/<slug>/memory/`,
  per ADR-128) that overlaps agent-ways' own `$XDG_STATE` claims must land on one side. This
  ADR provides the mechanism but does not commit the boundary; auto-memory (ADR-128) sitting
  in CC's `projects/<slug>/memory/` is the specific unresolved overlap.
- **What "exercise vanilla CC minimally" must include** to materialize the full runtime
  footprint — which CC operations are needed to emit which files, and how the generator
  guarantees it captured the whole floor rather than a subset.
- **Pattern-set breadth and review process** — how broad each CC-owned glob should be, and how
  the maintainer audits a refreshed snapshot against the prior pattern set to catch new
  top-level artifacts.
- **Refresh trigger** — whether baseline refresh is purely manual (maintainer notices a CC
  release moved the floor) or gets a lightweight detector (a CI clean-room run that diffs the
  current snapshot against the latest CC tag and flags drift).
- **Whether this stays a separate ADR or folds into ADR-144.** Recommendation: keep separate —
  the CC-baseline capture method and the `ways classify` surface are each substantial enough to
  warrant their own recorded decision, and ADR-144 is already long.
