# Sensors — the shipped set

Every sensor is one YAML file. attend embeds seven shipped files, kept in the repository under `attend/sensors/` and projected with the app to `$XDG_DATA_HOME/agent-ways/attend/sensors/`. Four are `crate` sensors, each configuring a compiled sensor: `context` and `git` are modules of attend under `tools/attend/src/sensors/` and always compiled; `peers` and `processes` are the crates `sensor-peers` and `sensor-processes`, linked behind feature flags. `example-command` and `example-mcp` ship with `enabled: false` as patterns to copy. The seventh, `ways-update`, is a `command` sensor that tells the session when agent-ways has a newer release ([`ways-update`](#ways-update)). A file of the same name in your user or project layer changes any field of a shipped one; [`authoring-sensors.md`](authoring-sensors.md) has the spec. The sensor host the attend mod starts for each listening session runs them ([`loop.md`](loop.md#the-sensor-host)); a session without the mod runs none. Keepwarm is not a sensor: the mod runs `attend keepwarm check` on its own ([`keepwarm.md`](keepwarm.md)).

`attend sensor list` shows every sensor the files declare:

```
  Sensor          Kind    Origin  State  Lane    Interval   Source                 File
  ─────────────────────────────────────────────────────────────────────────────────────
  context         crate   builtin active event   60s / 20s  attend@0.33.0          -
  example-command command builtin off    event   300s / 60s sh                     -
  example-mcp     mcp     builtin off    message 120s / 60s slack/list_unread      -
  git             crate   builtin active event   30s / 10s  attend@0.33.0          -
  peers           crate   builtin active event   30s / 10s  sensor-peers@0.6.0     -
  processes       crate   builtin active event   30s / 5s   sensor-processes@0.6.0 -
  ways-update     command builtin active event   604800s / 604800s ways               -
```

`Origin` is the lowest layer that defines the sensor: `builtin`, `user` or `project`. Each higher layer with a file for it follows, as `· user extends` or `· project extends`. `File` is the highest of those files; a shipped file shows its copy in the app dir, or `-` when the app dir has none. `Source` is the compiled sensor and its version for a crate sensor, the program for a command sensor, and the server and tool for an mcp sensor. Below the table, one line per sensor gives the reason behind a state and any note, such as a lane the author rule refused or a project file the allow-list refused. `--json` prints each sensor as an item: its origin and file, an `overrides` list with each higher layer's file, its state and reason, then its fields and the last poll's outcome.

| State | Meaning |
|---|---|
| `active` | attend polls it, and its last poll, if any, succeeded |
| `failing` | attend polls it, and its last poll failed; the line below says why |
| `off` | its file sets `enabled: false` |
| `invalid` | a problem in its merged files keeps it from running |
| `refused` | a project command or mcp file, from a project not in `attend.project_sensors`, with no lower layer |
| `degraded` | an `mcp` sensor with no probe record that holds for its server, tool and args, or a `ways` sensor, which ways runs |
| `not-compiled` | its file names a compiled sensor this build leaves out |
| `unknown` | `ways settings items` could not ask attend for the sensor's state; the detail says why |

## Summary

These are the shipped files' values.

| Sensor | Observes | Interval / min | Threshold | Decay | Lane | Requires |
|---|---|---|---|---|---|---|
| **context** | the session's own token usage | 60s / 20s | 1.5 | 3 | event | `Read` |
| **git** | the working tree and its upstream | 30s / 10s | 2.0 | 4 | event | `Bash(git:*)` |
| **peers** | other Claude Code sessions starting and leaving, and one in this project nearing compaction | 30s / 10s | 2.0 | 5 | event | `Read` |
| **processes** | build and dev-tool processes | 30s / 5s | 2.0 | 5 | event | `Bash(ps:*)` |
| **ways-update** | a newer agent-ways release on the install's channel | 7d / 7d | 1.0 | 1 | event | `Bash(ways:*)` |

Every sensor polls faster (toward its minimum interval) while it sees change and slower (toward its base interval) when quiet. Every shipped sensor that is on rides the event lane, through the action-potential refractory and the strict governor ([`engagement.md`](engagement.md)). A file of your own that declares `lane: message` skips the refractory and uses the message lane's permissive governor ([`loop.md`](loop.md#disclosure-and-the-governor)).

When a sensor discloses on the event lane, the sensor host writes its lines only if its accumulated magnitude has reached 3.0. Under that they are logged on stderr and not written ([`loop.md`](loop.md#the-sensor-host)). A written line reaches the session at its next turn and never wakes it ([`delivery.md`](delivery.md#system-signals)).

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

Discovers other Claude Code sessions from `~/.claude/sessions/*.json` and their transcripts, and reports their presence: who else is working, and where. It reads no messages; the attend mod's drain delivers those ([`delivery.md`](delivery.md)). Presence is no one's words, so its reports ride the event lane. It does not report a session turning between working and waiting. A session does that every turn, and two such reports in one window add up past the event lane's cut of 3.0, so the turn-end drain would continue the receiving session's turn to deliver them.

| Event | Magnitude |
|---|---|
| peer session started, same project / other project | 3.0 / 1.0 |
| peer session exited, same project / other project | 2.0 / 0.5 |
| same-project peer crossed 80% context | 2.0 |

## `ways-update`

A `command` sensor that runs `ways manage update --what-if --json` and tells the session once when `behind` is true. The notice reads `agent-ways has a newer release: <summary>. ` followed by a pointer to `ways manage update --what-if`. The update's own dry run decides what counts as behind: a release counts once its tag is fetched, and a checkout where a fast-forward pull would stop (`blocked`) or a result with no target (`behind: null`) stays silent. attend holds no update logic.

The trigger uses `memory: session` and `expires: session`. The host keeps its memory, so each listening session hears the notice at most once, on the first poll that finds the install behind, and the notice stays until the session reads it. Its magnitude is 3, the event lane's cut, so the host writes it. The notice reaches an idle session at its next turn and does not wake it.

To turn it off, write `$XDG_CONFIG_HOME/attend/sensors/ways-update.yaml` with one line:

```yaml
enabled: false
```

The file survives a repair or an update of the install, which rewrites only the shipped copy. `attend sensor list` then shows `ways-update` as `off`.

## `processes`

Tracks dev-tool processes in `ps` output, not every process. A process produces start and exit events only if its name is a watched tool, is on the sensor's tracked list (build tools, runtimes, editors, servers, `ssh`, `git` and similar, in `tools/sensor-processes/src/lib.rs`), or its `comm` or its tracked name contains one of the session's focus keywords.

A process can rename itself, so the name the sensor tracks is not always `comm` as `ps` prints it. The sensor renames only the tools it knows. An alias table maps an exact `comm` to a name: Node names its main thread, so `node-MainThread` is tracked as `node`. One rule reads npm's title, which is its command line. `npm exec …` and `npm x …` are tracked as `npx`, an app that launches a package, such as an MCP server; npx is never a build tool. Any other `npm …`, such as `npm test`, `npm run dev`, `npm install` or `npm ci`, is tracked as `npm`. Every other `comm` is kept unchanged, so `ssh-agent`, `cargo-watch`, `clang-format` and `containerd-shim` are never counted as `ssh`, `cargo`, `clang` or `containerd`. A new renamer is one entry in the alias table.

A `ps` read that fails or lists no process skips that poll. The first failed read and the recovery are each logged once on stderr. After six failed reads in a row, the recovering poll sets a new baseline silently and does not report what changed during the outage. The sensor reads `ps -u $USER -o pid=,ppid=,lstart=,comm=`. A `ps` that cannot print `lstart` or take `-u`, such as busybox's, fails that read. The sensor then reads `ps -o pid=,ppid=,user=,comm=` from then on, keeps the rows owned by `$USER`, and says so once on stderr. Busybox cuts the `user` column to eight characters, so a longer user name is matched by its first eight.

Apps are reported wherever they run on the machine. Build tools are reported only for the session's own builds: in the sensor host, a watched tool counts only when it runs under the session's Claude Code (the pid the attend mod passes as `--parent`, or the Claude Code found above the host). A host that finds no Claude Code reports every build the user runs, and logs that it does. A `claude -p` session started from the session's shell is a session of its own, and its builds are not reported here. A run that polls saw only in its first five seconds is brief. A brief run reports its finish unless it looks like a script running the tool in a loop: another run of the same tool shows in the same poll or the one before, or a brief run of it finished in the last two minutes. So a lone short `cargo test` reports, and the many short compiler runs of `./configure` report nothing. A brief run the build-status marker (below) names always reports. Where `ps` cannot print `lstart`, as with busybox, a run's start is unknown, no run is brief, and every finish reports. A build another session or a terminal runs is not reported to this session. A build the session started stays tracked when its shell exits and it is adopted by init, so `cargo build &` reports its finish when it ends. A watched tool above Claude Code, such as a `make` target that starts `claude`, does not hide the session's builds.

The sensor tracks application presence, not process churn (ADR-113). Four rules keep a build to one event:

- A watched tool running under another watched tool, such as `rustc` under `cargo` or `cc` under `make`, is part of that build and is not counted. A watched tool run on its own is counted. Any other app is counted wherever it runs, so `node` under `npm` and `docker` under `make` stay visible.
- A watched build tool announces no start. Its exit is always reported, as `cargo finished`. Each run is tracked by its pid and start, so concurrent `npm test` and `npm run dev` runs each report their own exit, and a run whose pid is reused by a new run of the same tool still reports its finish. On Linux the start is field 22 of `/proc/<pid>/stat`, clock ticks since boot, read for watched tools only and compared exactly; no clock step or time zone change moves it. Elsewhere the start is `lstart`, printed with `TZ=UTC0` so a time zone change does not move it, and matched within two seconds. A wall-clock step larger than two seconds there reads as a restart, and the running tool reports a finish: a known limit. Without `lstart` or `/proc`, a run is told apart by pid and name only.
- An app that exits in the same poll as the watched tool it ran under, or one poll before, is named in the tool's event: a server dying under `npm run dev` gives one event, `npm finished (node exited)`. An app that exits while its tool keeps running is reported one poll later as `node exited`. Apps are matched by name, not pid, so an unrelated app of the same name that exits in the same poll can be named in a build's finish. An exit is named in one tool's event only, the finished tool with the lowest pid.
- Any other app announces its start once it is still running at the next poll. The poll interval adapts between 5 and 30 seconds, so the start debounce window is up to 30 seconds. An app that starts and exits within it produces no event at all.

| Event | Magnitude | Example |
|---|---|---|
| app started (still running one poll later, up to 30 s) | 2.0 | `docker started` |
| app exited | 2.0 | `nvim exited` |
| watched build tool finished, no marker | 3.0 | ``cargo finished. Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps`` |
| watched build tool finished, success | 3.0 | `cargo finished (success). …` |
| watched build tool finished, failure | 3.5 | `cargo finished (failure, code 101). …` |
| watched build tool finished with a hosted app | 3.0, or 3.5 on a failure marker | `npm finished (node exited). …` |

The sensor host writes an event-lane batch only at magnitude 3.0 or more, so a lone app start or exit is logged and not delivered, and a lone build finish is delivered.

### The watch list

The watch list picks which exits get build enrichment: the affordance line and the success or failure magnitude. The default is:

```
cargo, rustc, make, cmake, ninja,
gcc, g++, cc, c++, clang, clang++,
go, npm, yarn, pnpm, tsc,
mvn, gradle, pip, pip3
```

`watch` in a `processes.yaml` of your own replaces the default, and an empty list turns enrichment off. An entry is matched against the tracked name, not the raw `comm`: `node` watches `node-MainThread`, `npm` watches `npm test` and `npm run dev`, and `npx` watches `npm exec …`. Every watched tool is tracked, so a name added here produces events even when it is not on the tracked list. To enrich only Rust and C builds, write `$XDG_CONFIG_HOME/attend/sensors/processes.yaml`, or `.claude/attend/sensors/processes.yaml` for one project:

```yaml
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

## Choosing where a new observation goes

1. **Does a shipped sensor cover it?** The `git` sensor already watches git state. A file of the same name in your user or project layer tunes it.
2. **Can a program print it?** Then write a `command` sensor from `example-command.yaml`: an argv and its output, with no Rust and no rebuild.
3. **Does a read-only MCP tool return it?** Then write an `mcp` sensor from `example-mcp.yaml`, and run `attend sensor probe <name>` before it runs.
4. **Does it need native speed, state no file can declare, or complex logic?** Then write a compiled sensor: a `sensor-*` crate in `tools/` that implements `Sensor`, an optional dependency of attend with a feature, and a shipped `crate` file that names it. See [`authoring-sensors.md`](authoring-sensors.md#compiled-sensors).

## Related

- [`authoring-sensors.md`](authoring-sensors.md) — the sensor file spec and writing new sensors
- [`first-sensor.md`](first-sensor.md) — a first sensor file, step by step
- [`engagement.md`](engagement.md) — the event lane's refractory
- [`loop.md`](loop.md) — the sensor host that runs these sensors
- [`delivery.md`](delivery.md) — how what the host writes reaches the session

## References

- **ADR-113** — attend and the first context sensor
- **ADR-117** — sensor crate extraction and feature flags
- **ADR-136** — the event and message lanes
- **ADR-403** — sensors as files under one spec
- **ADR-416** — the sensor host, the presence-only peers sensor, the update check and the `ways-update` sensor
