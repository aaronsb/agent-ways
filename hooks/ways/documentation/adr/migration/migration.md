---
description: migrating to ADR tooling, adopting ADRs, converting existing decisions, setting up adr.yaml, bootstrapping agent decision records
vocabulary: migrate adopt convert bootstrap setup greenfield legacy rename scan frontmatter yaml scaffold import consolidate
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# ADR Migration

> **Prefer the skill for greenfield scaffolding.** The `project-init` skill is the canonical scaffolder — it installs this tooling *and* the surrounding GitHub config, CODEOWNERS, and project ways in one pass. Reach for it first when setting up a new repo. The manual steps below are the underlying contract: use them when migrating an existing repo, when you only want the ADR/doc tooling, or to understand what the skill automates.

## Identify Your Starting State

| State | Signs | Strategy |
|-------|-------|----------|
| **Greenfield** | No records, no `docs/architecture/` | Scaffold from scratch |
| **Flat directory** | Records in one dir, sequential numbering (0001, 0002...) | Rename to `ADR-NNN-*.md`, then `adr import scan`; or park as legacy |
| **v0 frontmatter** | YAML frontmatter with `status: Accepted` etc., no `kind`/`verb`/`capability`/`basis` | `adr import scan` |
| **Inline metadata** | `Status: Accepted` in the markdown body, no YAML frontmatter | Move the metadata into frontmatter, then `adr import scan` |
| **Other tools** | adr-tools, MADR, Log4brains, or similar | No reader yet — convert through a sheet by hand, or park as legacy |

## Greenfield Setup

No existing ADRs. Scaffold the full structure.

1. **Vendor the tooling.** The install steps (copy-not-symlink, `adr.yaml` setup) live in the **adr** skill — that's the canonical *how*. For the optional doc catalog (prose + ADRs as one typed graph, ADR-302, sharing this `adr.yaml`), use the **docs** skill. To scaffold tooling *and* the surrounding repo health in one pass, prefer `project-init`.

2. Verify:
```bash
docs/scripts/adr domains    # Should show your configured domains
docs/scripts/adr list       # Should show 0 ADRs
```

The rest of this way is the migration-specific *why/when/what* the skill doesn't cover: which starting state you're in, and how to get existing decisions into the tooling without losing history.

## Converting Existing Records

Numbers are permanent identity (ADR-310) — conversion never renumbers or re-homes a record by domain range. A domain's range only allocates numbers for new records.

1. **Vendor the tooling** (greenfield step 1 — use the **adr** skill)

2. **Prepare what the scanner can't read.** `adr import scan` reads files named `ADR-NNN-*.md` that open with YAML frontmatter, and passes over anything else. Rename sequential files, keeping the number:
```bash
git mv docs/adr/0001-use-postgres.md docs/adr/ADR-001-use-postgres.md  # adr-cite-ignore: example number
```
Move inline metadata into frontmatter and delete the inline lines:
```markdown
---
status: Accepted
date: 2026-01-15
deciders:
  - alice
related: []
---

# ADR-001: Use Postgres for Session State   <!-- adr-cite-ignore: example number -->
```

3. **Scan** the existing records into editable import sheets:
```bash
docs/scripts/adr import scan docs/adr/          # or a list of files
```
This writes a sheet per record under `.import/`.

4. **Edit the sheets** in `.import/` — resolve each sheet's open todo items (the v1 fields the scan could not infer, such as `kind`, `verb`, `capability` and `basis`). Apply skips a sheet with open items.

5. **Dry-run the apply** — writes the records into the corpus, lints them, prints each issue, then restores every file and keeps the sheets:
```bash
docs/scripts/adr import apply --dry-run
```

6. **Apply for real** once the sheets are clean:
```bash
docs/scripts/adr import apply
```
`--partial` lands sheets that still carry open todo items, except ones lint can't re-find afterward (a status with no mapping, a Deprecated note, a number or domain mismatch). `--force` overwrites a record with uncommitted changes.

