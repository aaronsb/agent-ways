---
status: Proposed
date: 2026-09-26
deciders:
  - aaronsb
  - claude
related:
  - 302
  - 303
---

# ADR-304: Typed decision records: the adr/v1 contract

## Context

In a project where ADRs replace a tracker and a product team, one record does
three jobs. It says what to build or cut (product), how the thing works now
(spec), and why (history). Code then cites the record by number, and the
citations become an index that nothing maintains.

The knowledge-graph-system (kg) repo measured this across 108 active ADRs
(~297k words):

- Only 19 records do one job: 13 are decisions and 6 are specs. The other 89
  mix jobs, and decision plus spec is the commonest mix (59). The typical case
  is a decision carrying DDL, endpoint lists and a phased roadmap.
- No record is purely a capability. Every capability-dominant record also
  carries spec.
- About 20 Draft or Proposed records are plainly implemented. One is cited 62
  times.
- 84 code citations point at Superseded ADRs, and nothing checks them.
- Runbooks, benchmarks, an explanation essay and roadmaps sit in the ADR
  corpus too.

This repo shows the same shape at smaller scale: 1,617 citations outside
`docs/architecture/`, 162 of them to Superseded or Deprecated ADRs. ADR-104,
ADR-119 and ADR-121 draw 29-33 each. `adr lint` checks only files under
`docs/architecture/`, and `adr archive` (ADR-303) never touches the code that
cites an archived record.

Prior art keeps decision records append-only and puts current truth
elsewhere: PEPs, Rust RFCs, KEPs, and IETF RFCs with `Obsoletes` headers.
Conventional Commits with commitlint, and Kubernetes `apiVersion`/`kind`,
show a small versioned grammar that a linter enforces.

ADR here reads as Agent Decision Record. The code citation format `ADR-N`
stays as it is.

## Decision

Adopt a versioned record contract, `adr/v1`, declared in `adr.yaml`. Records
declare which contract they follow. `adr lint` enforces the grammar over each
record. `doclint` enforces citations from code against the grammar.

### 1. Two record kinds in one number space

| Kind | Meaning | Body after acceptance |
|---|---|---|
| `decision` | why a choice was made | append-only |
| `spec` | how the thing works now | rewritten in place |

Both kinds share the `ADR-N` number space, so existing citations keep
resolving. A spec record is the ADR-numbered counterpart of an ADR-302
reference or explanation page. It stays in the ADR series because code cites
it.

The decision kind is the Agent Decision Record proper.

### 2. Three vocabulary layers that share no word

| Layer | Holds | Words |
|---|---|---|
| L1 record lifecycle | `status`, set by tool operations | proposed, accepted, rejected, abandoned, superseded, archived |
| L2 decision verbs | what a decision does | add, cut, change, retire, constrain |
| L3 product state | derived, never written by hand | capability active/absent, surface present/gone, spec living/historical |

Rejected means considered and declined. Abandoned means dropped before
acceptance, the PEP "Withdrawn". The L1 operations (accept, reject, abandon,
supersede, archive) are `adr` subcommands. An operation and its state
sharing a stem is fine inside L1. No word or stem may appear in two layers,
and `adr lint` checks `adr.yaml` itself for this, so a later contract version
cannot reintroduce a collision.

### 3. Decision verbs

- **add**: brings a capability into the vocabulary as active.
- **cut**: makes a capability absent.
- **change**: alters an existing capability. It must supersede or partially
  supersede (ADR-303 section references) a prior decision on the same
  capability.
- **retire**: removes surface while the capability stays active. It carries
  `targets:` naming the surface, such as `cli:ingest`, `route:/v1/jobs` or
  `mcp:search`.
- **constrain**: a cross-cutting rule with no state change. Its `capability:`
  may be a list or `*`.

Verbs apply to decisions only. A spec record carries a capability and no
verb.

Capability state is derived from the latest accepted add or cut decision for
that capability. `adr capabilities` prints the derived L3 state, which is the
capability ledger. Nobody maintains the ledger by hand.

### 4. The contract lives in adr.yaml

```yaml
contract: adr/v1
kinds:
  decision: { mutable_after_accept: [status, enacted, superseded_by] }
  spec:     { mutable_after_accept: all }
capabilities:
  adr: Decision records, their contract, and the tooling that enforces it
  ingest: Document ingestion and extraction into the graph
surfaces:
  cli:   { inventory: "kg --list-commands" }
  route: { inventory: "scripts/list-routes" }
  mcp:   {}
```

- **capabilities** is a closed vocabulary with one line per capability. That
  line is the capability's only hand-written description. `adr` is seeded in
  v1, so a contract change is itself a decision with `capability: adr`.
- **surfaces** declares this project's target namespaces. The inventory
  command is optional. Without one, `retire` targets are checked for syntax
  only. With one, the lint checks that targets exist before enactment and are
  gone after it.
