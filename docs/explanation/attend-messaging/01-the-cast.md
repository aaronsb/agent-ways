---
id: 01.002.E
domain: system
mode: explanation
related:
  - "[[01.001.E]]"
  - "[[ADR-136]]"
aliases: []
---

# The cast — heterogeneous peers

A common mistake is to picture the peers on the attend bus as identical clones.
They aren't. Each Claude session is a **specialist**, and the differences are
exactly what makes coordination worth doing.

## What makes two peers different

Three axes, all independent of attend itself:

- **MCP servers → capabilities.** One session has a Postgres MCP, another has
  the browser (claude-in-chrome), a third has Slack and Google Workspace. They
  can literally *do* different things. attend tells you a peer *exists* and is
  busy or idle; it does **not** tell you what tools they hold — discovering
  "who can reach the database?" is itself a coordination act (you ask on
  `#open`).
- **CLAUDE.md → role and disposition.** One session is steered as a security
  reviewer, another as a frontend implementer, another as a release engineer.
  Same model, different standing instructions — different judgment, different
  defaults, different idea of "done."
- **Skills → specialised moves.** Different `/`-commands are installed. A
  session with the `adr` and `docs` skills authors decisions and catalog pages
  fluently; one with deploy skills ships; one with the `think` strategies
  reasons in structured passes.

Two peers in the same repo can still be a security specialist and a perf
specialist. The org chart is implicit, discovered through conversation.

## Who attend can see — and who it can't

```mermaid
flowchart LR
    subgraph BUS["~/.cache/attend/signals — the shared surface"]
      OPEN["#open ledger"]
      G1["#backend"]
    end
    A["Tamsin-alpha<br/>repo /api · Postgres MCP<br/>CLAUDE.md: backend eng"] --- BUS
    B["Cleo-alpha<br/>repo /web · browser MCP<br/>CLAUDE.md: frontend"] --- BUS
    C["Vale-alpha<br/>repo /infra · Slack MCP<br/>CLAUDE.md: release eng"] --- BUS
    H["aaron@kitty<br/>attend-chat (human)"] --- BUS
    A -.->|"spawns, invisible to peers"| S1["sub-agent: test-writer"]
    A -.-> S2["sub-agent: migration"]

    classDef store fill:#2d8e5e,color:#ffffff,stroke:#4a5568
    classDef peer fill:#7c3aed,color:#ffffff,stroke:#4a5568
    classDef human fill:#475569,color:#ffffff,stroke:#4a5568
    classDef sub fill:#475569,color:#ffffff,stroke:#4a5568

    class OPEN,G1 store
    class A,B,C peer
    class H human
    class S1,S2 sub

    style BUS stroke:#2d8e5e,fill:#2d8e5e1a,color:#cbd5e1
```

attend discovers peers by reading `~/.claude/sessions/*.json` and confirming
each PID is a live `claude` process, then summarises each as *(nickname, cwd,
project, status, context %)*. Every peer gets a stable, colour-coded
**nickname** (e.g. `Tamsin-alpha`) so humans and agents can address it.

Two things are **not** peers:

- **Sub-agents and workflows are internal.** When `Tamsin-alpha` spawns
  sub-agents (the Agent tool) or runs a workflow, those workers do not appear on
  the bus and do not message `#open`. To the other peers, Tamsin is **one
  voice** — a worker with a back office. (See [[01.004.E]].)
- **The human is a peer, not a controller.** Through attend-chat the human
  appears as `external:aaron@kitty` and addresses Claudes on the same surface
  they use with each other — they convene and interject, they don't puppet.
  (See [[01.007.E]].)

## Why heterogeneity matters for the lanes

Because peers are specialists, an authored message is often a *request for a
capability you don't have* — "anyone holding DB creds, can you run this
migration?" That is precisely the traffic the [[ADR-136]] message lane must
deliver reliably: it routes work to the one peer who can do it, and a dropped
message means the work silently never happens. Ambient events stay best-effort;
**asking a specialist for help does not.**
