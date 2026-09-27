---
status: Accepted
date: 2026-03-29
deciders:
  - aaronsb
  - claude
related:
  - ADR-107
  - ADR-108
  - ADR-005
  - ADR-111
  - ADR-151
  - ADR-200
---

# ADR-110: Way File Separation and Graph-Compatible Structure

> **Accepted as implemented; reconciled with later ADRs.** The `provenance.yaml` sidecar
> defined here is the storage for what **ADR-200** reframes as a compliance **claim** (a
> control-*design* assertion, not evidence). Its consumers named below — `governance.sh`
> and `provenance-scan.py` — predate the single-binary consolidation (**ADR-111**); the
> sidecar is now read by `ways governance`, moving to the `ways-audit` binary
> (**ADR-151**). Read those tool names accordingly.

## Context

A way directory currently mixes four distinct concerns into one or two files:

1. **Matching frontmatter** — trigger configuration consumed by `way-match`, `way-embed`, and the scanner scripts. Fields: `description`, `vocabulary`, `threshold`, `embed_threshold`, `pattern`, `commands`, `files`, `scope`.
2. **Guidance body** — markdown prose injected into Claude's context by `show-way.sh`. This is the way's actual content.
3. **Macros** — executable shell scripts that generate dynamic content based on project state. Already separated as `macro.sh` (good precedent).
4. **Governance provenance** — control mappings, policy URIs, and justifications consumed exclusively by `governance.sh` and auditors. Currently embedded in way.md frontmatter as deeply nested YAML.

The provenance block is the most problematic. A typical way's frontmatter is 6-8 lines of matching config, but provenance adds 10-15 lines of nested YAML that no other consumer reads. The matching pipeline ignores it. `show-way.sh` strips it before injection. Only the governance pipeline parses it. It's instrumentation wearing the costume of content.

At 84 way files across 4 domains, the corpus is becoming difficult for humans to navigate. The directory tree provides domain grouping, but cross-references between ways are implicit (vocabulary overlap, shared domain ancestry). There's no way to see the relationships between ways without running the embedding engine or reading all 84 files.

Meanwhile, the ways system is approaching a point where humans other than the original author need to participate — reading, understanding, tuning, and authoring ways. The current structure requires understanding the frontmatter schema, the scoring system, and the governance model before making any edit. That's too high a barrier for someone who just wants to tune the prose in a way or understand how ways relate to each other.

### What prompted this

Three observations converged:

1. **Anthropic's harness design research** (2025) found that "every component in a harness encodes an assumption about what the model can't do on its own." Ways are exactly this — each one encodes an assumption. As models improve, some assumptions become stale. There's no mechanism to signal which assumptions a way is making or how firm they are.

2. **Man page architecture** has solved the "many structured documents, navigable by humans and machines" problem for 40 years with a clear separation: minimal metadata (one line), conventional body sections (`NAME`, `SEE ALSO`), derived indexes (`mandb`/`whatis`), and locale as a directory concern. Ways could follow the same pattern.

3. **Graph visualization tools** (Logseq, Obsidian, Cytoscape, or plain JSONL export) can make a corpus of 84+ documents navigable — but only if the documents carry their relationships in a format that's both human-readable and machine-extractable. The current frontmatter doesn't express relationships between ways.

## Decision

### 1. Extract provenance to a sidecar file

Move governance data from way frontmatter to a separate `provenance.yaml` in the same directory:

```
hooks/ways/softwaredev/code/quality/
  quality.md          # matching frontmatter + guidance body ({dirname}.md)
  macro.sh            # dynamic content (already separated)
  provenance.yaml     # governance instrumentation (newly separated)
```

`provenance.yaml` contains exactly what's currently under the `provenance:` key in frontmatter:

```yaml
policy:
  - uri: governance/policies/code-lifecycle.md
    type: governance-doc
controls:
  - id: ISO/IEC 25010:2011 (Maintainability - Analyzability, Modifiability)
    justifications:
      - File length thresholds enforce analyzability through size limits
      - Nesting depth limit maintains modifiability by controlling complexity
```

The governance CLI (`ways governance`, later the `ways-audit` binary — ADR-111/151) reads `provenance.yaml` files instead of parsing way frontmatter. The pipeline never needs to parse markdown again.

