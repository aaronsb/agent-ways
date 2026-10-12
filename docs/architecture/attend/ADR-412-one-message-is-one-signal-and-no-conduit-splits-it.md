---
contract: adr/v1
kind: decision
verb: change
capability: attend
basis:
  - operator: aaronsb
    level: directed
    said: "I realized something watching you two communicate: the message 1/2 and 2/2 etc is a holdover from the monitor approach - the claude code monitor only allowed a maximum number of characters through so each message had to be split up. I think we should fix this flow"
    via: "attend messages to the head session, 2026-10-12 10:56, #open"
  - operator: aaronsb
    level: directed
    said: "one message needs to be sent is all"
    via: "attend messages to the head session, 2026-10-12 10:56, #open"
  - operator: aaronsb
    level: directed
    said: "ok, so what we're going to do is fix the multipart message (remove it, basically)"
    via: "attend messages to the head session, 2026-10-12 10:58, #open"
  - evidence: "McKay, receiving through the attend mod in attend-collaborator, 2026-10-12: the cuts of a multi-part send land mid-sentence; each part has its own ~id, so a reply threads to one part rather than the message; the parts already arrive together in one turn"
  - evidence: "Monitor probe, Claude Code 2.1.296, 2026-10-12: a Monitor command printed single lines of 499, 500, 501 and 520 characters. The 499- and 500-character lines arrived whole; the 501- and 520-character lines arrived as their first 500 characters followed by ...(truncated). A 3,000-character line arrived as its first 500 characters and the marker. Three 296-character lines printed together arrived whole in one notification, and twelve 489-character lines arrived as one notification cut at 3,000 characters. The full output stays in the Monitor task's output file."
  - precedent: ADR-406
  - precedent: ADR-122
agent:
  name: Claude
  model: claude-opus-5-5
amends:
  - ADR-406#Decision
  - ADR-122#Decision
considered:
  - operator: aaronsb
    said: "Accept the cut (Recommended)"
    via: "AskUserQuestion in the head session's terminal, 2026-10-12; question: whether it is acceptable that a Monitor reader sees only about the first 420 characters of a long message (Monitor truncates at 500), with attend inbox for the rest; option text written by the agent, chosen by the operator: Monitor is the fallback now that the mod's tools are the principal path. The line shows the sender and ~id before the body, so attend inbox or a reply still works. Simplest, and nothing splits."
    covers: [monitor]
status: accepted
date: 2026-10-12
deciders:
  - aaronsb
related:
  - ADR-406
  - ADR-122
  - ADR-172
observable:
  - "run: attend send -- \"<a body of 1,500 characters>\", then attend inbox --json: one message, its body as written, with no (i/n) prefix"
  - "see: the Stop-hook drain and the attend mod show that message on one line ending in its one ~id"
  - "see: under attend run the message is one line, `message from <sender> ~<id>: <body>`; Monitor shows its first 500 characters"
---

# ADR-412: One message is one signal, and no conduit splits it

## Summary

- **Decided:** `attend send` and `attend reply` write one signal per message, whatever its length. No conduit splits a message on delivery: the Stop-hook drain, the attend mod and attend-chat show it whole, and the Monitor line of `attend run` carries it whole on one line. Every delivery line names the message by its one `~id`. Signals an older attend wrote as numbered parts keep reading as the separate messages they are. This amends ADR-406 item 10, which moved splitting into `attend send`, and ends the Monitor-line chunking ADR-122 introduced.
- **Trades away:** Under the Monitor fallback a message longer than the line limit is cut. Claude Code shows the first 500 characters of a Monitor line and marks the rest `...(truncated)`, so a reader there sees the sender, the `~id` and about 420 characters of the body, and must run `attend inbox` for the rest. Before, a message split at send reached a Monitor reader whole, as numbered lines of at most 260 characters each.
- **One-way?** No. Splitting at send or at the Monitor line can be put back in one change, and no stored signal changes shape.
- **Probes:** *Confident (whole):* A long message from a peer arrives as one message with one handle, so answering it threads to the whole message. *Not confident (monitor):* A session that listens through Monitor instead of the mod sees only the first 500 characters or so of a long message and has to open `attend inbox` for the rest. You may want Monitor to show more.
- **Inversion:** The two ends are a message split wherever any reader has a limit, and a message never split anywhere. This decision takes the second end. A delivery-time split on the Monitor route only would sit between them.

