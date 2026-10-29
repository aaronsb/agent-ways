# Authoring sensors

A sensor translates raw environmental change into *magnitude-weighted observations* that feed attend's engagement model (ADR-123) and disclosure governor. A well-designed sensor encodes how much each kind of change matters in the magnitude, and lets the loop decide when to fire, how often, and whether to suppress. This is the discipline alarm management (ISA-18.2) brings to control rooms, applied to agent notifications: a channel stays trustworthy only as long as its loudest tier is reserved for what is genuinely loud.

Every sensor is one YAML file under one versioned spec (ADR-403). This page is the spec and the reference for writing a file. [`first-sensor.md`](first-sensor.md) walks through a first one. The shipped [`example-command.yaml`](../../attend/sensors/example-command.yaml) and [`example-mcp.yaml`](../../attend/sensors/example-mcp.yaml) comment every field of their kind; copy one to start a new sensor.

## The sensor-layer contract

Sensors in attend refuse to editorialize. Each sensor reports a **fact the framework can verify** rather than a guess the sensor produced. The token ledger is authoritative; the signal file is authoritative; the working tree is authoritative. The sensor surfaces state transitions; the consuming agent does whatever synthesis it wants on top.

The discipline was crystallized during a live peer-agent validation of the disclosure sensor (then the `sensor-disclosure` crate), when a second Claude instance observing the same mechanism wrote:

> "The discipline is refusing to editorialize at the sensor layer and letting the consuming agent do whatever synthesis it wants on top. Word-boundary chunking preserves that contract across the transport boundary, which is the only place prose fidelity actually matters."
>
> — *claude//home/aaron/temp, during sensor-disclosure validation, 2026-04-13*

When you build a sensor, emit measurable transitions. If a description is about to contain a verb like "should," "probably," or "likely," reframe it as observation. The sensor does not decide whether the state is good, bad, urgent, or ignorable; the consuming agent does. Magnitudes, thresholds, engagement states and the governor all assume honest input; a sensor that editorializes leaves them compensating for it.

## What runs today

| Kind | Host | The poll | In this release |
|---|---|---|---|
| `crate` | attend | compiled code | hosted: polled under the file's name, cadence, threshold and lane |
| `command` | attend | an argv run directly, with no shell | parsed, checked, listed and linted; `not hosted` until ADR-403 slice 2 |
| `mcp` | attend | one read-only tool call on a stdio MCP server | parsed, checked, listed and linted; `not hosted` until slice 3 |
| `ways` | ways (ADR-199) | the session's hook stream | attend lists it as `not hosted` and never runs it |

