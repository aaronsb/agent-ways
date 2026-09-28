---
status: Accepted
date: 2026-04-09
revised: 2026-04-10
deciders:
  - aaronsb
  - claude
related:
  - ADR-104
  - ADR-111
  - ADR-112
  - ADR-114
---

# ADR-113: `attend` — Active Awareness Module

## Context

The [Cognitive Loop and the Awareness Layer](../../design-notes/cognitive-loop-and-awareness-layer.md) design note reads the agent-ways system as a cognitive loop with one missing stage: **active perception**. Every other stage is in place — reactive guidance (ADR-100, ADR-103, ADR-105, ADR-108), attention allocation via disclosure gating (ADR-104), episodic memory (ADR-112 ledger), associative recall (ADR-112 KG), consolidation (compaction plus compaction-checkpoint way), project-level awareness at session entry (ADR-106), and the scoring infrastructure that binds these together. What has been missing is a mechanism by which Claude gains cheap peripheral awareness of its own approaching consequences and the environment around it, without spending reasoning tokens to compute either.

The consequence of that gap is well-defined: Claude operates in a partially blind configuration. Self-monitoring is expensive (costs tokens) and unreliable (can fail to fire). Environmental sensing requires explicit tool use (which consumes an entire turn for a single observation). Consequence tracking happens only when Claude chooses to check, by which point the consequence may be too near to respond to usefully.

This ADR defines the missing layer as a new binary, separate from `ways`, co-resident in the agent-ways workspace.

### The delivery primitive: `Monitor`

An additional and load-bearing reason this ADR is possible *now* rather than earlier is the `Monitor` tool in Claude Code. Before `Monitor`, there was no standardized channel by which a background process could surface observations into Claude's conversation as asynchronous events. The hook system delivers synchronous injections at event boundaries, but it has no mechanism for signals that occur *between* those boundaries.

`Monitor` closes that gap by treating a background script's stdout as an async event stream, delivering each line as a notification in Claude's chat. With `persistent: true`, a single `Monitor` invocation covers the duration of a Claude Code session. This is the delivery primitive the awareness layer depends on.

### Why a new binary (and not a ways subcommand)

The `ways` binary is a matcher, scorer, and disclosure gate — a stateless computation engine invoked by hooks. It responds to events; it does not observe between them. Adding a durable, long-lived executive layer to `ways` would conflate two different lifecycles.

A sibling crate in the same workspace preserves the integration surface while keeping lifetimes clean. `attend` runs as a separate process and writes single-line observations to stdout. `Monitor` delivers those lines to Claude as asynchronous notifications.

### Prior attempts and how this differs

ADR-101 (Wormhole relay, Deprecated) and ADR-102 (IRC-based agent communication, Abandoned) both attempted inter-instance awareness through external transports. Both failed: ADR-101 on transport fragility, ADR-102 on complexity.

This ADR succeeds where those failed because it uses the **filesystem as transport** — `~/.claude/sessions/*.json` for session discovery, `~/.cache/attend/signals/` for inter-session messaging. No external services, no fragile protocols. The filesystem is stable because Claude Code depends on it (session files) and third-party tools depend on it (abtop reads the same session files). The transport is load-bearing for the ecosystem, not just for attend.

## Decision

Introduce `attend`, a new Rust binary hosted as a sibling crate to `ways` in the agent-ways Cargo workspace. `attend` implements the active awareness layer: durable, restartable, session-scoped, additive to (not required by) Claude Code.

### Identity

- **Name:** `attend`
- **Binary:** `attend`
- **Crate:** `attend/` (sibling to `ways/` in the workspace)
- **Role:** Active awareness module — sensor loop, peer awareness, inter-session signaling
- **Lifecycle:** Per work session. Explicit start via `/attend` skill or `attend run`. Exits cleanly on signal.

### CLI

`attend` is a normal CLI. Bare `attend` prints help with an ANSI header matching the ways visual style.

```
attend run                          # sensor loop (launched via Monitor)
attend run --catchup                # process existing signals, then watch forward
attend send "message"               # signal to peers (project + focus scope)
attend send --broadcast "message"   # signal to all sessions
attend send --to /path "message"    # directed signal to specific project
attend inbox                        # read pending messages chronologically
attend peers                        # list active Claude Code sessions
attend focus add ~/path             # add project to focus group
attend focus remove ~/path          # remove from focus group
attend focus list                   # show current focus group
attend focus clear                  # back to project-only mode
attend status                       # running instances, signals, focus state
attend --version                    # version + git commit hash
```

### Internal architecture

`attend` is composed of small modules with clear responsibilities. The coordination center is the **tick loop** — a wall-clock-driven game loop that runs sensors on adaptive schedules.

