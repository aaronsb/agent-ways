---
description: a probe-set evaluative loop for a ranking, routing, retrieval, search, recommendation or scoring change; score a sampled evaluation set with a metric, adopt only on a gain on the path the change touches with no loss in separation, keep an unrelated set that must not start firing, and label the results self-evaluated
vocabulary: ranking routing retrieval search recommendation classifier matcher scoring threshold calibration embedding cosine similarity top-1 recall precision pass rate margin separation sampled set probe set eval set held-out unrelated prompts false positive stray fires sweep variant mode default adopt self-evaluated metric gain regression row-for-row
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Probe-Set Loop

The change alters a score: how a query ranks results, which handler a request routes to, what a classifier admits. No single output is right or wrong; the evidence is a metric over a set of cases. The loop scores a fixed set before and after, and adopts a change only on the result.

## The sets

- **A sampled set** that exercises the paths the system takes, drawn by a rule (position in a tree, one case per category, a hash) and committed so every run scores the same rows. Regenerate it with the rule when the source changes, and check in CI that the committed copy matches.
- **Variants of the set** that reach other code paths: multi-part inputs, inputs with noise in front.
- **An unrelated set** of inputs that should match nothing. It measures separation: how far the strongest wrong score sits below the firing bar.

## The adoption rule

A change is adopted when it gains on the path it changes and loses nothing in separation:

- The metric rises on the set that exercises the changed path.
- No unrelated input starts firing, and the margin below the bar is reported. A gain that spends most of the margin is reported as such.
- Paths the change should not touch come out row-for-row identical. Check by diffing outputs, not by comparing totals.

Sweep a small grid of settings and report every point, including the ones that lost. A recommended default carries its one-line reason.

## Honest labels

The sets are written by the people who built the system, so a pass shows the system follows its authored structure. Label results self-evaluated, with the row counts, and do not present them as real-world quality. When a store of real inputs exists, its results join the gate.

The fitting data for any tuned parameter is disjoint from every evaluation set. Name what was used.

## Record the result

Write each evaluation to a results file: the commands, the corpus or build it ran against, the table, and what was surprising. A variant that was measured and not adopted is recorded too, so the question is not reopened without new evidence.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- code/testing/evaluation(softwaredev) — pass counts carry their denominator
- delivery/groundwork/claims(softwaredev) — a reported gain is a claim until someone else has run it
