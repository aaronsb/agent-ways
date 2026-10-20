---
description: Covers tuning a skill, helper script, or agent tool by having fresh agents answer real questions with it. Compute answer keys from the live source on the run day, accept defensible alternatives in rubrics, mine agent friction notes but verify each claim, move rules from prose into helper code, smoke-test every command after each edit, write a fresh holdout each round, and stop at the plateau.
vocabulary: skill-tuning tune-skill optimize-skill skill-optimization helper-script agent-tool friction-notes fresh-holdout answer-keys live-data key-drift smoke-test subagent-answers plateau skill-version old-version-control
pattern: (optimi[sz]e|tune|improve|harden)s? (the |this |our |my )?([a-z-]+ )?(skill|helper|integration|agent tool)s? (with|using|via|by|through) (evals?|agents?|subagents?|test questions)|evals? (on|for) how (to|we) (best )?use (the |our |this )?([a-z-]+ )?(skill|integration|helper|tool)|\bfriction notes\b|\bfresh agents? (answer|run|try)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Tuning a Skill with Agent Evals

## Set up the run

- Freeze the current skill as v0 before any change. It is the baseline and the rollback.
- Write 10 to 20 dev cases from real user questions. Include the traps you already know, such as a definition a naive query gets wrong.
- Compute each answer key yourself with direct queries to the source. Keep keys in the harness or grader prompts only, where the agent under test cannot read them.
- One fresh agent answers each case per variant, at least twice. A separate grader scores the answer against the key with a points rubric. Prove the grader first. A reference answer must pass and an empty answer must fail (code/testing/gates).
- Each answering agent returns its answer, its tool-call count, its failed calls, the queries it relied on, and specific friction notes about the skill.
- Take time, output tokens, context tokens, and tool calls from the transcripts. Self-reported counts drift.
- Tell answering agents where scratch files go. Otherwise they write query dumps into the skill folder.

## Live data drifts

- Records change during a run. In one run a deal closed mid-run, and a correct answer looked wrong against the morning key.
- Re-query a disputed number before you blame the variant.
- Give count and dollar keys a tolerance, such as 2% or two records, and date every key.
- Recompute the keys before you reuse cases on a later day.

## Grade fairly

- When a question has two defensible readings, the rubric accepts both as long as the answer says which one it used.
- Agents like to build their own cleaned cohort and lead with it. Score the standard figure first, and credit the cleaned view as a reconciled second line.

## Use the friction notes, then verify them

- Friction notes from answering agents are the richest source of fixes. Broken commands, stale docs, and missing fields show up there first.
- A claim about the data in a friction note is a hypothesis. Check it against the source before it enters the skill. In one run an agent said two category names were an old and new label for one category. The data showed both in use every year.
- Many independent, verified fixes can ship as one version. Run the older versions on the same fresh cases in that round, and split the change apart if the score drops.

## Fix in code before prose

- A rule that decides a number, such as an exclusion, a split, or a dedupe, belongs in the helper's query. Agents follow code every time and follow prose some of the time.
- After every helper edit, run a smoke test over every command and every flag value, including the defaults. A broken default once shipped because only a non-default flag was tested, and agents worked around it without saying so in their answers.

## Keep the holdout fresh

- A holdout set becomes seen once its failures or friction notes shape a change.
- Write a new holdout set after each version and before you read that version's results. Report the final score only on cases no version was tuned on.
- Run the older versions on the new holdout in the same round as controls.

## Stop at the plateau

- Stop when two versions score within noise on fresh cases. With 10 runs per arm, a 3-point gap is noise.
- Report errors per run, time, and tokens next to the score. A version that ties on score and cuts failed calls is the better version.
- Install the best version, keep v0 as the rollback, and log each round with its score, cost, and what changed.

## Common rationalizations

| Rationalization | Counter |
|---|---|
| "The agent said the field means X, so I'll document it." | Query the source. Agents guess under pressure. |
| "It scored 99% on the holdout." | Check whether that holdout's friction notes shaped the version. If they did, it is seen. |
| "The new version got one wrong that the old one got right." | Re-query the key. The data may have moved. |
| "The helper ran fine when I tried it." | Run the smoke test. Your manual check covered one path. |
| "One more round will push it higher." | Two versions within noise on fresh cases means stop. |

## See Also

- evaluative/ablation(evaluative) — arms with and without a change, graded blind
- evaluative/agent-eval(evaluative) — repeated trials, transcripts, and graders
- evaluative/held-out(evaluative) — cases from a fresh writer, retired once seen
