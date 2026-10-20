---
description: an ablation evaluative loop for a change to what an agent is given, such as a rewritten system prompt, new instructions, a new tool or trimmed guidance, that seems better but hasn't been measured; compare arms with and without it, graded blind by an independent judge, with a positive control that the change engaged; hill-climbing one change per round with keep or revert, stop limits, a read-only harness, and a small regression suite gating changes in CI
vocabulary: rewrote the system prompt changed the instructions reworded the prompt added a tool trimmed the guidance seems smarter now feels better before and after did it actually help with and without ablation ablate arms baseline arm treatment arm blind judge shuffled order strip the injected text positive control remove-one leave-one-out attribute the effect noise floor three way comparison trials graders run-to-run anecdotal scenario hill climb hillclimb one change per round keep or revert iterate on the prompt stop condition stagnation cost cap max rounds self-grading regression suite ci gate eval threshold model release
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Ablation Loop

The change is to what an agent is given: a prompt, a skill, a tool, a block of injected guidance, a system prompt. The question is whether behaviour changes because of it. The loop runs the same scenarios in separate arms and has an independent judge compare them blind.

## When it applies

Someone changed what an agent is given, such as its system prompt, instructions, tools or guidance file, and it seems better since. The impression comes from a few conversations, with no comparison against the version without the change.

## Three roles

- **Baseline arm:** the agent without the change.
- **Treatment arm:** the agent with it, everything else identical.
- **Judge:** an independent model that grades both arms' results. It is not told which arm produced which, and the order it sees them in is shuffled per scenario.

Run each arm in its own isolated environment, so neither can read the other's leftovers. A disposable container per arm with its own credentials serves (see `evaluative/clean-room`).

## Keep the judge blind

Strip the change's own text from what the judge sees: injected guidance, hook output, tool descriptions the baseline never had. A judge that can see the treatment's guidance can tell the arms apart and is steered by the guidance's wording. Weight deterministic end-state checks first (the file exists, the command was not run, the test passes), then rubric dimensions, with "unknown" allowed. Graders and trials follow `evaluative/agent-eval`.

## Positive control

Confirm in every treatment trial that the change engaged: the guidance was loaded, the tool was called, the skill was invoked. A treatment trial where it did not engage is a baseline trial with a different label, and averaging it in dilutes the effect toward zero.

## Attribute with remove-one arms

When the treatment bundles several pieces, an arm with everything except one piece shows what that piece contributes. Spend remove-one arms on pieces whose effect shows in the end state, and sample which pieces to ablate rather than ablating each one.

## Trials, noise and cost

Run several trials per arm and scenario. Report trial counts, the model and version, and the resources each arm ran with. A difference smaller than the run-to-run spread between two identical arms is no finding; measure that spread once by running the baseline against itself.

Cost scales as scenarios × arms × trials. The loop runs locally and on demand when a context change is worth measuring, never on every merge.

## Iterate one change per round

When a change is tuned over several rounds, each round is one ablation against the last kept state.

1. Run the eval and store the baseline.
2. Make one change: a prompt section, a tool description, the model, the effort level, or one rule.
3. Rerun the same scenarios.
4. Keep the change if the paired difference clears the run-to-run spread. Otherwise revert to the last kept state.
5. Log the round with the version, score, cost, tokens, status (keep, discard, or crash), and one line on what changed.
6. Read the failing transcripts before the next change.

Keep the loop's state in files on disk. The model keeps nothing between runs.

When scores tie, the simpler version wins. Reject a tiny gain that adds complexity, and keep a change that removes material and holds the score. Add an evaluator step or more agents only when an arm shows a gain. Include a simple baseline arm, such as one model retried several times. On published coding benchmarks, simple baselines have matched elaborate agent designs at a fraction of the cost.

## Guard the loop

- The optimizing agent edits only the target. Scenarios, graders, answer keys, and the harness stay read-only. A grader fix is its own round, with its own selftest.
- Tune on dev scenarios, and report the final number only on a held-out set (`evaluative/held-out`).
- The agent that made the change does not grade it. Use a fresh-context reviewer or a stop-hook script. Tell a reviewer to flag only gaps that affect correctness or the stated requirements.
- Every loop has a pass condition a script can check, a stagnation breaker (such as stopping after three rounds that repeat the same states or errors), a cost cap, and a maximum round count.
- Never loop on a subjective goal such as "improve the experience". A person reads what the loop kept before it ships.

## The regression gate

A separate small suite, mostly deterministic, gates every prompt, model, or pipeline change in CI. Set its pass threshold explicitly against the production baseline. Some eval tools default to 1.0, which fails any imperfect case. Gate a user-facing agent on pass^k, and attach the results to the pull request. Add each new production failure to the suite, and rerun the full suite on each new model release before switching. Run expensive judges on sampled live traffic instead of in CI. Offline evals catch only part of what breaks, so pair them with production monitoring, user feedback, and transcript review.

## Common rationalizations

| Rationalization | Counter |
|---|---|
| "It passes, so the change worked." | Run the baseline arm. It may pass too. |
| "One more round will get it." | Check the stagnation breaker and the cost cap. |
| "Fixing the grader is faster than fixing the prompt." | The loop never edits the harness. Fix graders in their own round. |

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- evaluative/agent-eval(evaluative) — graders, trials, pass@k
- evaluative/clean-room(evaluative) — isolated, disposable environments per run
- evaluative/held-out(evaluative) — scenarios written by someone other than the author
