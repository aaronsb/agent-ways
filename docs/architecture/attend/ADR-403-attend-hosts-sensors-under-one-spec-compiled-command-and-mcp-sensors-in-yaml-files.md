---
contract: adr/v1
kind: decision
verb: change
capability: attend
basis:
  - operator: aaronsb
    level: directed
    said: "I want to be more formal in the specification for sensors, and improve the specification to externalize the configuration for sensors. There are then two kinds of sensors - the kinds that are compiled right into attend, that follow the crate spec (our existing sensors) and a different style of sensors that follows a toml configration spec. The toml files each specify one sensor, and are launched at runtime of attend. This way I could write a sensor that queries anything - dbus, or system state, or honestly anything."
    via: session 2026-10-03, filing #843
  - operator: aaronsb
    level: directed
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
    level: directed
    said: "right now section 6 of 'ways settings' lets us review the existing sensors for attend, and they can be edited fully outside the tui through the structured object configuration path"
    via: session 2026-10-03, on reviewing every sensor kind the same way
  - operator: aaronsb
    level: directed
    said: "we should be able to through the cli invoke or trigger an event for testing too"
    via: session 2026-10-03
  - evidence: "attend today: compiled sensors implement sensor_trait::Sensor and are wired by cargo feature plus register_builtin! with hard-coded defaults (tools/attend/src/sensors/mod.rs); script sensors are configured inline as attend.sensors.<name>.script, run under bash with a 10 s timeout and a magnitude|description stdout contract, and drop malformed output silently (tools/attend/src/sensors/script.rs); the lane is chosen by sensor name in rides_message_lane (tools/attend/src/cmd/run/tick.rs)"
  - evidence: "the workspace already parses and writes YAML through serde_yaml in attend-config and agent-settings; toml is used only by agent-theme"
  - precedent: ADR-503
  - precedent: ADR-506
agent:
  name: Claude
  model: claude-opus-5-5
observable:
  - 'see: attend sensors lists a shipped git.yaml, a user weather.yaml (kind: command, Open-Meteo) and a user slack-unread.yaml (kind: mcp), each with its source file and last poll state'
  - 'run: attend sensors fire weather/high-wind --input sample.json produces a [test] notification on the event lane and leaves the trigger''s stored state unchanged'
  - 'run: ways settings set attend.sensors.weather.triggers.high-wind.where.wind_speed_10m.above 40 writes the user file, and ways settings list --json attend.sensors.weather names that file as the source'
  - 'see: an mcp sensor on a tool without readOnlyHint is refused, and attend sensors says why'
  - 'see: a project sensor file of kind command is reported as refused until the project is on the allow-list'
status: proposed
date: 2026-10-03
deciders:
  - aaronsb
related:
  - ADR-136
  - ADR-172
  - ADR-182
  - ADR-501
  - ADR-503
  - ADR-504
  - ADR-506
---

# ADR-403: Attend hosts sensors under one spec: compiled, command and MCP sensors in YAML files

## Summary

- **Decided:** attend becomes a sensor host. Every sensor, compiled or declarative, is defined by one YAML file under one versioned spec covering identity, cadence, emission, lane, triggers and failure handling. There are three kinds: `crate` (compiled poll code, everything else in the file), `command` (an argv and an output format) and `mcp` (a read-only tool on a stdio MCP server). A trigger is a small condition object over the poll result, with wildcards, that fires on transitions. `ways settings` and its key path reach every sensor, trigger and field, and the CLI can poll and test-fire any sensor.
- **Trades away:** the inline `attend.sensors.<name>.script` form and the `attend.sensors.*` keys in `config.yaml` are replaced, with no legacy reader. Compiled sensors lose their hard-coded defaults. Remote MCP servers wait for #845.
- **One-way?** No. Sensor files are plain data, and the compiled sensors keep their poll code.
- **Probes:** *Confident (host):* attend is a host and sensors are data it loads, so adding a sensor needs no change to attend. *Not confident (project-trust):* a project's own command and MCP sensor files load only after you list that project in your user config. Is that the gate you want for sensors shared through a repository?
- **Inversion:** between every sensor as compiled code (one build, no runtime surprises) and every sensor as an external script (open, unvalidated, silent on failure). The decision keeps compiled code only where a poll needs it and puts everything else in validated data.

