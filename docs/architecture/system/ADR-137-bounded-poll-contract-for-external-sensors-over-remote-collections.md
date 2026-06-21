---
status: Draft
date: 2026-06-20
deciders:
  - aaronsb
  - claude
related:
  - ADR-120
  - ADR-136
---

# ADR-137: Bounded-poll contract for external sensors over remote collections

## Context

[ADR-120](ADR-120-interactive-chat-tui-human-in-the-signal-loop.md) introduced a
**GitHub Project sensor** as the canonical external sensor: a human drags an
issue from "Ready" to "In progress" on the board, attend notices, and the
relevant agent gets poked. Issue #25 tracks the implementation, and
`docs/attend-and-monitor/authoring-sensors.md` carries a walkthrough whose
skeleton — like the first proof-of-concept written for #25 — **snapshots the
whole board every poll and diffs it against a stored baseline** (`gh project
item-list --limit N`).

That shape is correct for a board you'd actually drag cards on (dozens to low
hundreds of items). It does not survive a board with thousands of items, and the
failure is not graceful:

1. **Cap blindness.** `--limit N` silently truncates. Cards past the cap are
   invisible — a move on item N+1 never fires, and because the baseline is built
   from the same truncated fetch, the sensor never even registers that those
   cards exist.
2. **Timeout kill — and it looks like a bug, not a limit.** External script
   sensors run under a hard **10-second timeout** in
   `tools/attend/src/sensors/script.rs`: on expiry attend kills the child and
   returns *no observations*. A multi-thousand-item GraphQL fetch can exceed 10s,
   so the sensor is killed every poll and emits nothing — **indistinguishable
   from the silent-swallow symptom under investigation in #44.** A scaling limit
   that presents as a phantom sensor bug is the worst kind.
3. **Whole-collection re-transfer.** The entire board crosses the wire, is
   re-parsed, and the baseline is fully rewritten every poll, for a handful of
   actual changes. Cost is O(collection), not O(changes).

This is not specific to GitHub Projects. Every polling sensor that watches an
**unbounded remote collection** — `gh api notifications`, `kubectl get pods`,
`docker events`, a Jira board — faces the same three failures against the same
10s ceiling. The decision below is therefore a **general contract**, with the
GitHub Project sensor as the motivating instance.

The lever we already have is the magnitude hierarchy from the sensor design.
Crucially, the tiers are about **how much detail a change earns**, not about
*which* items we watch — the sensor needs project-**wide** awareness (something
moved *anywhere*), and assignment only decides how loudly and richly to report
it:

| Change (anywhere on the board) | Magnitude | Detail surfaced |
|---|---|---|
| A card **assigned to me** changes | 3.0 | rich — "your card #25 moved Ready → In progress" |
| A card **assigned to someone else** changes | 0.8 | terse — "item #NN updated" |
| An **unassigned** card changes / appears | 0.5 | terse — background awareness |

The 3.0 tier is the signal that must not be lost; the lower tiers are **event-lane
noise** ([ADR-136](ADR-136-split-addressed-messaging-from-the-sensor-observation-bus.md))
that the salience/refractory/governor stack exists to suppress — *they are
allowed to drop.*

### What the Projects v2 API actually supports

The polling shape is constrained by what the GitHub API can and cannot do. These
were verified empirically against a live project, not assumed:

- **No server-side ordering or filtering of project items by update time.**
  `ProjectV2ItemOrderField` exposes exactly one value, `POSITION`. There is no
  "most-recently-changed first," so a sensor cannot cheaply *peek* the top to
  learn whether anything changed.
- **The card-move signal lives on `ProjectV2Item.updatedAt`, not the issue.**
  Moving a card advances the **project item's** `updatedAt`; the underlying
  issue's own `updatedAt` does not change (it wasn't edited). So board-move
  detection must read the *item* timestamp.
- **The search `updated:>` qualifier is a trap for this use case.**
  `search(query:"project:owner/N updated:>T", type:ISSUE)` works, but `updated:`
  keys on the **issue's** content timestamp (edits, comments, close) — a pure
  card drag with no issue edit is invisible to it. It is not a move-detector.
- **There is no change cursor / epoch counter.** GraphQL cursors are opaque
  pagination positions, not "what changed since" handles.
- **The only real change *stream* is webhooks** — `projects_v2_item` `edited`
  events fire on field-value (column/Status) changes. That is *push*, not poll.

The consequence is unavoidable: **detecting board moves by polling is inherently
O(board)** — with no server-side `updatedAt` filter, the sensor must page the
items and compare each `item.updatedAt` to a stored marker. There is no cheap
"did anything change" probe. This is precisely why an unbounded board collides
with the 10s timeout, and why the decision below caps the scan and names
webhooks as the way out.

## Decision

**1. The bounded-poll contract.** A polling external sensor MUST complete within
the script-sensor timeout. Therefore a sensor over a remote collection MUST cap
the work it does per poll — never an *un*bounded snapshot whose size it does not
control. "Fetch everything and diff" is disallowed for any collection that can
grow without bound.