```
attend/
├── build.rs              # Bakes git commit hash at compile time
├── src/
│   ├── main.rs           # CLI dispatch, subcommands, disclosure governor
│   ├── tick/mod.rs       # AdaptiveInterval: ramp-up on change, hysteresis decay
│   ├── delta/mod.rs      # DeltaAccumulator: per-sensor state-change tracking
│   ├── emit/mod.rs       # stdout for Monitor delivery, stderr for diagnostics
│   └── sensors/
│       ├── mod.rs        # Sensor trait, Focus struct, SensorSlot runtime wrapper
│       ├── context.rs    # Interoceptive: context window pressure via `ways context`
│       ├── git.rs        # Git state: dirty files, branch changes, upstream divergence
│       ├── peer.rs       # Peer sessions + signal file reading
│       └── process.rs    # Application presence tracking
```

### Sensor model

All sensors implement a common trait:

```rust
pub trait Sensor {
    fn name(&self) -> &str;
    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)>;
    fn emission_threshold(&self) -> f64;
    fn base_interval(&self) -> Duration;
    fn min_interval(&self) -> Duration;
    fn decay_threshold(&self) -> u32;
}
```

Each sensor is wrapped in a `SensorSlot` that provides adaptive interval scheduling and delta accumulation. The tick loop polls sensors via a priority queue ordered by next-fire time.

The `Focus` struct describes what Claude is currently attending to (description, working directory, keywords). Sensors filter observations through Focus to determine relevance.

#### Built-in sensors

| Sensor | Type | Polls | Reports |
|--------|------|-------|---------|
| **context** | interoceptive | `ways context --json` | threshold crossings, velocity, projection to critical |
| **git** | exteroceptive | `git status`, `git rev-list` | dirty files, branch changes, upstream divergence |
| **peers** | exteroceptive | `~/.claude/sessions/*.json` + signal files | session appear/exit, state changes, peer messages |
| **processes** | exteroceptive | `ps` | application presence (not PID churn) |

The context sensor is calibrated to complement — not duplicate — ways' existing context-threshold triggers (todos@75%, memory@80%, checkpoint@95%). Attend provides early warnings *before* those thresholds and velocity/projection *between* them.

#### Script sensors (planned)

Sensors as unit files — declare what to run, how often, what threshold matters. Attend is the scheduler; sensors are units. Two-layer config mirrors ways scoping:

```
~/.config/attend/config.yaml          # user scope — always loaded
{project}/.claude/attend.yaml         # project scope — layered on top
```

Project config uses +/- to modify the sensor set:

```yaml
# project/.claude/attend.yaml
sensors:
  +hardware:
    script: .claude/sensors/check-hardware.sh
    interval: 120
    threshold: 2.0
  -processes:  # not relevant here
```

Script sensor contract: output `magnitude|description` lines to stdout. Empty output = no change. Same adaptive interval and delta accumulation as built-in sensors.

Trust model follows ways: user-scope config is trusted, project-scope scripts get the same scrutiny as project-scope way macros.

### Tick loop

The core loop is wall-clock-driven, not turn-driven. It runs continuously for the session's lifetime.

Each tick:

1. Check the priority queue for sensors whose next-fire time has arrived
2. Run due sensors, collect observations
3. For each observation, feed through the sensor's delta accumulator
4. If state changed: shorten polling interval (ramp up), reset decay cooldown
5. If no change: increment decay cooldown; if threshold exceeded, lengthen interval back toward base
6. Check whether any accumulator has crossed its emission threshold
7. Feed threshold-crossing observations through the disclosure governor
8. Emit to stdout (Monitor delivery)

Quiet polls produce no stderr output. Only actual state changes are logged.

### Adaptive sensor scheduling

Each sensor maintains adaptive scheduling state:

- **Ramp-up is fast.** When change is detected, interval halves (down to `min_interval`). Missing real change is worse than over-sampling.
- **Decay is slow (hysteresis).** The sensor must see `decay_threshold` consecutive quiet ticks before its interval lengthens. Prevents oscillation.
- **False positives self-correct.** High-frequency samples confirm or deny, then the sensor decays back to baseline.

### Disclosure governor

The disclosure governor is a global rate limiter controlling when attend emits to stdout. Architecturally load-bearing — without it, sustained notification delivery destabilizes Claude's turn-taking model. Experimentally validated: 10+ notifications in 2 minutes caused confabulated user turns; 3 per 2 minutes was completely stable.

Two constraints:

1. **Adaptive cooldown.** Higher aggregate event rate → longer wait between disclosures. Scales with square root of event rate.
2. **Hard cap per window.** Maximum 3 disclosures per 120-second window.

