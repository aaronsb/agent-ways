---
description: iterating on a skill's or prompt's instructions against example cases until each gives its expected output; run it on the examples, see which outputs are wrong, adjust the wording, run them again; one edit per round, kept or reverted against the last kept version, fresh cases for the final score, dated answer keys, and stop limits on rounds, cost and stagnation
vocabulary: run it on these examples see what fails adjust repeat until the examples pass sample inputs expected outputs labels they should get iterate on the skill tweak the wording round hill climb hillclimb keep or revert last kept version stagnation breaker cost cap fresh cases answer keys recompute optimizing agent edits only the target read-only harness
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Tuning Over Rounds

A prompt, skill, tool description or helper is revised round by round against a set of cases until it passes them. Each round is one ablation (`evaluative/ablation`) against the last kept version.

## When it applies

Someone is iterating on a prompt or skill until a set of example cases passes: tweak it, rerun, read the output, tweak again. With no rule for what is kept, the latest version wins whether or not it beat the one before, and the cases it was tuned on are the ones that report its score.

## One change per round

1. Freeze the starting version as the baseline. It runs as the control in every round.
2. Run the eval and store the baseline's score.
3. Make one change: a prompt section, a tool description, the model, the effort level, or one rule.
4. Rerun the same cases.
5. Keep the change if the paired difference clears the run-to-run spread. Otherwise revert to the last kept version.
6. Log the round with the version, score, cost, tokens, status (keep, discard, or crash), and one line on what changed.
7. Read the failing transcripts before the next change.

A round carries one change. Several fixes in one round are a treatment with several pieces: a gain cannot be credited to any one of them, and a drop has to be split anyway. When fixes must land together, attribute them with remove-one arms (`evaluative/ablation`) before keeping them.

Keep the loop's state in files on disk. The model keeps nothing between runs.

## Simpler wins a tie

When two versions score within the noise floor, keep the simpler one, and between equally simple ones the one with fewer failed calls and lower cost. Reject a tiny gain that adds complexity, and keep a change that removes material and holds the score. Add an evaluator step or more agents only when an arm shows a gain. Include a simple baseline arm, such as one model retried several times. On published coding benchmarks, simple baselines have matched elaborate agent designs at a fraction of the cost.

## Guard the loop

- The optimizing agent edits only the target. Cases, graders, answer keys, and the harness stay read-only. A grader fix is its own round, with its own selftest (`evaluative/agent-eval/grading`).
- The agent that made the change does not grade it. Use a fresh-context reviewer or a stop-hook script. Tell a reviewer to flag only gaps that affect correctness or the stated requirements.
- Every loop has a pass condition a script can check, a stagnation breaker (such as stopping after three rounds that repeat the same states or errors), a cost cap, and a maximum round count. It also stops when two versions score within the noise floor on fresh cases.
- Never loop on a subjective goal such as "improve the experience".

## Cases the rounds have seen

- Cases whose failures or friction notes shaped a version are seen. Tune on dev cases, write fresh cases before reading a round's results, and report the final score only on cases no version was tuned on (`evaluative/held-out`).
- Answer keys computed from live data drift. Date each key, give counts and totals a stated tolerance, re-query a disputed figure before blaming the variant, and recompute keys before reusing cases on a later day.
- A trial agent's friction notes (a broken command, a stale doc, a missing field) are hypotheses. Check each against the source before it changes the subject.
- Take time, tokens and tool-call counts from the transcripts, not from the agents' own reports.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It passes, so the change worked." | Run the baseline arm. It may pass too. |
| "One more round will get it." | Check the stagnation breaker and the cost cap. |
| "Fixing the grader is faster than fixing the prompt." | The loop never edits the harness. Fix graders in their own round. |

## See Also

- evaluative/ablation(evaluative) — parent: arms with and without a change, a blind judge, a positive control
- evaluative/held-out(evaluative) — the cases no round was tuned on
- evaluative/statistics(evaluative) — paired tests and when a gap is inside the noise
- evaluative/agent-eval/grading(evaluative) — the grader selftest and keeping graders out of reach