**2. One bounded scan per poll, on the item-level change timestamp.** Because the
API offers no server-side "changed since" for board moves (see Context), the
sensor pages project items in a **single bounded scan**, requesting only the
fields it needs to detect and tier a change: `id`, `ProjectV2Item.updatedAt`, the
Status field, and the assignee logins. It compares each `item.updatedAt` against
a stored marker timestamp; items newer than the marker are the changes. The scan
is capped at a fixed number of items per poll — full coverage at or below the
cap, documented loss above it.

**3. Tier the *detail* inline, with project-wide awareness.** The scan sees the
whole (capped) board, not just the user's cards — awareness is project-wide. For
each changed item, assignment decides only how much to say:

- assigned to the authenticated user → **3.0**, rich (`"your card #25 moved
  Ready → In progress"`);
- assigned to someone else → **0.8**, terse (`"item #NN updated"`);
- unassigned → **0.5**, terse.

The lower tiers are event-lane noise (ADR-136) and accumulate; the 3.0 tier
breaks the emission threshold on a single event. A two-phase variant (minimal
`id`+`updatedAt` scan, then rich detail only for changed IDs) is a permissible
optimization but not required — at human-board scale the single scan is simpler
and within budget.

**4. Make the timeout observable.** A sensor killed at the 10s boundary must not
be silent. `script.rs` already logs the kill to stderr; that diagnostic is
retained and treated as the canonical signal that a sensor is over budget — and
is part of resolving the #44 ambiguity, since "killed for being slow" must be
distinguishable from "ran and emitted nothing."

**5. Webhooks are the named escape hatch, not built now.** Past the cap, polling
is the wrong tool — the correct architecture is event-driven (`projects_v2_item`
webhooks *push* the column change). That is a different sensor shape (a local
receiver, not a `gh` poll) and is **out of scope here**; the ADR names it as the
graduation path so the polling sensor is never stretched past where it belongs.
No webhook work is committed by this decision.

This ADR supersedes the snapshot-diff skeleton in `authoring-sensors.md`: that
walkthrough is updated to the bounded-scan, detail-tiered shape so the doc and
the shipped example agree.

## Consequences

### Positive

- Project-**wide** awareness is preserved — the scan sees every (capped) card, so
  a move by anyone surfaces, while detail is tiered by assignment. The canonical
  workflow (#25) works without narrowing the sensor to only the user's cards.
- The phantom-failure mode is closed: a bounded scan can't be killed-by-timeout
  into looking like a swallow, removing one whole class of confusion from #44.
- The contract generalizes — `gh-notifications`, `kubectl`, and any future remote
  sensor inherit a clear rule (bounded work per poll, lossy beyond the cap)
  instead of each rediscovering the timeout the hard way.
- The expensive dead-ends are now documented (no server-side `updatedAt` filter;
  `search updated:>` misses moves), so the next author doesn't re-walk them.

### Negative

- Change detection is inherently **O(board up to the cap)** — there is no cheap
  "did anything change" probe, so every poll pages the capped item set even when
  nothing moved. Cheap per item (a few fields), but not free.
- The scan is **lossy above the cap.** Someone expecting "every card move
  surfaces on a 5,000-item board" must be told plainly it doesn't — that's the
  webhook graduation point.
- "Assigned to me" depends on cards actually being assigned. A board that tracks
  state purely by column with no assignees gets only the terse tiers — acceptable,
  but worth flagging to users.

### Neutral

- The 10s timeout in `script.rs` is left unchanged — the fix is a bounded scan,
  not a longer leash (a slow sensor must not be able to stall the tick loop).
- Webhook-based sensors become a named future direction without committing to
  them now.

## Alternatives Considered

- **Keep the snapshot-diff, just raise `--limit` and the timeout.** Rejected: it
  only moves the cliff. A bigger cap still truncates a bigger board, and a longer
  timeout lets one slow sensor stall the whole loop — the tick budget exists for
  a reason. Treats the symptom, not the shape.
- **`search(query:"project:owner/N updated:>marker")` as the change-detector.**
  Tempting — it's a real server-side "changed since" filter — but rejected on
  evidence: `updated:` keys on the **issue's** content timestamp, so a pure card
  drag (no issue edit) is invisible to it, and it can't see draft cards. It would
  silently miss the exact event the sensor exists to catch. (Verified against a
  live board.)
- **Narrow the sensor to only the authenticated user's assigned items.** This was
  the first draft of this ADR. Rejected on review: it makes the targeted query
  cheap, but it **throws away project-wide awareness** — the user explicitly wants
  to know that *something moved anywhere*, with assignment controlling detail, not
  visibility. The bounded full scan keeps awareness; assignment tiers the detail.
- **Pure webhook/event-driven sensor now.** Rejected for *now*: it requires a
  local receiver and per-user webhook setup, which most single-developer users
  won't stand up, and it abandons the zero-infrastructure `gh`-poll model that
  makes the example approachable. Kept explicitly as the scale escape hatch.
