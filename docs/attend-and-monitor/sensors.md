# Sensors — the built-in set

Attend ships six built-in sensors. Three are modules of the attend crate and always compiled in: `context`, `git` and `disclosure`, under `tools/attend/src/sensors/`. Three are separate crates linked behind feature flags: `sensor-peers`, `sensor-processes` and `sensor-keepwarm`. Anything else is a sensor of your own, an external script or another crate (see [`authoring-sensors.md`](authoring-sensors.md)).

`attend sensors` lists what is running:

```
  Sensor     Kind    State  Interval  Description                                                   Source
  ────────────────────────────────────────────────────────────────────────────────────────────────────────────
  context    builtin active 60s / 20s tracks context window usage and projects compaction           attend@0.15.2
  git        builtin active 30s / 10s tracks dirty files, branch changes, and upstream divergence   attend@0.15.2
  peers      builtin active 30s / 10s discovers Claude Code sessions and reads peer signals         sensor-peers@0.6.0
  processes  builtin active 30s / 5s  watches build/package tools and correlates exits with markers sensor-processes@0.6.0
  disclosure builtin active 60s / 20s reheats affordance instructions on token-distance drift       attend@0.15.2
  keepwarm   builtin active 60s / 60s keeps the prompt cache warm with one wake per idle stretch    sensor-keepwarm@0.1.0
```

## Summary

| Sensor | Observes | Interval / min | Threshold | Lane |
|---|---|---|---|---|
| **context** | the session's own token usage | 60s / 20s | 1.5 | event |
| **git** | the working tree and its upstream | 30s / 10s | 2.0 | event |
| **peers** | other Claude sessions and peer messages | 30s / 10s | 2.0 | message |
| **processes** | build and dev-tool processes | 30s / 5s | 2.0 | event |
| **disclosure** | tokens since attend last taught the messaging contract | 60s / 20s | 5.0 | event |
| **keepwarm** | idle time against the prompt-cache hour | 60s / 60s | 3.0 | message |

