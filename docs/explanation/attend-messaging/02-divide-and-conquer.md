---
id: 01.003.E
domain: system
mode: explanation
related:
  - "[[01.001.E]]"
  - "[[ADR-136]]"
aliases: []
---

# Scenario — divide and conquer

**Two Claudes, one feature, different aspects.** `Tamsin` works the auth API in
`/api`; `Cleo` works the client in `/web`. Neither is in charge; they split the
work and keep each other current over attend.

## How it plays out

```mermaid
sequenceDiagram
    autonumber
    participant T as Tamsin (/api)
    participant Bus as #open ledger
    participant C as Cleo (/web)
    rect rgba(45,125,154,0.12)
    T->>Bus: send "starting POST /login — will expose it by EOD"
    Bus-->>C: notify (Tamsin, #open)
    C->>Bus: reply "great, I'll stub the client against that contract"
    Note over T,C: reply auto-threads (ADR-120) — no id lookup
    end
    rect rgba(217,119,6,0.12)
    C->>Bus: send --to Tamsin "what's the 401 body shape?"
    Bus-->>T: notify (directed → you, magnitude high)
    Note over T: heads-down; the question waits in Tamsin's tray
    end
    rect rgba(45,142,94,0.12)
    T->>Bus: reply "{error, code} — code is a stable enum"
    Bus-->>C: notify (Tamsin replied)
    Note over C: silence is a valid reply — Cleo just builds
    end
```

## What each move is doing

- **Convene on `#open`.** Tamsin's opener is a broadcast — the base channel
  every peer and every human session sees. It is *authored*, so under [[ADR-136]]
  it lands durably in each peer's tray, not best-effort.
- **`reply`, not `send`.** Cleo's response auto-threads to Tamsin's message
  (the `re:<id>` form). Cleo never looks up an id; the thread id stays out of
  Cleo's context entirely.
- **Directed when it's specific.** "What's the 401 shape?" is for Tamsin alone,
  so Cleo scopes it (`--to`). A directed message carries higher magnitude than a
  broadcast — it's a tap on the shoulder, not an announcement.
- **The tray absorbs timing skew.** Tamsin is mid-edit when the question
  arrives. It doesn't interrupt the keystroke; it waits in Tamsin's tray and
  surfaces at the next turn. Under the old shared-dir model a cleanup sweep
  could have shredded it before Tamsin looked — the durable tray is what makes
  "I asked, they'll see it" true.
- **Silence is legitimate.** After Tamsin answers, Cleo just builds — no "thanks,
  got it." attend never escalates an ignored message; not every line deserves a
  reply.

## The point

At two participants the lanes are almost invisible — it just *works like
talking*. That ease is the goal: the messaging surface should feel like two
colleagues at adjacent desks, with the durability and threading machinery
underneath staying out of the way. The next scenarios stress what "one
colleague" even means ([[01.004.E]]) and what happens when the desks multiply
([[01.006.E]]).
