---
name: start
description: Open a working session — orient to where work was left off, run a structured interview on a greenfield repo, check the context gauge to confirm it's really session-start, then recommend planning with context already warm. The opening bookend to /wrap. Use when the user says "let's start", "pick up where we left off", "what were we doing", "start a session", or invokes /start.
allowed-tools: Bash, Read, Grep, Glob, TaskList, TaskGet, AskUserQuestion
---

# Start — open the session

Open a working session by getting oriented *first*, then handing the operator a warm
runway into planning. This is the on-demand opening bookend to `wrap`: `wrap` reads
the context gauge to confirm the session is near its **end**; `start` reads the same
gauge to confirm it's near its **beginning**.

## Step 0 — Read the gauge, and guard the beginning

Before anything else, confirm this really is a start:

```bash
ways context --json   # used / total tokens, pct filled
```

Starting is a beginning-of-session act. Locate the fill level:

- **Early** (well below the curves — e.g. under ~30%): the expected state. Proceed.
- **Mid/late** (half or more of the window spent): **dissuade, don't pretend.** Opening
  fresh work deep into a used window strands it against the next compaction. Say so
  plainly, and offer the better move: `/wrap` if a piece just finished, or `/clear` and
  a fresh window if switching tasks. Only continue a `start` here if the operator
  insists.

## Step 1 — Read the state (where did we leave off?)

Assess in parallel — the opening move depends entirely on what you find:

```bash
git rev-parse --is-inside-work-tree 2>/dev/null   # a repo at all?
git status --short
git branch --show-current
git log --oneline -5
```

Also check for continuity markers: the `TaskList` (via the tool), any tracking files
under `.claude/`, and the `CLAUDE.md` where this was invoked. `tracking(meta)` is the
discipline here — check for existing tracking before beginning anything.

## Step 2 — Branch on what you found

- **Work in flight** (uncommitted changes, an open branch, live TaskList tasks) → this
  is a *resume*, not a cold start. Summarize where things stand and what the next
  concrete step is. Reconcile the TaskList honestly before proposing work.
- **Clean `main`, prior history** → a fresh piece of work in an established project.
  Read `CLAUDE.md` and recent history for the project's shape; propose candidate
  directions grounded in what the repo is.
- **Greenfield** (empty repo, or no repo) → nothing to pick up. Do **not** invent a
  plausible task. Go to Step 3.

## Step 3 — Greenfield: structured interview

When there's nothing to resume, the referent of "let's start" is itself the
uncertainty — name it, don't hunt for it. Interview the operator, structured
(`AskUserQuestion` where it helps): what are we building, what's the end-state, what
constraints bound it, what's the first load-bearing question. Enough to frame intent —
not a full plan yet.

## Step 4 — Recommend planning (warm context, can't self-invoke)

By now the context is *warm* — state read, intent framed. That's the whole reason the
order is start-then-plan: plan mode should inherit an oriented situation, not a blank
page.

A skill **cannot** flip plan mode for you — Claude Code forbids skills/hooks from
firing `/` commands (the same constraint `wrap` has with `/compact`). So either run a
lightweight planning pass inline, or **recommend** the operator enter plan mode now
that the ground is prepared. Don't claim to have entered a mode you can't.

If the work is multi-turn with a checkable end-state, mention `/goal` and the
`goal-author` skill as the autonomy path.

## Key Principles

- **Gauge first** — start is a beginning-of-session act; dissuade a mid-session start.
- **Read before proposing** — the opening move is dictated by the actual state, not a guess.
- **Greenfield gets an interview**, never an invented task.
- **Warm the context, then hand off to planning** — prepare, don't pretend to enter plan mode.

## Not for

- Doing the development work itself — that's `/develop` and the stage skills.
- Mid-session resumption deep into a used window — that's a `/wrap` + fresh window, not a start.

## See Also

- `wrap` — the closing bookend; same gauge, opposite pole.
- `develop` — the loop `start` opens into.
- `goal-author` — composing a `/goal` condition when the work wants autonomy.
- `context-status` — the gauge `start` reads in Step 0.
