---
status: Accepted
date: 2026-03-20
deciders:
  - aaronsb
  - claude
related:
  - ADR-103
  - ADR-104
---

# ADR-106: Project Pulse — Epoch-Mapped Project Awareness

## Context

Claude Code releases frequently — often daily. Each release may add features, change behaviors, or introduce capabilities that affect how this project (claude-code-config) should be built. Without a systematic way to compare upstream changes against our own work, we either miss opportunities or discover them accidentally.

Separately, this project's own ADRs can drift from reality. An ADR may be Accepted but never implemented, or Draft while the code has already shipped. We discovered this during a manual audit where 8 of 12 ADRs had incorrect statuses. The same epoch-mapping logic that compares us against upstream can compare our stated intentions (ADRs) against our shipped code (commits, branches, PRs).

The naive approach — projecting both projects onto a shared calendar and comparing dates — breaks down because commits and releases are epoch counters, not time events. A day with 12 commits isn't "more" than a day with 1 commit. The meaningful relationship is ordinal-causal: at the time of our epoch N, which Claude Code epoch was current, and how many upstream epochs passed between our epochs N and N+1?

Previous examples of this gap: the jump from 200K to 1M context window required rethinking our epoch tracking and compaction strategies (ADR-103, ADR-104). That change was discovered ad-hoc rather than surfaced systematically.

This project also has a velocity mismatch: Claude Code ships tagged releases nearly daily; we commit frequently but tag releases irregularly. The comparison tool must handle both granularities.

## Decision

Build a project awareness tool (`scripts/project-pulse`) and skill (`skills/project-pulse/SKILL.md`) with two modes:

### Upstream mode (default)

Compare Claude Code releases against our project's commits to surface what's new upstream and what might matter for us.

### Inward mode (`--inward`)

Compare our ADRs against our own commits, branches, and PRs. Searches for ADR numbers (e.g., "ADR-106") in branch names, commit messages, and PR descriptions. Flags mismatches:
- ADRs still Draft/Proposed but with substantial implementing code
- ADRs marked Accepted but with no referencing commits
- ADRs with no implementation activity at all (dormant)

### Data model

Treat both projects as epoch streams. Each epoch is a commit or release with an ordinal position and a timestamp (metadata, not primary key). The core data structure maps our epochs to the upstream epoch that was current at that moment, plus a delta showing how many upstream epochs passed since our previous epoch.

For inward mode, the mapping is between ADR creation/status-change epochs and the commits that reference each ADR.

### Feathered windowing

The default window starts from our most recent release tag (the anchor), includes all commits since that anchor, and pulls the corresponding Claude Code releases spanning the same period — plus 1-2 extra releases on each side for context bleed. This "feathered" approach avoids hard date cutoffs.

Overrides:
- `--since DATE` — everything from a date onward
- `--range DATE DATE` — specific window
- `--full` — entire history

### Two-piece architecture

1. **Script** (`scripts/project-pulse`) — data gathering. In upstream mode: fetches Claude Code releases via `gh api repos/anthropics/claude-code/releases`, our commits/tags via `git log`, builds the epoch mapping. In inward mode: scans ADR statuses, searches git log and branch names for ADR references, builds the reconciliation. Outputs structured markdown in both modes.

2. **Skill** (`skills/project-pulse/SKILL.md`) — interpretation. Reads the script output and the project's README/ADR index to understand what we care about. In upstream mode: filters changes through the project's charter (hooks, settings, skills, context window, plugins, subagents, permissions, MCP), produces 2-5 suggestions in plain prose. In inward mode: highlights status mismatches and dormant ADRs, suggests corrections.

### Tone and intent

The skill output is a discovery conversation starter, not a compliance dashboard. No coverage scores, no red/yellow/green, no "you're N epochs behind." It reads like a colleague saying "did you see they added X? That might matter for us" or "ADR-100 has been Draft for a while — is that still the plan?" The user decides what to act on.

### Watermark

After each run, the tool writes a lightweight marker (date + our HEAD + upstream latest release) so the next default invocation knows where to start.

### ADR provenance

When upstream changes inspire new work, the resulting ADRs can cite the specific Claude Code release version as a reference (e.g., "Inspired by Claude Code v2.1.80 `effort` frontmatter support").

### Meta way integration

A new `meta/project-health` way triggers on keywords related to upstream tracking, project status, and ADR reconciliation. It suggests running the project-pulse tool when relevant, and provides guidance on managing claude-code-config as a project — not just authoring its components.

## Consequences

### Positive

- Systematic awareness of upstream changes without manual changelog reading
- ADR status reconciliation catches drift between intent and implementation
- Epoch-based comparison handles velocity mismatches between the two projects
- Conversational output avoids FOMO treadmill — user stays in control
- ADR provenance links our decisions to the upstream features that inspired them
- Feathered windowing ensures context bleed at boundaries isn't lost
- Single tool serves both outward (upstream) and inward (self) awareness

### Negative

- Depends on `gh` CLI and GitHub API access to anthropics/claude-code
- Skill interpretation quality depends on the project's README/ADR index being reasonably current
- Watermark file is another piece of state to maintain
- Inward mode depends on consistent ADR number references in commits/branches (a convention we need to maintain)

### Neutral

- Encourages more consistent release tagging in our project (better anchor points)
- Encourages referencing ADR numbers in branch names and commits (better traceability)
- The epoch mapping data structure could be reused by other tools (e.g., checks, ways staleness detection)
- The meta way creates a self-referential loop: claude-code-config has a way about managing claude-code-config

## Alternatives Considered

- **Calendar-based comparison (interleaved timeline)**: Simpler to implement, but misrepresents the relationship between projects. Commits aren't time events — they're ordinal epochs. Calendar projection creates false "gaps" when one project is quiet.
- **Disposition ledger (ADOPTED/WATCHING/IRRELEVANT per feature)**: Adds a persistent state file that must be maintained. The epoch mapping plus feathered window achieves retroactive lookback without ongoing bookkeeping.
- **Website scraping**: The changelog at code.claude.com/docs/en/changelog has the same content as GitHub releases but requires HTML parsing and is more fragile. The `gh api` approach is structured, authenticated, and paginated.
- **Full-history comparison every time**: Context-expensive and noisy. The feathered window gives relevant scope by default while `--full` remains available for archaeology.
- **Separate tools for upstream vs inward**: The data model (epoch streams, feathered windows) is the same in both directions. Splitting into two tools would duplicate the windowing logic and force the user to remember two commands. Modes on one tool are simpler.
