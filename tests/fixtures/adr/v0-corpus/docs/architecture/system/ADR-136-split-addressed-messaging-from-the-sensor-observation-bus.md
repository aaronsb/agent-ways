---
status: Accepted
date: 2026-06-20
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-120]]"
  - "[[ADR-121]]"
  - "[[ADR-123]]"
  - "[[ADR-124]]"
  - "[[ADR-129]]"
---

# ADR-136: Split addressed messaging from the sensor-observation bus

> **Implemented and live-validated 2026-06-20** on branch
> `fix/attend-message-durability`. The decisions below are reconciled to
> what was actually built — see [Validation](#validation). Multi-recipient
> addressing (Bug 1) shipped separately in PR #137.

## Context

Attend carries two kinds of traffic over one pipeline, and they have
opposite handling requirements.

### The office analogy

Picture several office workers near each other. They **talk back and
forth** — conversations with continuity, where each knows roughly where
the exchange stands. That conversation is durable *state*; it must not
get wiped.

Meanwhile the **phone rings, a fax comes in, a work package is dropped
off.** These are events. A worker mid-conversation can queue them or
ignore them, and either choice is fine — handling an interrupt, or
choosing not to, never erases the conversation they're holding. The fax
sitting in the tray and the package on the desk are **durable work
items**: they wait there until that worker processes them. Nobody empties
the tray on a timer, and no co-worker walking past gets to shred an
unread fax.

Mapped to attend:

- **Ambient observations** — git churn, a peer appearing, a process
  starting. The phone ringing. These are *noise* / interrupts. The whole
  point of the salience gate ([[ADR-121]]), the action-potential
  refractory ([[ADR-123]]), and the disclosure governor is to *suppress*
  most of them so Monitor only wakes a session for something that moved.
  Queue-or-ignore, lossy, and rate-limited is exactly right here.
- **Addressed messages** — `attend send`, `attend reply`, and the
  attend-chat `@Name` / `#group` surface. The fax in the tray, the
  package on the desk, the conversation itself. These are *intentional
  communication* a human or agent composed on purpose. The work item
  waits in **that recipient's own tray** until they process it; the
  conversation state it belongs to is never wiped by event handling.

Today both ride the same path: a `.signal` file in a **shared**
directory, scanned by `sensor-peers` on each poll, then run through the
same gate stack built to throttle observations. Two concrete defects
surfaced, and both are symptoms of messages being second-class citizens
on a bus designed for disposable observations — work items thrown in a
shared tray that anyone can shred.

### Two clocks — and the bridge between them

attend and the ways/steering system run on **different time dimensions**,
and keeping them straight is what scopes this ADR.

- **attend is wall-clock.** It is deliberately a real-time system: it
  runs *between* an agent's turns, polls on a wall-clock cadence, decays
  salience in seconds, and measures "away for 21 minutes." Real time is
  its native substrate. Everything in this ADR — both lanes — lives here.
- **Ways / agent steering is epoch/turn-driven.** Ways fire and refire on
  conversation *turns*, match per-prompt, and disclosure-gate per-turn.
  There is no wall-clock in that dimension; a session paused for hours and
  resumed is the same turn-continuum. The salience engine is *shared*
  between the two ([[ADR-123]]), but consumed on two different clocks —
  `salience.rs` already names the "deliberate asymmetry vs. ways'
  `way_fire_outcome`, which always fires on first match regardless of
  age," precisely because ways live on the turn clock and sensors on the
  wall clock.

**The notification is the bridge.** A wall-clock event crosses into the
turn dimension when Monitor injects a `<task-notification>` that becomes
a prompt. That crossing is why **digest-not-replay** (below) is not just
ergonomics but *dimensionally correct*: a wall-clock burst of N messages
must not become N turn-injections — it must coalesce into **one** turn,
because the receiving dimension is turn-discrete and context-precious.
Many-in-wall-clock → one-in-turns is the correct impedance match. The
turn dimension itself is out of scope here; it only meets attend at this
boundary.

### The current flow

```mermaid
sequenceDiagram
    autonumber
    participant H as Human / Agent
    participant C as Compose<br/>(attend-chat / attend send)
    participant FS as Shared signal dir<br/>(~/.cache/attend/signals/…)
    participant S as sensor-peers<br/>(per-session, ~30s poll)
    participant G as Gate stack
    participant M as Monitor → session

    rect rgba(124,58,237,0.12)
    H->>C: "@Cleo @Tam hi" / attend send
    Note over C: parse_addressed → ONE Addressed<br/>CLI → ONE --to / --focus
    C->>FS: write_signal (one file per dest dir)
    end
    rect rgba(217,119,6,0.12)
    loop each peer polls independently
        S->>FS: scan own-cwd + _broadcast + focus
        S->>G: 1. seen-dedup
        S->>G: 2. salience gate (seeded from file MTIME)
        S->>G: 3. retention cleanup DELETES stale file (shared dir!)
        S->>G: 4. emission_threshold 2.0
        S->>G: 5. action-potential refractory (per-session state)
        S->>G: 6. disclosure governor: window cap + cooldown (per-session)
        S->>G: 7. priority filter: magnitude ≥ 3 → stdout
        G-->>M: survivors → stdout line
    end
    end
    M->>H: <task-notification>
```

Seven gates sit between "send" and "notify." Three of them (salience,
refractory, governor) carry **per-session timing state**, so two peers
watching the same message can legitimately diverge. One of them
(retention cleanup) is **destructive on a shared resource**.

### Bug 1 — `@multi` addresses only the first agent

The bus is single-recipient end to end.
`parse_addressed` (`tools/attend-chat/src/legend.rs:173`) parses only
the *leading* sigil token and returns one `Addressed`;
`handle_enter` (`tools/attend-chat/src/app/keys.rs:78`) matches one arm
and writes to one inbox — so `@Cleo @Tam msg` routes to Cleo and
`@Tam` becomes body text. The CLI mirrors this: `attend send` accepts a
single `--to` or single `--focus`
(`tools/attend/src/cmd/send.rs`). The *write* layer already loops over a
`Vec<PathBuf>` of destinations (`send.rs:151`) — it is only ever handed
one element. Multi-recipient was never representable above the write
layer.

### Bug 2 — a `#open` message reached one peer but not another

Reported live: a human sent `#open`; the KGS peer (Clio) surfaced it,
the agent in `~/.claude` did not — only a few minutes apart. Timeline
reconstruction from the session transcript confirmed the agent's
Monitor had **down-gaps** (stopped 06:04:42Z, and again 06:35:53Z) while
the peer ran continuously. The decisive mechanism, consistent with a
few-minutes gap:

1. The message was written during the recipient's Monitor-down gap.
2. The live peer scanned it within ~30s and was notified.
3. The retention cleanup sweep — run by *any* live attend, including the
   peer (`tools/sensor-peers/src/lib.rs:394`, and the periodic sweep in
   `tools/attend/src/cmd/run/tick.rs`) — **deleted the stale file from
   the shared dir**.
4. The recipient's Monitor came back *after* the file was gone, so it
   never saw the message.

The root defect is not "the Monitor was off" — sessions stop and start
routinely. It is that **delivery is best-effort against an ephemeral
shared directory with no durable, replayable, per-recipient inbox.** A
recipient that is momentarily away loses the message permanently. The
52-minute salience-decay backlog filter ([[ADR-121]]) was an early
suspect and is *not* the cause here; the message was fresh.

## Decision

Split the bus into two lanes with different delivery contracts. The
sensor-observation lane is unchanged. Addressed messages move to a
**message lane** that is reliable rather than throttled.

**The lane boundary is _authored communication_ vs _environmental
event_ — not directed vs broadcast.** A person or agent composing words
to reach someone is conversation, and rides the message lane. The
environment generating a notice — git changed, a peer appeared, a
process started; the phone, the fax, the package — is an event, and
rides the observation lane. This matters most for `#open`: it is
authored, so it is durable, full stop. `#open` is "talking loud in the
open office" — sometimes to convene ("we should all discuss this, maybe
break into smaller groups"), sometimes just because only a couple of
sessions are around and `#open` *is* the conversation. Both are talking.
Demoting `#open` to a best-effort bulletin would shred conversation in
exactly the small-room case where it carries the real exchange.
Convening-then-splitting rides the existing focus-group mechanism
([[ADR-118]], [[ADR-129]]); the migration from `#open` to a `#group` is a
workflow, not a durability tier.

**1. The message lane bypasses the noise-control stack.** Addressed
messages (directed `@`, group `#`, and `#open` broadcast) skip the
event-lane gates that exist to suppress ambient observation *noise*; a
composed message is not noise. As built, this is **three distinct
mechanisms**, and live peer testing found them one at a time — each fix
exposed the next:

- the per-signal **salience gate** (an mtime-seeded backlog filter) is
  *removed* from the message path, so unread mail is never aged-out — the
  "peers don't hear" half of Bug 2;
- the per-sensor **action-potential refractory** is *bypassed* for the
  message lane (it surfaces whenever anything is accumulated and records
  no engagement, so it can never build a refractory that holds
  conversation);
- the shared **disclosure governor** is replaced with a *separate
  permissive* governor for the message lane (flat 3 s cooldown, generous
  window, no rate-ballooning). This is permissive, **not** a full bypass:
  normal cadence flows, a true rapid burst coalesces into one digest
  (Decision 5) and discloses promptly, and nothing is ever starved or
  dropped.

The lane keeps only **dedup** (deliver once). The neuron-decay model
(`sensor_trait` engagement/curve) stays fully intact for the *event*
lane — git, process, and a future external-chat sensor (e.g. Slack).

**2. Each recipient owns its own seen-set; rooms stay shared.** A
directed multi-`@` message fans out one durable write per recipient
(all-or-nothing resolution + dedup — Bug 1, PR #137). Shared rooms
(`#open`'s `_broadcast`, focus groups) remain *shared directories* rather
than copying a broadcast into every inbox. Durability there comes from
three things together: nothing is reaped by age (Decision 3), each
session's own **persisted seen-set** (checkpointed) dedups across
restarts, and a **cold-start backlog baseline** keeps a fresh join from
dumping history. So the realized form of "each recipient has their own
tray" is *each recipient owns its own seen-set* — lighter than a
cross-party ack protocol, and lighter than fanning every broadcast out N
times. A restarting session restores its seen-set and surfaces only the
messages that arrived during the gap.

**3. Nothing is reaped by age; lifetime is bound to project liveness.**
The destructive in-read 5-minute shred that caused Bug 2 — one peer's
scan deleting a file another peer (or a returning session) hadn't read —
is removed outright. Messages are *never* removed by wall-clock age.
Instead, lifetime mirrors Claude Code's own model: a tray dies when its
project is gone. `run_cleanup` reaps a directed tray when its project is
no longer tracked in `~/.claude/projects/`, and reaps a shared-room
signal when its *sender's* project is gone (sender cwd read from the wire
format). The age-based machinery (`--older-than`, duration parsing,
`cleanup.retention`) was removed; `--dry-run` / `--all` remain.
Conversation state (a session's seen-set and thread context, [[ADR-120]])
is never wiped as a side effect of handling an event.

**4. Addressing is multi-recipient.** `parse_addressed` becomes
"parse the leading run of `@`/`#` tokens" and returns a *set* of
targets; `handle_enter` and `attend send` build a multi-element
`dest_dirs` and fan out one durable write per recipient. This is the
direct fix for Bug 1 and falls out naturally once messages are
first-class — the write layer already loops.

**5. Re-entry is a count-led digest, not a replay.** A single poll that
surfaces more than a small cap (8) of unseen messages coalesces into one
digest line — *"12 new messages: 3 to you, 9 on #open (newest 2m ago,
over 21m) — attend inbox for detail"* — instead of flooding the turn.
One mechanism covers both a warm-rejoin gap and a live burst from a
hyperactive peer: many-in-wall-clock → one-in-turns at the notification
bridge. Detail pulls from `attend inbox`
(`tools/attend/src/cmd/inbox.rs`), now **paged** (`--limit` / `--page` /
`--before <ts>`) over the never-reaped, chronological ledger. A live
finding: a real peer rarely triggers the digest end-to-end, because its
*own* auto-mode classifier spaces rapid sends into a 1–2-per-poll trickle
(under the cap) — which is the correct "spread-out = individual, prompt"
behavior. So the digest is the backstop for genuine bursts (a workflow, a
long down-gap rejoin); that path is covered by unit tests.

**Wall-clock is first-class in both lanes; the lanes differ only in how
they _use_ it.** The event lane uses time to **decay and drop** (a stale
observation is less worth a wake-up — correct lossiness). The message
lane uses time to **stamp and digest** (a stale message is never dropped,
only summarized as "how long ago — you decide"). Same clock, opposite
policy. That is the clean line between the lanes.

The exact on-disk shape (extend the existing signal-dir convention vs. a
dedicated message store) is an implementation choice for the follow-up
PRs; the contract above is what this ADR fixes. CLI remains the whole
interface ([[ADR-124]] / attend's CLI-is-the-contract rule) — no new
caller reaches into attend-owned state.

### Target flows

**Classification — what picks the lane** (authored vs environmental, the
one decision that routes everything):

```mermaid
flowchart TD
    X[New thing happens] --> Q{Authored by a person/agent<br/>to communicate?}
    Q -->|"yes — @Name, #group, #open"| M[MESSAGE lane<br/>durable · dedup-only · wall-clock stamped]
    Q -->|"no — git, process, peer-presence"| E[EVENT lane<br/>salience + refractory + governor]
    M --> MT[recipient tray / room ledger<br/>never wiped until that recipient saw it]
    E --> ET[coalesced · aged by wall-clock · may drop]

    classDef external fill:#f6821f,color:#1a1a1a,stroke:#4a5568
    classDef decision fill:#fbbf24,color:#1a1a1a,stroke:#4a5568
    classDef core fill:#7c3aed,color:#ffffff,stroke:#4a5568
    classDef process fill:#2d7d9a,color:#ffffff,stroke:#4a5568
    classDef store fill:#2d8e5e,color:#ffffff,stroke:#4a5568

    class X external
    class Q decision
    class M core
    class E process
    class MT store
    class ET process
```

**Authored message, live recipients** (also the Bug 1 multi-recipient fix):

```mermaid
sequenceDiagram
    autonumber
    participant A as Author
    participant L as Message lane
    participant T1 as Alice's tray
    participant T2 as Bob's tray / #open ledger
    participant Rx as Live recipient
    rect rgba(124,58,237,0.12)
    A->>L: "@Alice @Bob ship it" (one compose)
    Note over L: parse the SET {Alice, Bob}<br/>stamp wall-clock ts, one dedup id
    L->>T1: durable write
    L->>T2: durable write
    end
    rect rgba(45,142,94,0.12)
    Rx->>T1: scan (live)
    T1-->>Rx: notify once
    Note over T1: read ≠ delete — marked seen in<br/>Rx's own set, survives restart
    end
```

**Re-entry after a down-gap** (the Bug 2 fix; wall-clock front and center):

```mermaid
sequenceDiagram
    autonumber
    participant Rx as Returning session
    participant T as Trays + #open ledger
    Note over Rx: was down 06:04–06:25
    rect rgba(217,119,6,0.12)
    Rx->>T: on reconnect, scan UNSEEN (own seen-set)
    T-->>Rx: digest — "while away: 2 to you (newest 2m ago) ·<br/>6 on #open over 21m"
    Note over Rx: one coalesced turn, NOT a replay flood
    end
    rect rgba(45,142,94,0.12)
    Rx->>T: attend inbox (opt-in pull)
    T-->>Rx: full chronological ledger
    Note over Rx: silence still valid — glance if worth it
    end
```

**Environmental event** (unchanged — the lane the gate stack was built for):

```mermaid
sequenceDiagram
    autonumber
    participant Env as Environment
    participant S as Sensor poll
    participant G as Salience + Refractory + Governor
    participant Rx as Session
    rect rgba(45,125,154,0.12)
    Env->>S: git dirty / process up / peer appeared
    S->>G: magnitude + wall-clock age
    Note over G: aged, coalesced, most suppressed to stderr
    end
    rect rgba(217,119,6,0.12)
    G-->>Rx: only if loud enough
    Note over Rx: lossy BY DESIGN — the phone may ring unanswered
    end
```

## Validation

**Live-validated 2026-06-20** with two real sessions: a receiver
("Thaddeus", this repo) on the rebuilt binary and a peer ("Urban-beta",
`~/temp`). The test compared wall-clock send time against the message
path and drove a 12-message burst. Confirmed:

- **Single send/reply lane** clean both directions, ~36 s round-trip
  (≈ one sensor poll each way); ACKs surfaced reliably.
- **Permissive governor** discloses at a flat ~3 s cooldown — no more
  multi-minute starvation (pre-fix, a coalesced digest sat unshown for
  minutes behind the rate-ballooning event governor).
- **Refractory bypass** keeps the peers lane flowing after heavy prior
  traffic (pre-fix it hit `ABSOLUTE REFRACTORY` and held the digest).
- **Cold-start / warm-restart** surfaced no flood — the existing backlog
  stayed baselined.
- **Emergent safety**: a peer cannot be instructed by *another* peer to
  flood `#open` — the auto-mode classifier keys on *who* asks
  (operator-authorized bursts allowed, peer-instructed bursts refused).

The test is what surfaced the three-gate stack in Decision 1 — each fix
exposed the next gate, something unit tests alone would not have caught.
It also showed the digest's end-to-end path is hard to trigger from a
real peer (its own classifier spaces sends), so that path leans on unit
tests while the live lane behavior is exercised directly.

## Consequences

### Positive

- Messages a human or agent composed on purpose are delivered reliably
  and exactly once, including across a recipient's brief restart.
- Both reported bugs are fixed by construction: multi-recipient
  addressing (Bug 1) and no-silent-drop delivery (Bug 2).
- The salience / refractory / governor machinery gets a clearer mandate
  — it governs *observations*, the job it was designed for — instead of
  being asked to also not-lose intentional messages, which it was never
  built to guarantee.

### Negative

- Two lanes is more surface than one pipeline. The win is that each lane
  has a single, honest contract; the cost is that "it's all just
  signals" stops being true.
- Never reaping by age means the on-disk ledger grows until a project is
  removed. This is bounded by **project-liveness** reaping (a tray's, or a
  sender's, signals go when its `~/.claude/projects/` entry does) rather
  than a wall-clock timer. The constraint that actually matters is
  *context* — how much gets rehydrated, capped by the count-led digest and
  inbox paging — not disk.

### Neutral

- The three synchronized messaging docs must move in lockstep with any
  contract change: `skills/attend/SKILL.md`,
  `tools/sensor-disclosure/src/disclosures/messaging.md`, and
  `hooks/ways/softwaredev/environment/attend/attend.md`.
- `#open` broadcast keeps its semantics ([[ADR-124]] base channel); only
  its delivery guarantee changes from best-effort to durable.
- Threaded replies ([[ADR-120]]) ride the message lane unchanged.
- **Known limitation (follow-up).** The lane is selected per *sensor*
  (`peers`), but that sensor also emits peer-presence *events*, which
  therefore currently ride the message lane — skipping the refractory and
  neuron-decay the event lane gives git/process. No message loss; only
  presence-noise control is relaxed. The clean fix is to split message
  scanning into its own sensor so the lane is chosen per *observation* —
  also the seam a future external-chat (event) sensor wants.

## Alternatives Considered

- **Tune the gates so messages always pass.** Raise message magnitude
  above every threshold, exempt them from cleanup. Rejected: it keeps
  messages coupled to per-session timing state (governor window,
  refractory) that can still diverge between peers, and it is a pile of
  special-cases rather than a contract. The defect is structural, not a
  threshold value.
- **Make the salience gate anchor to first-observation instead of file
  mtime.** Fixes the backlog-decay edge but not this bug (the message
  was fresh) and does nothing for the destructive-cleanup race or
  multi-recipient addressing. A partial patch on one of seven gates.
- **Full cross-party acknowledgement protocol with shared-state GC.** A
  message lingers in a shared store until every recipient explicitly
  acks, then a collector reaps it. Rejected as too heavy: the office
  analogy says the durable thing is *each worker's own tray*, emptied
  when that worker processes their own fax — not a handshake the senders
  and receivers all have to participate in. Per-recipient ownership gets
  the same no-silent-drop guarantee with far less coordination.
- **Leave delivery best-effort; document that messages can drop.**
  Rejected: the human's mental model is "I sent it, the agent will see
  it." Silent loss of intentional communication is the worst failure
  mode for a coordination surface, and "silence is a valid reply"
  ([[ADR-121]]) only holds if the recipient actually *received* the
  message and chose not to answer.
