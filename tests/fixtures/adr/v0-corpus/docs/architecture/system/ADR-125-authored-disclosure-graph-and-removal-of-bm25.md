---
status: Accepted
date: 2026-04-17
deciders:
  - aaronsb
  - claude
related:
  - ADR-105
  - ADR-107
  - ADR-108
  - ADR-110
supersedes:
  - ADR-107
amends: ADR-108
---

# ADR-125: Authored Disclosure Graph and Removal of BM25

## Context

The way matching system has accumulated three matching tiers (embedding → BM25 → keyword/regex) and two parallel threshold systems (English `embed_threshold` in frontmatter, per-locale `embed_threshold` in `.locales.jsonl`). The original ADR-107 specified one threshold per way; in practice, 1411 per-locale entries now carry their own values, individually estimated and uncalibrated.

Recent multilingual feedback (Russian locale, six failing languages in `make test-multilingual`) traced to per-locale thresholds set higher than the multilingual model's actual cosine scores against real user queries. The existing `ways tune` command can't fix this — it measures stub-versus-corpus discrimination, not real-query recall, and produces "0 would change" across all 1411 entries while leaving the user-facing miss rate intact.

The deeper issue is architectural drift, not a calibration bug. Several concepts are already present in the system but under-named:

- ADR-105 established progressive disclosure (parents fire, children become reachable)
- ADR-110 established the way graph (nodes, edges, `ways-graph.jsonl` export)
- ADR-107 added locale stubs as packed JSONL aliases on each way
- ADR-108 added the multilingual embedding model alongside the English-only one

These four ADRs describe a single architecture — an authored DAG of ways, with embedding-coordinate aliases on each node, and disclosure semantics governing which subgraph is live in a session — but no ADR names that architecture explicitly. Without the name, each new feature was added as a localized patch (per-locale thresholds, per-tier fallbacks, per-language tuning logic) rather than as a property of the underlying model.

BM25 illustrates the cost of the unnamed model. It is a lexical tier that bypasses the graph entirely, uses an English-only stemmer (`Algorithm::English` in `bm25.rs:175`), and provided value primarily as a fallback when the embedding model was absent. With the embedding model now downloadable on four platforms via CI release artifacts, the fallback is rarely exercised, and extending BM25 to multilingual would require per-language stemmer wiring that fights the alias model.

## Decision

Name the architecture and remove what doesn't belong in it.

### 1. Authored disclosure graph

The way library is an **authored disclosure graph**:

- **Nodes** = ways (one file per node, unique filenames per ADR-110)
- **Edges** = parent/child (directory tree), siblings (cosine-weighted, computed by `ways siblings`), explicit `See Also` references
- **Coordinate aliases** = each node carries one or more embedding-space coordinates. The English content (frontmatter description + vocabulary + prose) produces the canonical alias. Locale stubs in `.locales.jsonl` produce additional aliases. Project-local extensions and domain-specific phrasings are also aliases under this model — multilingual is one application, not the headline.
- **Disclosure state** = the live subgraph reachable from the session frontier. Progressive disclosure (ADR-105) is the traversal mechanism over this graph.

"Authored" distinguishes this from LLM-extracted variants: nodes and aliases are written by humans (or by Claude under explicit authoring instruction), not derived at indexing time.

### 2. Embedding model as a black-box singularity

Retrieval is embedding-only. The multilingual model is treated as a black box: we accept its score distribution as the ground truth and design the system around that boundary, rather than augmenting it with lexical tiers that try to compensate for what the model "should" have done.

A node's match score against a query is:

```
node_score(query, node) = max over aliases A of cosine(embed(query), embed(A))
```

The `max` collapses node-aliasing into a single per-node score. A user query in Russian, a user query in English, and Claude's own native-language tool-call utterance all reach the same node through whichever alias is closest.

### 3. BM25 is removed

`bm25.rs`, BM25 score thresholds (`threshold:` in frontmatter when used for BM25), the two-tier fallback logic, and the engine-selection-by-model-availability code are removed. The embedding model becomes a hard dependency of `ways`.

### 4. One threshold per node, in English frontmatter

The per-locale `embed_threshold` field in `.locales.jsonl` is removed. Each node has at most one `embed_threshold` field, in the English frontmatter. Nodes that omit it use a system default.

Stub fidelity becomes a measurable graph property:

```
fidelity(node, alias) = cosine(embed(canonical_alias), embed(alias))
```

