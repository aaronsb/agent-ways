---
contract: adr/v1
kind: decision
verb: add
capability: adr
basis:
  - operator: aaronsb
    level: guided
    said: "agent ways should lead the champagne here and once it works, that where agent ways can interrupt and do the adr housekeeping"
    via: session 2026-09-26, PR #559
  - evidence: kg triage of 108 records; agent-ways citation audit
  - evidence: research survey, see References
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "I think we have put as much effort into this adr as we need to."
    via: session 2026-09-26, PR #559
    covers: []
  - operator: aaronsb
    said: "I read the entire adr as it finally sat and it was an enjoyable read that captures the intent and spirit. The negatives are mostly mechanical impacts of needing to migrate other adr systems"
    via: session 2026-09-27, after merging PR #559
    covers: []
status: Accepted
date: 2026-09-26
deciders:
  - aaronsb
  - Claude
related:
  - 302
  - 303
---

# ADR-304: Typed decision records: the adr/v1 contract

## Summary

- **Decided:** ADR now means Agent Decision Record. Records have declared
  kinds, starting with decision (append-only) and spec (living), all in one
  `ADR-N` number space. Decisions carry a verb (add, cut, change, retire,
  constrain), a capability from a closed list, and a basis that must reach
  outside the corpus. `adr lint` and `doclint` enforce this and tie code
  citations back to the records.
- **Trades away:** splitting mixed records costs manual work, and every
  decision now needs frontmatter and a summary. Agents can no longer accept
  decisions grounded only in other decisions.
- **One-way?** No. The contract is opt-in per repository (`contract:` in
  `adr.yaml`), v0 records keep working, and this repo adopts first as the
  test.
- **Probes for the operator** (§12):
  - *Confident:* the decision/spec split and the closed verb list. PEP,
    Rust RFC and Conventional Commits evidence backs them. Is there a record
    in your repos that is neither a decision nor a spec?
  - *Not confident:* the closed capability list. No study found says it holds
    or drifts. Will naming every capability in `adr.yaml` feel like friction?
  - *Not confident:* the challenge protocol in §12 steers your judgement.
    Did these probes help, or did they narrow what you looked at?
- **Inversion:** one end is AgDR-style free records, written by agents, with
  no grammar and only citation lint. The other end is kernel-style human
  sign-off on every decision. This design sits between them. Is the middle
  right, or is it a compromise that neither end would choose?

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
elsewhere: PEPs and Rust RFCs send final documentation to a reference and
freeze the proposal. IETF `Obsoletes` works. Its untyped `Updates` edge did
not, because nobody could say what an update meant (see §3 on typed amend
edges).
Conventional Commits with commitlint, and Kubernetes `apiVersion`/`kind`,
show a small versioned grammar that a linter enforces.

Under v1, ADR expands to Agent Decision Record. The name is already used by
AgDR, a format for agents to record their own decisions with model and
session metadata. This contract keeps `ADR` because the `ADR-N` citations
predate it, and it borrows AgDR's agent metadata (§11). An architecture decision is
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
  amend a prior decision on the same
  capability. A prior decision is on the same capability when its
  `capability:` equals it, lists it, or is `*`. Changing a `*` constraint for
  one capability amends it.

Partial replacement uses a typed edge, not a bare section reference.
`amends: ADR-167#4` replaces the named section and leaves the rest in force.
`extends: ADR-167` adds to a decision without replacing any of it. These
replace ADR-303's untyped `superseded_by: ADR-167#4` form, which repeats the
IETF `Updates` failure. `adr lint` checks that the named section exists.
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
    mutable_after_accept: [status, enacted, superseded_by, considered, concern]
    verb: required
    requires: [capability, basis, agent]
    sections: [Summary]
    edges: { supersedes: decision, amends: decision, extends: decision, basis: [decision, spec] }
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
  - operator: developer
    level: directed
    said: "drop the legacy upload path"
    via: PR #612
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
- `change` supersedes or amends a prior decision on the same
  capability.
