# Attend and Monitor

This directory documents the active awareness layer — how `attend` rides on top of Claude Code's `Monitor` tool to surface environmental changes as async notifications into a running session, and how humans and other Claude agents can both participate in the same signal stream.

## The thesis

Monitor is Claude Code's general-purpose async delivery mechanism: launch a command, stream its stdout as notifications. Anthropic's assumption in designing it seems to be that Claude will wire up whatever ad-hoc command fits the moment — a `tail -f`, a `cargo watch`, a bespoke shell pipeline — and let Monitor relay whatever comes out. That's a powerful primitive but it puts the burden on every Claude session to reinvent the observation logic from scratch.

**Attend is Monitor with intention.** It's a long-lived logic module that Claude doesn't have to tune case by case. It knows what kinds of changes matter, it governs their rate and salience through a formal engagement model (ADR-119), it routes messages between peer agents through focus groups (ADR-118), and it cleans up after itself (ADR-121 for presentation decay, plus the 30-day disk sweep). A Claude session drops into attend and gets a stable, opinionated awareness channel for free.

Said another way: **Monitor is a delivery mechanism; attend is the editorial layer that decides what's worth delivering.** The combination turns "sporadic unpredictable state" into a structured stream of observations the agent can act on.

## The pair

**Attend** is a sensor loop. It watches the local environment (git state, peer sessions, context pressure, process activity, peer messages, and any external sensors a user wires in) and emits observations when something crosses a threshold. It runs as a single long-lived process inside the Claude Code session.

**Monitor** is the Claude Code tool feature that launches a command and delivers each line of its stdout as an asynchronous notification into the conversation. It's general-purpose: build watchers, test runners, deploy pipelines, custom event logs all fit.

The common thread across every Monitor use case is **sporadic unpredictable state** — changes the agent didn't cause, arriving at unknown times, that still need to land in the conversation in time to affect the next decision. Attend is one class of sporadic state — the class that comes from *complex interactions across peer agents plus ambient environmental awareness*. A build watcher is another class. They share the mechanism; they differ in what they observe.

## Two consumers, one channel

Attend is not exclusively for AI agents. A human can launch it too, and both consumers share the same signal bus.

**Agent mode — `attend run`:** An AI agent session invokes attend through Monitor. The sensor loop emits notifications into the conversation. The agent responds to what it sees, sends peer messages back through `attend send`, and participates in focus groups through `attend focus`. See [`loop.md`](loop.md) for the sensor loop substrate.

**Human mode — `attend chat`:** A human launches attend in an interactive TUI mode. The same signals the agent sees stream into a scrollable message view. The TUI is also a first-class conversation interface — the human addresses enrolled Claude agents (`@infra ship it`, `@api please rebase first`) through the same peer-messaging infrastructure, sees replies in real time, and steers a whole multi-agent session from one surface. See [`tui.md`](tui.md).

The phrase that captures it: **humans wear the same clothes as an AI agent** as far as attend is concerned. Same signal protocol, same routing, same engagement model. The human just happens to have a keyboard and eyeballs instead of a context window.

This dual-consumer property is deliberate. It means the signal protocol is dogfooded — the human feels the routing semantics directly, and any friction in how messages land is friction the agents also experience. It also means a solo developer watching one agent gets as much value as a coordinator orchestrating four.

## Scale context

The attend development effort so far has put significant weight on the multi-agent peer-messaging story: focus groups, action-potential engagement, per-peer magnitude boosts, cross-session signal routing. That's a real use case but it's not the common one. **Most uses of attend will be single-user: one human, one Claude agent, external state sources.**

The common-case shape looks like this: you're writing code, Claude is helping, and you want the session to notice things that happen outside the conversation — a `make test` finished, an issue got assigned to you on GitHub, a long-running deploy finished. Attend's sensors feed those into the conversation without you having to interrupt and ask "what's the status?" The peer-messaging layer is still there if you ever run four agents at once, but you don't have to opt into it to get value.

This is why **external sensors are a first-class design surface** — they're the bridge between attend's formal engagement model and whatever the user actually cares about watching. See the next section.

## Sensor authorship is a design surface

Attend is extensible through two sensor implementations, both first-class:

**1. Compiled crate sensors.** A Rust crate implementing the `Sensor` trait from `sensor-trait`. Gets linked into the attend binary at build time, runs at full native speed, shares process memory with the loop. Used for the built-in sensors: `sensor-context`, `sensor-git`, `sensor-peers`, `sensor-processes`. The right choice when performance matters or when the sensor needs fine-grained control over its own state.