A low-fidelity alias is one whose embedding sits far from the canonical; the fix is to re-author the alias text (re-translate, fix vocabulary), not to lower a per-alias gate. `ways tune` is rewritten to measure and report fidelity, not to tune per-alias thresholds.

### 5. Explicit triggers survive

The `pattern:` and `commands:` regex fields in way frontmatter are not BM25 — they are explicit, deterministic triggers. They survive the tier removal and remain the override mechanism for "fire this way exactly when this pattern appears."

## Consequences

### Positive

- **One architectural model, named.** Future ADRs can extend "authored disclosure graph" instead of inventing parallel concepts. The model composes: aliases for non-language extensions (project-local, domain-specific) are now well-typed.
- **Threshold surface collapses from ~1411 dials to ≤83.** The reporter's Russian-locale bug (and all six failing languages in `make test-multilingual`) become a one-line consequence: per-alias gates were never the model.
- **Stub quality becomes measurable.** Fidelity is a number; low-fidelity aliases are visible in audit output and direct re-authoring effort to where it matters.
- **Matcher pipeline simplifies.** One tier, one scoring function, no engine selection. The black-box framing also stops the temptation to add lexical patches when the embedding behavior surprises us.
- **Removes ~English-only assumptions in the matcher.** With BM25 and its `Algorithm::English` stemmer gone, the matcher has no English-special-case code paths.

### Negative

- **Embedding model is a hard dependency.** `ways` cannot match without it. Setup must succeed at fetching/building the model; offline or air-gapped installs need the model present. Mitigation: model is 127MB, four-platform CI artifacts are already shipped, and `make setup` is the single command users run.
- **Per-locale calibration tweaks are no longer possible.** A language whose stub embeddings cluster lower than English's must be addressed by re-authoring the stub (raising fidelity) or lowering the node's threshold globally — there's no per-language escape valve. This is intentional, but it raises the bar on stub authoring quality.
- **Migration touches 1411 lines across ~83 `.locales.jsonl` files.** Mechanical (delete `embed_threshold` field), but it does change every locale entry on disk.

### Neutral

- The corpus generator still emits per-alias rows; the runtime just stops reading per-alias thresholds.
- `ways graph` and `ways siblings` (already shipped per ADR-110) are unchanged — they were already operating in the model this ADR names.
- `ways tune --audit` continues to surface ambiguous nodes (those whose canonical alias is confused with neighboring nodes); the audit's value increases now that fidelity is the explicit metric.

## Alternatives Considered

- **Per-locale calibration via `ways tune --from-queries`.** Curate query-set-per-language, calibrate thresholds to admit real queries, leave per-locale dials in place. Rejected — keeps the per-locale dial surface (1411 entries), shifts the calibration burden to query curation per language, and doesn't address the deeper drift from ADR-107's original intent.
- **Keep BM25 as a fallback for offline/no-model installs.** Rejected — BM25's English-only stemmer makes it actively misleading for multilingual users (silently degrades to "no match" rather than "incorrect match"), and maintaining a parallel matching path for the rare offline case is not worth the architectural cost.
- **Universal global threshold (single number, all nodes).** Considered, rejected for now — different ways have different score distributions against natural queries (broad ways like `ea` score lower than narrow ways like `delivery/commits`), so per-node thresholds carry real signal. May revisit once we have empirical data on whether per-node tuning matters in practice.
- **Coordinate-only ways (drop text, store vectors as source of truth).** Rejected — destroys human auditability of way intent. Authoring needs to happen in text; embeddings are derived.
- **Adopt "GraphRAG" as the architectural label.** Rejected — accumulates baggage from the Microsoft variant (LLM entity extraction, hierarchical community detection, expensive offline preprocessing) that doesn't match what we do. "Authored disclosure graph" describes the actual mechanism without importing the framework's reputation.

## Migration

1. Delete `bm25.rs` and BM25-related code paths in `ways-cli`
2. Remove BM25 `threshold:` fields from way frontmatter (migration script: identify which `threshold:` values were BM25 vs. other uses)
3. Strip `embed_threshold` from all `.locales.jsonl` entries (sed-able, one-time change)
4. Rewrite `ways tune` to measure and report alias fidelity (cosine to canonical), not per-alias thresholds
5. Update ADR-107 status to `Superseded by ADR-125 (in part — locale support model)`
6. Update ADR-108 to note BM25 fallback is removed; embedding tier is sole tier
7. Update `make test-multilingual` to verify the Russian and other failing-language queries now resolve via the alias model, with no per-locale threshold tuning required
