---
contract: adr/v1
kind: evidence
capability: matching
status: proposed
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-155
  - ADR-188
  - ADR-191
  - ADR-406
  - ADR-606
---

# ADR-705: Claude Code's tool surface and hook events, surveyed as tool types for trigger lanes and session sensing

## What this record is

A survey taken on 2026-10-22 of the tools Claude Code offers the model and the hook events it sends, read for two uses: which tool calls ways could trigger on, and which session events an attend sensor could observe. It decides nothing. ADR-188, ADR-706 and ADR-415 cite it.

Sources:

- the installed Claude Code 2.1.308 binary, read with `strings`;
- the tool-input declarations Claude Code writes beside a loaded mod, `BuiltinToolInputs` in `claude-code-tools/index.d.ts` under the plugin cache, 73 tools, written 2026-10-20;
- the hook payload declarations in the sibling `claude-code/index.d.ts`, written by 2.1.304 (the `HookInput` union, and `BaseHookInput`);
- the hooks and permissions references at code.claude.com/docs/en/hooks and code.claude.com/docs/en/permissions.

A statement marked *inferred* was not read directly from one of these.

## Where tools come from

| Source | Names | Notes |
|---|---|---|
| Built-in | a canonical name such as `Bash` or `Agent` | 73 in this build's declarations. The binary names more, among them `Glob`, `Grep`, `PowerShell`, `SubagentHandback`, a structured-output tool and nine `self_hosted_runner_*` tools. |
| Deferred built-ins | the same names | 46 tool definitions in the binary are deferred: the model loads their schema through ToolSearch. Hooks key on the call, so deferral does not change which hooks fire (inferred). |
| MCP servers | `mcp__<server>__<tool>`; plugin servers `mcp__plugin_<plugin>_<server>__<tool>`; claude.ai connectors `mcp__claude_ai_<server>__<tool>` | Arguments follow each server's own schema. No argument field is common to all of them. |
| Mod-registered tools | MCP names, such as `mcp__attend__attend_send` | They fall in the MCP class and need the mods preview flag, which is off by default. |

Some tools exist only under a mode, role, surface or platform: `ExitPlanMode` in plan mode, `SubagentHandback` in a subagent (inferred), team tools in a team, claude.ai and Cowork tools on those surfaces, `PowerShell` on Windows. `Glob` and `Grep` are named in the binary but absent from this build's tool set; search runs through Bash on this install (inferred). Neither plan-mode tool is offered under `claude -p`.

**Legacy names.** Matchers and permission rules resolve old names through a `legacyToolNames` table in the binary: `Task` to `Agent`, `KillShell` and `KillBash` to `TaskStop`, `ListPeers` to `ListAgents`, `Brief` to `SendUserMessage`, and others. A shipped matcher naming `Task` therefore still reaches `Agent`. Payloads carry the canonical name (inferred).

## Claude Code already groups its tools

Permission rules are written `Tool` or `Tool(specifier)`, and the specifier targets one primary field per tool: `command` for Bash and PowerShell, `file_path` for Read, Edit and Write, `path` for Grep and Glob, `notebook_path` for NotebookEdit, and the URL's domain for WebFetch. Rules also group tools into families: an `Edit` rule covers every file-editing tool, and a `Read` rule covers Grep and Glob on a best-effort basis. The hook `if` field uses the same syntax on the five tool events and is evaluated before the hook process is spawned. A settings file rejects an `mcp__` rule with a parenthesised specifier.

Grouping the tool set along those families gives these types. Value is rated H (high), M (medium), L (low) or N (noise), for ways and for attend.

| Type | Tools | Primary field | Ways | attend |
|---|---|---|---|---|
| shell | Bash, PowerShell | `command` | H: 23 ways use `commands:` | L; a finished background shell is M, seen through Stop |
| file-modify | Edit, Write, NotebookEdit, MultiEdit (legacy) | `file_path`, `notebook_path` | H: 31 ways use `files:`; NotebookEdit is not wired | N; the git sensor covers it |
| file-read | Read, Glob, Grep, LSP | `file_path`, `path`, `filePath` (LSP is camelCase) | M; the most frequent tool, so the spawn cost is highest | N |
| web | WebFetch, WebSearch | `url`; WebSearch has `query` and no URL | M: the moment untrusted content enters | N |
| delegation | Agent, Workflow | `subagent_type`; Workflow `name` | H: the task lane exists | H: subagent and workflow start and finish |
| plan/mode | EnterPlanMode, ExitPlanMode, ProposeGoal, EnterWorktree, ExitWorktree | none useful; the tool name | H at ExitPlanMode, after approval | H: plan approved, goal set, worktree entered |
| task/state | TaskCreate, TaskUpdate, TaskGet, TaskList, TaskStop, TodoWrite | `subject`, `status` | M | M; the TaskCreated and TaskCompleted events carry the same state with less noise |
| scheduling | CronCreate and its siblings, ScheduleWakeup, Monitor, RemoteTrigger | the tool name; Monitor also takes a `command` | M | H: the session will wake on its own |
| messaging | SendMessage, ListAgents, SendUserMessage, PushNotification, `mcp__attend__*` | `to` | L | M |
| user-interaction | AskUserQuestion, EndConversation | the tool name | M | H: the agent is waiting on its human |
| extension | Skill, ToolSearch, the plugin and connector search tools | `skill`, `query` | M for Skill; N for the rest | L |
| publish | Artifact and its siblings, PublishPlugin, SendFeedback | `action`, `url` | H: an outward step | M |
| memory | memory_list, memory_read, memory_write | `path` | M | L |
| MCP (class) | `mcp__<server>__<tool>` | the name; no common argument field | H per server, such as a mail or chat server's send posture | M: an audit of external writes |
| output, runner ops | ReportFindings, structured output, `self_hosted_runner_*` | none | N | N |

