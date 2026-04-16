---
status: Accepted
date: 2026-04-11
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-118
  - ADR-119
---

# ADR-120: Interactive Chat TUI — Human in the Signal Loop

## Context

attend provides inter-session signaling for Claude Code agents, but the human has no first-person view of the signal topology. The human's current tools are:

- **`attend send --broadcast`** from a bare terminal — fire-and-forget, no conversational flow
- **`attend inbox`** — a snapshot, not a stream
- **Switching between Claude terminal sessions** — one agent at a time, no overview

This creates two problems:

**1. Routing confusion.** Agents misroute messages because the routing semantics (broadcast, focus group, project scope) are invisible abstractions. In a typical failure, an agent wanting to "report to Aaron" sends `--to /home/aaron/.claude` instead of `--broadcast`, because it reasons about paths rather than intent. The routing model has no physical form that teaches correct usage.

**2. No orchestration surface.** When running 4+ Claude sessions across related projects, the human has no way to see the whole board. Steering requires switching to each agent's terminal individually. There is no equivalent of a team lead walking through an open-plan office, hearing conversations, and dropping context where needed.

### The real-world workflow

A concrete use case that motivates this:

- A Konsole window with 4 Claude sessions working on related projects (the team)
- A separate terminal running `attend chat` (the coordination surface)
- A GitHub sensor (new attend sensor) detects issue state changes in each agent's repo — an issue moves from backlog to "in progress" and attend notifies the relevant agent
- The human watches progress in the chat TUI, steers with targeted messages (`@infra hold off, @api needs to land first`)
- When deep focus is needed, the human switches to that agent's terminal session directly
- The chat TUI is peripheral vision; the Claude sessions are foveal vision

### Prior art

Terminal chat clients have solved multi-channel navigation and threading:

- **WeeChat** — buffer model (every channel = a numbered buffer with activity indicators), split views, relay protocol decoupling UI from engine
- **Matterhorn** — three-tier channel switching (all / unread / by-name), dedicated thread pane
- **iamb** — Vim-modal navigation, tabs for spaces, splits for rooms, built in Rust with ratatui

Multi-agent orchestration tools have solved session management:

- **Agent Deck** — status filters (`!@#$` for running/waiting/idle/error), tmux status bar integration, cost tracking
- **Agent of Empires** — tmux-native persistence, git worktree isolation per agent

What nothing has built: a tool where the human sits *inside* the same signal protocol the agents use, sending and receiving through the same routing infrastructure, with contextual dispatch (`@group` addressing, `#issue` references) from within the stream.

## Decision

Add `attend chat` — an interactive TUI that places the human inside the signal loop as a first-class participant.

### Core design

The TUI is an **orchestration surface, not a work surface**. Deep work happens in each agent's terminal. The chat TUI provides:

- **Visibility** — see all signals across all rooms in real-time
- **Steering** — send targeted messages to groups or broadcast to all
- **Awareness** — ambient metadata (agent status, branch, context %) alongside messages

### Framework

**iocraft** (Rust, React-like TUI framework). Chosen for:

- Declarative layout via `element!` macro with flexbox semantics (taffy)
- React hooks model (`use_state`, `use_future`, `use_terminal_events`)
- Component composition via `#[component]` functions
- First-class async for watching signal directories
- Mouse event support for click-to-reply

iocraft provides the layout/state scaffolding. Custom components (scrollable message list, focus group sidebar with activity indicators) are built on top.

### Layout

```
┌─────────────┬──────────────────────────────────────┐
│  Focus      │  Messages (chronological stream)      │
│             │                                        │
│ ● broadcast │  claude/api  @deploy                   │
│   @deploy   │  Landed the auth refactor, ready for   │
│   @infra    │  review.                               │
│   project/… │                                        │
│             │  claude/infra  @deploy                  │
│─────────────│  Pulling in the new auth types now.     │
│  Agents     │                                        │
│             │  aaron  broadcast                       │
│  api ↑2 73% │  @infra hold off until api merges.     │
│  infra · 91%│                                        │
│  docs · 45% │                                        │
│  slack ↑1 88│                                        │
│             ├──────────────────────────────────────┤
│             │ > @deploy looks good, ship it_        │
└─────────────┴──────────────────────────────────────┘
```