## Context

ADR-122 chunked peer messages on the Monitor line because Monitor was believed to cut a stdout line at about 400 characters. ADR-406 item 10 then moved splitting into `attend send` itself, so a body over 400 characters went out as several signals led by `(1/3)`, `(2/3)` and so on, each with its own id. The mod spike had split by hand, and the trial asked for the send verb to do it.

The mod and the Stop-hook drain have no line limit. On those routes, numbered parts only did harm. Cuts landed mid-sentence. Each part had its own `~id`, so a reply threaded to one part. The parts arrived together in one turn anyway. The operator named the parts a holdover from the Monitor approach and asked that one message be sent.

The Monitor limit was measured directly for this decision. On Claude Code 2.1.296 a Monitor line is cut at 500 characters, and a notification at 3,000. Lines printed within 200 ms batch into one notification.

## Decision

1. **Send.** `attend send` and `attend reply` write exactly one signal for one message, whatever its length. `Sent` reports its one `id`. A write that no tray takes is the `not_written` error, and nothing went out.
2. **Delivery.** No conduit splits a message. The drain and the mod show it on one line ending in ` ~id`. attend-chat shows it as it shows any message. The Monitor line of `attend run` is `message from <sender> ~<id>: <body>`. The body's whitespace collapses to single spaces, so one message is one line. The handle comes ahead of the body, so a line Monitor cuts still names the message. `attend inbox` holds the full text.
3. **One delivery.** A message is one signal file, so it is one entry in the seen-set and counts as one in `attend unread` and in the cold-start plan, whichever conduit carries it (ADR-172).
4. **Legacy parts.** Readers keep ordering signals that share an mtime by the send time in their ids. Parts that an older peer sent as numbered parts still read in order. Each part stays its own message with its own id; nothing rejoins them.
5. **A bounded size.** One argv element is capped at 128 KiB on Linux (`MAX_ARG_STRLEN`), and the CLI and the mod both pass the body as one, so a larger body never reached attend. `attend send` and `attend reply` refuse a body over 64 KiB with a `too_long` error that names the limit: half the argv ceiling leaves room for the header, and a drain injects at most about 16k tokens per message. Readers load a signal file only up to 256 KiB, above anything an attend wrote, and skip a larger one with a note on stderr, marking it seen so the note comes once.
6. **Guidance.** The messaging guidance says that a message goes out as one message, that there is no need to split it by hand, and that a reader under Monitor sees about the first 500 characters of its line.

## Consequences

### Positive

- A long message reads as written, with no cut mid-sentence and no counter.
- A reply to any delivery of a message threads to that message.
- The send path loses its splitter and its partial-send error, and the peers sensor loses its chunker.

### Negative

- Under Monitor a message past the line limit is cut. The reader has the handle and the start of the message, and runs `attend inbox` for the rest.
- During rollout a peer on an older attend still sends numbered parts. They read as before until that peer updates.

### Neutral

- The short handle moves into `agent-identity`, so the peers sensor and attend compute it from one implementation. The mod computes the same handle in TypeScript.
- The Monitor limits measured here are Claude Code's, and an update can change them. The decision does not depend on their values.

## Alternatives Considered

- **Split only on the Monitor route, at delivery, on sentence boundaries, with every part carrying the message's one `~id`.** Rejected by the operator's direction to remove multipart messages altogether. Monitor is the fallback route since ADR-406, and a split there would keep a second message shape alive for that route alone. The measured batching would allow it: lines printed together arrive as one notification of up to 3,000 characters.
- **Keep splitting at send and give the parts a shared id.** Rejected. It changes the wire format for a limit that only the fallback route has, and the mod and the drain would still show cut sentences.
- **Raise the send-time limit to Monitor's measured 500 characters.** Rejected. It still splits on routes that have no limit.
