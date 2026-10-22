---
contract: adr/v1
kind: decision
verb: change
capability:
  - matching
  - authoring
  - disclosure
supersedes: ADR-155#4
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
    said: "let's do a full survey. it might just be as straightforward as enumerating the types of tools to afford trigger types on"
    via: "the operator in the head session, 2026-10-22"
  - operator: aaronsb
    level: directed
    said: "Rewrite it"
    via: "the operator's pick in the head session, 2026-10-22, between rewriting this record in place and rejecting it; the option label was agent-written"
  - operator: aaronsb
    level: directed
    said: "One key per type"
    via: "the operator's pick in a multiple-choice question in the head session, 2026-10-22, over a single `tool:` key in Claude Code's permission-rule syntax; the option label was agent-written"
  - evidence: "issue #528, closed 2026-09-29: verified on ways-v1.22.0 that in a fresh `claude -p` session running `git commit`, three ways arrived as PreToolUse context, the transcript recorded `hook_additional_context`, and the model quoted their headings. The undelivered output measured in September was this project's emit shape, a top-level `decision` and `additionalContext`"
  - evidence: "tools/ways-cli/src/cmd/scan/mod.rs: the file and command scans emit through `emit_hook_context(\"PreToolUse\", …)` (line 1028), the `hookSpecificOutput` envelope; its docstring (lines 1493-1505) records about 5,458 PreToolUse invocations that delivered nothing under the old shape, against 143 of 143 PostToolUse deliveries"
  - evidence: "the events log, 2026-10-20 01:37Z to 2026-10-22 17:58Z: 7,034 `way_fired` events, of which 5,705 (81%) on `semantic:bash:*`, 668 on the `commands:` regex and 179 on `files:`. The semantic Bash lane fired `ea/intelligence` and `ea/comms/recap` on `cat` and `sed` reads of ADR files"
  - evidence: "issue #528, September 2026 measurement of the semantic Bash lane: off-topic fires at high scores (`workstation/shell/prompt` at 0.91 on a `head` of a slide deck), the median firing call showed 1 way and 20% showed 4 or more, and one `git commit` described as \"commit the change\" produced 10.4k characters of way text"
  - evidence: ADR-705, the survey of Claude Code's tool surface, which groups the tools into types by primary field and lists the current lanes on that grid
  - evidence: "ADR-704, proposed on PR #1115's branch (feat/tools-trigger-lane) and not on main on 2026-10-22: a `tools:` regex on the tool name, `tool_phase: pre|post|both`, and the adapter `hooks/ways/check-tool.sh` over `ways hook tool`, wired for EnterPlanMode, ExitPlanMode and Workflow; its `scan::tool` copies `scan::file`'s hit loop"
  - operator: aaronsb
    level: directed
    said: "I don't really use plan mode"
    via: "the operator in the head session, 2026-10-22, relayed verbatim by the coordinator after the probes were answered"
  - operator: aaronsb
    level: directed
    said: "instead, we use our own skills"
    via: "the operator in the head session, 2026-10-22, following the message before it; relayed verbatim by the coordinator"
  - upstream: "Claude Code hooks reference (code.claude.com/docs/en/hooks.md, fetched 2026-10-22), UserPromptExpansion: a PreToolUse hook matching the Skill tool fires only when Claude calls the tool, typing `/skillname` bypasses PreToolUse, and UserPromptExpansion fires on that direct path; it matches on `command_name`, carries `expansion_type` (`slash_command` or `mcp_prompt`), `command_name`, `command_args`, `command_source` and the typed `prompt`, and can add `additionalContext` alongside the expanded prompt. UserPromptSubmit receives the line as typed, not the expanded skill"
  - precedent: ADR-706
  - precedent: ADR-155
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Measure first"
    via: multiple-choice question in the agent-ways session, 2026-10-22; options were retire it, keep it and repair per ADR-191, or measure first (sample the 5,705 fires for on-topic rate before deciding)
    covers: [semantic]
status: proposed
date: 2026-10-22
deciders:
  - aaronsb
  - claude
