---
status: Accepted
date: 2026-07-31
deciders:
  - aaronsb
  - claude
related:
  - ADR-155
---

# ADR-175: Standing delegation authorization satisfies the harness permission gate rather than overriding it

## Context

Claude Code 2.1.219+ injects a system-prompt section (`heron_brook`) carrying a single
constant, emitted as one block:

```
Do not call the AgentTool unless the user requested it
Do not use workflows or deep-research unless the user requested it
```

The gate is the `opus_5_prompt_bundle` model capability plus the killswitch flag
`tengu_fennel_godwit`, currently `false`. Confirmed live from inside two sessions during
the investigation that produced this ADR. Upstream: `anthropics/claude-code#80988`, open,
no staff response at time of writing.

There is no opt-out. `CLAUDE_INTERNAL_FC_OVERRIDES` is dead code in 2.1.220 — an
unconditional `return` precedes the env read. `DISABLE_GROWTHBOOK=1` makes matters worse,
because the killswitch defaults `false` and blocking the fetch guarantees the gate stays
open. Flipping `tengu_fennel_godwit` is all-or-nothing: the same capability gates five
other sections including `delivering_work_max` and `overcorrection`, both of which are
wanted. Buying delegation by dropping those is not a trade worth making.

The constant carves out no exception by agent type. `code-reviewer` is suppressed exactly
as `Explore` or `general-purpose` is. The observed effect is that the operator must name
delegation explicitly on every occasion, including for procedures whose own documented
steps call for it. That is the gate working as written, not a defect in it.

A second, independently gated nudge exists: the `subagent_steer_delegation` experiment,
arm `counter_steer`, injecting *"Subagents multiply cost and time… Delegate only when the
payoff clearly exceeds that overhead."* It was not observed in either sampled session.
Both samples share one account, and GrowthBook buckets on stable user attributes derived
from email and account uuid, so two sessions on one account land the same arm by
construction. The observation discriminates nothing; the gate is untested, not absent.

### The two gates have different shapes

`heron_brook` asks a permission question: *did the user request this?* Its condition is
satisfiable — and satisfiable in advance, in writing, by the operator.

`counter_steer` asks a cost/benefit question: *does the payoff exceed the overhead?* Its
condition is answerable by stating the payoff.

Neither is a prohibition. A countermand written as "ignore the above" fails both, and
wins only on recency — user-authored way text and user-authored `CLAUDE.md` carry the same
authority, so recency is the only lever such text has, and it is a weak one.

### Placement is not firing

`hooks/ways/meta/subagents/subagents.md` is the natural home for delegation doctrine, and
`scope: agent` delivers it to the main loop — `scan/mod.rs` filters the task surface to
ways whose scope contains `subagent`, so `agent` scope is main-loop delivery. But its
trigger surface is:

```
pattern: subagent|delegat|spawn.{0,30}agent|review.{0,30}\bpr\b|organiz.{0,30}docs
```

Every one of those tokens is something the operator says only once they have already named
delegation — the moment at which the gate is already satisfied and the text is redundant.
The cases that need it most (*"let's merge this"*, *"research X"*) do not match at all.
Correct text on a surface that never fires changes nothing.

Widening the pattern only partly helps, and is partly blocked. `review` is in the linter's
`COMMON_WORDS` and is flagged regardless of anchoring, so the review half is unavailable.
`merge` is not on that list, and `delivery/merge` already ships a lint-clean phrase pattern
(`merge (this|it|the pr)`) that matches *"let's merge this"* today — so reach was never the
whole problem.

The binding constraint is different. A way has to *win* a match to disclose, and measurement
during this work put `meta/subagents` fifth at 0.3 on *"delegate the code review to a
subagent"* — below a project-local way — even though that query names delegation outright.
A clause that must win a ranking to appear is a clause that sometimes doesn't. Sites don't
rank; they are read when the procedure is read.

## Decision

Encode the operator's authorization as **evidence that satisfies each gate on its own
terms**, placed in two tiers.

**Tier 1 — doctrine, in `hooks/ways/meta/subagents/subagents.md`.** Two sections. The
first states that invoking a skill or way whose steps call for delegation *is* the user's
request, and names the conditions under which delegation still requires asking. The second
answers the cost/benefit nudge by requiring a one-clause payoff statement at the point of
delegation. This tier explains the reasoning and is allowed to fire only when delegation
is already named; at that moment it supplies the *how*, not the permission.

