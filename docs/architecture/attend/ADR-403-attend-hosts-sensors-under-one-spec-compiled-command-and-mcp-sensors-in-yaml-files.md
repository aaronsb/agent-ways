---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-117#two-sensor-paths, ADR-117#config-as-control-plane, ADR-503#decision, ADR-136#decision]
basis:
  - operator: aaronsb
    level: directed
    said: "I want to be more formal in the specification for sensors, and improve the specification to externalize the configuration for sensors. There are then two kinds of sensors - the kinds that are compiled right into attend, that follow the crate spec (our existing sensors) and a different style of sensors that follows a toml configration spec. The toml files each specify one sensor, and are launched at runtime of attend. This way I could write a sensor that queries anything - dbus, or system state, or honestly anything."
    via: session 2026-10-03, filing #843
  - operator: aaronsb
    level: guided
    said: "the goal is to make attend less of a monolith"
    via: session 2026-10-03
  - operator: aaronsb
    level: directed
    said: "I think an external sensor that drives an mcp server with a deterministic call could be really valuable. for instance, if a slack mcp server can look for new messages but not trigger the 'message has been read' state (or otherwise get a delta between previous call and next call) then that becomes something that the agent gets an attend notification for."
    via: session 2026-10-03, filing #844
  - operator: aaronsb
    level: directed
    said: "I think we should start with stdio connector support only. remote end oauth needs to be handled cleanly where there's no user auth gate interruption"
    via: session 2026-10-03, settling #844 decision 2 and filing #845
  - operator: aaronsb
    level: guided
    said: "it could be yaml too - I'm not stuck on toml just whatever makes most sense so we don't add extra libraries to manage it"
    via: session 2026-10-03
  - operator: aaronsb
    level: directed
    said: "I th ink that the toml shape for this needs to be able to define a property to a value to diff against for the sensor to trigger"
    via: session 2026-10-03; followed by "and probably needs more than one - it could be a small object definition" and "we probably should allow wildcards too"
  - operator: aaronsb
    level: guided
    said: "right now section 6 of 'ways settings' lets us review the existing sensors for attend, and they can be edited fully outside the tui through the structured object configuration path"
    via: session 2026-10-03, on reviewing every sensor kind the same way
  - operator: aaronsb
    level: directed
    said: "we should be able to through the cli invoke or trigger an event for testing too"
    via: session 2026-10-03
  - operator: aaronsb
    level: directed
    said: "(and delete, unless it's a bundled item (compiled sensors, bundled themes)"
    via: "session 2026-10-03, on copying and renaming themes in the TUI; preceded by \"this will come in handy to have defined that tui flow for sensors\""
  - evidence: "attend today: a built-in sensor's interval, min_interval, threshold, decay_threshold and requires are rows of SENSOR_DEFAULTS, and the built-in names are BUILTINS, both in tools/attend-config/src/schema.rs; Config::from_layers fills every sensor from them (tools/attend-config/src/config.rs). register_builtin! in tools/attend/src/sensors/mod.rs repeats interval, min_interval and decay_threshold as literal fallbacks, and peers is registered by hand outside the macro. processes reads an extra key, watch"
  - evidence: "the script sensor (tools/attend/src/sensors/script.rs) is configured inline as attend.sensors.<name>.script, runs under bash with a 10 s timeout and a magnitude|description stdout contract, and reports a missing script, an exec failure, a timeout and a wait or output-read failure on stderr; only a non-zero exit and a malformed line are dropped silently"
  - evidence: "the lane is chosen by sensor name: rides_message_lane in tools/attend/src/cmd/run/tick.rs returns true for peers and keepwarm"
  - evidence: "attend's sensor keys are written to the user file $XDG_CONFIG_HOME/attend/config.yaml and the project overlay .claude/attend.yaml (attend_config::user_path and project_path); ways settings maps each file kind to one path per layer (file_of in tools/ways-cli/src/cmd/settings/mod.rs)"
  - evidence: "the workspace parses and writes YAML through serde_yaml in attend-config and agent-settings; toml is a dependency too, used by agent-theme for theme files"
  - precedent: ADR-503
  - precedent: ADR-136
  - operator: aaronsb
    level: directed
    said: each sensor gets a jailed place to write a bounded sized state file, of whatever content it wants. this way, the sensor could run the slack mcp, get a count of messages, and if the count of messages from last to next increments, or maybe the date of the last message increments, then that triggers the configured message back to the agent, with sime simple variable passing.
    via: session 2026-10-29
  - operator: aaronsb
    level: directed
    said: the actual message could also be a variable, but for a simple implementation, it's just anything from 'go check new messages in slack mcp' all the way to "the last message in slack mcp from $user at $time says $message"
    via: session 2026-10-29
  - operator: aaronsb
    level: directed
    said: I think the storage format should be extremely bounded, declared with a couple value types (number, string, boolean) and a strict check on that. this reduces the path of a malicious mcp server or something writing invalid or adversarial instructions. on startup, attend wipes the storage
    via: session 2026-10-29
  - operator: aaronsb
    level: directed
    said: ok, maybe we don't wipe
    via: session 2026-10-29
  - operator: aaronsb
    level: directed
    said: we should have a reasonable limit for cleanup though
    via: session 2026-10-29
  - operator: aaronsb
    level: directed
    said: 'One poller per project: the first attend in a project takes a lock and polls; the others read its results'
    via: session 2026-10-29, choice question on two attend instances in one project, selected from agent-written options
  - operator: aaronsb
    level: directed
    said: 'Both lanes: any filled description is made one line with frame-forging < defused, whichever lane it rides'
    via: session 2026-10-29, choice question on the event lane, selected from agent-written options
  - operator: aaronsb
    level: directed
    said: 'Refuse the write: an emitted value for a field with a from mapping refuses the whole write'
    via: session 2026-10-29, choice question on an emitted value for a mapped field, selected from agent-written options
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Yes, allow-list (Recommended)"
    via: session 2026-10-03, selected from agent-written options on the gate for repository sensor files
    covers: [project-trust]
  - operator: aaronsb
    said: "Allow-list: a repo's command and mcp sensors are refused until the project is in attend.project_sensors in the user config"
    via: session 2026-10-29, choice question on the project-trust probe, selected from agent-written options
    covers: [project-trust]
  - operator: aaronsb
    said: "other than that I like the adr and can accept. perhaps we could implement using worktrees"
    via: "session 2026-10-29, after the PR #1204 amendment"
