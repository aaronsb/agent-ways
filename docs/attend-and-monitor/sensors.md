# Sensors — the built-in set

Attend ships with four built-in sensors, each compiled in as a separate Rust crate and wired into the orchestrator via a feature flag. They form the baseline observation surface of "what's happening outside the conversation" — context state, git state, peer sessions, and process activity. Everything beyond these four is a user-authored sensor, either another crate or an external script (see [`authoring-sensors.md`](authoring-sensors.md)).

This page covers what each built-in observes, what magnitudes it emits, and what kinds of events a reader will see in notifications.

## Summary

| Sensor | Observes | Base interval | Min interval | Threshold | Emits |
|---|---|---|---|---|---|
| **context** | Claude's own token usage | 60s | 20s | 1.5 | tier crossings, velocity spikes |
| **git** | Working tree git state | 30s | 10s | 2.0 | branch changes, new commits, dirty file deltas |
| **peers** | Other Claude sessions + signal files | 30s | 10s | 2.0 | peer status changes, peer messages |
| **processes** | Build/dev-tool processes | 30s | 5s | 2.0 | process start/exit (cargo, npm, make, etc.) |

All four use the adaptive interval scheme — they poll fast (`min_interval`) during active change and slow (`base_interval`) when quiet. All four participate in the action potential engagement model (ADR-119) with the shared global config.

## `sensor-context` — interoceptive

The canonical first sensor from ADR-113 — the one that prevents Claude from silently running off the context cliff. It's called "interoceptive" because it's the one sensor that watches *Claude itself* rather than the external world. The data source is `ways context --json`, which reads the running session's current token usage.

### What it emits

**Tier crossings.** Context percentage is bucketed into tiers; crossing into a new tier fires an observation. Each tier has an increasing magnitude:

| Tier | % used | Magnitude | Label |
|---|---|---|---|
| low | 40% | 1.5 | approaching midpoint — plan wrap-up scope |
| mid | 50% | 2.0 | midpoint — wrap-up window opening |
| high | 65% | 3.0 | ways will fire todos checkpoint at 75% |
| alert | 85% | 4.0 | ways fired memory save at 80% — verify it happened |
| critical | 92% | 5.0 | compaction checkpoint at 95% — finish current task |

Each tier is disclosed once per session — once you've crossed 65%, you won't see the 65% tier message again even if you bounce around that value. The emit includes the current percentage and a projection to 95% based on recent velocity.

**Velocity spikes.** If context burn rate exceeds 2%/min and the percentage change is significant (>5%), a magnitude 2.0 observation fires separately: "context velocity spike: X% in last N min (V%/min)." This catches the case where you're burning through context unexpectedly fast — useful for detecting runaway tool loops or overly verbose file reads.

**Affordance strings.** High-tier observations (alert and critical) append `Use \`ways show attend context-pressure --session $CLAUDE_SESSION_ID\` for reflection guidance` to the message. This points the agent at the context-pressure way for structured next-step decisions.

### State the sensor carries

- Last-disclosed tier per session (so tiers fire once, not repeatedly)
- Prior snapshot (token count, percentage, wall-clock) for velocity computation

### What it doesn't do

Doesn't enforce anything. Doesn't compact for you. Doesn't save state. It only surfaces observations — the response is up to the agent and to the ways layer that fires at specific thresholds.

## `sensor-git` — working tree state

Watches git state in the current working directory. Reports application-level deltas (branch changed, N new commits, M new dirty files) rather than file-by-file churn. The underlying data comes from `git` shell-outs (with `GIT_OPTIONAL_LOCKS=0` to avoid races against foreground commits).

### What it emits

