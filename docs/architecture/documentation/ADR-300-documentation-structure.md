---
status: Accepted
date: 2026-02-17
deciders:
  - aaronsb
  - claude
related:
  - ADR-004
  - ADR-005
  - ADR-014
---

# ADR-300: Documentation Structure

## Context

The documentation makes a strong first impression. A reader landing on the README quickly grasps the core concepts: ways inject contextual guidance, skills are semantically discoverable, governance traces policy to agent behavior. The Severance metaphor lands. The pitch works.

Then the reader tries to *do* something — add governance to a way, create a way for a new domain, understand how semantic matching actually works — and the docs sprawl. There's no clear path from "I understand the concept" to "I know how to do this." Content appears in multiple files with no indication of which is canonical. References to NCD and BM25 contradict each other across files because the duplication made drift invisible. Domain docs describe ways that were never built. The governance system has code but no data pipeline. The README tries to be both landing page and reference manual and succeeds at neither.

This isn't a restructuring — the docs were never structured. They grew file-by-file as features were added, each locally coherent but collectively incoherent. This ADR establishes what each documentation layer is for, identifies what's cruft, and defines how a reader moves from concept to practice.

### What triggered this

Reading through the documentation and repeatedly encountering NCD references where BM25 should be. The specific inconsistency revealed the general problem: there's no coherent structure governing what goes where, so every addition risks drift.

### The structural audit

A full audit (`docs/audit-findings.md`) cataloged specific issues. A programmatic link graph (`scripts/doc-graph.sh --docs-only --stats`) confirmed the structural problems:

- **41 doc files, 29 links, 34 dead ends, 23 orphans**
- Only **3 hub files** do all the linking (README, docs/hooks-and-ways/README, governance/README)
- `docs/hooks-and-ways/README.md` — the best guide page (10 outgoing links) — is **unreachable from the main README**
- **7 domain docs** (cloud.md, mcp.md, ai.md, etc.) are completely isolated — zero links in or out
- **5 files** still present gzip NCD as primary when BM25 is the actual implementation
- **6 content areas** duplicated across 2-4 files each, several already diverged
- **README at 463 lines** — contains full tutorials that already exist in docs/

### Already resolved

As part of this ADR's development:

- **ADR tooling adopted**: `docs/scripts/adr` installed, `docs/architecture/adr.yaml` configured with domain numbering (system 100s, governance 200s, docs 300s)
- **ADR path reconciled**: project now uses `docs/architecture/` matching the way's convention
- **Legacy ADRs triaged**: ADR-001, 002, 003 deleted (superseded/irrelevant). ADR-004, 005, 013, 014 kept as legacy
- **Doc graph tool created**: `scripts/doc-graph.sh` programmatically maps the documentation link graph, identifies dead ends and orphans

## Decision

### 1. Define what each documentation layer is for

| Layer | Location | Audience | Purpose |
|-------|----------|----------|---------|
| **Landing** | `README.md` | First-time visitor | "What is this? How do I try it?" |
| **Guide** | `docs/hooks-and-ways/` | Practitioner | "How do I do X?" |
| **Reference** | `docs/hooks-and-ways.md`, `docs/architecture.md` | Contributor/debugger | "How does X work internally?" |
| **Policy source** | `governance/policies/*.md` | Governance chain | Source docs referenced by way provenance |
| **ADRs** | `docs/architecture/` | Decision record | Design decisions with context and consequences |
| **Machine layer** | `hooks/ways/*/way.md` | LLM runtime | Injected context — self-contained by design |

Content belongs in exactly one layer. Other layers link to it.

### 2. README becomes a landing page (~200 lines)

The README answers three questions: "What is this?", "How do I start?", and "Where do I go deeper?"

**Keep in README:**
- Hero image + Severance tagline
- What this is (overview + Mermaid diagram)
- Prerequisites table (corrected: BM25 primary, gzip fallback)
- Quick Start: fork-and-clone (recommended) and direct clone
- How It Works: trigger flow + directory tree (~20 lines)
- Configuration (`ways.json`)
- Built-in Ways table
- Philosophy
- Updating + License

**Replace with summary + link:**

| Current README section | Canonical location |
|----------------------|-------------------|
| Creating a Way + Frontmatter | `docs/hooks-and-ways/extending.md` |
| Semantic Matching | `docs/hooks-and-ways/matching.md` |
| Way Macros | `docs/hooks-and-ways/macros.md` |
| Project-Local Ways | `docs/hooks-and-ways/extending.md` |
| Ways vs Skills comparison | `docs/hooks-and-ways/README.md` |
| Governance example + chain | `governance/README.md` |
| Once-Per-Session Gating details | `docs/hooks-and-ways.md` |

### 3. Remove aspirational domain docs

Seven files in `docs/hooks-and-ways/` describe domains with zero way.md implementations:

`cloud.md`, `mcp.md`, `ai.md`, `research.md`, `enterprise.md`, `devops.md`, `sysadmin.md`

These are aspirational — they describe what ways *could* exist, not what does exist. A reader encountering `cloud.md` expects to find cloud ways and doesn't. Remove these files. When ways are built for new domains, documentation should accompany the implementation.

