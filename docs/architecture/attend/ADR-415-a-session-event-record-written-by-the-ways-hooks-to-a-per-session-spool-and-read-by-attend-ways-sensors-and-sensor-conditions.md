---
contract: adr/v1
kind: decision
verb: add
capability: attend
basis:
  - operator: aaronsb
    level: directed
    said: "I think we should take his pr about the tools and research what other tools we likely can sense against. this might become an attend built in sensor crate ( a claude code sensor crate)"
    via: "the operator in the head session, 2026-10-22, on PR #1115"
  - operator: aaronsb
    level: directed
    said: "*in addition to ways triggers"
    via: "the operator in the head session, 2026-10-22, correcting the message before it"
  - operator: aaronsb
    level: directed
    said: "One schema now"
    via: "the operator's pick in a multiple-choice question in the head session, 2026-10-22, over an attend-only spool first; the option label was agent-written"
  - evidence: ADR-705, the survey of Claude Code's hook events, which rates the events an attend sensor would gain from and the cost of each hook entry
  - evidence: "issue #846: ways sensors observe the session through the ways hooks, which already receive every payload; it names a section vocabulary and requires every record to carry the session id, agent id, agent type, a timestamp, the project and a per-session sequence number"
  - evidence: "issue #843: a sensor spec with crate sensors and declarative triggers, whose `where:` conditions select on fields of a record"
  - evidence: "tools/ways-cli/src/session/log.rs `append_jsonl_line`: one `write_all` of record and newline on an O_APPEND handle, adopted after two racing PreToolUse hooks interleaved and corrupted each other's lines (#528)"
  - evidence: "tools/sensor-trait/src/lib.rs: a sensor is polled (`poll`) and persists its own state across restarts (`export_state`, `import_state`); attend's context and disclosure sensors already read ways state through `ways context --json`"
  - evidence: "plugins/attend/hooks/register.tsx (lines 877-1067 on main at 7fd6c0de): the attend mod registers in-process `session.start`, `session.end`, `session.measure` and `turn.complete` handlers, and `tool.call` handlers scoped to its own five MCP tools (`attend_listen`, `attend_send`, `attend_reply`, `attend_join`, `attend_peers`); they run only with Claude Code's mods preview flag on, off by default"
  - precedent: ADR-706
  - precedent: ADR-188
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Acceptable as written"
    via: multiple-choice question in the agent-ways session, 2026-10-22; options were acceptable as written, fields only for now, or no excerpts
    covers: [excerpt]
status: proposed
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-181
  - ADR-182
  - ADR-406
  - ADR-500
  - ADR-502
  - ADR-705
observable:
  - "In an interactive session without the attend mod, a subagent's start and finish each add one record to the session's spool, and attend's `claude-code` sensor reports the finish on its next poll"
  - "A Workflow launched in the background appears in a Stop record's `background_tasks` while it runs; the first Stop record after it completes shows it finished, and the sensor reports the finish once"
  - "`/compact` adds a PreCompact and a PostCompact record; switching model adds one PostModelSwitch record carrying from_model, to_model and context_tokens"
  - "A permission prompt adds a PermissionRequest record with the tool's type and primary field value before the operator answers it"
  - "A Bash call with a 10 KB heredoc adds a record of at most 4,096 bytes whose `value` is cut to 512 bytes and whose `truncated` lists `value`"
  - "Two PreToolUse hooks fired in parallel leave two whole records with consecutive `seq` values"
  - "attend killed and restarted mid-session resumes from its stored offset, reporting no record twice; a segment dropped while it was down is reported as a gap in `seq`"
  - "The spool's files are mode 0600 under the session's directory in the sessions root, and are removed with the session's other state"
---

# ADR-415: A session event record, written by the ways hooks to a per-session spool and read by attend, ways sensors and sensor conditions

## Summary

