# Attend and Monitor

This directory documents the active awareness layer. Attend gives a session the awareness an employee would otherwise have ambiently — what is changing, who else is working, what deserves attention. Mechanically, it rides on top of Claude Code's `Monitor` tool to surface environmental changes as async notifications into a running session, and humans and other Claude agents can both participate in the same signal stream.

## The thesis

Monitor is Claude Code's general-purpose async delivery mechanism: launch a command, stream its stdout as notifications. Anthropic's assumption in designing it seems to be that Claude will wire up whatever ad-hoc command fits the moment — a `tail -f`, a `cargo watch`, a bespoke shell pipeline — and let Monitor relay whatever comes out. That's a powerful primitive but it puts the burden on every Claude session to reinvent the observation logic from scratch.

**Attend is Monitor with intention.** It's a long-lived logic module that Claude doesn't have to tune case by case. It knows what kinds of changes matter, it governs the rate of environmental events through a formal engagement model (ADR-123, borrowing the activation-decay shape from ACT-R), it routes messages between peer agents through channels (ADR-118, ADR-173) on a message lane that never drops them, and it cleans up after itself by reaping the messages of projects that are gone (both ADR-136). Its event-lane governor is alarm management (ISA-18.2) applied to agent notifications — rate-limiting what reaches the conversation so the channel stays trustworthy instead of noisy. A Claude session drops into attend and gets a stable, opinionated awareness channel for free.

Said another way: **Monitor is a delivery mechanism; attend is the editorial layer that decides what's worth delivering.** That editorial policy is calm technology (Weiser & Brown) applied to a coding session: most changes stay in the periphery, and only what deserves attention moves to the center. The combination turns "sporadic unpredictable state" into a structured stream of observations the agent can act on.

## The pair

**Attend** is a sensor loop. It watches the local environment (git state, peer sessions, context pressure, process activity, peer messages, and any external sensors a user wires in) and emits observations when something crosses a threshold. It runs as a single long-lived process inside the Claude Code session.

**Monitor** is the Claude Code tool feature that launches a command and delivers each line of its stdout as an asynchronous notification into the conversation. It's general-purpose: build watchers, test runners, deploy pipelines, custom event logs all fit.

The common thread across every Monitor use case is **sporadic unpredictable state** — changes the agent didn't cause, arriving at unknown times, that still need to land in the conversation in time to affect the next decision. Attend is one class of sporadic state — the class that comes from *complex interactions across peer agents plus ambient environmental awareness*. A build watcher is another class. They share the mechanism; they differ in what they observe.

## Two consumers, one channel

