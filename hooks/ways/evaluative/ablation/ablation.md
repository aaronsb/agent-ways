---
description: an ablation evaluative loop for a change to an agent's context such as a prompt, skill, tool, injected guidance or system prompt; compare a baseline arm without it and a treatment arm with it, graded blind by an independent judge, with a positive control that the change engaged and remove-one arms to attribute an effect
vocabulary: ablation ablate arms baseline arm treatment arm with and without a b comparison does this prompt help does the guidance change behaviour system prompt skill tool injected context blind judge shuffled order strip the injected text positive control engaged loaded remove-one leave-one-out attribute the effect noise floor scenarios times arms times trials three way comparison
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Ablation Loop

The change is to what an agent is given: a prompt, a skill, a tool, a block of injected guidance, a system prompt. The question is whether behaviour changes because of it. The loop runs the same scenarios in separate arms and has an independent judge compare them blind.

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

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- evaluative/agent-eval(evaluative) — graders, trials, pass@k
- evaluative/clean-room(evaluative) — isolated, disposable environments per run
- evaluative/held-out(evaluative) — scenarios written by someone other than the author