related:
  - ADR-123
  - ADR-125
  - ADR-126
  - ADR-155
  - ADR-157
  - ADR-160
  - ADR-181
  - ADR-191
  - ADR-415
  - ADR-500
  - ADR-606
  - ADR-705
  - ADR-706
observable:
  - "An existing `commands: git commit` way fires on PreToolUse for a Bash `git commit` through `check-tool.sh`, and the transcript pairs its `hook_success` row with a `hook_additional_context` row"
  - "A way with `files: \\.ipynb$` fires when the model calls NotebookEdit, and a way with `agents: ^Explore$` fires when it calls Agent with subagent_type Explore"
  - "A way with `tool_name: ExitPlanMode` and `tool_event: [PostToolUse]` fires once the operator approves a plan in an interactive session"
  - "A way with `skills: ^merge$` fires when the operator types `/merge`, through a UserPromptExpansion hook with `command_name: merge`, and fires when the model calls the Skill tool with `skill: merge`, through PreToolUse; each fire is paired with a `hook_additional_context` row in the transcript, and the events log shows it once per invocation"
  - "After the build, the events log shows no `semantic:bash:*` fires; `commands:` and `files:` fires continue"
  - "`ways author lint` warns on a way that uses `reads:` or `mcp:` while that type is unwired, names the type, and says how to wire it; it rejects `tool_event: [PostToolUseFailure]`"
  - "`settings.json` carries one PreToolUse, one PostToolUse, one PostToolUseFailure and one UserPromptExpansion entry for `check-tool.sh`, each with an explicit timeout, and no entries for `check-bash-pre.sh`, `check-file-pre.sh` or `check-post.sh`"
imported:
  from: docs/architecture/system/ADR-188-posttooluse-delivery-for-tool-lane-ways-and-retirement-of-the-semantic-bash-surface.md
  format: v0
  status: Proposed
---

# ADR-188: One tool-call lane keyed by tool type, and retirement of the semantic Bash surface

## Summary

- **Decided:** ways match tool calls in one lane. The tool catalog (ADR-706) resolves each call's tool to an abstract type and the value of that type's primary field, and one hit loop matches ways against it. Each type with a field has one frontmatter key whose value is a regex over that field: `commands:` (shell), `files:` (file-modify), `reads:` (file-read), `urls:` (web), `agents:` (delegation), `skills:` (skills), `mcp:` (MCP tools). A skill has two ways in, and `skills:` catches both: the model's Skill tool call on PreToolUse, and the operator's typed `/name` on UserPromptExpansion, which bypasses PreToolUse. `tool_name:` matches the tool name for types with no useful field, and `tool_event:` lists the hook events a way fires on, PreToolUse by default. Existing `commands:` and `files:` ways keep working unchanged and now also reach PowerShell and NotebookEdit. One adapter, `check-tool.sh`, serves every tool event and absorbs the Bash, file and post-tool adapters. The shell, file-modify, delegation, web, skill and plan-mode types are wired by default; file-read and MCP are not. The semantic lane on the Bash surface retires, so a Bash command fires only the regexes an author wrote. This replaces this record's first draft, which moved tool-lane ways to PostToolUse on the belief that PreToolUse context never reaches the model; that belief was wrong.
- **Trades away:** the meaning-based match on Bash commands, which made 81% of all way fires over the three days before this record, along with whatever of it was on topic. Each newly wired tool costs one process per call per wired event. Ways on file reads and MCP calls stay silent until a user wires them, and lint can warn but cannot wire them.
- **One-way?** No, not yet. The adapter, the wiring and the semantic retirement can each be reversed in a session. The key names (`reads:`, `urls:`, `agents:`, `mcp:`, `tool_name:`, `tool_event:`) become a contract with users' own ways at the first release that ships them.
- **Probes:** *Confident (keys):* your existing ways fire as they do now. New keys let a way fire on what a file read, a web fetch, a subagent launch or an MCP call touches, and the file-read and MCP keys fire only once those tools are wired, because reads are the most frequent call and MCP servers differ per user. *Not confident (semantic):* Bash commands stop pulling ways in by meaning; only patterns an author wrote fire on a command. In the three days before this record that meaning-based matching made 5,705 of 7,034 way fires, many of them off topic. Is losing it what you want?
- **Inversion:** at one end, a key or a lane per Claude Code tool (`Bash:`, `Edit:`, `Task:`), coupled to every tool name and rename. At the other, one generic `tool:` key in Claude Code's permission-rule syntax. The decision keys on the type, which is narrower than a tool and wider than a name.

