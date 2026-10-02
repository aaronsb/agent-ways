# Configuration

Attend reads a two-layer YAML configuration following the same overlay pattern as ways (ADR-115). User-scope config applies to every attend invocation; project-scope config layers on top to add or override specific settings for work in that project.

This page is the reference for the full config surface: where files live, what every key does, and how the overlay works.

## File locations

```
~/.config/attend/config.yaml                    # user scope — always loaded
<cwd>/.claude/attend.yaml                       # project scope — layered on top
```

Both files are optional. Attend ships with sensible defaults in code, so running with no config at all works. User-scope is for your persistent defaults; project-scope is for per-repo overrides.

The user-scope path respects `XDG_CONFIG_HOME` if set; otherwise it falls back to `$HOME/.config/`.

## Reading and changing it

The keys are described by one schema, `attend-config`, which `ways settings` composes with ways' own (ADR-503). Every key is named `attend.<section>.<key>`:

```bash
ways settings                                   # the settings screens: attend's tabs are `attend` and `sensors`
ways settings list attend                       # every attend key in effect, as key=value
ways settings get attend.engagement.decay_per_minute
ways settings set attend.sensors.git.interval 60 --project .   # writes .claude/attend.yaml
ways settings help attend.sensors.*.script      # what a key does, its type, range and default
ways settings lint                              # findings in every settings file; exit 3 with any
ways settings fix attend.engagement             # repair what that section's findings point at

attend config init      # write the default config to ~/.config/attend/config.yaml, if none is there
attend config show      # every attend key in effect, as key=value
attend config path      # the file paths attend loads from
attend config lint      # the findings in attend's two files; exit 3 with any
```

Every write goes through the one settings writer: it takes a lock beside the file, changes only the keys it sets, keeps every comment and the order of the rest, and renames a temporary file into place. `attend tune --apply` and `ways settings set` leave the same bytes.

A value of the wrong type or out of range is a finding, never clamped. A section with a finding is ignored in that file, so its keys resolve from the layers beneath, ending at the defaults; each sensor falls back alone. A switch fails closed: a sensor or `cleanup` whose `enabled` holds anything but `true` in an entry with a finding reads as off. A sensor name the schema refuses (such as the retired `-processes:`) closes that file's `sensors:`: every built-in and every sensor the file names reads off until the name is edited by hand, and `ways settings fix` will not guess at it. A file that does not parse sets nothing; `cleanup` and every built-in sensor are off in its scope until its syntax is fixed by hand. Each finding is one line on stderr of `attend run` and `attend config show`, naming the file, line and section.

`attend config init` creates the user-scope file with fully commented defaults, and never overwrites one that exists.

## Complete schema

```yaml
# Disclosure governor — global rate limiting across all sensors
governor:
  base_cooldown: 15          # seconds between any two disclosures
  max_per_window: 3          # max disclosures in rate_window
  rate_window: 120           # seconds of the rolling rate window

# Action potential engagement model (ADR-123)
# Run `attend tune` to auto-derive these from real session history
engagement:
  burst_threshold: 3         # disclosures before refractory kicks in
  step_multiplier: 1.25      # per-disclosure threshold elevation past burst_threshold
  absolute_refractory: 60    # seconds of complete suppression after burst
  decay_per_minute: 0.1      # rate at which elevated threshold returns to baseline
  peer_activity_window: 900  # sliding window for per-peer engagement boost

# Background cleanup of the signals base
cleanup:
  enabled: true              # master switch
  interval: 600              # seconds between auto-sweeps (10 minutes)

# Per-sensor configuration — applies to built-ins and script sensors
sensors:
  context:
    interval: 60             # base polling interval in seconds
    min_interval: 20         # fastest polling interval
    threshold: 1.5           # emission threshold (accumulator must exceed)
    decay_threshold: 3       # quiet polls before interval decays back
    requires:                # permission audit (ADR-116)
      - Read
  git:
    interval: 30
    min_interval: 10
    threshold: 2.0
    decay_threshold: 4
    requires:
      - Bash(git:*)
  peers:
    interval: 30
    min_interval: 10
    threshold: 2.0
    decay_threshold: 5
    requires:
      - Read
  processes:
    interval: 30
    min_interval: 5
    threshold: 2.0
    decay_threshold: 5
    requires:
      - Bash(ps:*)
```

## Section reference

### `governor`

Global rate limiting for disclosures. Even if every sensor is ready to fire, the governor caps how many actually reach the conversation.

- **`base_cooldown`** (seconds, default 15): minimum time between any two consecutive disclosures. A burst of sensors all ready at the same time will have their disclosures serialized with at least this gap.
- **`max_per_window`** (count, default 3): maximum disclosures allowed within the rolling `rate_window`. Additional ready sensors are held; their magnitudes stay in the accumulator. 0 holds every disclosure, which mutes attend without stopping it.
- **`rate_window`** (seconds, default 120): length of the rolling window for `max_per_window`.

