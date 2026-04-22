---
description: Architecture Decision Records — creating, managing, and referencing ADRs for technical choices
vocabulary: adr architecture decision record design pattern technical choice trade-off rationale alternative
pattern: (^| )adr( |$)|architect|decision|design.?pattern|technical.?choice|trade.?off
files: docs/architecture/.*\.md$
macro: prepend
scope: agent, subagent
requires: ["Bash(chmod:*)", "Bash(cp:*)", "Bash(mkdir:*)", "Bash(touch:*)"]
refire: 0.15
---
<!-- epistemic: convention -->
# ADR Way

## When to Write an ADR
- Architectural choices (databases, frameworks, patterns)
- Technical approaches with trade-offs
- Process or methodology changes
- Security or performance decisions
- Anything you'll need to remember "why we did it this way"

## ADR Tooling

**Always use `docs/scripts/adr` to manage ADRs.** It handles numbering, domain routing, and templates.

| Command | Purpose |
|---------|---------|
| `docs/scripts/adr new <domain> <title>` | Create new ADR |
| `docs/scripts/adr list [--group]` | List all ADRs |
| `docs/scripts/adr view <number>` | View an ADR |
| `docs/scripts/adr lint [--check]` | Validate ADRs |
| `docs/scripts/adr index -y` | Regenerate index |
| `docs/scripts/adr domains` | Show domain number series |

## Directory Structure

```
docs/
├── scripts/adr              # CLI tool (symlink to hooks/ways/softwaredev/architecture/adr/adr-tool)
└── architecture/
    ├── adr.yaml              # Domain config: number ranges, statuses, defaults
    ├── INDEX.md              # Auto-generated index (adr index -y)
    ├── system/               # ADR 100-199: Ways, matching, hooks, lifecycle
    ├── governance/           # ADR 200-299: Provenance, controls, compliance
    ├── documentation/        # ADR 300-399: Doc structure, tooling
    └── legacy/               # ADR 1-99: Pre-domain numbering
```

Projects define their own domains and ranges in `adr.yaml`. Run `docs/scripts/adr domains` to see the active configuration.

## ADR Format

ADRs use **YAML frontmatter** for metadata and a standard body structure:

```markdown
---
status: Draft
date: 2026-02-17
deciders:
  - aaronsb
  - claude
related: []
---

# ADR-NNN: Decision Title

## Context
Why this decision is needed. What forces are at play.

## Decision
What we're doing and how.

## Consequences

### Positive
- Benefits and wins

### Negative
- Costs and risks

### Neutral
- Other implications

## Alternatives Considered
- Other options evaluated
- Why they were rejected
```

Statuses: `Draft` | `Proposed` | `Accepted` | `Superseded` | `Deprecated`

## Fixing ADR Issues

**Run `docs/scripts/adr lint` before editing ADR files.** The linter identifies exactly what's wrong (missing frontmatter, invalid status, missing fields). Use its output to guide targeted fixes rather than opening files and guessing.

The linter detects:
- Missing YAML frontmatter (including inline metadata that needs conversion)
- Missing or invalid status, date, deciders
- Unclosed frontmatter delimiters
- Invalid YAML syntax

## ADR Workflow
1. **Debate**: Discuss problem and potential solutions
2. **Draft**: `docs/scripts/adr new <domain> <title>` — creates numbered ADR in correct subdirectory
3. **PR**: Create pull request for ADR review
4. **Review**: User reviews, comments, iterates
5. **Merge**: ADR becomes accepted, update index with `docs/scripts/adr index -y`
6. **Implement**: Create branch, reference ADR in work

## See Also

- architecture/adr-context(softwaredev) — read existing ADRs before building
- architecture/adr/migration(softwaredev) — adopting ADR tooling in existing projects
- delivery/implement(softwaredev) — ADRs feed implementation planning
