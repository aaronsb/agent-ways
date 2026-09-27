---
status: Accepted
date: 2026-06-19
deciders:
  - aaronsb
  - claude
related:
  - ADR-300
  - ADR-301
---

# ADR-302: A unified documentation model — typed graph, ways packaging, cross-repo convergence

> **Scope note.** This ADR was first drafted (2026-06-16) as "Diátaxis as the
> documentation classification model" and grew off the rails by stapling three
> things together. It is rewritten here around a single spine, renamed to match
> (`ADR-302-unified-documentation-model.md`), and kept in the `documentation`
> domain (300s) — its subject is documentation tooling; the ways packaging in §7
> is the delivery vehicle, not the subject.

## Spine

Documentation is **one typed graph**. The type lives in **frontmatter and a
linter**, not in filenames. The **filesystem is a serialization** of the graph
for human readers — a view, not the source of truth. We **package the model as a
`documentation/` ways root plus skills**, and **converge repos onto it** through
the existing `project-init` / `project-audit` rail. Diátaxis, ADR/doc numbering
unification, `doclint`, and folder layout are all *facets of this one spine*, not
separate decisions.

## Context

ADR-300 established *where* documentation lives (location/audience layers) and
shipped `doc-graph.sh`. ADR-301 directed prose to lead with established
vocabulary. Two gaps remain:

1. **ADR-300's "Reference" layer conflates two needs** — "how X works internally"
   (mechanism = *explanation*) and "the exact facts" (*reference*) under one word.
2. **ADRs and prose docs are two uncoordinated systems.** Their numbering has
   never been coordinated; "where is the decision behind this page?" is answered
   by grepping and hoping.

The `knowledge-graph-system` project (KGS) independently worked this through and
arrived at a corrected, *running* model — ADR-900 (domain numbering), ADR-908
(documentation strategy, amended and corrected 2026-06-16), and a working
`doclint.py`. **KGS is the reference implementation and the driver.** Its hard-won
corrections are load-bearing here and are adopted rather than re-derived:

- Diátaxis is a **closed 2×2** — four modes, no fifth. KGS tried an `operations`
  fifth mode and retired it (2026-06-16) as a *category error*: it put an
  *audience* on the *posture* axis.
- The catalog id is **`DD.NNN.P`** (domain band · domain-scoped serial · trailing
  mode pole), *not* `<domain>.<mode>.<serial>`. Baking the mutable mode into the
  middle of an identity churns the "part number" on every reclassification.

### The thesis that makes this worth the rigor

A typed system's sustainable richness is bounded by **who maintains it**. Humans
cap out at a level of cross-referential bookkeeping and respond by inventing loose
conventions (organize-by-audience, folder-by-feature) — and, on complex projects,
by hiring a person whose whole role is to hold the schema in working memory. That
cap is a property of the *maintainer*, not the problem. An AI coding agent does
not cap at the same place: a typed graph it would take a human minutes to reason
through, it sustains in one pass. So the right design **pushes rigor past the
human-convention comfort line on purpose** — into frontmatter and a linter, where
the maintainer that actually maintains it pays almost nothing — and demotes the
filesystem to an ergonomic *serialization* for the humans who still read and
occasionally edit.

## Decision

Adopt a unified documentation model with the following parts. `adr.yaml` is the
single source of truth for the domain axis, shared by ADRs and docs alike.

### 1. One typed graph

Docs and ADRs are **nodes** in one graph; `related` / `supersedes` are **edges**.
Not two systems with a convention bolted between them. "Where's the decision
behind this page?" becomes an edge traversal.

### 2. The type lives in frontmatter, not the filename

Every catalog node carries frontmatter that is **dual-readable** — `doclint` reads
it as the type, Obsidian reads it as a graph:

```yaml
---
id: 04.001.H                       # DD (domain band) . NNN (domain-scoped serial) . P (mode pole)
domain: auth                        # ADR-900/adr.yaml domain key — the shared "first octet"
mode: how-to                        # Diátaxis: tutorial | how-to | reference | explanation
aliases: ["04.001.H", "04.001"]     # stable handles so [[04.001]] resolves in Obsidian
related: ["[[ADR-300]]", "[[05.002]]"]   # wikilink edges — doclint strips [[ ]] & resolves; Obsidian draws them
supersedes: []
---
```

- **Identity is `DD.NNN`** — assigned once, immutable, never reused.
- **The mode pole `P` trails** because mode is the *mutable* attribute;
  reclassifying flips `…​.H → …​.E` and the identity is untouched. The pole is a
  *view* of `mode:`, enforced to agree, never the key.