Way files that have no governance mapping simply don't have a `provenance.yaml`. Absence is the default, not an empty block.

### 2. Add conventional body sections for graph data

Instead of adding more frontmatter fields, use conventional markdown sections in the way body — following the man page pattern where `SEE ALSO` is a body section, not metadata:

**Epistemic stance** as an HTML comment (invisible when rendered, extractable by tooling):

```markdown
<!-- epistemic: heuristic -->
```

Four values, each declaring what kind of claim the way is making:

| Value | Meaning | Durability |
|-------|---------|------------|
| `premise` | Reasoning to incorporate, not a rule to follow | Durable — tied to the cognitive model |
| `convention` | How we do it here — functional choice, not universal truth | Stable but overridable per-project |
| `heuristic` | Works most of the time — override when context demands | May deprecate as models improve |
| `constraint` | Hard boundary (legal, security, compliance) | Durable — tied to external requirements |

**See Also** as a standard markdown section with typed references following the `name(domain)` convention from man pages:

```markdown
## See Also

- code/testing(softwaredev) — test coverage and fixtures
- code/errors(softwaredev) — error handling boundaries
- trust(meta) — why quality matters beyond the code
```

These are readable as plain text, parseable by any tool that knows the convention, and ignorable by any tool that doesn't.

### 3. Keep frontmatter matching-only

After provenance extraction, `{name}.md` frontmatter contains only fields the matching pipeline reads:

```yaml
---
description: code quality, refactoring, SOLID principles
vocabulary: refactor quality solid principle decompose
threshold: 2.0
pattern: solid.?principle|refactor
scope: agent, subagent
macro: append
---
```

Maximum 8-10 lines. No nesting deeper than one level. A human can read and edit this without understanding YAML block scalars or nested arrays.

The `scan_exclude` field stays in frontmatter because `macro.sh` reads it — it's part of the macro's configuration, not governance.

### 4. Build derived artifacts, not authoritative indexes

Following the `mandb` pattern, a generator script reads all way files and produces derived artifacts. The source of truth is always the individual files. The artifacts are build outputs, regenerated on demand:

| Artifact | Format | Consumer | Content |
|----------|--------|----------|---------|
| `ways-corpus.jsonl` | JSONL | `way-match`, `way-embed` | Matching fields (already exists per ADR-107) |
| `ways-graph.jsonl` | JSONL | Any graph tool, export scripts | Nodes (ways) + edges (See Also, sibling scores) |

All artifacts are gitignored or explicitly regenerated. None are authoritative. The generator is idempotent — running it twice produces identical output.

The `.vault/` directory previously described in this ADR (Logseq-compatible markdown with uniquely-named symlinks) is no longer needed. The file rename from `way.md` to `{dirname}.md` (see Section 7) gives every way file a unique name, making symlink indirection unnecessary. Any graph tool can consume the source files directly or use `ways-graph.jsonl`.

The `ways-graph.jsonl` format is deliberately simple:

```jsonl
{"id":"code/quality","domain":"softwaredev","epistemic":"heuristic","description":"..."}
{"id":"code/testing","domain":"softwaredev","epistemic":"convention","description":"..."}
{"source":"code/quality","target":"code/testing","type":"see_also"}
{"source":"code/quality","target":"code/errors","type":"see_also"}
{"source":"code/quality","target":"docs/standards","type":"sibling","weight":0.69}
```

Nodes and edges in the same stream. Importable by Cytoscape, D3, Logseq plugin, or a `jq` one-liner. The format doesn't assume any visualization tool.

### 5. Sibling scoring as an authoring compass

`way-embed` gains a `siblings` subcommand that scores way-vs-way across the full corpus:

```bash
way-embed siblings code/quality
# code/quality <-> code/testing:     0.78  (expected, different concern)
# code/quality <-> code/errors:      0.71  (expected, adjacent)
# code/quality <-> docs/standards:   0.69  (vocabulary overlap?)
# code/quality <-> delivery/commits: 0.43  (distant, good)
```

This is an **authoring tool, not a matching tool**. It never runs during prompt evaluation. It runs when a human or agent is tuning vocabulary and wants to understand how a way relates to its neighbors.

