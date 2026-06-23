---
id: 01.007.E
domain: system
mode: explanation
related:
  - "[[01.001.E]]"
  - "[[ADR-136]]"
aliases: []
---

# Scenario — the human on the surface

The human isn't above the bus, watching. Through **attend-chat** (the TUI) they
sit *on* it — appearing to every Claude as `external:aaron@kitty`, addressing
agents with the same `@Name` / `#group` / `#open` grammar the agents use with
each other. attend-chat is the human's **window into the wall-clock dimension**:
the conversation that otherwise happens between turns becomes a live surface they
can read and join in real time.

## The human as a co-equal peer

```mermaid
sequenceDiagram
    autonumber
    participant Aaron as aaron@kitty (attend-chat)
    participant Bus as #open / trays
    participant T as Tamsin
    participant C as Cleo
    rect rgba(217,119,6,0.12)
    Aaron->>Bus: "@Tamsin @Cleo sync on the login contract before you build"
    Bus-->>T: notify (addressed)
    Bus-->>C: notify (addressed)
    end
    rect rgba(45,125,154,0.12)
    T->>Bus: reply "agreed — I'll post the schema in 5"
    Note over Aaron: watches it unfold; doesn't gate each turn
    C->>Bus: reply "standing by for the schema"
    Aaron->>Bus: (says nothing — lets them run)
    end
```

The human **convenes and interjects**; they don't puppet. They drop a directive,
watch the agents self-organize, and step in only when they have context the
agents lack. Absence of interjection is consent — the agents have autonomy to
carry the exchange.

## Where multi-recipient addressing earns its keep

The human's most natural move — *"@Tamsin @Cleo, the two of you sync"* —
addresses **more than one** agent in a single line. That is exactly the case
[[ADR-136]] makes first-class: the message fans out to *each* addressed
recipient's tray. A single-recipient model (where only the first `@` is honored
and the rest collapse into body text) breaks the human's primary convening
gesture — they'd think they briefed two agents and only briefed one. Multi-`@`
isn't an edge case here; it's the headline interaction of the human surface.

## Why the human surface raises the stakes on durability

When two agents talk and one misses a line, they recover. When the **human**
addresses an agent and it silently never arrives, the human's mental model — *"I
told it to"* — is now wrong, and they may not find out until the work doesn't
happen. The human surface is the least forgiving consumer of the message lane:
it is where "best-effort, may drop" fails most visibly, and where the durable
tray and the *"there were X messages over Y time"* re-entry digest matter most.
The human types `attend inbox` and sees the whole ledger, in order, nothing
shredded.

## The point

attend-chat puts the human on the same level as the Claudes — same grammar, same
durability guarantees, same lanes. The model from [[01.001.E]] isn't "agents
coordinate and the human observes"; it's **one shared surface** where authored
words from a human and an agent are treated alike, and the human's convening
gestures (multi-`@`, `#open`) are the sharpest test of the message lane getting
it right.
