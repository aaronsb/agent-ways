---
contract: adr/v1
kind: decision
verb: add
capability: config
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
  - evidence: "ways hook <event> already receives every hook payload Claude Code sends: UserPromptSubmit (the human's message), PreToolUse (tool name and input), PostToolUse and PostToolUseFailure (the result), Stop (hook/response.rs records the last reply; the full text is in the transcript), SubagentStart and SubagentStop; ways keeps per-agent session records since PR #818"
  - precedent: ADR-403
  - precedent: ADR-502
  - precedent: ADR-503
agent:
  name: Claude
  model: claude-opus-5-5
observable:
  - 'run: with attend not installed, a tool-audit ways sensor writes every tool call and result of the main agent and its subagents to one JSONL file, in sequence, with no gaps'
  - 'run: ways sensors fire tool-audit/bash-policy --section tool_call against an endpoint that never answers is killed at its timeout, and the hook returned before it'
  - 'see: three rules matching one record dispatch three times, and a consumer that fails leaves the other two delivered'
  - 'see: the ways settings sensors tab lists a ways sensor''s rules with their dispatched, timed out, dropped and spooled counts, and toggling one writes enabled: to the user file'
status: proposed
date: 2026-10-03
deciders:
  - aaronsb
related:
  - ADR-181
  - ADR-403
  - ADR-502
  - ADR-503
---

# ADR-199: Ways sensors dispatch session activity to external consumers, async under a watchdog

## Summary

- **Decided:** a fourth sensor class, the ways sensor (`kind: ways`), is hosted by ways and faces inward. It observes the session itself (what the human asks, what the agent says, what the agent does) and dispatches selected records to external consumers (a file, an HTTP endpoint, a command), which process them outside ways and the agent loop. A sensor file holds named rules; overlapping rules fan out to separate consumers. Dispatch is async by default, each dispatch has a watchdog timeout that kills it, and blocking is a per-rule toggle still bounded by that timeout. It runs whether or not attend does.
- **Trades away:** an `http` or forwarding `command` rule sends session content off the machine, and a blocking rule can delay a turn by up to its timeout.
- **One-way?** No. Removing a sensor file stops the dispatch; records already delivered stay with their consumer.
- **Probes:** *Confident (no-halt):* no consumer, slow or dead, can hold the session longer than the timeout its rule declares. *Not confident (redaction-default):* secrets are masked by default in every dispatched record, and an audit rule that must be byte-complete opts out with `redact: none`. Is masking-by-default what you want for an audit log?
- **Inversion:** between observing the session from inside the agent loop (a way or a guard, which shapes what the agent does) and leaving it unobserved. The decision observes from the hook path and hands everything to consumers outside it, so observation never becomes control.

## Context

ways sees the whole session through Claude Code's hooks: every prompt, every tool call and result, every subagent start and stop, and the agent's reply at each turn's end. That stream is what an independent audit log, a research journal, a policy checker or a team dashboard needs. Today there is no way to send any of it anywhere. ways only uses it to match and disclose guidance.

The consumers are external on purpose. A record should be processed by something the agent loop can't influence and doesn't wait for. A slow endpoint must not slow the session. A broken one must not break it. Several consumers may want the same record.

## Decision

### 1. A ways sensor is a sensor file under ADR-403's spec

A ways sensor is a YAML file with `kind: ways`, layered shipped → user → project and managed by the same settings surfaces as every other sensor (ADR-403 §2, §5). It is hosted by ways: it needs only the ways hooks, and works with attend absent, stopped or switched off. The spec types come from the shared `sensor-spec` crate (ADR-403 §7).

### 2. Known sections

A fixed, documented vocabulary of what can be selected, each with a stable JSON shape:
- **human input:** `user_prompt`, and `queued_message` for text typed while the agent works;
- **agent output:** `assistant_response`, the reply text at each turn's end;
- **agent actions:** `tool_call` (name and input) and `tool_result` (output or error, paired to its call);
- **structure:** `subagent` (start and stop, with type and agent id), `compaction`, `session_start`, `session_end`;
- **ways' own records:** `way_event` (fired, suppressed, judged).

Every record carries the session id, `agent_id` and agent type, a timestamp, the project, and a per-session sequence number. `assistant_response` is read at Stop from the transcript: text blocks only, thinking excluded, and tool use represented by its `tool_call` records.

### 3. Rules fan out

A sensor file holds one or more rules. A rule has:
- a `name`, unique in the file;
- `enabled`, which defaults to true;
- a selection: `sections` plus a `where` condition (ADR-403 §3, wildcards included);
- a `dispatch` target: `file` (append JSON lines or text), `http` (POST the record as JSON), or `command` (an argv with the record on stdin or as arguments).

