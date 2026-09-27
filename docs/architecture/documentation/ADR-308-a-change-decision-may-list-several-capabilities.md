---
contract: adr/v1
kind: decision
verb: change
capability: adr
amends: [ADR-304#3]
basis:
  - operator: aaronsb
    level: guided
    said: "Allow a multi-capability change"
    via: "session 2026-09-27, selected from agent-written options while converting agent-ways' records; the label was written by the agent"
  - evidence: "converting agent-ways' v0 records: about 20 of 92 alter several capabilities at once, e.g. ADR-123 unifies firing dynamics across attend, matching and disclosure, and ADR-174 spans disclosure, method and authoring"
  - precedent: ADR-305
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Accept, and warn on long lists"
    via: "session 2026-09-27, selected from agent-written options on PR #599; the operator chose the option that adds a lint warning past three capabilities"
    covers: [list-inflation]
status: accepted
date: 2026-09-27
deciders:
  - aaronsb
  - Claude
related:
  - 304
  - 305
  - 306
---

# ADR-308: A change decision may list several capabilities

## Summary

- **Decided:** a `change` decision may name several capabilities as a list, when it genuinely alters each of them. Each listed capability must have its own prior, as a single one does: a supersede or amend edge covering it, or the baseline standing in (ADR-305). `*` stays reserved for `constrain`.
- **Trades away:** the rule that one change touches one capability, which kept each change's blast radius obvious from its frontmatter.
- **One-way?** No. A later change can narrow the rule again. Records written with a list stay valid, since each listed capability is checked on its own.
- **Probes:** *Confident (list-matches-history):* the ~20 converted records that alter several capabilities describe them more truthfully as a list than under one main capability. *Not confident (list-inflation):* whether authors will list capabilities a change only brushes, so that a list becomes a way to hedge instead of a statement of what changed.
- **Inversion:** at one end a change names one capability, and a cross-cutting change is either split or misrepresented. At the other end, a change names whatever it touches, and capability no longer says where a decision's weight sits. This decision allows a list when each capability is altered, and checks each one.

## Context

ADR-304 §3 lets only `constrain` name several capabilities. Converting agent-ways' own records showed that about 20 of 92 alter more than one capability. ADR-123 unifies the firing dynamics that attend, matching and disclosure each carried. ADR-305 left ADR-123 on v0 for exactly this reason. Assigning each such record one main capability makes it pass lint, but it understates what the decision did.

## Decision

### 1. The amended rule

The ADR-304 §3 rule on capabilities now reads:

- A `change` decision's `capability` is one name, or a list of names when the decision alters each of them. A `constrain` decision's `capability` may be a list or `*`. Other verbs take one name.

### 2. Each listed capability is checked on its own

The ADR-304 §3 requirement that a change supersede or amend a prior decision applies to each listed capability separately. The ADR-305 baseline exemption also applies per capability.

### 3. What a list claims

A list claims that the decision alters each named capability. A capability the decision only mentions, or depends on without changing, belongs in `related`, not in the list. Lint warns when a change lists more than three capabilities, as a prompt to check that each one is altered.

## Consequences

### Positive

- Cross-cutting changes convert truthfully, ADR-123 among them.
- A query for decisions on a capability finds every change that altered it.

### Negative

- A change's frontmatter no longer shows a single place where its weight sits.
- Lint checks that each listed capability has a prior. It can't check that the decision really alters each one; the warning past three only prompts the check.

### Neutral

- `add`, `cut` and `retire` keep one capability. `constrain` is unchanged.

## Alternatives Considered

- **Main capability only.** The agent's recommendation. Each record carries the capability it mostly changes, and the rest appear through `related`. The operator chose the list, since it describes history more truthfully.
- **Split each cross-cutting record into one change per capability.** Faithful to the rule, but it rewrites history for imported records, and it multiplies records for a single decision.