Every sensor polls faster (toward its minimum interval) while it sees change and slower (toward its base interval) when quiet. Event-lane sensors pass through the action-potential refractory and the strict governor ([`engagement.md`](engagement.md)). Message-lane sensors skip the refractory and use the permissive governor ([`delivery.md`](delivery.md#the-monitor-line)).

When a sensor discloses, its events print to stdout only if its accumulated magnitude has reached 3.0 (medium priority) or 5.0 (high). Below that they go to stderr, and if another sensor in the same batch did print, one `[attend] also: N quiet event(s) from …` line counts them.

## `context`

The sensor that watches the session itself rather than the world (ADR-113). Each poll runs `ways context --json` and reads the current token usage.

**Tier crossings.** Crossing into a tier emits once per session:

| % used | Magnitude | Message |
|---|---|---|
| 50% | 1.5 | halfway through the context window — keep going, or scope the next stages |
| 65% | 3.0 | agent-ways will capture todos (75%) and memory (80%) shortly |
| 83% | 4.0 | compaction checkpoint fires at 85% — finish the current task and sync |
| 90% | 5.0 | stop or compact right now — quality degrades past here |

When the burn rate is known, the line adds it and, below 90%, the minutes left to 90%. The 90% line also names the way to read: ``Use `ways show attend context-pressure --session $CLAUDE_SESSION_ID` for reflection guidance``.

**Velocity spikes.** Burning more than 2% a minute with more than 5 points of change since the previous reading emits `context velocity spike: X% in last N min (V%/min)` at 2.0.

The sensor surfaces observations only. The ways layer acts at its own thresholds.

## `git`

Watches git state in the working directory through `git` with `GIT_OPTIONAL_LOCKS=0`, so it never races a foreground commit. It reports deltas between polls:

| Event | Magnitude | Example |
|---|---|---|
| branch changed | 3.0 | `branch changed: main → feat/new-thing` |
| new commits on the current branch | 2.0 | `new commits on feat/attend (HEAD abc123 → def456)` |
| upstream moved ahead | 2.5 | `4 new commits on upstream (now 4 behind)` |
| new dirty files | 1.5 | `3 new dirty files: src/main.rs, src/config.rs, Cargo.toml` |
| working tree clean | 1.0 | `working tree clean (changes committed or stashed)` |
| ahead of upstream | 1.0 | `2 commits ahead of upstream (unpushed)` |

It does not read commit messages, tags or hooks.

## `peers`

Discovers other Claude Code sessions from `~/.claude/sessions/*.json` and reads peer messages from the signals base. This is the sensor that makes multi-agent work possible: who else is working, where, and what they said.

**Sessions.**

| Event | Magnitude |
|---|---|
| peer session started, same project / other project | 3.0 / 1.0 |
| peer session exited, same project / other project | 2.0 / 0.5 |
| same-project peer changed status (working, waiting) | 1.5 |
| same-project peer crossed 80% context | 2.0 |

**Messages.** Each poll scans the project tray, `_broadcast/` and every joined channel, and emits every unseen message. Base magnitudes are 7.0 for a message sent to the project, 5.0 for a channel and 4.0 for `#open`. The base is multiplied by how often the same peer has written within `engagement.peer_activity_window` (default 900 s): ×1.0 for the first message, ×1.75 for the second, ×2.5 from the third. The boost raises priority; it never suppresses a message. More than 8 messages in one poll become one count line. [`delivery.md`](delivery.md) covers the line format, the cold start and the Stop-hook drain.

The whole sensor rides the message lane, so session events skip the refractory too. That is a known limitation of ADR-136, which chose lanes per sensor rather than per observation.

The sensor re-reads `_groups.yaml` on every poll, so `attend join` and `attend leave` take effect on the next scan.

## `processes`

Tracks dev-tool processes in `ps` output, not every process. A process produces start and exit events only if its name is on the sensor's tracked list (build tools, runtimes, editors, servers, `ssh`, `git` and similar, in `tools/sensor-processes/src/lib.rs`) or contains one of the session's focus keywords.

| Event | Magnitude | Example |
|---|---|---|
| process started | 2.0 | `cargo started` |
| process exited | 2.0 | `nvim exited` |
| watched build tool exited, no marker | 2.5 | ``cargo exited. Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps`` |
| watched build tool exited, success | 2.5 | `cargo exited (success). …` |
| watched build tool exited, failure | 3.5 | `cargo exited (failure, code 101). …` |

### The watch list

The watch list picks which exits get build enrichment: the affordance line and the success or failure magnitude. The default is:

```
cargo, rustc, make, cmake, ninja,
gcc, g++, cc, c++, clang, clang++,
go, npm, yarn, pnpm, tsc,
mvn, gradle, pip, pip3
```

A watched tool that is not also on the tracked list produces no events. `sensors.processes.watch` replaces the default; it is not merged, and an empty list turns enrichment off:

```yaml
sensors:
  processes:
    watch: [cargo, rustc, mix, zig]
```

### Exit codes through a marker file

`ps` cannot see an exit code after the process is gone. To get success or failure, wrap the build so it writes one line to `$XDG_STATE_HOME/attend/last-build-status` (`~/.local/state/attend/last-build-status`) as it finishes:

```sh
attend_build() {
  "$@"
  local code=$?
  local dir="${XDG_STATE_HOME:-$HOME/.local/state}/attend"
  mkdir -p "$dir"
  printf '%s|%d|%d\n' "$1" "$code" "$(date +%s)" > "$dir/last-build-status"
  return $code
}
# usage: attend_build cargo build
```

The format is `cmd|exit_code|unix_ts`. When a watched tool exits, the marker's `cmd` matches, and the marker is under 60 seconds old, the exit is enriched; otherwise it falls back to the plain line. The marker is one slot for the whole machine, so concurrent builds overwrite each other. On a network filesystem, write to a temporary file and `mv` it into place.

The sensor does not capture output, and it does not batch a build's child processes: `cargo` and each `rustc` it starts are separate events.

## `disclosure`

Re-teaches the messaging contract when it has drifted out of the agent's recent context (ADR-122). Each poll reads the token count from `ways context --json`. The first poll of an `attend run` emits the contract (how to send, reply and scope messages, the drain, keepwarm, the CLI-is-the-contract rule), and it emits again each time the session has moved 25% of the context window since the last time. The magnitude is 5.0, so the line prints at high priority on a single poll.

The body is `tools/attend/src/sensors/disclosures/messaging.md`, compiled into the binary. It is kept in step with the attend skill and the attend way. The ledger is in memory only, so a restarted run teaches again on its first poll.

## `keepwarm`

While armed, wakes an idle session once at 50 idle minutes so the prompt cache is read before its hour lapses (ADR-182). It emits at 3.0 on the message lane and needs a resolved session id; without one it does not start. Arming, the status card, the cost model and the agent's one-word contract are in [`keepwarm.md`](keepwarm.md).

## Choosing where a new observation goes

1. **Does a built-in cover it?** The `git` sensor already watches git state.
2. **Can a shell command observe it?** Then write an external script sensor: no Rust and no rebuild. Almost everything belongs here, from GitHub Project boards to custom event logs. See [`authoring-sensors.md`](authoring-sensors.md).
3. **Does it need native speed, shared state or complex logic?** Then write a crate sensor: a new `sensor-*` crate in `tools/` that implements `Sensor`, added to attend's `Cargo.toml` as an optional dependency with a feature, and registered in `tools/attend/src/sensors/mod.rs`.

## Related

- [`authoring-sensors.md`](authoring-sensors.md) — writing new sensors
- [`engagement.md`](engagement.md) — the event lane's refractory
- [`delivery.md`](delivery.md) — the message lane and its two conduits
- [`keepwarm.md`](keepwarm.md) — the keepwarm sensor in full
- [`configuration.md`](configuration.md) — per-sensor keys and defaults
- **ADR-113** — attend and the first context sensor
- **ADR-117** — sensor crate extraction and feature flags
- **ADR-122** — the disclosure sensor
- **ADR-136** — the event and message lanes
- **ADR-182** — keepwarm
