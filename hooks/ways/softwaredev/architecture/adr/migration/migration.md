---
description: migrating to ADR tooling, adopting ADRs, converting existing decisions, setting up adr.yaml, bootstrapping architecture records
vocabulary: migrate adopt convert bootstrap setup greenfield legacy rename renumber frontmatter yaml scaffold import consolidate
macro: prepend
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# ADR Migration

## Identify Your Starting State

| State | Signs | Strategy |
|-------|-------|----------|
| **Greenfield** | No ADRs, no `docs/architecture/` | Scaffold from scratch |
| **Flat directory** | ADRs exist in one dir, sequential numbering (0001, 0002...) | Park as legacy, adopt domains going forward |
| **Inline metadata** | `Status: Accepted` in markdown body, no YAML frontmatter | Add frontmatter, keep body |
| **Scattered** | Decision docs in various locations (wiki, README, etc.) | Consolidate into `docs/architecture/` |
| **Different tool** | Using adr-tools, Log4brains, or similar | Export and convert |

## Greenfield Setup

No existing ADRs. Scaffold the full structure.

1. Create the directory tree:
```bash
mkdir -p docs/architecture docs/scripts
```

2. Create `docs/architecture/adr.yaml` from the template:
```bash
cp hooks/ways/softwaredev/architecture/adr/adr.yaml.template docs/architecture/adr.yaml
# Edit: set project_name, define domains for your project
```

3. Symlink or copy the ADR tool:
```bash
ln -s ../../hooks/ways/softwaredev/architecture/adr/adr-tool docs/scripts/adr
chmod +x docs/scripts/adr
```

4. Verify:
```bash
docs/scripts/adr domains    # Should show your configured domains
docs/scripts/adr list       # Should show 0 ADRs
```

## Flat Directory Migration

Existing ADRs like `docs/adr/0001-use-postgres.md` with sequential numbering.

1. **Scaffold the tooling** (greenfield steps 1-3 above)

2. **Park existing ADRs as legacy** — don't renumber:
```bash
mkdir -p docs/architecture/legacy
git mv docs/adr/0001-*.md docs/architecture/legacy/
# Rename to ADR-NNN format if needed:
git mv docs/architecture/legacy/0001-use-postgres.md docs/architecture/legacy/ADR-001-use-postgres.md
```

3. **Set the legacy range** in `adr.yaml` to cover existing numbers:
```yaml
legacy:
  range: [1, 99]
  label: "Legacy (Pre-Domain Numbering)"
```

4. **Add frontmatter** to each legacy file (see frontmatter conversion below)

5. **New ADRs use domains** — `docs/scripts/adr new core "Next Decision"` starts at 100+

## Inline Metadata Conversion

ADRs with metadata in the markdown body instead of YAML frontmatter:

```markdown
# ADR-014: Use Postgres for Session State          # Before (inline)

Status: Accepted
Date: 2026-01-15
Deciders: @alice, @bob
```

Convert to YAML frontmatter:

```markdown
---                                     # After (frontmatter)
status: Accepted
date: 2026-01-15
deciders:
  - alice
  - bob
related: []
---

# ADR-014: Use Postgres for Session State
```

Remove the inline metadata lines from the body after moving them to frontmatter. Run `docs/scripts/adr lint` to verify the conversion.

## Scattered Decisions

Decision records spread across wiki pages, README sections, or issue threads.

1. **Scaffold the tooling**
2. **For each decision**: `docs/scripts/adr new <domain> "Title"` to get a proper template
3. **Copy the substance** — extract Context, Decision, Consequences from the original source
4. **Set status to `Accepted`** if the decision is already in effect
5. **Link back** — add a `related:` entry or comment pointing to the original source for provenance

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

**A template is available** at `hooks/ways/softwaredev/architecture/adr/adr.yaml.template`.

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