Policy source docs moved to `governance/policies/` — they're governance chain artifacts, not system documentation.

### 4. Address governance pipeline gap

> *Reconciled (2026-07): the tools named below (`governance.sh`, `provenance-scan.py`,
> `provenance-verify.sh`) were consolidated into `ways governance` (ADR-111) and now move
> to the `ways-audit` binary (ADR-151); provenance moved from way frontmatter to a
> `provenance.yaml` sidecar (ADR-110); and the subsystem is reframed as compliance
> claims/findings (ADR-200). The "pipeline gap" below is addressed there.*

The governance system has code (governance.sh, provenance-scan.py, provenance-verify.sh) and provenance metadata in way frontmatter, but output artifacts are not generated, tracked, or consumed. The docs explain what governance *is* but not how to *use* it.

Documentation fixes (scoped to this ADR):
- **governance/README.md**: add "Getting Started" at the top — `bash governance/governance.sh` is the entry point
- **docs/hooks-and-ways/provenance.md**: add "add provenance to your first way" walkthrough
- **State the gap honestly**: governance output is ephemeral, not CI-integrated. That's the current design, not an oversight

Pipeline fixes (separate ADR): whether to track output, integrate CI, or keep ephemeral.

### 5. Create docs/README.md as a map

A `docs/README.md` serves as a directory — not a guide. It answers "where do I find X?" with a file listing and one-line descriptions. The role-based reading paths stay in `docs/hooks-and-ways/README.md`.

Explicitly notes the relationship between `hooks-and-ways.md` (reference) and `hooks-and-ways/` (guides).

### 6. Fix stale content

| Item | Action |
|------|--------|
| NCD/BM25 in 5 files | Update to BM25-primary, NCD-fallback |
| Prerequisites docs (4 files) | Note gzip is for fallback path |
| ADR-014 | Accept (it's deployed) |
| `nested-ways-exploration.md` | Archive — nested ways are implemented |
| `rationale.md` | Update matching tiers, note model-based deprecated |
| ADR-013 lines 175, 201 | Update "gzip NCD" → "BM25 (with NCD fallback)" |

### 7. Severance theme: functional purpose test

The theme stays where it serves functional purposes:

- **"Write for the innie"** — concrete authoring principle (agent has no memory of previous sessions)
- **"Lumon handbooks"** — frames ways as institutional knowledge transfer
- **"The floor above"** — maps governance/policy separation to management/worker separation

**Guideline**: a Severance reference earns its place if it makes a concept *more understandable* than plain language alone. If removing it loses nothing, don't add it.

### What this ADR does NOT cover

- **Governance pipeline** (should output be tracked? CI-integrated?). Separate ADR.
- **Agent files** in `agents/` — potentially stale, separate assessment
- **Way file content** — this ADR is about docs, not the ways themselves

## Consequences

### Positive
- Each doc layer has a defined purpose — new content has an obvious home
- README drops from ~463 to ~200 lines — scannable landing page
- Aspirational domain docs removed — no more documenting unbuilt features
- NCD/BM25 inconsistency fixed across all files
- Governance docs gain a practitioner entry point
- ADR tooling adopted — project follows its own conventions
- `scripts/doc-graph.sh` provides ongoing coherence checking

### Negative
- **Link rot risk**: moving from inline content to links. Mitigation: `doc-graph.sh` detects broken links.
- **Contributor friction**: contributors must learn which file is canonical. The "everything in README" model had zero navigation overhead.
- **README loses grep-ability**: searching the repo for "semantic matching" finds a summary instead of the explanation.

### Neutral
- No structural changes to the hooks-and-ways guide/reference architecture — it works
- Severance theme stays exactly where it is — no additions, no removals

## Alternatives Considered

### Just fix the NCD/BM25 references and leave everything else
**Rejected**: The duplication that caused drift still exists. The next feature addition will duplicate content again and diverge again.

### Full docs restructure (guides/, reference/, concepts/ hierarchy)
**Rejected**: The existing docs tree works. The problem isn't the tree — it's that the README tries to be the tree, and aspirational docs sit alongside real guides.

### Documentation site generator (mdbook, Docusaurus)
**Rejected**: This is a config repo, not a framework. Adding build tooling contradicts the "bash + jq, no dependencies" philosophy.

### Keep README comprehensive, add a "canonical" marker system
**Rejected**: Metadata doesn't prevent drift — writers still need to update multiple files.

## Implementation Plan

1. Remove aspirational domain docs (cloud.md, mcp.md, ai.md, research.md, enterprise.md, devops.md, sysadmin.md)
2. Create `docs/README.md` (map, not guide)
3. Create `docs/installation.md` (full install guide extracted from README)
4. Slim README: replace duplicated sections with summary + link
5. Fix NCD/BM25 references across all identified files
6. Update `rationale.md` matching tiers
7. Archive `nested-ways-exploration.md`
8. Add "Getting Started" to governance/README.md
9. Validate: `scripts/doc-graph.sh --docs-only --stats` — check for new dead ends
