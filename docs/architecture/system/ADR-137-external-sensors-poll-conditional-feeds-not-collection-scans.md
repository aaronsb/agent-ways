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

# ADR-137: External sensors poll conditional feeds, not collection scans

## Context

[ADR-120](ADR-120-interactive-chat-tui-human-in-the-signal-loop.md) introduced a
**GitHub Project sensor** as the canonical external sensor: a human drags an
issue from "Ready" to "In progress" on the board, attend notices, and the
relevant agent gets poked. Issue #25 tracks it, and
`docs/attend-and-monitor/authoring-sensors.md` carries a walkthrough whose
skeleton — like the first proof-of-concept written for #25 — **snapshots the
whole board every poll and diffs it** (`gh project item-list --limit N`).

We prototyped that approach to validate it before accepting an ADR. **The
prototype disproved it.** Everything below was verified empirically against a
live board and the live API, not assumed.

**The snapshot-diff fails at scale, three ways.** A full board fetch (a) silently
truncates at the fetch cap (cards past it are invisible *and* absent from the
baseline), (b) can exceed attend's hard **10-second script-sensor timeout** in
`tools/attend/src/sensors/script.rs` — on expiry attend kills the child and
returns *nothing*, indistinguishable from the silent-swallow under investigation
in #44 — and (c) re-transfers the whole board every poll for a handful of changes.

**And it can't be made cheap, because Projects v2 is GraphQL-only.** The
efficiency trick every good GitHub client uses is the **conditional request**:
send a stored `ETag`/`If-Modified-Since`, and the server replies **`304 Not
Modified`** when nothing changed — nearly free, and it does not count against the
rate limit. Verified:

- **GraphQL responses carry no `ETag`.** A Projects v2 board can only be read via
  GraphQL, so it has **no conditional path** — every poll is a full scan. There
  is no "nothing changed, here's a 304."
- **REST feeds do support it.** The **Notifications API** returns `ETag`,
  `Last-Modified`, and **`X-Poll-Interval: 60`** (the server tells you how often
  to poll); a re-poll with `If-None-Match` returned **`304 Not Modified`**. REST
  issue lists carry `ETag`s too.

**This is how VS Code's GitHub integration actually works** — and confirms the
instinct that scanning a board is the wrong shape. The PR/Issues extension does
not mirror a board. It runs **scoped queries** (assigned to me, involving me,
this repo), uses **conditional requests** so idle refreshes are 304s, and
refreshes on an interval / on window focus. No whole-collection scan, no webhooks
(a desktop editor has no inbound endpoint).

**The catch that reframes the sensor.** The Notifications API is efficient
*because* it only reports subscription events — assignment, mention,
review-requested, comment, issue/PR state change. **A bare project-board column
move does not generate a notification.** So the *literal* "card dragged between
columns" trigger is the one thing GitHub gives no efficient signal for (scan-only,
or webhooks). The tractable, useful, efficiently-pollable signal is **"something
I'm involved with changed"** — which is exactly what VS Code surfaces and what
attend's existing `gh-notifications.sh` example already watches.

## Decision

**1. The bounded-poll contract (general).** A polling external sensor MUST finish
within the script-sensor timeout and bound its per-poll work. A full scan of a
collection that can grow without bound is disallowed as a sensor's primary
mechanism.

**2. Prefer conditional-request feeds.** Where the upstream supports
`ETag`/`If-Modified-Since` (REST: Notifications, issues, …), a sensor MUST use
conditional requests so the steady state is a `304` — cheap and rate-limit-safe —
and SHOULD honor the server's advised cadence (`X-Poll-Interval`). The marker a
conditional-feed sensor stores is the ETag / last-modified token, not a snapshot
of the collection.

**3. Scanning an unbounded collection is the anti-pattern** — most sharply a
GraphQL collection (Projects v2) that offers no conditional path. It collides
with the 10s timeout, truncates silently, and can't be made cheap. Documented
here so the next author doesn't rebuild it (we did, and rejected it).

**4. The GitHub sensor is notifications-based, not a board mirror.** It watches
the Notifications API for involvement events (assigned, mentioned,
review-requested, state changes) with conditional polling — the VS Code model.
attend already seeds this in `tools/attend/examples/gh-notifications.sh`; that is
the canonical pattern, and the `authoring-sensors.md` walkthrough is reoriented
from the board snapshot to it. Magnitude stays the author's lever (ADR-136):
review-requested loud, mention next, comment quiet.

**5. Raw board-column awareness is out of scope** for the efficient sensor —
GitHub provides no efficient signal for it. A user who genuinely needs "any card
moved columns" must go event-driven (`projects_v2_item` webhooks): a different
sensor shape (a local receiver, not a `gh` poll), named here as the graduation
path, **not built by this decision.**

## Consequences

### Positive

- Steady-state polls are `304`s — nearly free, rate-limit-safe, and server-paced
  by `X-Poll-Interval`. A sensor can poll often without cost when idle.
- Scales to a board/repo of any size: the feed is scoped to involvement, so cost
  is O(things that concern me), never O(collection).
- The timeout-swallow ambiguity (#44) is sidestepped — a conditional feed poll is
  tiny and can't be killed for being slow.
- Matches how real clients (VS Code) work, and reuses the existing
  `gh-notifications.sh` pattern instead of inventing a worse one.

### Negative

- The sensor does **not** see pure board-column moves — the original #25 framing
  ("human drags my card") is not fully served. #25 re-scopes from "project board
  sensor" toward "GitHub activity sensor," or narrows to webhooks for true board
  awareness.
- "Involvement" depends on GitHub's subscription model — an event you're not
  subscribed to won't surface. That is the same scoping VS Code lives with.
- Notification bookkeeping (read/unread, the `since`/`Last-Modified` marker,
  thread `reason` tiers) is its own fiddly surface to get right.

### Neutral

- `projects_v2_item` webhooks become the named future direction for board-column
  awareness, without committing to them now.
- The 10s timeout in `script.rs` is unchanged — the fix is cheap conditional
  polls, not a longer leash.

## Alternatives Considered

- **Bounded GraphQL board scan (what we prototyped).** Page items capped, detect
  on `ProjectV2Item.updatedAt`, tier detail by assignee. Rejected on evidence:
  GraphQL has no conditional requests, so it is structurally scan-only and
  O(board); it sat at ~3.4s for 127 items and would approach the 10s timeout a
  few hundred items later; and it still can't be made cheap when idle. It works
  on a toy board and degrades exactly where it matters. Kept as a documented
  dead-end, not shipped.
- **`search(query:"project:owner/N updated:>marker")` as the change-detector.**
  Rejected: `updated:` keys on the **issue's** content timestamp, so a pure card
  drag (no issue edit) is invisible to it, and it can't see draft cards. Verified
  against the live board.
- **Narrow a board scan to only the authenticated user's assigned items.** An
  earlier draft of this ADR. Rejected: it throws away project-wide awareness, and
  it is moot anyway once the sensor is notifications-based.
- **Pure webhook/event-driven sensor now.** Rejected for *now*: requires a local
  receiver and per-user webhook setup most single-developer users won't stand up,
  and abandons the zero-infrastructure `gh`-poll model. Kept as the escape hatch
  for board-column awareness specifically.
