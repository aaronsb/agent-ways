---
description: Managing agent-ways as a project — upstream tracking against Claude Code releases, ADR reconciliation, release discipline, maintaining relevance
vocabulary: upstream changelog release version claude-code update adr status reconcile drift stale dormant shipped implemented current behind project pulse health review audit recently changed since last relevance feature gap opportunity
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Project Health

## This Project's Relationship to Claude Code

agent-ways is an opinionated framework built *on top of* Claude Code. It adds ways, skills, hooks, ADR tooling, governance, and structured thinking — none of which ship with Claude Code itself. But every capability we build depends on Claude Code's primitives (hooks, settings, skills, MCP, permissions).

When Claude Code changes, we may need to adapt, adopt, or deliberately ignore.

## Project Pulse Tool

Run `scripts/project-pulse` for systematic awareness:

| Mode | What it does |
|------|-------------|
| `scripts/project-pulse` | Compare upstream Claude Code releases against our commits |
| `scripts/project-pulse --inward` | Compare our ADRs against our shipped code |
| `scripts/project-pulse --since DATE` | Widen the window |
| `scripts/project-pulse --full` | Full history (rare) |

The default window is feathered: anchored at our last release tag, expanded to include all commits since, with upstream releases mapped to the same period plus context bleed.

## When to Run

- Starting a new feature — check if upstream shipped something relevant
- After a burst of commits — check if ADR statuses still match reality
- Periodically, when you wonder "are we current?"
- When the user asks about upstream changes or project status

## ADR Discipline

- Reference ADR numbers in branch names (e.g., `feature/ADR-106-project-pulse`)
- Reference ADR numbers in commit messages when implementing a decision
- Update ADR status when implementation lands, not when the ADR is written
- Draft = intent not yet committed to. Accepted = we're building or built it.

## Release Mechanics

Two steps, because `main` is branch-protected (ADR-150):

```bash
make cut-release COMPONENT=<c> LEVEL=<patch|minor|major>   # opens a version-bump PR
# after that PR merges:
make publish-release COMPONENT=<c> PUSH=1                  # tags <c>-vX.Y.Z on main
```

Components: `ways`, `ways-audit`, `attend`, `attend-chat`. `way-embed` versions through its own Makefile.

Pushing the tag is the one outward step. `build-<c>.yml` then builds every platform and creates the GitHub Release with per-platform binaries and `checksums.txt`.

Tags are GPG-signed, so `publish-release` needs a passphrase from the operator's own terminal. An agent cannot complete it — do everything up to that point and hand over the command.

Bump level follows the commit types, against the *binary*. A corpus-only change with a `fix:` commit is a patch even when it changes what every session sees; the CLI interface is what the version tracks.

## What to Adopt vs Ignore

Filter upstream changes through what this project cares about:
- **Hooks, settings, config** — core to our way system
- **Skills, slash commands** — we author these
- **Context window, compaction** — our epoch tracking depends on this
- **Plugins, marketplace** — we build on this
- **Subagents, teams** — we have agent definitions
- **Permissions model** — we configure these
- **MCP integration** — we use these

Things we typically ignore: UI polish, voice mode, IDE integrations, API proxy fixes.

This is not a checklist. Use judgment. A "tiny" upstream change (200K to 1M context) can reshape entire subsystems.
