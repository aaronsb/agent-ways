---
name: adr
description: Manage Agent Decision Records using the project's ADR CLI tool. Use when the user wants to create, list, view, lint, or index ADRs, or when working with docs/architecture/ files. Triggers on "create an ADR", "new ADR", "list ADRs", "lint ADRs", "what ADRs exist", "ADR domains".
allowed-tools: Bash, Read, Grep, Glob
---

# ADR Management

ADR now means Agent Decision Record; the `ADR-N` citation form is unchanged. Operate ADRs through the `docs/scripts/adr` CLI tool. Never create ADR files manually. If the tool isn't present in the project yet, vendor it first — see [Vendoring the tool into a project](#vendoring-the-tool-into-a-project).

A project declares its record contract in `docs/architecture/adr.yaml`. With `contract: adr/v1` it uses declared kinds, verbs, capabilities and lifecycle commands (below). Without it, the project is on the legacy `adr/v0` contract: Draft/Proposed/Accepted/Superseded/Deprecated status, and Context/Decision/Consequences/Alternatives Considered sections. `adr contract` shows the contract `adr.yaml` declares and the one the tool writes; `adr contract --upgrade` brings `adr.yaml` to the tool's contract. Both contracts share the same tool and the same `ADR-N` number space.

## Commands

```bash
# Discover
docs/scripts/adr domains                  # Show domain number series and ranges
docs/scripts/adr list --group             # List active ADRs grouped by domain
docs/scripts/adr list --archived          # Archived only; --all for both
docs/scripts/adr list --domain <domain>   # Filter to one domain
docs/scripts/adr list --status Accepted   # Filter by status (v0 title case or v1 lower case; compared case-insensitively)
docs/scripts/adr view <number>            # View an ADR (accepts 14, 014, ADR-014)

# Create
docs/scripts/adr new <domain> "Title" [--kind K] [--verb V] [--capability C] [--agent A --model M]
                                           # --kind: decision (default), spec, evidence (adr/v1 only)
                                           # --verb: add, cut, change, retire, constrain (decisions only)
                                           # --capability: from the adr.yaml vocabulary; --agent/--model: who is writing it

# Maintain
docs/scripts/adr lint                     # Check all ADRs (archive included) for issues
docs/scripts/adr lint --check             # Exit 1 on errors (CI mode)
docs/scripts/adr cite [--check]           # Check ADR-N citations in code against the records
docs/scripts/adr index -y                 # Regenerate INDEX.md from the active set

# Lifecycle (adr/v1) — status changes only through these, never by setting the field
docs/scripts/adr accept <n> [--dry-run]                 # Accept a proposed record
docs/scripts/adr reject <n> --reason "..." [--dry-run]  # Considered and declined
docs/scripts/adr abandon <n> --reason "..." [--dry-run] # Dropped before a decision

# Archive (ADR-303) — not deletion: the file stays tracked, linted, linkable
docs/scripts/adr archive <n> --reason "why" [--superseded-by ADR-N[,ADR-M#sec]] [--status S] [--dry-run]

# Domains (ADR-310) — a record's number is permanent identity; a domain's range only allocates new numbers
docs/scripts/adr domain add <name> --range A-B --folder F [--label L] [--description D]
docs/scripts/adr domain rename <old> <new> [--folder F] [--dry-run]   # moves the folder, rewrites paths
docs/scripts/adr domain move <n> <domain> [--dry-run]                 # adr/v1: moves the file, rewrites paths
docs/scripts/adr domain move --plan moves.yaml [--dry-run]            # [{record, domain}, ...] at once

# Rename a record's title or filename slug (distinct from `domain rename`)
docs/scripts/adr rename <n> ["New Title"] [--slug SLUG]

# Query by frontmatter (adr/v1); a list field matches when it lists the value
docs/scripts/adr list --kind evidence --capability attend --verb change   # also --field KEY[=VALUE]
docs/scripts/adr list --group-by capability               # a listed record appears in each group
docs/scripts/adr list --json                              # number, title, path, status, frontmatter

# Edit records (adr/v1): only the touched field's lines change; each lints the record after
docs/scripts/adr consider <n> --said "..." --via "..." [--covers PROBE...] [--canary caught|missed]
docs/scripts/adr set <n> key=value key+=item key-=item [--force] [--dry-run]
                                           # refuses status (use the lifecycle commands above) without --force
docs/scripts/adr supersede <old> --by <new> [--amends SECTION] [--dry-run]   # writes both sides
docs/scripts/adr enact <n> <commit>                      # accepted cut or retire only

# Import (ADR-306) — convert v0 or foreign records into adr/v1 through editable import sheets
docs/scripts/adr import scan <paths...> [--force]         # writes a sheet per record under .import/
docs/scripts/adr import apply [sheets...] [--partial] [--force] [--dry-run]  # writes finished sheets as adr/v1 records
# A source's text between its frontmatter and its title moves into the body, under the title

# Config
docs/scripts/adr config                   # Show current adr.yaml configuration
docs/scripts/adr contract                 # The contract adr.yaml declares, and the one the tool writes
docs/scripts/adr contract --upgrade       # Bring adr.yaml to the tool's contract; edits lines, keeps comments
```

