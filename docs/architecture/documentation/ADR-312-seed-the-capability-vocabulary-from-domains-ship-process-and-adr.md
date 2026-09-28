---
contract: adr/v1
kind: decision
verb: change
capability: adr
amends: [ADR-304#4]
basis:
  - evidence: "issue #616: knowledge-graph-system's migration of 108 v0 records split its 9 v0 domains into 18 capabilities; domain and capability are orthogonal, about a third of the records carry 2 or 3 capabilities, and `process` caught codebase-wide conventions that belong to no product capability"
  - evidence: "a comparison of two vocabularies: agent-ways' 13 capabilities (docs/architecture/adr.yaml) and knowledge-graph-system's 18 (#616) share about four entries in spirit (cli, install/deploy, config, process/method); the rest are specific to each project"
  - operator: aaronsb
    level: guided
    said: "1. yes, but the seeds can be discarded at the operator (agent's) choice. 2. both 3. yes"
    via: "session 2026-09-28, after the operator said \"let's consider 616\"; a reply to three agent-written questions: (1) \"Seed capabilities from domains in `--upgrade` — yes/no, and in #615 or a follow-up?\" (2) \"Default capability name: `process`, `adr`, or both?\" (3) \"Draft the record?\". The questions and their option labels were written by the agent; the operator's words are only the reply"
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-09-28
deciders:
  - aaronsb
  - Claude
related:
  - 304
  - 308
  - 311
---

# ADR-312: Seed the capability vocabulary from domains; ship process and adr

## Summary

- **Decided:** when `adr contract --upgrade` brings a v0 `adr.yaml` to adr/v1, it writes one capability per domain already declared, in place of the `core` placeholder. The seeds are a starting point that the operator, or the agent acting for them, may keep, split, rename or discard. The template also ships two capabilities every adopting project has: `process` and `adr`. The migration way teaches how to split the seeds, with two worked examples.
- **Trades away:** a default vocabulary that is ready without editing. A project still writes its own capabilities; the seeds only save it from starting at `core`.
- **One-way?** No. The template and the upgrade command can change again, and a project's vocabulary is its own to edit either way.
- **Probes:** *Confident (domains-are-a-start):* a project's v0 domains name the areas its decisions were filed under, so as seeds they are closer to its capabilities than `core`. *Not confident (seeds-kept-as-is):* whether projects split the seeds or keep them unchanged, leaving a vocabulary as coarse as the domains, which #616 found too coarse. *Not confident (process-absorbs):* whether `process` stays limited to conventions that belong to no product capability, or collects records that belong to one.
- **Inversion:** at one end the template ships only a placeholder and every project writes its vocabulary from nothing. At the other it ships a generic set that fits one kind of project. This decision seeds from what the project already declares, and ships only the two capabilities that do not depend on what the project does.

## Context

The template in `adr.yaml.template` ships one capability, `core: The project's core behaviour (placeholder; replace)`. `adr contract --upgrade` (#615, for #614) writes the template's contract blocks into an `adr.yaml` that is behind, so a v0 project that upgrades gets that placeholder.

#616 reports knowledge-graph-system's migration. Its 9 v0 domains became 18 capabilities, because several domains held capabilities that decisions change independently. Domain and capability turned out to be orthogonal, and about a third of the records carry two or three capabilities. `process` caught conventions that apply across the codebase. The issue asked for a default set or a worked example.

The two vocabularies at hand overlap little. agent-ways' 6 domains became 13 capabilities, and they share about four in spirit with knowledge-graph-system's 18. A default set drawn from either would misfit the other.

#616 also noted that a capability with no accepted `add` might fail migration. ADR-311 removed that check, so this no longer applies.

## Decision

1. **Seed from domains.** When `adr.yaml` declares domains, `adr contract --upgrade` writes one capability per domain, with the name and description taken from the domain, in place of the `core` placeholder. The tool neither requires the seeds nor protects them: the operator, or the agent acting for them, may keep, split, rename or discard each one. The upgrade output says so. A project with no domains keeps the placeholder.
2. **Ship `process` and `adr`.** The template declares two capabilities beside the placeholder or the seeds:
   - `process`: conventions, documentation and the development process that belong to no product capability.
   - `adr`: decision records, their contract and the adr tool.

   The macro's statement that adopting v1 is a decision with `capability: adr` stays true in every project that adopts it.
3. **Teach the split.** The migration way gives the method: split a seed when decisions change its parts independently; a record may carry more than one capability (ADR-308); use `process` for conventions that apply across the codebase. #616's domain-to-capability table is the worked example, and agent-ways' vocabulary is a second example from a different kind of project.

## Consequences

### Positive

- An upgraded project starts from its own areas, not from `core`.
- Every adopting project can record the adoption itself, and its conventions, without first writing a capability for them.

### Negative

- A seed kept unchanged is as coarse as the domain it came from. Only the way's method and review push it finer.
- `process` can collect records that belong to a product capability.

### Neutral

- Amends ADR-304 §4, which said `adr` is seeded in v1; the template now does this, and adds `process`.
- The implementation follows in a separate change to the tool, the template and the migration way. It settles what happens when a domain has the same name as `process` or `adr`.
- Records already written are unaffected.

## Alternatives Considered

- **A generic default set in the template**, from #616's list (`jobs`, `deploy`, `backup`, `storage`, `auth`, `query`, `ingest`, `providers`, `cli`, `web`, `process`, and a slot for the core model). It fits a service project and gives a tooling project capabilities it does not have; agent-ways would have to delete most of it.
- **Keep the placeholder and add only a worked example.** Leaves every upgrade at `core` when the project's domains are already in the same file.
