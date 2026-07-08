---
status: Draft
date: 2026-07-07
deciders:
  - aaronsb
  - claude
related:
  - 128
  - 138
  - 143
---

# ADR-165: Loop-control bookends: start, develop, merge, release, wrap

## Context

The project already teaches every *stage* of software work as a way — design,
prototype, ADR, plan, build, PR, review, merge, docs, release each disclose
themselves when triggered. What it has never named is the **loop that carries a
piece of work through those stages**, or the ritual of **opening** and **closing**
a working session around it.

Two concrete pains motivated this:

1. **The tail is retyped every session.** Outside a `/goal` loop, the operator
   hand-types variations of "run a code review on this, fix the findings, and
   merge" many times a day. The doctrine for that tail already exists (the deliver
   workflow in `meta/workflows`, the review-before-merge default in
   `delivery/github`) but there was no single named invocation for it.

2. **There is a closing bookend but no opening one.** `wrap` (skill + `meta/wrap`
   way) is a fully-realized *terminal* ritual: it squares the TaskList, writes a
   continuation prompt, and hands the operator a directed `/compact`. Nothing
   symmetric exists at the *start* of a session — orienting to where work was left
   off, framing intent, and warming context before planning.

Underneath both is a shift already recorded in `core.md`: the method is no longer
"ADR-driven development." Prose states *claims*; claims are held to the evidence
the running system produces; "the ledger is not the whole method." An ADR is one
stage among several, and the *order* of the early stages is not fixed —
design-first, prototype-first, and adr-first are all valid openings depending on
**where the uncertainty lives** (`core.md`'s uncertainty-location map). The loop
has a **variable front and a stable tail**, and no way named that.

## Decision

Introduce a **five-verb loop-control family**, each an operator-invocable skill
with a corresponding way (the way is the *when/why*, the skill is the *how* —
ADR-138):

| Skill | Role | Way |
|-------|------|-----|
| `/start` | Open the session — orient to prior state, greenfield interview, gauge-guard, warm context, then recommend planning | `meta/start` (new) |
| `/develop` | Carry the core loop; pick the loop shape (variable front / stable tail) and **borrow** the stage skills | `meta/develop` (new) |
| `/merge` | Land an increment — the four-square review gate → merge → cleanup | `delivery/merge` (new) |
| `/release` | Publish a release — version, changelog, artifacts | `delivery/release` (exists) |
| `/wrap` | Close the session → hand to a new one | `meta/wrap` (exists) |

Five load-bearing choices:

1. **`/start` and `/wrap` are inverse gauge-aware bookends.** Both read the same
   instrument (`ways context`). `wrap` checks the session is near its *end* and
   scales the handoff to how much is about to be lost; `start` checks the session
   is near its *beginning* and **dissuades** starting if it is not — starting is a
   beginning-of-session act. Same instrument, opposite pole.

2. **Planning lives inside `/start`, not as a `/plan` skill.** Claude Code reserves
   plan mode; we do not claim that verb. `/start` gathers state *first*, so when it
   recommends planning the context is already warm — plan mode inherits an oriented
   situation instead of a blank one. Like `wrap` and `/compact`, `/start` cannot
   itself invoke plan mode (skills/hooks cannot fire `/` commands); it prepares the
   ground and hands the operator the trigger.

3. **`/develop` is a borrowing-router, not an orchestrator.** It establishes the
   loop, selects the front order by where the uncertainty lives, lays the TaskList,
   then delegates to the reviewer (the `code-reviewer` subagent), `/merge`, and
   `/release`, and lets the existing stage ways disclose. It does not reimplement
   what those already teach. (The monolithic-orchestrator reading was considered and
   rejected — see Alternatives.)

4. **`ship` splits into `merge` + `release`.** "Ship" carried release-weight —
   version bumps, changelogs, artifacts — which made it the wrong name for the daily
   act of landing an increment. `/merge` is the light, high-frequency tail (branch →
   commit → PR → review-gate → merge → cleanup, minus publish); `/release` is the
   occasional heavy publish (ship's former Publish step). The daily retype now maps
   to `/merge`.

5. **The review gate is a four-square, not a single policy.** `/merge` classifies
   the work on two axes and picks the path, surfacing the call when ambiguous:

   |                         | machine review: light | machine review: deep / swarm |
   |-------------------------|-----------------------|------------------------------|
   | **human gate: none**    | run it — quick review → auto-merge | swarm review + adversarial verify → agent-gated merge |
   | **human gate: required**| single-agent review → offer to read before merge | swarm review **and** operator approval before merge |

   The X axis is driven by complexity / blast-radius; the Y axis by whether the work
   sets direction (e.g. an ADR) and whether the operator has already read it.

### Skill ↔ way coverage

Most stages are already covered because `/develop` *borrows* them. New authoring is
concentrated in four places: `meta/start`, `meta/develop` (the variable-front /
stable-tail selector — the corpus's largest prior gap), `delivery/merge` (the
four-square + the previously-partial "fix/remediate per finding" coverage), and the
rename ripple through references to `ship`.

## Consequences

### Positive

- The daily "review, fix, merge" retype collapses to `/merge`.
- The session gains a symmetric opening ritual; `/start` warms context so planning
  starts oriented.
- The loop's variable-front / stable-tail shape is named and disclosed for the first
  time, grounded in the claim→evidence method.
- `merge` and `release` each become honestly-scoped; neither overloads the other.

### Negative

- A rename ripple: references to `ship` across ways, skills, and the skills catalog
  must all move to `merge`/`release`.
- Five coordinated surfaces (two new skills, one rename, one split, a hook, and
  new/revised ways) are more moving parts to keep coherent than a single skill.

### Neutral

- `/review` and `/code-review` (built-in review skills) are unchanged; the merge gate
  borrows the `code-reviewer` subagent for its automated review.
- Composes with `/goal`: the manual `/merge` gate is the single-shot form of what a
  goal loop automates, mirroring the `wrap` / `compaction-checkpoint` duality.

## Alternatives Considered

- **Fold the tail into `ship` / enrich `ship`.** Rejected: "ship" reads as
  release-weight, and the operator's daily act is iteration, not publishing.
- **`/develop` as a monolithic orchestrator** that runs every stage itself.
  Rejected: it would duplicate the stage ways and violate the borrow-don't-recreate
  posture; the mode-setter + shape-selector reading keeps it thin.
- **A standalone `/plan` skill.** Rejected: Claude Code reserves plan mode; planning
  folds into `/start` as an optional, context-warmed branch.
- **A dedicated non-agile naming metaphor** (film: action/take/wrap; compiler;
  systems). Explored at length; the plain SE verbs (start/develop/merge/release)
  won for being literal, low-fatigue, and already the words for the acts.