- **Serials are domain-scoped**, so any id collision is a real clash.
- **Edges are `[[wikilinks]]` and `aliases` carries each node's stable handle**, so
  the same `related`/`supersedes` lists `doclint` validates also render as a live
  graph in Obsidian (§6). `doclint` strips the `[[ ]]` and resolves the inside
  against ids/aliases; Obsidian needs the alias to resolve `[[ADR-300]]` to
  `ADR-300-….md`. Canonical type fields (`id`/`domain`/`mode`) stay first-class —
  no `tags` mirror to drift.

This explicitly **rejects** the original draft's `3.H.4` (`domain.MODE.serial`),
which bakes the mutable classifier into the middle of the identity — the exact
scheme KGS retired.

### 3. Classification is Diátaxis — four modes, closed 2×2

`tutorial | how-to | reference | explanation`, derived from two orthogonal axes
(action/cognition × acquisition/application). There is **no fifth mode**.
`operations` is *not* a mode — see §6. This also resolves ADR-300's "Reference"
conflation: mechanism → **explanation**, dry lookup facts → **reference**.

### 4. The domain axis is shared with ADRs (the "first octet")

The `DD` band resolves through each repo's `adr.yaml`. A doc and the ADRs that
govern it share the band, so "everything about auth" spans both trees. **The
grammar is universal; the domains are per-repo.** `04` is `auth` in KGS and
unassigned in agent-ways — ids are repo-local, *not* a global namespace.
Convergence means conforming to the same grammar and lint, not sharing IDs.

### 5. Linting is the authority — and the enforcement tier is the real decision

`doclint` (successor to `doc-graph.sh`, generalized from KGS's `doclint.py`,
reads `adr.yaml`) treats docs+ADRs as one graph and checks:

1. **Frontmatter validity** — well-formed `id`/`domain`/`mode`; id's band and pole
   agree with the fields; `id ∈ aliases` (so Obsidian wikilinks resolve — violation
   = broken links, which is why this invariant earns its place).
2. **Edge integrity** — `related`/`supersedes` resolve (after stripping `[[ ]]`);
   no supersede cycles; no orphans outside the nav.
3. **Coverage matrix** — which `(domain × mode)` cells are populated, surfacing
   gaps (e.g. "auth has reference but zero how-to").

A typed system with an *advisory* linter is a loose convention with extra YAML.
The type exists only where violation **gates** (fails CI). Default tier (from
KGS): **errors on catalog pages, warnings on ADRs** until the ADR frontmatter
sweep lands. Governing rule: **an invariant earns its place only if its violation
is a real defect a human would eventually hit** — coverage gaps, dangling edges,
supersede cycles, id/mode disagreement all qualify; "every page needs N links"
does not.

### 6. Serialization for human readers is a separate layer

The typed graph is **canonical**. Folders, filenames, navigation, and the
rendered site are **serializations** — views, possibly several of one graph:

- The developer's on-disk tree (loose, refactor-freely).
- The published site (mkdocs strips unknown frontmatter — readers never see
  `04.001.H`; maintainers and the linter do).
- Audience bundles — an operator-facing destination gathering the nodes an
  operator needs, *regardless of their individual modes*.
- **Obsidian's graph view** — the wikilink edges in frontmatter (§2) render as a
  live, navigable graph of the decision corpus with zero extra tooling. Because the
  edges live in *frontmatter*, mkdocs strips them (the published site never sees a
  `[[wikilink]]`) while the note *body* stays plain portable markdown. One graph,
  several readers (dev tree, Obsidian, mkdocs), no divergence — the §6 thesis in
  miniature.

**This is where audience lives — never as a type.** "Operations" was never a
Diátaxis mode and only awkwardly a domain; it is an **audience serialization**.
KGS's `self-host/` folder (a multi-mode operator destination) is exactly this.
The original draft's error was promoting an audience (operations) into a type (a
fifth mode); serialization is where audience was supposed to go all along.

### 7. Packaging: a `documentation/` ways root + skills

The model ships as a new top-level ways root — the single source of truth for the
convention — with two altitudes:

- **The model** (types the corpus): `graph` (premise — nodes+edges), `frontmatter`
  (the typed node / id grammar), `diataxis` (the mode enum), `adr` (a node *type* —
  the decision record; **moved in** from `architecture/adr`, since the graph frame
  privileges adr-as-node and co-locates it with what types and lints it),
  `linting` (doclint + enforcement-tier doctrine), `serialization` (the
  human-reader projection).