When multiple sensors are ready simultaneously, they're emitted as a **batch** within Monitor's 200ms batching window, consuming one disclosure slot.

Prototype ratio: 76 internal sensor ticks → 3 notifications over 2 minutes.

### Peer awareness and inter-session signaling

Attend discovers peer Claude Code sessions by reading `~/.claude/sessions/*.json` and their transcript JSONL files — the same stable pattern used by `abtop`. This provides:

- **Session discovery:** who else is running, which project, what model, context usage
- **State tracking:** session appear/exit, status changes (working/waiting), context pressure

#### Signal files

Inter-session messaging uses signal files in a project-scoped directory structure:

```
~/.cache/attend/signals/
├── _broadcast/                    # all attend instances read this
├── -home-aaron-.claude/           # only attend in ~/.claude reads this
├── -home-aaron-temp/              # only attend in ~/temp reads this
└── focus                          # list of peer project dirs to watch
```

Signal format: `from|project|cwd|message` (one line, atomic write via tmp+rename).

Sender identity is automatically detected:
- **From a Claude session:** `claude:session-id` → displays as `claude/project`
- **From a human terminal:** `external:user@terminal` → displays as `aaron@kitty`

Terminal detection checks KITTY_PID, ALACRITTY_SOCKET, WEZTERM_PANE, TMUX, STY, TERM_PROGRAM, SSH_CONNECTION.

#### Sidetone prevention

Attend filters own signals by checking the `from` field against its own session ID, not by filename prefix. This works across all scoped directories.

#### Forward-only mode

`attend run` marks all existing signals as seen on startup. Only signals arriving *after* launch produce notifications. `attend inbox` reads all pending messages (one-shot catchup). `attend run --catchup` processes existing signals then watches forward.

#### Focus groups

A focus group is a set of peer project directories. Send scope mirrors receive scope — messages go to everyone you're listening to.

```bash
attend focus add ~/Projects/foo ~/temp    # listen to + send to these
attend focus list                          # show current group
attend focus clear                         # project-only mode
```

#### Reply hints

The first peer message includes a reply hint: `(reply: attend send --to /path <msg>)`. Subsequent messages are clean — progressive disclosure, not repetitive nagging.

### Self-documenting startup

On launch, attend emits a single usage summary to stdout so Monitor delivers it as Claude's first notification:

```
[attend] v0.1.0 (459534c) — sensors: context, git, peers, processes | focus: project + temp | commands: attend send <msg>, attend inbox, attend peers, attend focus add <path>
```

### Emission format

All notifications use bracketed key-value format:

```
[attend sensor=context priority=medium] context at 50% — midpoint, wrap-up window opening (burning 1.8%/min, ~25 min to todos checkpoint at 75%)
[attend sensor=peers priority=high] message from aaron: checking updates
[attend sensor=git priority=low] new commits on main (HEAD abc1234 → def5678)
```

This format was chosen empirically: Monitor entity-escapes angle brackets but passes square brackets verbatim.

### Way integration

A way at `softwaredev/environment/attend` provides progressive disclosure of attend's capabilities. The way body is static CLI reference; a `macro.sh` script (appended at disclosure time) checks live state: whether attend is installed and running, current focus group, active peer count, pending signals.

The `/attend` skill launches attend via Monitor with explicit instructions to use Monitor (not Bash).

### Coordination with ways context-threshold triggers

Ways fires actions at context thresholds: todos@75%, memory@80%, checkpoint@95%. Attend's context sensor provides:

- Early warnings before ways thresholds (40%, 50%, 65%)
- Verification prompts after ways fires (85% = "verify memory saved")
- Pre-critical warning (92% = "finish task before 95% checkpoint")
- Velocity and projection between all thresholds

Attend handles trajectory awareness. Ways handles threshold actions.

### Build and install

```
make attend          # build (or skip if already built)
make attend-rebuild  # force rebuild
make install         # symlinks bin/attend → tools/target/release/attend
```

Binaries are symlinked to the cargo build output, not copied. Cargo handles atomic replacement of the target binary; the old process keeps running on the old inode. No ETXTBSY on rebuild while attend is running.

### Hard invariants

These constraints are non-negotiable:

1. **Filesystem as transport.** Inter-session awareness uses the filesystem (session files, signal files), not external services. No fragile protocols, no servers, no pubsub.
2. **Informational, not enforceable.** Attend emits observations. It never overrides Claude, never forces action, never bypasses the disclosure gate.
3. **Consequence-anchored.** Context warnings track real mechanical consequences. No arbitrary urgency, no simulated affect.
4. **Metadata-only for content-bearing sensors.** Sensors that touch content-rich sources emit only boolean or categorical state, never the content itself.
5. **Additive, never required.** Attend is optional. Ways that depend on attend signals gracefully no-op when attend is not running.
6. **Explicit invocation only.** Attend never autostarts.
7. **Send scope mirrors receive scope.** Messages go to the same set of projects you're listening to. No silent broadcasting.

