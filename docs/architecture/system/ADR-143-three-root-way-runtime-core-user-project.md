---
status: Draft
date: 2026-06-29
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-142]]"
  - "[[ADR-140]]"
  - "[[ADR-111]]"
---

# ADR-143: Three-root way runtime — core, user, project

## Context

This is a child of ADR-142 (agent-ways 1.0). The spine moves the *application* to
`$XDG_DATA` and the *operator's own ways* to `$XDG_CONFIG`. This ADR records what that
split means for the **runtime that loads ways**.

Today the `ways` runtime scans **two** root classes:

- **global** — `~/.claude/hooks/ways` (`home_dir().join(".claude/hooks/ways")`, e.g.
  `tools/ways-cli/src/cmd/language.rs:18`, `permissions.rs:12`). The corpus builder scans
  it as the first root (`tools/ways-cli/src/cmd/corpus.rs:69-74`, "Scan global ways").
- **project** — `<project>/.claude/ways`, resolved from `CLAUDE_PROJECT_DIR`
  (`corpus.rs:89-94`), scanned second and namespaced per project.

`ways status` reports exactly these two: a single **"Global ways"** count and a **Projects**
list (verified against a live `ways status`: "Global ways: 115 total, 107 semantic" followed
by per-project counts).

A two-tier **precedence already exists** at file-resolution time: `resolve_way_file`
(`tools/ways-cli/src/session.rs:563-577`) looks up a way ID in `<project>/.claude/ways/<id>`
first and falls back to `~/.claude/hooks/ways/<id>` — "Project-local takes precedence." So
*project > global* is not new; what is missing is a tier *between* them for the operator's own
ways, because there is no operator-owned root distinct from global to resolve against.

The defect is that **"global" conflates two different owners in one directory.** A way
agent-ways *ships* and a way the *user wrote themselves* both land in `~/.claude/hooks/ways`,
indistinguishable to the runtime. Three consequences follow:

1. **You cannot shadow a shipped way without forking it.** To change how a core way behaves,
   the only lever is editing the shipped file in place — which the next update clobbers (and,
   pre-1.0, which made update unsafe to automate at all; ADR-142 §1).
2. **The corpus cannot tell app ways from user ways**, so neither can any tooling built on it
   (telemetry, tuning, `siblings`, permissions audit).
3. **The runtime's root model contradicts the storage model ADR-142 just created.** The spine
   puts core ways in `$XDG_DATA` and user ways in `$XDG_CONFIG` — two physically separate
   locations — but a two-root runtime can only see one "global." The runtime has to learn the
   split or the storage split is invisible at match time.

## Decision

**Promote the runtime from two roots to three — core, user, project — scanned as one runtime
class, with precedence `project > user > core` and dedup-by-name.** This splits today's
single "global" into its two real owners and makes shadowing a first-class, non-destructive
operation.

### 1. Three roots, one runtime class