- **Decided:** the ways hooks write every Claude Code event they receive as one normalised record, appended to a per-session JSONL spool under the session's runtime directory. One schema serves three readers: attend's new `claude-code` crate sensor, which reads new lines on each poll from a stored offset; #846's ways sensors, whose rules select records by section and condition; and #843's `where:` conditions, which test record fields. A record carries the event name, its #846 section, a sequence number, a timestamp, the session and agent ids, the agent type, the permission mode, the tool's raw name with its abstract type and primary field value (ADR-706), and a capped excerpt of the event's own fields. Events that only a sensor needs are wired as asynchronous hooks, so they never block. The sensor reports subagent starts and finishes, workflows and background tasks finishing, compaction, model switches, permission waits and completed tasks.
- **Trades away:** a spawn per newly wired event, and one file append per event on every session whether or not anything reads it. Session content (the first 512 characters of commands, paths, prompts and replies) sits unredacted in a per-user file for the session's life. A tool the ways hooks are not wired for, such as Read or an MCP tool by default, leaves no record. Faster in-process routes, such as the attend mod's own handlers, become optimisations over the file rather than sources.
- **One-way?** No. The spool is a file under the session directory, and its writer is one call per adapter. The field names become a contract once #846 lets consumers outside this repo read them; the `v` field allows a later schema beside this one.
- **Probes:** *Confident (sensor):* attend notices subagents and workflows finishing, compaction, model switches and permission prompts in any session with ways installed, with or without the attend mod, because the ways hooks write each event down and attend reads it on its poll. *Not confident (excerpt):* each record keeps up to 512 characters of the command, path, prompt or final reply it describes, in a file only you can read under the session's runtime directory, with nothing redacted. Is that acceptable, or should prompts and replies stay out until a redaction rule exists?
- **Inversion:** at one end, every consumer hooks Claude Code for itself (attend's hooks, #846's hooks, a mod), each with its own schema and its own spawns. At the other, one resident process that every event must pass through and that loses events when it is down. This decision uses the hooks that already run and a file that needs nothing running.

## Context

attend learns about the session it serves by polling. Its context and disclosure sensors shell out to `ways context --json`; nothing tells it that a subagent finished, that a background Workflow completed, that the session compacted or changed model, or that the agent is waiting on a permission prompt. Claude Code announces each of these as a hook event (ADR-705), and hooks push one process per event, while sensors pull on a poll.

The ways hooks already receive the payloads. Issue #846 builds on that: ways sensors that select records of the session's own activity (prompts, replies, tool calls) and dispatch them to consumers outside ways, such as an audit log or a policy checker, with or without attend. It names a section vocabulary and the fields every record carries. Issue #843 specifies attend's sensors, whose triggers test `where:` conditions against fields of what a sensor observes. A tool-call event seen by an attend sensor, a ways-sensor rule and a sensor condition is the same event, and each of the three needs a stable shape for it.

The operator asked for a Claude Code sensor crate for attend "in addition to ways triggers", and chose one record schema now over a narrower attend-only spool first. The tool catalog (ADR-706) gives the schema harness-neutral tool fields. The tool-call adapter (ADR-188) is the writer for tool events.

## Decision

1. **The writer.** Each event has exactly one recording hook command. Where a ways adapter already runs on the event, it writes the record before its own matching: `check-tool.sh` for the tool events and for UserPromptExpansion, a typed skill or command (ADR-188), `check-prompt.sh` for UserPromptSubmit, `check-response.sh` for Stop, `inject-subagent.sh` for SubagentStart, `check-state.sh` for SessionStart. A new adapter, `hooks/ways/record-event.sh` over `ways hook record`, is wired with `async: true` for the events that carry no ways matching: SubagentStop, TaskCreated, TaskCompleted, PreCompact, PostCompact, PostModelSwitch, StopFailure, PermissionRequest, PermissionDenied, Notification, CwdChanged and SessionEnd. MessageDisplay and PostToolBatch are never wired: they fire on every streamed chunk and every step. The engine also appends a `way_event` record when it logs a fire or a suppression for the session. A failure to write is counted and never fails the hook.