| Event | Magnitude | Example |
|---|---|---|
| branch changed | 3.0 | `branch changed: main → feat/new-thing` |
| new commits on current branch | 2.0 | `new commits on feat/attend: HEAD abc123 → def456` |
| new dirty files | 2.0 (burst scales) | `3 new dirty files: src/main.rs, src/config.rs, Cargo.toml` |
| working tree clean | 1.0 | `working tree clean (changes committed or stashed)` |
| new upstream commits | 2.0 | `4 new commits on upstream (now 4 behind)` |
| unpushed local commits | 1.0 | `2 commits ahead of upstream (unpushed)` |

Branch changes are the loudest event (3.0) because they're usually the signal of "I just switched contexts." New commits on the current branch (2.0) fire when something lands that you care about. Clean state (1.0) is soft — it aggregates with other background events rather than firing on its own.

### State the sensor carries

- Previous snapshot: branch name, HEAD SHA, dirty file set, ahead/behind counts

### What it doesn't do

Doesn't watch git hooks, doesn't parse commit messages, doesn't look at tags. It's strictly about working tree state and upstream divergence.

## `sensor-peers` — other sessions and signal files

Discovers other Claude Code sessions and reads peer signal files from `~/.cache/attend/signals/`. This is the sensor that makes multi-agent coordination possible — it reads messages sent via `attend send` from other sessions.

### What it observes

**Peer sessions.** Walks `~/.claude/sessions/*.json` to find other running Claude sessions. For each, tracks cwd, PID, project name, context percentage, model, and working/waiting status. Emits when peers appear, disappear, or change state.

**Signal files.** Scans the sender's own project directory, `_broadcast`, and every focus group the session has joined. New `.signal` files (not in the seen-set) are parsed and emitted as peer message events.

### What it emits

| Event | Magnitude | Notes |
|---|---|---|
| peer session appeared | 2.0 | new Claude session detected |
| peer session disappeared | 1.5 | session exited |
| peer status change | 1.5 | working → waiting, etc. |
| peer message | base × peer boost | see below |

**Per-peer engagement boost.** Peer messages don't have a fixed magnitude — they're amplified based on how much back-and-forth the recipient has had with that specific peer:

- First message from a peer in the activity window (default 15 min): × 1.0
- Second message: × 1.75
- Third and beyond: × 2.5

This creates emergent auto-grouping: active conversation partners climb above the refractory threshold, while uninvolved broadcast noise stays at baseline and gets suppressed. See [`engagement.md`](engagement.md) for the full model.

### State the sensor carries

- Prior snapshot of all known peer sessions
- Set of already-seen signal filenames (keyed by directory + filename)
- Per-peer engagement history (sliding window of recent message timestamps)
- Reply-hint-shown flag (once per session, not per message)
- Own session ID (to skip self-signals)

### Focus group provider