observable:
  - 'see: attend sensor list shows a shipped git.yaml, a user weather.yaml (kind: command, Open-Meteo) and a user slack-unread.yaml (kind: mcp), each with its source file and last poll state'
  - 'run: attend sensor fire weather/high-wind --input sample.json produces a [test] notification on the event lane and leaves the trigger''s stored state unchanged'
  - 'run: ways settings set attend.sensors.weather.triggers.high-wind.where.wind_speed_10m.above 40 writes the user file, and ways settings list --json attend.sensors.weather names that file as the source'
  - 'see: an mcp sensor with no probe record stays disabled, and attend sensor list says to run attend sensor probe <name>; after a probe whose second call returns every item of the first and the operator confirms in another client that the items are still unread, it is enabled'
  - 'see: an mcp sensor on a tool without readOnlyHint is refused unless its file sets allow_unannotated: true, and attend sensor list marks that sensor as overridden'
  - 'see: a project sensor file of kind command is reported as refused until the project is in attend.project_sensors in the user config'
  - 'run: ways settings lint on a config.yaml that still holds attend.sensors.git.interval names that key and the sensor file to write instead'
  - 'run: ways settings lint flags a command sensor that declares lane: message with no author field, and attend runs it on the event lane'
  - 'run: a command sensor that emits a state object with an undeclared field, a wrong type or an over-long string has the write refused whole; its previous state stays, and attend sensor list and the settings tab show the refusal as its sensor state'
  - 'see: a store file edited by hand to hold an undeclared field, a wrong type or an over-long string is discarded at the next attend start and shown as a sensor state; an untouched store survives a restart and the next poll fires only on a real change'
  - 'see: a store whose sensor file is deleted is gone after the next start, and attend sensor list names the deletion; a store older than the retention limit is removed, and its sensor''s first poll sets a baseline without firing'
  - 'run: attend sensor fire slack-unread/new-message --input FILE, where FILE raises unread from 2 to 3 and carries a new last_ts, produces one [test] notification on the message lane reading "the last message in slack mcp from <user> at <ts> says <text>" with the values from FILE'
