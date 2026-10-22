---
description: the SPE loop (Specify, Predict, Evaluate) run through Claude Code plan mode, where the user's plan-mode prompt is a lightweight spec, the approved plan is a numeric prediction, a dynamic Workflow runs the eval, and re-entering plan mode respecs the next round; keeps a long unattended loop honest round after round
vocabulary: spe specify predict evaluate respec round prediction spec plan-mode approved plan dynamic workflow eval rounds prediction log failures proposals draft baseline same-as-last-round naive variance converge pre-registered rnn tag
pattern: \bspe\b|specify[-,\s>]+(and\s+)?predict[-,\s>]+(and\s+)?evaluate
scope: agent
refire: normal
---
<!-- epistemic: convention -->
# The SPE Loop in Claude Code

SPE stands for Specify, Predict, Evaluate. The user's plan mode prompt is the spec. The approved plan is the prediction. One dynamic Workflow run is the eval. The user re-enters plan mode, and Claude respecs from the last round's evidence. One pass is one round.

## Scope

These rules apply when the user asks for an SPE step or names the loop. For any other request during a loop, such as a typo fix or a question, do that request and report only that. Do not mention rounds, predictions, evals, or SPE commit names, and do not touch spe/ files or a running Workflow. Commit it only if asked, with a plain message.

## Steps and files

| Step | Way | Moment | Files |
|---|---|---|---|
| Specify | evaluative/spe/specify | the respec, when a round is planned | `spe/spec/transcripts/<date>-rNN.md` (verbatim, never edited), `spe/spec/spec.md`, `spe/spec/disagreements.md` |
| Predict | evaluative/spe/predict | right after the plan is approved | `spe/plans/rNN.md`, `spe/predictions/rNN.yaml` |
| Evaluate | evaluative/spe/evaluate | when the Workflow launches | `spe/evals/harness/rNN-workflow.js` |
| Learn | evaluative/spe/learn | the close-out after the run | `spe/prediction-log.md`, `spe/failures.md`, `spe/README.md` |

## The plan is the prediction

- **Specify:** a requirement table. Each source is the user's words, a decision, or a failure ID like "r02 F7". Turn last round's proposals into rows. Adopt or reject each proposal with a reason. When running autonomously, record choices as defaults.
- **Predict:** one row per metric, one column per configuration, every cell a number, a one-line Why per row. From round 2 on, start from "same as last round" and move a number only for a stated reason. Give every number a band, such as 0.85 ± 0.05, and say a hit is inside the band. Name the grader check for each number. Change one thing per configuration.
- **Evaluate:** harness steps with gates, the Workflow arguments, and an unmeasured cost estimate.

## After approval, before anything else

Copy the plan to `spe/plans/rNN.md` and write `spe/predictions/rNN.yaml`, then commit both when the user asked for commits (evaluative/spe/predict). Only then run the gates and launch the Workflow. Never edit a saved prediction after a trial has run.

A Workflow returns at launch. Close out when its completion notice arrives (evaluative/spe/learn).

## Stop and halt

- Stop when predictions match results for 3 rounds in a row.
- At a ceiling, when every config hits the metric's maximum, record each prediction as not scorable, leave the stop streak unchanged, and check once for a leak or a grader bug. Then tell the user in two or three sentences that the tasks no longer separate the configs, propose harder tasks or stricter graders, and let the user decide.
- Halt and report on a budget cap, a leaked answer key, a broken grader, or a decision only the user can make.
- In an unattended loop, never push, deploy, or send messages unless the user asked for that in this loop. The final report gives each round's headline, the prediction accuracy trend, and open decisions.

## See Also

- evaluative(evaluative) - parent: the author does not touch the judge
- evaluative/ablation(evaluative) - arms with and without a change, graded blind
- evaluative/agent-eval(evaluative) - trials, graders, and the end state of an agent run
