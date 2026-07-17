---
status: Draft
date: 2026-07-17
deciders:
  - aaronsb
  - claude
related:
  - 120
  - 124
  - 129
  - 136
---

# ADR-168: Instance-addressable directed messaging for same-cwd attend siblings

## Context

Directed `@Nickname` messaging in `attend-chat` is **cwd-keyed**. `resolve_nickname`
(`tools/attend-chat/src/chip/routing.rs`) maps a nickname to a `KnownIdentity.cwd`,
and `write_signal` posts to `signals_base/<encoded-cwd>/`. The cwd *is* the routing
address.

Two prior decisions collide on this point:

- **ADR-129** gave same-cwd sessions distinguishable identities via instance
  suffixes — `@Tamsin-alpha`, `@Tamsin-beta` — so the legend and Tab-completion
  can tell two Claude Code sessions in one working directory apart. This is a
  **display + completion** affordance: `with_instance` decorates the nickname,
  but `KnownIdentity.cwd` is *identical* for both siblings.
- **ADR-136** established the message lifecycle and the invariant that there is
  **one attend agent per canonicalized cwd** (the `last_inbound` same-cwd filter
  in `sensor-peers` depends on it).

The consequence is a promise/behavior mismatch. `@Tamsin-alpha` and
`@Tamsin-beta` *display* as two addressable agents but *resolve to the same cwd*.
`resolve_recipients` dedups destinations by cwd dir, so a message addressed to
both collapses to a single inbox write; and because the inbox is cwd-keyed, a
message addressed to *one* sibling is not delivered to that sibling specifically —
it lands in the shared cwd inbox. There is no way, under the current model, to
direct a message to exactly one of several instances sharing a cwd.

This surfaced as a user-reported symptom: mentioning two agents appeared to
deliver to only one. The routing fix in `c7c938f` (address every recipient in a
leading `@`/`#` run) resolved the general multi-recipient case, but the same-cwd
sibling case is not a parsing bug — it is a limit of cwd-keyed addressing, and
resolving it is a design decision rather than a patch.

Scope note: this ADR is specifically about *addressing precision for co-located
instances*. The separate "directed sends don't echo in the sender's own view"
defect is a display concern fixed independently (issue #370 / its PR) and does
not depend on this decision.

## Decision

**Proposed (pending debate — status Draft).** Adopt **Option A** now: treat a
mention that resolves to a cwd hosting multiple live instances as **cwd-scoped**
addressing, and make the UI say so — collapse sibling suffixes at send time and
report "reaches all instances in `<cwd>`" in the status line / echo. This keeps
ADR-136's one-inbox-per-cwd invariant intact and ships a small, honest change.

If instance-*precise* delivery proves to be a real need (not just legend
distinguishability), escalate to **Option C** (session-id addressing) as the
follow-on decision — it removes the cwd/instance coupling at the root and is a
cleaner long-term model than sub-inboxing (Option B). This ADR does not commit to
C; it records that C is the preferred path *if* precision is required.

The choice hinges on one question for the deciders: **is addressing one specific
co-located instance a workflow anyone actually needs, or is distinguishing them
in the legend enough?** The answer selects A alone vs. A-then-C.

## Consequences

### Positive

- Removes the display-vs-routing mismatch: what the UI says a mention does
  matches what the bus does.
- Option A is cheap and preserves the ADR-136 invariant, so it can land without
  reworking the sensor's `last_inbound` filter.
- Framing the decision around "is instance-precise delivery a real need" keeps us
  from building instance inboxes speculatively.

### Negative

- Under Option A, ADR-129's per-instance *addressability* remains display-only;
  users who read `@Tamsin-alpha` as "only alpha" must learn it means "alpha's
  cwd, both siblings." The UI copy has to carry that.
- Deferring C means the precise-delivery capability, if later needed, is a second
  migration rather than one move now.

### Neutral

- Either precise option (B or C) requires putting instance/session identity on
  the wire and revisiting the ADR-136 "one attend per cwd" invariant, since two
  instances in a cwd would then each read a distinct inbox lane.

## Alternatives Considered

- **Option A — Accept cwd-level addressing; make the UI honest.** No new inbox
  structure. At send time, siblings collapse to their shared cwd; status/echo
  state the message reaches every instance there. *Chosen as the immediate step.*
  Cheapest, invariant-preserving. Cost: cannot target one sibling.

- **Option B — Instance-keyed sub-inboxes.** Extend the routing key from cwd to
  `(cwd, instance-id)`; write to `signals_base/<encoded-cwd>/<instance>/` and have
  each instance read only its own lane. Faithful to ADR-129's promise, but layers
  instance structure onto the cwd dir, still couples routing to cwd, and forces a
  rework of the ADR-136 same-cwd `last_inbound` filter. Rejected as the more
  complex of the two precise options with no offsetting benefit over C.

- **Option C — Session-id addressing.** Route directed sends by target session
  UUID (already unique per session and the key ADR-129 heartbeats use) rather than
  by cwd: the signal carries a target session id; a recipient accepts signals
  addressed to its own id. Decouples routing from cwd entirely, handles same-cwd
  siblings for free, and is more precise for directed messaging generally.
  Preferred *if* instance-precise delivery is required — but a larger change to
  the addressing model (`accept_path`, target resolution, the encode scheme), so
  not adopted pre-emptively.

- **Do nothing.** Leave the mismatch. Rejected: the UI actively implies a
  capability the bus does not provide, which is the exact shape of the original
  bug report.