- **Left sidebar, top**: Focus group list with activity indicators (● = unread). Click or tab to filter message view.
- **Left sidebar, bottom**: Agent status — name, upstream commits, context %. Pulled from peer sensor.
- **Main area**: Chronological message stream, scoped to selected group or "all".
- **Input bar**: Text input with `@group` and `#issue` inline addressing.

### Inline addressing

- **`@name`** at the start of a message routes to that focus group: `@infra look at #172` → writes signal to `@infra` directory
- **`#NNN`** is a repo-local issue reference — the TUI doesn't resolve it. The receiving agent interprets `#172` against its own repo via `gh issue view 172`
- **No prefix** → broadcast (human default, inverse of agent default which is project-scoped)

The human's default is broadcast because the human is the coordination layer — most messages should be visible to all. Agents' default is project-scoped because most agent work is local.

### Signal protocol addition: threading

Current signal format:
```
from|project|cwd|message
```

Extended format:
```
from|project|cwd|re:signal-id|message
```

The `re:` field is optional (empty string for new threads). When an agent or human replies to a specific signal, the reply carries the original signal's ID. The TUI uses this to draw thread lines; agents can use it to maintain conversational context. This is a lightweight addition — no thread trees, no nested replies, just one level of "in response to."

### GitHub sensor (new attend sensor)

A new sensor alongside git, peers, and processes:

- **Polls**: issue state changes, PR status, review requests via `gh` CLI
- **Scope**: each agent's own repo only (not cross-repo)
- **Surfaces**: issue assignments, status transitions (backlog → in progress), PR merge/close, review comments
- **Signal format**: standard attend signals, routed to the agent's project scope

This enables the workflow: human moves an issue to "in progress" on GitHub → attend notifies the agent → agent picks up the work. The TUI shows the notification alongside other signals.

### What the TUI does NOT do

- **Replace Claude sessions** — deep work happens in the agent's terminal
- **Fetch GitHub data** — `#issue` references are resolved by agents, not the TUI
- **Manage agent lifecycle** — starting/stopping agents is outside scope
- **Provide an editor** — no code editing, no file browsing

## Consequences

### Positive

- Routing semantics become visible — the human sees where signals land, which rooms are active, how messages flow
- Cross-agent coordination without terminal switching
- The human experiences the same signal protocol agents use, creating empathy for routing design (dogfooding)
- GitHub sensor closes the loop between project management and agent work
- Threading enables conversational context without full chat protocol complexity

### Negative

- New dependency: iocraft (0.8.0, still pre-1.0)
- The TUI is another process to run alongside agents — adds operational surface
- Threading changes the signal format — existing signal readers need to handle the new field
- GitHub sensor requires `gh` CLI auth in each agent's environment

### Neutral

- ADR-118 (focus groups) — the TUI's sidebar directly depends on the focus group model and scoped scenes
- ADR-119 (action potential) interactions — agents in the TUI message stream are subject to the same engagement dynamics
- The attend binary grows a significant new subcommand; may warrant a separate binary or feature flag if it pulls in heavy TUI dependencies

## Alternatives Considered

### Web dashboard
A browser-based UI showing agent status and signals. Rejected because it breaks the terminal-native workflow — the human would context-switch between browser and terminals. The TUI stays in the same environment as the agents.

### tmux wrapper
Use tmux panes to tile agent sessions with a monitoring pane. This is what Agent of Empires does. Rejected because tmux panes show raw session output, not the signal layer. You'd see everything an agent does, not the coordination-relevant signals. Too much information, no routing visibility.

### Extend existing tools (Agent Deck, etc.)
These are session managers — they manage agent lifecycle and show status. They don't participate in a signal protocol. attend's signal infrastructure is the differentiator; building on a session manager would mean reimplementing the signal layer or bridging two systems.

### ratatui (imperative layout)
The ecosystem standard for Rust TUIs. Rejected as primary framework because layout is imperative (compute rects, render into them) rather than declarative. For a chat UI with dynamic group lists, resizable panes, and nested components, declarative flexbox is significantly more productive. ratatui remains available as a fallback for custom widget rendering inside iocraft components if needed.