2. **The record.** One JSON object per line, schema version 1:

   | Field | Content |
   |---|---|
   | `v` | `1` |
   | `seq` | per-session sequence number |
   | `ts` | UTC timestamp with milliseconds |
   | `event` | the literal hook event name |
   | `section` | #846's section: `user_prompt`, `queued_message`, `assistant_response`, `tool_call`, `tool_result`, `subagent`, `compaction`, `session_start`, `session_end`, `way_event`; and `permission`, `model_switch`, `task`, `turn_end` and `notification` for the events #846 does not name |
   | `session_id`, `agent_id`, `agent_type` | from the payload; `agent_id` and `agent_type` are null in the main session |
   | `permission_mode`, `cwd` | from the payload |
   | `tool_name` | the tool's name as the payload gives it (tool events and permission events) |
   | `tool_type` | the catalog's type, or `unmapped` |
   | `tool_use_id` | pairs a call with its result |
   | `field`, `value` | the abstract field (`command`, `path`, `url`, `agent`, `skill`, `tool`) and its value |
   | `status` | on a tool result: `ok`, `error` for PostToolUseFailure, or the response's own status where it reports one, such as a Workflow's `async_launched` |
   | `x` | the event's excerpt, below |
   | `truncated` | the fields that were cut |

   The excerpt takes a fixed list of fields per event and nothing else:
   - UserPromptSubmit: `prompt`, `source`.
   - Stop: `background_tasks`, each as `id`, `type`, `status` and `name`, up to 32; the count of `session_crons`; `last_assistant_message`.
   - StopFailure: `error`.
   - SubagentStop: `last_assistant_message`.
   - PreCompact, PostCompact: `trigger`; PostCompact also `compact_summary`.
   - PostModelSwitch: `from_model`, `to_model`, `source`, `context_tokens`.
   - PermissionDenied: `reason`.
   - Notification: `notification_type`.
   - TaskCreated, TaskCompleted: `task_subject`, `teammate_name`, `team_name`.
   - SessionStart: `source`, `transcript_path`. SessionEnd: `reason`.
   - Tool events: nothing beyond the primary field. Tool output, file contents and the rest of `tool_input` are never copied.

3. **Size.** Each excerpted string and `value` is cut to 512 bytes at a character boundary. A record is at most 4,096 bytes; past that, `x` is dropped and listed in `truncated`. Each record is one `write` of line and newline on an append handle, as `append_jsonl_line` does today, so parallel hooks never interleave.

4. **Location and retention.** The spool is `spool/` in the session's directory under the sessions root: `$XDG_RUNTIME_DIR/claude-sessions/<session>/spool/` on Linux, the per-user fallbacks elsewhere. Subagents write into their parent session's spool; `agent_id` tells them apart. Records go into numbered segments (`000001.jsonl`, …). The writer starts a new segment at 4 MiB and keeps the newest 8, so a session holds at most 32 MiB. `seq` is assigned under an advisory lock on `spool/lock`, held for the increment and the append. A writer that waits more than 50 ms writes the record with `seq: null` and counts the miss. Files are created with mode 0600. The spool is removed with the session's other state, and the runtime directory goes at logout. A consumer that needs history beyond the session copies what it needs, as a #846 `file` rule does.

5. **Excerpts are not redacted.** The spool stays on the machine, in a per-user directory, readable only by its owner. Sending records anywhere else is #846's dispatch, which owns the redaction decision for what leaves the machine.

