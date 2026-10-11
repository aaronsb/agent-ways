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

Every sensor polls faster (toward its minimum interval) while it sees change and slower (toward its base interval) when quiet. Event-lane sensors pass through the action-potential refractory and the strict governor ([`engagement.md`](engagement.md)). Message-lane sensors skip the refractory and use the message lane's permissive governor ([`delivery.md`](delivery.md#the-monitor-line)).

When a sensor discloses, its events print to stdout only if its accumulated magnitude has reached 3.0 (medium priority) or 5.0 (high). Below that they go to stderr, and if another sensor in the same batch did print, one `[attend] also: N quiet event(s) from …` line counts them.

## `context`

The sensor that watches the session itself rather than the world (ADR-113). Each poll runs `ways context --json` and reads the current token usage.

**Tier crossings.** Crossing into a tier emits once per session:

| % used | Magnitude | Gist of the message |
|---|---|---|
| 50% | 1.5 | halfway; keep going if on track, scope the next stages if barely started |
| 65% | 3.0 | agent-ways will capture todos (75%) and memory (80%) shortly |
| 83% | 4.0 | the compaction checkpoint fires at 85%; finish the current task and sync |
| 90% | 5.0 | stop or compact now; quality degrades past here, and auto-compact is off |

The line reads `context at N% — <message>`. When the burn rate is known it adds `(burning V%/min`, and below 90% also `, ~M min to critical` when that estimate is under an hour. The 90% line also names the way to read: ``Use `ways show attend context-pressure --session $CLAUDE_SESSION_ID` for reflection guidance``.

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

**Messages.** Each poll scans the project tray, `_broadcast/` and every joined channel, and emits every unseen message. Base magnitudes are 7.0 for a message sent to the project, 5.0 for a channel and 4.0 for `#open`, raised by the per-peer boost ([`engagement.md`](engagement.md#per-peer-boost)). More than 8 messages in one poll become one count line. [`delivery.md`](delivery.md) covers the line format, the cold start and the Stop-hook drain.

The whole sensor rides the message lane, so session events skip the refractory too. That is a known limitation of ADR-136, which chose lanes per sensor rather than per observation.

The sensor re-reads `_groups.yaml` on every poll, so `attend join` and `attend leave` take effect on the next scan.

## `processes`

Tracks dev-tool processes in `ps` output, not every process. A process produces start and exit events only if its name is a watched tool, is on the sensor's tracked list (build tools, runtimes, editors, servers, `ssh`, `git` and similar, in `tools/sensor-processes/src/lib.rs`), or its `comm` or its tracked name contains one of the session's focus keywords.

A process can rename itself, so the name the sensor tracks is not always `comm` as `ps` prints it. The sensor renames only the tools it knows. An alias table maps an exact `comm` to a name: Node names its main thread, so `node-MainThread` is tracked as `node`. One rule reads npm's title, which is its command line. `npm exec …` and `npm x …` are tracked as `npx`, an app that launches a package, such as an MCP server; npx is never a build tool. Any other `npm …`, such as `npm test`, `npm run dev`, `npm install` or `npm ci`, is tracked as `npm`. Every other `comm` is kept unchanged, so `ssh-agent`, `cargo-watch`, `clang-format` and `containerd-shim` are never counted as `ssh`, `cargo`, `clang` or `containerd`. A new renamer is one entry in the alias table.

A `ps` read that fails or lists no process skips that poll. The first failed read and the recovery are each logged once on stderr. After six failed reads in a row, the recovering poll sets a new baseline silently and does not report what changed during the outage. A `ps` that cannot print `lstart`, such as busybox's, is read without it from then on. Processes are then told apart by pid and name only, and the sensor says so once on stderr.

The sensor tracks application presence, not process churn (ADR-113). Four rules keep a build to one event:

- A watched tool running under another watched tool, such as `rustc` under `cargo` or `cc` under `make`, is part of that build and is not counted. A watched tool run on its own is counted. Any other app is counted wherever it runs, so `node` under `npm` and `docker` under `make` stay visible.
- A watched build tool announces no start. Its exit is always reported, as `cargo finished`. Each run is tracked by its pid and start time, so concurrent `npm test` and `npm run dev` runs each report their own exit, and a run whose pid is reused by a new run of the same tool still reports its finish. Start times match within two seconds, so a wall-clock step does not end a running tool.
- An app that exits in the same poll as the watched tool it ran under, or one poll before, is named in the tool's event: a server dying under `npm run dev` gives one event, `npm finished (node exited)`. An app that exits while its tool keeps running is reported one poll later as `node exited`. Apps are matched by name, not pid, so an unrelated app of the same name that exits in the same poll can be named in a build's finish. An exit is named in one tool's event only, the finished tool with the lowest pid.
- Any other app announces its start once it is still running at the next poll. The poll interval adapts between 5 and 30 seconds, so the start debounce window is up to 30 seconds. An app that starts and exits within it produces no event at all.

| Event | Magnitude | Example |
|---|---|---|
| app started (still running one poll later, up to 30 s) | 2.0 | `docker started` |
| app exited | 2.0 | `nvim exited` |
| watched build tool finished, no marker | 2.5 | ``cargo finished. Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps`` |
| watched build tool finished, success | 2.5 | `cargo finished (success). …` |
| watched build tool finished, failure | 3.5 | `cargo finished (failure, code 101). …` |
| watched build tool finished with a hosted app | 2.5, or 3.5 on a failure marker | `npm finished (node exited). …` |

### The watch list

The watch list picks which exits get build enrichment: the affordance line and the success or failure magnitude. The default is:

```
cargo, rustc, make, cmake, ninja,
gcc, g++, cc, c++, clang, clang++,
go, npm, yarn, pnpm, tsc,
mvn, gradle, pip, pip3
```

`sensors.processes.watch` replaces the default; it is not merged, and an empty list turns enrichment off. Every watched tool is tracked, so a name added here produces events even when it is not on the tracked list. To enrich only Rust and C builds:

```yaml
sensors:
  processes:
    watch: [cargo, rustc, make, cmake, ninja, gcc, g++]
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

The sensor does not capture output.

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
