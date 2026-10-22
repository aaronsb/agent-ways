---
description: the Predict step of an SPE round, right after the user approves the plan; save the plan and a numeric prediction file and commit them before any harness work or trial, start from the same-as-last-round baseline, give each number a reason, and never edit a saved prediction after the run
vocabulary: approved plan prediction yaml saved before the run pre-registered commit before trials why line numbers not adjectives baseline-first same as last round naive guess configuration author date never edit adjusted after grader map
files: (^|/)spe/(predictions|plans)/
tools: ExitPlanMode
tool_phase: post
when:
  file_exists: spe/README.md
scope: agent
refire: frequent
---
<!-- epistemic: constraint -->
# SPE Predict

The approved plan is the prediction. It measures the judgment of whoever built the agent, so it has to exist before any result does.

## First actions after approval

Do these before writing any grader, task, or Workflow script.

1. Copy the approved plan to `spe/plans/rNN.md`. The plan mode file is overwritten each round, so this copy is the only lasting record.
2. Write `spe/predictions/rNN.yaml` from the plan's Predict table.
3. Commit both as `SPE rNN predict: ...` when the user has asked for commits. Otherwise save them and note the time.

A prediction written after the run is a description. If that happens, say so in the file and in the log.

## The prediction file

Start the file with the comment `# Saved before the eval ran. Do not edit after the run.` Use these fields.

| Field | Content |
|---|---|
| `round`, `date`, `author` | The author line reads "Claude, approved by the user in plan mode" unless the user wrote it |
| `plan` | The path to `spe/plans/rNN.md` |
| `changes_from_rNN` | The one change per configuration |
| `constants` | Trials per cell, caps, judge model |
| `configs` | One entry per configuration |
| Metrics | Pass rate, tool calls, tokens, minutes, and cost per task, per configuration |
| `band` | The error band for each number, such as 0.85 ± 0.05. A hit is a result inside the band |
| `why` | One line per number |
| `same_as_last_round` | Last round's measured values (round 2 on) |

## How to predict

- Start from "same as last round". Move a number only when a deliberate change gives a reason, and write that reason in the Why line.
- Predict every round, even when nothing changed.
- Give each predicted number the grader check that will measure it. A number with no check gets dropped or gets a check.
- Never edit `rNN.yaml` after a trial has run. When a grader bug forces a correction, the log shows both the original and the corrected comparison.

## Then the gates

After the commit, run the gates in order: the grader selftest, last round regraded as a control, and a dry run on 2 or 3 cells. Only then launch the full Workflow (evaluative/spe/evaluate).

## See Also

- evaluative/spe(evaluative) - parent: the loop and the plan sections
- evaluative/spe/evaluate(evaluative) - the harness that tests the prediction