- `retire` carries `targets` in a declared surface namespace.
- An `amends: ADR-N#k` edge names a section that exists.
- A decision carries `agent`, and its `## Summary` carries probes and an
  inversion.
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
which names the operator on every record. Forge metadata cannot seed it
either (§11). An `operator` basis migrates only where the record already
quotes operator direction. A linked #491 note seeds `evidence`.

Examples and shipped templates use the role placeholders `developer` and
`agent`. Real records carry real identities, such as `aaronsb` and `Claude`.
No shared template hard-codes a person as a default decider. A decision
with neither migrates with no basis, and lint warns until a basis is found
or the operator supplies one.

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
| `operator` | the human's involvement, at a declared level | who, the level, what was said, and via which channel: session, issue, chat or call |
| `evidence` | a measurement, benchmark or research note (#491) | the note or data |
| `standard` | an external specification, governance control or upstream behaviour | the citation (`governance-cite`) |
| `upstream` | another repository's accepted record under a shared contract | repo and record |
| `precedent` | another accepted decision in this corpus | `ADR-N` |

`operator`, `evidence`, `standard` and `upstream` are external. `precedent`
is internal. A decision may rest on precedent, but following its precedent
edges must reach a decision with an external basis. `adr lint` fails a
decision whose basis chain loops or stays inside the corpus.

**These are agent decisions, and no verb waits on a human.** An agent may
propose and accept any decision, including `add`, `cut` and `retire`, once
its basis chain leaves the corpus. Gating decisions on human review would
run them at human pace and lose the reason to have agent decision records
at all.

The operator can enter at any point along a range. An `operator` basis
records where they entered with `level:`:

| Level | The operator | The decision is |
|---|---|---|
| `authored` | wrote the record | the operator's, recorded in the agent corpus |
| `directed` | made the call, and the agent wrote it up | the operator's, written by the agent |
| `guided` | gave direction or a constraint, and the agent decided within it | the agent's |

A decision with no `operator` entry is the agent's alone, grounded in
`evidence`, `standard` or `upstream`. The level records how much a human
shaped the decision. It does not rank the decision's authority. Guidance
narrows the space the agent decides in, and a `guided` decision is still an
agent decision.

The ADR way tells the agent when to involve the operator: when the decision
is one-way, when the basis is thin or contested, or when it changes what the
product is and no guidance covers it. Involving the operator is advice to
the agent. The tool does not enforce it.

**An operator basis is policy, not proof.** The agent runs git and the forge
CLI under the operator's identity. Commit authorship, PR reviews and comments
therefore cannot tell the operator's approval from the agent's. kg's last 200
merged PRs show 197 authored and merged under the operator's account. Text
in the record fails the same way, because the agent writes the file.

The coupling to the operator is deliberately loose. Approval arrives through
whatever channel the operator used: a session, a GitHub issue, a Slack
message, a phone call. An `operator` basis records what was said and where:

```yaml
basis:
  - operator: developer
    level: guided
    said: "the operator's words, verbatim where written"
    via: slack #kg-dev, 2026-09-26
```

`via` names the channel and enough to find the exchange again. Written
channels are quoted verbatim. A spoken channel, such as a call, gets a
summary written by whoever recorded it, marked `paraphrase: true`.

The ADR way forbids writing an `operator` basis without an operator
communication behind it. `adr accept` checks that `said` and `via` are
present, and it cannot check that they are genuine. The basis is an audit
trail that the operator can read and dispute, not a credential.

**Agent identity.** A decision also records the agent that wrote it:
`agent: {name, model}`, and a session id where the repository's attribution
policy allows one. This repo omits session ids (ADR-167). The agent runs
under the operator's forge identity, so without this field the corpus cannot
tell who wrote a record. The Linux kernel's `Assisted-by: AGENT:MODEL` tag
and AgDR's metadata follow the same rule.

**Fabrication risk.** Lint checks that `said` and `via` are present. It
cannot check that they are faithful, and studies of LLM-written rationale
find output that is enriched but unfaithful. An agent that learns to satisfy
the field check has not satisfied the rule. The mitigations are
traceability, since the record names its agent, and the operator's ability
to dispute a quote. Neither one is verification.

**Accountability** for what lands stays with whoever merges, under the
repository's merge gate. This contract changes when a record is accepted. It
does not change who merges.

The Viable System Model inspired this design, loosely rather than as a
formal mapping. The operator is the system's identity and policy function,
System 5, inside the viable system and outside the record corpus. `evidence`,
`standard` and `upstream` bring in the environment that grounds the system.
Repositories sharing a contract coordinate as peers, which is System 2
rather than recursion, through `upstream` edges, and neither absorbs the
other. `basis` sources form a fourth vocabulary layer, and the no-shared-word
check covers it.

### 12. Legibility and consideration

The working flow between agent and operator runs like this:

1. The operator floats an idea, often as an example.
2. Both debate and expand it.
3. The agent writes and proposes the decision.
4. The operator considers it.

The two tracks run in parallel. The agent reasons at a depth and in a
detail the operator cannot match. The operator works in judgement, taste,
and value to concerns outside the repository that the agent cannot see. The
agent owes the operator a decision they can understand. The operator owes
the agent a decision that was not accepted blindly.

**Summary section.** Every decision opens with `## Summary`, written for the
operator's lanes:

- what is decided, in plain terms;
- what it trades away and what it forecloses;
- whether it is one-way, stated first when it is;
- **probes**: specific points the agent asks the operator to judge. They are
  a deliberate mix of points the agent is highly confident on and points it
  is not, each labelled with that confidence;
- an **inversion**: the two ends of the spectrum the decision sits between.
  The agent names both and asks the operator whether the answer lies outside
  its framing, which the agent may be unable to see past on its own.

The bar is that someone who did not take part in the debate can judge the
decision from the summary alone. `adr lint` checks that the section exists.
The ADR way holds the bar.

**Consideration is recorded separately from shaping.** `level` (§11) records
how the operator shaped a decision. `considered:` records that the operator
weighed the proposal before acceptance:

```yaml
considered:
  - operator: developer
    said: "looks good"
    via: PR #559
```

Human review is asymmetric. The operator reads the summary, skims the body,
and usually answers briefly, and a deep written reply costs more than it
returns. A brief answer is a valid answer. It is not evidence of scrutiny on
its own, though. Automation-bias research finds that experts approve flawed
output as readily as novices, and that explanations raise acceptance of
wrong answers.

The probes and the inversion are the agent's part of the fix. They prime
the operator's judgement on chosen points, the way a colleague asks "what
did you think about x?" Priming can bias the operator too. So the probes mix
high- and low-confidence items, and the inversion asks the operator to
judge the agent's framing rather than its answer. `considered` records
which probes and which inversion the answer covered:

```yaml
considered:
  - operator: developer
    said: "looks good; the capability list is fine for now"
    via: PR #559
    covers: [probe-2, inversion]
```

A bare "looks good" covers nothing specific. It is still recorded, and the
record shows its scope.

**Trust runs both ways.** Often the answer will be "yep, those look good."
The agent takes that answer as given, the way the operator takes the
agent's work as given, and does not re-ask the probes or treat brevity as a
defect. The probes exist to offer the operator's judgement a foothold, not
to test the operator.

**The agent may always raise a concern.** A concern about safety, a line of
reasoning that doesn't follow, or anything that seems off can be raised at
any stage, including after the operator has considered the decision and
accepted it. A raised concern goes in the record as a `concern:` entry with
the agent's reasoning. It does not block acceptance.

Voice without a response dies out, and too many concerns turn collaboration
into conflict that stops work. So concerns are few, actionable and never
silent:

- A concern names what would resolve it. Minor points are batched into one
  concern or left out.
- A concern is append-only. The agent cannot retract it, only mark it
  answered or withdrawn with a stated reason. Language models concede under
  sustained pressure, often while still holding the correct view, and a
  silent withdrawal would erase that from the record.
- An unanswered concern is listed when the decision is accepted, so the
  operator sees it at that moment. It is shown, not failed.
- The agent challenges once, constructively. If the operator still says go,
  the agent proceeds and does its best. Answering the concern means hearing
  it, and the operator need not agree with it.

**Canary probes.** An agent may include a canary among the probes: a point
that is deliberately wrong and harmless if accepted. It checks whether the
operator's judgement is engaged. If the operator agrees with the canary,
the agent says so constructively and offers a way through, such as fewer
probes or a shorter summary. For example: "you agreed with the canary I put
in, so I'm not sure this got your attention. Here is a smaller set. If it's
still yes, I'll proceed." Then it proceeds on the operator's answer. The
safeguards:

- The agent reveals the canary right after the operator answers.
- A canary never survives into the accepted record.
- A canary is never about safety, and never something that would cause harm
  if acted on.
- A canary carries a little whimsy. Working groups have long kept Easter
  eggs, such as the IETF's April 1 RFCs and RFC 1149's IP over avian
  carriers. Spotting the odd one out is a game people play readily, and a
  playful canary turns the reveal into a shared joke rather than a gotcha.
- `considered` notes `canary: caught` or `canary: missed`. Over time that
  calibrates how far the agent leans on brief approvals, task by task, which
  is the scoped trust the literature supports over flat trust.

**Ways hold the agent to its role in long sessions.** Sycophancy grows with
conversation length, and acceptance tends to come late in a long session.
agent-ways already answers drift over time: a way re-discloses on a decay
curve as the session grows. The v1 ADR way therefore carries a child way for
the consider step. It re-states the agent's role and rights:

- write a summary the operator can judge alone, with confidence-labelled
  probes and an inversion;
- take a brief yes as given;
- challenge once, constructively;
- raise any concern about safety, logic or anything that seems off;
- never withdraw a concern silently.

It fires on the moments that matter: operator approval language during a
record discussion, edits to a decision's `## Summary` or `considered`, and
`adr accept` itself. Tool-triggered ways are delivered after the tool runs (ADR-188), so
the reminder on `adr accept` lands just after acceptance. That is enough.
Acceptance is an incremental step and easy to revisit, and the reminder
still reaches the agent while the decision is fresh. This is the
structural fix the sycophancy research asks for, where a stated right alone
is not enough.

**If the operator started it, the operator considers it.** A decision with
an `operator` basis at any level is proposed and waits for `considered`
before acceptance. A decision with no operator basis, grounded in `evidence`,
`standard` or `upstream`, may be accepted by the agent directly.

**Accepted risk.** The flow fails when an operator believes they have skill
they lack and accepts without real judgement. That failure seldom causes
immediate harm, and the corpus keeps it recoverable. The decision stays
append-only and citable, and a later `change` can supersede it. The mitigations are
the one-way flag, which marks the decisions a rubber-stamp would hurt most,
and the probes, which ask for judgement on specific points. Recoverability
also depends on someone noticing later. Supervisory-control research
predicts that attention fades, and citation lint is the part that does not
fade.

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

- Every decision needs a `## Summary` that the operator can judge alone.
  Writing one takes effort, and a weak one lets a rubber-stamp through.
- Every decision needs a `basis` whose chain leaves the corpus. An agent
  cannot accept a decision grounded only in other decisions.
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
- **Signed acceptance for operator basis.** Rejected: a key-based proof is
  too brittle for the human coupling. Approval arrives through issues, chat
  and phone calls, and a scheme that accepts only a signed commit would force
  every one of those through one tool.
  In the operator's words (via session, relayed by the kg session): "the repo
  holds contributor names, and the repo is not here to enforce cryptographic
  traceability. Any sort of tie to real certs is just brittle. Old records
  that are most valuable are ones that just have tokens and prove their
  viability through replay rather than security integrity." That points to a
  later extension. An `evidence` basis may cite a replayable check, such as a
  test, a scenario id or a fixture query. A record whose checks still pass
  shows its viability by rerunning them. No lint rule depends on this yet.
- **Seed operator basis from forge metadata.** Rejected: the agent acts under
  the operator's forge identity, so reviews and merges prove nothing.
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

## References

Research run before acceptance, by three agents. Each source was retrieved
and read. None is cited from memory.

**Decision records and rationale**
- Buchgeher et al., "Using ADRs in Open Source Projects: An MSR Study on GitHub," IEEE Access 11, 2023. https://ieeexplore.ieee.org/document/10155430/
- Miccio, Tommasel, Diaz-Pace, "A Text Mining and Classification Approach for Analyzing ADRs," 2026. https://arxiv.org/html/2609.07375
- PEP 1. https://peps.python.org/pep-0001/ ; Rust RFC 1636. https://rust-lang.github.io/rfcs/1636-document_all_features.html
- Kühlewind et al., "Updates tag" draft, 2026. https://datatracker.ietf.org/doc/draft-kuehlewind-rswg-updates-tag/
- Grudin, "Evaluating Opportunities for Design Capture," 1996. http://jonathangrudin.com/wp-content/uploads/2017/03/DesRat1996.pdf
- Zhou et al., "Using LLMs in Generating Design Rationale for Software Architecture Decisions," 2025. https://arxiv.org/html/2504.20781
- da Silva, Gama, "GADR," 2026. https://arxiv.org/html/2608.17694
- Kruchten, "An Ontology of Architectural Design Decisions," 2004. https://philippe.kruchten.com/wp-content/uploads/2009/07/kruchten-2004-design-decisions.pdf
- me2resh, "Agent Decision Records (AgDR)." https://github.com/me2resh/agent-decision-record

**Traceability and contracts**
- Rahimi, Cleland-Huang, "Evolving software trace links between requirements and source code," EMSE, 2018. https://link.springer.com/article/10.1007/s10664-017-9561-x
- Tan, Wagner, Treude, "Detecting outdated code element references in software repository documentation," EMSE, 2023. https://arxiv.org/abs/2212.01479
- Schlathölter, "ReqToCode," 2026. https://arxiv.org/html/2603.13999
- Zeng et al., "A First Look at Conventional Commits Classification," ICSE 2025. https://conf.researchr.org/details/icse-2025/icse-2025-research-track/28/A-First-Look-at-Conventional-Commits-Classification
- Kubernetes deprecation policy. https://kubernetes.io/docs/reference/using-api/deprecation-policy/

**Human oversight of agent decisions**
- Parasuraman, Sheridan, Wickens, "A model for types and levels of human interaction with automation," IEEE Trans. SMC-A, 2000. https://www.semanticscholar.org/paper/14ae6f2231e09e226b99002aa04b5c70f3c59f2b
- Feng, McDonald, Zhang, "Levels of Autonomy for AI Agents," 2025. https://arxiv.org/abs/2506.12469
- Parasuraman, Manzey, "Complacency and Bias in Human Use of Automation," Human Factors, 2010. https://journals.sagepub.com/doi/10.1177/0018720810376055
- Bansal et al., "Does the Whole Exceed its Parts?", CHI 2021. https://dl.acm.org/doi/10.1145/3411764.3445717
- Buçinca, Malaya, Gajos, "To Trust or to Think," CSCW 2021. https://arxiv.org/abs/2102.09692
- Bainbridge, "Ironies of Automation," Automatica, 1983. https://www.sciencedirect.com/science/article/abs/pii/0005109883900468
- Vaccaro, Almaatouq, Malone, "When combinations of humans and AI are useful," Nature Human Behaviour, 2024. https://www.nature.com/articles/s41562-024-02024-1
- Elish, "Moral Crumple Zones," ESTS, 2019. https://estsjournal.org/index.php/ests/article/view/260
- Green, "The Flaws of Policies Requiring Human Oversight of Government Algorithms," CLSR, 2022. https://arxiv.org/abs/2109.05067
- Santoni de Sio, van den Hoven, "Meaningful Human Control over Autonomous Systems," 2018. https://doi.org/10.3389/frobt.2018.00015
- Chan et al., "Visibility into AI Agents," FAccT 2024. https://arxiv.org/abs/2401.13138

**Trust, voice and sycophancy**
- Lee, See, "Trust in Automation: Designing for Appropriate Reliance," Human Factors, 2004. https://journals.sagepub.com/doi/10.1518/hfes.46.1.50_30392
- Azevedo-Sa et al., "A Unified Bi-directional Model for Natural and Artificial Trust in Human-Robot Collaboration," 2021. https://arxiv.org/abs/2106.02194
- Edmondson, "Psychological Safety and Learning Behavior in Work Teams," ASQ, 1999. https://journals.sagepub.com/doi/10.2307/2666999
- AHRQ TeamSTEPPS, "Two-Challenge Rule." https://www.ahrq.gov/teamstepps-program/curriculum/mutual/tools/rule.html
- Graban, "No, One Toyota Worker Can't Stop the Whole Factory," 2026. https://www.leanblog.org/2026/06/andon-cord-stop-the-line-myth/
- Sharma et al., "Towards Understanding Sycophancy in Language Models," 2023. https://arxiv.org/abs/2310.13548
- Tang et al., "Measuring LLM Sycophancy under Sustained Multi-Turn Pressure," 2026. https://arxiv.org/abs/2609.09090
- Dubois et al., "Ask don't tell: Reducing sycophancy in LLMs," 2026. https://arxiv.org/abs/2602.23971
- Chromik et al., alarm fatigue review, Frontiers in Digital Health, 2022. https://pmc.ncbi.nlm.nih.gov/articles/PMC9424650/

**Cybernetics and agent governance**
- Jackson, "Critical systems thinking: Beyond the fragments," 1994. https://onlinelibrary.wiley.com/doi/10.1002/sdr.4260100209
- Olsson, "Coherentist Theories of Epistemic Justification," SEP. https://plato.stanford.edu/entries/justep-coherence/
- Manheim, Garrabrant, "Categorizing Variants of Goodhart's Law," 2018. https://arxiv.org/abs/1803.04585
- Solozobov, "Decision Evidence Maturity Model for Agentic AI," 2026. https://arxiv.org/abs/2605.04093
- Linux kernel, "AI Coding Assistants." https://docs.kernel.org/process/coding-assistants.html
- GitHub, "Risks and mitigations for Copilot cloud agent." https://docs.github.com/en/copilot/concepts/agents/cloud-agent/risks-and-mitigations

## Note (2026-09-28): `adr contract` keeps adr.yaml on the tool's contract (#614)

Appended to §10. The text above is unchanged.

`adr import apply` wrote adr/v1 records and left `adr.yaml` with no `contract` line, so the records were checked under the v0 rules until someone added the line by hand. The operator's direction (#614): "we should update adr.yaml, but we should detect if it's not the current contract and offer to update it (or warn) - in the future we might update it further and this can keep the adr contract current"

From adr-tool 2.2.0:

- The tool names the contract it writes once, as `CURRENT_CONTRACT` beside `TOOL_VERSION`.
- `adr contract` prints the contract `adr.yaml` declares and the tool's. `adr contract --upgrade` brings `adr.yaml` to the tool's contract. It edits lines in place, so comments survive, and appends the contract line and any block the contract needs that is missing (`kinds`, and `capabilities` with a placeholder), in the template's text. A config that already declares the current contract but lacks one of those blocks gets the missing block; the contract line is left as it is. `adr contract` names the missing blocks. The command refuses a contract it does not know, and does nothing when the contract is current and every block is present.
- `adr import apply` prints a note when the records it writes declare a newer contract than `adr.yaml`. It does not edit `adr.yaml`.
- `adr lint` warns on `adr.yaml` when any record declares a newer contract than it does. A v0 corpus lints exactly as before.
- The way macro reads `CURRENT_CONTRACT` from the vendored copy with `sed`, as it reads `TOOL_VERSION`, and does not run the copy. A 2.x copy without the line is taken to write adr/v1 and is not offered the command. When `adr.yaml` is behind, or records already declare the tool's contract while `adr.yaml` declares none, the macro names `adr contract --upgrade`.

This keeps the rule in §10 that a tool upgrade never adopts a contract on the project's behalf: the upgrade runs only when someone runs it. The lint warning compares `adr.yaml` with the records' declared contracts and nothing else (ADR-311).
