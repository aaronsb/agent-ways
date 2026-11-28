# Attend

This directory documents the active awareness layer. Attend gives a session the awareness an employee would otherwise have ambiently: what is changing, who else is working, and what deserves attention. In Claude Code it reaches a session through the attend mod, a Claude Code plugin that delivers what attend has for the session as turns ([`mod.md`](mod.md)), and humans and other Claude agents participate in the same signal stream. The directory's name dates from when attend ran under Claude Code's `Monitor` tool; every delivery now goes through the mod.

## The thesis

A Claude Code session sees what happens in its conversation and nothing else. A branch switch in another terminal, a build that finished, a peer session that started in the same repository, or a message from another agent arrives at an unknown time, and the agent only learns of it when someone asks. Each session could poll for such things with commands of its own, but then every session reinvents the observation logic from scratch.

Attend is that observation logic, written once. It knows what kinds of changes matter, it governs the rate of environmental events through a formal engagement model, borrowing the activation-decay shape from ACT-R, it routes messages between peer agents through channels on a message lane that never drops them, and it cleans up after itself by reaping the messages of projects that are gone. Its event-lane governor is alarm management (ISA-18.2) applied to agent notifications: it rate-limits what reaches the conversation so the channel stays trustworthy instead of noisy. That editorial policy is calm technology (Weiser & Brown) applied to a coding session: most changes stay in the periphery, and only what deserves attention moves to the center. A Claude session that listens gets a stable, opinionated awareness channel without tuning it case by case.

## The parts

**The sensor host** is attend's sensor loop. It watches the local environment (git state, peer sessions, context pressure, process activity, and any sensor a user adds as a file) and, when something crosses a threshold, writes the observation into the signal store for its session. The attend mod starts one host per listening session, as its own child process (`attend sensor host`), and the host lives as long as the session listens ([`loop.md`](loop.md)).

**The attend mod** decides whether a session listens, polls the signal store while the session idles, and delivers what is pending as a turn when something in it should wake the session. It also serves typed tools for sending, draws one status row, and drives keepwarm ([`mod.md`](mod.md)). A Stop hook delivers whatever is pending at the end of each turn ([`delivery.md`](delivery.md)).

**The signal store** is a directory of small files under attend's cache. Peer messages, human-typed lines and the sensor host's reports are all signals in it, and every reader marks what it delivered in one seen-set per session ([`signals.md`](signals.md)).

A session without the mod gets none of this delivered: no idle wake, no turn-end drain, no sensor reports and no keepwarm wake. That covers a `claude -p` run or an SDK host that loads no plugins, an install whose policy blocks plugins, and Claude Code before 2.1.296. The `attend` CLI still sends, reads and lists by hand there.

## Two consumers, one channel

