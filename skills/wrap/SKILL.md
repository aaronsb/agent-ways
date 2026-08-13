---
name: wrap
description: Wrap up a session at a natural seam — land in-flight work, square the TaskList honestly, write a copy-paste continuation prompt, then hand off to a directed /compact. Use when the user says "wrap up", "wrap this up", "let's wrap", "end of session", "checkpoint and compact", "hand off", or invokes /wrap.
allowed-tools: Bash, Read, Grep, Glob, TaskList, TaskCreate, TaskUpdate, TaskGet, AskUserQuestion, Agent
---

# Wrap

Close out a session at its end. This is the on-demand sibling of the
`meta/compaction-checkpoint` way — the way fires automatically near the context limit;
`/wrap` runs the same muscle whenever you decide the session is done.

**Wrap is not merge.** Landing is *iterative* — many merges happen across a session as
each piece of work lands. Wrap is *terminal* — it runs once, at the end. By the time you
wrap, the session's work should already be merged. So `/wrap` does **not** land work; it
confirms nothing got stranded, then produces the artifacts that outlive the session.

The deliverable is two things that survive a reset: a **TaskList that honestly mirrors
reality**, and a **copy-paste continuation prompt**. The skill ends by handing you a
ready-to-run `/compact` line — it cannot run it for you (see Step 4).

## Arguments

- `/wrap` — full flow: land work (offer), square the TaskList, write the handoff, hand off to `/compact`.
- `/wrap stay` — wrapping mid-work on purpose: **skip** the land/merge step. Don't land anything; just capture state accurately and produce the handoff. Use when the seam is "I'm pausing", not "I'm done".

## Step 0 — Read the context gauge first

Before anything else, check where the session sits. Same source as `context-status`:

```bash
ways context --json   # used / total tokens, pct filled; total auto-detected from model
```

The fill level calibrates the entire wrap, and is worth saying out loud — the user may be
wrapping early *on purpose*, and that's valid. Locate it against the ways trigger curves
(the `todos` way fires ~75%, `compaction-checkpoint` ~85%):

- **Early** (well below the curves — e.g. 300k/1M, 30%): lots of headroom. Say so plainly
  ("we'd be wrapping early — 300k of 1M used"), but **don't talk the user out of it** —
  end-of-day, task-switching, or banking a clean state are all good reasons. When early,
  `/compact` reclaims little and costs you the live thread, so in Step 4 *offer the lighter
  paths*: checkpoint-and-continue (no compact), or `/clear`+paste later. The handoff can be
  leaner — less accumulated context means less to reconstruct.
- **Late** (near or past the curves): compaction is imminent or already mandatory. This is
  the urgent case — be thorough and quick. A lot is about to be summarized away, so the
  continuation prompt and TaskList carry more weight; get them down before auto-compact
  fires on its own.

Let this scale Steps 2–3: **how much detail the handoff needs is proportional to how much
context is about to be lost.** A near-full window deserves a dense, complete prompt; an
early wrap can be crisp. Knowing the *total* window (1M vs 200k) is what makes that call.

## Step 1 — Loose-ends check (not a merge step)

Wrap assumes the session's work already merged. This step only confirms that and catches
anything stranded — it is **not** the place to run a full delivery. Assess in parallel:

```bash
git status --short              # uncommitted changes?
git branch --show-current
git log --oneline main..HEAD    # unmerged commits?
```

