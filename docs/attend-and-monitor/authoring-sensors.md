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
| `command` | attend | an argv run directly, with no shell | runs: attend runs the argv each poll and evaluates its output |
| `mcp` | attend | one read-only tool call on a stdio MCP server | runs once a probe record holds: attend calls the tool each poll and evaluates its result |
| `ways` | ways (ADR-199) | the session's hook stream | attend lists it as `degraded` and never runs it |

Triggers, declared state, `where` conditions, JSONPath and templates parse and are checked when a file loads. attend evaluates them over each command sensor's output and each mcp sensor's tool result, and writes declared state to the store. `attend sensor probe` records the read-only check an mcp sensor needs before it runs ([`mcp` files](#mcp-files)). `ways settings` reaches the shared fields of every sensor file ([Settings keys](#settings-keys)); write the other fields in the files directly, and `ways settings lint` checks them.

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

`attend run` and the sensor host watch the `.yaml` files in the user and project sensor directories, and `attend.project_sensors` in the user config file. A sensor file added, changed or removed, or a project trusted or untrusted, reloads the run, and the banner names the reason; the host re-executes in place. The shipped files change with the binary.

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

A `crate` file configures a compiled sensor, named in `crate`: `context`, `disclosure`, `git`, `keepwarm`, `peers` or `processes`. Any other field is the compiled sensor's own, checked against the fields it declares; `processes` takes `watch` ([`sensors.md`](sensors.md#the-watch-list)), and the others take none. A crate sensor's poll is compiled, so its file takes no `triggers` and no `state`. A name in `crate` that attend does not have makes the file invalid; one this build leaves out by feature flag lists as `not-compiled`.

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
- **`take_failure()`** says why the last poll failed, such as a tool that could not run. attend takes it after each poll and records the sensor as `failing`. `git` reports a git it cannot run this way.

To add one: a `sensor-*` crate in `tools/` as an optional dependency of attend behind a feature, a construction arm in `build` in `tools/attend/src/sensors/mod.rs`, an entry in `CRATES` in `tools/attend-config/src/sensors.rs` naming its own fields and author fields, and a shipped file under `attend/sensors/` added to `SHIPPED` beside it.

### `command` files

A `command` sensor runs one program each poll. [`example-command.yaml`](../../attend/sensors/example-command.yaml) is the pattern: it reports systemd user units that fail.

| Field | Value | Unset |
|---|---|---|
| `argv` | the program and its arguments, run directly with no shell; name a shell when the command needs a pipe | required |
| `env` | extra environment, as `name: value`, added to what the command inherits | none |
| `cwd` | the directory it runs in | the project attend runs for |
| `timeout` | seconds before the run is killed, 1 to 600 | 10 |
| `output` | `json`: one JSON document, the poll result, with an optional top-level `state` object of emitted fields; `lines`: `magnitude\|description` lines | required |

The session's variables and tokens are kept out of the command's environment. What it inherits depends on the layer of the highest file that sets its `argv`, `env` or `cwd`:

- **A shipped or user file:** attend's environment, less every variable whose name starts with `CLAUDE` (Claude Code's session id, project directory, config directory and OAuth token), `ANTHROPIC_` (the API key and base URL) or `ATTEND_` (attend's own markers for its restarts and session moves), and the variables `AI_AGENT` and `AWS_BEARER_TOKEN_BEDROCK`. General-purpose credentials and terminal variables, such as `AWS_*`, `GOOGLE_*`, `TMUX*` and `SSH_AUTH_SOCK`, still reach it.
- **A project file:** an empty environment with only `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, every `LC_*`, `TZ`, `TMPDIR`, `XDG_RUNTIME_DIR` and `DBUS_SESSION_BUS_ADDRESS` carried over. A project file that sets `argv`, `env` or `cwd` over a user file puts the command under this rule; one that only tunes its cadence does not.

In both cases the file's `env` adds on top, and a name it sets replaces the inherited value. One variable is set after both: `ATTEND_SENSOR_HOST`, the sensor marker, described below.

This is not a sandbox: the command runs as the operator, with the operator's files and network. attend finds the session through parent processes, not the environment, and it refuses to act as the session for a process a sensor command started. It knows such a process in two ways:

- **A sensor host among its ancestors**, up to pid 1. A host is a process whose command line is `attend run` or `attend sensor host`, directly or through an interpreter such as `sh /path/attend run`, or whose executable file is named `attend` with those arguments.
- **The sensor marker in its environment.** attend sets `ATTEND_SENSOR_HOST` to `<pid>:<start time>`, naming itself, in the environment of every process that runs a command a sensor file or a repository can choose: a `command` sensor's child, an `mcp` sensor's server, and the `git` and `ways` the git, context and disclosure sensors run. The fixed `ps` of the processes sensor and keepwarm's `ways` are not marked. The children of `attend sensor poll` carry the marker too, and since that verb is not a sensor host, the marker is what stops a command it runs. A process keeps the marker when it detaches from the host, as `( attend message send x & )` or `setsid -f` does. The marker refuses only while the process it names is a running attend that started at the time it records. A marker naming a process that has exited, or a pid now held by another process, is stale and refuses nothing. attend reads the marker on every check. A marker can leak, for example into a daemon a sensor command starts and the shells that daemon opens; it then refuses session verbs there, the session's own included, until the attend that set it exits, and `unset ATTEND_SENSOR_HOST` clears it at once.

So an `attend` a sensor command runs refuses every verb that sends, replies, joins, listens, drains or reads the session's messages, store or enrollment, with exit status 3. Under a host the message is "this attend was started by a sensor command, so it does not act as the session". When the marker refused, the message names the variable, its value and the attend it names, and says to run `unset ATTEND_SENSOR_HOST` if no sensor started the shell. attend keeps the verbs that need no session: `message cleanup`, `peer list`, `channel list`, `sensor list`, `sensor permissions` and `config`. A sensor of your own that ran `attend message send` to notify you is refused; write the notice as the sensor's output instead.

The guard stops a sensor command from acting as the session by accident or through `attend`. It is not a defence against a hostile command running as you. The control against a hostile command is the trust you give its sensor file: a project's `command` and `mcp` files load only for a project you list (see [Project files and the allow-list](#project-files-and-the-allow-list)), and a user file is one you wrote or chose. Because the command runs as the same user, the guard does not cover:

- asking a process that already runs outside the host to start `attend` for it: a service manager (`systemd-run --user`, `launchctl`), a terminal multiplexer's server that was already up (`tmux new-window`), `at` or cron, `ssh`, or `sudo`, which resets the environment. The `attend` it starts has neither the host among its ancestors nor the marker;
- unsetting the marker and then detaching from the host, which leaves neither check anything to see;
- writing a Claude Code session record, which takes effect only for a process that has neither check against it, as one started either of those two ways has;
- writing attend's store or signal files directly;
- reading Claude Code's environment from `/proc/<pid>/environ` or `ps eww`;
- typing into the session's terminal, for example with `tmux send-keys`;
- starting `claude` either of those two ways;
- running verbs as an unresolved caller, as any shell outside Claude Code does.

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

**The server.** A named server is looked up as Claude Code finds it: the project's entry under `projects` in `~/.claude.json` first (`$CLAUDE_CONFIG_DIR/.claude.json` when that is set), then the project's `.mcp.json`, then the user's `mcpServers` in `~/.claude.json`. `.mcp.json` is read only when the project's entry misses. A malformed one leaves your own servers usable, as in Claude Code, and fails only a server nothing else defines. A `.mcp.json` server is code the repository chose, so it starts only for a project in `attend.project_sensors`. Only a stdio server runs: an entry whose `type` is `http` or `sse` is the sensor's recorded failure. In a `~/.claude.json` entry, `${NAME}` and `${NAME:-default}` in `command`, `args` and `env` expand from attend's environment, as Claude Code expands them; a reference to an unset variable with no default is the sensor's failure, naming the variable. attend expands nothing in a repository's `.mcp.json` entry, and an entry with a reference is the sensor's failure, naming it. The server runs in the project's directory, with the environment a command sensor's child gets ([`command` files](#command-files)): attend's environment less the session's variables, or for a project file or a `.mcp.json` server the fixed project set, with the entry's or the file's `env` on top.

**Its life.** A server is started at the first poll that needs it and kept across polls: one per distinct launch (program, args, env, directory, environment rule and definition) in the sensor host, shared by every sensor that names it. A server that exits, times out or breaks the protocol is ended at once, and that poll records why, such as `server slack exited with status 1: <its last stderr line>`. The next poll starts it again. One `attend sensor poll` or `probe` starts its own server and ends it before it exits.

**Its end.** A server is ended in the order the MCP stdio transport gives: its input is closed, and after a second its process group is sent TERM, and after another second KILL; a server that failed gets a tenth and a quarter of a second. The host ends its servers when it ends: when its parent is gone, when its output is closed, or on TERM, HUP or INT, which it takes within a second. Before it re-executes itself on a changed binary or sensor file, it ends them too. A host killed with SIGKILL ends nothing, so each running server is noted under `$XDG_STATE_HOME/agent-ways/attend/mcp-servers/` with its process group, its leader's start time and the process that started it. The next host for the project ends a noted server whose starter is gone while the server's leader still runs with the recorded start time. A server noted by a host that still runs, such as another session's in the same project, is left alone, and so is an entry whose start times cannot be read. When the leader is gone, the entry is dropped and nothing is sent, since the group's id may be free and taken by another program, such as the second half of a pipeline you run. A server that cannot have its start times read when it starts gets no entry. A server's leader that exited while something it started runs on is not swept.

**Its limits.** Each step has a 10-second limit: the start (spawn, `initialize` and the `initialized` notice), `tools/list` with all its pages, and each `tools/call`. A server runs in a process group of its own, and a missed limit ends the whole group, so nothing it started outlives it. A message over 1 MiB, or a line on stdout that is not JSON-RPC, breaks the protocol.

**The read-only check.** The tool must declare `readOnlyHint: true` in `tools/list`. `allow_unannotated: true` calls a tool that declares no hint, and `attend sensor list` notes it as overridden. A tool that declares `readOnlyHint: false` says it writes, and is refused with or without the override. A refusal, a missing tool and a result with `isError` are the sensor's recorded failure, shown by `attend sensor list`.

**The result.** The triggers and `from` paths read the call's `structuredContent`. Without it they read the first text item when that text is JSON, and otherwise the result object as the server sent it, so a path such as `$.content[0].text` reaches the text.

**The probe.** The hint is advisory, so an mcp sensor also needs a probe record before it runs. `attend sensor probe <name>` calls the tool twice in a row. The second call must return every item the first did: by its `key` for an item trigger, also meeting `where` as before for an `enters` or `leaves` trigger; the same value in each watched field for a state trigger; and the same result for a sensor with no triggers. A first call that returns nothing a trigger selects has nothing to compare, and the record says the calls do not match: probe while the tool returns items. A call cannot show read state the service keeps, such as a message marked read, so the probe then asks you at the terminal to confirm in another client that the items are untouched; `y` or `yes` is a yes, and anything else is a no. With no terminal on its input, nothing is recorded as confirmed: the record says `confirmed: false`, so a caller that pipes an answer in cannot give one. From Claude Code, run it as `! attend sensor probe <name>`, so the question reaches you. This is not a hard barrier: a caller that gives the probe a terminal of its own, as `script` does, can still answer, and what stands between an agent and that answer is the permission prompt on the command it would run.

The record goes in your user file for the sensor, `$XDG_CONFIG_HOME/attend/sensors/<name>.yaml`. A sensor no project file defines keeps it under `probe`. A sensor a project file defines keeps one record per project under `probes`, keyed by the project's canonical path, so a project reached through a symlink finds its record, and same-named sensors of two projects each keep theirs; another project's record does not count. The probe edits its own entry alone, or creates the file with the records alone when the sensor has none; such a file defines no sensor of its own. It checks the file before the calls, so a record that could not be written costs no call. A `probe` or `probes` key in a project or shipped file is ignored, and `attend sensor list` and `ways settings lint` say so. The record names the server's binding: where its definition came from (`inline`, `local`, `project` or `user`) and a hash of the definition as written, with the program and args a `~/.claude.json` entry runs after expansion, so it holds no value of the definition. A variable that makes the program or args run something else voids the record, and a rotated token in `env` keeps it. The sensor runs once the record holds: taken with its current server definition, tool and args, with `calls_match` and `confirmed` both true. An inline server's changed command, args or env voids it at once; a named server's changed definition voids it at the next call, which fails saying so. Until it holds, `attend sensor list` shows the sensor `degraded` with the reason, and `attend sensor poll` refuses it, whether or not it is enabled. A probe whose call fails writes no record, and a probe that does not hold exits 3.

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

- `path` selects the items, and `key` names the field that identifies one across polls. An item that lacks the key field is skipped. A trigger with a wildcard path and no `key` identifies each item by its JSON text, so a changed item reads as one removed and one added, and `changed` never fires.
- `on` is `added`, `removed` or `changed` against the previous poll, or `enters` or `leaves` the `where` condition. `enters` and `leaves` need a `where`. For `added`, `removed` and `changed`, only the items that meet `where` count, so `removed` also fires when an item stops meeting `where`.
- `where` maps each property to a value or to operators. A bare value tests equality; a bare string with `*`, `?` or `[…]` is a glob, and a property name may be a glob too: `"wind_*"` tests every matching field, and holds when at least one field matches and every matching field meets the condition. A glob property name matches the item's top-level fields only. A dotted property such as `user.name` reads a nested field. A property the item lacks fails its condition, so `not: x` on a missing property does not hold either. `equals` and `not` compare exactly, with no glob, and numbers compare by value, so `1` equals `1.0`. The operators are `equals`, `not`, `in` (a list), `contains`, `like` (a glob), `matches` (a regular expression), `above` and `below` (numbers). Conditions do not nest, and a bare list is written `in: [...]`.
- `match` combines the properties: `all` (unset) or `any`.

**A state trigger** looks at declared state fields, each mapped to `changed`, or for a number `rises` or `falls`, combined by `match`:

```yaml
- name: all-clear
  state: { failed: falls }
  magnitude: 1
  description: "{state.failed} units still failed, down from {prev.failed}"
```

A trigger tests `state` or `path` with `on`, never both. A state trigger compares the fields after the poll's write with the values its store kept, so the first poll of a fresh store sets a baseline and fires nothing. A write the checks refuse leaves the state as it was, so the state triggers see no change on that poll.

### Memory and expiry

Two fields set where a trigger keeps what it last saw and how long what it reports stays deliverable. `expires` is allowed on any trigger, and `memory: session` on an item trigger only:

```yaml
- name: behind
  path: "$"
  on: enters
  where: { behind: true }
  memory: session
  expires: session
  magnitude: 1
  description: "agent-ways has a newer release: {summary}"
```

- `memory` is `store` or `session`, and unset is `store`. With `store`, the trigger's memory lives in the sensor's store for the session ([The store](#the-store)) and survives a restart. A fresh store's first poll sets a baseline and fires nothing, and after that the trigger fires on each change. The memory is kept with a fingerprint of what the trigger tests: a hash of its `path`, `key`, `on`, `where`, `match` and `state` fields as the file writes them, with keys in sorted order. A trigger whose fingerprint changes starts from a fresh baseline even when it keeps its name, and so does a renamed trigger, because the memory is kept under the name. The fingerprint depends on the file alone, so an upgrade of attend keeps every trigger's memory.
- With `memory: session`, the memory belongs to the session and is not written to the store. It starts empty and takes no baseline, so the first poll is compared against nothing seen: `on: enters` fires on the first poll whose result meets `where`, even when the condition already held when the session started. Its line then reaches the session at most once, whatever `on` it uses: once it was written, or held under the event lane's cut, a condition that leaves and enters again, a later item or a later change fires nothing more. While the loop holds its line, such as behind the governor, it fires nothing more either. A line the loop drops before it goes out, by the refractory, the cache gate, a failed write or a restart, is offered again on the next poll that meets the trigger. The once-per-session mark belongs to the fingerprint, not the name. Renaming a trigger that has fired keeps it silent, and editing what it tests makes a new definition, which may fire once more in the session. The memory travels in the session's checkpoint, which is saved right after the line is written, so it survives `attend run` or the sensor host restarting itself after a sensor file or the binary changes. A kill between the write and that checkpoint can repeat the notice. Use it for a notice each session should get once, such as an update check.
- `memory: session` does not apply to a state trigger, which compares against the last state its store kept, and the sensor is invalid until the trigger uses `store` or leaves `memory` unset.
- `expires` is a whole number of seconds from 1 to 2592000 (30 days), or `session`. A report lapses that many seconds after it is written, or with `session` stays until the session reads it. Unset is 3600, the event lane's decay window.
- Any other value of either field makes the sensor invalid, and the lint message says what the field takes.

The sensor host and `attend sensor poll` and `fire` write each report's `expires` into its signal as an `expires:` tag in Unix seconds, and write none for `session`. Dropping a lapsed event-lane report unread is the drain's work, which #1313 adds.

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
- `lane_exception` is removed from the spec. A file in any layer that still carries it is a lint error that names the removal, and the field changes no lane.
- The `peers` and `keepwarm` compiled sensors always ride the message lane, under any file name: a lapsed cache drops what the event lane holds, and peers carries messages while keepwarm is the wake that keeps the cache warm. A file that declares `lane: event` for either is a lint error, and the lane is ignored.

## Testing from the CLI

| Command | What it does |
|---|---|
| `attend sensor list [--json]` | every sensor the files declare: kind, origin, file, source and version, state, lane, interval, and each reason and note; `--json` prints each sensor as an item, with an `overrides` list of the higher layers' files, then its fields and the last poll: this session's own, or from a shell the newest of any session in the project |
| `attend sensor poll <name> [--session ID] [--dry-run] [--json]` | one real poll of a crate, command or mcp sensor, with each trigger's verdict, passed through its threshold and lane, and unless `--dry-run` written to the session as system signals through the sensor host's writer |
| `attend sensor probe <name> [--json]` | two calls of an mcp sensor's tool, compared, and the operator's confirmation at a terminal, written as the `probe` record in the sensor's user file ([`mcp` files](#mcp-files)) |
| `attend sensor fire <name>[/<trigger>] [--input FILE\|-] [--session ID] [--dry-run] [--commit] [--json]` | a synthetic result through the sensor's threshold, lane and the same writer, each line marked `[test]` |
| `attend sensor permissions` | each sensor's `requires` against `~/.claude/settings.json` |

A command sensor with `output: json`, or an mcp sensor, takes a poll result as `--input`, one JSON document, and runs it through its declared state and triggers. A fire calls no server, so it works on an mcp sensor before its probe. A crate sensor's, or an `output: lines` sensor's, `--input` is a JSON list of `{magnitude, description}` objects or `magnitude|description` lines, from a file or `-` for stdin; with none, `fire` sends one observation at the sensor's threshold. Each verdict says whether the result was delivered, and the id of each signal written, or held, with the accumulated magnitude against the threshold. An event-lane result under the cut of 3.0 is logged and written to no one, as the host does. The session is `--session`, else the one the shell runs in. A named session with no record is refused, since its tray is unknown, and with no session at all `poll` and `fire` deliver nothing unless `--dry-run`. A compiled sensor's first poll sets its baseline, so `poll` on a fresh process often reports no observations.

`<name>/<trigger>` addresses one trigger, and `--commit` keeps the trigger memory and declared state a fire leaves. `--commit` with one trigger named is refused, because a commit advances the whole sensor and advancing one trigger alone would leave the others' memory behind. A crate sensor has no triggers, and `--commit` changes nothing on it. `poll` and `fire` compare against this session's store and write nothing to it, except a fire with `--commit`. A name with no sensor exits 2; a sensor attend cannot poll here, such as an `mcp` file with no probe record that holds or one that is `invalid`, exits 3.

## The store

Each poll `attend run` or the sensor host makes is recorded in the sensor's store, one JSON file per sensor, project and session under `$XDG_STATE_HOME/agent-ways/attend/sensor-state/`, named by a hash of the three. Each report goes to one session, so two sessions in one project keep separate stores, and each starts from its own baseline. A sensor keeps a store only when attend knows the session's id: an `attend run` whose session did not resolve keeps none. `attend sensor poll` and `fire` read the session's store and write nothing to it, except a fire with `--commit`. A running sensor reads its store again before a poll when another writer, such as a committed fire, changed it, so the commit is kept. The store holds the last poll's time, whether it succeeded, why it failed, and how many observations it returned, the memory of each trigger with `memory: store`, and the declared state with a fingerprint of the declaration that wrote it. A store whose state another declaration wrote, or that fails the checks, loses its state and its state triggers' memory, and the next poll records why. `attend sensor list` reads it for the `failing` state and `last_poll`: the session's own store, or from a shell the newest poll of any session in the project.

Each store file records its format and its sensor, project and session. A file of an older format, such as one keyed by sensor and project alone, or one whose key does not match, reads as no store, so the sensor starts from a baseline. When `attend run` or the sensor host starts its sensors, it removes every file of the directory not written for 30 days, and every `.json` file that is not valid JSON or whose `store` format is missing or older than the current one. It leaves a store of a newer format alone, so a newer attend on the same machine keeps its stores, and it skips a file it cannot read. After `/clear`, `attend run`, or a sensor host started with `--moved-from`, moves the session's stores to its new id.

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

## References

- **ADR-403** — sensors as files under one spec
- **ADR-416** — `memory`, `expires`, stores keyed by session, and the removal of `lane_exception`
- **ADR-117** — sensor crate extraction and feature flags
- **ADR-123** — firing dynamics; the action potential engagement model in force
- **ADR-116** — permission requirements for sensors
- **ADR-136** — the event and message lanes
- **ADR-199** — ways sensors
