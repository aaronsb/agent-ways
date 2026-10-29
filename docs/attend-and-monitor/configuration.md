# Configuration

Attend reads a two-layer YAML configuration following the same overlay pattern as ways (ADR-115). User-scope config applies to every attend invocation; project-scope config layers on top to add or override specific settings for work in that project.

This page is the reference for the full config surface: where files live, what every key does, and how the overlay works. Each sensor is a YAML file of its own, layered the same way; [`authoring-sensors.md`](authoring-sensors.md) has their spec.

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
ways settings                                   # the settings screens
ways settings list attend                       # every attend key in effect, as key=value
ways settings get attend.engagement.decay_per_minute
ways settings set attend.governor.base_cooldown 30 --project .   # writes .claude/attend.yaml
ways settings help attend.governor.base_cooldown   # what a key does, its type, range and default
ways settings lint                              # findings in every settings file and sensor file; exit 3 with any
ways settings fix attend.engagement             # repair what that section's findings point at

attend config init      # write the default config to ~/.config/attend/config.yaml, if none is there
attend config show      # every attend key in effect, as key=value
attend config path      # the file paths attend loads from
attend config lint      # the findings in attend's two files; exit 3 with any
```

Every write goes through the one settings writer: it takes a lock beside the file, changes only the keys it sets, keeps every comment and the order of the rest, and renames a temporary file into place. `attend config tune --apply` and `ways settings set` leave the same bytes.

A value of the wrong type or out of range is a finding, never clamped. A section with a finding is ignored in that file, so its keys resolve from the layers beneath, ending at the defaults. A switch fails closed: a `cleanup` whose `enabled` holds anything but `true` in a section with a finding reads as off. A file that does not parse sets nothing; `cleanup` is off in its scope until its syntax is fixed by hand. Each finding is one line on stderr of `attend run` and `attend config show`, naming the file, line and section.

`attend config init` creates the user-scope file with fully commented defaults, and never overwrites one that exists.

## Complete schema

```yaml
# Disclosure governor — global rate limiting across all sensors
governor:
  base_cooldown: 15          # seconds between any two disclosures
  max_per_window: 3          # max disclosures in rate_window
  rate_window: 120           # seconds of the rolling rate window

# Action potential engagement model (ADR-123)
# Run `attend config tune` to auto-derive these from real session history
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

# Projects whose own command and mcp sensor files run (user file only)
project_sensors: []
```

The `governor` block governs the event lane. The message lane (`peers`, `keepwarm`) uses a fixed permissive governor with no keys (see [`delivery.md`](delivery.md#the-monitor-line)).

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

**Auto-tuning.** Run `attend config tune` to derive these from real session history. See [`engagement.md`](engagement.md#tuning-with-attend-tune) for how tuning works and how the linear-derived `decay_per_minute` relates to the engine's exponential half-life.

### `cleanup`

Background signal-file cleanup. Prevents the signals base from accumulating indefinitely. Scoped strictly to `~/.cache/attend/signals/`; never touches ways data or anything else.

Reaping is by **project liveness** (ADR-136), not by age: a signal is removed when the project that owns it is gone, mirroring Claude Code's notion of a live project. A project-tray signal belongs to the recipient's project; a `#open` or channel signal belongs to the sender's. There is no age cutoff and no `retention` dial. See [`signals.md`](signals.md#the-full-lifecycle).

- **`enabled`** (bool, default true): master switch. If false, auto-cleanup is skipped and you must run `attend message cleanup` manually to reclaim space.
- **`interval`** (seconds, default 600): how often the auto-sweep runs inside `attend run`. At this interval the loop scans the signals base and reaps signals whose owning project is no longer live.

The sweep also removes empty encoded-cwd project subdirectories left as shells after their signals are reaped. Reserved names (`_broadcast`, `@groups`, anything starting with `_` or `@`) are never removed.

`attend message cleanup` can be run manually:

```bash
attend message cleanup                        # reap signals of dead projects
attend message cleanup --dry-run              # list what would be removed
attend message cleanup --all                  # nuke everything (ignore liveness)
```

### `project_sensors`

The projects whose own `command` and `mcp` sensor files attend runs (ADR-403). A list of project directories; entries expand `~`, `$HOME` and `$XDG_*`. Only the user file sets it. A project's `.claude/attend/sensors/` file of either kind is `refused` until its project is listed; [`authoring-sensors.md`](authoring-sensors.md#project-files-and-the-allow-list) says what runs meanwhile.

