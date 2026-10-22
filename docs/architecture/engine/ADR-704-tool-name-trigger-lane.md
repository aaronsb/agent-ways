---
contract: adr/v1
kind: decision
verb: add
capability: matching
basis:
  - operator: forayconsulting
    level: directed
    said: "Wait, there should be tool use hooks avalable for EnterPlanMode, ExitPlanMode, and Workflow!"
    via: plan review, 2026-10-21
  - operator: forayconsulting
    level: directed
    said: "I would like for any hooks we build to EXTEND the agent-ways approach (so that we can PR this back to aaronsb in the future) rather than build shortcut user-only hooks"
    via: plan review, 2026-10-21
  - evidence: "probe, 2026-10-21: a payload-logging PreToolUse and PostToolUse hook on `Workflow` in a scratch project under `claude -p` logged both events. tool_input carried `script`, and the PostToolUse tool_response carried `status: async_launched`"
  - standard: "https://code.claude.com/docs/en/hooks"
  - precedent: ADR-188
agent:
  name: claude
  model: claude-opus-5-5
status: proposed
date: 2026-10-21
deciders:
  - aaronsb
related:
  - ADR-126
  - ADR-157
  - ADR-188
  - ADR-504
---

# ADR-704: A tool-name trigger lane

## Summary

- **Decided:** a way can fire when Claude Code calls a named tool. The frontmatter field `tool_name:` is a regex matched against the hook payload's `tool_name`, case-insensitively like every trigger regex (ADR-157). The field `tool_event:` says when: a list of literal hook event names, `[PreToolUse]` by default, `[PostToolUse]` after the tool returns, `[PreToolUse, PostToolUse]` on either. A failed call (`PostToolUseFailure`) fires only a way that lists it. The lane is served by `ways hook tool` through the adapter `hooks/ways/check-tool.sh`, which the settings template wires on PreToolUse and PostToolUse for `EnterPlanMode|ExitPlanMode|Workflow`.
- **Trades away:** dynamic coverage. A `tool_name:` regex reaches only the tools the shipped matcher names, and `tool_event:` only the events it wires. A way that names another tool stays silent until the template adds it; lint warns and names the wired set.
- **One-way?** No. The lane is additive. Removing the template entries switches it off, and a way that also carries other triggers keeps them.
- **Inversion:** between a matcher on every tool call (`.*`), which spawns the binary for each call, and no tool lane, where moments such as an approved plan or a launched Workflow have no trigger at all.

## Context

The existing tool lanes key on what a tool touches: `commands:` matches the Bash command and `files:` matches the Edit or Write path. Some moments in a session are tool calls with nothing to match but the name. A plan is approved when `ExitPlanMode` returns. A Workflow starts when the `Workflow` tool is called. A way written for those moments had no trigger. The workaround was a postcheck (ADR-135) that tests `tool_name` and exits 0, plus a wider PostToolUse matcher. That puts a match rule in a script that the frontmatter cannot show.

The motivating case is a planning loop run through plan mode and Workflows: the prompt is the specification, the approved plan is a prediction, and the Workflow is the evaluation. Guidance for each step belongs at the moment the step starts.

What Claude Code delivers, per the hooks reference (fetched 2026-10-21) and a probe:

- PreToolUse and PostToolUse match on any tool name. The reference names `ExitPlanMode` explicitly. For it, `tool_input` carries `plan` and `planFilePath`, and the PostToolUse `tool_response` carries `plan` and `filePath`. PostToolUse fires only when the user approves the plan.
- `EnterPlanMode` is a tool, so hooks fire on it by the general rule. Toggling plan mode with Shift+Tab is not a tool call and fires no hook.
- `Workflow` PreToolUse and PostToolUse both fire. The Workflow runs in the background, so PostToolUse fires at launch, not at completion (probe, 2026-10-21).
- Neither plan-mode tool is available under `claude -p`, so their firing is unverified in `-p`. It rests on the reference.
- PreToolUse `additionalContext` in the `hookSpecificOutput` envelope reaches the model (#528).

## Decision

1. **Fields.** `tool_name:` is a regex on the tool name, and it is fire-bearing, so the way needs `refire:`. The field takes the name of the payload key it matches. `tool_event:` is a one-line list of hook event names, written as Claude Code names them, and defaults to `[PreToolUse]`. Names are compared exactly. Lint accepts the five events Claude Code filters by tool name: `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `PermissionDenied`. Any other name is an error, an empty list is an error, and `tool_event:` without `tool_name:` is a warning.
2. **Matching.** `scan::tool` mirrors `scan::file`. It applies `scope:` and `when:` (including `when: file_exists`, which the way schema now lists), checks the event against the way's `tool_event:`, matches the regex, orders the hits, and shows each way on channel `tool`. The envelope's `hookEventName` is the real event. The default list leaves out `PostToolUseFailure`, because a match there would stamp the way and hold it back from the retry that succeeds (ADR-188 item 2). A way that lists it accepts that cost.
3. **Wiring.** `hooks/ways/check-tool.sh` is a two-line adapter over `ways hook tool` (ADR-504 §11). The settings template adds one PreToolUse entry and one PostToolUse entry with matcher `EnterPlanMode|ExitPlanMode|Workflow`. Each is its own entry, so existing entries keep their matchers. Extending the lane to a new tool is a one-line change to both matchers. Extending it to another event is one more entry.
4. **Refire.** The lane uses the same refire curve as every other lane (ADR-126). A second call inside the window is held back. A call far past it fires again, so a loop that approves a plan each round gets the way each round.
5. **What is wired is declared, and lint checks against it.** The way schema (`frontmatter-schema.yaml`) lists the wired tools under `tool_name.wired` and the wired events under `tool_event.wired`, and a unit test holds both equal to the `check-tool.sh` entries in the shipped `settings.json`. Lint warns when a `tool_name:` regex matches none of the wired tools, and when `tool_event:` lists an event that is not wired, and names the wired set in the warning. The warning applies to user and project ways too: they cannot widen the shipped matcher, so a way that names an unwired tool would otherwise lint clean and never fire.
6. **`tool_event:` belongs to the tool-call lane, not to this field.** Every tool hook names its event, and a way on any tool surface may need to say which one. `tool_event:` is defined for the lane: the literal event names, the default, and the lint rules above. A later field on the same lane, such as a matcher on `tool_input`, reuses it rather than adding its own phase field. A type-keyed tool-call lane, which the rewrite of ADR-188 is drafting, builds on `tool_name:` and `tool_event:` as defined here.

## Consequences

### Positive

- A way can carry guidance for the moment a plan is approved or a Workflow launches, and its frontmatter shows the trigger.
- The lane reuses the scan, show and envelope code of the file lane, so budget, scope, `when:` and refire behave the same.

### Negative

- Two matchers in the template must name the tools a way may target. A way that names an unwired tool never fires. Lint warns, but a user or project author can only ask for the template to change.
- The schema's `wired` lists repeat what `settings.json` says. The unit test catches a template edit that skips the schema.

### Neutral

- The field reads only the tool name. Matching on `tool_input` (for example the plan text) is left for a later decision.
- The common hook input carries `permission_mode` ("plan" in plan mode). A gate on it would let a way fire only while planning. It is not part of this decision.

## Alternatives Considered

- **Matcher `.*` on PreToolUse and PostToolUse.** Rejected: it spawns the binary on every tool call to serve a handful of ways.
- **A postcheck per way.** Works today without engine changes, but hides the trigger in a script and runs a process per way.
- **Field names `tools:` and `tool_phase: pre|post|both`.** The first draft of this decision used them. Rejected before release. `tools:` already means the tools an agent may use in agent definitions (`agents/*.md`), so a reader moving between the two would confuse them. `tool_name:` is Claude Code's own payload key, so the field says what it matches. `tool_phase:` mapped three words onto two events and had no word for the other three events Claude Code filters by tool name; a list of literal event names covers all five with no translation table. Once user and project ways carry a field, renaming it is a contract change, so the names were settled before the lane shipped.
- **Field names `tool_names:` or `on_tools:`.** They avoid the agent-definition clash, but neither names the payload key it matches.
- **Fire on `PostToolUseFailure` by default.** Rejected for the reason in ADR-188 item 2. A way may still list it.

## Follow-ups

- A `when: permission_mode:` gate.
- Matching `tool_input` fields, with `tool_event:` as defined here.
- A type-keyed tool-call lane in the rewrite of ADR-188, built on `tool_name:` and `tool_event:`.
