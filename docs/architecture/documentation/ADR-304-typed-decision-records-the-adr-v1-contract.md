---
contract: adr/v1
kind: decision
verb: add
capability: adr
basis:
  - operator: aaronsb, directed over attend and in PR #559
  - evidence: kg triage of 108 records; agent-ways citation audit
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

Under v1, ADR expands to Agent Decision Record. An architecture decision is
one kind of agent decision, alongside product choices to add, cut or retire.
The code citation format `ADR-N` stays as it is. The expansion changes in the
tool's help text, the ADR way's description, and the generated index title.

## Decision

Adopt a versioned record contract, `adr/v1`, declared in `adr.yaml`. Records
declare which contract they follow. `adr lint` enforces the grammar over each
record. `doclint` enforces citations from code against the grammar.

### 1. Record kinds are declared, and v1 seeds two

Kinds are data in the contract. Each kind declares its lifecycle, its
fields, and the edges it may carry to other kinds (§4). The tool lints any
record against its kind's declaration, so adding a kind is a contract change
and needs no tool change. The contract is a graph schema: kinds are node
types, and fields such as `supersedes`, `decided_by` and `basis` are edge
types. The corpus is the graph ADR-302 describes.

v1 seeds two kinds:

| Kind | Meaning | Body after acceptance |
|---|---|---|
| `decision` | why a choice was made | append-only |
| `spec` | how the thing works now | rewritten in place |

Likely later kinds include an evidence kind for #491 notes, if those move
into the number space. Each one arrives as a `change` decision on
`capability: adr`.

All kinds share the `ADR-N` number space, so existing citations keep
resolving. A spec record is the ADR-numbered counterpart of an ADR-302
reference or explanation page. It stays in the ADR series because code cites
it.

The decision kind is the Agent Decision Record proper. A decision is frozen
once it leaves proposed. That covers accepted, and also rejected, abandoned,
superseded and archived. Only the lifecycle fields listed in §4 move after
that point.

**Sub-parts.** A record numbered `N.k` (kg has `304.1`, `305.2`, `715.1`) is
its own record with its own kind, verb and status. A bare `ADR-N` resolves to
the family `{N, N.1, N.2, ...}`. A family has no status of its own:
supersession and enactment act on individual records. §6 says how a bare
citation is checked against the family.

**Splitting a mixed record.** The number stays with the half that code
citations describe. In kg that is the spec. Sampled citations of ADR-200 and
ADR-304 name DocumentMeta nodes and edge provenance metadata, not reasons.
Keeping the number on the spec leaves the 2,230 existing citations valid, and
keeping it on the decision would mean re-pointing nearly all of them by hand.
The decision half gets a new number and keeps the original `date`. It is
accepted on creation, because it transcribes a decision that was already
accepted. The spec links to it with `decided_by:`.

### 2. Vocabulary layers that share no word

| Layer | Holds | Words |
|---|---|---|
| L1 record lifecycle | `status`, set by tool operations | proposed, accepted, rejected, abandoned, superseded, archived |
| L2 decision verbs | what a decision does | add, cut, change, retire, constrain |
| L3 product state | derived, never written by hand | capability active/absent, surface present/gone, spec living/historical |
| L4 basis sources | what a decision rests on (§11) | operator, evidence, standard, upstream, precedent |

Rejected means considered and declined. Abandoned means dropped before
acceptance, the PEP "Withdrawn". The L1 operations (accept, reject, abandon,
supersede, archive) are `adr` subcommands. `reject` and `abandon` require
`--reason`, the same way `archive` does today. An operation and its state
sharing a stem is fine inside L1. L1 states compare case-insensitively, so a
v0 `Accepted` needs no rewrite.

No word or stem may appear in two layers. `adr lint` checks `adr.yaml` for
this, so a later contract version cannot reintroduce a collision. The check
covers the vocabulary layers only: the status set, the verbs, the derived
state words and the basis sources. Other `adr.yaml` keys are outside it, for example kg's
legacy `retired: true` range flag.

**Spec state.** A spec is living while its capability is active and nothing
has superseded it. It becomes historical when an enacted cut makes its
capability absent, or when a newer spec supersedes it. A historical spec may
then be archived. Specs use the same L1 operations as decisions, and a spec
supersedes a spec.

### 3. Decision verbs

- **add**: brings a capability into the vocabulary as active.
- **cut**: makes a capability absent.
- **change**: alters an existing capability. It must supersede or partially
  supersede (ADR-303 section references) a prior decision on the same
  capability. A prior decision is on the same capability when its
  `capability:` equals it, lists it, or is `*`. Changing a `*` constraint for
  one capability is a partial supersession, by section reference.
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
  decision:
    mutable_after_accept: [status, enacted, superseded_by]
    verb: required
    requires: [capability, basis]
    edges: { supersedes: decision, basis: [decision, spec] }
  spec:
    mutable_after_accept: all
    verb: forbidden
    requires: [capability]
    edges: { supersedes: spec, decided_by: decision }
