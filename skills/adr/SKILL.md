---
name: adr
description: Manage Architecture Decision Records using the project's ADR CLI tool. Use when the user wants to create, list, view, lint, or index ADRs, or when working with docs/architecture/ files. Triggers on "create an ADR", "new ADR", "list ADRs", "lint ADRs", "what ADRs exist", "ADR domains".
allowed-tools: Bash, Read, Grep, Glob
---

# ADR Management

Operate ADRs through the `docs/scripts/adr` CLI tool. Never create ADR files manually. If the tool isn't present in the project yet, vendor it first — see [Vendoring the tool into a project](#vendoring-the-tool-into-a-project).

## Commands

```bash
# Discover
docs/scripts/adr domains                  # Show domain number series and ranges
docs/scripts/adr list --group             # List all ADRs grouped by domain
docs/scripts/adr list --domain system     # Filter to one domain
docs/scripts/adr list --status Accepted   # Filter by status
docs/scripts/adr view <number>            # View an ADR (accepts 14, 014, ADR-014)

# Create
docs/scripts/adr new <domain> "Title"     # Create from template in correct subdirectory

# Maintain
docs/scripts/adr lint                     # Check all ADRs for issues
docs/scripts/adr lint --check             # Exit 1 on errors (CI mode)
docs/scripts/adr index -y                 # Regenerate INDEX.md

# Config
docs/scripts/adr config                   # Show current adr.yaml configuration
```

## Workflow

1. **Check domains first**: `docs/scripts/adr domains` to see available domains and number ranges
2. **Create**: `docs/scripts/adr new <domain> "Decision Title"` — assigns next number, uses YAML frontmatter template
3. **Edit**: Fill in Context, Decision, Consequences, Alternatives sections
4. **Lint**: `docs/scripts/adr lint` before committing
5. **Index**: `docs/scripts/adr index -y` after adding or changing ADRs

## Configuration

Each project defines its domain structure in `docs/architecture/adr.yaml`:
- **domains**: Name, number range, description, folder for each domain
- **statuses**: Valid status values (Draft, Proposed, Accepted, Superseded, Deprecated)
- **defaults**: Default deciders and initial status for new ADRs
- **legacy**: Number range for pre-domain ADRs

Always run `docs/scripts/adr domains` to discover the project's actual configuration rather than assuming domains.

## ADR Format

The tool generates ADRs with YAML frontmatter:

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
## Decision
## Consequences
### Positive
### Negative
### Neutral
## Alternatives Considered
```

## Vendoring the tool into a project

`docs/scripts/adr` is **not** part of a project by default — it's vendored from
the agent-ways install. When it's missing (the `adr` way's macro will observe
this and remind you), install a standalone **copy** — never a symlink, since a
symlink into `~/.claude` breaks for collaborators and CI who don't have that
directory:

```bash
mkdir -p docs/scripts docs/architecture
cp ~/.claude/hooks/ways/documentation/adr/adr-tool docs/scripts/adr
cp ~/.claude/hooks/ways/documentation/adr/adr.yaml.template docs/architecture/adr.yaml
chmod +x docs/scripts/adr
```

Then edit `docs/architecture/adr.yaml` for the project's domains and ranges, and
validate: `docs/scripts/adr domains && docs/scripts/adr lint`.

For a full repo scaffold (ADRs + GitHub config + CODEOWNERS + project ways), run
`/project-init` instead — it vendors this tool as one step of a larger setup. The
**docs** skill is the catalog half and shares this `adr.yaml`.

## Updating a vendored copy

The tool carries a `TOOL_VERSION` stamp (`docs/scripts/adr --version`), and the
`adr` way's macro compares it against the installed template — so a disclosure
saying the copy is *out of date* means ways ships a newer tool (ADR-177).

- **Unmodified copy** (only the `TOOL_VERSION` line differs, if anything):
  re-run the `cp` from the vendoring steps above. `adr.yaml` is untouched —
  config and tool update independently.
- **Customized copy**: diff first, then carry the local changes forward onto
  the new version — never overwrite on sight:

```bash
diff docs/scripts/adr ~/.claude/hooks/ways/documentation/adr/adr-tool
```

If the macro instead says the *installed template* is behind the project's
copy, the agent-ways install is stale — update it (`/ways-update`), don't
downgrade the project.

## Key Rules

- **Always use the CLI** — never create `ADR-*.md` files by hand
- **Run `domains` first** when working in an unfamiliar project — domain names and ranges vary
- **Status lives in frontmatter** — edit the YAML `status:` field, not inline text
- **Regenerate index** after any ADR changes with `docs/scripts/adr index -y`

## Not for

- Authoring catalog documentation pages — that's the **docs** skill (the decisions half vs. the prose half of the same graph).
- Hand-editing `ADR-*.md` files directly — always go through the CLI.
- Designing the decision itself — capture a made decision; deliberation is the human's and `system-architect`'s job.