Any number of rules, in one file or many, can match the same record. Each match dispatches independently, so one tool call can reach an audit log, a policy checker and a dashboard at once. One rule's consumer never delays or affects another's.

### 4. Dispatch never halts the session

- **Hand-off:** the hook appends the record to a per-session spool and returns. The dispatcher drains the spool. It runs in the resident ways agent (ADR-502) when that is loaded, and otherwise in a detached `ways sensors dispatch` process the hook starts. Either way, dispatch is off the hook path, and a hook process exiting right after the hand-off loses nothing.
- **Watchdog:** every dispatch has a timeout, 5 s by default, set per rule. On expiry the watchdog kills the dispatch: a command's whole process group, or an HTTP request at connect or read. The timeout is recorded.
- **Blocking toggle:** `mode: blocking` makes the hook dispatch that rule itself and wait for it, for a consumer that must see the record before the agent goes on. It runs under the same watchdog, with a ceiling of 10 s, well inside Claude Code's hook timeout. On expiry the dispatch is killed and the session continues.
- **Bounded backlog:** each rule's queue is capped at 1,000 records. Past the cap, records are spooled (under `delivery: at_least_once`) or dropped and counted.
- **Visible failures:** timeouts, kills, refusals and drops are counted per rule, logged as events, and shown in `ways status` and the sensors tab.

### 5. Delivery and integrity

- **Best effort by default:** a record dispatched within its timeout is delivered, and anything else is counted as lost.
- **`delivery: at_least_once`:** the spool entry is kept until the consumer accepts it. Retries continue across kills and restarts, and sequence numbers make gaps detectable.
- **`chain: true`:** a hash chain across a rule's records, so a later edit to an audit log shows.

### 6. Redaction

Every dispatched record passes a built-in secrets scan, which masks API keys, tokens, private keys and `.env`-style assignments with a marked placeholder. A rule may set `redact: none` for a byte-complete record. The sensor's lint output then says that it dispatches unmasked session content, and where to.

### 7. Trust and off-machine data

- **Off-machine data:** an `http` rule, or a `command` rule, may send session content off the machine. `ways settings` and lint say so per rule.
- **Project files:** a project's ways sensor file loads only when the project is on the user config's sensor allow-list (ADR-403 §2, settled with #825). A repository can't silently start exporting your session.

### 8. Observation, not control

A ways sensor never edits or blocks a tool call. A blocking rule only delays, within its timeout. Guard hooks (ADR-181) stay the only blocking path.

### 9. Managed and tested from both surfaces

- **Keys:** every rule field has a key (`ways.sensors.<sensor>.rules.<rule>.<field>`), reachable through `ways settings get|set|unset|help|list --json`.
- **Sensors tab:** lists each ways sensor's rules with sections, condition, target, mode, timeout, the off-machine flag and live counts. It toggles a rule or a sensor live, and edits rules in a form validated against the spec. Sensor files are copied, renamed and deleted with the same named-item flow the theme tab uses (shipped files can't be renamed or deleted).
- **Testing:**
  - `ways sensors test <sensor>/<rule> [--session ID]` replays a recorded session through a rule without dispatching. The editor shows the same preview, and a new rule starts disabled until its preview has been seen.
  - `ways sensors fire <sensor>[/<rule>] --section <section> [--record FILE|-]` sends a `test: true` record through the real dispatcher, watchdog and target. `--dry-run` stops before dispatch.

## Consequences

### Positive

- An independent audit trail of everything the human and the agents did, subagents included, with no change to the agent loop.
- Session activity reaches any external system that takes a file, an HTTP POST or a command.
- The sensors skill (#847) can author, lint, test and enable ways sensors through the CLI alone.

### Negative

- A misconfigured `http` rule can send session content somewhere it shouldn't. Redaction, lint and the project allow-list limit this, but can't remove it.
- The dispatcher is a new background component to supervise, with a spool on disk.
- A blocking rule costs up to its timeout per matching record.

### Neutral

- The section vocabulary becomes a documented contract, so a field added later must keep older consumers working.
- `docs/reference/events.md` gains the sensor events (timeout, kill, drop, spool).

## Alternatives Considered

- **An attend sensor.** Rejected: it would require attend to be running, and attend observes the outside world rather than the session.
- **Dispatch inside the hook, synchronously.** Rejected: a slow endpoint would stall every tool call. Blocking stays an explicit, bounded per-rule choice.
- **Composing rules with ways, guards or governance.** Rejected for this decision: the point is processing outside ways and the agent loop. Disclosing guidance from session conditions would be a separate decision.
- **No default redaction.** Rejected: an `http` rule written in a hurry would ship keys pasted into a prompt. The opt-out is explicit and visible instead.
