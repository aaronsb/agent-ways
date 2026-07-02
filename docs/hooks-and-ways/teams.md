# Teams

When you go from one agent to a team of them, each teammate arrives with no prior context, receives injected guidance it didn't ask for, follows structured procedures, communicates through approved channels, and gets observed through telemetry. This is organizational socialization applied to a fleet: every teammate is a new hire, every session, and the onboarding is re-enacted mechanically at spawn time. The problem — consistent governance across multiple autonomous agents — is the same whether you're one person with a side project or an organization managing a fleet.

The ways system handles it either way. Every teammate operates under the same governance, regardless of who spawned it or what it's working on. The ways are the onboarding, delivered at the moment it applies. These policy docs are the management rationale the onboarding doesn't include.

## The Three-Scope Model

Every Claude Code session runs in one of three scopes:

| Scope | What it is | How it's detected |
|-------|-----------|-------------------|
| **agent** | Your main session — the one you're talking to | Default. No marker file exists. |
| **teammate** | A named agent in a coordinated team | Marker file at `{SESSIONS_ROOT}/{session_id}/teammate` |
| **subagent** | A quick Task tool delegate (no team, no name) | Spawned via Task without `team_name` parameter |

Scopes matter because they control which ways fire. A teammate should get coordination norms but shouldn't try to write MEMORY.md (concurrent writes from three teammates corrupt the file). A subagent doing a quick search doesn't need team coordination guidance at all.

### How Scope Detection Works

The detection chain is a two-phase handoff between the lead agent and the spawned session:

**Phase 1 — Lead agent's hooks (PreToolUse:Task):**
`check-task-pre.sh` reads the Task tool's parameters. If `team_name` is present, it writes a stash file with `is_teammate: true` and the team name. It also scans ways with `scope: teammate` or `scope: subagent` for content to inject.

**Phase 2 — Teammate's hooks (SubagentStart):**
`inject-subagent.sh` reads the stash, sees `is_teammate: true`, and creates a persistent marker at `{SESSIONS_ROOT}/{session_id}/teammate` containing the team name. From this point forward, the `ways` binary detects the marker and filters ways by scope accordingly.

### Scope Filtering

Each way declares which scopes it applies to via the `scope:` frontmatter field:

```yaml
---
scope: agent              # Only fires for the main session
scope: teammate           # Only fires for team members
scope: agent, teammate    # Fires for both, but not quick subagents
scope: agent, subagent    # Fires for main session and delegates, not teammates
---
```

When no `scope:` is declared, the default is `agent` — backward compatible with all existing ways.

Scope filtering is handled by the `ways` binary: it checks the way's `scope:` field against the current session's scope. This runs on every trigger evaluation.

## The Teams Way

`collaboration/teams/teams.md` fires on `session-start` with `scope: teammate`. When a teammate's first `UserPromptSubmit` hook runs, `check-state.sh` evaluates this trigger, confirms the scope matches, and injects the coordination norms:

- Check TaskList after completing each task
- Use SendMessage to report progress and blockers
- Mark tasks completed via TaskUpdate
- Prefer Edit over Write (reduces merge conflicts)
- Read before editing (another teammate may have changed the file)
- Don't commit to git unless explicitly told to
- Don't stall silently — message the lead if blocked

These norms exist because teammates, unlike subagents, are long-lived and collaborative. A subagent does one thing and exits. A teammate participates in a shared workflow where coordination failures compound.

## What Gets Gated by Scope

Two important ways are gated to `scope: agent` only:

**meta/memory** (the MEMORY.md checkpoint way) — If three teammates all try to write MEMORY.md simultaneously, the file gets corrupted. Only the main agent session should manage persistent memory.

**meta/subagents** (delegation guidance) — Teammates don't need advice about how to delegate work to subagents. They ARE the delegated work. Showing them delegation guidance wastes context tokens on irrelevant information.

## Team Names in Telemetry

When a team spawns, the team name propagates through the entire way-firing pipeline:

```
Task tool (team_name param) → stash file → marker file → detect_team() → log events
```

Every way that fires for a teammate logs the team name alongside the usual fields (way, domain, trigger, scope). The stats tool can then group activity by team — useful for understanding which teams triggered which ways and how much.

## Working With Teams

The teams feature is currently in beta. Enable it by setting the environment variable:

```json
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  }
}
```

in your `settings.json`. When the flag isn't set, no teammate markers are created, no team-scoped ways fire, and the stats show only agent and subagent scopes. The system degrades cleanly — no errors, no missing data, just the pre-teams world.