## Context

### What the first draft decided, and why it no longer holds

The first draft of this record (2026-09-19) rested on issue #528's measurement: 3,132 PreToolUse hook outputs carrying a way, none delivered, against PostToolUse outputs delivered 144 of 145. It moved every tool-lane match to PostToolUse inside `check-post.sh`, made PreToolUse a refusal-only surface (ADR-181's guards), and named the condition for undoing that: a measured `hook_additional_context` row paired with a PreToolUse output.

That condition was met ten days later. The undelivered output was this project's emit shape, a top-level `decision` and `additionalContext` that Claude Code drops silently. Once the scans emitted the `hookSpecificOutput` envelope, #528 was closed with PreToolUse delivery verified on ways-v1.22.0: in a fresh session running `git commit`, three ways arrived as PreToolUse context and the model quoted them. The code has emitted that envelope on PreToolUse since (`scan/mod.rs:1028`, docstring at 1493-1505). The move to PostToolUse and the refusal-only rule therefore have no reason left, and PreToolUse is once again where act-time guidance lands. ADR-704, proposed on PR #1115's branch, relies on that delivery and cited this record's stale draft as precedent; this rewrite removes the contradiction.

Three findings of the first draft still hold and are carried below: the semantic Bash lane's noise, the rule that a fire is recorded when it is delivered, and the rule that a failed call fires nothing.

### The tool lanes as they stand

Four lanes match tool calls, each with its own adapter, request type and scan function (ADR-705):

- `commands:` matches the Bash command on PreToolUse (`check-bash-pre.sh`), and `pattern:` matches the call's description there too;
- `files:` matches the Edit or Write path on PreToolUse (`check-file-pre.sh`);
- postchecks run per-way scripts on PostToolUse and PostToolUseFailure (`check-post.sh`);
- the task lane scores the delegation prompt semantically and delivers at SubagentStart (`check-task-pre.sh`).

ADR-704 adds a fifth, `tools:`, a regex on the tool name with a `tool_phase:` that only it understands, and its scan copies the file lane's hit loop.

`commands:` and `files:` are already type keys. Each names one kind of tool and matches a regex against that kind's primary field. Claude Code groups its own tools the same way: a permission rule's specifier targets one primary field per tool, and rule families cover several tools at once (ADR-705). The lanes do not follow those families. PowerShell never reaches `commands:`, NotebookEdit never reaches `files:`, and nothing matches a file read's path, a fetched URL, a subagent's type or an MCP tool. Each gap closed so far has added a lane.

The operator asked to take the contributor's tool lane, survey what else can be sensed, and build an attend sensor "in addition to ways triggers", and chose one frontmatter key per type over a single permission-rule key. Asked about plan approval, the moment ADR-704 was built for, the operator answered "I don't really use plan mode" and "instead, we use our own skills". Skills reach Claude Code by two routes: the model calls the Skill tool, or the operator types `/name`. The typed route makes no tool call; Claude Code expands the skill into the prompt and fires UserPromptExpansion, not PreToolUse (ADR-705). ADR-706 decides the catalog that maps Claude Code's tool names to types. This record decides the lane that uses it.

### The semantic Bash lane now delivers