The sibling scores feed into `ways-graph.jsonl` as weighted edges. A graph visualization shows clusters (expected), outliers (review), and suspicious overlaps (vocabulary collision or candidate for `See Also`).

The compass metaphor is deliberate: it points north, it doesn't steer. The author interprets the direction. An agent reading sibling scores understands the sparseness landscape — which ways are close, which are distant — and uses that to calibrate vocabulary when authoring or tuning.

### 6. Tool agnosticism as a design constraint

The way file format does not reference or optimize for any specific visualization tool. Logseq, Obsidian, Cytoscape, a plain text editor, or `grep` are all valid ways to interact with the corpus:

- **vim/emacs**: Edit `{name}.md` directly. Frontmatter is short YAML. Body is markdown. See Also is readable text.
- **Logseq/Obsidian**: Open the ways tree directly. Every file has a unique name, so graph view works without a generated vault. Edit in place; regenerate graph JSONL if needed.
- **Cytoscape/D3**: Import `ways-graph.jsonl`. Visualize clusters and edge weights.
- **CLI**: `way-embed siblings`, `governance.sh --trace`, `lint-ways.sh`. Same data, terminal interface.
- **Claude**: Reads `{name}.md` as today. New sections are markdown it can interpret. Sibling scores inform vocabulary tuning.

If a tool requires a format the source files don't provide, the answer is a generator that produces the format — not modifying the source files to accommodate the tool.

### 7. File rename: `way.md` to `{dirname}.md`

Way files are renamed from the generic `way.md` to `{dirname}.md` — the file takes its name from its parent directory. For example:

```
hooks/ways/softwaredev/code/quality/quality.md
hooks/ways/softwaredev/code/testing/testing.md
hooks/ways/softwaredev/delivery/commits/commits.md
hooks/ways/meta/trust/trust.md
```

Similarly, check files are renamed from `check.md` to `{dirname}.check.md` (e.g., `quality.check.md`).

**Rationale:** Every way file being named `way.md` created several problems:

1. **Graph tool compatibility.** Logseq, Obsidian, and similar tools identify documents by filename. With 84 files all named `way.md`, these tools see 84 identically-named nodes — useless for navigation or graph visualization. The original plan (Section 4) required a `.vault/` directory with uniquely-named symlinks to work around this. Unique filenames eliminate that workaround entirely.

2. **Editor tab ambiguity.** Opening multiple ways in any editor shows tabs all labeled `way.md`. The developer must check the path to know which way they're editing. `quality.md` vs `testing.md` is immediately distinguishable.

3. **Search result clarity.** `grep` and `ripgrep` output includes the filename. Results from `quality.md` are self-documenting; results from `way.md` require reading the full path.

4. **Shell completion.** Tab-completing into a way directory and hitting tab again now shows a meaningful filename, not the generic `way.md` that every directory shares.

The rename is a one-time migration. Scanner scripts (`check-prompt.sh`, `check-bash-pre.sh`, `check-file-pre.sh`) discover way files by glob pattern, updated from `way.md` to `*.md` with exclusions for known non-way files (`macro.sh`, `provenance.yaml`, `*.check.md`). The `lint-ways.sh` and corpus generator scripts are updated correspondingly.

## Consequences

### Positive

- Way files become dramatically simpler. Frontmatter drops from 20+ lines to 8-10.
- Provenance is independently auditable without parsing markdown.
- Humans can author and tune ways without understanding governance mappings.
- Graph relationships become explicit and visible across the corpus.
- Epistemic stance makes the authority level of each way inspectable — both by humans choosing how firmly to follow guidance and by agents assessing which ways may need revision as models improve.
- Format is tool-agnostic. No vendor lock-in to any visualization tool.
- Sibling scoring gives authors (human and agent) a calibration tool for vocabulary sparseness.
- Unique filenames per way eliminate the need for a `.vault/` symlink directory and make the corpus directly navigable by graph tools, editors, and search.

### Negative

