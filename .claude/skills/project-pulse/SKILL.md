---
name: project-pulse
description: >
  Review what changed in Claude Code upstream and compare against this
  project's commits and ADRs. Surfaces opportunities, gaps, and ADR
  status drift. Discovery conversation, not compliance dashboard.
allowed-tools: Bash, Read
---

# Project Pulse

Compare this project against Claude Code upstream releases, or reconcile ADR statuses against shipped code.

## Usage

```
/project-pulse                    # What's new upstream?
/project-pulse --inward           # Are our ADRs current?
/project-pulse --since 2026-03-01 # Wider window
/project-pulse --full             # Full history
```

## How to Run

This is a **project-scoped dev-harness skill** — it lives in the agent-ways repo's
own `.claude/skills/` (not the projected framework surface), so it's available when
you're developing agent-ways, never shipped into an operator's install. The helper
lives in the checkout at `scripts/project-pulse`, so resolve it against the project:

```bash
bash "${CLAUDE_PROJECT_DIR:-$PWD}/scripts/project-pulse" [flags]
```

The script outputs structured markdown. Your job is to interpret it.

## Upstream Mode (default)

The script outputs:
1. Claude Code releases in the window (with full changelogs)
2. Our commits in the same window
3. An epoch mapping table (our commits mapped to which upstream release was current)
4. High-delta alerts (periods where many upstream releases passed between our commits)

### Your interpretation

Read the upstream changelogs and filter through what this project cares about:

**Relevant to us** (suggest these):
- Hooks: new hook types, hook behavior changes, hook lifecycle
- Settings/config: new settings fields, managed settings, permissions model
- Skills/slash commands: new frontmatter fields, skill discovery changes, effort levels
- Context window: size changes, compaction behavior, token tracking
- Plugins: marketplace changes, plugin sources, installation flow
- Subagents/teams: isolation modes, worktree behavior, agent coordination
- MCP: new capabilities, server management, tool patterns

**Usually irrelevant** (skip these):
- UI polish, terminal rendering, keybindings
- Voice mode, dictation
- IDE-specific integrations (VSCode, Cursor)
- API proxy fixes, Bedrock/Vertex specifics
- Remote Control, bridge sessions

### Output format

Pick the 2-5 most interesting upstream changes and explain in plain prose why they matter for this project. Reference the specific version. Example:

> Claude Code v2.1.80 added `effort` frontmatter for skills. We author skills — this would let our think strategies control reasoning effort. Worth an ADR?

> v2.1.76 added `PostCompact` hook. Our compaction-checkpoint way currently uses `UserPromptSubmit` at 95% threshold. A PostCompact hook could be a cleaner safety net.

If nothing interesting happened in the window, say so. That's a valid outcome.

Do NOT:
- List every change
- Score coverage
- Show red/yellow/green
- Say "you're N releases behind"

### High-delta periods

When the epoch mapping shows high deltas (3+ upstream releases between our commits), note this as context — "the ground shifted here, worth a closer look at what shipped in that window."

## Inward Mode (`--inward`)

The script outputs a table of ADRs with their status, referencing commit count, branch references, and an assessment.

### Your interpretation

Focus on mismatches:
- **"Accepted but no referencing commits"** — was this ADR adopted based on existing code, or is something missing?
- **"Draft with N commits — promote?"** — the work exists, the ADR may need a status update
- **"Dormant"** — no commits, no branches. Is this still planned?

Present as conversational observations, not a status report:

> ADR-100 (Ways Scaffolding Wizard) is still Draft but has 4 referencing commits. The testing side exists but the scaffolding wizard itself doesn't. Is this still the plan, or should we scope it down and accept what we have?

## Suggesting ADRs

When upstream changes suggest new work, propose ADR topics with the upstream reference:

> Upstream v2.1.80 added `--channels` for MCP server push messages. If we want ways or skills to receive asynchronous notifications, that's a new architectural capability. ADR candidate: "Channel-based way triggers for async MCP events" (inspired by Claude Code v2.1.80).

## Tone

You're a colleague who follows the releases. Casual, suggestive, not prescriptive. The human decides what to act on.

## Not for

- Coverage scoring or "you're N releases behind" reporting — it's a discovery conversation, not a compliance dashboard.
- Applying the changes — it surfaces what's worth discussing; you decide and act (an ADR, an issue, a `/ship`).
