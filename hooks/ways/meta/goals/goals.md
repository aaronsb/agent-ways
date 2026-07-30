---
description: When and why to set a /goal — running Claude in goal mode toward a completion condition, and the regime that creates
vocabulary: goal goal-mode autonomous loop completion condition keep working until evaluator continue persist bounded objective set a goal ralph regime evidence approvals operator turns stopping
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Goals

`/goal` is a Claude Code built-in: it sets a completion condition and keeps the session working turn-over-turn until a separate evaluator judges the condition met. It's the autonomy primitive — it removes per-turn approvals. This way is the *who/what/when/why*; the **goal-author** skill is the *how* (composing a well-bounded condition).

## When to set a goal (vs. just act)

Set a goal when the work is **multi-turn, has a checkable end-state, and you'd otherwise be re-prompting Claude to "keep going"** — a test suite to get green, a consistency pass across many files, a migration. Don't set one for a single-step task, or for open-ended exploration where the direction isn't settled yet — there, ordinary turns and signposts serve better.

## What changes in goal mode

- **Per-turn approvals are gone.** Claude continues on its own until the condition holds; the `◎` indicator shows the regime is active. Setting a goal is consent to that — the operator should know they've entered it.
- **The evaluator judges surfaced text, not the world.** It can't run commands itself, so a condition must be met by *shown evidence* (an exit code, a clean status), not by assertion. Author for evidence, not claims.
- **There is no model-side abort.** Only the operator (`/goal clear`), the met condition, a timeout, or session end stops it. Claude can always *decline* an action and surface a concern — the loop blocks stopping, it never forces an action — but it can't end the loop itself. So the bounds live in the condition.

## Author the condition deliberately

Because the condition is the whole contract once per-turn approval is gone, frame it with care: one measurable end-state, the evidence that proves it, a turn/time bound, scope limits, and a door clause for any irreversible action (stop before it, surface). The **goal-author** skill walks this collaboratively.

## See also

- the **goal-author** skill — composing the condition (the how)
- choices(meta) — surfacing curated decisions to the operator