6. **attend's `claude-code` sensor.** A crate sensor, registered beside the existing crate sensors and moved under #843's spec when that lands. It reads the spool of the session attend runs for. On each poll it reads lines from its stored segment and byte offset, kept through `export_state`. If its segment has been dropped, it resumes at the oldest segment and reports the `seq` gap. It emits transitions, not states:
   - a subagent started or finished, with its type (SubagentStart, SubagentStop);
   - a Workflow, background shell or monitor finished: a task id in one Stop record's `background_tasks` whose status has left running, or which is gone, in the next;
   - the session compacted (PostCompact), which is ground truth for the context sensor;
   - the model and window changed (PostModelSwitch);
   - the agent is waiting on a permission (PermissionRequest, or a Notification of type `permission_prompt`) or on its human (`idle_prompt`);
   - a task completed (TaskCompleted);
   - a turn failed (StopFailure), a plan was approved (ExitPlanMode's tool result), the working directory changed (CwdChanged).

   Magnitudes and the lane each signal rides are set in the sensor's configuration, under #843 and #139.

7. **#846 and #843 read the same records.** A ways-sensor rule selects by `section` and a `where:` condition over these fields, and its dispatcher reads the spool. Where that dispatcher runs is #846's decision. A #843 condition tests the same fields: `where: { tool_type: shell, value: { like: "git push*" } }`. Neither defines a second shape for a session event.

8. **The attend mod is a fast path.** When the mod is loaded, in-process handlers may wake attend before the next poll. Its `session.*` and `turn.complete` handlers see those events today; its `tool.call` handlers cover only attend's own tools, so a fast path for other tool calls needs handlers registered on them. The spool stays the source of the record, and the sensor drops a duplicate by `seq` or `tool_use_id`.

## Consequences

### Positive

- attend sees subagent, workflow, compaction, model and permission events in every session with ways installed, with or without the mod.
- One schema serves the sensor, the ways sensors and sensor conditions, so a condition written for one reads the others' records.
- The record is harness-neutral where it matters: tool fields come from the catalog, and the raw name stays beside them.
- The spool doubles as a per-session event trace for debugging hooks: the record of what Claude Code sent, with sequence numbers.

### Negative

- Twelve event types gain an asynchronous spawn each, and every session pays one append per event even when nothing reads the spool.
- Unredacted excerpts of commands, prompts and replies sit in a per-user file for the session's life.
- Tools the ways hooks are not wired for leave no record. Reads and MCP calls are invisible to the sensor unless a user wires them.
- The `seq` lock adds a small serial step to every hook that records. A writer that loses the race past 50 ms leaves a record without a sequence number.
- Comparing `background_tasks` between Stop records misses a task that starts and finishes inside one turn, and Stop does not fire on a user interrupt.

### Neutral

- The writer adds one call to each existing adapter. The hooks stay single-binary invokers.
- `events.jsonl` keeps its role as the global telemetry log. `way_event` records in the spool repeat, per session, what it already logs.
- The record's field names follow Claude Code's payload keys where one exists (`tool_name`, `permission_mode`), so a reader of both sees one name per value.

## Alternatives Considered

- **The attend mod's in-process handlers as the source.** The mod already registers `session.*` and `turn.complete`, and `tool.call` for its own five tools. Rejected as the source: mods run only behind Claude Code's preview flag, which is off by default, so most sessions would record nothing. Kept as a fast path (item 8).
- **Hooks call `attend` directly.** Rejected: one more process per event; attend must be installed and running for the event to exist anywhere; the hooks would gain logic; and #846's ways sensors must work with attend absent.
- **An attend-only spool first.** A narrower format for the sensor alone, with #846 deciding its own later. Not chosen: the operator picked one schema now. Two shapes for the same event would leave #843's conditions written against one of them.
- **Tail the Claude Code transcript.** It records tool calls, results and messages, but not hook-only events such as PermissionRequest, PostModelSwitch or Notification. Its format is internal to Claude Code and changes between builds, which is the coupling ADR-706 removes for tools.
- **Reuse `events.jsonl`.** Rejected: it is global across sessions, so every reader filters by session; it is compacted at 32 MiB, which breaks stored offsets; and it carries telemetry in another shape.
- **A resident process receives events over a socket** (the ways agent, ADR-502). Rejected for now: events are lost while it is down, and it adds a dependency to every hook. A later transport can carry this same schema.
- **Store whole payloads.** Rejected: Write and Edit inputs carry whole files, tool results carry whole outputs, and both carry secrets. The excerpt keeps what the named signals need.
