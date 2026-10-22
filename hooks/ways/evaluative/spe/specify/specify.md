---
description: the Specify step of an SPE round; record the user's prompt verbatim, write a requirement table where each row cites its source and names its test, list disagreements with an owner, and turn last round's proposals and failures into this round's requirements
vocabulary: requirement table source test column transcript verbatim disagreements owner decides open decision proposals adopted rejected autonomous defaults testability two reviewers failure id cite respec revision history
files: (^|/)spe/spec/
tools: EnterPlanMode
when:
  file_exists: spe/README.md
scope: agent
refire: frequent
---
<!-- epistemic: convention -->
# SPE Specify

The spec says what the work must do and what counts as done right. In Claude Code the user's plan mode prompt is the spec, often in one or two sentences. Claude turns it into testable requirements.

## Keep the raw record

Save the prompt and every planning answer word for word in `spe/spec/transcripts/<date>-rNN.md`. Add any plan review corrections the user made. Never edit this file later. Every other document cites it.

## The requirement table

`spe/spec/spec.md` holds one table per round with the columns ID, Requirement, Source, and Test.

- IDs keep counting across rounds (R1, R2, then R10 in round 2). Retired rows stay with a note.
- The source is the user's words, a named decision, or a failure ID such as "r02 F7".
- The test names the task or grader check that measures the row.
- If two senior reviewers could not agree on pass or fail, the requirement is not specific enough yet.

## Disagreements

List each open decision on its own row in `spe/spec/disagreements.md` with the columns ID, Question, Option A, Option B, Work affected, Decides, and Status. Do not blend two sides into one requirement. When the user says to run autonomously, pick a default, mark it as a default, and give the reason. The user can override it next round.

## The respec

From round 2 on, the last learn step left a "Proposed for round N" table and a draft prediction.

1. Read the prediction log, the failures, and the proposals before planning.
2. Adopt or reject each proposal. Log each rejection with its reason in the round's defaults table.
3. Each failure that changed the plan becomes a requirement that cites the failure ID.
4. Change one thing per configuration, so the next eval can say what caused a difference.
5. Update the round number everywhere: file names, commit subjects, and the text of copied prompts.

## See Also

- evaluative/spe(evaluative) - parent: the loop and the plan sections
- evaluative/spe/learn(evaluative) - where the proposals come from