basis_sources: [operator, evidence, standard, upstream, precedent]
capabilities:
  adr: Decision records, their contract, and the tooling that enforces it
  ingest: Document ingestion and extraction into the graph
surfaces:
  cli:   { inventory: "kg --list-commands" }
  route: { inventory: "scripts/list-routes" }
  mcp:   {}
```

- **kinds** declares each record kind: which fields may change after
  acceptance, whether a verb is required or forbidden, which fields are
  required, and which kinds each edge field may point at. The lint rules in
  §6 read this declaration and do not hard-code the two seeded kinds.
- **basis_sources** is the closed set of grounds a decision may cite (§11).
- **capabilities** is a closed vocabulary with one line per capability. That
  line is the capability's only hand-written description. `adr` is seeded in
  v1, so a contract change is itself a decision with `capability: adr`.
- **surfaces** declares this project's target namespaces. The inventory
  command is optional. Without one, `retire` targets are checked for syntax
  only. With one, the enactment rule in §5 applies to the inventory as well.
- **mutable_after_accept** names the lifecycle fields that may still change
  on a frozen decision (§1). The rest of the frontmatter and the body stay
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
basis:
  - operator: aaronsb, PR #612
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
- Where the surface has an inventory command, the same rule applies to the
  inventory. A retired target still listed warns before enactment and fails
  after it. A target the inventory never listed warns as a likely typo.

`enacted` uses a field, not a status, so L1 keeps its lifecycle unchanged.

### 6. Lint rules

`adr lint` checks each record against the grammar:

- `kind` is declared. A decision requires a `verb`, and a spec forbids one.
- `capability` is in the vocabulary. An unknown name fails.
- Every capability in the vocabulary has an accepted `add` decision. This
  warns while any v0 record remains and fails after, so a corpus that is
  still migrating does not fail on every capability.
- `change` supersedes or partially supersedes a prior decision on the same
  capability.
- `retire` carries `targets` in a declared surface namespace.
- A frozen decision's frontmatter changes only in `mutable_after_accept`
  fields.
- `adr.yaml` itself has no cross-layer word or stem reuse.

`doclint` checks code citations against the records:

- A number that resolves to nothing fails. This check exists today.
- A citation of a superseded decision warns and names the successor.
- A citation of a proposed record prompts acceptance. This covers proposed
  decisions, proposed specs, and v0 records in Draft or Proposed.
- Citations governed by a cut or retire decision follow the enactment rule
  in §5.
- A bare `ADR-N` citation is checked against its family (§1). It warns as
  superseded only when every member is superseded or archived, and then it
  names the successors. It prompts acceptance when any member is proposed.
  Enactment applies through each member's capability.

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

A migrated decision needs a `basis` (§11). v0 `deciders` cannot seed an
`operator` basis, because `adr new` fills it from the `adr.yaml` default,
which names the operator on every record. An `operator` basis comes from a
record of approval: an operator review or comment on the merging PR, or
operator direction quoted in the record. A linked #491 note seeds `evidence`.
A decision with neither migrates with no basis, and lint warns until a basis
is found or the operator supplies one.

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

### 10. Delivery: tool version and contract version are separate axes

Projects vendor `adr-tool` and `doclint` through the installer (ADR-177). A
project that vendored the legacy tool keeps working. The ADR way, however,
ships to every project, including ones still on the legacy shape. So the
guidance it discloses cannot assume v1.

Two things vary independently:

- **Tool version**: the vendored copy's `TOOL_VERSION`. The v1-capable tool
  is a major bump, and it lints `adr/v0` records exactly as the legacy tool
  does. Re-vendoring is therefore safe, and ADR-177's stale/customized/ahead
  disclosure applies unchanged.
- **Contract version**: `contract:` in the project's `adr.yaml`. When it is
  absent the project is on `adr/v0`. A project adopts v1 by declaring it, and
  a tool upgrade never adopts it on the project's behalf.

The ADR way's body stays contract-neutral: when to write a record, and what
belongs in one. The way's macro reads both axes and discloses the guidance
that fits:

| Vendored tool | `adr.yaml` contract | Disclosure |
|---|---|---|
| legacy | absent | v0 command reference; stale tool, re-vendor is safe |
| v1-capable | absent | v0 command reference; v1 is available, and adopting it is a decision (`capability: adr`) |
| v1-capable | `adr/v1` | v1 guidance: kinds, verbs, capabilities, enactment |
| legacy | `adr/v1` | the project declares a contract its tool cannot enforce; re-vendor before writing records |

`doclint` follows the same rule. Its v1 checks run only when `adr.yaml`
declares `adr/v1`. Contract-specific prose lives in the macro's output or in
files the macro selects, never in the always-on way body, so a v0 project is
never told to write `verb:` fields its tool rejects.

### 11. Basis: every decision grounds outside the corpus

A decision corpus that justifies itself only by citing its own records can
drift anywhere and still look consistent. Each decision therefore carries a
`basis:` naming what it rests on, and the chain has to reach something
outside the corpus.

| Source | Grounds the decision in | Reference |
|---|---|---|
| `operator` | the human who directed or approved it | who, and where: PR, issue or session |
| `evidence` | a measurement, benchmark or research note (#491) | the note or data |
| `standard` | an external specification, governance control or upstream behaviour | the citation (`governance-cite`) |
| `upstream` | another repository's accepted record under a shared contract | repo and record |
| `precedent` | another accepted decision in this corpus | `ADR-N` |

`operator`, `evidence`, `standard` and `upstream` are external. `precedent`
is internal. A decision may rest on precedent, but following its precedent
edges must reach a decision with an external basis. `adr lint` fails a
decision whose basis chain loops or stays inside the corpus.

Some decisions need the human:

- `add`, `cut` and `retire` change what the product is. They need an
  `operator` basis before acceptance. An agent may propose them, and they
  stay proposed until the operator's approval is recorded.
- A decision the record calls one-way, or irreversible, needs an `operator`
  basis whatever its verb.
- `change` and `constrain` may be accepted on `evidence`, `standard` or
  `upstream` alone. The ADR way tells the agent to raise them with the
  operator when the evidence is thin or contested.

The v1 ADR way discloses these rules and tells the agent when to stop and
ask. `adr accept` refuses a decision whose basis does not meet them.

The model follows Beer's Viable System Model. Records under a shared contract
form one system, and the operator is the external identity and policy
function that system cannot supply for itself. Repositories that share a
contract regulate each other through `upstream` edges, without either one
absorbing the other. `basis` sources form a fourth vocabulary layer, and the
no-shared-word check covers it.

### Rollout

This repo adopts first. It owns `adr-tool` and `doclint`, and its own corpus
is the migration test.

1. **Tool.** Implement the grammar in `adr-tool` (major bump per ADR-177) and
   the shared `doclint`, and add the macro branches from §10. The gate is a
   v0 regression test: on this repo's corpus with no `contract:`, the new tool
   must produce the same output as the legacy tool. Golden and negative
   fixtures cover each grammar rule.
2. **Adoption.** Declare `contract: adr/v1` in this repo's `adr.yaml` and
   accept this ADR. Accepting it is the `add` decision for `capability: adr`.
3. **Housekeeping.** Migrate this repo's records to v1, with splits,
   supersession chains, Deprecated mappings and at least one enacted cut or
   retire. Friction found here is fixed as `change` decisions on
   `capability: adr`, under the contract just adopted.
4. **Other repos.** kg and other adopters re-vendor the proven tool and
   declare v1 when ready. kg's triage and citation data inform steps 1-3.

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
- Family resolution makes bare citations lenient. A bare `ADR-N` stays quiet
  while any member is in force, even if the cited content moved.
- A split produces a decision record written after the fact. Its date and
  content come from the original, but its number is new.

- Every decision needs a `basis`, and `add`, `cut` and `retire` wait on the
  operator. Agents can no longer accept product-shaping decisions alone.
- The basis-chain check needs the whole corpus loaded, and a v0 record in a
  chain has no basis to follow. Until migration ends, the chain check treats
  a v0 record as external basis and warns.

### Neutral

- ADR-303's archive operation stays. It becomes one of the L1 operations and
  follows from a decision rather than needing its own.
- Implemented-but-proposed records need a one-time accept or abandon pass.
  kg has about 20.
- The ADR way's body currently carries v0 specifics: the status list, the
  template frontmatter, and the Draft-to-Accepted workflow. Those move into
  the macro's v0 branch, and the body keeps only contract-neutral guidance
  (§10).

## Alternatives Considered

- **Hard-code the two kinds in the tool.** Rejected: each new kind would
  need a tool release, and repos could not add kinds of their own. Declaring
  kinds in the contract costs one schema reader.
- **Basis as free prose in the Context section.** Rejected: prose cannot be
  checked, and nothing would stop a corpus that justifies itself.
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
- **The decision keeps the number when a record splits.** Rejected: kg's
  citations point at spec content, so every one would need re-pointing by
  hand.