With defaults: at most 3 disclosures per 2 minutes, with at least 15 seconds between each.

### `engagement`

The action potential model parameters. Governs per-sensor refractory behavior. See [`engagement.md`](engagement.md) for the full model and the ADR-123 reframing; the short version is:

- **`burst_threshold`** (count, default 3): number of recent fires that count toward triggering refractory. After the threshold is hit, the sensor enters the elevated-threshold state.
- **`step_multiplier`** (float, default 1.25): contributes to the peak refractory multiplier as `peak_multiplier = 1.0 + step_multiplier`. At the default, the peak multiplier is 2.25 — the effective threshold just after a burst is 2.25× the base threshold.
- **`absolute_refractory`** (seconds, default 60): complete suppression after a burst. No events fire during this window regardless of magnitude. Directly mapped to `Curve::ActionPotential::absolute_refractory` in ticks (= seconds for attend).
- **`decay_per_minute`** (float, default 0.1): exponential decay rate for the relative-refractory multiplier. At load time attend converts this to `multiplier_half_life = ln(0.5) / ln(1 - decay_per_minute) × 60` seconds. At `0.1`, the half-life is ≈ 395 s (~6.6 min); at `0.0256` (typical tune output), ≈ 1611 s (~27 min).
- **`peer_activity_window`** (seconds, default 900): sliding window used by `sensor-peers` for the per-peer engagement boost. This *is* still tick-windowed — sensor-peers implements its own count-in-window logic rather than going through the shared curve engine, because the per-peer boost is a different shape than per-sensor refractory.

> **`burst_window`.** Pre-ADR-123 configs carried a `burst_window` key. It is an unknown key now: the `engagement` section it sits in falls back to the layers beneath, and `ways settings fix attend.engagement` removes it.

**Yaml field stability.** The yaml keys are deliberately preserved from pre-ADR-123 attend configs, so existing tuned configs keep loading without changes. Internally the keys are translated to the `Curve::ActionPotential` parameters attend actually runs on. The doc-level mapping is:

| yaml key              | runtime parameter                                  |
|-----------------------|----------------------------------------------------|
| `burst_threshold`     | `burst_threshold`                                  |
| `step_multiplier`     | `peak_multiplier = 1.0 + step_multiplier`          |
| `absolute_refractory` | `absolute_refractory` (seconds)                    |
| `decay_per_minute`    | `multiplier_half_life = ln(0.5)/ln(1-rate) × 60`   |

