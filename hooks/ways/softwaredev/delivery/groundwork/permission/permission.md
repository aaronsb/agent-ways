---
description: when contributions cluster on the files people are allowed to edit because the tests, pipeline, or deployment belong to another team, naming the ownership boundary and the decision it needs instead of building around it
vocabulary: permission allowed to change ownership boundary another team owns platform team approval negotiate their backlog authority route around workaround periphery clustered on config skew who can change this
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# What We Are Allowed to Touch

People work on what they have permission to change. An engineer can edit a skill file without asking anyone. Improving the tests may need another team's approval; fixing the deploy may need two managers to negotiate. So the skill file gets better and the build stays broken — not from poor judgment, but from where the permission lines fall.

This shows up in a repository as **skew**: a queue heavy with docs, config, adapters, prompts, and new abstractions on top, while the layer underneath goes untouched. Skew is diagnostic information about the project. Read it that way.

## What we do when we hit the boundary

1. **Name it.** Which condition in `delivery/groundwork` is missing, who owns the thing that would fix it, and what decision or access is needed. Write that down as a finding.
2. **Do not compensate with a primitive.** Another agent, a longer prompt, or a wrapper around the broken step hides the boundary and adds something else to maintain. Three agents discussing a repository nobody is allowed to fix is still a repository nobody is allowed to fix.
3. **Hand the decision to a person.** We cannot negotiate a place in another team's planning cycle. Surface it as a choice (see `choices`): who needs to say yes, what existing commitment moves, how soon the return is wanted.
4. **Watch for the standing intervention.** If a leader has to step in every time to get a change deployed, that dependency is itself a missing condition. Put it in the report, not in the deployment instructions.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "That's the platform team's backlog" | It can sit in their backlog for a very long time. Name who decides the priority. |
| "I can at least improve what I can reach" | Improving the periphery while the core is untestable is the pattern we are trying to stop. Report the boundary first. |
| "A wrapper script gets us past it for now" | It gets us past it silently. The boundary is now invisible and still there. |
| "They'll get to it" | Maybe. Give the human a decision, not a hope. |

## See Also

- choices(meta) — presenting a genuine decision to the human
- trust/autonomy(meta) — when to act and when to check in
- delivery/groundwork(softwaredev) — parent: the conditions the boundary is blocking