- **mutable_after_accept** names the lifecycle fields that may still change
  on an accepted decision. The rest of the frontmatter and the body stay
  append-only. State lives in the file, so a squash, rebase or severed
  history cannot lose it.

A decision record's frontmatter:

```yaml
---
contract: adr/v1
kind: decision
verb: retire
capability: ingest
targets: [cli:ingest-legacy, route:/v1/upload]
status: accepted
enacted: 3f9c2a1
---
```

### 5. Enactment

A decision lands before the code it governs changes. That is the add/cut-first
flow. `cut` and `retire` stay open until their removal is done.

- With no `enacted:`, citations of the cut capability's records, or of the
  retired targets, warn. The warnings are the removal worklist.
- Once `enacted: <commit>` is set, the same citations fail.

`enacted` uses a field, not a status, so L1 keeps its lifecycle unchanged.

### 6. Lint rules

`adr lint` checks each record against the grammar:

- `kind` is declared. A decision requires a `verb`, and a spec forbids one.
- `capability` is in the vocabulary. An unknown name fails.
- Every capability in the vocabulary has an accepted `add` decision.
- `change` supersedes or partially supersedes a prior decision on the same
  capability.
- `retire` carries `targets` in a declared surface namespace.
- An accepted decision's frontmatter changes only in `mutable_after_accept`
  fields.
- `adr.yaml` itself has no cross-layer word or stem reuse.

`doclint` checks code citations against the records:

- A number that resolves to nothing fails. This check exists today.
- A citation of a superseded decision warns and names the successor.
- A citation of a proposed decision prompts acceptance.
- Citations governed by a cut or retire decision follow the enactment rule
  in §5.

### 7. Legacy records are adr/v0

A record without `contract:` is `adr/v0` and is linted as it is today. It
moves to v1 when someone next edits it. `adr lint` reports the v0 count, so
the migration stays visible. v0 statuses map as follows:

| v0 | v1 |
|---|---|
| Draft, Proposed | proposed |
| Accepted | accepted |
| Superseded | superseded |
| Rejected | rejected |
| Deprecated | superseded if something replaced it, else accepted with the spec historical |

### 8. What leaves the record corpus

- Runbooks and explanation essays go to ADR-302 catalog docs (how-to and
  explanation).
- Research, findings and benchmarks go to evidence notes that the decision
  links to (#491).
- Roadmaps become proposed add decisions.

### 9. Portability

Final decisions and all L3 state live in repo files. An issue tracker may
mirror them, and ADR-180 issues still track work in flight. An issue tracks
the work that moves a capability. It does not record what the capability is.

### Rollout

kg prototypes first. It triages its 108 records by kind and extends its
`doclint` to read each cited record's kind, verb and enactment. Once the
prototype holds, the grammar moves into `adr-tool` (version bump per ADR-177)
and the shared `doclint`. This repo then migrates its own records under the
v0 rules.

## Consequences

### Positive

- Code citations get a maintenance loop. Superseded, proposed and cut
  targets surface in lint.
- The capability ledger is generated from records, so it cannot drift from
  them.
- A decision record stays a readable history, because current truth moves
  to specs.
- A cut or retire decision produces its own removal worklist.
- Adoption is incremental. Untouched v0 records keep linting as they do now.

### Negative

- Splitting a mixed record is manual work. In kg that is 89 of 108 records.
- Every capability name has to be added to `adr.yaml` before a record can use
  it.
- `retire` checks are only as good as the project's inventory command. Many
  projects will have none at first.
- Two tools, `adr lint` and `doclint`, share the contract and must read
  `adr.yaml` the same way.

### Neutral

- ADR-303's archive operation stays. It becomes one of the L1 operations and
  follows from a decision rather than needing its own.
- Implemented-but-proposed records need a one-time accept or abandon pass.
  kg has about 20.

## Alternatives Considered

- **Capability as a third record kind.** Rejected: kg has no record that is
  purely a capability. Capabilities show up as the scope of decisions and
  specs, so a tag with a derived ledger fits the data.
- **Rename ADRs and split into separate document systems.** Rejected: the
  `ADR-N` citations in code are the most valuable part of the corpus.
  Renaming breaks them, and one number space keeps them resolving.
- **A new status for enacted cuts.** Rejected: it adds a fourth in-force state
  to L1 and mixes product state into the record lifecycle.
- **An `Enacts: ADR-N` commit trailer instead of an `enacted:` field.**
  Rejected: squash merges and history rewrites can drop trailers. A field in
  the file survives both.
- **Free-text capability tags.** Rejected: free-text names drift. A closed
  vocabulary makes an unknown name a lint failure.
- **A separate citation tool.** Rejected: `doclint` already scans code for
  ADR citations and guards retired number ranges.

## Open Questions

- When a mixed record splits, which half keeps the original number? The
  default proposal: the original keeps its number as the decision, and the
  spec gets a new number. `doclint` then suggests the spec for citations that
  describe behaviour.
- Does `abandoned` need a reason field the way `adr archive` requires
  `--reason`?