- **Clean / already merged** → the expected state. Move on.
- **Something stranded** (uncommitted changes, or commits never PR'd) → this is a guard, not wrap's job. Surface it and let the user decide: invoke `merge` to land it, or leave it parked.
- **Parked on purpose** (`/wrap stay`, or user chooses to leave it) → do **not** merge. Record *exactly* what's uncommitted, on which branch, and why it's parked — verbatim into Step 3 so post-reset-you can resume the partial work cold.

The rule: wrap never silently drops stranded work, and never lands it behind your back.
Either the user lands it (via `merge`), or it's described in the handoff.

## Step 2 — TaskList honesty pass (load-bearing)

This is the point of the skill, not a footnote. Post-compaction-you (or a fresh session)
inherits the TaskList with **zero memory of this conversation** — so the list must be
*true*, not optimistic. Use `TaskList` to read current state, then reconcile it:

**Close the books** (`TaskUpdate`):
- Mark genuinely-finished tasks `completed`.
- Retire stale, obsolete, abandoned, or aspirational tasks — don't carry cruft forward. A task that no longer reflects the plan is noise that will mislead the next session.

**Write what's real** (`TaskCreate`):
- One task per piece of *actual* remaining work.
- Each `description` must be resumable cold: file paths, decisions already made (so they aren't re-litigated), what was tried, and the next concrete step. The `subject` is imperative; `activeForm` is present-continuous.
- Mark the genuine next task `in_progress`.

**The bar is honesty.** Before moving on, sanity-check: does any "pending" task describe
work that's actually done? Does any real remaining thread have no task? A dishonest list
is worse than none — it hands the next session a confident lie.

**Then print the list.** Reconciling silently gives the user nothing to check against.
Enumerate every task — finished, in progress, and backlog — with its full description:

```
#1 [IN PROGRESS] <subject>
  <description verbatim: paths, decisions already made, what was tried, next concrete step>

#2 [OPEN] <subject>
  <description>

#3 [OPEN, blocked by #2] <subject>
  <description>

#4 [DONE] <subject>
```

Number in the order the next session should work them, not creation order. Mark blocking
relationships inline (`[OPEN, blocked by #2]`). Finished tasks get subject only — enough to
confirm they were closed deliberately, not enough to crowd the live work.

Do not compress the descriptions here. This block and Step 3's prompt carry different
weight: the prompt is the cold-start narrative, this is the per-thread detail, and a
summarized description is exactly the content post-reset-you cannot reconstruct.

## Step 3 — Write the continuation prompt

Produce a single copy-pastable block. Compaction keeps the session alive; this prompt is
the harder guarantee — it survives even a `/clear` or a brand-new window. Tailor sections
to the work, but cover:

```
Continue work on <project> (<path>) — <one-line what it is and its purpose>.
<build/test status: how to build, what's green>.

LANDED — <what shipped this session, with PR #s / commit refs; "merged to main, green">.

STATE — IN FLIGHT — <anything parked: branch, uncommitted files, why; from Step 1>.

INVARIANTS / GOTCHAS — <things NOT recoverable from git: crash-safety rules, ordering
constraints, "X is noise, trust Y", env quirks. Skip if none.>

TASKS — recreate these with the task list tool before starting; they are the working set.
<the Step 2 enumeration verbatim — number, status, subject, full description>.

DO NEXT — <the immediate next step, then 1-3 alternatives, each specific enough to start>.

CONVENTIONS — <project-specific workflow worth repeating: branch→PR→review→merge, lint
quirks, verification patterns>.

KEY FILES / BRANCHES — <the handful that matter, with paths>.
```

If a `/goal` is active, **restate the goal condition** at the top — it's the anchor the
post-compaction turns steer by, and it survives compaction.

The `TASKS` section carries the Step 2 enumeration in full and tells the next session to
rehydrate it with `TaskCreate`. Pasting this prompt into a `/clear`ed or brand-new window
arrives with an empty task list, so spell the instruction out — "recreate these as tasks
before starting" — rather than assuming the list travels with the prompt.

Keep it dense and concrete. The gold standard is a prompt a stranger could resume from.

## Step 4 — Hand off (compaction is gauge-dependent)

First, let Step 0 decide whether compaction even applies:

- **Early wrap** → `/compact` is *optional*; it reclaims little and ends the live thread.
  Lead with the lighter paths: checkpoint-and-continue (keep working in the same session —
  the TaskList and handoff are already saved), or `/clear`+paste later for a clean switch.
  Mention `/compact` as available, not as the default.
- **Late wrap** → compaction is the point. Make the directed `/compact` the headline action.

A skill **cannot** trigger `/compact` — Claude Code forbids hooks/skills from invoking
`/` commands. So when compaction is the move, end by handing the user a one-keystroke
directed compaction, and explain why directed beats generic:

> The synthesis above is now the freshest content in context. Run `/compact` with these
> focus instructions and it'll preserve the synthesis as the anchor instead of the system
> keeping whatever it guesses:

Then output the exact line, with focus instructions tailored to what should survive:

```
/compact keep the continuation prompt and TaskList state; <project>'s next step is <X>; drop the exploratory back-and-forth
```

Offer the alternative explicitly: for a hard reset (switching tasks entirely), `/clear`
then paste the Step 3 prompt into a fresh session — the old one stays in `/resume`.

Make the `/compact ...` line the **last thing** in your response so it's immediately
copyable. Do not claim the session is compacted — you've prepared it; the user pulls the trigger.

## Key Principles

- **The TaskList must be honest** — done is done, stale is gone, real work is captured with resume-detail. That's the load-bearing deliverable.
- **Print the list you squared** — an unenumerated honesty pass is unreviewable; the user should see every task and description without asking.
- **Prepare, don't pretend** — the skill can't self-compact; it hands off a ready `/compact`. Never report compaction as done.
- **Directed > generic** — a synthesis-anchored `/compact` keeps what matters; an unanchored one keeps what the system guesses.
- **On-demand, not automatic** — that's the whole reason this exists alongside the `compaction-checkpoint` way. You choose the seam.
- **Two survival layers** — TaskList + continuation prompt survive even a hard reset; `/compact` is the lighter same-session path on top. The prompt carries the tasks *and* the instruction to rehydrate them, because a fresh window starts with an empty list.

## Not for

- Authoring the code being wrapped — `/wrap` closes out work, it doesn't write it.
- Replacing the `compaction-checkpoint` way — that fires passively near the limit; this is the on-demand version. They share DNA on purpose.
- Actually performing compaction — it can't. It prepares everything and hands you `/compact`.

## See Also

- `merge` — Step 1 delegates here to land work.
- `compaction-checkpoint` (meta way) — the automatic sibling that fires near the context limit.
- `todos` (meta way) — the TaskList-at-compaction discipline this enforces on demand.
- `context-status` — check how much room is left before you decide to wrap.