## The current ways lanes on this grid

| Lane | Type and field | Event | Adapter |
|---|---|---|---|
| `commands:` | shell, `command`; Bash only | PreToolUse | `check-bash-pre.sh`, `ways hook command` |
| `files:` | file-modify, `file_path`; Edit and Write, NotebookEdit missed | PreToolUse | `check-file-pre.sh`, `ways hook file` |
| postcheck | Edit, Write, Bash, Task; any predicate over the payload | PostToolUse, PostToolUseFailure | `check-post.sh`, `ways hook post-tool` |
| task lane | delegation; semantic match on the prompt | PreToolUse on Task, delivered at SubagentStart | `check-task-pre.sh` |
| `tools:` (ADR-704, proposed on PR #1115's branch) | any wired tool; the name only | PreToolUse and PostToolUse | `check-tool.sh`, `ways hook tool` |

`commands:` and `files:` each name one type and match a regex against that type's primary field. The `tools:` lane matches the name alone. Its scan copies the file lane's hit loop.

## Hook events beyond tool calls

Every event carries `session_id`, `transcript_path`, `cwd`, `permission_mode`, `effort`, and `agent_id` with `agent_type` inside a subagent. Comparing `permission_mode` across consecutive events recovers a plan-mode switch made with Shift+Tab, which fires no hook (inferred, not probed).

| Event and key fields | Known gaps | Ways | attend |
|---|---|---|---|
| PermissionRequest (tool_name, tool_input) | none known | L | H: waiting on approval |
| PermissionDenied (tool_name, reason) | auto mode only | M | M |
| SubagentStart, SubagentStop (agent_id, agent_type, agent_transcript_path, last_assistant_message) | none known | SubagentStart wired | H |
| Stop (last_assistant_message, background_tasks with id, type, status and name; session_crons) | not on a user interrupt | wired | H: comparing `background_tasks` between Stops shows workflows, shells and monitors finishing |
| StopFailure (error, error_details) | none known | L | H |
| TaskCreated, TaskCompleted, TeammateIdle | none known | TaskCreated wired | H |
| PreCompact, PostCompact (trigger, compact_summary) | none known | M | H: the context sensor's ground truth |
| PreModelSwitch, PostModelSwitch (from_model, to_model, source, context_tokens) | 30 s timeout | M | H: the window size changed |
| SessionStart (source), SessionEnd (reason) | SessionEnd shares a 1.5 s budget | SessionStart wired | M |
| UserPromptSubmit (prompt, source), UserPromptExpansion | none known | wired | L |
| Notification (notification_type: permission_prompt, idle_prompt) | none known | L | H |
| CwdChanged, WorktreeCreate, WorktreeRemove | none known | L | M |
| ConfigChange, InstructionsLoaded, FileChanged | InstructionsLoaded skips direct AGENTS.md reads | M (#493) | L |
| MessageDisplay, PostToolBatch, DirectoryAdded, Setup, Elicitation | MessageDisplay fires on every streamed chunk; PostToolBatch on every step | N | N |

Gaps on tool calls themselves: `EndConversation` fires neither PreToolUse nor PostToolUse. PostToolUseFailure does not fire for a call rejected before it runs. Workflow's PostToolUse fires at launch, with `status: async_launched`, not at completion.

## What a hook entry costs

- Every entry in the shipped `settings.json` reaches every user through the three-way merge, which adds entries per event and leaves the user's own entries in place.
- Each entry spawns one process per matching event. With no `timeout` set, Claude Code's default is 600 seconds, and a hung synchronous hook blocks the tool or the turn until then.
- An entry with `async: true` never blocks. It suits an event that only feeds a sensor.
- SessionEnd hooks share a 1.5 second budget.
- A per-handler `if` stops a spawn before it happens, but holds one rule per handler and is best effort, so it can only narrow a matcher.

## Where attend could observe the session

attend's sensors are polled (`Sensor::poll` in `sensor-trait`), and hooks push one process per event. Three routes join them:

- The ways hooks already receive every payload they are wired for (#846). They could write each event to a file that a sensor reads on its poll. attend already reads ways state this way: its context and disclosure sensors call `ways context --json`.
- The attend mod registers in-process `tool.call`, `turn.complete` and `session.*` handlers (`plugins/attend/hooks/register.tsx`). It runs only with the mods preview flag on.
- A hook could call `attend` itself, one more process per event. The hooks today invoke one binary and add no logic (`attend-drain-stop.sh`).
