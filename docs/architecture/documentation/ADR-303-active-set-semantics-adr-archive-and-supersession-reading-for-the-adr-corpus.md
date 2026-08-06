---
status: Draft
date: 2026-08-06
deciders:
  - aaronsb
  - claude
related:
  - 177
  - 302
---

# ADR-303: Active-set semantics, adr archive, and supersession reading for the ADR corpus

## Context

Issue #438 laid out the problem against this repo's own numbers: 85 ADRs on
disk, all 85 in the index, ten formally `Superseded`/`Deprecated`, nine
`supersedes`/`superseded_by` frontmatter declarations — and zero code reading
any of it. Three defects fall out:

1. **Nothing bounds the active set.** `list` and `index` always answer "all of
   them"; a reader cannot ask for the decisions in force.
2. **The supersession frontmatter is a producer with no consumer.** It is
   written, never parsed, never validated — a typo'd or dangling reference is
   invisible.
3. **Supersession is usually partial, and `status` is whole-document.** Most
   prose supersession says "§4 was replaced; the rest stands." Marking the
   document `Superseded` discards what still lives; leaving it `Accepted`
   hides what died.

## Decision

Adopt issue #438's parts A and B as specified, and resolve its part C
(partial supersession) as **option 2 — section references, no new status**.

**A — `adr archive` and the active set.** `adr archive <number> --reason
"..."` moves the file to `docs/architecture/archive/<domain>/` via `git mv`
(plain rename outside a work tree), rewrites `status` to the archive status
(default `Superseded`, `--status` validated against `adr.yaml`), and prepends
a banner after the H1 carrying the date, the mandatory `--reason`, and any
`--superseded-by`. `find_adrs()` excludes `archive/` paths by default; `list`
shows the active set (`--archived`, `--all` widen it); `index` generates
main-table rows from the active set only, with archived ADRs in a collapsed
section. **`lint` deliberately still covers the archive** — moving a file must
not hide its problems.

**B — read the frontmatter that already exists.** `supersedes` and
`superseded_by` are parsed into `ADRInfo`; `lint` errors on a reference that
resolves to no known ADR and warns on non-reciprocal pairs; `list` and `index`
surface the relationship; `archive --superseded-by` writes the frontmatter
field, not only the banner prose.

**C — partial supersession by section reference.** A `superseded_by` entry may
carry a section reference (`ADR-167#4`). A document with any `superseded_by`
and an in-force status is *partially* superseded — no `Partially-Superseded`
status is added, and no consumer grows a fourth in-force state. `archive`
refuses to archive a partially superseded document: the archive is for
documents a reader no longer needs to open.

## Consequences

### Positive

- The corpus gains an askable active set; the index's front door shrinks to
  the decisions in force while every existing cross-reference still resolves.
- Nine existing frontmatter declarations become validated, navigable data;
  dangling and one-directional supersession links surface in lint.
- Partial supersession becomes indexable without a status-model change.

### Negative

- Archiving rewrites `status` and prepends a banner — the archived document is
  no longer byte-identical to its last active revision (history holds it).
- Section references are a convention inside a string field; nothing validates
  that `#4` names a real section.

### Neutral

- `adr-tool` bumps to 1.1.0 (ADR-177); vendored copies read as stale until
  re-vendored, which is the mechanism working.
- This repo's own ten superseded/deprecated ADRs become candidates for
  `adr archive` — a separate housekeeping pass, not part of this change.

## Alternatives Considered

- **Delete superseded ADRs.** Rejected: 27 documents reference each other's
  supersession; deletion breaks the web the archive untangles. Archived files
  stay tracked, linted, and linkable.
- **A `Partially-Superseded` status** (issue #438 option 3). Rejected: every
  consumer grows a fourth in-force state and the status still cannot say
  *which part*.
- **Leave partial supersession in prose** (option 1). Rejected: honest but
  unindexable, and worse at 200 ADRs than at 85.