| Root | Location | Owner | Updatable |
|---|---|---|---|
| **core** | `$XDG_DATA_HOME/agent-ways/hooks/ways` (today's `~/.claude/hooks/ways` content) | agent-ways (shipped) | replaced wholesale on update |
| **user** | `$XDG_CONFIG_HOME/agent-ways/ways` | the operator | never touched by update |
| **project** | `<project>/.claude/ways` (unchanged) | the repo | per-repo, committed |

They are scanned as **one runtime class**: a way is a way regardless of which root it came
from. The runtime tags each loaded way with its origin root so downstream consumers (corpus
namespacing, `status`, telemetry, permissions audit) can distinguish them — the tag is what
today's model structurally lacks.

### 2. Precedence `project > user > core`, dedup-by-name

When the same way *name* appears in more than one root, the higher-precedence root wins and the
lower one is dropped from the active set (dedup-by-name). Precedence runs **project > user >
core** — this *inserts the user tier* into the project-over-global order `resolve_way_file`
already implements (session.rs:563-577), rather than inventing precedence from scratch: a repo
can override anything for its own checkout; an operator can override a shipped way for all their
work; core is the floor. This is the mechanism that lets a user **shadow** a shipped way — drop a same-named way in `$XDG_CONFIG`, and it wins over core without editing or
forking the shipped file.

(Open question, below: whether shadow should be *whole-file replacement* by name or *field-level
overlay*. This ADR commits to whole-file-by-name as the 1.0 semantics and parks overlay.)

### 3. Corpus builder and `ways status` extend from two roots to three

- **Corpus** (`cmd/corpus.rs`): the "Scan global ways" step splits into "Scan core ways" +
  "Scan user ways," each content-hashed for staleness (the existing `global_hash` mechanism,
  `corpus.rs:71`, generalizes to per-root hashes). Project scanning is unchanged. Dedup-by-name
  is applied across the three before embedding so a shadowed core way isn't double-embedded.
- **`ways status`**: the single "Global ways" line becomes two — **core** and **user** — making
  the split observable, and surfacing shadowing (e.g. "3 user ways, 1 shadowing a core way").

### 4. Scope boundary

This ADR changes *where the runtime looks and how it resolves collisions*. It does **not** change
the matching pipeline (ADR-108 embeddings), progressive disclosure (ADR-105), or the single-binary
consolidation (ADR-111) — three roots feed the same matcher. It depends on ADR-142 for the
existence of the `$XDG_CONFIG` user root; absent the spine, "user root" has no home.

## Consequences

### Positive

- **Shadowing without forking.** An operator overrides a shipped way by name in `$XDG_CONFIG`;
  the override survives every update because user scope is never in the update blast radius
  (ADR-142 §7). This is the runtime-side payoff of the whole 1.0 restructure.
- **Clean open/closed boundary.** Core is extended (by user/project ways) without being modified —
  the SOLID open/closed principle expressed structurally rather than by convention. Today's only
  extension mechanism is *modifying* the shipped tree, the exact opposite.
- **App-vs-user becomes visible to every corpus consumer**, because origin is now a tag, not a lost
  distinction — tuning, telemetry, `siblings`, and the permissions audit can all scope by root.
- **The runtime model and the storage model finally agree** (ADR-142's two physical locations map
  to two runtime roots).

### Negative

- **Precedence is a new surface for surprise.** "My way isn't firing" can now mean "a
  higher-precedence root shadows it." `ways status` must make shadowing legible or it becomes a
  silent debugging trap — the same class of "topology-fragile, went stale once already" failure
  ADR-140 flagged for path-based checks.
- **Dedup-by-name needs a defined collision policy across three roots**, including the awkward case
  of a project way and a user way colliding with *different intent* — name collision is now load-
  bearing where before there was one namespace per scope.
- **More roots to scan at every corpus build**, with per-root staleness hashing; modest added cost
  and more cache-invalidation paths.
- **Whole-file shadow is coarse.** Overriding one frontmatter field (e.g. a trigger threshold)
  requires copying the whole way into user scope, which then *won't* track upstream improvements to
  the rest of that way — a real maintenance cliff the overlay alternative would avoid (parked).

### Neutral

- The two-root corpus namespacing (`encode_project_key`, `corpus.rs:94`) generalizes to three; the
  on-disk corpus format gains a root tag but is otherwise unchanged.
- Project scope is entirely unaffected in mechanism — only its position in a now-explicit precedence
  order is stated.

## Alternatives Considered

- **Keep two roots; put user ways in the same dir as core and tag by some marker.** Rejected: it
  re-creates the exact conflation ADR-142 §1 exists to remove, and leaves user ways inside the
  update blast radius (a marker file doesn't move them out of `$XDG_DATA`). The split must be
  physical to make update safe.
- **Field-level overlay instead of whole-file shadow** (user way patches named fields of a core
  way; unspecified fields inherit). More powerful and avoids the maintenance-cliff Negative — but
  needs a merge semantics, conflict rules, and a way to express "remove this field," none of which
  exist today. Deferred to a follow-on ADR; 1.0 ships whole-file-by-name and learns from it.
- **Four roots** (split project into project-shipped vs project-local, or add an org/team root).
  Rejected for 1.0 as scope the brief doesn't call for; the org-rollout need ADR-140 raised could
  motivate a team root later, but it would slot into the same precedence chain without re-deciding
  this model.

## Open Questions

- **Shadow semantics: whole-file vs field-level overlay** (committed to whole-file for 1.0; overlay
  parked as a follow-on).
- **Collision policy detail** when project and user ways collide by name with divergent intent.
- **Whether this child stays a separate ADR or folds into ADR-142.** Recommendation: keep separate —
  it has its own decision (precedence + dedup), its own consequences (the shadow maintenance cliff),
  and touches a distinct subsystem (the matcher/corpus), which is enough to stand alone.