status: accepted
date: 2026-10-03
deciders:
  - aaronsb
related:
  - ADR-103
  - ADR-116
  - ADR-172
  - ADR-182
  - ADR-199
  - ADR-501
  - ADR-504
  - ADR-406
---

# ADR-403: Attend hosts sensors under one spec: compiled, command and MCP sensors in YAML files

## Summary

- **Decided:** attend becomes a sensor host. Every sensor, compiled or declarative, is defined by one YAML file under one versioned spec. The spec is a shared core (identity, file layout, layering, `where` conditions, enable and disable, the management flow) plus a block per kind. attend hosts three kinds: `crate` (compiled poll code), `command` (an argv and an output format) and `mcp` (a read-only tool on a stdio MCP server). ways hosts the fourth, `ways` (ADR-199). An attend trigger is a small condition object over the poll result, with wildcards, that fires on transitions. An attend sensor may declare a small typed state that only attend writes and checks, and its triggers and descriptions read it. `ways settings` and its key path reach every sensor, trigger and field, and the CLI can poll, probe and test-fire any sensor.
- **Trades away:** the inline `attend.sensors.<name>.script` form and the `attend.sensors.*` keys in `config.yaml` and `.claude/attend.yaml` are replaced, with no legacy reader. Compiled sensors lose their hard-coded defaults. A sensor's memory is limited to a few declared scalar fields. Remote MCP servers wait for #845.
- **One-way?** No. Sensor files are plain data, and the compiled sensors keep their poll code.
- **Probes:** *Confident (host):* attend is a host and sensors are data it loads, so adding a sensor needs no change to attend. *Not confident (project-trust):* a project's own command and MCP sensor files load only after you list that project in your user config. Is that the gate you want for sensors shared through a repository? Answered: the allow-list (§2).
- **Inversion:** between every sensor as compiled code (one build, no runtime surprises) and every sensor as an external script (open, unvalidated, quiet on failure). The decision keeps compiled code only where a poll needs it and puts everything else in validated data.

## Context

attend observes the session's surroundings through sensors and wakes the agent when something changes. Each built-in sensor is code in attend's build. Its defaults are rows of `SENSOR_DEFAULTS` in `attend-config/src/schema.rs`, its name is in `BUILTINS` beside them, and `Config::from_layers` fills every sensor from those rows. `register_builtin!` repeats three of the values as literal fallbacks, and `peers` is registered by hand outside the macro. Adding a sensor means a row, a name, a registration, a cargo feature or module, and a release.

The one escape is the script sensor. It is configured inline in `config.yaml` or `.claude/attend.yaml`, runs under `bash`, and reports `magnitude|description` lines. It has no identity of its own, no state between polls and no choice of lane. It reports a missing script, an exec failure, a timeout and a read failure on stderr, where no surface shows them, and it drops a non-zero exit or a malformed line without a trace. The lane a sensor rides is chosen by name in code.

The settings tab already shows attend's sensors, and each can be edited outside the TUI through `ways settings` and its object mode (ADR-503). A new model has to keep that, for every kind.

Some observations are worth having that no compiled sensor will ever cover: a D-Bus property, a systemd unit, the weather, a database row, the unread messages behind an MCP server the agent already uses. Each needs a different source, and none should need a change to attend.

"Sensor" already names something else in this corpus: ADR-103's checks are epoch-distance-aware confidence sensors for ways. This record's sensors are a different thing. The session-facing sensors of ADR-199 are called "ways sensors" throughout.

## Decision

### 1. One spec: a shared core and a block per kind

A sensor is a YAML file conforming to a versioned sensor spec. Every file, of every kind, carries the shared core:
- **identity:** `name`, `description`, `kind`, the spec version, and for a compiled sensor its source and version;
- **file layout and layering:** one file per sensor, layered by name (§2);
- **`where` conditions:** the condition object of §3, used by attend's triggers and by ways' rules;
- **`enabled`**, which defaults to true;
- **management:** the settings keys, the copy, rename and delete flow, and the CLI of §5 and §6.

