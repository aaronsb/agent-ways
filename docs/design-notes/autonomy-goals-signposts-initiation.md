# Autonomy as a Layered System: Goals, Signposts, and the Initiation Pattern

> **Type:** Design note (not an ADR)
> **Status:** Working draft, subject to revision
> **Cites:** ADR-138
> **Motivates:** a future signpost-convention way, a workflow way, the deliver workflow, and continuance guidance

## What this note is

This note reads how agent-ways gives Claude Code *more autonomy without trading
away the friction that forces thinking*. It argues that most of the machinery we
reached for already exists — the autonomy primitive ships in Claude Code itself —
and that the project's real contribution is a thin **authoring + surfacing layer**
on top of it. It names the pattern future decisions in this space should honor
(the initiation pattern, the signpost, the substrate ladder, the gate taxonomy,
the safety stack) so the ADRs and ways it motivates can cite a frame rather than
re-derive it.

It is deliberately not an ADR. It decides nothing; it describes how we are
choosing to read the autonomy surface. If the reading is wrong, the note is
updated and affected artifacts revisited.

## The primitive we didn't have to build: the goal loop

Claude Code's built-in `/goal` *is* the autonomy primitive. It sets a completion
condition and keeps the session working turn-over-turn until a separate evaluator
(a fast model) judges the condition met — injecting the evaluator's reasoning as
guidance each turn, auto-clearing when satisfied. We were about to design a
parallel "goal loop"; the built-in already provides the mandate-to-continue, the
done-criteria, the one-active-at-a-time, and `--resume`.

Three properties of the built-in shape everything downstream (verified against the
official docs):

- **The evaluator judges *surfaced text*, not the world.** It can't run commands
  or read files. So a condition is only as good as the evidence Claude *shows* —
  which is why conditions must demand proof (an exit code, a clean status), not
  assertion. A loop that rewards convincing prose over real results is the failure
  mode; evidence-phrasing is the antidote.
- **There is no model-side abort.** Claude cannot clear its own goal — only the
  operator, the met condition, a timeout, or session end can. The bounds therefore
  have to live *in the condition*.
- **Goal mode and the auto-mode classifier are orthogonal.** A goal never bypasses
  the per-tool-call safety classifier; they stack.

## The initiation pattern

Substantial work should open the same way, whatever executes it:

**assess → align → plan → [greenlight] → dispatch**

1. **Assess (shallow).** Read just enough to make the next step concrete. Its only
   job is to *earn an informed conversation* — deep-planning before alignment burns
   context on a direction the human would redirect in one sentence.
2. **Align.** Interview the operator to reach shared agreement. This is the one
   high-leverage human touch-point.
3. **Plan.** Develop the detailed plan from the alignment.
4. **Greenlight** — a review of the plan before dispatch. **Waived in goal mode**,
   because setting the goal *was* the greenlight (see Gates).
5. **Dispatch** to the substrate that fits (see Ladder).

`/merge` (formerly `/ship`, split per ADR-165) is the first real instance: its ad-hoc review-gate pause and "merge or
squash?" question are an embryonic alignment step bolted onto a substrate-first
skill. Refactoring it means lifting those into this pattern.

## Signpost events

The surfacing at **align** (and at gates) is a *signpost*: Claude does the work,
forms an opinion, and presents a **curated set of directional choices with a
recommendation** — "which direction do we *agree upon*." It is simultaneously
directional (the human picks) and informational (it teaches the human the current
state). The deepest property: **the menu is itself the evidence that Claude
understood** — a signpost is proof-of-work, not a quiz.

The balance it strikes: **human attention is the scarce resource.** Complexity
lives with Claude; each human touch-point should be high-leverage and low-load.
Fewer, better signposts — not more approvals. A fixed *spine* of archetypes (act
on all / triage to critical / rethink the approach / log the rest) carries most
cases; Claude fills the specifics and the recommendation per situation.

## The substrate ladder

The detailed plan dispatches to one of three execution substrates, in increasing
setup cost and yield:

| Substrate | Shape | Cost / yield |
|---|---|---|
| Tasklist (linear) | one driver, sequential | lowest setup, watch loosely |
| Tasklist + subagents | fan-out within stages, top-level driver | medium |
| Workflow tool | deterministic orchestration, fan-out + verify + synthesis | highest setup, highest yield, background |

The choice is **not a separate question** — it falls out of the alignment
interview (how many independent sub-tasks? parallelizable? how risky? how closely
will you watch?). The human reasons about *the work*, never *the machinery*.

## Two kinds of gate

Goal mode waives *direction* gates, not *consequence* gates:

| Gate | Asks | Under goal mode |
|---|---|---|
| **Greenlight** | "proceed with this *direction*?" | **waived** — the goal encoded the direction |
| **One-way door** (merge, publish, destructive) | "accept this irreversible *consequence*?" | **still stops** unless the condition explicitly authorized that door |

"Implicit acceptance" extends to direction, not consequence. The `◎` indicator
makes the regime legible — which is what makes the hand-off fair rather than
sneaky.

## The three-layer safety stack (the coffin corner)

Goal mode + the auto-mode classifier is a narrow, flyable envelope — active
margin-management, not a setting. Three complementary layers, at different scopes,
make it safe:

| Layer | Scope | Catches |
|---|---|---|
| Authoring-time bounds | the condition | doors ruled out before launch; the (evaluator-judged, soft) escape clause |
| Per-trajectory (Claude) | the whole contract | drift the classifier can't feel; Claude **declines + surfaces** — the loop stalls safely, it never executes the bad action |
| Per-action classifier | one tool call | "is *this call* immediately destructive?" — independent of the goal |

Because there is no model-side abort, the per-trajectory layer is *decline +
surface loudly*, not *abort*. The Stop hook blocks **stopping**, never an
**action** — so nothing irreversible executes while the human is the off-switch.
The floor holds.

## Where the pieces live (ADR-138)

Per ADR-138 (skills own the *how*; ways own the who/what/where/when/why):

- **Built-in `/goal`** — the *how* of the loop (harness-provided).
- **`goal-author` skill** — the *how* of composing a bounded condition.
- **`meta/goals` way** — the 5W of goal mode (when to set one vs. just act).
- **The signpost** — a *convention* (a future way): how to surface a decision.
- **Workflows** — a third *how*-carrier alongside skills and macros; a future
  **workflow way** owns *when to reach for one*.

## What we deliberately did not build

Per prototype-before-accept: a parallel goal system, a one-way-door tripwire hook,
a custom goal loop with a first-class abort, and a model-side kill switch — all
**deferred, unbuilt**. The floor (decline + surface) plus the auto-mode classifier
already make the regime safe; the speculative apparatus must prove it's needed
before it earns code. The session validated the minimal version end-to-end:
`goal-author` produced a goal, goal mode drove a multi-item campaign, and the door
clause parked it at an open PR instead of merging.

## Open threads this motivates

- A **signpost-convention way** so every skill surfaces decisions consistently.
- A **workflow way** + the **deliver workflow** (`develop→pr→review→remediate→merge`).
- **Continuance** guidance: offering a handoff near compaction, reconsidering the
  checkpoint threshold for large (1M) windows, and `/goal`-enabled continuation.
- Refactoring **`/merge`** (formerly `/ship`) into the first explicit instance of the initiation pattern.