Under adr/v1, a record's folder decides its area (ADR-310), and its number is
its permanent identity: a move keeps the number, and `ADR-N` citations stay
valid. An area's range only allocates numbers for new records. Under adr/v0,
domain and number range are the same thing, unchanged from before.

## Workflow

1. **Check domains first**: `docs/scripts/adr domains` to see available domains and number ranges
2. **Create**: `docs/scripts/adr new <domain> "Decision Title"` (adr/v1: add `--kind`, `--verb`, `--capability`, `--agent`/`--model`) — assigns the next number, seeds frontmatter for the project's contract
3. **Fill in the body** matching the record's kind — a v1 decision opens with `## Summary` and its `basis`; a v0 record uses Context, Decision, Consequences, Alternatives Considered
4. **Lint**: `docs/scripts/adr lint` before committing
5. **For a v1 decision with an operator basis, record their answer**: `docs/scripts/adr consider <n> --said "..." --via "..."` before `docs/scripts/adr accept <n>`
6. **Index**: `docs/scripts/adr index -y` after adding or changing ADRs

## Configuration

Each project defines its structure in `docs/architecture/adr.yaml`, always
discovered with `docs/scripts/adr domains` / `docs/scripts/adr config` rather
than assumed:
- **domains**: name, number range, description, folder for each domain (adr/v1: the folder is the record's area; the range only allocates new numbers, ADR-310)
- **statuses**: valid v0 status values (Draft, Proposed, Accepted, Superseded, Deprecated); adr/v1 has a fixed status set (proposed, accepted, rejected, abandoned, superseded, archived)
- **defaults**: default deciders and initial status for new ADRs
- **legacy**: number range for pre-domain ADRs
- **contract** (adr/v1 only): `adr/v1`, opting the project into the fields below; absent means adr/v0. `adr contract --upgrade` writes it, with `kinds` and a placeholder capability when they are missing
- **kinds** (adr/v1 only): `decision`, `spec`, `evidence`, each declaring its required fields, sections, and edges
- **capabilities** (adr/v1 only): the closed vocabulary a record's `capability` field draws from
- **basis_sources** (adr/v1 only): what a decision may ground itself in — operator, evidence, standard, upstream, precedent
- **repository** (optional): this repository as URLs name it (host/owner/repo), so `adr domain` rewrites links into it when a record moves; the origin remote otherwise

## ADR Format

The record's shape follows the project's contract, so ask `adr config` rather
than assume one. `adr lint` is the authority on what a given record needs; it
reads the contract and reports what is missing rather than a fixed checklist.

**adr/v1** (`docs/scripts/adr new` writes this shape):
```markdown
---
contract: adr/v1
kind: decision
verb: add
capability: adr
basis:
  - operator: <name>
    level: guided
    said: "<their words, quoted>"
    via: <where they said it>
agent:
  name: <agent>
  model: <model>
status: proposed
date: <YYYY-MM-DD>
---

# ADR-NNN: Decision Title

## Summary
- **Decided:** ...
- **Trades away:** ...
- **One-way?** ...
- **Probes:** ...
- **Inversion:** ...

## Context
## Decision
## Consequences
## Alternatives Considered
```
A `spec` record carries `capability` and no `verb`, and stays mutable in place. An `evidence` record carries no `verb`, is corrected by appending once accepted, and is what a decision's `basis` cites for findings, surveys, audits or explorations (ADR-309).

**adr/v0** (the legacy contract, unchanged):
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
validate: `docs/scripts/adr domains && docs/scripts/adr lint`. The template
declares `contract: adr/v1` with the decision, spec and evidence kinds and a
placeholder capability: replace it with the project's capabilities. To stay on
adr/v0, delete the `contract` line and the v1 blocks under it.

For a full repo scaffold (ADRs + GitHub config + CODEOWNERS + project ways), run
`/project-init` instead — it vendors this tool as one step of a larger setup. The
**docs** skill is the catalog half and shares this `adr.yaml`.

A project with existing decision records in another shape (a flat directory,
inline metadata, a different tool) converts them with `docs/scripts/adr import
scan <paths>` then `docs/scripts/adr import apply` (ADR-306) rather than by
hand-editing frontmatter — the import sheets are editable and re-lintable
before anything is written as an adr/v1 record. `import apply` does not edit
`adr.yaml`; when the records it writes declare a newer contract than `adr.yaml`,
it says so, and `docs/scripts/adr contract --upgrade` brings `adr.yaml` to it.

## Updating a vendored copy

The tool carries a `TOOL_VERSION` stamp (`docs/scripts/adr --version`), and the
`adr` way's macro compares it against the installed template — so a disclosure
saying the copy is *out of date* means ways ships a newer tool (ADR-177).

- **Unmodified copy** (only the `TOOL_VERSION` line differs, if anything):
  re-run the `cp` from the vendoring steps above. `adr.yaml` is untouched —
  config and tool update independently. When the new tool writes a newer
  contract than `adr.yaml` declares, `adr contract` says so, and
  `adr contract --upgrade` brings `adr.yaml` to it.
- **Customized copy**: diff first, then carry the local changes forward onto
  the new version — never overwrite on sight:

```bash
diff docs/scripts/adr ~/.claude/hooks/ways/documentation/adr/adr-tool
```

From 1.2.0, lint checks are registered rule functions in one "Lint rules"
section, and `adr lint` runs them. They no longer run inline in
`parse_adr`. A local check added to `parse_adr` in an older copy belongs there
as a `@file_rule`, or as a `@corpus_rule` when it resolves against other
ADRs.

If the macro instead says the *installed template* is behind the project's
copy, the agent-ways install is stale — update it (`/ways-update`), don't
downgrade the project.

## Key Rules

- **Always use the CLI** — never create `ADR-*.md` files by hand
- **Run `domains` first** when working in an unfamiliar project — domain names and ranges vary
- **Status changes through the tool** — adr/v0: edit the YAML `status:` field; adr/v1: `adr accept`/`reject`/`abandon`/`supersede`/`archive`, which `adr set` refuses without `--force`
- **Correct by appending** — an accepted record is corrected by appending, a change in what the project does is a new decision that names what it replaces, and the operator is quoted in their own words. The tool checks shape and references, not these; review catches them, and git keeps every earlier version (ADR-311)
- **Regenerate index** after any ADR changes with `docs/scripts/adr index -y`

## Not for

- Authoring catalog documentation pages — that's the **docs** skill (the decisions half vs. the prose half of the same graph).
- Hand-editing `ADR-*.md` files directly — always go through the CLI.
- Designing the decision itself — capture a made decision; deliberation is the human's and `system-architect`'s job.