```yaml
project_sensors:
  - ~/Projects/example
```

### Sensors

Each sensor is a file under `attend/sensors/` in the user and project layers, not a section of these files. The `sensors:` section these files once held, its `+name:` and `-name:` prefixes and its inline `script:` sensors are retired with no reader. `ways settings lint`, `attend config lint` and `attend run` report each of their keys with the sensor file to write and the field to set there ([`authoring-sensors.md`](authoring-sensors.md#the-retired-forms)).

## Overlay semantics

The overlay layers project-scope on top of user-scope, key by key: a project value replaces the user value for that key, and every key the project leaves out keeps the user's. `project_sensors` is read from the user file alone.

Example. User-scope:

```yaml
governor:
  base_cooldown: 15
  max_per_window: 3
```

Project-scope at `<repo>/.claude/attend.yaml`:

```yaml
governor:
  max_per_window: 1      # at most one event-lane disclosure per window in this repo
```

Merged result in that project: `base_cooldown` 15 (from user), `max_per_window` 1 (from project), `rate_window` 120 (the default).

## Permissions (ADR-116)

A sensor file's `requires:` list declares the harness permissions the sensor needs. `attend sensor permissions` checks each one against `~/.claude/settings.json`:

```bash
$ attend sensor permissions

  Attend Permissions Audit (ADR-116)

  Sensor          Requires       Status
  ──────────────────────────────────────
  context         Read           granted
  disclosure      Bash(ways:*)   granted
  git             Bash(git:*)    granted
  keepwarm        Bash(ways:*)   granted
  peers           Read           granted
  processes       Bash(ps:*)     granted

  All permissions granted.
```

A missing grant prints as `MISSING`, with the line to add to `settings.json`.

## The schema

The files are YAML, read by `serde_yaml` and checked against attend's schema in the `attend-config` crate. Flow and block lists read the same (`project_sensors: [~/a, ~/b]`, or one `- item` per line). Anchors, block scalars and any other YAML work; a key the schema does not name is a finding.

Removed with the hand-written parser (ADR-506; the release notes say how to move a file by hand):

- the `x-` escape hatch for keys of your own: any key the schema does not name is a finding;
- `attend config lint --fix` and `--check`: `attend config lint` exits 3 with findings, and `ways settings fix <section>` repairs them;
- the startup refusal of `burst_window`: it is an unknown key like any other.

## The attend mod's settings

The attend mod's settings live in the ways config, not in these files ([`mod.md`](mod.md#configuration)). [ADR-512](../architecture/platform/ADR-512-each-shipped-claude-code-mod-has-one-wanted-state-in-settings-under-mods-name-and-ways-manage-converges-every-target-to-it.md) renamed their section from `attend_mod` to `mods.attend`, under the same key names. Nothing reads `attend_mod`, so until a file is moved its values are not read: a project that set `listening: active` starts its sessions dormant, and each other value reads as its default.

`ways settings lint` names each file that holds it, with the command that moves it, and changes none of them. So do `ways manage update --what-if` and `ways manage update --commit` once this release is installed. The update that installs this release runs the previous release's updater, which does not know the section is retired and reports neither. Run `ways settings lint` after that update to see every file:

```bash
ways settings fix mods.attend                      # the user config
ways settings fix mods.attend --target <dir>       # a target's own config
ways settings fix mods.attend --project <dir>      # a project's .claude/ways.yaml
```

Where the file has no `mods` section, fix renames the block in place: `attend_mod:` becomes `mods:` and `attend:`, and every line under it, comments included, moves with its value. Where `mods` exists, each value is written under `mods.attend` one by one and the old block is removed with the comments inside it, which fix says. A value the file already holds under `mods.attend` stays, and so does a value of another shape where a section was expected; fix names each old value it dropped and why. Comments outside the block stay.

## Related

- **ADR-115** — declarative config with project-scope overlay (the pattern this implements)
- **ADR-116** — permission requirements
- **ADR-117** — sensor crate extraction (feature flags for compile-time sensor selection)
- **ADR-123** — action potential engagement (the `engagement` block)
- [`engagement.md`](engagement.md) — engagement model in depth
- **ADR-403** — sensors as files under one spec
- [`authoring-sensors.md`](authoring-sensors.md) — the sensor file spec, its layers and the project allow-list
- [`sensors.md`](sensors.md) — the shipped sensors and their values
