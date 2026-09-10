# Cypress Survey: What a Node-Routed Seed Teaches a Hook-Disclosed Corpus

A reading of [CYPRESS](https://github.com/llopresto87/Cypress) (Luigi Lopresto, MIT) against the ways corpus, taken 2026-09-09 at its 7.x line. The survey covers the method surface: postures, protocols, skills, delegation briefs, agents, and tooling. The harvested corpora (library, legal, tool, agent, skill) are project residue and were skipped.

## How the two systems relate

Cypress is structurally a ways corpus with the injection inverted. Every protocol, skill, agent charter, and posture file is a graph node carrying `load_when` phrases in frontmatter. A UserPromptSubmit hook tokenizes the prompt, matches it against those phrases with IDF weighting, and injects a suggestion naming the nodes to open. The model then reads them. Our hook injects the way body itself, and our matcher adds an embedding lane and a per-way decay curve (ADR-143, ADR-155). The routing layer therefore teaches us nothing. The method content underneath it is where the value sits, and roughly a third of it fills holes our corpus has.

Cypress also carries a heavy mandatory layer: a T0 to T3 tier classification spoken aloud before every task, spec-before-code, a fifteen-section plan-of-record document, a close-out librarian spawn per task, `spawn_id` tracing with depth caps, and a `docs/graph/` tree every project must host. All of it presumes an always-on kernel file and a parallel ledger. That conflicts with disclosure-on-trigger and with ADRs plus issues as the single ledger (ADR-180). None of it is imported.

The source prose is dense with em-dashes, contrastive negation, boosters, and maxim closers. Every rule below is a paraphrase; the wording lands in a way only after a rewrite to plain construction, and `scripts/check-register.sh` (ADR-178) is the gate.

## Verdicts

| Cypress source | Content | Verdict | Home |
|---|---|---|---|
| protocols/recover | Six failure classes, one allowed move each; three-attempt cap across all strategies; no-progress counts as a failed attempt; a gate red twice means the increment is wrong-sized; an intermittent defect is fixed only on mechanism evidence | adopt | new way `softwaredev/environment/recovery` |
| protocols/verify | Gate states executed / discovered / absent-with-reason; silence never implies a pass; report a zero only with a positive control the probe caught on the same run; assertion shape questions; self-expiring known-bug marker; the instrument is the first suspect | adopt | new way `softwaredev/code/testing/gates`, child `assertions` |
| protocols/verify | Golden-master oracle captured before a refactor or migration, landed as its own slice, diffed against an enumerated intended-delta list | adopt | addition to `softwaredev/freshness/groundtruth` |
| protocols/test-first | Characterize untested code first (`characterizes_` names); prove an inherited green suite by reintroducing the historical defect; test-level table, pick the lowest level | adopt | addition to `softwaredev/code/testing/tdd` and `testing` |
| skills/holistic-editing | Forbidden moves (`_v2`, `handleXNew`, boolean flags routing around old behavior); a purely additive diff must justify why nothing needed to die; dangling-import check; a rename fails the trivial test when the identifier crosses a wire or process boundary; the class sweep, one defect with six locations and the handed location is not privileged | adopt | new way `softwaredev/code/quality/integration` |
| integrations/bound-hook.py | PreToolUse guard refusing blocking-prone commands lacking a timeout prefix or a detached launch; pattern kills refused outright; exits only 0 or 2, internal failure exits 0 | adopt | hook `check-bash-bound.sh` plus new way `softwaredev/environment/bounded-execution` |
| protocols/toolcraft | Running is claimed only on an observed liveness signal; completion is the marker, never the absence of output or a timeout | adopt | same way as above |
| agents/security | Model output is never control flow; retrieved content is adversarial; test for injection, hijacking, exfiltration | adopt | extend `softwaredev/code/security/injection` |
| method/design-posture | A restrictive rule is designed against two sets, what it must catch and what must keep working; a silent fallback is a fail-open; never weaken a control to clear a symptom | adopt | new child under `softwaredev/code/security` |
| method/engineering-posture | Green on the authoring host is evidence about the authoring host; untracked or unpushed files do not exist for CI; report as "verified on the authoring host only" | adopt | new child `softwaredev/environment/hostparity` |
| method/contract-posture | Missing config fails loudly, a default added to silence it becomes a silent wrong value; reject rather than repair out-of-domain input | adopt | addition to `softwaredev/code/errors` |
| skills/validate-knowledge, clean-context brief | A fresh agent given only the entry point, required to declare what it loaded, asked an adversarial false-premise question; a linter you have only seen pass is untested | adopt | new way `documentation/validate`, step in the docs skill |
| agents/devils-advocate | Read-only hostile pass over a finished deliverable from primary sources only; closed verdict vocabulary: false, unsupported-as-written, overstated, stale, mis-cited, scope-error, could-not-refute | adopt, renamed | new agent `skeptic` |
| agents/pentest, skill-corpus | Four-part authorization gate (targets, environment, techniques, window and owner) in the conversation before any active request; remediation loop ending in a failing security test; PROVEN versus INFERRED; could-not-test section | adopt | new way `softwaredev/code/security/pentest`; report format in the supply-audit skill |
| templates/prompts/handback-payload | Return shape: status, failure class, work done with paths, out-of-domain needed, gates run, tools built | adopt | addition to `meta/subagents` and each agent file |
| method/delegation | A preference restated as a bound, or a bound as a preference, is a brief defect; leaves carry no spawn tool, one depth cap | adopt | addition to `meta/subagents` |
| templates/prompts/investigation-brief | Say "not found" rather than guess; every claim carries a path and an exact value; sample, never bulk-read; end with what was omitted | adopt | addition to `research` and `meta/subagents` |
| skills/brainstorm-socratic | One to three questions per turn; reflect every two answers; nine-question cap; eight-point convergence checklist | adopt | addition to the start and goal-author skills |
| protocols/deliver, canonize | Owner decisions as a numbered list; the four-question cold-pickup test; close-out categories: a sharp edge and its tell, a corrected assumption, a trigger that should have fired | adopt | addition to the wrap skill |
| skills/adr-writer | Reversibility tag that graduates at a milestone; doing nothing counts as a decision; an as-built observation does not; a deviation recorded with reason, scope, and an end condition | adopt | addition to `documentation/adr` |
| method/secrets-posture | Record a credential by name and location only, a partial mask leaks shape and prefix; committed means compromised; rotation order: signing keys, datastores, third parties | adopt | addition to `softwaredev/code/security/secrets` |
| method/incident-posture | Fix-forward default; out-of-scope work becomes a named residual with owner and reopen trigger; a hazard seen twice is a fact | adopt | addition to `itops/incident` and `softwaredev/delivery/issues` |
| method/release-posture | Promote the verified artifact by digest, rebuilding invalidates the gates; a rollback counts only once rehearsed; the irreversible step lands last | adopt | addition to `softwaredev/delivery/release` |
| method/prose-posture | Genre table (technical doc, ADR, brief, proposal: prioritize and avoid) | adopt | addition to `writing` |
| method/stewardship-posture | Synthetic test data only; masking is not anonymization | adopt | addition to `softwaredev/code/testing` |
| tools/prose-lint.py | "not just X but Y", "it's not X, it's Y", split-sentence contrast, chatbot residue, bold-label runs; fact drift check: numbers, headings, code spans, link targets survive an edit as identical multisets | adapt | rule classes in `scripts/check-register.sh` |
| agents/_routes.golden.tsv, agent-lint --eval | A committed task-to-expected-target file run in CI, failing on misroute | adapt | golden prompt-to-way file for ways-tests |
| integrations/status-hook.py | Session-start injection of open lifecycle counts | adapt | macro on `documentation/adr` counting Draft and Proposed ADRs |
| templates/prompt-contract, data-contract | Prompt as fenced code with tool permissions, output schema, refusal conditions, adversarial cases; data quality-check table, freshness cadence, privacy classification | deferred | candidate ways `softwaredev/code/prompts`, `data/contracts` |
| agents/reliability | Named runbook set and per-feature operability checklist | deferred | candidate child under `itops/runbooks` |
| agents/multi-agent-architect | Dormant-but-enabled anti-pattern: a component enabled in config that registers no hooks | deferred | one paragraph, home undecided |
| tiers, grill, specify, canonize spawn, grow, harvest, graft, graph-lint, growth-audit, agent router | Always-on kernel, parallel ledger, mandatory spawn ceremony | skip | |
| skills/humanizer | Third-party AI-tell detector | skip, `meta/trust/prose` covers it | |
| method/vcs-posture no-worktrees rule | Conflicts with `softwaredev/delivery/branching` and the harness | skip | |
| library, legal, tool, agent, skill corpora | Harvested project residue | skip | |

## On the name of the refutation agent

Cypress calls the role "devil's advocate". The phrase implies malice, and "contrarian" implies opposing for its own sake. The role withholds belief until a claim is shown in a primary source and closes with one of seven verdicts. `skeptic` names that stance.

## Macros

One candidate emerged, the ADR lifecycle count at session start. Cypress's delegation briefs and handbacks are static text and yield no other dynamic-context shape.

## What this note does not decide

Each adopt row is tracked as a GitHub issue (#461 to #476) that closes when the way, agent, hook, or skill edit lands. A new way's trigger vocabulary and re-disclosure curve are tuned at authoring time (see `meta/knowledge/authoring`). The deferred rows wait for a project that needs them.