- Provenance extraction is a migration across 84 files. Each way with a `provenance:` block needs the block moved to `provenance.yaml` and the frontmatter cleaned. This is mechanical but must be validated by `governance.sh` and `lint-ways.sh` after migration.
- `governance.sh` and `provenance-scan.py` need to read `provenance.yaml` sidecar files instead of way frontmatter. The scan logic changes from "parse YAML frontmatter from markdown" to "read YAML file directly" — simpler, but a code change.
- `frontmatter-schema.yaml` needs updating: `provenance` moves from way schema to a separate provenance schema.
- Generator tooling is new code to write and maintain.
- `See Also` sections need to be added to existing ways. This is authoring work — each cross-reference should be intentional, not bulk-generated.
- The file rename requires updating all scanner scripts and any tooling that hardcodes `way.md`. This was a one-time migration but touched every scanner and the linter.

### Neutral

- `macro.sh` is unchanged. It was already separated correctly.
- The matching pipeline (`way-match`, `way-embed`, scanner scripts) requires a glob pattern update for file discovery but reads the same frontmatter fields.
- `show-way.sh` is unchanged except for removing the provenance-stripping step (no longer needed).
- `lint-ways.sh` gains new checks: validate `provenance.yaml` schema, validate `See Also` references point to existing ways, validate `epistemic` value if present.
- ADR-107's locale convention (`{name}-es.md`, `{name}-fr.md`) applies only to way files. `macro.sh` and `provenance.yaml` are shared across locales — macros execute the same logic regardless of language, and controls don't change by locale.

## Interaction with ADR-107

ADR-107 Phase 3 defines locale support with `{name}-{lang}.md` files (e.g., `quality-es.md`, `quality-fr.md`). This ADR clarifies which files are locale-specific and which are shared:

| File | Per-locale? | Rationale |
|------|-------------|-----------|
| `{name}.md` / `{name}-{lang}.md` | Yes | Matching vocabulary and guidance body vary by language |
| `macro.sh` | No | Detection logic is language-independent |
| `provenance.yaml` | No | Controls and policies don't vary by language |

This simplifies ADR-107's scope: locale support only touches way files, not the entire directory.

## Alternatives Considered

- **Provenance in the way body as a markdown section**: Would keep everything in one file. Rejected — provenance is deeply structured YAML (nested controls with justification arrays). Representing this as markdown would be awkward and harder to parse than a YAML file. The governance pipeline needs structured data, not prose.

- **A single domain-level provenance manifest** (e.g., `softwaredev/provenance.yaml` mapping all ways to controls): Would reduce file count. Rejected — it centralizes what should be co-located. When you're editing a way, its governance mapping should be in the same directory, not in a parent file you have to cross-reference. The sidecar pattern preserves locality.

- **Wikilinks in way.md frontmatter** (`see_also: ["[[code-testing]]"]`): Would make graph edges machine-readable from frontmatter. Rejected — this is tool-specific syntax (Logseq/Obsidian) in what should be a tool-agnostic format. The `## See Also` body section with `name(domain)` references is readable as plain text and parseable by a generator into any format.

- **Embedding graph data in frontmatter** (`epistemic`, `see_also`, `model_floor` as YAML fields): Would be machine-readable without body parsing. Rejected — every field added to frontmatter increases the barrier to authoring. The man page lesson is that conventional body sections scale better than metadata fields. Frontmatter should contain only what machines need for matching; everything else is body.

- **Knowledge graph MCP as the graph backend**: The project already has `mcp__knowledge-graph__*`. Ways could be ingested as nodes. Rejected for this purpose — adds a runtime dependency for what should be a static, file-based operation. The knowledge graph is appropriate for the cognitive framework paper's concepts, not for the way corpus which is already file-based and should stay that way.

## References

- ADR-107: Way-Match Corpus, Batch Mode, and Locale Support
- ADR-108: Embedding-Based Way Matching with all-MiniLM-L6-v2
- ADR-005: Governance Traceability for Ways
- Anthropic Engineering: "Harness Design for Long-Running Application Development" (2025)
- `hooks/ways/frontmatter-schema.yaml`: Current field definitions
- Provenance consumer: `ways governance` (consolidated from the former `governance/governance.sh` per ADR-111; moving to the `ways-audit` binary per ADR-151)
- Unix man pages: `man(7)`, `mandb(8)` — prior art for structured document corpora
