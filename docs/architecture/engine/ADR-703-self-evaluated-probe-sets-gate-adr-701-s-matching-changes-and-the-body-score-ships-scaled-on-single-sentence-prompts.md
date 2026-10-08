---
contract: adr/v1
kind: decision
verb: change
capability: matching
basis:
  - operator: aaronsb
    level: directed
    said: "I'm hedging that we don't need to perform a deep eval on all items, we can sample some things and essentially develop a set of 'eigenvalues' (for lack of a better term) that shouldn't deeply affect the model's reasoning (it could be mostly based on the disclosure tree relationship, for example) but most importantly exercises the retrieval search flow"
    via: chat, 2026-10-08
  - operator: aaronsb
    level: directed
    said: "I think what we can do (since we are evaluating ourselves) is to make a best effort on this, we can use the container test model for the spot checks, and then ensure that our corpus is up to date with our eigendata."
    via: chat, 2026-10-08
  - operator: aaronsb
    level: directed
    said: "Scaled, single path only (Recommended)"
    via: chat choice, 2026-10-08, after the scaled body-score sweep
  - evidence: "experiments/content-corpus/results-probes.md"
  - evidence: "experiments/content-corpus/results-body-rank.md"
  - precedent: ADR-701
agent:
  name: claude
  model: claude-opus-5-5
status: proposed
date: 2026-10-08
deciders:
  - aaronsb
amends: "ADR-701#Decision"
related:
  - ADR-700
  - ADR-701
---

# ADR-703: Self-evaluated probe sets gate ADR-701's matching changes, and the body score ships scaled on prompts that chunk to one piece

## Summary

- **Decided:** a matching change under ADR-701 is adopted on a self-evaluation: probe sets derived from the per-way golden prompts by position in the disclosure tree, scored through the live scan with the judge off, plus unrelated prompts for separation. The real-prompt store becomes optional. Under that gate the body score ships as `(alias + 0.25 × best section) / 1.25` on single-sentence prompts, the single-vector path, as the default; multi-sentence prompts keep alias scores.
- **Trades away:** evidence from real user prompts. The probes are written by the corpus authors, so they show that retrieval follows the authored structure, not that real prompts route well.
- **One-way?** No. The gate is a procedure, and the body score is a config value (`matching.body_rank`) with `off` still selectable.
- **Probes:** *Confident (sampled-gate):* a small tree-sampled set that exercises the retrieval path is enough to decide matching changes for now. *Not confident (single-only):* leaving multi-sentence prompts on alias scores is acceptable until a variant gains there.
- **Inversion:** between gating on held-out real prompts, which the project does not collect, and adopting changes on reasoning alone. The decision gates on measured, reproducible self-evaluation.

## Context

ADR-701 §6 adopts the body score in ranking only after an evaluation on held-out real prompts, and §9 makes the golden-prompt harness the standing evaluation with a real-prompt store when available. No real-prompt store exists, and collecting one reads session transcripts.

The golden prompts now travel with the corpus (one direct and one situational prompt beside each semantic core way, checked by lint). From them, `ways author golden --probes` samples a fixed set by tree position (every root and parent, one hashed leaf per parent), and `--probes --joined` builds two-sentence probes that reach late interaction. `ways author probe` scores both through the live scan. Both sets are drift-checked in CI.

Measured on those sets (results-body-rank.md): the ADR-701 formula, `alias + 0.25 × best section`, raised every way's score above the scale the calibration was fitted on. It passed more one-chunk probes but made an unrelated prompt fire and tripled stray fires, and gained nothing on the late path. Dividing by `1 + w` keeps the alias scale. On prompts that chunk to one piece it moved pass from 81 to 86 of 130 and top-1 from 71 to 75, cut other ways fired from 456 to 433, and left every unrelated prompt below the bar. On the 79 joined prompts it moved pass from 55 to 54, losing six rows at body confirmation against the next-best section. With a short pleasantry in front of each prompt (the scan drops a fragment under 12 characters, so the prompt still chunks to one piece), the default moved pass from 76 to 86 of 130 against `off` and top-1 from 67 to 75, with no unrelated prompt firing.

## Decision

1. **Evaluation gate (amends ADR-701 §6 and §9).** A matching change is adopted when, on the committed probe sets (`tests/probes/tree-sample.tsv`, `tests/probes/tree-sample-joined.tsv`) and the unrelated prompts (`hooks/ways/golden-none.jsonl`, the `none` rows of `tests/routing-golden.tsv`, and their joined combinations), it gains on the path it changes with no loss in separation: no unrelated prompt starts firing. Results are written under `experiments/content-corpus/`. The real-prompt store is optional; when one exists, its results join the gate.
2. **Body score (amends ADR-701 §6).** On the single-vector path the ranking score is `(alias + w × best section) / (1 + w)` with `w = 0.25`, the default `matching.body_rank` mode. The late-interaction path keeps alias scores, and its body confirmation is unchanged. `off`, `on` (the original formula) and `scaled` (both paths) stay selectable for evaluation. Decision records carry the mode when a scan ranked on fused scores.
3. **The limit stated.** Results under this gate are reported as self-evaluated, with the probe counts, and are not presented as real-prompt quality.

## Consequences

### Positive

- Matching changes have a fast, reproducible gate that runs on any checkout with the local engine.
- Single-sentence prompts route better with the body text, with no new stray fires.

### Negative

- The gate can confirm structure the authors wrote while missing how real prompts are phrased.
- Two ranking rules exist, one per path; a reader of a decision record checks the mode.

### Neutral

- A variant that gains on the late path, such as a different confirmation rule, is evaluated under the same gate.
- The highest unrelated-prompt score rose from 0.437 to 0.461 against the 0.5 firing bar, so the separation margin fell from 0.063 to 0.039. A later change that raises scores again meets the gate with less room.
- The probes run without the assistant's last response. Live prompts often carry it and then take the late path, where the default changes nothing, so the one-chunk gain is an upper bound on the live effect. Decision records mark fused scans, so the event log measures how often it applies.

## Alternatives Considered

- **Wait for a real-prompt store.** Rejected for now: it needs transcript collection the project does not run, and blocks every matching change meanwhile.
- **Ship the ADR-701 formula unchanged.** Rejected: an unrelated prompt fired and stray fires tripled.
- **Scaled on both paths.** Simpler, but the late path lost a pass for eight fewer stray fires; the operator chose the single path only.
- **Confirm a one-section way against its lone section.** Measured worse on the late path (52 against 54).