Each kind adds its own block, and its host owns it:

| Kind | Host | The poll or source | The kind's block |
|---|---|---|---|
| `crate` | attend | compiled code, as a crate or an attend module | the crate's own fields, such as `processes`' `watch` |
| `command` | attend | an argv run directly (no shell), with optional `env`, `cwd` and `timeout` | `output: json` or `output: lines` (the `magnitude|description` form) |
| `mcp` | attend | one tool call on a stdio MCP server, with fixed `args` | `server`, `tool`, `args`, the probe record (§4) |
| `ways` | ways | the session's own hook stream | `rules` and `dispatch`, defined by ADR-199 |

The three attend kinds also share attend's block:
- **cadence:** `interval`, `min_interval`, `decay_threshold`;
- **emission:** `threshold` and `lane` (§3a);
- **permissions:** `requires`, the tool permissions the sensor needs (ADR-116);
- **triggers** (§3);
- **state:** the declared state fields (§3b);
- **failure handling:** a timeout, non-zero exit, malformed output, exec failure or auth failure is a recorded sensor state shown by `attend sensor list` and the settings tab, never a silent drop and never only a line on stderr.

A compiled sensor keeps only the code a poll needs. `SENSOR_DEFAULTS` and `BUILTINS` move out of `attend-config/src/schema.rs` into shipped sensor files, the literal fallbacks in `register_builtin!` go, and `peers` is registered from its file like the others. A built-in sensor that a `command` file can express moves out of attend's code into a shipped file. `git` is the first candidate; its file records the reason if it stays compiled.

### 2. YAML, layered, and validated

Sensor files are YAML. The settings registry, its layering and its per-field provenance (ADR-503) operate on YAML files, so sensor files join them without a second model. `serde_yaml`'s deprecation (#498) applies to all of them alike.

attend's files layer by sensor name:
- **shipped:** embedded in the binary and projected to `$XDG_DATA_HOME/agent-ways/attend/sensors/`;
- **user:** `$XDG_CONFIG_HOME/attend/sensors/<name>.yaml`;
- **project:** `.claude/attend/sensors/<name>.yaml`.

A later layer overrides any field of an earlier one, triggers included. A new name adds a sensor, and `enabled: false` turns one off. attend picks up added or changed files without a restart. ways sensor files live in ways' own config root (ADR-199 §1); attend never reads them, and the ways hook path never reads attend's (ADR-503 §5).