**2. External script sensors.** A shell script (or any executable) declared in the attend config under a `+sensor-name:` block. Attend runs it as a subprocess on the configured interval, parses its stdout as events, and feeds those into the same engagement/threshold/disclosure machinery. No Rust required. No recompile. The right choice for integrations with CLI tools (`gh`, `kubectl`, custom ops scripts) or for per-project sensors that don't belong in the main codebase.

**Both implementations share one constraint: the sensor author has to understand the loop's intention.** Events are not log lines — they're magnitude-weighted observations that feed an accumulator, decay over time, and fire disclosure only when they cross a refractory-aware threshold. A sensor that emits "something happened" on every tick will either flood the governor or get suppressed into silence by action potential. A well-designed sensor encodes *how much each kind of change matters* in the event magnitude, and lets the loop handle the rest.

See [`authoring-sensors.md`](authoring-sensors.md) for the full author's guide, including a walkthrough of the canonical external sensor example: a `gh`-CLI bash wrapper that watches a GitHub Project board associated with the repo attend was invoked in, surfacing card movements as signals with magnitude tuned to whether the moved issue is assigned to the current git user.

## What lives in this directory

| File | Purpose |
|---|---|
| [`README.md`](README.md) | Orientation — you are here |
| [`loop.md`](loop.md) | The sensor loop — state diagrams, timing, signal flow |
| [`first-sensor.md`](first-sensor.md) | Walkthrough — build your first external sensor from zero |
| [`authoring-sensors.md`](authoring-sensors.md) | Writing crate and external sensors (reference) |
| [`tui.md`](tui.md) | `attend chat` — human mode and conversation interface (planned) |
| [`sensors.md`](sensors.md) | What each built-in sensor observes and how it emits (planned) |
| [`signals.md`](signals.md) | Signal file format, storage layout, lifecycle (planned) |
| [`engagement.md`](engagement.md) | Action potential model in prose and diagrams (planned) |
| [`salience.md`](salience.md) | Turn-based presentation decay — the ADR-121 mechanism (planned) |
| [`focus-groups.md`](focus-groups.md) | Dynamic group membership and signal routing (planned) |
| [`configuration.md`](configuration.md) | Config schema, overlay semantics, tuning workflow (planned) |

Files marked **planned** are part of the ongoing documentation pass.

## Reading order

1. **[`loop.md`](loop.md)** — if you only read one file, read this. The sensor loop is the substrate everything else rides on.
2. **[`first-sensor.md`](first-sensor.md)** — if you want to build a sensor and you've never done it before. Walkthrough, top to bottom, from zero to a shipped sensor.
3. **[`authoring-sensors.md`](authoring-sensors.md)** — reference for the subprocess contract, config schema, and design surface once the tutorial isn't enough.
4. **[`tui.md`](tui.md)** — if you're a human using attend directly, or coordinating multiple agents.
5. **`sensors.md`** — reference for the built-in sensors' behavior.
6. **`engagement.md`** — the action potential model governs when sensors fire.
7. **`signals.md`** — the wire format and storage layout for peer messages and notifications.
8. **`focus-groups.md`** — how groups scope signal routing between agents.
9. **`salience.md`** — presentation-layer aging.
10. **`configuration.md`** — reference manual for the YAML surface.

## Related docs

- **ADR-113** (`docs/architecture/system/`) — the original decision to build attend as an active awareness module
- **ADR-114** — attend as an insistent trigger type for ways
- **ADR-115** — declarative config with project-scope overlay
- **ADR-116** — permission requirements
- **ADR-117** — sensor crate extraction
- **ADR-118** — focus groups, dynamic agent grouping
- **ADR-119** — action potential engagement model
- **ADR-120** — interactive chat TUI, human in the signal loop
- **ADR-121** — salience decay for signal presentation (draft)
- `docs/hooks-and-ways/` — sibling docs for the synchronous hook mechanism
- `docs/design-notes/cognitive-loop-and-awareness-layer.md` — earlier design exploration that informed ADR-113

## Where the code lives

- `tools/attend/` — orchestrator, config, groups, state, CLI (`run`, `chat`, `send`, `inbox`, `focus`, `cleanup`, `tune`, `status`)
- `tools/sensor-trait/` — base `Sensor` trait, `SensorSlot`, engagement state
- `tools/sensor-context/`, `sensor-git/`, `sensor-peers/`, `sensor-processes/` — the built-in crate sensors
- `tools/agent-fmt/` — shared terminal formatting (banners, tables, commands)
- `skills/attend/SKILL.md` — the invocation skill the agent reads when the user asks for awareness
- `hooks/ways/softwaredev/environment/attend/` — the way that surfaces live attend state in the steering layer
