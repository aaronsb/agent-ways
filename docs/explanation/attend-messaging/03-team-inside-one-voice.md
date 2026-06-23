---
id: 01.004.E
domain: system
mode: explanation
related:
  - "[[01.001.E]]"
  - "[[ADR-136]]"
aliases: []
---

# Scenario — a team inside one voice

**One Claude, many workers.** `Vale` is leading a big migration. Vale fans out
across sub-agents (the Agent tool) and runs a workflow that grinds for several
minutes of wall-clock. To the rest of the bus, Vale is still **one peer** — one
nickname, one tray, one voice.

## The boundary of a "participant"

```mermaid
flowchart TB
    subgraph VALE["Vale — one peer on the bus"]
      direction TB
      L["lead session"]
      L --> W1["sub-agent: rename call sites"]
      L --> W2["sub-agent: update tests"]
      L --> W3["workflow: verify per-module"]
    end
    BUS["#open ledger + Vale's tray"]
    VALE -->|"speaks as one voice"| BUS
    P["Tamsin, Cleo, …"] --> BUS
    BUS -.->|"messages accrue while Vale is heads-down"| VALE

    classDef core fill:#7c3aed,color:#ffffff,stroke:#4a5568
    classDef process fill:#2d7d9a,color:#ffffff,stroke:#4a5568
    classDef store fill:#2d8e5e,color:#ffffff,stroke:#4a5568
    classDef external fill:#475569,color:#ffffff,stroke:#4a5568
    class L core
    class W1,W2,W3 process
    class BUS store
    class P external
    style VALE stroke:#8b5cf6,fill:#7c3aed1a,color:#cbd5e1
```

The sub-agents and the workflow are **internal**. They don't register as peers,
don't appear in `attend peers`, and don't post to `#open`. This is deliberate
and matches the office intuition: a manager with a back office is *one* colleague
to everyone else — you talk to Vale, not to Vale's assistants. The internal team
is Vale's private parallelism, surfaced to peers only as Vale's synthesized
output.

## Where the two clocks collide

This scenario is the sharpest illustration of [[01.001.E]]'s two-clock point.
While the workflow runs, Vale is **deep in the turn dimension** — a single long
stretch of reasoning that doesn't yield to check messages. Meanwhile the
**wall-clock keeps running**, and peers keep talking: questions, a `#open`
heads-up, a directed ask all land in Vale's tray.

```mermaid
sequenceDiagram
    autonumber
    participant P as Peers
    participant Tray as Vale's tray
    participant V as Vale (in a 6-min workflow)
    rect rgba(217,119,6,0.12)
    P->>Tray: 2 directed + 4 on #open (over ~6 min)
    Note over V: heads-down — does not turn to read mid-workflow
    end
    rect rgba(45,125,154,0.12)
    V->>Tray: workflow done — surface
    Tray-->>V: digest: "while you were deep:<br/>2 to you (newest 40s ago) · 4 on #open over 6m"
    Note over V: ONE turn, not 6 interrupts
    end
    rect rgba(45,142,94,0.12)
    V->>P: synthesize + answer the 2 directed asks
    end
```

If each accrued message had been injected as its own turn, the workflow would
have been shredded by interrupts — or the messages dropped to protect it. The
**durable tray plus the re-entry digest** is what lets Vale stay heads-down
*and* lose nothing: the wall-clock burst coalesces into a single turn-level
"here's what you missed," and Vale pulls detail with `attend inbox` if a line
warrants it.

## The point

A "participant" is **one session = one tray**, not its internal team. Deep,
turn-bound work (a workflow, a long reasoning pass) is exactly when wall-clock
messages pile up — so the message lane's durability and digesting aren't a
nicety here, they're what makes delegation and conversation coexist.