## Context

attend observes the session's surroundings through sensors and wakes the agent when something changes. Each sensor today is code in attend's build: its interval, minimum interval and threshold sit in a `register_builtin!` call, and adding one means a cargo feature and a release. The one escape is the script sensor. It is configured inline in `config.yaml`, runs under `bash`, and reports `magnitude|description` lines. It has no identity of its own, no state between polls, no choice of lane, and it drops a failed poll without a trace. The lane a sensor rides is chosen by name in code.

Some observations are worth having that no compiled sensor will ever cover: a D-Bus property, a systemd unit, the weather, a database row, the unread messages behind an MCP server the agent already uses. Each needs a different source, and none should need a change to attend.

## Decision

### 1. One spec, three kinds, one file per sensor

A sensor is a YAML file conforming to a versioned sensor spec. The file declares:
- **identity:** `name`, `description`, the spec version, and for a compiled sensor its source and version;
- **cadence:** `interval`, `min_interval`, `decay_threshold`;
- **emission:** `threshold`, and `lane` (`event` or `message`), which replaces selection by name (#139);
- **triggers** (§3);
- **failure handling:** a timeout, non-zero exit, malformed output, or auth failure is a recorded sensor state shown by `attend sensors` and the settings tab, never a silent drop.

The three kinds differ only in where a poll's data comes from:

| Kind | The poll | The file adds |
|---|---|---|
| `crate` | compiled code, as a crate or an attend module | nothing beyond the spec |
| `command` | an argv run directly (no shell), with optional `env`, `cwd` and `timeout` | `output: json` or `output: lines` (the `magnitude|description` form) |
| `mcp` | one tool call on a stdio MCP server, with fixed `args` | `server`, `tool`, `args` |

A compiled sensor keeps only the code a poll needs. Its defaults move from `register_builtin!` to a shipped file. A built-in sensor that a `command` file can express moves out of attend's code into a shipped file. `git` is the first candidate; its file records the reason if it stays compiled.

### 2. YAML, layered, and validated

Sensor files are YAML. attend's config, `ways.yaml` and the settings registry already parse and write YAML, so the settings layering and per-field provenance apply without a new parser. `serde_yaml`'s deprecation (#498) applies to all of them alike.

Files layer by sensor name: shipped (embedded in the binary and projected to `$XDG_DATA_HOME/agent-ways/attend/sensors/`) → user (`$XDG_CONFIG_HOME/attend/sensors/<name>.yaml`) → project (`.claude/attend/sensors/<name>.yaml`). A later layer overrides any field of an earlier one, triggers included. A new name adds a sensor, and `enabled: false` turns one off. attend picks up added or changed files without a restart.

A project file of kind `command` or `mcp` runs code chosen by whoever wrote the repository. It loads only when the project is listed in the user config's sensor allow-list; otherwise it is reported as refused. That allow-list is settled together with project macro trust (#825), and both use one mechanism.

### 3. Triggers fire on transitions

A sensor declares one or more triggers. A trigger fires only on a change between two polls, never while a condition merely holds:
- `name`: unique within the sensor, so the settings key path can address it;
- `path`: where to look in the poll result. A JSONPath subset: `$`, `.field`, `[n]`, `[*]`, `..`;
- `key`: for a collection, the field that identifies an item across polls;
- `on`: `added`, `removed` or `changed` against the previous poll, or `enters`/`leaves` the `where` condition;
- `where`: a condition object over one or more properties. Each property maps to a value or an operator: `equals`, `not`, `in`, `contains`, `like`, `matches`, `above`, `below`. `match: all` (the default) or `match: any` combines the properties, and there is no nesting in this version;
- `magnitude` and `description`, a template filled from the item's fields (`{field}`, `{value}`).

Wildcards apply at three levels:
- **paths:** `[*]` and `..`;
- **property names:** `"wind_*"` tests every matching field;
- **values:** a bare string with `*`, `?` or `[…]` is a glob, `like` is an explicit glob, `matches` is a regex, and `equals` forces an exact match.

attend stores what each trigger last saw across polls and restarts: the value, the key set, or whether `where` held. That stored state is what lets a stateless command or a read-only MCP call report transitions.

### 4. MCP sensors: stdio, read-only, attend-side diff

An `mcp` sensor's `server` names a server from Claude Code's MCP configuration (user `~/.claude.json`, project `.mcp.json`), or defines one inline with `command`, `args` and `env`. attend holds one long-lived session per server, shared by every sensor that uses it, and restarts it on failure.
- **Transport:** stdio only. Remote servers wait for #845, where OAuth must refresh without an interactive login.
- **Read-only:** the tool must declare `readOnlyHint: true`. A file may set `allow_unannotated: true`, which `attend sensors` and the settings tab show as an override.
- **Diff on attend's side:** attend computes the delta itself, through triggers keyed on an item field. It never relies on a server cursor that advances when read.
- **Auth:** attend never stores credentials or starts an auth flow. The server keeps its own token, and a missing or expired credential is a sensor failure, never a prompt.
- **Delivery:** events ride the same lanes and conduits as every other sensor (Monitor and the Stop-hook drain, ADR-172). A notification names the server and the item handle, so the agent can fetch it with its own MCP tools. The MCP channel route (ADR-501) is a later choice.

### 5. Two surfaces, one model

Every field of every sensor, trigger and rule has a settings key: `attend.sensors.<name>.<field>`, for example `attend.sensors.weather.triggers.high-wind.where.wind_speed_10m.above`. The key resolves through the layering, so:
- `ways settings get|set|unset|help|list --json` reads and writes the right file, with provenance;
- `set` refuses a value that would make the sensor invalid, and says why;
- `help` reads the spec's schema.

The sensors tab (ADR-503, ADR-504) is a view over the same commands. It lists every sensor of every kind with its source file and live state, toggles a sensor or trigger, edits fields in a form, and previews a change with a dry run. Anything one surface can do, the other can.

### 6. Test from the CLI

- `attend sensors poll <name>` runs one real poll and prints the result and each trigger's verdict.
- `attend sensors fire <name>[/<trigger>] [--input FILE|-]` sends a synthetic result through the real trigger evaluation, lane and delivery. The notification it produces is marked `[test]`, with `test: true` in the record.
- `--dry-run` on either stops before delivery.
- A test fire leaves trigger state untouched unless `--commit` is given.

### 7. Where the spec lives

The spec's types, validation and schema live in a new crate, `sensor-spec`. attend and ways both depend on it: ways sensors (ADR-199) follow the same file layout, layering and conditions, and run without attend. `sensor-trait` keeps the runtime trait that compiled sensors implement.

The `attend.sensors.*` keys in `config.yaml` and the inline script form are replaced by sensor files, with no legacy reader (ADR-506). The release notes say so.

## Consequences

### Positive

- A new sensor is a file. It needs no feature flag, registration or release, and the sensors skill (#847) can author, lint, poll and test-fire one through the CLI alone.
- Compiled sensors become tunable per user and per project without code changes.
- Failed polls become visible states instead of silent drops.
- The lane is declared per sensor, which closes #139.

### Negative

- The spec is a new contract to version. A field added later must keep older files valid.
- Long-lived MCP sessions are a new resource for attend to supervise.
- Project command and MCP files need the allow-list before they run, which adds a step for shared sensors.

### Neutral

- `sensor-spec` is a new workspace crate.
- The settings registry gains per-file sensor keys alongside its fixed keys.
- `docs/attend-and-monitor/authoring-sensors.md` and `sensors.md` are rewritten against the spec.

## Alternatives Considered

- **TOML files.** Rejected: YAML is what every other settings file here uses, so the layering and provenance code applies unchanged.
- **Keep the script sensor and add fields to it.** Rejected: it would leave the defaults of compiled sensors in code, keep the shell wrapper, and give no shared model for the settings surfaces.
- **A wrapper script for MCP calls.** Rejected: every sensor would re-implement the session, auth handling and result parsing, and the read-only gate would be unenforceable.
- **Streaming processes (`dbus-monitor`, `journalctl -f`) in this version.** Deferred: a poll per run covers the cases in #843, and streaming adds process supervision. It returns when a sensor needs sub-interval latency.
- **Remote MCP servers now.** Deferred to #845 by operator decision, until OAuth refresh needs no interactive login.
