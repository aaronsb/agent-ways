---
contract: adr/v1
kind: decision
verb: add
capability: method
basis:
  - operator: aaronsb
    level: directed
    said: "I'd like to consider a new branch of ways - it's not exactly software dev more about agent driven development approaches. I like to maybe call it 'evaluative engineering' or 'evaluative loop engineering' - where coding agents author, and we use known test harness and test approaches to act as part of the 'greater loop' for development."
    via: chat, 2026-10-09
  - operator: aaronsb
    level: directed
    said: "evaluative (Recommended)"
    via: chat choice, 2026-10-09, domain directory name
  - operator: aaronsb
    level: directed
    said: "Sit beside, link (Recommended)"
    via: chat choice, 2026-10-09, placement relative to softwaredev/code/testing/evaluation
  - operator: aaronsb
    level: directed
    said: "let's get all these authored, linted and scored, ensure the corpus builds, and we can merge in one pr then cut a new release"
    via: chat, 2026-10-09, relayed by the coordinating session
  - evidence: "aaronsb/broke-flats: .claude/skills/verify-game/SKILL.md, scripts/smoke.mjs:1546, commits 1725e14, 7b2ddac, 2af37b0"
  - evidence: "aaronsb/kwin-canvas: dev/nest.sh:344, effect/contents/ui/main.qml:1708, tests/lib.sh:21, commit 3dd5b82, issue #12"
  - evidence: "aaronsb/view1108: CLAUDE.md:705-733, tools/gate.sh, tools/imgdiff.py, commits 1365aab, 8e096bf, 3085a14"
  - upstream: "https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents"
  - upstream: "https://www.anthropic.com/engineering/infrastructure-noise"
  - upstream: "https://platform.claude.com/docs/en/test-and-evaluate/develop-tests"
  - precedent: ADR-604
  - precedent: ADR-703
  - precedent: ADR-508
agent:
  name: claude
  model: claude-opus-5-5
status: proposed
date: 2026-10-09
deciders:
  - aaronsb
related:
  - ADR-604
  - ADR-703
  - ADR-508
  - ADR-186
---

# ADR-605: An evaluative ways domain: the agent authors, the harness and oracle judge

## Summary

- **Decided:** a new top-level ways domain, `evaluative`, for work where a coding agent writes the artifact and instruments it does not control decide whether the work is done. A root way states the roles and a shared core, and routes by the kind of output to nine loop types: perceptual, sandboxed, clean-room, fidelity, probe-set, rubric, agent-eval, forecast and model-oracle. The domain sits beside `softwaredev/code/testing` and links to it; no existing way moves.
- **Trades away:** one home for evaluation guidance. Test-writing mechanics stay under `softwaredev/code/testing`, so a reader follows links between two domains, and the two can drift. Ten prompt-matched ways also join routing.
- **One-way?** No. A domain is a directory; it can be disabled per user or project with `disabled_domains`, renamed, or folded into another domain.
- **Probes:** *Confident (loop-types):* you wanted agents given the variety of loops your projects use to pick from; children organised by loop type serve that. *Not confident (operator-baselines):* several loops send every baseline or oracle change to the operator for approval, solo projects included; is that the default you want?
- **Inversion:** between leaving the method implicit in each project's own skills and harness, and encoding it as guidance every agent receives. The decision encodes the roles, the shared failure modes, and a catalogue of loop shapes, and leaves each project's instruments to the project.

## Context

Projects built largely by coding agents close the development loop in different ways, chosen by what the product emits and what can judge it. One drives an interactive program headlessly and reads screenshots. One runs the real platform in a nested, isolated instance and reads a text state dump. One compares a reconstruction against surviving historical output and a frozen byte-exact baseline. This repository scores ranking changes on a sampled probe set (ADR-703), scores model behaviour with rubrics in its live fixture, and installs itself from zero in a container before each release (ADR-186, ADR-508).

Each loop failed in the same few ways. A smoke harness exited zero whatever it found, so its verdict existed only as text the agent read. Commit messages reported pass counts no saved log supported. A capture script mixed two output streams, so the diff never saw the numbers it compared. A check script that did not parse dropped its rubric while the run reported a full pass. A nested test session leaked into the operator's live desktop.

