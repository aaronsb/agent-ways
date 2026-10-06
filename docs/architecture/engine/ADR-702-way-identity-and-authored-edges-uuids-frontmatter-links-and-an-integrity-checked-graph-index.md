---
contract: adr/v1
kind: decision
verb: change
capability: [authoring, matching, disclosure]
basis:
  - operator: aaronsb
    level: guided
    said: "the nesting of ways offers an acyclic graph relationship. additionally, relative symlinks could make it cyclic, if it was warranted. we could essentially have a graph of 'progressive disclosure' content that looks just like files in directories, but treat it as a knowledge graph"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "using a filesystem seems like an ideal tradeoff"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "the other option is an obsidian approach where the frontmatter has links. this would be where orienting them on disk doesn't really matter except for ease of authoring, but the actual graph is represented in computing all the frontmatter"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "the toggle probably needs to be independent of the way. does the way get a unique id?"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: directed
    said: "what if it was a UUID minted at the time of creating a new way from a template? we should still lint for colliding IDs. now we're running into the problem of enforcing a graph database..."
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: directed
    said: "maybe it's just something we check when building/checking integrity. the UUID has to exist, and it has to be unique. when the corpus is generated, we are really looking at the names of the way, it's path, and related ways (edges). Edges can be defined to other ways, just like the links that obsidian uses to make edges. But when we generate, we're actually using the UUIDs to form the graph and check it's integrity, and combine the relationships of UUIDs to edges defined in frontmatter. this means we can keep the index of the graph, and addtionally offer near neighbors in terms of semantic distance"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "let's write this up. I think this sounds pretty reasonable. we should launch a subagent to get some opinions if we're overbuilding this, or if there's (rust) code that does this for us already"
    via: chat, session 796c50d0, 2026-10-05
  - evidence: ADR-700
  - precedent: ADR-125
  - precedent: ADR-110
  - precedent: ADR-310
agent:
  name: claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "yes"
    via: "chat, session 796c50d0, 2026-10-05, approving the split with ADR-702 leading on UUIDs, parents: and related: and the reviewer's simpler forms as alternatives"
status: proposed
date: 2026-10-05
deciders:
  - aaronsb
related:
  - ADR-105
  - ADR-110
  - ADR-125
  - ADR-131
  - ADR-143
  - ADR-302
  - ADR-310
  - ADR-700
  - ADR-701
---

# ADR-702: Way identity and authored edges: UUIDs, frontmatter links and an integrity-checked graph index

## Summary

- **Decided (proposed):** each way carries a UUID minted by the template. Authors write further edges by name in frontmatter, `parents:` and `related:`, as `[[name]]` links, with directory nesting as the default parent. Corpus build resolves names to UUIDs, checks three integrity rules, and writes a graph index keyed by UUID that adds each way's nearest semantic neighbours. Toggles name ways by UUID or name, and the enabled state follows the parent graph at scan time. This record stays proposed until a concrete need for multiple parents or non-directory edges appears; ADR-701 does not depend on it.
- **Trades away:** three frontmatter fields and the lint that guards them, a one-time minting pass, a path-to-UUID alias map, and multi-parent rules in four places that today assume string-prefix ancestry.
- **One-way?** Reversible until UUIDs reach installed toggles and logged events; expensive after, because both then key on them.
- **Probes:** *Confident (uuid-at-creation):* identity is a UUID minted when a way is created from the template, and lint refuses a duplicate within a root. *Not confident (frontmatter-edges):* reference edges move from the body See Also section into `related:` in frontmatter, with See Also kept as prose that must agree.
- **Inversion:** between identity by location, where a way is its path, and identity held in a database. The decision puts identity in the file and enforces the graph with lint, keeping files the only source.

## Context

Way identity is the root-relative path. Toggles (ADR-131), project and user overrides (ADR-143), refire stamps, telemetry, golden files and the graph export all key on it. The directory tree is the only parent relation, and parent logic is string-prefix ancestry: `ancestors()` and `withheld_for_parent` in `scan/mod.rs`, `is_proper_ancestor` in `order.rs`, `with_ancestor` in `gate.rs`. Reference edges are the body See Also section (ADR-110 §2), which `graph.rs` already emits as edges.

Moves are rare: 149 way-file renames in the whole history, 12 since 2026-04-01, in four commits. A move today breaks the toggles, splits the telemetry and changes the override key of the way it touches.

As the corpus grows into domains beyond software, some ways belong under more than one parent, and an intentionally designed neighbourhood (ADR-701) needs edges that the directory tree cannot express. The corpus is small, authored and reviewed, so its integrity can be enforced by lint on files, as `adr lint` does for record numbers and references (ADR-310) and doclint for the documentation graph (ADR-302).

## Decision

### 1. Identity

- Each way carries `uuid:`, minted by the template when the way is created and never changed.
- The same UUID in two roots means the project or user way overrides the core way (ADR-143). The same UUID twice in one root is an error: it catches a way duplicated to start a new one.
- Runtime state keys on the UUID: toggles, session markers, refire stamps, events and judge candidates. Events also record the path at the time, so logs stay readable.
- People address ways by name or path. The CLI resolves them and writes the UUID with the path as a comment.

### 2. Authored edges

```yaml
uuid: 7f3c9a1e-…
parents: ["[[itops]]"]
related: ["[[threat-modeling]]"]
```