A project file of kind `command` or `mcp` runs code chosen by whoever wrote the repository. It loads only when the project is listed in `attend.project_sensors`, a user-scope key in `$XDG_CONFIG_HOME/attend/config.yaml` that a project file can't set. Otherwise it is reported as refused. ways keeps its own list for ways sensors (ADR-199 §7), so each host's trust is in its own file. Project macro trust (#825) is settled in the same shape.

### 3. Triggers fire on transitions

An attend sensor declares one or more triggers. A trigger fires only on a change between two polls, never while a condition merely holds:
- `name`: unique within the sensor, so the settings key path can address it;
- `path`: where to look in the poll result, as a JSONPath subset;
- `key`: for a collection, the field that identifies an item across polls;
- `on`: `added`, `removed` or `changed` against the previous poll, or `enters`/`leaves` the `where` condition;
- `where`: a condition object over one or more properties, each mapped to a value or an operator, combined by `match: all` (the default) or `match: any`;
- `state`: in place of `path`, `key` and `on`, a map from declared state fields (§3b) to `changed`, or for a number `rises` or `falls`, combined by `match`;
- `magnitude` and `description`, a template filled from the item's fields and the sensor's state (§8).

Wildcards apply to paths, to property names (`"wind_*"` tests every matching field) and to values (a bare string with glob characters is a glob; an explicit operator forces glob, regex or exact matching).

attend stores what each trigger last saw across polls and restarts: the value, the key set, or whether `where` held. That stored state is what lets a stateless command or a read-only MCP call report transitions. A sensor's trigger memory is deleted with its state store (§3b).

### 3a. Lanes

ADR-136 draws the lane boundary at authored communication: words a person or agent composed ride the message lane, and everything the environment generates rides the event lane. A sensor file states its lane, which replaces selection by name in `rides_message_lane`:
- `lane` defaults to `event`.
- `lane: message` requires an `author` field in the trigger, naming the item field, or on a state trigger the state field (§3b), that holds who composed the item. A file that declares the message lane with no author is a lint error, and attend runs that sensor on the event lane and says why.
- The shipped `keepwarm` file is the one exception, recorded in the file: its timed wake floor rides the message lane by ADR-182, and it has no author.

The author requirement is chosen over restricting who may set the lane. A lane granted by file origin says nothing about the items, while an author field is checkable and is what ADR-136's boundary means.

An MCP sensor that reads chat messages people wrote, such as Slack through an MCP server, carries authored communication, so it may ride the message lane. This amends ADR-136's Decision in one clause only: the closing sentence of its item 1, which named "a future external-chat sensor (e.g. Slack)" among the event-lane sensors. That sensor now rides the message lane for authored chat messages. The rest of ADR-136's Decision stays in force. Chat metadata with no author, such as a channel created or a member joined, stays on the event lane.

The lane is per sensor, which is a step toward #139 and not its fix. `peers` still emits both authored messages and presence events from one poll, and #139's per-observation lane selection, by splitting message scanning out of `peers`, stays open.

### 3b. Sensor state: declared, typed, checked

An attend sensor may declare a `state` block: a map from field name to a type, `number`, `string` or `boolean`. A string field carries a `max` length in characters. There is no object, array or null type, and no nesting. A new field starts at `0`, `""` or `false`. §8 holds the bounds.

Only attend writes the store, and in a project only its poller does. It keeps one JSON file per sensor under its state directory, `$XDG_STATE_HOME/agent-ways/attend/sensor-state/`, named by a hash of the sensor's name and the project attend runs for, so no part of the path comes from a sensor file. Every attend sensor has a store, empty when it declares no state. Its trigger memory sits beside the store, outside the declared fields, and is deleted whenever the store is. Trigger memory is attend-internal: no template reads it, and the declaration check does not cover it. No template field, argument or environment variable gives the path to a command, a server or the agent.

One attend per project polls. The first attend to start in a project takes a lock in its state directory and becomes the project's poller: it runs every sensor, holds the project's MCP sessions (§4) and is the only writer of the project's stores. Every other attend in the project reads the poller's results and delivers them to its own session. The poller holds the lock for the life of its process, so the lock is released when the poller exits for any reason, and another instance takes it at its next tick and polls on from the stores as they stand.

A sensor fills its fields in two ways:
- **Mappings.** A field's `from` is a JSONPath of the §8 subset over the poll result, and must select one scalar. A path that selects nothing leaves the field as it was. `mcp` and `command` sensors both map.
- **Emitted fields.** A `command` sensor with `output: json` may put a `state` object at the top of its output, with a value for any declared field that has no `from`. A field it leaves out keeps its value, and a value for a field that has a `from` refuses the whole write.

Every write is checked whole against the declaration:
- every field name is declared;
- no emitted value is for a field that has a `from`;
- every type is exact, with no coercion: a number is a finite JSON number, a boolean is `true` or `false`, and the string `"3"` is a string;
- every string is within its `max`, and the serialized state within the §8 size;
- no string holds a control or format character (Unicode Cc or Cf). The check rejects such a string, so a stored string is always what the source sent.

A write that fails any check is refused whole. The previous state stays, the state triggers see no change on that poll, and the refusal is a recorded sensor state shown by `attend sensor list` and the settings tab (§1), naming the field and the check.

Stores persist across restarts, as trigger memory does. When attend starts, and whenever a sensor file's `state` declaration changes, it checks each stored file against the current declaration with the same check as a write. A file that fails is discarded whole, its sensor starts from a fresh baseline, and the discard is a recorded sensor state.

At start attend also deletes two kinds of store:
- **orphaned:** the store of a sensor that no longer has a file in any layer, after a delete or a rename. A disabled sensor keeps its store, and turning it back on resumes from its last state.
- **expired:** a store not written for longer than the retention limit of §8. Every successful poll rewrites its sensor's store, so the age is the time since the sensor last polled. A sensor disabled for longer than the limit loses its state.

An expired or discarded store is recorded as its sensor's state. A sensor whose file is gone has no row of its own, so `attend sensor list` shows each orphan deletion as a line until the next start. A sensor whose store was discarded or expired sets a baseline on its first poll and fires nothing.

A string inserted into a description is still sensor output. The typed bounds limit its size and shape, and attend's framing carries what it says to the model as data. Whichever lane it rides, a filled description is made one line, with every `<` that could forge a frame's tags replaced by `‹`, by the rule `defuse_fence` in `tools/attend/src/cmd/inbox.rs` and `defuseFence` in `plugins/attend/hooks/lib.ts` share. On the message lane it is then delivered inside attend's message frame (ADR-406 item 4).

A Slack sensor through an MCP server, counting unread messages and describing the newest:

```yaml
name: slack-unread
description: Unread messages in Slack
kind: mcp
lane: message
server: slack
tool: list_unread
args: { channel: general }
state:
  unread:  { type: number, from: "$.unread_count" }
  last_ts: { type: string, max: 32, from: "$.messages[0].ts" }
  user:    { type: string, max: 64, from: "$.messages[0].user.name" }
  text:    { type: string, max: 1024, from: "$.messages[0].text" }
triggers:
  - name: new-message
    state: { unread: rises, last_ts: changed }
    match: any
    author: user
    magnitude: 1
    description: "the last message in slack mcp from {state.user} at {state.last_ts} says {state.text}"
```

A poll that raises `unread` and changes `last_ts` fires the trigger once. The plainest form of the same trigger sets `description: go check new messages in slack mcp` and declares only `unread` and `last_ts`.

### 4. MCP sensors: stdio, read-only, attend-side diff

An `mcp` sensor's `server` names a server from Claude Code's MCP configuration (user `~/.claude.json`, project `.mcp.json`), or defines one inline with `command`, `args` and `env`. The project's poller (§3b) starts one stdio instance of that server for the project, holds one long-lived session per server shared by every sensor that uses it, and restarts it on failure.
- **Transport:** stdio only. Remote servers wait for #845, where OAuth must refresh without an interactive login.
- **Read-only, checked:** the MCP specification makes `readOnlyHint` advisory and untrusted, so the hint is necessary but not sufficient.
  - The tool must declare `readOnlyHint: true`. A file may set `allow_unannotated: true`, which `attend sensor list` and the settings tab show as an override.
  - `attend sensor probe <name>` calls the tool twice in a row and checks whether the second call returned every item of the first, by the trigger's `key`. This catches a side effect that changes the result set, such as a cursor that advances or a listing that filters out what was read. It can't catch read state the server sets while the result stays the same, such as a message marked read on the service. A sensor whose triggers are all state triggers is compared on the state each call would write.
  - For that, the probe then asks the operator to confirm in another client (the service's own app, for example) that the items it returned are still unread or otherwise untouched, and records the answer.
  - The probe record holds both results. A sensor with no record, a failed call comparison or no operator confirmation stays disabled. The record names the server, tool and args it was taken with, and a change to any of them voids it.
- **Diff on attend's side:** attend computes the delta itself, through triggers keyed on an item field. It never relies on a server cursor that advances when read.
- **Auth:** attend never stores credentials or starts an auth flow. Its instance reads the server's own token store, and a missing or expired credential is a sensor failure, never a prompt.
- **Delivery:** events ride the same lanes and conduits as every other sensor (Monitor and the Stop-hook drain, ADR-172). A notification names the server and the item handle, so the agent can fetch it with its own MCP tools. The MCP channel route (ADR-501) is a later choice.

### 5. Two surfaces, one model

Every field of every attend sensor and trigger has a settings key: `attend.sensors.<name>.<field>`, for example `attend.sensors.weather.triggers.high-wind.where.wind_speed_10m.above`. ways sensors have their own prefix, `ways.sensors.<sensor>.rules.<rule>.<field>` (ADR-199 §9). A key resolves through the layering, so:
- `ways settings get|set|unset|help|list --json` reads and writes the right file, with provenance;
- `set` refuses a value that would make the sensor invalid, and says why;
- `help` reads the spec's schema.

This amends ADR-503's Decision in three places, and leaves the rest in force:
- **§1, build-time composition:** `ways-cli` composes the registries at build time from fixed schemas. Sensor sections are instead discovered at runtime, one per sensor file found in the layers. The sensor kinds' key shapes stay declared in the schema.
- **§3, the layers:** a shipped layer sits below the user layer for sensor keys.
- **§8 and `file_of`, where `set` writes:** `file_of` in `ways-cli` maps each file kind to one path per layer. A sensor key's file instead depends on the sensor name and the layer being written: `set` writes the sensor's user or project file, creating it with only the changed field when the sensor comes from a lower layer.

Sensor files are managed with the same named-item flow the theme tab uses: copy any sensor to a new name, rename it, or delete it. Shipped files are bundled, so they can be copied (the usual way to start a variant) but not renamed or deleted; a user or project override of one can be. Each action has its CLI form (ADR-504 §10): `attend sensor copy|rename|delete`, and `ways sensors copy|rename|delete` for ways sensors.

The sensors tab (ADR-503, ADR-504) is a view over the same commands. It shows both prefixes, `attend.sensors` and `ways.sensors`: the tab's single `prefix` in `tools/ways-cli/src/cmd/settings/tui/build.rs` becomes a list, and the `ways` tab skips `ways.sensors`. The tab lists every sensor of every kind with its source file and live state, toggles a sensor or trigger, edits fields in a form, and previews a change with a dry run. Anything one surface can do, the other can.

### 6. Test from the CLI

- `attend sensor poll <name>` runs one real poll and prints the result and each trigger's verdict.
- `attend sensor probe <name>` runs the read-only probe of §4, asks for the operator's confirmation, and writes its record.
- `attend sensor fire <name>[/<trigger>] [--input FILE|-]` sends a synthetic result through the real trigger evaluation, lane and delivery. The notification it produces is marked `[test]`, with `test: true` in the record. ways uses the same `--input` flag (ADR-199 §9).
- `--dry-run` on poll or fire stops before delivery.
- A test fire leaves trigger state and the state store untouched unless `--commit` is given. Its input passes the state check of §3b as a real poll's result does.

### 7. Where the spec lives, and what it replaces

The spec's types, validation and schema live in a new crate, `sensor-spec`: the shared core, attend's block and the ways block. attend and ways both depend on it, and ways sensors run without attend. `sensor-trait` keeps the runtime trait that compiled sensors implement.

The `attend.sensors.*` keys in `$XDG_CONFIG_HOME/attend/config.yaml` and in a project's `.claude/attend.yaml`, and the inline script form, are replaced by sensor files with no legacy reader. The user base is small, and a reader for the old keys would keep a second sensor model alive in attend-config for every later change to the spec. Instead, `ways settings lint` reports each old key it finds, with the sensor file to write and the field to set there, and the release notes say so.

This amends ADR-117. Its two sensor paths (crate and script, where a crate sensor wins a name clash) become the kinds of §1, where a name is one sensor whose layers merge. Its control plane moves from `attend.yaml`'s sensor section to sensor files. Feature flags stay the build-time control for compiled code, and a sensor registers from its shipped file, not from a call site.

### 8. Initial values

The spec owns these and may change them in a later version without a new decision:
- the JSONPath subset: `$`, `.field`, `[n]`, `[*]`, `..`;
- the `where` operators: `equals`, `not`, `in`, `contains`, `like`, `matches`, `above`, `below`, with no nesting;
- template fields: `{field}` and `{value}`; nested item fields such as `{message.user.name}`; `{state.<field>}`; `{prev.<field>}`, the value before this poll's write; and `{trigger.count}`, the number of items the trigger saw change on this poll. `state.`, `prev.` and `trigger.` are reserved prefixes;
- the glob characters: `*`, `?`, `[…]`;
- a `command` sensor's default timeout: 10 s, as the script sensor has today;
- sensor state: at most 16 fields, each named `[a-z][a-z0-9_]*` in at most 32 characters; a string's `max` defaults to 256 characters and may be at most 1024; a number is a finite 64-bit float; the serialized state is at most 4 KiB, and `ways settings lint` flags a declaration whose maximums could exceed it;
- the retention limit for a sensor's store: 30 days since its last write.

## Consequences

### Positive

- A new sensor is a file. It needs no feature flag, registration or release, and the sensors skill (#847) can author, lint, poll, probe and test-fire one through the CLI alone.
- Compiled sensors become tunable per user and per project without code changes.
- Failed polls become visible states instead of stderr lines or silent drops.
- The lane is declared and checked per sensor, and the lane rule is written down where a file author meets it.
- A sensor can carry counts and the newest item's fields from poll to poll, and its triggers and descriptions read them, with no state handling in the command or server.
- A hostile server or command can write at most the declared fields within their bounds. Nothing it writes changes what attend runs, where attend writes, or how a sensor is configured.
- A refused write, a discarded or expired store and an orphan deletion each show in `attend sensor list`.

### Negative

- The spec is a new contract to version. A field added later must keep older files valid.
- Long-lived MCP sessions are a new resource for attend to supervise.
- An MCP sensor runs a second instance of a server Claude Code may already be running, with the same credentials. Both instances count against the service's rate limits, and a server that locks its token store may refuse the second.
- An MCP sensor needs a probe before it runs, which adds a step and fails for a tool that can't be read twice without side effects.
- Project command and MCP files need the allow-list before they run, which adds a step for shared sensors.
- An install with sensor settings in `config.yaml` or `.claude/attend.yaml` loses them on upgrade until the operator writes the files lint names.
- A sensor that wants richer memory must flatten it into at most 16 scalar fields.
- A string field holds whatever text the source sent within its bounds. What that text says is guarded only by the framing.
- A store that expires, or fails the check after its declaration changes, re-baselines its sensor, and an item that arrived in the gap never fires.

### Neutral

- `sensor-spec` is a new workspace crate.
- The settings registry gains per-file sensor sections alongside its fixed keys.
- `docs/attend-and-monitor/authoring-sensors.md` and `sensors.md` are rewritten against the spec.
- #139 stays open.
- Trigger memory and state stores survive restarts, so an item that arrived while attend was down fires on the first poll after start, however long ago it arrived.

## Alternatives Considered

- **TOML files.** Rejected: TOML is already a dependency (agent-theme reads themes in it), but the settings registry, its layering and its provenance operate on YAML files. TOML sensor files would need a second loader and writer in the registry.
- **Keep the script sensor and add fields to it.** Rejected: it would leave the defaults of compiled sensors in code, keep the shell wrapper, and give no shared model for the settings surfaces.
- **A wrapper script for MCP calls.** Rejected: every sensor would re-implement the session, auth handling and result parsing, and the read-only gate would be unenforceable.
- **Trust `readOnlyHint` alone.** Rejected: the MCP specification calls every tool annotation a hint and tells clients not to base tool-use decisions on annotations from untrusted servers, and a wrong hint on a chat server marks messages read before the agent sees them.
- **Restrict `lane: message` to shipped files.** Rejected: an MCP chat sensor is a user file, and file origin says nothing about whether its items are authored.
- **A legacy reader for the old keys.** Rejected for the reasons in §7.
- **A state file the sensor writes itself.** Rejected: a command or server given a path can write anything there, and attend could check the file only after it was written.
- **Wipe every store at start.** Withdrawn by the operator. Each start would re-baseline every sensor, and an item that arrived while attend was down would never fire. The check at load and the retention limit bound what a store can hold over time.
- **Streaming processes (`dbus-monitor`, `journalctl -f`) in this version.** Deferred: a poll per run covers the cases in #843, and streaming adds process supervision. It returns when a sensor needs sub-interval latency.
- **Remote MCP servers now.** Deferred to #845 by operator decision, until OAuth refresh needs no interactive login.

## Addendum, 2026-10-29: §2 and §5 amended by ADR-516

Appended after acceptance; nothing above is changed. ADR-516 amends §2 and §5, which call the lowest sensor layer `shipped` and its files `bundled`. That layer's sensors report the origin `builtin`, and a key one of its files sets reports `layer: builtin`. A user or project file for a shipped sensor's name reads as that builtin sensor with an `extends` override naming the file.