7. **Bring `adr.yaml` to the records' contract.** Apply writes adr/v1 records and leaves `adr.yaml` as it is. When `adr.yaml` declares an older contract, apply prints a note and `adr lint` warns. Run:
```bash
docs/scripts/adr contract --upgrade
```
It adds the contract line, and the `kinds` and `capabilities` blocks when they are missing, and leaves the other lines and comments as they are. The `capabilities` block it writes holds one capability per domain, then `process` and `adr`; with no domains it holds the placeholder `core` instead of seeds. Refine the list as in [Capabilities from domains](#capabilities-from-domains).

A corpus not worth converting can instead be parked as read-only history under `docs/architecture/legacy/`, with `legacy.range` in `adr.yaml` covering its numbers. The tool lists only files named `ADR-NNN-*.md` with frontmatter, so rename and add frontmatter as in step 2 for parked records to appear in `adr list` and resolve in `adr cite`.

For adr-tools, MADR, Log4brains, or other foreign formats — no reader exists yet — either copy each record's substance into a sheet by hand before applying, or skip conversion and park the corpus in `legacy/`.

## Capabilities from domains

`adr contract --upgrade` seeds one capability per v0 domain, using the domain's description as its line. The seeds are a starting point. Keep, split, rename or delete each one.

- Split a seed when decisions change its parts independently.
- A record may carry 2–3 capabilities when it changes several (ADR-308).
- `process` holds conventions that apply across the codebase and belong to no product capability: datetime utilities, record numbering, the docs strategy.
- `adr` holds the records themselves, their contract and the tool.

knowledge-graph-system's 9 domains became 18 capabilities (#616), part of it:

| v0 domain | capabilities |
|---|---|
| infra | `jobs`, `deploy`, `backup`, `storage` |
| db | `graph`, `annealing` |
| ingest | `ingest`, `extraction` |
| query | `query`, `grounding` |
| ui | `cli`, `web`, `fuse` |
| meta | `process` |

agent-ways, a tooling project, went from 6 domains to 13 capabilities: `adr`, `docs`, `method`, `matching`, `disclosure`, `authoring`, `cli`, `attend`, `install`, `config`, `governance`, `loop`, `testing` (its `docs/architecture/adr.yaml`).

## Writing adr.yaml

The config file defines your project's ADR structure. Required fields:

```yaml
# Required
project_name: My Project        # Used in generated index

domains:                         # At least one domain
  core:                          # Domain key (used in CLI: adr new core "Title")
    range: [100, 199]            # Number range (non-overlapping, leave room to grow)
    name: Core                   # Display name
    description: Core patterns   # One-line description
    folder: core                 # Subdirectory under docs/architecture/

# Recommended
statuses:                        # Valid status values
  - Draft
  - Proposed
  - Accepted
  - Superseded
  - Deprecated

defaults:
  deciders: [alice, bob]         # Default deciders for new ADRs
  status: Draft                  # Initial status

legacy:
  range: [1, 99]                 # Range for pre-domain ADRs
  label: "Legacy"

viewer: cat {file}               # Command for `adr view` ({file} is placeholder)
```

**Domain range design:**
- Use 100-wide ranges (100-199, 200-299) — room to grow without renumbering
- Reserve 1-99 for legacy
- Don't overlap ranges — the tool assigns the next available number within a domain's range
- `folder` can be a string or list (for domains spanning multiple directories)

**A template is available** at `hooks/ways/documentation/adr/adr.yaml.template`.

## Validation

After any migration, verify with:

```bash
docs/scripts/adr lint           # Check for missing fields, invalid statuses
docs/scripts/adr list --group   # Verify domain assignment
docs/scripts/adr index -y       # Regenerate the index
```

`adr lint --check` exits non-zero on errors — use in CI to prevent regressions.

## Updating Cross-References

After moving files, search for broken references:

```bash
grep -r "ADR-" docs/ --include="*.md" | grep -v architecture/
# Fix paths in any docs that link to old ADR locations
```