- The directory is the default parent. `parents:` adds further parents. Parent edges stay acyclic.
- `related:` holds reference edges and may form cycles. The body See Also section stays as prose the model reads, and lint checks that it agrees with `related:`. Both fields feed matching, which keeps frontmatter within ADR-110 §3.
- A bare name is the way's basename, unique within a root.
- **Resolution stays inside the corpus that declares the edge.** An edge from a core way resolves among core ways. An edge from a user or project way resolves in its own root, then in the roots below it in precedence. Because an override keeps the UUID of the way it replaces, an edge that resolves to a core way lands on the override in an install that has one. The graph therefore does not change shape between projects, only its node content.
- Symlinks inside a ways root are refused by lint.

### 3. Integrity

Three rules, checked by `ways author lint` and again at corpus build:

1. Every way has a UUID, unique within its root.
2. Every edge resolves to a way.
3. Parent edges are acyclic.

Lint fails the author. Corpus build degrades and reports: a dangling edge is dropped, a duplicate UUID keeps the first file found, and `ways status` names what was dropped. A rule joins these only when its violation is a real defect. A rename is caught at authoring time: lint reads `git diff -M` for moved way files and offers to rewrite `[[name]]` links that pointed at the old name. The build stays a function of the files alone (ADR-110 §4).

### 4. Enabling across several parents

- Toggles stay where ADR-131 and the user config put them: `disabled_domains` in user scope, per-way toggles in project scope. Per-way toggles in user scope stay out, as ADR-131 decided.
- A domain disable and a path-prefix toggle are subtree toggles. A per-way toggle is a node toggle.
- The effective state is computed at scan time, because project toggles are only known then: an explicit toggle on the way decides; otherwise a way with no parents is enabled; otherwise a way is enabled when any parent is enabled.
- The parent boost (ADR-105), parent withholding and the judge's ancestor block apply when any ancestor qualifies. The nearest-ancestor choice in `gate.rs` becomes the nearest by graph distance.
- `ways status` and `ways author tree` show each way's state and the toggle or parent that decided it.

### 5. The graph index

Corpus build extends `ways-graph.jsonl` (ADR-110 §4): nodes keyed by UUID with name, path, root and domain; authored edges with their source (directory or frontmatter); and computed edges to each way's nearest semantic neighbours by brute-force cosine over the alias vectors, above a floor. The index is derived and rebuildable. This amends ADR-125 §1 when accepted.

### 6. Compatibility

- A way without `uuid:` gets a deterministic UUIDv5 from its root-relative path at build, and lint warns until the field is written.
- A one-time pass writes UUIDs into the shipped ways. The manifest maps each old path id to its UUID, so existing toggles, `disabled_domains`, events and golden files keep resolving.
- `parents:` and `related:` are optional. Without them a way behaves as today.

### Implementation notes

`uuid` is the one new dependency. `petgraph` is optional, for cycle reporting and topological order; the alternative is about 40 lines. Link extraction, resolution, the integrity checks, the effective-state pass and the neighbour computation are short in-house code, with `docs/scripts/doclint` as the pattern for resolution and cycle checks and `cosine_similarity` in `ways-cli/src/cmd/siblings.rs` for neighbours. No existing tool covers UUID identity, frontmatter edges, precedence across roots and enabled state over a DAG.

### When to accept

Accept when a way needs a second parent or an edge the tree cannot express, or when a move breaks toggles or telemetry in practice. Until then ADR-701 runs on path ids, the directory tree and See Also.

## Consequences

### Positive

- Moves stop breaking toggles, telemetry and edges.
- A way can sit in two domains without a copy.
- Overrides and accidental duplicates become distinguishable.
- The judge and the lookup tools can show authored and semantic neighbours, labelled.

### Negative

- Multi-parent rules replace string-prefix ancestry in four places.
- `related:` and See Also are two places for the same edge, held together by lint.
- Every event, toggle and stamp gains an alias path during migration.

### Neutral

- Files remain the only source of truth. The index, masks and neighbour edges are derived.
- Disabling a domain no longer hides a way that an author also placed under an enabled parent.

## Alternatives Considered

- **Path ids, as today, with an optional `formerly:` field or a committed `moved.tsv`.** Old toggles, events and edges resolve through the alias, lint checks it against `git diff -M`, and an accidental copy is caught by alias-vector cosine of 0.98 or more. The cheapest option at today's move rate; it keeps location as identity and cannot tell an override from a duplicate by id.
- **Readable slugs as ids.** Rejected: a copied way keeps its slug, so lint cannot tell an override from a duplicate.
- **Subtree toggles as path prefixes in the project `ways:` map, with no `parents:`.** Covers today's toggling need with string ancestry and no graph walk. ADR-701 adopts this form now; it does not give a way two parents.
- **See Also as the only reference-edge source, with a resolution lint and no `related:`.** One source of truth, no sync lint, no amendment to ADR-110 §2. Fits until edges need types the prose section cannot carry.
- **Semantic neighbours computed on request** with `ways author siblings`, not stored in the index. Adequate for authoring; storing them serves the lookup tools without recomputation.
- **Rename detection through the previous build's index.** Rejected: the build would depend on the previous build, and a fresh clone has none.
- **Relative symlinks as edges.** Rejected: the scanner reads a linked way as a second node, and symlinks break under some platforms and git settings.
- **Hold the graph in a database.** Rejected: three integrity rules enforced by lint cover what a database's keys and constraints would. Concurrent writers, transactions and live queries do not occur, since writes arrive through pull requests and queries run on the derived index.
