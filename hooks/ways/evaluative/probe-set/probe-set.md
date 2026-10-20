---
description: a probe-set evaluative loop for a change to ranking, search, matching, routing or scoring that was judged by trying a handful of examples; score a fixed sampled set with a metric, adopt only on a gain with no loss in separation, keep an unrelated set that must not start matching, label results self-evaluated; error analysis by open and axial coding, standard errors with clustering and paired tests, interval width by set size, and pass rates corrected for judge error
vocabulary: tried a few queries handful of examples spot check eyeballed some results search ranking weights relevance fuzzy matching name matching routing rules classifier threshold recommendation retrieval sampled set probe set eval set unrelated inputs false positives top-1 recall precision margin separation sweep adopt self-evaluated metric grid variants disjoint row-for-row multi-part real-world error analysis open coding axial coding failure taxonomy criteria drift dead metric standard error confidence interval error bars clustered paired test power analysis judge error correction bootstrap
pattern: \b(open|axial) coding\b|error analysis\b|\berror bars\b|\bpower analysis\b|\bdead metric|clustered (standard )?errors|paired (t.?)?test
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Probe-Set Loop

The change alters a score: how a query ranks results, which handler a request routes to, what a classifier admits. No single output is right or wrong; the evidence is a metric over a set of cases. The loop scores a fixed set before and after, and adopts a change only on the result. The reading and statistics below hold for any score over a set of cases, model evals included.

## When it applies

A change to how results are ranked, matched, routed or scored, checked by trying a handful of examples by hand. The examples look better, but nobody knows whether other inputs got worse or whether things now match that should not.

## The sets

- **A sampled set** that exercises the paths the system takes, drawn by a rule (one case per category, a hash) and committed so every run scores the same rows. Regenerate it with the rule when the source changes, and check in CI that the committed copy matches.
- **Variants of the set** that reach other code paths: multi-part inputs, inputs with noise in front.
- **An unrelated set** of inputs that should match nothing. It measures separation: how far the strongest wrong score sits below the decision threshold.

## The adoption rule

A change is adopted when it gains on the path it changes and loses nothing in separation:

- The metric rises on the set that exercises the changed path.
- No unrelated input starts matching, and the margin below the threshold is reported. A gain that spends most of the margin is reported as such.
- Paths the change should not touch come out row-for-row identical. Check by diffing outputs, not by comparing totals.

Sweep a small grid of settings and report every point, including the ones that lost. A recommended default carries its one-line reason. When a change moves several metrics, report all of them, cost and latency included, and plot the score against cost for each variant.

## Read the failures before the score

- After each significant change, read 20 to 50 outputs by hand. Read some passing cases too, and look at what they left out.
- **Open coding:** write a free-text note on each failing case, with no fixed categories.
- **Axial coding:** group the notes into a failure taxonomy and count each group. Fix the largest groups first.
- In a multi-step trace, record only the first upstream failure. For a pipeline, label each step and compute accuracy per step.
- In a first full pass, review at least 100 traces. Annotate the first 30 yourself before reading any agent's suggested labels. Stop when new traces show no new failure modes.
- Revisit the criteria after each pass, and re-label older cases when a definition changes.
- A metric that never moves across rounds is dead. Check that its grader measures what its rule says, then fix or drop it before the next round.

## Error bars

- Report each score with its standard error and a 95% interval: the mean plus or minus 1.96 standard errors.
- Cluster the standard errors when cases come in groups, such as several questions about one document. Clustered errors can be several times the naive ones.
- Compare variants with a paired test on the same cases. Report the mean difference, its interval, and the correlation between the variants.
- Choose the set size with a power analysis before the run.
- Near a 50% pass rate, the 95% interval is about plus or minus 14 points at 50 cases, 10 at 100, and 5 at 400. That is arithmetic from the standard error of a proportion. A smaller gap is unsettled. A finer metric, such as key-point coverage, narrows it.

## Correct for an imperfect judge

When a model judge grades the set, correct the raw pass rate with the judge's true positive rate (TPR) and true negative rate (TNR) from its test labels: theta = (p_obs + TNR - 1) / (TPR + TNR - 1). Clip theta to the range 0 to 1, and report a bootstrap interval. The correction fails when TPR + TNR is near 1.

## Honest labels

The sets are written by the people who built the system, so a pass shows the system follows its authored structure. Label results self-evaluated, with the row counts, and do not present them as real-world quality. When a store of real inputs exists, its results join the gate.

The fitting data for any tuned parameter is disjoint from every evaluation set. Name what was used.

## Record the result

Write each evaluation to a results file: the commands, the corpus or build it ran against, the table, and what was surprising. A variant that was measured and not adopted is recorded too, so the question is not reopened without new evidence.

## Common rationalizations

| Rationalization | Counter |
|---|---|
| "It went from 62% to 68%, so it helped." | On 50 cases that gap is inside the interval. Call it unsettled. |
| "The numbers tell the story." | Read the failures. Otherwise you do not know whether the graders work. |
| "The metric held steady, so nothing broke." | Check that it can move at all. A dead metric holds steady too. |

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- code/testing/evaluation(softwaredev) — pass counts carry their denominator
- delivery/groundwork/claims(softwaredev) — a reported gain is a claim until someone else has run it
