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
  - operator: aaronsb
    level: directed
    said: "could we have a --whatif (--whatif is not default though, but running the adr tool with no commands should reveal commands including whatif)"
    via: "session 2026-09-28, the operator's own request; the agent chose to make it an alias of `--dry-run` on every command that has one"
  - operator: aaronsb
    level: guided
    said: "could we (waves hands around silicon valley son-of-anton style) run a text search on the adr corpus and suggest domains if there is a lot of adr content and almost no or very few domains?"
    via: "session 2026-09-28, the operator's question; it led to the agent's proposal below, which does not search text inside the tool"
  - operator: aaronsb
    level: guided
    said: "this is good: mechanistic identification that corpus is fat and domains are skinny, let the agent know that's probably not ideal. let's build this into this now. it could be re-used as part of normal operations too"
    via: "session 2026-09-28, accepting an agent-written proposal that the tool detect the shape and the agent suggest domains or capabilities; the proposal was written by the agent"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "1. yes. the core placeholder is the backstop, and the domains should be seeds 2. they could split. 3. I think it is highly variable based on the project 4. that sounds like a good starting threshhold"
    via: "Claude Code session 2026-09-28, answering the agent's four probe questions in order: (1) whether v0 domains are a better starting point than the core placeholder, (2) whether projects split seeds or keep them as coarse as the domains, (3) whether process stays limited to conventions or collects product records, (4) whether the default thresholds fit. Question wording is the agent's; only the reply is the operator's."
    covers: [domains-are-a-start, seeds-kept-as-is, process-absorbs, thresholds-fit]
status: accepted
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

- **Decided:** when `adr contract --upgrade` brings a v0 `adr.yaml` to adr/v1, it writes one capability per domain already declared, in place of the `core` placeholder. The seeds are a starting point that the operator, or the agent acting for them, may keep, split, rename or discard. The template also ships two capabilities every adopting project has: `process` and `adr`. The migration way teaches how to split the seeds, with two worked examples. The tool also counts records per domain and, under v1, per capability, and gives an advisory notice when a large corpus sits in too few of them; the agent reads the records and proposes a finer set, and the operator decides. `adr contract --upgrade` gains `--dry-run`, and every `--dry-run` also accepts `--whatif`.
- **Trades away:** a default vocabulary that is ready without editing. A project still writes its own capabilities; the seeds only save it from starting at `core`. The shape notice counts records and does not read them, so it can say a vocabulary is too coarse but not what it should be.
- **One-way?** No. The template, the upgrade command and the notice's thresholds can change again, and a project's vocabulary is its own to edit either way.
- **Probes:** *Confident (domains-are-a-start):* a project's v0 domains name the areas its decisions were filed under, so as seeds they are closer to its capabilities than `core`. *Not confident (seeds-kept-as-is):* whether projects split the seeds or keep them unchanged, leaving a vocabulary as coarse as the domains, which #616 found too coarse. *Not confident (process-absorbs):* whether `process` stays limited to conventions that belong to no product capability, or collects records that belong to one. *Not confident (thresholds-fit):* whether the notice's default thresholds flag the corpora that need a finer vocabulary without nagging ones that are fine.
- **Inversion:** at one end the template ships only a placeholder and every project writes its vocabulary from nothing. At the other it ships a generic set that fits one kind of project. This decision seeds from what the project already declares, and ships only the two capabilities that do not depend on what the project does.

## Context