- **The craft** (authors one artifact well): `standards/readme`, `mermaid`, `api`,
  `docstrings`, `standards` — largely relocated from `softwaredev/docs`.

`documentation.md` is the **premise parent** that states the spine and routes to
children (the `architecture.md` / `code.md` idiom). The `doclint` tool sits beside
`adr-tool` (they share `adr.yaml`), exposed as a `/doclint` skill mirroring
`/adr`, scaffolded by `project-init` and verified by `project-audit`. Matching is
embedding-based (path-independent), but the **tree is the disclosure graph** — the
move reshapes disclosure parents and carries a mechanical blast radius (the
`docs/scripts/adr` symlink target, `macro.sh` self-refs, `See Also` cross-links,
corpus rebuild).

### 8. Rollout — converge onto a *frozen* convention, KGS first

You cannot converge N repos onto a moving target. Sequence:

1. **Freeze the convention** in one home — the `documentation/` ways + a versioned
   portable `doclint` (the `adr-tool` precedent).
2. **KGS first.** It is the most-evolved instance and the reference; formalize the
   generalized convention against it and bring it fully into conformance. This is
   the **first real test** of the ways/skills additions.
3. **agent-ways second** — its `docs/` + ADRs (a clean target).
4. **Any repo** — greenfield via `project-init`.

## Out of scope

- **Typing the ways corpus itself** (`hooks/ways/`). Ways blend modes by design
  (just-in-time steering legitimately fuses how-to + reference + explanation), so
  one-mode-per-artifact does not apply. At most an authoring `mode:` hint — a
  separate decision. "Converge everything" must not quietly promise to type the
  ways.
- **The full `doclint` specification and CI wiring** — the invariant *set* is named
  here (§5); exact exit codes, tiers, and pipeline integration are implementation.
- **Retroactive classification of existing pages** — branch work, not a decision.

## Consequences

### Positive

- Decision↔prose is one cross-linked graph; ADR-300's "Reference" conflation is
  resolved.
- The domain axis is reused, not reinvented — ADRs and docs cannot disagree about
  what a domain is.
- The model is portable: universal grammar, per-repo domains, one tool, one rail.
- A coverage matrix makes documentation gaps measurable.
- Filesystem stays human-friendly and refactorable; the rigor that would burden a
  human lives where an agent sustains it cheaply.

### Negative

- Every catalog page needs frontmatter — per-page authoring and migration cost.
- A `documentation/` ways root + `adr` relocation has real blast radius (symlinks,
  skill defs, disclosure-tree parents, corpus rebuild).
- The model has more moving parts (graph / frontmatter / diataxis / linting /
  serialization) than "put docs in folders"; the payoff is only realized if the
  linter actually gates.

### Neutral

- Supersedes `doc-graph.sh`'s role conceptually; it stays until `doclint` lands.
- Forces the eventual ways-corpus question (out of scope here) into the open.

## Alternatives considered

### The original draft: `operations` as a fifth mode, id `3.H.4`
**Rejected.** KGS already disproved both empirically: a fifth mode puts an
audience on the posture axis (category error), and `domain.MODE.serial` churns the
identity on every reclassification. Serialization (§6) and `DD.NNN.P` (§2) are the
corrected forms.

### Keep ADR-300's layer model as the only taxonomy
**Rejected.** Conflates explanation with reference and offers no decision↔prose
graph.

### Diátaxis folders *as* the structure
**Rejected.** Folders are a *serialization*, not the type. Binding structure to
folders reproduces the audience-drift ADR-908 hit (one feature living in several
folders, pages drifting apart).

### Docs and ADRs as separate systems
**Rejected.** The fusion — one graph, shared domain band — is the point.

### A standalone explainer doc instead of an ADR + ways
**Rejected.** A prose doc restating a decision is a second source of truth to keep
in sync — the drift this model exists to prevent.

### Build a new cross-repo convergence tool
**Rejected.** The rail exists — `project-init` / `project-audit` already scaffold
and audit ADRs, ways, and docs into any repo. Add the catalog car; don't build a
new train.

## Open questions

- **`documentation/` as a root peer** vs `softwaredev/documentation/` — the
  practice generalizes beyond dev (research, writing), but most existing children
  are dev-flavored. Resolved as part of the `softwaredev/` decomposition (separate
  implementation plan).

Resolved during drafting: this ADR stays **ADR-302 / `documentation` domain** (its
subject is documentation tooling); the file is renamed to
`ADR-302-unified-documentation-model.md`; frontmatter adopts Obsidian-compatible
wikilink edges + `aliases` (§2, §5, §6).
</content>
</invoke>
