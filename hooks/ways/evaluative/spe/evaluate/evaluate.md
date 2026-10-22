---
description: the Evaluate step of an SPE round run as one dynamic Workflow; confirm the prediction is saved first, pass the harness gates and a dry run, keep answer keys out of trial reach, cap the budget, and grade with code checks before judges
vocabulary: workflow script harness gates selftest dry run args.only control regrade answer keys peek detection budget cap credit gate top-up split cells trials judge critic synthesis schema configurations same tasks same graders round constant
files: (^|/)spe/evals/
tools: Workflow
tool_phase: pre
when:
  file_exists: spe/README.md
scope: agent
refire: frequent
---
<!-- epistemic: convention -->
# SPE Evaluate

The eval runs the same tasks under each configuration. The tasks and graders stay fixed. The configuration changes.

## Before the Workflow call

- `spe/predictions/rNN.yaml` exists for this round and is committed. If not, stop and write it first (evaluative/spe/predict).
- The grader selftest passes. The oracle answer passes, and the empty and wrong answers fail.
- Last round's results, regraded with this round's graders, give a control. A change in the control is a grader change, not an agent change.
- A dry run on 2 or 3 cells with `args.only` returns valid output.

## The script

- Save it as `spe/evals/harness/rNN-workflow.js`. Copying last round's script is fine.
- Put the round in one `ROUND` constant or argument. Prompt text never names a round number by hand. Copied text like "say what round 2 should change" goes stale.
- Use phases such as Trials, Grade, Judge, Top-up, and Synthesis.
- Give every trial a schema for its result.
- Run code checks first. Use a judge only for what code cannot check. A judge from a different model family sees different blind spots, so note it when one is not available.
- Cells where only some trials passed get extra trials, up to a stated cap.
- End with a failure-analysis agent and a completeness critic.

## A new model

When a new model ships, keep the pinned model as the reference config and add the new model as a new config that changes only the model. Same tasks and graders. Do not switch production before the eval.

## Keep the answer keys hidden

Trial agents must not read graders, reference answers, or predictions. Committed files are visible in worktrees. So tell trials which paths they may read, scan their transcripts for reads of `spe/evals/` and `spe/predictions/`, and void any trial that peeked.

## Budget

Give every paid resource a cap in the arguments, such as credits, API spend, or agent count. Check the cap before each costly trial, and skip the trial when the cap would be crossed. Log what was skipped.

## Measure

Measure accuracy plus runtime, tool calls, tokens, and tool errors per trial. Count only completed trials. A harness error or rate limit is not a model failure.

## See Also

- evaluative/spe(evaluative) - parent: the loop and stop rules
- evaluative/agent-eval(evaluative) - trials, end state, and trajectory graders
- evaluative/held-out(evaluative) - cases scored only at the gate
