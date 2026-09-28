---
contract: adr/v1
kind: decision
verb: change
capability: adr
supersedes:
  - ADR-305
amends: [ADR-304#1, ADR-304#5, ADR-304#6, ADR-304#11, ADR-304#12, ADR-308#2]
basis:
  - operator: aaronsb
    level: directed
    said: "git is always the backstop. we should never try to recreate anything that git does"
    via: session 2026-09-27, after reviewing the friction in #604
  - operator: aaronsb
    level: directed
    said: "we're really just recording decisions and responsibility in a ledger style in git tracked files"
    via: session 2026-09-27
  - operator: aaronsb
    level: directed
    said: "not trying to invent some sort of complex checks and balances"
    via: session 2026-09-27, the same message
  - operator: aaronsb
    level: guided
    said: "Audit all gates first"
    via: "session 2026-09-27, selected from agent-written options on where to draw the line; the label was written by the agent"
  - evidence: "an audit of every check in adr-tool 2.x: shape and reference checks cost little and keep the ledger readable; the policy gates (frozen history, capability add, change prior, precedent chain, vocabulary stems, Summary legibility, enactment inventories, accept's corpus re-lint) produced most of the configuration and review friction in #602 to #604"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: Aaron Bockelie
    said: "Holds"
    via: session 2026-09-27, selected from agent-written options when the probes were asked; the label was written by the agent
    covers: [shape-is-enough]
  - operator: aaronsb
    said: "Way teaches, review catches (Recommended)"
    via: session 2026-09-27, selected from agent-written options when the probes were asked; the label was written by the agent
    covers: [convention-holds]
status: accepted
date: 2026-09-27
deciders:
  - aaronsb
  - Claude
related:
  - 304
  - 305
  - 308
---

# ADR-311: The adr tool checks shape and references; git keeps the history

## Summary

- **Decided:** records are a ledger of decisions and who made them, kept in git-tracked files. The adr tool writes records, checks their shape, and checks that their references resolve. It enforces no policy about how decisions relate or whether a record changed; that is convention, taught by the ADR way, and git holds every version.
- **Trades away:** automatic detection of an accepted record edited in place, a capability with no `add`, a `change` with no prior, and a precedent chain that stays inside the corpus. A reviewer or a reader of git history finds those now.
- **One-way?** No. A removed check can come back as its own decision if its absence costs more than it did.
- **Probes:** *Confident (shape-is-enough):* the records stay readable and navigable with shape and reference checks alone, since those are what a reader relies on. *Not confident (convention-holds):* whether agents follow "correct by appending" and "a change names what it replaces" without a check, or drift once nothing flags it.
- **Inversion:** at one end the tool only formats files and nothing is checked. At the other it audits every record against the corpus and its history, and the checks need their own configuration and exceptions. This checks what a reader of one record needs: its fields are there, and its links go somewhere.

## Context

ADR-304 made records typed and added lint rules that enforce relationships: frozen decisions (§1, §6), enactment checked against the code (§5), an `add` for every capability, a prior for every `change`, precedent that reaches outside the corpus (§11), labelled probes (§12). ADR-305 added a `baseline` to excuse capabilities that predate adoption, and ADR-308 extended the prior rule to lists. Building the frozen check on moved records took three designs in #604, each adding configuration. An audit of the rules found the same pattern across the policy gates.

## Decision

1. **What the tool checks.** A record's fields and their values (kind, status, verb, capability in the vocabulary, a basis whose entries each name a source, agent, `imported`, `observable`), its required sections, leftover placeholders, `adr.yaml`'s own shape, and every reference: supersession pairs, `amends` sections, precedent, and `adr cite`'s citations.
2. **What it no longer checks.** Frozen records and `mutable_after_accept`; an `add` per capability and `baseline`; a prior per `change`, for one capability or a list; a precedent chain reaching outside; vocabulary stems across layers; Summary probe labels and inversion; enactment inventories, `surfaces`, and `cite`'s warnings on citations of a cut capability; a retire decision's `targets`. `enacted` and `targets` stay fields.
3. **Commands.** `adr accept` refuses a record that fails its own shape check, and does not lint the corpus. `adr set` refuses `status`, which the lifecycle commands own. `adr supersede` writes both sides on any record. `considered` stays a field; accept does not require it.
4. **Conventions.** Correct an accepted record by appending; record a change as a new decision that names what it replaces; quote the operator. The ADR way teaches these.
5. **Git is the backstop.** Earlier versions, and who changed them, are read with git.

## Consequences

### Positive

- The tool shrinks by several hundred lines, and `adr.yaml` loses `baseline`, `surfaces` and the per-kind mutable lists.
- A move or a correction never needs an exception in configuration.

### Negative

- An in-place edit or a missing `amends` goes unflagged until someone reads the record or its history.

### Neutral

- Supersedes ADR-305. ADR-308's allowance for a list of capabilities stands; its rule that each needs a prior goes.
- Records already written keep their fields; nothing needs rewriting.

## Alternatives Considered

- **Keep the gates as warnings.** Warnings that fire on legitimate history train readers to ignore them.
- **Check only the change under review against its base.** Accepted briefly on an unmerged branch; still a check that git makes unnecessary.
