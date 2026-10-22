---
description: the learn step that closes an SPE round after the Workflow eval finishes; compare each prediction with the measured result and the naive same-as-last-round guess, find which reason broke, log new failure types, propose the next round, and keep a long unattended loop within its stop rules
vocabulary: close out prediction log verdict measured predicted naive closer which why broke failure types failures.md proposals next round draft prediction tag readme refresh unattended overnight halt report ceiling saturated
files: (^|/)spe/(prediction-log|failures|README)\.md
tools: Workflow
tool_phase: post
when:
  file_exists: spe/README.md
scope: agent
refire: frequent
---
<!-- epistemic: convention -->
# SPE Learn

The learn step closes the round when the Workflow finishes. A Workflow returns at launch, so finish this step when its completion notice arrives.

## The prediction log

Add a section for the round to `spe/prediction-log.md`. Use one row per metric and configuration, with the columns Metric, Config, Predicted, Measured, Same as last round, Verdict, and Closer.

- Verdict is "hit" when the measured value is inside the stated error, and "miss" when it is not.
- Closer names which was nearer, the prediction or the naive guess.
- Under the table, write "Which Why broke". For each miss, quote the Why line and say which assumption was wrong.
- If the naive guess beats the predictions, say so plainly. The predictions are not adding value yet.

## Failures

Add one entry per new failure type to `spe/failures.md`, numbered F1, F2, and so on across rounds. Read the highest F number in the file before you add one. Never leave a placeholder number. Each entry has these fields.

- **Name and kind** (harness, grader, task design, agent, environment) and the round.
- **What** happened.
- **How to spot it.**
- **Seen in**, with trial ids.
- **Fix**, or "open".

A grader bug found after the run is a failure entry. Disclose any regrade in the log and keep the original numbers next to the regraded ones.

## Seed the next round

- Add a "Proposed for round N+1" table at the end of the spec, with the columns ID, Proposed change, and Why (round N evidence).
- Write `spe/predictions/rNN-draft.yaml` marked `status: draft`, starting from this round's measured values.
- Update `spe/README.md` with the round's one-paragraph result and the rerun command.
- When commits were asked for, commit `SPE rNN learn: ...` and tag `spe-rNN`. The round's commits are `SPE rNN specify`, `SPE rNN predict` (before any harness work), `SPE rNN evaluate (harness)`, `SPE rNN evaluate (results): <headline>`, and `SPE rNN learn`.

## At a ceiling

If every config hits the metric's maximum, record each prediction as not scorable, leave the stop streak unchanged, and check once for a leak or a grader bug. Then tell the user in two or three sentences that the tasks no longer separate the configs, propose harder tasks or stricter graders, and let the user decide.

## Long unattended loops

When the user has said to keep going without them, run each round to the learn step, then respec on your own. Stop and report when any of these happens:

- Predictions match for 3 rounds.
- The stated round or budget cap is reached.
- A failure needs a decision only the user can make.

Never push, deploy, or send messages unless the user asked for that in this loop. The report gives each round's headline, the prediction accuracy trend, and the open decisions.

## See Also

- evaluative/spe(evaluative) - parent: the loop and stop rules
- evaluative/spe/specify(evaluative) - where the proposals become requirements