**Tier 2 — operative clause, inline at each delegation site.** Every skill or way whose
own steps direct a delegation carries a short authorization clause at that step. Skills are
read at invocation, so the clause is present exactly when delegation is about to happen,
with no dependence on a way firing. This is what reaches the *"let's merge this"* case: the
merge skill loads, the clause sits in the step that dispatches `code-reviewer`.

**Scope is `AgentTool` only.** Workflows and deep-research are the second line of the same
constant and remain gated. They are token-invasive enough that the operator wants them
proposed and discussed, never invoked unprompted. A change that also frees them is out of
scope for this decision.

The authorization is not unconditional. Delegation still requires asking when it is not
part of an invoked procedure, when it spends significant tokens outside the stated task, or
when the operator has said to work solo.

It also grants *whether*, not *how wide*. A procedure that says "fan out" authorizes the
fan-out it describes, not an unbounded agent count; the width is stated before spawning and
asked for past roughly half a dozen. Without that bound the permission gate is answered and
the cost gate quietly isn't.

Tier 2 carries a corollary: a way that instructs the *opposite* verb at the same moment
re-opens the recency contest this decision declines to enter. `delivery/github` said to
"offer" a reviewer on the same trigger where `delivery/merge` now says to dispatch one.
Aligning both was part of the change, and any future way covering a delegation moment has
to be checked the same way.

## Consequences

### Positive

- Procedures that document a delegation step execute it, instead of stopping to re-ask for
  permission the operator already granted by invoking the procedure.
- The mechanism does not contradict the system prompt, so it does not depend on winning a
  recency contest against it.
- The payoff clause is useful on its own merits and remains correct whether or not
  `counter_steer` is ever present in a given session.
- Tier 2 is inherently drift-resistant in one direction: a delegation site that is deleted
  takes its clause with it.

### Negative

- Tier 2 duplicates a short clause across several files. Adding a new delegating skill
  means remembering to carry the clause, and nothing enforces it. The drift is fail-closed
  — a site without a clause grants nothing, so the model asks, which is the pre-change
  behaviour — but the tiers can still fall out of step, and Tier 1 must name only
  procedures that carry a clause at their own dispatch site.
- The change is unverifiable from inside the repository. Whether the model actually
  delegates without asking can only be observed in live sessions.
- If upstream flips `tengu_fennel_godwit` or removes the section, Tier 1 becomes an
  explanation of a constraint that no longer exists and will need pruning.

### Neutral

- `subagents.md` needed a correctness pass regardless, done alongside: it documented the
  tool as `Task` (the model-facing name is `Agent`; the `Task` matchers in `settings.json`
  are the harness event namespace and stay), and its roster listed only the `agents/`
  directory, omitting the harness built-ins `Explore` and `general-purpose` that the
  research way's fan-out step now names.
- `counter_steer` remains untested. Confirming its presence requires a sample from a
  different account, not another session on this one.

## Alternatives Considered

- **Widen `subagents.md`'s `pattern` to reach the moment-of-use vocabulary.** Rejected, but
  not because it is impossible: `review` is in the linter's `COMMON_WORDS` and unavailable,
  while `merge` is clean and already used in phrase form elsewhere. Rejected because reach
  is not the constraint — the way still has to win a match to disclose, and it ranked fifth
  on a query that named delegation explicitly. It would also make a frequently-firing way
  out of one that should fire narrowly.
- **Flip the `tengu_fennel_godwit` killswitch.** Rejected: all-or-nothing across the
  `opus_5_prompt_bundle` capability, which would also drop `delivering_work_max` and
  `overcorrection`.
- **`CLAUDE_INTERNAL_FC_OVERRIDES` / `DISABLE_GROWTHBOOK`.** Rejected: the first is dead
  code in 2.1.220; the second guarantees the gate stays open, since the killswitch defaults
  `false`.
- **Put the authorization in user `CLAUDE.md`.** Rejected: same authorship as way text, so
  it wins on recency alone, and it applies to every project rather than to the procedures
  that actually specify delegation.
- **Write the text as an explicit override of the system prompt.** Rejected: fails both
  gates on their own terms, and instructs the model to disregard its own instructions —
  a pattern that should not be normalized in this corpus regardless of whether it works.
- **Wait for upstream.** Rejected as the sole response: issue 80988 is open with no staff
  reply, and the friction is present in every session meanwhile. This decision is
  compatible with an upstream fix and is cheap to remove if one lands.