The first draft retired the semantic lane on the Bash surface (ADR-155 §4) because its samples were off topic at high scores and its volume was large; at the time its output was silent. It was never removed, and since #528's fix it delivers. Over 2026-10-20 to 2026-10-22 it made 5,705 of 7,034 way fires (81%), against 668 for the `commands:` regex. Its samples still read as noise: reading ADR files with `cat` fires executive-assistant ways about meeting recaps and cross-service intelligence. ADR-191 explains why: a shell command is too sparse to carry intent, and where it is dense it collides with the wrong vocabulary. The repairs ADR-191 proposes (a lookbehind, chunking, a winner confirmation) are not built and would need calibration of their own.

## Decision

1. **One lane, keyed by tool type.** Every tool event a ways hook receives goes through one path. The catalog (ADR-706) resolves the payload's `tool_name`, legacy aliases included, to a type and the value of the type's abstract field. One hit loop then applies `scope:` and `when:`, matches each way's keys, orders the hits, applies refire (ADR-126) and the context budget, and emits the bodies in the `hookSpecificOutput` envelope for the payload's real event. ADR-704's `scan::tool` and the separate command and file scans collapse into this loop, keeping their channel names in the events log.

2. **Frontmatter keys.** Each regex compiles case-insensitively (ADR-157).

   | Key | Type | Abstract field | Tools today, through the catalog |
   |---|---|---|---|
   | `commands:` | shell | `command` | Bash, PowerShell |
   | `files:` | file-modify | `path` | Edit, Write, NotebookEdit, MultiEdit (legacy) |
   | `reads:` | file-read | `path` | Read, Glob, Grep, LSP |
   | `urls:` | web | `url` | WebFetch |
   | `agents:` | delegation | `agent` | Agent's subagent type, Workflow's name |
   | `skills:` | skill | `skill` | the Skill tool's `skill`; a typed `/name`, through UserPromptExpansion's `command_name` |
   | `mcp:` | mcp | `tool` | any `mcp__` tool, matched as `server__tool` |

   - `skills:` is a regex over the skill's name. The catalog maps both routes into the skill type (ADR-706 item 2): a Skill tool call by its `skill` field, and a UserPromptExpansion payload whose `expansion_type` is `slash_command` by its `command_name`. The typed route also covers custom commands, which Claude Code expands the same way. A typed `/name` arrives at the prompt lane as the line the operator typed, and the lane never sees the expanded skill text there; `skills:` is the trigger for the skill itself. For the skill type, `tool_event: [PreToolUse]` covers both routes, because UserPromptExpansion fires before the skill's text reaches the model, as PreToolUse does before a tool runs. `PostToolUse` fires only on the tool route; the typed route has no after-event. Whether the tool route also fires UserPromptExpansion is not verified; if it does, refire (ADR-126) holds the second fire, and the build checks which events one model-invoked skill produces.

   - `tool_name:` is a regex over the canonical tool name. It serves the types with no useful field (plan-mode, scheduling, user-interaction, publish and the rest) and tools the catalog does not map. It replaces ADR-704's `tools:`, which collides with `tools:` in agent definitions; `tool_name` is also the payload key and the session event record's field (ADR-415).
   - `tool_event:` lists literal hook event names and applies to every key above. It defaults to `[PreToolUse]`. The lane admits `PreToolUse` and `PostToolUse`. It replaces ADR-704's `tool_phase: pre|post|both`.
   - A way's keys are independent triggers: any one that matches fires the way, as `pattern:` and `commands:` do today. `pattern:` keeps matching the shell type's description field.
   - `.check.md` checks keep their scoring on the shell and file-modify types.

3. **The semantic lane on the Bash surface retires.** The lane stops scoring ways by embedding on the shell type, and the `semantic:bash:en` and `semantic:bash:multi` channels are removed. This supersedes ADR-155 §4, declared in frontmatter on both records. The embed pass remains for checks that declare `description` and `vocabulary`, fed by the lookbehind (ADR-160 stage 1), and runs only when such a check is in scope and no regex matched it. Reasoning before an action still reaches the prompt lane at the next prompt through the response-context channel (ADR-155 §3). A semantic act-time lane can return on its own decision, starting from ADR-191's pipeline and a calibration pass.