Published guidance on evaluating agents adds a loop none of these projects had written down: tasks with reference solutions, isolated trials, graders on the end state, repeated trials reported as pass@k or pass^k, and transcripts read to check the graders.

The corpus covers how to write a test (`softwaredev/code/testing` and its children), how to report gates, and how to read an agent's report as a claim (`delivery/groundwork/claims`). It does not cover who authors and who judges, or which loop fits which product. ADR-604 has ways defer to external enforcement; this domain applies that to the loop that judges an agent's own work.

## Decision

1. A top-level domain `hooks/ways/evaluative/` holds the method. It is not a child of `softwaredev`: the method applies to any artifact an agent authors and an instrument can judge, including models and analyses.
2. The root way, `evaluative`, names three roles: the author (the agent), the instruments (harness, drivers, readouts), and the oracle (what the output is compared against). It carries the core every loop shares:
   - the author never writes, weakens, or regenerates the oracle that judges its own change;
   - the verdict is the exit status;
   - gates skip or fail loudly;
   - a cited count comes from a run in this change;
   - the instruments are tested;
   - the harness grows with each feature and each escaped defect.
   It ends with a table that routes to a loop type by what the output is and what can judge it.
3. Each child is a loop type, one per kind of output and judge:
   - `perceptual`: a running interactive program, driven headlessly and looked at.
   - `sandboxed`: behaviour inside a platform, observed in an isolated nested instance with ground truth read by a separate route.
   - `clean-room`: the install path, proven from zero in a fresh disposable environment seeded with a user's existing configuration.
   - `fidelity`: an external answer key, with a frozen byte-exact regression baseline kept separate from fidelity argued with numbers.
   - `probe-set`: a ranking or scoring change, adopted only on a metric gain with no loss in separation, labelled self-evaluated.
   - `rubric`: a single model answer, judged by deterministic checks and a rubric whose items are themselves tested.
   - `agent-eval`: a multi-turn, tool-using agent, judged on its end state and trajectory over repeated isolated trials.
   - `forecast`: a predictive model of time-ordered data, judged on a frozen held-out future window against a naive baseline.
   - `model-oracle`: a model of normal behaviour used as the judge where no fixed expected value exists, by the residual against a threshold set before the change.
4. The existing testing, gates, claims, host-parity, recovery, container-safety and bounded-execution ways stay where they are. The new ways link to them and do not restate them.

## Consequences

### Positive

- An agent working on a project with a harness receives the author-and-judge core whether or not the project wrote it down, and a catalogue of loop shapes to choose from.
- The failure modes seen in practice (a verdict nothing enforces, a count no run supports, an instrument with a blind spot, a harness that harms the host, an oracle the author changed) each have a named rule.
- A project that does not work this way disables one domain.

### Negative

- Guidance on evaluation now spans two domains, and a change to one can leave the other stale.
- Ten more prompt-matched ways compete in routing. Their vocabulary has to stay apart from the testing ways' and from each other's.

### Neutral

- The tree-sampled probe sets gain rows for the new domain and are regenerated with it.
- A later decision may move `softwaredev/code/testing/evaluation` under this domain once both have been used.
- New loop types join as children of the root and as rows in its routing table.

## Alternatives Considered

- **Children by cross-cutting principle** (observability, oracle, harness, readback, growth). Drafted first and dropped: the principles apply to every loop and fit in the root, while the useful choice for an agent is which kind of loop to build.
- **A child of `softwaredev/code/testing`.** Rejected: the operator placed the method outside software development, and the testing subtree is about writing tests, not about separating the author from the judge.
- **Reparent `softwaredev/code/testing/evaluation` into the new domain.** Rejected for now by the operator's choice: it renames way ids and moves matching data for a gain in tidiness.
- **Project-level skills only.** Each project already encodes parts of this in its own skills. Rejected as the only carrier: the failure modes recur across projects, and a project learns them only after hitting them.