As of the awareness-stabilization bundle (issue #15), the list of focus-group directories to scan is refreshed on every poll via a closure provider, not snapshotted at startup. This means mid-session `attend focus on <name>` takes effect immediately without restarting the sensor loop.

## `sensor-processes` — build and dev tools

Tracks specific dev-tool processes running under the user's session. Not a general process monitor — it's scoped to a whitelist of compilers, build tools, and package managers, because those are the processes whose lifecycle matters to a coding session.

### Watched processes

```
cargo, rustc, make, cmake, ninja,
gcc, g++, cc, c++, clang, clang++,
go, npm, yarn, pnpm, tsc,
mvn, gradle, pip, pip3
```

A future extension could make this list configurable; today it's hardcoded.

### What it emits

| Event | Magnitude | Example |
|---|---|---|
| process started | 2.0 | `cargo started` |
| process exited (non-build) | 2.0 | `nvim exited` |
| build exited, no marker | 2.5 | `cargo exited. Use \`ways show attend build-complete --session $CLAUDE_SESSION_ID\` for next steps` |
| build exited (success) | 2.5 | `cargo exited (success). …` |
| build exited (failure, code N) | 3.5 | `cargo exited (failure, code 101). …` |

Exit events on build tools include an affordance string pointing at the build-complete way. Failures get a louder magnitude (3.5) so they break through refractory gating — success is quieter (2.5) because most successful builds don't need the agent's attention.

### Opt-in: exit-code awareness via a build-status marker

The sensor observes `ps` diffs, so by the time it notices an exit the process is already gone — it can't read the exit code directly. To enrich build exits with success/failure context, wrap your build command so it writes a single-line marker file when it finishes:

```
$XDG_STATE_HOME/attend/last-build-status    # or ~/.local/state/attend/last-build-status
```

Format: `cmd|exit_code|unix_ts`, e.g. `cargo|101|1712983456`. A minimal shell wrapper:

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

The sensor reads this file on each poll. When it detects an exit for a build tool *and* the marker's `cmd` matches *and* the marker timestamp is within 60 s, it enriches the event. Otherwise it falls back to the legacy "X exited" text — there's no penalty for skipping the wrapper.

**Known limits of v1.**

- **Single-slot, global marker.** There's one marker file for the whole machine. Two concurrent `cargo build` invocations in different directories will last-writer-wins, and a quick `cargo --version` that happens inside the 60 s window can mask a real build's failure. For single-user, single-project sessions the aliasing is rare; for parallel builds across projects you'll want a smarter wrapper that keys the marker filename on `$PWD` or `$CLAUDE_SESSION_ID`. Widening the marker to a per-session slot is tracked as a follow-up.
- **Non-atomic write.** The wrapper above uses `> "$dir/last-build-status"`. For a ~30-byte payload on local ext4/xfs this is effectively atomic (one `write()` syscall, well under a page), but on NFS or if the wrapper is killed mid-write the reader could see a truncated line. `parse_marker` treats malformed input as "no fresh marker" (falls back to legacy text), so the actual failure mode is bounded — but if you're on a networked filesystem, prefer an atomic `printf … > "$tmp" && mv "$tmp" "$dst"`.

### State the sensor carries

- Previous snapshot: map of app name → instance count
- Most recently parsed build-status marker (if any) and its mtime

### What it doesn't do

Doesn't capture stdout/stderr from the watched processes. Without the build-status marker, it also doesn't know exit codes — plain `ps` diffs can't see a return status after the fact.

Also doesn't batch multiple events from the same build tool. If `cargo` starts, then `rustc` starts as a child, then `rustc` exits, then `cargo` exits, that's four events. Smoothing build lifecycles into "build started → build finished" aggregate events is a known todo (see issue #2 — "build event batching").

## Picking the right sensor for a new observation

If you want attend to notice *something new*, ask in order:

1. **Does an existing built-in cover it?** If you want to know when git state changes, `sensor-git` already does. Don't duplicate.
2. **Can it be done with an external script?** Anything you can observe with a shell command is a candidate for an external sensor. This is usually the right choice — no recompile, no Rust required, fast iteration. See [`authoring-sensors.md`](authoring-sensors.md).
3. **Does it need native performance, shared state, or complex logic?** If yes, write a new crate sensor. Create a new `sensor-*` crate in `tools/`, implement `Sensor`, add it to `attend`'s Cargo.toml as an optional dep + feature, register it in `sensors/mod.rs`.

Almost everything falls into category 2. Crate sensors are for the core observation surfaces that justify being in the binary — the current four. User-level observations (GitHub Project boards, Slack activity, custom event logs, etc.) are external sensors.

## Related

- [`authoring-sensors.md`](authoring-sensors.md) — how to write new sensors
- [`engagement.md`](engagement.md) — the action potential model the sensors play into
- [`signals.md`](signals.md) — the signal file format `sensor-peers` reads and writes
- [`loop.md`](loop.md) — the sensor loop substrate
- **ADR-113** — the original design of attend, including the first context sensor
- **ADR-117** — sensor crate extraction and feature flags
- **ADR-118** — focus groups (used by `sensor-peers`)
- **ADR-119** — action potential engagement model