**Auto-tuning.** Run `attend tune` to derive these from real session history. See [`engagement.md`](engagement.md#tuning-with-attend-tune) for how tuning works and how the linear-derived `decay_per_minute` relates to the engine's exponential half-life.

### `cleanup`

Background signal-file cleanup. Prevents the signals base from accumulating indefinitely. Scoped strictly to `~/.cache/attend/signals/`; never touches ways data or anything else.

Reaping is by **project liveness** (ADR-136), not by age: a signal is removed when its owning project is gone, mirroring Claude Code's notion of a live project. Durable messages therefore wait as long as their recipient project is alive — there is no age cutoff and no `retention` dial.

- **`enabled`** (bool, default true): master switch. If false, auto-cleanup is skipped and you must run `attend cleanup` manually to reclaim space.
- **`interval`** (seconds, default 600): how often the auto-sweep runs inside `attend run`. At this interval the loop scans the signals base and reaps signals whose owning project is no longer live.

The sweep also removes empty encoded-cwd project subdirectories left as shells after their signals are reaped. Reserved names (`_broadcast`, `@groups`, anything starting with `_` or `@`) are never removed.

`attend cleanup` can be run manually:

```bash
attend cleanup                        # reap signals of dead projects
attend cleanup --dry-run              # list what would be removed
attend cleanup --all                  # nuke everything (ignore liveness)
```

### `sensors`

Per-sensor configuration. Each built-in sensor can have its intervals, threshold, decay, and permissions overridden. A sensor of your own is declared in the same block: any name with a `script`. A sensor's name is letters, digits, `-` and `_`.

**Existing built-in override:**

```yaml
sensors:
  git:
    interval: 60         # poll less often in this project
    threshold: 3.0       # raise the bar
```

**Disable a built-in:**

```yaml
sensors:
  processes:
    enabled: false
```

**Add a new script sensor:**

```yaml
sensors:
  github-project:
    script: $XDG_DATA_HOME/attend/sensors/github-project.sh
    interval: 300
    min_interval: 60
    threshold: 2.5
    decay_threshold: 3
    requires:
      - Bash(gh:*)
```

The `script` key makes it a sensor of your own. Script paths are deliberately unconstrained — they can be:

- **User-global**, under `$XDG_DATA_HOME/attend/sensors/` (the convention this config documents by default). Survives across projects; lives in your own trusted script dir.
- **Project-scoped**, at `.claude/sensors/name.sh` in a specific repo. Only loads when attend runs from that project.
- **Absolute paths** to anywhere on disk — your personal tools repo, a team-shared scripts dir, `~/bin`, wherever you keep trusted executables.

Attend only cares that the path resolves and that the script respects the subprocess contract. The `$HOME`, `~`, and `$XDG_*` prefixes are expanded when the config loads, so `$XDG_DATA_HOME/...` in config becomes an absolute path at load time. This keeps configs portable across machines.

**The shipped example.** Attend ships one external sensor at `tools/attend/examples/xdg-downloads.sh` in the agent-ways repo as a reference implementation. The default user-scope config declares it as `xdg-downloads:` with `enabled: false`. To actually run it you copy the script to a trusted location you control (the comment in the default config walks through `$XDG_DATA_HOME/attend/sensors/` as the XDG-convention choice), review it, and flip `enabled: true`. The "copy to a trusted path, review, then enable" workflow is intentional — external sensors run arbitrary shell under your user, and you should always audit a sensor's code before letting it run.

### Per-sensor keys

All sensors (built-in or script) accept:

- **`interval`** (seconds): base polling interval when quiet
- **`min_interval`** (seconds): fastest polling interval during active change
- **`threshold`** (float): emission threshold — accumulator must exceed this
- **`decay_threshold`** (count): number of quiet polls before interval decays back to base
- **`enabled`** (bool): set false to disable without removing the entry
- **`script`** (path, script sensors only): path to the executable
- **`requires`** (list): permission strings audited against `settings.json` (ADR-116)

Some sensors accept additional sensor-specific keys:

- **`processes.watch`** (list): overrides the default build-tool watch list. Only processes on this list get exit-code enrichment (success/failure magnitudes, marker correlation); everything else produces a plain `X exited`. Explicit-replace — passing `watch:` drops the defaults. See [`sensors.md`](sensors.md#watched-processes) for the default list and format examples.

## Overlay semantics

The overlay layers project-scope on top of user-scope. For each setting:

- **Scalar values** (numbers, strings, bools): project-scope replaces user-scope entirely
- **Sensor blocks**: merge on a per-key basis — a project can override just one sensor's interval without touching the others
- **Sensors of your own** (a name with `script:`): union — the project can add script sensors the user-scope doesn't know about
- **Sensor disables** (`enabled: false`): the sensor is off for this project only

Example. User-scope:

```yaml
sensors:
  git:
    interval: 30
    threshold: 2.0
```

Project-scope at `<repo>/.claude/attend.yaml`:

```yaml
sensors:
  git:
    interval: 60         # slow down git polling in this repo only
  processes:
    enabled: false       # don't run the process sensor here
  build-watcher:
    script: .claude/sensors/build-watcher.sh
    interval: 20
```

Merged result in that project:

- `git`: interval 60 (from project), threshold 2.0 (inherited from user)
- `processes`: disabled
- `context`, `peers`: unchanged defaults
- `build-watcher`: active per project-scope declaration

## Permissions (ADR-116)

The `requires:` list on each sensor block declares the harness permissions that sensor needs. Running `attend permissions audit` walks your config and checks each declared requirement against `settings.json`:

```bash
$ attend permissions audit
Attend Permissions Audit
────────────────────────
  context        Read               ✓ granted
  git            Bash(git:*)        ✓ granted
  peers          Read               ✓ granted
  processes      Bash(ps:*)         ✓ granted
  build-watcher  Bash(cargo:*)      ✗ MISSING
```

Use this to confirm your config will actually work before launching attend — a sensor that requires a permission you haven't granted will silently emit nothing.

## The schema

The files are YAML, read by `serde_yaml` and checked against attend's schema in the `attend-config` crate. Flow and block lists read the same (`requires: [Bash(gh:*), Read]`, or one `- item` per line). Anchors, block scalars and any other YAML work; a key the schema does not name is a finding.

Removed with the hand-written parser (ADR-506; the release notes say how to move a file by hand):

- the `+name:` and `-name:` sensor prefixes: write `name:` with a `script`, and `enabled: false`;
- the `x-` escape hatch for keys of your own: any key the schema does not name is a finding;
- `attend config lint --fix` and `--check`: `attend config lint` exits 3 with findings, and `ways settings fix <section>` repairs them;
- the startup refusal of `burst_window`: it is an unknown key like any other.

## Related

- **ADR-115** — declarative config with project-scope overlay (the pattern this implements)
- **ADR-116** — permission requirements
- **ADR-117** — sensor crate extraction (feature flags for compile-time sensor selection)
- **ADR-123** — action potential engagement (the `engagement` block)
- [`engagement.md`](engagement.md) — engagement model in depth
- [`authoring-sensors.md`](authoring-sensors.md) — how to declare and write new sensors
- [`sensors.md`](sensors.md) — the built-in sensors and their default values