4. **One adapter.** `hooks/ways/check-tool.sh` over `ways hook tool` is the one tool-call adapter for ways. It reads `hook_event_name`. On PreToolUse it runs the lane. On UserPromptExpansion it runs the lane for the skill type and emits in that event's envelope, whose `additionalContext` Claude Code adds alongside the expanded prompt. On PostToolUse it runs the lane, then the postchecks (ADR-123) and the deferred-macro collector (ADR-606) that `ways hook post-tool` runs today, and prints one envelope. On PostToolUseFailure it runs the postchecks only. `check-bash-pre.sh`, `check-file-pre.sh` and `check-post.sh` fold into it. These stay as they are: `check-task-pre.sh`, whose semantic match on the delegation prompt is a different matcher from a field regex; the guard `strip-session-link-pre.sh` (ADR-181); `mark-tasks-active.sh`; `check-queued.sh` (ADR-161); `issues-pull.sh`; `check-pull.sh`. The adapter also writes the session event record for each tool event (ADR-415).

5. **Default wiring and its cost.** The adapter's matchers are generated from the catalog's wired set (ADR-706 item 2), one entry each on PreToolUse, PostToolUse and PostToolUseFailure, plus one UserPromptExpansion entry with an empty matcher, since skill names are not known in advance. Every entry reaches every user through the settings merge (ADR-500), and each wired tool costs one process per call per event.

   | Type | Wired by default | Cost against today |
   |---|---|---|
   | shell | yes | none for Bash: two adapters on Pre and Post become one each. PowerShell adds two per call, on Windows. |
   | file-modify | yes | none for Edit and Write. NotebookEdit adds two per call. |
   | delegation | yes | Agent gains one on Pre beside `check-task-pre.sh`; its Post spawn exists. Workflow adds two per launch. |
   | web | WebFetch, yes | two per fetch |
   | plan-mode | EnterPlanMode, ExitPlanMode, yes | two per call; rare |
   | skill | yes: Skill on PreToolUse and PostToolUse, and UserPromptExpansion with an empty matcher | two per Skill call; one per typed slash command, which the operator types by hand |
   | publish | the outward publish tools, by name | two per call; rare |
   | file-read | no | the most frequent tool; two per Read, Glob or Grep |
   | mcp | no | servers differ per user |
   | the rest | no | reachable through `tool_name:` once wired |

   The adapter entries set an explicit `timeout` of 10 seconds, so a hung binary holds a tool call for at most that long rather than Claude Code's 600-second default. A per-handler `if` may narrow a spawn for a shipped way, as an optimisation and never as the trigger. Until a ways setting wires a type, a user wires file-read or MCP with a matcher entry in their own settings.

6. **A fire is recorded when it is delivered.** A lane records the fire, stamps engagement (ADR-123) and logs `way_fired` when the body is emitted on an event measured to deliver; PreToolUse and PostToolUse in the `hookSpecificOutput` envelope both are. A lane that separates match from delivery stamps when it drains. The task stash drained at SubagentStart is checked against this rule as part of the build. This keeps the defect the first draft found from recurring: a fire nobody read stamped the way and muted the prompt lane.

7. **A failed call fires nothing.** `tool_event:` rejects `PostToolUseFailure` in lint. A match on a failed call would stamp the way and hold it back from the retry that succeeds. `PermissionRequest` and `PermissionDenied` are also rejected until a decision says what a way fire there means; the session event record observes them meanwhile (ADR-415).

8. **Lint.** `ways author lint` validates the keys against the schema, rejects an unknown `tool_event:` value, warns on `tool_event:` with no tool key, warns on a key whose type is unwired by default and names how to wire it, and warns on a `tool_name:` regex that matches no catalogued tool (ADR-706 item 6).

9. **ADR-704 becomes this lane's name-only case.** Its fields are renamed `tool_name:` and `tool_event:` before they ship in a release; its adapter path and template entries carry over. If PR #1115 merges under its own names, the rename lands before the next ways release.