The template in `adr.yaml.template` ships one capability, `core: The project's core behaviour (placeholder; replace)`. `adr contract --upgrade` (#615, for #614) writes the template's contract blocks into an `adr.yaml` that is behind, so a v0 project that upgrades gets that placeholder.

#616 reports knowledge-graph-system's migration. Its 9 v0 domains became 18 capabilities, because several domains held capabilities that decisions change independently. Domain and capability turned out to be orthogonal, and about a third of the records carry two or three capabilities. `process` caught conventions that apply across the codebase. The issue asked for a default set or a worked example.

The two vocabularies at hand overlap little. agent-ways' 6 domains became 13 capabilities, and they share about four in spirit with knowledge-graph-system's 18. A default set drawn from either would misfit the other.

#616 also noted that a capability with no accepted `add` might fail migration. ADR-311 removed that check, so this no longer applies.

Seeding helps a project whose domains already describe its areas. A project with many records and one or two domains, or one domain holding most of them, gets seeds as coarse as those domains, and nothing in the tool says so. `adr contract --upgrade` also writes `adr.yaml` with no way to see the result first.

## Decision

1. **Seed from domains.** When `adr.yaml` declares domains, `adr contract --upgrade` writes one capability per domain, with the name and description taken from the domain, in place of the `core` placeholder. The tool neither requires the seeds nor protects them: the operator, or the agent acting for them, may keep, split, rename or discard each one. The upgrade output says so. A project with no domains keeps the placeholder.
2. **Ship `process` and `adr`.** The template declares two capabilities beside the placeholder or the seeds:
   - `process`: conventions, documentation and the development process that belong to no product capability.
   - `adr`: decision records, their contract and the adr tool.

   The macro's statement that adopting v1 is a decision with `capability: adr` stays true in every project that adopts it.
3. **Teach the split.** The migration way gives the method: split a seed when decisions change its parts independently; a record may carry more than one capability (ADR-308); use `process` for conventions that apply across the codebase. #616's domain-to-capability table is the worked example, and agent-ways' vocabulary is a second example from a different kind of project.
4. **Notice a coarse vocabulary.** The tool counts records per domain and, under v1, per capability, and compares the counts against default thresholds. The defaults are: at least 40 records, with at most 2 domains in use or one domain holding at least 60% of them; for capabilities, one capability holding at least 50% of the records or at most 2 in use. The implementation may tune them. When a threshold is crossed, the tool gives an advisory notice in `adr contract` and `--upgrade`, in `adr domains`, as a lint warning under v1 only, and in the session macro. The notice says the vocabulary is probably too coarse for the corpus. The agent then reads the records and proposes domains or capabilities, and the operator decides. The tool applies nothing.
5. **`--whatif`.** `adr contract --upgrade` gains `--dry-run`, which prints what it would write and changes nothing. Every `--dry-run` flag also accepts `--whatif`. The usage text shown when the tool runs with no command says that any command that writes accepts it. Without the flag, commands behave as before.

## Consequences

### Positive

- An upgraded project starts from its own areas, not from `core`.
- Every adopting project can record the adoption itself, and its conventions, without first writing a capability for them.
- A corpus that has outgrown its vocabulary is flagged during normal work, not only at migration.
- An upgrade can be previewed before it writes.

### Negative

- A seed kept unchanged is as coarse as the domain it came from. Only the way's method and review push it finer.
- `process` can collect records that belong to a product capability.
- Counts are a proxy. The notice can fire on a corpus whose vocabulary is right, and stay quiet on one that has enough names that do not fit.

### Neutral

- Amends ADR-304 §4, which said `adr` is seeded in v1; the template now does this, and adds `process`.
- The implementation follows in a separate change to the tool, the template and the migration way. It settles what happens when a domain has the same name as `process` or `adr`.
- Records already written are unaffected.
- The shape notice adds a lint warning under v1 that compares the corpus with `adr.yaml`, next to the contract warning from #615. It checks no record's content, so it stays within ADR-311.

## Alternatives Considered

- **A generic default set in the template**, from #616's list (`jobs`, `deploy`, `backup`, `storage`, `auth`, `query`, `ingest`, `providers`, `cli`, `web`, `process`, and a slot for the core model). It fits a service project and gives a tooling project capabilities it does not have; agent-ways would have to delete most of it.
- **Keep the placeholder and add only a worked example.** Leaves every upgrade at `core` when the project's domains are already in the same file.
- **Cluster the records inside the tool and propose domains.** The tool is one vendored file that depends only on the standard library and PyYAML. Keyword clusters would need rewriting by the agent anyway, and the embedding engine is not available where the tool is vendored. Counting in the tool and proposing in the agent splits the work where each is reliable.

## Note (2026-09-28): §5 usage wording

§5 says the no-command usage text states that "any command that writes" accepts `--whatif`. `new`, `rename`, `index` and `domain add` write and have no dry-run, so the implementation (#619) says instead that a command shown with `[--dry-run]` also takes it as `--whatif`. That wording replaces §5's sentence. Found in the review of #619.