#### Invariant revision note

The original draft of this ADR included "no cross-session signal" and "no inter-instance protocol" as hard invariants. These were revised after implementation demonstrated that inter-session awareness via the filesystem is stable, useful, and architecturally clean — the same session files Claude Code already publishes, the same pattern abtop already reads. The prior invariants were a reaction to ADR-101/102's failures with fragile external transports, not a principled objection to inter-session awareness itself. The revised invariants preserve the underlying concern (no fragile protocols) while permitting what works (filesystem-based observation and signaling).

## Consequences

### Positive

- **Agency preservation.** Claude gains accurate awareness of approaching consequences in time to act on them.
- **Reduced token waste.** Interoceptive sensors replace expensive self-checks.
- **Proactive environmental awareness.** Git state, peer sessions, process lifecycle — ambient signal at near-zero token cost.
- **Inter-session collaboration.** Multiple Claude instances and human terminals communicate through a shared signal system. No protocol overhead — just files on disk.
- **Sensor toolkit composability.** Adding a new capability means adding a script or a compiled sensor module. The scheduler, delta accumulation, and disclosure governor handle the rest.
- **Cleanly additive.** Sessions that don't need active awareness pay no cost.
- **Standard delivery primitive.** `attend` is a well-behaved Monitor client. Its integration surface is "write lines to stdout."

### Negative

- **New binary to maintain.** Mitigation: sibling crate in the same workspace, shares build infrastructure with `ways`.
- **Dependency on Monitor.** If Monitor is unavailable, attend cannot deliver. Mitigation: attend still runs and maintains state; delivery resumes when Monitor becomes available.
- **Signal file management.** Stale signals accumulate if no attend instance polls the directory. Mitigation: 5-minute cleanup on each poll; `attend cleanup` planned.
- **Binary version skew.** Multiple attend instances may run different versions. Mitigation: the signal format and directory layout are stable; version skew doesn't break interop.

### Neutral

- **Rust implementation.** Matches `ways`. Zero external dependencies (no serde, no clap, no tokio).
- **XDG conventions.** Signals in `~/.cache/attend/signals/`, config planned for `~/.config/attend/`.

## Remaining work

Tracked in aaronsb/agent-ways#2:

- **Config externalization** — `~/.config/attend/config.yaml` with project-scope overlay
- **Script sensor runner** — poll scripts, parse `magnitude|description` from stdout
- **State persistence** — checkpoint/restore sensor baselines across restarts
- **Self-reload** — watch own binary mtime, exec self on change
- **ADR-114 integration** — affordance strings, `trigger.type: attend` in ways schema
- **Insistence tracker** — unacted observations re-surface with escalating urgency
- **Consequence model** — generalized beyond context pressure

## Alternatives Considered

- **Subcommand of `ways`.** Rejected. Conflates stateless per-invocation lifecycle with durable per-session lifecycle.
- **Pure hook-based implementation.** Rejected. Sensors that observe between turns need a persistent process.
- **External transport (wormhole, IRC).** Rejected by ADR-101/102 failures. Filesystem transport is simpler and more stable.
- **Fixed-interval scheduler.** Rejected. Adaptive intervals ramp on change, decay on quiet — better sampling efficiency.
- **Separate repository.** Rejected per ADR-111. Shared workspace avoids coordination churn.

## References

- **Design note:** [Cognitive Loop and the Awareness Layer](../../design-notes/cognitive-loop-and-awareness-layer.md)
- **Tracking issue:** [aaronsb/agent-ways#2](https://github.com/aaronsb/agent-ways/issues/2)
- **Related ADRs:**
  - [ADR-104](./ADR-104-token-gated-way-re-disclosure-for-long-context-windows.md) — Disclosure gate
  - [ADR-111](./ADR-111-unified-ways-cli-single-binary-tool-consolidation.md) — Sibling-crate pattern
  - [ADR-112](./ADR-112-session-ledger-and-knowledge-graph-integration.md) — Session ledger
  - [ADR-114](./ADR-114-attend-as-insistent-way-trigger-type.md) — Way trigger type for attend signals
- **Prior attempts:**
  - [ADR-101](./ADR-101-wormhole-relay-protocol-for-cross-instance-agent-communication.md) — Wormhole relay (Deprecated)
  - [ADR-102](./ADR-102-irc-based-local-agent-communication.md) — IRC-based agent communication (Abandoned)