10. **Bash-derived write targets (#525) feed `files:`.** A write target derived from a shell command (`sed -i`, `tee`, a redirect) is matched by the file-modify key on the same hook, with channel `bash-write`.

11. **Implementation items.**
    - The hit loop and catalog lookup in one `scan` function, with `ways hook tool` reading the event.
    - The settings template generated from the catalog, with a test that compares them.
    - The schema entries for the new keys, and the lint rules in item 8.
    - Removal of the semantic branch in the command scan, and lazy embedding for checks.
    - A delivered-versus-emitted count in `ways introspect`, so a lane that emits and never delivers is visible within days.
    - The acceptance observation: one interactive session in which a `commands:` way, an `agents:` way and a `skills:` way fire through `check-tool.sh`, the last both on a typed `/name` and on a Skill tool call, each paired with a `hook_additional_context` row, and no `semantic:bash` fire appears.

## Consequences

### Positive

- A new kind of tool trigger is a key and a catalog entry, not a lane.
- `commands:` reaches PowerShell and `files:` reaches NotebookEdit with no edit to a way.
- Ways can fire on what a subagent launch, a web fetch or a plan approval carries, at act time.
- Way fires on the Bash surface fall from thousands a day to the regexes authors wrote. The tail of calls carrying four or more off-topic ways goes with the semantic lane.
- One adapter and one loop replace four adapters and three copies of a loop, and the hook count per Bash and Edit call does not rise.

### Negative

- Guidance that the semantic lane delivered on topic is lost on the Bash surface. It still reaches the prompt lane at the next prompt.
- Each newly wired tool adds a process per call per event.
- A way on an unwired type lints with a warning and stays silent. A user who wants `reads:` must wire Read, and pays its frequency.
- Six new frontmatter keys to learn. `tool_name:` remains coupled to Claude Code's names.

### Neutral

- ADR-155 §4 is superseded by item 3; the rest of ADR-155 stands.
- The task lane keeps its own adapter and semantic match. `agents:` sits beside it on the same tool.
- PostToolUse remains the place for postchecks, the queued-message lane (ADR-161) and the deferred-macro collector (ADR-606). PreToolUse carries guidance again, as well as guards.
- Events logged on `semantic:bash:*` before the build stay in the log and read as delivered from 2026-09-29 on.

## Alternatives Considered

- **Keep this record's first draft: move tool-lane matching to PostToolUse.** Rejected. Its reason, that PreToolUse context is never delivered, was this project's emit shape, and #528 closed with PreToolUse delivery verified. Moving to PostToolUse would turn act-time guidance into after-the-fact guidance for no gain.
- **Reject this record and accept ADR-704 as a separate lane.** The cheapest path. Rejected as the end state: a lane per gap means a scan, a request type, an adapter and a phase concept per lane, and the field-matching gaps (PowerShell, NotebookEdit, reads, URLs, agent types, MCP) stay open. ADR-704's mechanism is kept as the name-only case.
- **A single `tool:` key in permission-rule syntax** (`Bash(git *)`, `Edit(**/*.rs)`). It is Claude Code's own syntax and matches the hook `if` field. Not chosen: the operator picked one key per type. It would also name harness tools in every way and use globs where 54 existing ways use regexes.
- **A key per tool** (`bash:`, `edit:`, `notebook_edit:`). Rejected: every tool rename or new family member becomes a way edit, which is the coupling ADR-706 exists to remove.
- **Keep the semantic Bash lane, or rebuild it on ADR-191's pipeline now.** Rejected for this record. The lane delivers today at 81% of all fires with off-topic samples, and ADR-191's repairs are unbuilt and uncalibrated. A semantic act-time lane can return on its own decision.
- **Wire every tool (`.*`) so any way works without wiring.** Rejected: one process per tool call per event, Read included, to serve a handful of ways.
- **Fire on PostToolUseFailure too.** Rejected for the reason in item 7.
- **A separate hook command for the lane on PostToolUse beside `check-post.sh`.** Rejected: a second process per call where one already reads the same payload and prints the envelope.
