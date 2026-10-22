---
description: Checking whether a skill earns its place — the same tasks run with the skill and without it before a change is called better, a re-check after model updates, and for a skill that documents commands or ships a helper, rules moved into code and every command and flag run
vocabulary: is my skill better does the skill help without the skill skill evals plugin eval changed the skill base model already does it model update helper script documented commands flags defaults code before prose
pattern: \bskill evals?\b|\bplugin evals?\b|\b(with|without) the (skill|plugin)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Measuring a Skill's Effect

The parent way's last check is that the skill fires. Firing is half the check. The other half is whether the skill changes what gets done.

## Build the cases from failures

- Run representative tasks with the skill absent first, and record the failures. Build at least three scenarios from them. Write only enough skill to pass, and keep a rule only if removing it causes mistakes.
- When the skill encodes someone's judgment, that person is the expert. An agent may propose labels and failure notes, and the person confirms them before they become cases.

## Run with and without it

- A fresh session with the skill runs each scenario. Watch which files it reads and which rules it ignores.
- Run the same cases without the skill. The method is evaluative/ablation, and the run happens in a harness, not by eye. In agent-ways, `make test-live-matrix` runs arms, models and trials. A case that passes both ways did not pass because of the skill.
- A near-zero effect where the skill was never invoked usually means its description does not trigger. Fix the trigger before the body.
- Tell the agents in a run where scratch files go. Otherwise they write query dumps into the skill folder.
- Test on each model the skill will run with, and rerun after each model update. If the base model passes without the skill, the skill may no longer be needed.

## A skill that documents commands or ships a helper

- A rule that decides a result, such as an exclusion, a split, or a dedupe, belongs in the helper's code. Agents follow code every time and follow prose some of the time.
- After every helper edit, run every documented command and every flag value, including the defaults. Agents work around a broken default without saying so in their answers.

## See Also

- skills(meta) — parent: naming, scope, and the skill-or-way call
- evaluative/ablation(evaluative) — arms with and without a change, graded blind
- evaluative/held-out(evaluative) — cases from a fresh writer, retired once seen
