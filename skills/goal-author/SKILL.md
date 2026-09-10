---
name: goal-author
description: Help the operator and Claude jointly compose a well-structured `/goal` condition before it's set — clarifying the end-state, the evidence, and the bounds. Use when the user wants to set a goal, frame a goal, "give Claude a goal", run Claude in goal mode, or asks for help writing a goal condition. Not for clearing a goal (`/goal clear`), not for running multi-stage delivery, and it does not invoke `/goal` itself — it hands you a condition to set.
allowed-tools: Bash, Read, Grep, Glob, AskUserQuestion
---

# Goal Author

`/goal` is a Claude Code built-in: it sets a completion condition and keeps Claude
working turn-over-turn until a separate evaluator judges the condition met. The
mechanics live in the canonical reference — read it for the "how":

> https://code.claude.com/docs/en/goal.md

This skill is the judgment that doc can't make for you: **how to author a
condition worth handing to that loop.** A vague goal sends an autonomous loop in a
vague direction. A well-bounded one is the single highest-leverage moment the
operator gets — once it's set, per-turn approvals are gone — so it's worth
authoring deliberately, and authoring it *together*.

## Why together

The operator holds the intent and the bounds that matter; Claude holds the read of
the actual work. Neither alone writes a good condition. The shape is **assess →
align → draft**, with Claude doing the work of understanding *first* so the
operator's choices are informed, not cold. The point isn't to extract a spec from
the human — it's to converge on a shared one.

## Procedure

### 1. Assess (shallow)

Read just enough to make the conversation concrete: the current state, what "done"
would plausibly look like, and which irreversible actions this work *could* reach
(push/merge, publish, deploy, destructive deletes). Don't deep-plan — the only job
of this step is to earn an informed interview. A few `git` / `Read` / `Grep`
calls, not a survey.

### 2. Align (interview)

Propose a draft condition, then reach agreement with the operator on:

- **The end-state** — one measurable thing, not a vibe.
- **The evidence** — what *shown output* proves it (the evaluator judges what
  Claude surfaces in the conversation, not the world — see Key rules).
- **The bounds** — a turn or time cap, and the scope (which files/areas are in play).
- **The doors** — for each irreversible action the work could reach, rule it *in*
  or *out*. Default: out. The goal stops before it and surfaces.

Use `AskUserQuestion` with curated options and a recommendation. The menu is itself
how Claude demonstrates it understood the work — a signpost, not a quiz.

Pace the interview: one to three questions per turn; after every two answers, reflect
the draft back ("I now believe the goal is X. Tell me where I'm wrong"); cap at nine
questions. Past nine, the interview is trying to plan the work as well as bound it.
Accept the gaps, write each one into the condition as an explicit bound, and draft.

The interview has converged when the four items above are settled and you can also
state the non-goals (what the loop must leave alone) and the assumptions that, if
false, change the condition. The `start` skill carries the fuller eight-point
convergence checklist for framing a piece of work; a condition needs only these.

### 3. Draft (the condition)

Emit a single paste-ready condition string. The operator sets it. **This skill
does not run `/goal`** — composing and setting are deliberately separate steps so
the human reads the condition before entering the regime.

## What a good condition contains

- **One measurable end-state**, phrased so it's met by *shown evidence* — an exit
  code, a clean `git status`, a passing check — not by assertion.
- **A turn/time bound** — `…or stop after N turns` — so a stuck loop ends.
- **Scope constraints** that matter — `…without modifying files outside src/auth`.
- **A door clause** for any irreversible action ruled out, phrased so the evaluator
  can recognize the halt-state from the conversation — e.g. `…stop before any push
  or merge and surface for approval`.

Example:

```
the auth/ unit tests pass (npm test output shown, exit 0) and lint is clean,
without touching files outside src/auth, or stop after 15 turns;
stop before any push or merge and surface for my approval.
```

## Key rules

- **The evaluator reads surfaced text, not the world.** It can't run commands or
  read files itself, so the condition must be satisfiable by what Claude *shows*.
  This is why evidence beats assertion: "tests pass" clears on the claim; "npm test
  exits 0, output shown" clears on the proof — and that's the antidote to a loop
  optimizing for convincing prose over real results.
- **There is no model-side abort.** Once set, Claude cannot clear its own goal —
  only the operator (`/goal clear`), the met condition, an evaluator timeout, or
  session end can. A door clause is the only Claude-adjacent early exit, and it's
  soft (the evaluator judges it from surfaced text). Claude can always *decline* a
  destructive action and surface — the loop blocks stopping, it never forces an
  action — but it can't end the loop itself. Author the bounds accordingly.
- **Setting a goal is a regime change.** It removes per-turn approvals; the `◎`
  indicator shows it's active. Make sure the operator knows they're entering it —
  that legibility is what makes the hand-off fair.
- **One goal per session.** Setting a new one replaces the active goal.

## Not for

- Clearing or inspecting a goal — that's `/goal clear` and bare `/goal`.
- Running the work — this only authors the condition.
- Multi-stage delivery orchestration — that's a workflow, not a goal.

## See also

- the canonical `/goal` reference — https://code.claude.com/docs/en/goal.md
- the **skills** way (`meta/.../skills`) — skill-vs-way conventions in this repo
