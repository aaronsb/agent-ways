---
contract: adr/v1
kind: decision
verb: change
capability:
  - matching
  - authoring
  - disclosure
supersedes: ADR-155#4
extends: ADR-606
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
  - operator: aaronsb
    level: directed
    said: "I'd like to discuss the nature of the bash commands before and after, and if there's something we're missing about the catpure of the commands. it's clear that claude uses bash commands for just about everything, and discarding the signal lane seems like a regression in capability."
    via: "the operator in the head session's chat, 2026-10-22, after the on-topic measurement; relayed verbatim by the coordinator"
  - operator: aaronsb
    level: directed
    said: "I have a strong suspicion that each 'bash tool' is actually a whole universe of specific tools, sometimes in pipes etc."
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - operator: aaronsb
    level: directed
    said: "conceptually, we could stop naievely matching a bash cool call and instead parse it. use 'tldr' or man or something (an actual tool with a dataset we can pull in) as part of the descriptive pipeline about what that tool is about to run, and match on what that parsing is. (against a way/match/encoded etc) . there's even super fast tldr crates that run faster too. (we want to stay performance optimized)"
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - operator: aaronsb
    level: directed
    said: "so the flow is [parse the bash call in order for the actual tools]-> [match to ways corpus] -> [present findings to judge, including the explainations of the tdlr in order if piped] -> judge 'judges'"
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - operator: aaronsb
    level: directed
    said: "it feels like theres a risk of building an unmaintainable matcher monolith here. let's be careful to keep concerns seperated"
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - operator: aaronsb
    level: directed
    said: "Rebuild per the flow"
    via: "the operator's pick in a multiple-choice question in the head session, 2026-10-22, after the surface ablation and the judge study; the option label was agent-written"
  - operator: aaronsb
    level: directed
    said: "this can be a use of our late response pipeline"
    via: "the operator in the head session, 2026-10-22, with the pick \"Parallel, deliver post\" (an agent-written option label), naming ADR-606's deferred pipeline as the carrier"
  - evidence: ADR-707, the three measurements of the Bash surface - the semantic lane at about 3% on topic with no score threshold that separates, the alternative match surfaces and the segment parse, and the judge reading an ordered described block (0.175 on-topic and 0.14 off-topic injections per call for verbs plus description, against 0.025 and 1.36 for today's unjudged lane)
  - evidence: "tools/ways-cli/src/cmd/scan/mod.rs:397: `gate::apply` (ADR-196) has one call site, in the prompt lane; Bash-lane hits are injected unjudged"
  - evidence: "tools/ways-cli/src/cmd/scan/lookbehind.rs:31: `intent(session_id)` reads `find_transcript(None, session_id)`, the main transcript, for every caller, subagents included"
  - evidence: "ADR-606 is accepted and unbuilt: no `macro_defer` handling, deferred result store or collector exists under tools/ways-cli or tools/ways-core on 2026-10-22"
  - precedent: ADR-706
  - precedent: ADR-606
  - precedent: ADR-196
  - precedent: ADR-155
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Measure first"
    via: multiple-choice question in the agent-ways session, 2026-10-22; options were retire it, keep it and repair per ADR-191, or measure first (sample the 5,705 fires for on-topic rate before deciding)
    covers: [semantic]
  - operator: aaronsb
    said: "Rebuild per the flow (Recommended)"
    via: "multiple-choice question in the agent-ways session, 2026-10-22, after the surface ablation and the judge study; option text: the Bash lane is rebuilt as parse, then match on command verbs + Bash description, then judge reading the ordered tldr-described block, plus per-segment commands: regexes; supersedes 'retire'"
    covers: [semantic]
  - operator: aaronsb
    said: "Parallel, deliver post (Recommended). this can be a use of our late response pipeline"
    via: "multiple-choice question in the agent-ways session, 2026-10-22, after the surface ablation and the judge study; option text: the command runs immediately, the judge runs alongside, approved ways arrive on PostToolUse; the operator then named ADR-606's deferred (late response) pipeline as the carrier"
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
  - ADR-156
  - ADR-157
  - ADR-160
  - ADR-181
  - ADR-191
  - ADR-196
  - ADR-415
  - ADR-500
  - ADR-606
  - ADR-705
  - ADR-706
  - ADR-707
observable:
  - "An existing `commands: git commit` way fires on PreToolUse for a Bash `git commit` through `check-tool.sh`, and the transcript pairs its `hook_success` row with a `hook_additional_context` row"
  - "A way with `commands: ^gh\\s+pr` fires on `git push && gh pr create`, matched on the call's second segment"
  - "A way with `files: \\.ipynb$` fires when the model calls NotebookEdit, and a way with `agents: ^Explore$` fires when it calls Agent with subagent_type Explore"
  - "A way with `tool_name: ExitPlanMode` and `tool_event: [PostToolUse]` fires once the operator approves a plan in an interactive session"
  - "A way with `skills: ^merge$` fires when the operator types `/merge`, through a UserPromptExpansion hook with `command_name: merge`, and fires when the model calls the Skill tool with `skill: merge`, through PreToolUse; each fire is paired with a `hook_additional_context` row in the transcript, and the events log shows it once per invocation"
  - "ADR-707's uniform-call table, re-run on a fresh held-out set of 200 Bash calls drawn after this record and graded blind under the same rubric, shows the shipped matcher and judge delivering more on-topic and fewer off-topic injections per call than today's unjudged lane, with both matchers' rows (verbs plus description, and tldr text) recorded here and the shipped one named"
  - "In a live session, a way the judge passes for a Bash call arrives at that agent's next PostToolUse or UserPromptSubmit under ADR-606's header naming the way, the Bash lane, the segment that matched and its age; a verdict collected past the relevance budget arrives as ADR-606's one-line pointer"
  - "Over 100 Bash calls with candidates, the PreToolUse hook's median wall time with the judge on is within 50 ms of its median with the judge off, and every hook returns before its judge request completes"
  - "A subagent's lookbehind text comes from the subagent's own transcript, and a lead's prose never appears in it"
  - "`ways author lint` warns on a way that uses `reads:` or `mcp:` while that type is unwired, names the type, and says how to wire it; it rejects `tool_event: [PostToolUseFailure]`"
  - "`settings.json` carries one PreToolUse, one PostToolUse, one PostToolUseFailure and one UserPromptExpansion entry for `check-tool.sh`, each with an explicit timeout, and no entries for `check-bash-pre.sh`, `check-file-pre.sh` or `check-post.sh`"
imported:
  from: docs/architecture/system/ADR-188-posttooluse-delivery-for-tool-lane-ways-and-retirement-of-the-semantic-bash-surface.md
  format: v0
  status: Proposed
---

# ADR-188: One tool-call lane keyed by tool type, and a Bash surface that is parsed, described and judged

## Summary

- **Decided:** ways match tool calls in one lane. The tool catalog (ADR-706) resolves each call's tool to an abstract type and the value of that type's primary field, and one hit loop matches ways against it. Each type with a field has one frontmatter key whose value is a regex over that field: `commands:` (shell), `files:` (file-modify), `reads:` (file-read), `urls:` (web), `agents:` (delegation), `skills:` (skills), `mcp:` (MCP tools). A skill has two ways in, and `skills:` catches both: the model's Skill tool call on PreToolUse, and the operator's typed `/name` on UserPromptExpansion, which bypasses PreToolUse. `tool_name:` matches the tool name for types with no useful field, and `tool_event:` lists the hook events a way fires on, PreToolUse by default. Existing `commands:` and `files:` ways keep working unchanged and now also reach PowerShell and NotebookEdit. One adapter, `check-tool.sh`, serves every tool event and absorbs the Bash, file and post-tool adapters. The shell, file-modify, delegation, web, skill and plan-mode types are wired by default; file-read and MCP are not. The semantic lane on the Bash surface is rebuilt, not retired: each call is parsed into its ordered segments, `commands:` regexes run per segment and fire at once, and a matcher on the call's verbs and description proposes candidate ways that the relevance judge (ADR-196) reads against the described call. The command never waits for the judge; approved ways arrive through ADR-606's deferred pipeline at the agent's next tool call or prompt. The pieces are separate components with one-way dependencies. This replaces this record's first draft, which moved tool-lane ways to PostToolUse on the belief that PreToolUse context never reaches the model; that belief was wrong.
- **Trades away:** act-time delivery for meaning-based Bash guidance, which arrives after the command runs. A judge request on about half of Bash calls, at about a tenth of a cent each, and a network dependency for that part of the lane. About a fifth of the judged injections were still off topic in the study. The lane waits on ADR-606's deferred pipeline, which is unbuilt. Each newly wired tool costs one process per call per wired event, and ways on file reads and MCP calls stay silent until a user wires them.
- **One-way?** No, not yet. The adapter, the wiring and the Bash pipeline can each be reversed in a session. The key names (`reads:`, `urls:`, `agents:`, `skills:`, `mcp:`, `tool_name:`, `tool_event:`) become a contract with users' own ways at the first release that ships them.
- **Probes:** *Confident (semantic):* Bash commands keep pulling in guidance by meaning, from what the command does and its description rather than from the surrounding conversation, with the judge deciding; the patterns authors wrote still fire before the command, and the judged guidance arrives just after it. *Not confident (cost):* about half of Bash calls send a judge request at about a tenth of a cent each, roughly $8 over the 8,000 Bash calls of the three days measured. Is that spend acceptable for this lane?
- **Inversion:** at one end, a key or a lane per Claude Code tool (`Bash:`, `Edit:`, `Task:`), coupled to every tool name and rename. At the other, one generic `tool:` key in Claude Code's permission-rule syntax. The decision keys on the type, which is narrower than a tool and wider than a name. On the Bash surface it sits between injecting every embedding match at act time, as today, and matching only authored regexes: meaning-based candidates are kept, and a judge decides which reach the model.

## Context

### What the first draft decided, and why it no longer holds

The first draft of this record (2026-09-19) rested on issue #528's measurement: 3,132 PreToolUse hook outputs carrying a way, none delivered, against PostToolUse outputs delivered 144 of 145. It moved every tool-lane match to PostToolUse inside `check-post.sh`, made PreToolUse a refusal-only surface (ADR-181's guards), and named the condition for undoing that: a measured `hook_additional_context` row paired with a PreToolUse output.

That condition was met ten days later. The undelivered output was this project's emit shape, a top-level `decision` and `additionalContext` that Claude Code drops silently. Once the scans emitted the `hookSpecificOutput` envelope, #528 was closed with PreToolUse delivery verified on ways-v1.22.0: in a fresh session running `git commit`, three ways arrived as PreToolUse context and the model quoted them. The code has emitted that envelope on PreToolUse since (`scan/mod.rs:1028`, docstring at 1493-1505). The move to PostToolUse and the refusal-only rule therefore have no reason left, and PreToolUse is once again where act-time guidance lands. ADR-704, proposed on PR #1115's branch, relies on that delivery and cited this record's stale draft as precedent; this rewrite removes the contradiction.

Two findings of the first draft still hold and are carried below: a fire is recorded when it is delivered, and a failed call fires nothing. Its third, retiring the semantic Bash lane, was measured and replaced (below).

### The tool lanes as they stand

Four lanes match tool calls, each with its own adapter, request type and scan function (ADR-705):

- `commands:` matches the Bash command on PreToolUse (`check-bash-pre.sh`), and `pattern:` matches the call's description there too;
- `files:` matches the Edit or Write path on PreToolUse (`check-file-pre.sh`);
- postchecks run per-way scripts on PostToolUse and PostToolUseFailure (`check-post.sh`);
- the task lane scores the delegation prompt semantically and delivers at SubagentStart (`check-task-pre.sh`).

ADR-704 adds a fifth, `tools:`, a regex on the tool name with a `tool_phase:` that only it understands, and its scan copies the file lane's hit loop.

`commands:` and `files:` are already type keys. Each names one kind of tool and matches a regex against that kind's primary field. Claude Code groups its own tools the same way: a permission rule's specifier targets one primary field per tool, and rule families cover several tools at once (ADR-705). The lanes do not follow those families. PowerShell never reaches `commands:`, NotebookEdit never reaches `files:`, and nothing matches a file read's path, a fetched URL, a subagent's type or an MCP tool. Each gap closed so far has added a lane.

The operator asked to take the contributor's tool lane, survey what else can be sensed, and build an attend sensor "in addition to ways triggers", and chose one frontmatter key per type over a single permission-rule key. Asked about plan approval, the moment ADR-704 was built for, the operator answered "I don't really use plan mode" and "instead, we use our own skills". Skills reach Claude Code by two routes: the model calls the Skill tool, or the operator types `/name`. The typed route makes no tool call; Claude Code expands the skill into the prompt and fires UserPromptExpansion, not PreToolUse (ADR-705). ADR-706 decides the catalog that maps Claude Code's tool names to types. This record decides the lane that uses it.

### The semantic Bash lane, measured

The semantic lane on the Bash surface (ADR-155 §4) embeds the assistant's prose since the last human turn, the command and its description, and injects every way that clears the calibrated threshold, unjudged; the relevance gate (ADR-196) runs only on the prompt lane. Since #528's fix its output reaches the model. ADR-707 measured it over 2.7 days and 8,016 Bash calls:

- It made 5,705 of 7,034 way fires (81%). About 3% were on topic and 77% off topic, and no score threshold separates the two.
- The matched text is mostly prose, not the command, and a subagent's call is matched against its lead's prose, because the lookbehind reads the main transcript for every caller (`lookbehind.rs:31`).
- A Bash call is several tools: a mean of 3.1 operative segments per call, and most calls with an action segment also carry reads. The `commands:` regex runs on the raw string, so a pattern anchored at `^gh` misses `gh` after `git push &&`.
- Matching on the call's verbs and its description, with narration and arguments removed, found on-topic guidance 7.6% of the time on uniformly drawn calls, against 0% graded for today's surface, and found 52 on-topic fires on calls where nothing semantic had fired. Forty of the 98 on-topic fires it found belonged to ways with no `commands:` field.
- The relevance judge reading an ordered description of the call (its description, then each segment with its connector and a tldr-pages description) passed 0.175 on-topic and 0.14 off-topic injections per call for those candidates, against 0.025 and 1.36 for today's unjudged lane. Adding narration to the judge's input brought the off-topic passes back.
- A judge request took 1.4 s at the median and 5.9 s at p95, so a synchronous judge would hold most Bash calls that carry candidates.

The draft's retirement would have removed the off-topic volume and the on-topic guidance with it. The operator read retirement as "a regression in capability", suspected that "each 'bash tool' is actually a whole universe of specific tools, sometimes in pipes", and set the flow: parse the call in order, match to the ways corpus, present the findings to the judge with the tldr explanations in order, and let the judge decide. They chose to run the judge alongside the command and deliver through the deferred pipeline ADR-606 decided, and asked that the concerns stay separated rather than grow into a matcher monolith.

## Decision

1. **One lane, keyed by tool type.** Every tool event a ways hook receives goes through one path. The catalog (ADR-706) resolves the payload's `tool_name`, legacy aliases included, to a type and the value of the type's abstract field. One hit loop then applies `scope:` and `when:`, matches each way's keys, orders the hits, applies refire (ADR-126) and the context budget, and emits the bodies in the `hookSpecificOutput` envelope for the payload's real event. ADR-704's `scan::tool` and the separate command and file scans collapse into this loop, keeping their channel names in the events log.

2. **Frontmatter keys.** Each regex compiles case-insensitively (ADR-157).

   | Key | Type | Abstract field | Tools today, through the catalog |
   |---|---|---|---|
   | `commands:` | shell | `command`, matched per operative segment (item 3) | Bash, PowerShell |
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

3. **The Bash surface is parsed, described and judged.** This replaces the semantic lane ADR-155 §4 added and supersedes that section, declared in frontmatter on both records.

   **a. Parse.** The command describer (item 4) splits each call into ordered segments. Each segment carries the connector before it (`;`, `&&`, `||`, a pipe, `&`, a newline, a subshell or a command substitution), a role (navigation, filter or operative), its program, subcommand and flags, and a class (publish, version-control write, service, package, build and test, file write, version-control read, search, read and inspect, other). Heredoc bodies are dropped. Each `commands:` regex runs against each operative segment's own text, not the raw command, so a pattern anchored at the start matches after a connector, and one segment's arguments cannot satisfy a pattern meant for another program. Regex fires go out at once on PreToolUse with no judge, as authored triggers do today. Bash-derived write targets (#525) come from the same segments (item 10).

   **b. Candidates.** The matcher scores ways against a text built from the call's operative verbs (program, subcommand and flags per operative segment, with arguments and paths dropped) and the Bash tool's `description` field, through the calibrated fire rule (ADR-156). Ways that clear it are candidates, not injections. No narration enters the matched text. The alternative recorded here is the tldr-text surface: the description, then each segment's tldr description in order, joined by its connector in words. Before this record is accepted, the two are compared on a fresh held-out set of uniformly drawn Bash calls under ADR-707's blind protocol, and the one with more on-topic injections per call, at no more off-topic injections per call, ships. If their intervals overlap, verbs plus description ships, since it needs no tldr text at match time and measured better on every column.

   **c. Judge.** The candidates go to the relevance gate (ADR-196), batched as on the prompt lane. The judge reads an ordered block: `About to run: <description>`, then each operative and filter unit in order, led by its connector in words, with its description from the describer (a tldr-pages example or summary, or an authored entry, ADR-706 item 8). Navigation segments are dropped. Narration is left out of the judge's input as well as the matcher's. Only ways the judge passes at the gate's threshold are delivered. A judge error, a missing API key or a run past ADR-606's 60 s bound delivers nothing from this lane and is logged; the regex fires of part a are unaffected.

   **d. Timing.** The command never waits for the judge. At PreToolUse the adapter emits the regex fires, writes the candidates and the ordered block as a deferred job in the calling agent's deferred directory (ADR-606 §1), starts the judge detached, and returns. The verdict is delivered through ADR-606's deferred pipeline: the collector picks it up at the same agent's next PostToolUse or UserPromptSubmit, often the same Bash call's PostToolUse, and injects each approved way once under ADR-606's header, naming the way, the Bash lane and the segment that matched, and the age. ADR-606's relevance budget applies unchanged; a verdict collected past it becomes ADR-606's one-line pointer to the result file. A subagent's verdict is collected by that subagent, and its budget is time alone. ADR-606 is accepted and unbuilt: the engine has no `macro_defer` handling, result store or collector. This lane is the pipeline's second consumer, so the build of the store and the collector serves both: the collector reads results by kind, a macro's output or a judge's verdict, from the same per-agent directory and under the same budget.

   **e. Lookbehind.** Any lookbehind reads the calling agent's own transcript: a subagent's own file for a subagent, the main transcript for the main agent. Today `lookbehind::intent` reads the main transcript for every caller (`lookbehind.rs:31`). The fix applies to the check-scoring embed pass, which keeps the lookbehind (ADR-160 stage 1) and runs only when a check in scope declares `description` and `vocabulary` and no regex matched it. The Bash lane's matcher and judge use no lookbehind.

4. **Components and their dependencies.** The lane is built from separate components. Each depends only on the ones it names, no component calls back into one that calls it, and each is tested on its own inputs.

   | Component | Owns | Depends on | Knows nothing of |
   |---|---|---|---|
   | Hook adapter (`check-tool.sh`, `ways hook tool`) | reading the payload, writing the session event record (ADR-415), calling the components below in order, emitting the envelope | the catalog, the describer, the matcher, the judge gate, ADR-606's store | how any of them work |
   | Tool catalog (ADR-706) | tool names to types and fields; command entries and their descriptions | nothing; it is data | ways, hooks, embeddings |
   | Command describer | parsing a command string into ordered, described segments | the catalog's command data | ways, embeddings, the judge, hook events; its own crate |
   | Edge index builder | precomputed edges from command entries to candidate ways, at corpus build | the catalog, the corpus | hooks; optional for verbs plus description, which embeds the description per call, and needed by a description-free matcher |
   | Matcher | scoring ways against abstract signals (type, field value, segment verbs, description) | the corpus, the edge index | Claude Code's names, hook events, the judge |
   | Judge gate (ADR-196) | yes or no per candidate against the ordered block | the model client in ways-agent | how candidates were found |
   | Lint and reach reports | `ways author lint`, `ways status`, the reach and delivered-versus-emitted counts in `ways introspect` | logged results, the catalog, the describer | nothing calls them |

   The figure shows the flow and the dependencies.

   ![The tool-call lane. At corpus build, tldr-pages data, authored entries and user overrides form the tool catalog's command data, and the way corpus with that data feeds the optional edge index. At a Bash call's PreToolUse, the hook adapter writes the event record and passes the command and description to the command describer, which returns ordered segments with roles, classes and descriptions. Per-segment commands: regexes fire at once in the hook's envelope. The matcher turns operative verbs plus the description into candidate ways, which the judge gate reads against the ordered described block in a detached run. The verdict lands in the deferred result store, and the collector delivers approved ways at the agent's next PostToolUse or UserPromptSubmit with ADR-606's header, or a one-line pointer past the relevance budget. Lint and reach reports read the logged results.](figures/ADR-188-tool-call-lane-flow.svg)

5. **One adapter.** `hooks/ways/check-tool.sh` over `ways hook tool` is the one tool-call adapter for ways. It reads `hook_event_name`. On PreToolUse it runs the lane, and for the shell type starts the detached judge of item 3. On UserPromptExpansion it runs the lane for the skill type and emits in that event's envelope, whose `additionalContext` Claude Code adds alongside the expanded prompt. On PostToolUse it runs the lane, then the postchecks (ADR-123) and ADR-606's collector, and prints one envelope. On PostToolUseFailure it runs the postchecks only. `check-bash-pre.sh`, `check-file-pre.sh` and `check-post.sh` fold into it. These stay as they are: `check-task-pre.sh`, whose semantic match on the delegation prompt is a different matcher from a field regex; the guard `strip-session-link-pre.sh` (ADR-181); `mark-tasks-active.sh`; `check-queued.sh` (ADR-161); `issues-pull.sh`; `check-pull.sh`. The prompt lane's hook runs ADR-606's collector on UserPromptSubmit.

6. **Default wiring and its cost.** The adapter's matchers are generated from the catalog's wired set (ADR-706 item 2), one entry each on PreToolUse, PostToolUse and PostToolUseFailure, plus one UserPromptExpansion entry with an empty matcher, since skill names are not known in advance. Every entry reaches every user through the settings merge (ADR-500), and each wired tool costs one process per call per event.

   | Type | Wired by default | Cost against today |
   |---|---|---|
   | shell | yes | none for Bash: two adapters on Pre and Post become one each. A judge request on calls with candidates, about half of them, at about $0.001 per Bash call (ADR-707). PowerShell adds two per call, on Windows. |
   | file-modify | yes | none for Edit and Write. NotebookEdit adds two per call. |
   | delegation | yes | Agent gains one on Pre beside `check-task-pre.sh`; its Post spawn exists. Workflow adds two per launch. |
   | web | WebFetch, yes | two per fetch |
   | plan-mode | EnterPlanMode, ExitPlanMode, yes | two per call; rare |
   | skill | yes: Skill on PreToolUse and PostToolUse, and UserPromptExpansion with an empty matcher | two per Skill call; one per typed slash command, which the operator types by hand |
   | publish | the outward publish tools, by name | two per call; rare |
   | file-read | no | the most frequent tool; two per Read, Glob or Grep |
   | mcp | no | servers differ per user |
   | the rest | no | reachable through `tool_name:` once wired |

   The adapter entries set an explicit `timeout` of 10 seconds, so a hung binary holds a tool call for at most that long rather than Claude Code's 600-second default; the detached judge does not count against it. A per-handler `if` may narrow a spawn for a shipped way, as an optimisation and never as the trigger. Until a ways setting wires a type, a user wires file-read or MCP with a matcher entry in their own settings.

7. **A fire is recorded when it is delivered.** A lane records the fire, stamps engagement (ADR-123) and logs `way_fired` when the body is emitted on an event measured to deliver; PreToolUse and PostToolUse in the `hookSpecificOutput` envelope both are. A lane that separates match from delivery stamps when it drains: a judged Bash way is recorded when the collector delivers it, not when the judge passes it, and a verdict that becomes a pointer records no fire. The task stash drained at SubagentStart is checked against this rule as part of the build. This keeps the defect the first draft found from recurring: a fire nobody read stamped the way and muted the prompt lane.

8. **A failed call fires nothing.** `tool_event:` rejects `PostToolUseFailure` in lint. A match on a failed call would stamp the way and hold it back from the retry that succeeds. `PermissionRequest` and `PermissionDenied` are also rejected until a decision says what a way fire there means; the session event record observes them meanwhile (ADR-415).

9. **Lint.** `ways author lint` validates the keys against the schema, rejects an unknown `tool_event:` value, warns on `tool_event:` with no tool key, warns on a key whose type is unwired by default and names how to wire it, and warns on a `tool_name:` regex that matches no catalogued tool (ADR-706 item 6).

10. **ADR-704 becomes this lane's name-only case, and Bash-derived write targets feed `files:`.** ADR-704's fields are renamed `tool_name:` and `tool_event:` before they ship in a release; its adapter path and template entries carry over. If PR #1115 merges under its own names, the rename lands before the next ways release. A write target the describer derives from a segment (`sed -i`, `tee`, a redirect; #525) is matched by the file-modify key on the same hook, with channel `bash-write`.

11. **Implementation items.**
    - The hit loop and catalog lookup in one `scan` function, with `ways hook tool` reading the event.
    - The command describer as its own crate, with tests on command strings alone.
    - The matcher on verbs plus description, the semantic branch of today's command scan removed, and lazy embedding for checks.
    - ADR-606's deferred result store and collector, built once for macros and judge verdicts.
    - The lookbehind fix of item 3e.
    - The settings template generated from the catalog, with a test that compares them.
    - The schema entries for the new keys, and the lint rules in item 9.
    - A delivered-versus-emitted count in `ways introspect`, so a lane that emits and never delivers is visible within days.
    - The held-out comparison of item 3b, recorded under `observable` before acceptance.
    - The acceptance observation: one interactive session in which a `commands:` way, an `agents:` way and a `skills:` way fire through `check-tool.sh`, the last both on a typed `/name` and on a Skill tool call, each paired with a `hook_additional_context` row, and a judged Bash way arrives at the next hook under the deferred header.

## Consequences

### Positive

- A new kind of tool trigger is a key and a catalog entry, not a lane.
- `commands:` reaches PowerShell and `files:` reaches NotebookEdit with no edit to a way, and an anchored `commands:` regex matches after a connector.
- Ways can fire on what a subagent launch, a web fetch, a skill or a plan approval carries, at act time.
- On the measured window, the judged Bash lane delivers about seven times the on-topic guidance of today's lane and about a tenth of its off-topic injections per call. Ways with no `commands:` field, such as the delivery parent or testing evaluation, are still reached by meaning.
- The command and the regex fires never wait on the judge.
- The describer, the matcher, the judge gate and the reports are tested and replaced separately. The describer knows nothing of ways, so a later matcher or another consumer can reuse it.
- Building ADR-606's store and collector once serves its deferred macros and this lane.

### Negative

- Meaning-based Bash guidance arrives after the command it was about, often at that call's PostToolUse and sometimes later. A heads-up written for the moment before a command lands after it; authored `commands:` regexes still fire before.
- The lane needs the judge's API key and network. Without them it delivers no meaning-based Bash guidance, though its regex fires are unaffected.
- The judge costs about $0.001 per Bash call, about $8 over the 8,016 calls of the measured 2.7 days.
- About 21% of the judged injections in the study were still off topic, and every grade behind that figure came from a Claude model.
- The judge sends `temperature: 0`. The claude-api reference (migrating to API version 2026-10-21, change 1) returns 400 for `temperature`, `top_p` and `top_k` on API version `2026-10-21`. The judge sends `2023-06-01` (`tools/ways-agent/src/net.rs:275`), where nothing changes; the first bump of `ANTHROPIC_VERSION` drops the parameter, and verdict stability at the default temperature is unmeasured (#1130).
- The lane cannot ship before ADR-606's store and collector exist.
- The parser is heuristic and the tldr data misreads some invocations; a wrong description reaches the judge.
- Each newly wired tool adds a process per call per event. A way on an unwired type lints with a warning and stays silent; a user who wants `reads:` must wire Read, and pays its frequency.
- Seven new frontmatter keys to learn. `tool_name:` remains coupled to Claude Code's names.

### Neutral

- ADR-155 §4 is superseded by item 3; the rest of ADR-155 stands.
- The task lane keeps its own adapter and semantic match. `agents:` sits beside it on the same tool.
- PostToolUse remains the place for postchecks, the queued-message lane (ADR-161) and ADR-606's collector. PreToolUse carries guidance again, as well as guards.
- Events logged on `semantic:bash:*` before the build stay in the log and read as delivered from 2026-09-29 on.
- ADR-707's graded sets can seed the calibration of the matcher and the judge's threshold on this lane.

## Alternatives Considered

- **Keep this record's first draft: move tool-lane matching to PostToolUse.** Rejected. Its reason, that PreToolUse context is never delivered, was this project's emit shape, and #528 closed with PreToolUse delivery verified. Moving to PostToolUse would turn act-time guidance into after-the-fact guidance for no gain.
- **Reject this record and accept ADR-704 as a separate lane.** The cheapest path. Rejected as the end state: a lane per gap means a scan, a request type, an adapter and a phase concept per lane, and the field-matching gaps (PowerShell, NotebookEdit, reads, URLs, agent types, MCP) stay open. ADR-704's mechanism is kept as the name-only case.
- **A single `tool:` key in permission-rule syntax** (`Bash(git *)`, `Edit(**/*.rs)`). It is Claude Code's own syntax and matches the hook `if` field. Not chosen: the operator picked one key per type. It would also name harness tools in every way and use globs where 54 existing ways use regexes.
- **A key per tool** (`bash:`, `edit:`, `notebook_edit:`). Rejected: every tool rename or new family member becomes a way edit, which is the coupling ADR-706 exists to remove.
- **Retire the semantic Bash lane, and repair the `commands:` regexes it misses.** The measurement's first recommendation, and this record's earlier text. Cheap, and it removes about 1,600 off-topic injections a day. Rejected: it also drops the on-topic guidance, 40 of whose 98 measured fires belonged to ways no regex could reach, and the operator read it as a regression in capability.
- **Keep the lane as it is, or raise its threshold.** Rejected: about 3% on topic, and precision is flat across score bands; a threshold of 0.7 keeps 2 of 7 on-topic fires at the same off-topic share.
- **Repair today's surface per ADR-191 (lookbehind, chunking, winner confirmation).** Rejected: the narration the lookbehind brings in is the main source of off-topic matches, and a judge given narration passed more off-topic ways.
- **The tldr-text matcher.** Recorded as the alternative in item 3b and decided on the held-out set before acceptance. It measured lower than verbs plus description on every column, inside overlapping intervals.
- **Judge synchronously at PreToolUse.** Act-time delivery for judged ways. Rejected: a judge request took 1.4 s at the median and 5.9 s at p95, and the prompt lane's 2,000 ms deadline would fail 13–29% of requests open, delivering unjudged candidates.
- **Inject the candidates unjudged.** Rejected: unjudged verbs-plus-description candidates were 66% off topic.
- **Describe commands with `man`, `whatis` or an installed tldr client at run time.** Rejected: their presence and content vary by host, and the operator ruled out depending on a package installed on the machine. The tldr data ships as corpus data instead (ADR-706).
- **Precompute embeddings of every tldr example at corpus build.** About 29 MB at f16, and only a matcher without the description escapes per-call embedding. Kept as the optional edge index for such a matcher.
- **Wire every tool (`.*`) so any way works without wiring.** Rejected: one process per tool call per event, Read included, to serve a handful of ways.
- **Fire on PostToolUseFailure too.** Rejected for the reason in item 8.
- **A separate hook command for the lane on PostToolUse beside `check-post.sh`.** Rejected: a second process per call where one already reads the same payload and prints the envelope.