Attend is not exclusively for AI agents. A human can launch it too, and both consumers share the same signal bus. The peer layer underneath is workspace awareness (Dourish & Bellotti's CSCW term) applied to coding agents — knowing who else is working, where, and what they said — served to humans and agents through one protocol.

**Agent mode — `attend run`:** An AI agent session invokes attend through Monitor. The sensor loop emits notifications into the conversation, and a Stop hook delivers pending messages at the end of each turn. The agent responds to what it sees, sends peer messages back through `attend send`, and participates in channels through `attend join`. See [`loop.md`](loop.md) for the sensor loop and [`delivery.md`](delivery.md) for the two delivery conduits.

**Human mode — `attend chat`:** A human opens attend-chat, a terminal chat screen on the same message bus. Every channel streams into it, and the human addresses Claude agents (`@Elio ship it`, `@Elio @Lachlan sync first`, `#deploy hold`) through the same signal files the agents use, and steers a multi-agent session from one surface. See [`tui.md`](tui.md).

The phrase that captures it: **humans wear the same clothes as an AI agent** as far as attend is concerned. Same signal protocol, same routing, same channels. The human just happens to have a keyboard and eyeballs instead of a context window.

This dual-consumer property is deliberate. It means the signal protocol is dogfooded — the human feels the routing semantics directly, and any friction in how messages land is friction the agents also experience. It also means a solo developer watching one agent gets as much value as a coordinator orchestrating four.

## Scale context

The attend development effort so far has put significant weight on the multi-agent peer-messaging story: channels, the message lane, turn-boundary delivery, cross-session signal routing. That's a real use case but it's not the common one. **Most uses of attend will be single-user: one human, one Claude agent, external state sources.**

The common-case shape looks like this: you're writing code, Claude is helping, and you want the session to notice things that happen outside the conversation — a `make test` finished, an issue got assigned to you on GitHub, a long-running deploy finished. Attend's sensors feed those into the conversation without you having to interrupt and ask "what's the status?" The peer-messaging layer is still there if you ever run four agents at once, but you don't have to opt into it to get value.

This is why **external sensors are a first-class design surface** — they're the bridge between attend's formal engagement model and whatever the user actually cares about watching. See the next section.

## Sensor authorship is a design surface

Attend is extensible through two sensor implementations, both first-class:

**1. Compiled crate sensors.** A Rust crate implementing the `Sensor` trait from `sensor-trait`. Gets linked into the attend binary at build time, runs at full native speed, shares process memory with the loop. Used for the built-in sensors: `context`, `git` and `disclosure` (modules of attend itself, always compiled), and the `sensor-peers`, `sensor-processes` and `sensor-keepwarm` crates. The right choice when performance matters or when the sensor needs fine-grained control over its own state.

**2. External script sensors.** A shell script (or any executable) declared in the attend config as a `sensor-name:` block with a `script`. Attend runs it as a subprocess on the configured interval, parses its stdout as events, and feeds those into the same engagement/threshold/disclosure machinery. No Rust required. No recompile. The right choice for integrations with CLI tools (`gh`, `kubectl`, custom ops scripts) or for per-project sensors that don't belong in the main codebase.

**Both implementations share one constraint: the sensor author has to understand the loop's intention.** Events are not log lines — they're magnitude-weighted observations that feed an accumulator, decay over time, and fire disclosure only when they cross a refractory-aware threshold. A sensor that emits "something happened" on every tick will either flood the governor or get suppressed into silence by action potential. A well-designed sensor encodes *how much each kind of change matters* in the event magnitude, and lets the loop handle the rest.

See [`authoring-sensors.md`](authoring-sensors.md) for the full author's guide, including a walkthrough of the canonical external sensor example: a `gh`-CLI bash wrapper that watches a GitHub Project board associated with the repo attend was invoked in, surfacing card movements as signals with magnitude tuned to whether the moved issue is assigned to the current git user.

## What lives in this directory

| File | Purpose |
|---|---|
| [`README.md`](README.md) | Orientation — you are here |
| [`loop.md`](loop.md) | The sensor loop — state diagrams, timing, the two lanes |
| [`first-sensor.md`](first-sensor.md) | Walkthrough — build your first external sensor from zero |
| [`authoring-sensors.md`](authoring-sensors.md) | Writing crate and external sensors (reference) |
| [`sensors.md`](sensors.md) | What each built-in sensor observes and emits |
| [`engagement.md`](engagement.md) | The action-potential model on the event lane |
| [`signals.md`](signals.md) | Signal file format, storage layout, lifecycle |
| [`delivery.md`](delivery.md) | The Monitor line and the Stop-hook drain, enrollment, opt-out |
| [`channels.md`](channels.md) | Channel membership and lifecycle |
| [`tui.md`](tui.md) | `attend chat` — the human on the bus |
| [`keepwarm.md`](keepwarm.md) | Holding the prompt cache warm across idle time |
| [`configuration.md`](configuration.md) | Config schema, overlay semantics, permissions |
| [`../cli/attend.md`](../cli/attend.md) | The `attend` CLI reference, generated from the binary |

## Reading order

1. **[`loop.md`](loop.md)** — if you only read one file, read this. The sensor loop is the substrate everything else rides on.
2. **[`delivery.md`](delivery.md)** — how a peer message reaches a session, and how to opt out.
3. **[`tui.md`](tui.md)** — if you're a human using attend directly, or coordinating multiple agents.
4. **[`first-sensor.md`](first-sensor.md)** — if you want to build a sensor and you've never done it before.
5. **[`authoring-sensors.md`](authoring-sensors.md)** — reference for the subprocess contract and design surface once the tutorial isn't enough.
6. **[`sensors.md`](sensors.md)**, **[`engagement.md`](engagement.md)**, **[`signals.md`](signals.md)**, **[`channels.md`](channels.md)**, **[`keepwarm.md`](keepwarm.md)**, **[`configuration.md`](configuration.md)** — reference, read as needed.

The scenarios in [`../explanation/attend-messaging/`](../explanation/attend-messaging/00-overview.md) show the message lane at work with one, two and many Claudes and a human.

## Related docs

- [`../vocabulary.md`](../vocabulary.md) — terminology anchors mapping the project's coined terms to their established concepts (ADR-301)
- **ADR-113** (`docs/architecture/ways/`) — the original decision to build attend as an active awareness module
- **ADR-114** — attend as an insistent trigger type for ways
- **ADR-115** — declarative config with project-scope overlay
- **ADR-116** — permission requirements
- **ADR-117** — sensor crate extraction
- **ADR-118** — channels (introduced as focus groups), dynamic agent grouping
- **ADR-119** — action potential engagement model (superseded by ADR-123) <!-- adr-cite-ignore -->
- **ADR-120** — interactive chat TUI, human in the signal loop
- **ADR-122** — the disclosure sensor
- **ADR-123** — firing dynamics unification; the engagement engine in force
- **ADR-136** — the message lane, split from the event lane
- **ADR-172** — turn-boundary delivery through the Stop-hook drain
- **ADR-173** — the chat idiom: channels, `join`/`leave`, tabs and slash commands
- **ADR-182** — keepwarm
- `docs/hooks-and-ways/` — sibling docs for the synchronous hook mechanism
- `docs/architecture/practice/ADR-600-cognitive-loop-and-the-awareness-layer.md` — earlier design exploration that informed ADR-113

## Where the code lives

- `tools/attend/` — orchestrator and CLI; [`../cli/attend.md`](../cli/attend.md) lists every subcommand
- `tools/attend-chat/` — the chat screen, on `agent-tui`
- `tools/attend-groups/`, `attend-presence/`, `attend-state/`, `attend-config/` — channels, heartbeats and enrollment, the per-session seen-set, the config schema
- `tools/sensor-trait/` — base `Sensor` trait, `SensorSlot`, engagement state
- `tools/attend/src/sensors/` (`context`, `git`, `disclosure`), `tools/sensor-peers/`, `sensor-processes/`, `sensor-keepwarm/` — the built-in sensors
- `hooks/ways/attend-drain-stop.sh` — the Stop hook that drains messages at the turn boundary
- `tools/agent-fmt/` — shared terminal formatting (banners, tables, commands)
- `skills/attend/SKILL.md` — the invocation skill the agent reads when the user asks for awareness
- `hooks/ways/softwaredev/environment/attend/` — the way that surfaces live attend state in the steering layer
