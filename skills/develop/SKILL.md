---
name: develop
description: Carry a piece of work through the development loop — pick the loop shape (variable front, stable tail) by where the uncertainty lives, lay the tasklist, then borrow the stage skills rather than reimplementing them. Use when the user says "let's develop this", "work through this feature", "how should we approach this", "what do we do first", or invokes /develop.
allowed-tools: Bash, Read, Grep, Glob, TaskList, TaskCreate, TaskUpdate, Agent
---

# Develop — carry the loop

The core loop, between `start` (open) and `wrap` (close). `develop` is a **router,
not an orchestrator**: it establishes the loop, chooses the front order, lays the
TaskList, then delegates to the stage skills and lets the stage ways disclose as you
hit them. It does not reimplement design, review, or merge — it *sequences* them.

## Step 1 — Name the load-bearing uncertainty

Before choosing an order, find the one question the work turns on, and locate it (the
`core.md` uncertainty map):

- **In the external world** — an API's behavior, a latency budget, a payload size.
- **In the trade-offs** — several plausible shapes, none yet obviously right.
- **In the direction** — the shape is known; the decision needs to be on record.

## Step 2 — Pick the front (variable)

The front reorders by where the uncertainty lives:

- **Prototype-first** → external claim. Probe the real system before committing. (`prototype` way)
- **Design-first** → open trade-offs. Deliberate and converge. (`design` way)
- **ADR-first** → direction is the point. Record the decision, then earn it with evidence. (`adr` skill/way)

Often more than one, in sequence — but lead with the stage that answers the
load-bearing question. Don't default to ADR-first out of habit; the ledger is part of
the method, not the whole of it.

## Step 3 — Lay the stable tail

The tail is the same almost every time: **build → review → fix → merge**. Lay the
stages into the TaskList (`TaskCreate`) with resume-cold detail — file paths, the ADR
reference if any, dependencies, isolation strategy. This is the `delivery/implement`
briefing discipline: defend the plan before creating tasks.

## Step 4 — Carry it, borrowing

Work the stages. At each one, **borrow** rather than rebuild:

- design / prototype / adr — the front stages and their ways
- build — author the code; `code` and `testing` ways disclose
- review + fix + merge — hand the tail to **`/merge`** (the four-square gate lives there)
- release, if warranted — **`/release`**

Let the stage ways fire on their own as you go; `develop` doesn't repeat what they
teach. When the piece is landed, that's one turn of the loop — pick up the next, or
`wrap` if the session is done.

## Key Principles

- **Route, don't reimplement** — `develop` sequences the stage skills/ways; it is thin.
- **Front by uncertainty** — lead with the stage that answers the load-bearing question.
- **Stable tail** — build → review → fix → merge, every time, via `/merge`.
- **Claim then evidence** — record decisions, then hold them to what the system proves.

## Not for

- The one-shot tail alone (review → fix → merge of already-built work) — that's `/merge` directly.
- Opening or closing the session — that's `/start` and `/wrap`.

## See Also

- `merge` — the stable tail this hands off to.
- `release` — when a landed increment warrants publishing.
- `start` / `wrap` — the session bookends this loop runs between.
- develop(meta) — the way behind this skill: the variable-front / stable-tail doctrine.
