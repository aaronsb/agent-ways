---
description: building the case set an eval scores, before any held-out set exists; success criteria written as numbers against a baseline, failures read by hand and grouped by open and axial coding into a failure taxonomy, a first set drawn from those failures with edge and should-not cases, two-step synthetic cases from dimension tuples, versioning, and benchmark contamination
vocabulary: success criteria eval cases eval dataset golden set observed failures error analysis open coding axial coding failure taxonomy first upstream failure accuracy per step re-label criteria definition edge cases should-not cases ambiguous inputs harmful inputs two domain experts same verdict synthetic cases tuples persona feature scenario dimensions version the set public benchmark training data canary strings relabelled answer options
pattern: \b(open|axial) coding\b|\bfailure taxonomy\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Building the Case Set

`evaluative/held-out` says who writes held-out cases and who may see them. This way covers the set the author builds and tunes against: what it targets, where its cases come from, and how it stays trustworthy. It applies to the cases of every loop type.

## When it applies

An eval is being set up from nothing, or its cases were made up on the spot: no stated target, a few happy-path examples, and no record of which failures the cases came from.

## Criteria before cases

- Write each success criterion as a number on a named set. "Safe outputs" becomes "fewer than 1 in 1,000 outputs flagged, over the full set".
- State the target as a change from a baseline measured on the current version.
- Cover each dimension that can fail: task fidelity, consistency, relevance, tone, privacy, use of context, latency, and cost. Say which errors are minor, which are egregious, and what share of minor errors is allowed.

## Read the failures before the score

Cases and criteria come from failures someone has read. Read again after each significant change.

- Run the task and read the outputs by hand. Read some passing cases too, and look at what they left out.
- **Open coding:** write a free-text note on each failing case, with no fixed categories.
- **Axial coding:** group the notes into a failure taxonomy and count each group. Fix the largest groups first.
- In a multi-step trace, record only the first upstream failure. For a pipeline, label each step and compute accuracy per step.
- Keep reading until new outputs show no new failure modes.
- Revisit the criteria after each pass, and re-label older cases when a definition changes.

## The first self-written set

- Run the task with no change and write down each failure in plain words. Those failures and the manual checks you already make become the first cases. Twenty to fifty real cases is enough to start.
- Add edge cases on purpose: missing, irrelevant, very long, harmful, and ambiguous inputs. Include cases where the behaviour should not happen. A set of positive cases alone rewards a subject that always acts.
- Write each case so two domain experts would reach the same verdict on their own. Shape the expected answer so code can check it: a choice, a string, a file state, or a list of key points.
- Tag each case by kind so results can be split later.

## Synthetic cases

1. Name the dimensions, such as feature, scenario, and persona.
2. Write about 20 tuples by hand.
3. Have a model generate more tuples. Turn each tuple into a query in a separate prompt. The separate step avoids repetitive phrasing.
4. Run the queries through the real system and read the traces.

The generating model sees the same one-sentence statement a held-out writer sees. Synthetic cases cannot show how common a failure is in real use, so compare them with real inputs once you have some. A person reviews model-written cases before they count.

## Versions and contamination

- Version the set each time it changes. A set that a script builds from sources is never edited by hand.
- Treat public benchmark tasks as possibly present in a model's training data. Prefer private cases drawn from your own failures. Canary strings do not reliably keep a task out of training data.
- Check that a format change, such as relabelled answer options, does not move the score.

## See Also

- evaluative/held-out(evaluative) — parent: cases from a writer who never saw the subject, scored only at the gate
- evaluative/agent-eval(evaluative) — tasks grown from real failures, the capability and regression suites
- evaluative/statistics(evaluative) — how many cases a gap of a given size needs
- evaluative/rubric/judge(evaluative) — labelled outputs that validate a model judge