Attend is not exclusively for AI agents. A human can use it too, and both consumers share the same signal bus. The peer layer underneath is workspace awareness (Dourish & Bellotti's CSCW term) applied to coding agents, meaning knowing who else is working, where, and what they said, served to humans and agents through one protocol.

**Agent mode, through the attend mod:** a session listens when its project is set active or when the user asks for attend and the agent calls `attend_listen`. The mod delivers messages and sensor reports, and the agent answers with the `attend_reply` and `attend_send` tools and joins channels with `attend_join` ([`mod.md`](mod.md)). See [`loop.md`](loop.md) for the sensor host and [`delivery.md`](delivery.md) for the two delivery conduits.

**Human mode, `attend chat`:** a human opens attend-chat, a terminal chat screen on the same message bus. Every channel streams into it, and the human addresses Claude agents (`@Elio ship it`, `@Elio @Lachlan sync first`, `#deploy hold`) through the same signal files the agents use, and steers a multi-agent session from one surface. See [`tui.md`](tui.md).

As far as attend is concerned, a human and an AI agent are the same kind of participant: same signal protocol, same routing, same channels. The human reads with a screen and types with a keyboard, where the agent reads its context window.

This dual-consumer property is deliberate. It means the signal protocol is dogfooded: the human feels the routing semantics directly, and any friction in how messages land is friction the agents also experience. It also means a solo developer watching one agent gets as much value as a coordinator orchestrating four.

## Scale context

The attend development effort so far has put significant weight on the multi-agent peer-messaging story: channels, the message lane, turn-boundary delivery, cross-session signal routing. That's a real use case but it's not the common one. **Most uses of attend will be single-user: one human, one Claude agent, external state sources.**

The common-case shape looks like this: you're writing code, Claude is helping, and you want the session to notice things that happen outside the conversation — a `make test` finished, an issue got assigned to you on GitHub, a long-running deploy finished. Attend's sensors feed those into the conversation without you having to interrupt and ask "what's the status?" The peer-messaging layer is still there if you ever run four agents at once, but you don't have to opt into it to get value.

This is why **sensors of your own are a first-class design surface** — they're the bridge between attend's formal engagement model and whatever the user actually cares about watching. See the next section.

## Sensor authorship is a design surface

Every sensor is one YAML file under one spec (ADR-403), layered by name: the files attend ships, then your own under `$XDG_CONFIG_HOME/attend/sensors/`, then a project's under `.claude/attend/sensors/`. A file's `kind` says what polls:

- **`crate`** configures a compiled sensor: a Rust crate or attend module implementing the `Sensor` trait from `sensor-trait`. Most shipped sensors are of this kind: `context` and `git` (modules of attend itself, always compiled), and the `sensor-peers` and `sensor-processes` crates. A file of your own changes any of their fields without a rebuild.
- **`command`** runs an argv and reads its output, and **`mcp`** calls one read-only tool on a stdio MCP server. Neither needs Rust or a recompile. attend runs both; an `mcp` sensor runs once `attend sensor probe` has recorded that reading its tool leaves it unchanged. The shipped `example-command.yaml` and `example-mcp.yaml` are the patterns to copy.

**Every kind shares one constraint: the sensor author has to understand the loop's intention.** Events are not log lines. They are magnitude-weighted observations that feed an accumulator, decay over time, and are disclosed only when they cross a refractory-aware threshold. A sensor that emits "something happened" on every tick will either flood the governor or get suppressed into silence by action potential. A well-designed sensor encodes *how much each kind of change matters* in the event magnitude, and lets the loop handle the rest.

See [`authoring-sensors.md`](authoring-sensors.md) for the spec and the author's guide.

## What lives in this directory

| File | Purpose |
|---|---|
| [`README.md`](README.md) | Orientation — you are here |
| [`loop.md`](loop.md) | The sensor host and its loop — state diagrams, timing, the two lanes |
| [`first-sensor.md`](first-sensor.md) | Walkthrough — write your first sensor file |
| [`authoring-sensors.md`](authoring-sensors.md) | The sensor file spec and writing sensors (reference) |
| [`sensors.md`](sensors.md) | The shipped sensors, their states, and what each observes and emits |
| [`engagement.md`](engagement.md) | The action-potential model on the event lane |
| [`signals.md`](signals.md) | Signal file format, storage layout, lifecycle |
| [`delivery.md`](delivery.md) | The mod's poll and the Stop-hook drain, system signals, enrollment, opt-out |
| [`mod.md`](mod.md) | The attend mod in Claude Code: dormant and active sessions, its config, install |
| [`channels.md`](channels.md) | Channel membership and lifecycle |
| [`tui.md`](tui.md) | `attend chat` — the human on the bus |
| [`keepwarm.md`](keepwarm.md) | Holding the prompt cache warm across idle time |
| [`configuration.md`](configuration.md) | Config schema, overlay semantics, permissions |
| [`../cli/attend.md`](../cli/attend.md) | The `attend` CLI reference, generated from the binary |

## Reading order

1. **[`mod.md`](mod.md)** — how a Claude Code session starts listening, and what the mod does while it listens.
2. **[`loop.md`](loop.md)** — the sensor host, which every sensor report comes from.
3. **[`delivery.md`](delivery.md)** — how a peer message or a sensor report reaches a session, and how to opt out.
4. **[`tui.md`](tui.md)** — if you're a human using attend directly, or coordinating multiple agents.
5. **[`first-sensor.md`](first-sensor.md)** — if you want to tune or write a sensor and you've never done it before.
6. **[`authoring-sensors.md`](authoring-sensors.md)** — the sensor file spec and design surface once the tutorial isn't enough.
7. **[`sensors.md`](sensors.md)**, **[`engagement.md`](engagement.md)**, **[`signals.md`](signals.md)**, **[`channels.md`](channels.md)**, **[`keepwarm.md`](keepwarm.md)**, **[`configuration.md`](configuration.md)** — reference, read as needed.

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
- **ADR-123** — firing dynamics unification; the engagement engine in force
- **ADR-136** — the message lane, split from the event lane
- **ADR-172** — turn-boundary delivery through the Stop-hook drain
- **ADR-173** — the chat idiom: channels, `join`/`leave`, tabs and slash commands
- **ADR-182** — keepwarm
- **ADR-403** — sensors as files under one spec: crate, command and mcp
- `docs/hooks-and-ways/` — sibling docs for the synchronous hook mechanism
- `docs/architecture/practice/ADR-600-cognitive-loop-and-the-awareness-layer.md` — earlier design exploration that informed ADR-113

## Where the code lives

- `tools/attend/` — orchestrator and CLI; [`../cli/attend.md`](../cli/attend.md) lists every subcommand
- `tools/attend-chat/` — the chat screen, on `agent-tui`
- `tools/attend-groups/`, `attend-presence/`, `attend-state/`, `attend-config/` — channels, heartbeats and enrollment, the per-session seen-set, the config schema
- `tools/sensor-trait/` — base `Sensor` trait, `SensorSlot`, engagement state
- `tools/attend/src/cmd/run/` — the sensor host (`attend sensor host`) and its loop
- `tools/attend/src/sensors/` (`context`, `git`), `tools/sensor-peers/`, `sensor-processes/` — the compiled sensors
- `tools/sensor-keepwarm/` — keepwarm's judgment, which `attend keepwarm check` runs
- `attend/sensors/` — the shipped sensor files; `tools/sensor-spec/` parses, layers and checks them, and `tools/attend-config/src/sensors.rs` finds them
- `plugins/attend/` — the attend mod
- `hooks/ways/attend-drain-stop.sh` — the Stop hook that drains messages at the turn boundary
- `tools/agent-fmt/` — shared terminal formatting (banners, tables, commands)
- `skills/attend/SKILL.md` — the skill behind `/attend`, which calls `attend_listen` or says the mod is not loaded
- `skills/supervise/SKILL.md` — the skill behind `/supervise`, which watches a relay-mode worker every 30 minutes, nudges it with a neutral question and forwards its decisions to the operator
- `hooks/ways/softwaredev/environment/attend/` — the way that surfaces live attend state in the steering layer

## References

- ADR-118: channels, which route messages between peer agents.
- ADR-122: the disclosure sensor, which ADR-416 removed.
- ADR-123: the engagement model that governs the rate of environmental events.
- ADR-136: the message lane, and reaping the messages of projects that are gone.
- ADR-173: the channel vocabulary and chat verbs.
- ADR-406: the attend mod.
- ADR-416: awareness reaches a Claude Code session only through the attend mod, which starts the sensor host; Monitor delivery removed.
