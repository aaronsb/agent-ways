---
contract: adr/v1
kind: decision
verb: add
capability: config
amends: [ADR-503#decision]
basis:
  - operator: aaronsb
    level: directed
    said: "let's add one more sensor class, which is a ways sensor. it can observe the session and send known sections of the stream to different destinations: copy to an endpoint, or a json endpoint, or even call a command with the contents of the selected known sections. For instance, if I wanted to write down every tool call into an independent audit log"
    via: session 2026-10-03, filing #846; followed by "or every explaination or response, or every user question"
  - operator: aaronsb
    level: directed
    said: "it's a special sensor class for ways"
    via: session 2026-10-03
  - operator: aaronsb
    level: directed
    said: "this way the ways sensor doesn't /actually/ need attend to be running."
    via: session 2026-10-03
  - operator: aaronsb
    level: directed
    said: "the point of the ways sensor is to be able to externally and independently send agent calls and actions (and human calls) to another destination, where it can be processed independently of ways and the agent loop entirely. the one thing that we have to be careful of is it needs to probably have a way to async dispatch the command, and have a watchdog/timeout to kill the dispatch if it fails within a defined time (like say, the endpoint that was configured doesn't respond or is slow - we don't want to halt everything because of this). perhaps having it blocking or not can be a toggle. multiple 'rules' for agent sensors can coexist, so technically the same criteria could be matched multiple times and dispatched to different external consumers"
    via: session 2026-10-03
  - operator: aaronsb
    level: directed
    said: "this way our settings ui can easily review the rules, enable/disable them and hopefully, provide a tui to edit them too"
    via: session 2026-10-03
  - operator: aaronsb
    level: directed
    said: "we should be able to through the cli invoke or trigger an event for testing too"
    via: session 2026-10-03
  - evidence: "settings.json wires PreToolUse only for Edit|Write, Bash, TaskCreate and Task, PostToolUse and PostToolUseFailure only for Edit|Write|Bash|Task, and UserPromptSubmit, Stop, SubagentStart, SessionStart and TaskCreated; there is no SubagentStop, SessionEnd or PreCompact hook. ways hook <event> (tools/ways-cli/src/cmd/hook/input.rs) handles prompt, state, command, file, task, post-tool, queued, stop, subagent-start, session-start and tasks-active, with no SubagentStop event. Stop's payload carries the transcript path, from which the reply text is read; ways keeps per-agent session records since PR #818"
  - evidence: "ADR-504 §11 sets the latency method for the commands hooks run: hyperfine, 20 warm-up and 200 measured runs, a fixed fixture input, HOME and XDG_* pointing at a fixture install, no agent daemon running"
  - precedent: ADR-503
  - precedent: ADR-504
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Mask by default (Recommended)"
    via: session 2026-10-03, selected from agent-written options on redaction defaults for audit logs
    covers: [redaction-default]
observable:
  - 'run: with attend not installed, a tool-audit ways sensor writes every tool call and result of the main agent and its subagents, Read and Grep included, to one JSONL file, in sequence, with no gaps'
  - 'run: ways sensors fire tool-audit/bash-policy --section tool_call against an endpoint that never answers is killed at its timeout, and the hook returned before it'
  - 'run: three blocking rules with 3 s, 8 s and 30 s timeouts matching one tool call against endpoints that never answer hold that hook event for 10 s, not 41 s'
  - 'see: three rules matching one record dispatch three times, and a consumer that fails leaves the other two delivered'
  - 'run: hyperfine by the ADR-504 §11 method on ways hook sensors with a PreToolUse Read fixture and no ways sensor files shows a median within 2 ms of ways --version'
  - 'run: the same measurement with 20 ways sensor files and a warm compiled index, one async rule selecting tool_call, shows a median within 5 ms of ways --version'
  - 'run: interrupt a reply mid-turn under a recording ways sensor and check whether a Stop event reached ways; the result decides how §3 reads an interrupted reply'
  - 'run: remove a project from ways.project_sensors, and the next hook event in that project dispatches nothing from its project sensor files'
  - 'see: the ways settings sensors tab lists a ways sensor''s rules with their dispatched, timed out, dropped and spooled counts, and toggling one writes enabled: to the user file'
status: proposed
date: 2026-10-03
deciders:
  - aaronsb
related:
  - ADR-103
  - ADR-181
  - ADR-403
  - ADR-502
---

# ADR-199: Ways sensors dispatch session activity to external consumers, async under a watchdog

## Summary

- **Decided:** a fourth sensor kind, the ways sensor (`kind: ways`), is hosted by ways and faces inward. It observes the session itself (what the human asks, what the agent says, what the agent does) and dispatches selected records to external consumers (a file, an HTTP endpoint, a command), which process them outside ways and the agent loop. To see all of that, ways widens its hook wiring to every tool and to SubagentStop, SessionEnd and PreCompact, behind a fast path that exits at once when no ways sensor wants the event. A sensor file holds named rules, and overlapping rules fan out to separate consumers. Dispatch is async by default, through a detached per-session dispatcher, and each dispatch has a watchdog timeout that kills it. Blocking is a per-rule toggle, and one hook event waits at most 10 s however many blocking rules match. It runs whether or not attend does.
- **Trades away:** every tool call, subagent stop, session end and compaction now starts a `ways` process, even with no ways sensor configured. An `http` or forwarding `command` rule sends session content off the machine, and a blocking rule can delay a hook event by up to 10 s.
- **One-way?** No. Removing a sensor file stops the dispatch, and removing the wiring stops the hook calls. Records already delivered stay with their consumer.
- **Probes:** *Confident (no-halt):* no consumer, slow or dead, can hold a hook event longer than 10 s, and an async one never holds it. *Not confident (redaction-default):* secrets are masked by default in every dispatched record, and an audit rule that must be byte-complete opts out with `redact: none`. Is masking-by-default what you want for an audit log?
- **Inversion:** between observing the session from inside the agent loop (a way or a guard, which shapes what the agent does) and leaving it unobserved. The decision observes from the hook path and hands everything to consumers outside it, so observation never becomes control.

## Context

ways sees the session through Claude Code's hooks, but only the parts it is wired for. Today PreToolUse fires ways for `Edit|Write`, `Bash`, `TaskCreate` and `Task`, and PostToolUse and PostToolUseFailure for `Edit|Write|Bash|Task`. A `Read`, `Grep`, `WebFetch` or MCP tool call never reaches ways. No hook is wired for SubagentStop, SessionEnd or PreCompact, and `ways hook` has no SubagentStop event. The prompt, the main agent's reply at each Stop, each subagent start and each session start do reach it.

An independent audit log, a research journal, a policy checker or a team dashboard needs the whole stream. Today there is no way to send any of it anywhere. ways only uses what it sees to match and disclose guidance.

The consumers are external on purpose. A record should be processed by something the agent loop can't influence and doesn't wait for. A slow endpoint must not slow the session. A broken one must not break it. Several consumers may want the same record.

"Sensor" already names ADR-103's checks, the epoch-distance-aware confidence sensors for ways. A ways sensor is a different thing: an ADR-403 sensor file of kind `ways` that exports session records. This record always says "ways sensor".

## Decision

### 1. A ways sensor is a sensor file under ADR-403's spec

A ways sensor is a YAML file with `kind: ways`. It carries ADR-403's shared core (identity, layering, `where` conditions, `enabled`, the management flow) and the `ways` block this record defines: `rules` and their `dispatch`. The spec types come from the shared `sensor-spec` crate (ADR-403 §7). It is hosted by ways: it needs only the ways hooks, and works with attend absent, stopped or switched off.

The files live in ways' own roots and layer by name:
- **shipped:** embedded in the `ways` binary;
- **user:** `$XDG_CONFIG_HOME/agent-ways/sensors/<name>.yaml`, beside `config.yaml`, `ways/` and `themes/` in ways' config root;
- **project:** `.claude/ways-sensors/<name>.yaml`.

The project directory is not `.claude/ways/sensors/`, because `.claude/ways/` is the project way root and every directory in it reads as a way domain. ways never reads attend's sensor files, and its hook path reads no attend file at all (ADR-503 §5).

### 2. Wiring and the fast path

ways adds one hook entry for sensors, `ways hook sensors`, which reads the event name from the payload. It is wired with matcher `*` on PreToolUse, PostToolUse and PostToolUseFailure, and on UserPromptSubmit, Stop, SubagentStart, SubagentStop, SessionStart, SessionEnd and PreCompact. The existing entries and their scans stay as they are, so every record comes from this one entry and none is spooled twice.

The fast path comes first. ways compiles its sensor files into an index in its cache root, keyed by the path, size and modification time of each sensor file and of ways' user `config.yaml`, which holds the `ways.project_sensors` allow-list (§7). Removing a project from that list changes the key and rebuilds the index, so a project sensor never outlives its trust. The index holds the sections each enabled rule selects. The hook stats the sensor directories, reads the index, and exits before any other work when no enabled rule selects the event's section. It rebuilds the index only when a key changed. It loads no settings section, way corpus or session record on that path.

The latency budget, measured by ADR-504 §11's method against `ways --version` on the same machine:
- **no rule selects the section:** median within 2 ms;
- **async rules select it:** the hand-off of §5 included, median within 5 ms with 20 sensor files;
- **the existing `ways hook` commands:** unchanged, within ADR-504 §11's budget, since the sensor entry is separate.

A change over budget is fixed before it merges or carries the measurement and the reason in its PR.

### 3. Known sections

A fixed, documented vocabulary of what can be selected, each with a stable JSON shape and the hook it comes from:

| Section | Hook | Reached ways before this decision |
|---|---|---|
| `user_prompt` | UserPromptSubmit | yes |
| `queued_message`, text typed while the agent works | PostToolUse and Stop, from the transcript | for four tool matchers only |
| `assistant_response`, the reply text at a turn's end | Stop (main agent), SubagentStop (subagent), from the transcript | main agent only |
| `tool_call`, name and input | PreToolUse | for five tool matchers only |
| `tool_result`, output or error, paired to its call | PostToolUse, PostToolUseFailure | for four tool matchers only |
| `subagent`, start and stop, with type and agent id | SubagentStart, SubagentStop | start only |
| `compaction` | PreCompact, and SessionStart with `compact` | after compaction only |
| `session_start` | SessionStart | yes |
| `session_end` | SessionEnd | no |
| `way_event`, fired, suppressed or judged | ways' own records | yes |

Every record carries the session id, `agent_id` and agent type, a timestamp, the project, and a per-session sequence number. `assistant_response` holds text blocks only: thinking is excluded, and tool use is represented by its `tool_call` records.

Some of the session stays out of reach even with this wiring. Whether a reply the user interrupts reaches a Stop hook is unverified. No primary source settles it, and it is tested before an export relies on it (an observable below). If it does not, the reply's text is read from the transcript at the next Stop or at SessionEnd. A session killed without SessionEnd produces no `session_end`, and the gap shows in the sequence numbers. Thinking is never exported.

### 4. Rules fan out

A sensor file holds one or more rules. A rule has:
- a `name`, unique in the file;
- `enabled`, which defaults to true;
- a selection: `sections` plus a `where` condition (ADR-403 §3, wildcards included);
- a `dispatch` target: `file` (append JSON lines or text), `http` (POST the record as JSON), or `command` (an argv with the record on stdin or as arguments).

Any number of rules, in one file or many, can match the same record. Each match dispatches independently, so one tool call can reach an audit log, a policy checker and a dashboard at once. One rule's consumer never delays or affects another's.

### 5. Dispatch never halts the session

- **Hand-off:** the hook appends the record to a per-session spool in ways' state root and returns. A detached `ways sensors dispatch --session <id>` process drains the spool. The hook starts it when no dispatcher holds the session's lock, and it exits after the spool stays empty for an idle period or at `session_end`. Dispatch is off the hook path, and a hook process exiting right after the hand-off loses nothing.
- **Watchdog:** every dispatch has a timeout, set per rule. On expiry the watchdog kills the dispatch: a command's whole process group, or an HTTP request at connect or read. The timeout is recorded.
- **Blocking toggle:** `mode: blocking` makes the hook dispatch that rule itself and wait, for a consumer that must see the record before the agent goes on. All blocking rules matching one hook event dispatch in parallel under one shared deadline: the largest of their timeouts, capped at 10 s. One hook event therefore waits at most 10 s, whatever the number of records or rules, well inside Claude Code's hook timeout. Each rule is still killed at its own timeout. The shared deadline only bounds how long the hook waits for the group, and on expiry every unfinished blocking dispatch is killed and the session continues.
- **Bounded backlog:** each rule's queue is capped. Past the cap, records are spooled (under `delivery: at_least_once`) or dropped and counted.
- **Visible failures:** timeouts, kills, refusals and drops are counted per rule, logged as events, and shown in `ways status` and the sensors tab.

### 6. Delivery and integrity

- **Best effort by default:** a record dispatched within its timeout is delivered, and anything else is counted as lost.
- **`delivery: at_least_once`:** the spool entry is kept until the consumer accepts it. Retries continue across kills and restarts, and sequence numbers make gaps detectable.
- **`chain: true`:** a hash chain across a rule's records, so a later edit to an audit log shows.

### 7. Redaction, trust and off-machine data

- **Redaction:** every dispatched record passes a built-in secrets scan, which masks API keys, tokens, private keys and `.env`-style assignments with a marked placeholder. A rule may set `redact: none` for a byte-complete record. The sensor's lint output then says that it dispatches unmasked session content, and where to.
- **Off-machine data:** an `http` rule, or a `command` rule, may send session content off the machine. `ways settings` and lint say so per rule.
- **Project files:** a project's ways sensor file loads only when the project is listed in `ways.project_sensors`, a user-scope key in `$XDG_CONFIG_HOME/agent-ways/config.yaml` that a project file can't set. attend keeps its own list for its sensors (ADR-403 §2). A repository can't silently start exporting your session.

### 8. Observation, not control

A ways sensor never edits or blocks a tool call. A blocking rule only delays, within the shared deadline. Guard hooks (ADR-181) stay the only blocking path.

### 9. Managed and tested from both surfaces

- **Keys:** every rule field has a key, `ways.sensors.<sensor>.rules.<rule>.<field>`, and every sensor field `ways.sensors.<sensor>.<field>`, reachable through `ways settings get|set|unset|help|list --json`. ways declares these key shapes in its own schema, and `set` writes the sensor's file at the chosen layer.
- **ADR-503 amended:** as ADR-403 §5 does for attend's sensors, this amends ADR-503's Decision for ways' own registry. Its §1 build-time composition gains `ways.sensors` sections discovered at runtime, one per ways sensor file. Its §3 layers gain a shipped layer below the user layer for those keys. Its §8 write target, one path per file kind through `file_of`, becomes the sensor's own file at the layer being written. The rest of ADR-503 stays in force.
- **Sensors tab:** shows `ways.sensors` beside `attend.sensors`: the tab's single `prefix` in `tools/ways-cli/src/cmd/settings/tui/build.rs` becomes a list, and the `ways` tab skips `ways.sensors`. It lists each ways sensor's rules with sections, condition, target, mode, timeout, the off-machine flag and live counts. It toggles a rule or a sensor live, and edits rules in a form validated against the spec.
- **Files:** `ways sensors copy|rename|delete`, the same named-item flow the theme tab uses (ADR-504 §10). Shipped files can be copied but not renamed or deleted.
- **Testing:**
  - `ways sensors test <sensor>/<rule> [--session ID]` replays a recorded session through a rule without dispatching. The editor shows the same preview, and a new rule starts disabled until its preview has been seen.
  - `ways sensors fire <sensor>[/<rule>] --section <section> [--input FILE|-]` sends a `test: true` record through the real dispatcher, watchdog and target. `--input` is the flag attend's `sensors fire` uses too (ADR-403 §6). `--dry-run` stops before dispatch.

### 10. Initial values

The spec owns these and may change them without a new decision:
- a rule's default timeout: 5 s;
- the blocking deadline's cap: 10 s;
- each rule's queue cap: 1,000 records;
- the dispatcher's idle exit: 60 s with an empty spool.

## Consequences

### Positive

- An independent audit trail of everything the human and the agents did, every tool and subagents included, with no change to the agent loop.
- Session activity reaches any external system that takes a file, an HTTP POST or a command.
- The sensors skill (#847) can author, lint, test and enable ways sensors through the CLI alone.

### Negative

- Every tool call and the three new events start a `ways` process, even on installs that never use a ways sensor. With no sensor configured, the fast-path budget of §2 bounds that cost.
- A misconfigured `http` rule can send session content somewhere it shouldn't. Redaction, lint and the project allow-list limit this, but can't remove it.
- The per-session dispatcher is a new background process to supervise, with a spool on disk.
- A blocking rule costs up to the shared deadline per matching hook event.

### Neutral

- The section vocabulary becomes a documented contract, so a field added later must keep older consumers working.
- `settings.json` gains a `ways hook sensors` entry on ten hook events, and `ways reconcile` merges it.
- `docs/reference/events.md` gains the sensor events (timeout, kill, drop, spool).

## Alternatives Considered

- **An attend sensor.** Rejected: it would require attend to be running, and attend observes the outside world rather than the session.
- **Keep today's wiring.** Rejected: an audit log that misses every `Read`, `Grep`, MCP call, subagent stop and session end is not an audit log.
- **Wire `*` without a fast path, or record through the existing scan entries.** Rejected: every tool call would pay for loading settings and sessions whether or not a sensor exists, and the existing scans' latency budget would absorb the sensor work.
- **Dispatch in the resident ways agent (ADR-502).** Rejected: the agent is per user and may not be loaded, so dispatch would need the per-session process anyway. A per-session dispatcher also keeps one session's stuck consumer away from other sessions, and keeps session export out of the process that holds provider keys.
- **Dispatch inside the hook, synchronously.** Rejected: a slow endpoint would stall every tool call. Blocking stays an explicit, bounded per-rule choice.
- **Composing rules with ways, guards or governance.** Rejected for this decision: the point is processing outside ways and the agent loop. Disclosing guidance from session conditions would be a separate decision.
- **No default redaction.** Rejected: an `http` rule written in a hurry would ship keys pasted into a prompt. The opt-out is explicit and visible instead.