Triggers, declared state, `where` conditions, JSONPath and templates parse and are checked when a file loads. attend evaluates them, and writes declared state to the store, from slice 2. `attend sensor probe` arrives with slice 3. `ways settings` reaches the shared fields of every sensor file ([Settings keys](#settings-keys)); write the other fields in the files directly, and `ways settings lint` checks them.

## What sensor authors are designing around

Every observation a sensor emits goes through the same pipeline:

1. **Accumulator.** Each `(magnitude, description)` pair is added to the sensor's `DeltaAccumulator`. Magnitudes accumulate across ticks until the sensor is drained or decays.
2. **Emission threshold.** The accumulator has to exceed the file's `threshold` before the sensor is a candidate to disclose. Low-magnitude events accumulate silently until several add up; a single high-magnitude event may cross it on its own.
3. **Engagement / refractory (ADR-123).** After the sensor fires, its effective threshold is temporarily raised (relative refractory) or it is fully suppressed (absolute refractory, 60 s by default). New events still accumulate, and only a magnitude high enough to clear the raised threshold gets through.
4. **Disclosure governor.** A global governor rate-limits disclosures across the loop (default: 3 per 120 s, at least 15 s apart). When several sensors are ready at once, some are held until the window rolls.
5. **Monitor delivery.** What survives, at medium or high priority (accumulated magnitude 3.0 or more), prints as one stdout line per event and reaches the conversation through Monitor. Lower priority goes to stderr.

A sensor rides this event lane unless its file declares the message lane, which skips step 3 and has a permissive governor in step 4 ([Lanes](#lanes), [`delivery.md`](delivery.md)).

*Magnitude is the author's main lever.* A sensor that emits 5.0 on every tick floods the governor; one that emits 0.1 for everything never fires. Pick a few tiers: background changes at 0.5–1.0, routine notable events at 2.0–3.0, and 5.0 or more for what should break through refractory. With a threshold of 2.5, an issue assigned to you that moves on a project board might emit 3.0 and disclose on its own, while a move on someone else's issue emits 0.8 and accumulates: one is noise, four in a polling window add up to 3.2 and disclose together.

## Where sensor files live

A sensor's name is its file's stem: letters, digits, `-` and `_`, starting with a letter or digit. Its files layer by name, lowest first:

| Layer | Origin | Path |
|---|---|---|
| shipped | `builtin` | embedded in attend; projected with the app to `$XDG_DATA_HOME/agent-ways/attend/sensors/<name>.yaml` (in the repository, `attend/sensors/`) |
| user | `user` | `$XDG_CONFIG_HOME/attend/sensors/<name>.yaml` |
| project | `project` | `.claude/attend/sensors/<name>.yaml` |

A later layer overrides any field of an earlier one. Mappings merge key by key. `triggers` merge by trigger name: a trigger with the name of a lower one merges into it, and any other is added after them. Every other value, a list included, is replaced whole. A new name adds a sensor, and `enabled: false` turns one off.

A problem that makes the merged file invalid keeps the sensor from running; the lower layers do not stand in for it. `ways settings lint` reports each problem at the file and line of the highest layer that sets the field.

### Settings keys

Each sensor's `enabled`, `interval`, `min_interval`, `threshold`, `decay_threshold`, `lane` and `requires`, and a `processes` crate sensor's `watch`, are settings keys named `attend.sensors.<name>.<field>`. They resolve through the layers above, with the shipped layer lowest, and `ways settings get --json` and `list --json` name the file a value comes from:

```bash
ways settings set attend.sensors.git.interval 90                    # $XDG_CONFIG_HOME/attend/sensors/git.yaml
ways settings set attend.sensors.git.enabled false --project .      # .claude/attend/sensors/git.yaml
ways settings list --json attend.sensors.git
```

`set` writes the user's file, or with `--project` the project's, and creates it holding only that field when the sensor comes from a lower layer; a shipped file is never written. A value that would make the sensor invalid exits 3, names the problem and writes nothing. The settings screens' sensors tab lists every sensor of every layer with its card and these keys. A field of a sensor a project's file defines is written to that file, its `enabled` aside, which the screens keep in the user's file so the switch stays the operator's. A user file that holds only `enabled` defines no sensor of its own. Triggers, state and a kind's own fields have no keys yet.

`attend run` watches the `.yaml` files in the user and project sensor directories, and `attend.project_sensors` in the user config file. A sensor file added, changed or removed, or a project trusted or untrusted, reloads the run, and the banner names the reason. The shipped files change with the binary.

### Project files and the allow-list

A project's `command` or `mcp` file runs code the repository's author chose, so it loads only when the project is listed in `attend.project_sensors` in `$XDG_CONFIG_HOME/attend/config.yaml`:

```yaml
project_sensors:
  - ~/Projects/example
```

Entries expand `~`, `$HOME` and `$XDG_*`, and compare as canonical paths. Only the user file sets the key; a project's `.claude/attend.yaml` cannot list itself. Until the project is listed, a sensor whose only file is the project's is `refused`; it is still read, so `attend sensor list` shows its kind and source. A project file over a user or shipped sensor of the same name is left out of the merge, the lower layers run, and the list notes the refusal. A project file that changes fields of a `crate` sensor needs no listing.

## The file

### Fields every kind takes

| Field | Value | Unset |
|---|---|---|
| `spec` | the spec version the file is written to; a later version than attend reads makes it invalid | `1` |
| `name` | the sensor's name, which must match the file's stem | the stem |
| `description` | one line for `attend sensor list` | empty; a crate sensor shows its compiled description |
| `kind` | `crate`, `command`, `mcp` or `ways` | required |
| `enabled` | `true` or `false` | `true` |

### Fields of attend's kinds

`crate`, `command` and `mcp` files also take these:

| Field | Value | Unset |
|---|---|---|
| `interval` | seconds between polls while quiet, 1 to 604800 | 60 |
| `min_interval` | seconds between polls while things change, 1 to `interval` | 15, or `interval` when that is shorter |
| `threshold` | the accumulated magnitude that discloses, 0 to 100 | 2.0 |
| `decay_threshold` | quiet polls, at least 1, before the interval eases back to `interval` | 4 |
| `lane` | `event` or `message` ([Lanes](#lanes)) | `event` |
| `requires` | the tool permissions the sensor needs (ADR-116), checked by `attend sensor permissions` | none |
| `triggers` | a list of triggers ([Triggers](#triggers)); `command` and `mcp` only | none |
| `state` | declared state fields ([State](#state)); `command` and `mcp` only | none |

A field the kind does not take makes a `command` or `mcp` file invalid.

### `crate` files

A `crate` file configures a compiled sensor, named in `crate`: `context`, `disclosure`, `git`, `keepwarm`, `peers` or `processes`. Any other field is the compiled sensor's own, checked against the fields it declares; `processes` takes `watch` ([`sensors.md`](sensors.md#the-watch-list)), and the others take none. A crate sensor's poll is compiled, so its file takes no `triggers` and no `state`. A name in `crate` that attend does not have makes the file invalid; one this build leaves out by feature flag lists as `not compiled`.

A crate file in the user or project layer usually carries only what it changes. Polling git less often everywhere:

```yaml
# $XDG_CONFIG_HOME/attend/sensors/git.yaml
interval: 90
threshold: 3.0
```

A file of its own name with `kind: crate` runs a compiled sensor under that name. One sensor per compiled sensor runs: when two enabled files name the same `crate`, the one named after it runs, else the first by name, and the others are invalid. To run a copy, set `enabled: false` on the original.

### Compiled sensors

A compiled sensor implements `sensor_trait::Sensor`:

```rust
pub trait Sensor: Send {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }
    fn source(&self) -> String { String::new() }
    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)>;
    fn emission_threshold(&self) -> f64;
    fn base_interval(&self) -> Duration;
    fn min_interval(&self) -> Duration;
    fn decay_threshold(&self) -> u32;

    // Optional
    fn export_state(&self) -> Vec<(String, String)> { Vec::new() }
    fn import_state(&mut self, _state: &[(String, String)]) {}
    fn take_failure(&mut self) -> Option<String> { None }
}
```

- **`poll(focus)`** returns `(magnitude, description)` pairs; an empty list means nothing changed. `Focus` carries the working directory, a description of the current work, and keywords to scope observations by.
- **`description()`** and **`source()`** are what `attend sensor list` prints. `sensor_trait::sensor_metadata!();` inside the `impl` fills both from the crate's `Cargo.toml`.
- **`emission_threshold()`**, **`base_interval()`**, **`min_interval()`** and **`decay_threshold()`** are overridden by the sensor's file: attend wraps the compiled sensor in the file's cadence and threshold.
- **`export_state` / `import_state`** carry state across an attend restart; attend checkpoints every 30 seconds.
- **`take_failure()`** says why the last poll failed, such as a tool that could not run. attend takes it after each poll and records the sensor as `failed`. `git` reports a git it cannot run this way.

To add one: a `sensor-*` crate in `tools/` as an optional dependency of attend behind a feature, a construction arm in `build` in `tools/attend/src/sensors/mod.rs`, an entry in `CRATES` in `tools/attend-config/src/sensors.rs` naming its own fields and author fields, and a shipped file under `attend/sensors/` added to `SHIPPED` beside it.

### `command` files

A `command` sensor runs one program each poll. [`example-command.yaml`](../../attend/sensors/example-command.yaml) is the pattern: it reports systemd user units that fail.

| Field | Value | Unset |
|---|---|---|
| `argv` | the program and its arguments, run directly with no shell; name a shell when the command needs a pipe | required |
| `env` | extra environment, as `name: value` | none |
| `cwd` | the directory it runs in | the project attend runs for |
| `timeout` | seconds before the run is killed, 1 to 600 | 10 |
| `output` | `json`: one JSON document, the poll result, with an optional top-level `state` object of emitted fields; `lines`: `magnitude\|description` lines | required |

A `lines` sensor's lines are its observations, read as the retired script sensor's were; a `state` block needs `output: json`. A timeout, a non-zero exit, a failed exec or output that does not parse is the sensor's recorded state, shown by `attend sensor list`.

### `mcp` files

An `mcp` sensor calls one read-only tool on a stdio MCP server each poll. [`example-mcp.yaml`](../../attend/sensors/example-mcp.yaml) is the pattern: it reads unread Slack messages through the Slack MCP server.

| Field | Value | Unset |
|---|---|---|
| `server` | a server of Claude Code's MCP configuration (`~/.claude.json`, a project's `.mcp.json`) by name, or a server of the file's own: `{ command, args, env }` | required |
| `tool` | the tool to call; it must declare `readOnlyHint: true` | required |
| `args` | the tool's fixed arguments, sent unchanged on every call | none |
| `allow_unannotated` | `true` runs a tool that does not declare `readOnlyHint`, which the list shows as an override | `false` |
| `probe` | the record `attend sensor probe` writes; a sensor with no record that holds for its server, tool and args stays off | none |

An `mcp` sensor fills state only through `from`; a field without one is a lint warning.

## State

A `command` or `mcp` sensor declares the fields attend keeps for it between polls:

```yaml
state:
  unread:  { type: number, from: "$.unread_count" }
  last_ts: { type: string, max: 32, from: "$.messages[0].ts" }
```

- At most 16 fields, each named `[a-z][a-z0-9_]*` in at most 32 characters.
- `type` is `number`, `string` or `boolean`. A string carries `max`, its length in characters: 256 when unset, at most 1024. `max` on another type is invalid.
- `from` is a path over the poll result that selects one value; a path with `[*]`, `.*` or `..` can select many and is invalid. A field with no `from` is emitted: a `command` sensor prints it in a top-level `state` object.
- The serialized state is at most 4 KiB. When the declared maximums could exceed it, lint warns.

Paths use a JSONPath subset: `$`, `.field`, `[n]`, `[*]` and `..`, and `.*` for every field of an object.

## Triggers

A trigger fires on a change between two polls, never while a condition merely holds. Each has a `name` (letters, digits, `-` and `_`, unique in the sensor), a `magnitude` from 0 to 100 and a `description` template, and one of two tests.

**An item trigger** looks at items in the poll result:

```yaml
- name: unit-failed
  path: "$.units[*]"
  key: unit
  on: added
  where:
    unit: "*.service"
    load: { not: not-found }
  magnitude: 2
  description: "systemd user unit {unit} failed"
```

- `path` selects the items, and `key` names the field that identifies one across polls.
- `on` is `added`, `removed` or `changed` against the previous poll, or `enters` or `leaves` the `where` condition. `enters` and `leaves` need a `where`.
- `where` maps each property to a value or to operators. A bare value tests equality; a bare string with `*`, `?` or `[…]` is a glob, and a property name may be a glob too (`"wind_*"` tests every matching field). The operators are `equals`, `not`, `in` (a list), `contains`, `like` (a glob), `matches` (a regular expression), `above` and `below` (numbers). Conditions do not nest, and a bare list is written `in: [...]`.
- `match` combines the properties: `all` (unset) or `any`.

**A state trigger** looks at declared state fields, each mapped to `changed`, or for a number `rises` or `falls`, combined by `match`:

```yaml
- name: all-clear
  state: { failed: falls }
  magnitude: 1
  description: "{state.failed} units still failed, down from {prev.failed}"
```

A trigger tests `state` or `path` with `on`, never both.

### Templates

A `description` is text with `{…}` holes:

| Hole | Reads |
|---|---|
| `{field}`, `{a.b}` | the item's field, dotted for a nested one |
| `{value}` | the item itself |
| `{state.<field>}` | a declared state field after the poll's write |
| `{prev.<field>}` | that field before it |
| `{trigger.count}` | how many items changed |

A hole that reads `state.` or `prev.` must name a declared field, and a state trigger sees no item, so it reads only `state.`, `prev.` and `trigger.`. Each description goes out as one line.

## Lanes

ADR-136 draws the lane boundary at authored communication: words a person or agent composed ride the message lane, and everything the environment generates rides the event lane. A file states its lane:

- `lane` defaults to `event`.
- `lane: message` needs an author. A `command` or `mcp` file names it with `author` on each trigger: the item field, or for a state trigger the declared state field, that holds who composed the item. A `crate` file names it with a sensor-level `author`, a field the compiled sensor's items have; `peers` has `sender`.
- A message-lane file without its author is a lint error, and attend runs that sensor on the event lane and says why in `attend sensor list`.
- `lane_exception` records why a shipped file rides the message lane with no author. `keepwarm`'s shipped file carries the one exception (ADR-182). Only a shipped file may set it; in a user or project file it is an error and is ignored.
- The `peers` and `keepwarm` compiled sensors always ride the message lane, under any file name: a lapsed cache drops what the event lane holds, and peers carries messages while keepwarm is the wake that keeps the cache warm. A file that declares `lane: event` for either is a lint error, and the lane is ignored.

## Testing from the CLI

| Command | What it does |
|---|---|
| `attend sensor list [--json]` | every sensor the files declare: kind, origin, file, source and version, state, lane, interval, and each reason and note; `--json` adds each layer's file and the last poll |
| `attend sensor poll <name> [--dry-run] [--json]` | one real poll of a crate sensor, passed through its threshold and lane, delivered as `attend run` would unless `--dry-run` |
| `attend sensor fire <name>[/<trigger>] [--input FILE\|-] [--dry-run] [--commit] [--json]` | a synthetic result through the sensor's threshold, lane and delivery, each line marked `[test]` |
| `attend sensor permissions` | each sensor's `requires` against `~/.claude/settings.json` |

A crate sensor's `--input` is a JSON list of `{magnitude, description}` objects or `magnitude|description` lines, from a file or `-` for stdin; with none, `fire` sends one observation at the sensor's threshold. Each verdict says whether the result was delivered, and at which priority, or held, with the accumulated magnitude against the threshold. A low-priority delivery is logged and wakes no one. A compiled sensor's first poll sets its baseline, so `poll` on a fresh process often reports no observations.

`<name>/<trigger>` addresses one trigger, and `--commit` keeps the trigger state and store a fire leaves. Both take effect once attend hosts `command` and `mcp` sensors; a crate sensor has no triggers, and `--commit` changes nothing on it. `poll` and `fire` record nothing in the sensor's store. A name with no sensor exits 2; a sensor attend cannot poll here, such as a `command` file or one that is `invalid`, exits 3.

## The store

Each poll `attend run` makes is recorded in the sensor's store, one JSON file per sensor and project under `$XDG_STATE_HOME/agent-ways/attend/sensor-state/`, named by a hash of the two. It holds the last poll's time, whether it succeeded, why it failed, and how many observations it returned. `attend sensor list` reads it for the `failed` state and `last_poll`. The declared state and each trigger's memory join it once attend hosts `command` and `mcp` sensors.

## The retired forms

The `sensors:` section of `$XDG_CONFIG_HOME/attend/config.yaml` and `.claude/attend.yaml`, its `+name:` and `-name:` prefixes, and the inline `script:` sensor have no reader. `ways settings lint` and `attend config lint` report each old key with the sensor file to write and the field to set there, and `attend run` prints the same on stderr when it loads the config:

| Old key | Write instead |
|---|---|
| `sensors.<name>.<field>: <value>` | `<field>: <value>` in `<name>.yaml` in the same layer's sensor directory |
| `-<name>:` | `enabled: false` in `<name>.yaml` |
| `<name>: { script: <path> }` or `+<name>:` | `<name>.yaml` with `kind: command`, `argv: [bash, <path>]` and `output: lines`, plus the old cadence fields |

The scripts under `tools/attend/examples/` (`xdg-downloads.sh`, `gh-notifications.sh`) keep the `magnitude|description` contract, so each becomes a `lines` command sensor that way, and runs once attend hosts the kind.

## Design rules of thumb

Written as heuristics:

1. **Design magnitudes as a hierarchy.** Pick a few discrete tiers (background, notable, urgent, critical) and assign each kind of change to one.
2. **Make background events sum to something useful.** If a sensor mostly emits 0.5, check that three to five of them cross the threshold.
3. **Reserve the top tier for emergencies.** A 5.0 should be worth interrupting a conversation about something else. Abuse of the top tier trains the user to ignore attend: alarm flooding, the failure mode alarm management exists to prevent.
4. **Prefer empty polls to noisy ones.** When nothing is happening, report nothing. The loop slows a quiet sensor's polling and lets its engagement relax.
5. **Report the current situation, and let triggers find the change.** A command sensor prints what is true now; attend's stored state and triggers turn two polls into a transition.
6. **Fail as a failure.** An unavailable upstream (network down, `gh` not authenticated, a rate limit) is a non-zero exit or a failed poll, which attend records as the sensor's state. An error dressed as an event pollutes the notification stream.
7. **Stay inside the timeout.** A command sensor is killed at its `timeout`, 10 seconds unless set. Work that takes longer belongs in a background job that writes a file the poll reads.

> **What is not a sensor: watching one PR's CI.** A sensor is for *ambient* observation: the local world or a remote stream that is always worth a glance. Watching one PR's checks until they land is session-specific and bounded to a task in hand, so it belongs in a **Monitor** the agent launches when it starts on the PR and that ends when CI resolves (ADR-137, boundedness).

## Related

- [`first-sensor.md`](first-sensor.md) — a first sensor file, step by step
- [`sensors.md`](sensors.md) — the shipped sensors and what each emits
- [`loop.md`](loop.md) — the substrate a sensor plugs into
- [`engagement.md`](engagement.md) — the action potential model in detail
- [`delivery.md`](delivery.md) — the message lane and its conduits
- **ADR-403** — sensors as files under one spec
- **ADR-117** — sensor crate extraction and feature flags
- **ADR-123** — firing dynamics; the action potential engagement model in force
- **ADR-116** — permission requirements for sensors
- **ADR-136** — the event and message lanes
- **ADR-199** — ways sensors
