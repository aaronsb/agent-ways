---
description: a fidelity evaluative loop for a reconstruction, port or reimplementation with an external answer key such as surviving reference output, original listings, published tables or measured data; keep a frozen byte-exact regression baseline separate from fidelity argued against the reference with numbers, and route baseline deltas to the operator
vocabulary: reconstruction reimplementation port emulation historical original surviving output reference reference output answer key published tables measured data listing film fidelity faithful byte-exact bit-exact baseline frozen numerics hex double precision tolerance rms residual fit diff queue auto-pass approved delta magnitude dual implementation cross-check fallback native wasm second implementation
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Fidelity Loop

The target is something that already exists elsewhere: the output of a lost program, a published table, a measured trajectory, an older implementation's results. That external reference is the answer key. Two questions get two separate instruments: did this change move anything (regression), and is the output faithful to the reference (fidelity).

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
