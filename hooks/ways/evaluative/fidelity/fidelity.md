---
description: a fidelity evaluative loop for a port, rewrite or replacement that must give exactly the same results as the old system or a reference such as a spec's examples or published output; a frozen byte-exact baseline for regression kept separate from fidelity argued against the reference with numbers
vocabulary: port rewrite replacement replacing the old system legacy system must give exactly the same results same totals identical output matches the old one to the cent spec examples test vectors reference output answer key reconstruction emulation byte-exact baseline frozen numerics tolerance diff queue auto-pass approved delta dual implementation cross-check
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Fidelity Loop

The target is something that already exists elsewhere: the output of a lost program, a published table, a measured trajectory, an older implementation's results. That external reference is the answer key. Two questions get two separate instruments: did this change move anything (regression), and is the output faithful to the reference (fidelity).

## When it applies

A port to another language, a rewrite, or a replacement for an old system, where the new version has to produce the same numbers, totals or output as the old one or as published examples. A few spot checks agree and the rest has not been compared.

## Regression: a frozen baseline, byte-exact

Capture the current approved output exactly: generated tables, numbers written as their bit patterns, rendered images, header values. A change either reproduces it or produces a delta. The baseline says nothing about whether the output is right. It records what was last approved, so any movement is seen.

- **Score image diffs, but never on pixels alone.** An auto-pass needs a pixel score above the threshold and identical numerics. A changed count or header value queues the case whatever its pixel score, because a tolerance on pixels can hide a numeric drift.
- **The agent reads the queue only.** Cases that auto-passed stay unopened. Each queued case is read, explained, and either fixed or proposed as a delta.
- **Deltas go to the operator.** A baseline moves only with the operator's approval, given against a report of what changed. Record each approved delta with its size ("moved 72 table values by at most 0.012 degrees") so the baseline's lineage can be audited.

## Fidelity: argued against the reference, with numbers

Fidelity is judged by measurement against the answer key: a fit of the output to reference marks with its residual, the predicted value against the published one with the difference. State the number and what remains unexplained. Every claim cites a held source or is labelled a guess. An anachronism or a convenience the original lacked is fenced and named.

A model that is wrong against the reference is fixed in the model. A patch at the symptom that makes one frame look right hides the fault and is removed once the model is fixed.

## Cross-check with a second implementation

Where the product runs through more than one path (a compiled build and a fallback, a native and a browser build, two readers of one format), run them over the same inputs and require agreement. Two implementations that disagree have found a defect without needing the answer key.

## Gate output for a long suite

Each gate prints one pass or fail line and writes its full log to disk. On failure it prints only the failing lines. Run a slice while iterating. Before a change is proposed, every gate runs once in full.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- code/testing/gates/assertions(softwaredev) — golden baselines and the enumerated-delta rule
- freshness/groundtruth(softwaredev) — the golden master captured before a migration
