---
description: Writing the prompt or instructions for a subagent before sending it off, or working out why one came back wrong, given that it inherits none of this conversation, memory or corrections. The brief carries the user's writing and send rules, the named model and skills, the shared contract verbatim, the files it owns and the layout it needs.
vocabulary: brief good instructions write the prompt for a subagent delegate hand off send an agent off subagent came back with garbage wrong didn't know the layout missed context inherits nothing writing rules never send drafts only named model skill contract verbatim owned files absolute paths batches headless
pattern: \bbriefs? (for|to) (the |each |every |a )?(sub)?agents?\b|\bheadless (claude|cc)\b|\bfor an(other)? agent to (execute|run)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# What a Brief Must Carry

A subagent, workflow agent or headless run starts with nothing from this session: no memory, none of the user's corrections, none of the conversation. What the brief leaves out, the worker does not know. The parent way covers brief fidelity and the return shape. This way covers what goes in.

## Put it in the brief

- **The user's writing rules, verbatim**, when the worker writes text the user will read. Paste the positive rule. "Match the user's style" carries nothing, and a list of banned phrases plants the phrases.
- **Drafts only** for any worker that touches email, chat or other messages. Write "never send" in those words.
- **The model the user named**, on every agent the run starts. A named version means that exact version. A newer model in the same family is a different model.
- **The skills or tools the user named**, by name, such as "use the <named> skill throughout".
- **The shared contract, verbatim**: the API, data shape or output format that parallel workers must agree on.
- **The files the worker owns**, the scope it stays inside, and the repository layout it needs to find them.
- **Absolute paths** in shell commands. The worker's working directory can reset between calls.
- **One task.** Chaining two goals in one brief gets one of them done badly.

For research and judgment work, add to the return shape the parent lists a confidence level with its reason, and the caveats, stated before the evidence.

## Bulk work

- Split bulk work into small fixed batches, so a killed worker loses little.
- When a quota stops workers partway, count the valid result files and start workers for exactly the missing units.

## Headless Claude Code

In a repository that already has project instructions, give a headless run a short, plain task prompt. Add a custom system prompt, tool definitions, timeouts or turn limits only when the task needs them; each overrides what the repository already set.

For a goal condition another agent will run toward, use the goal-author skill.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The worker will pick up the project rules" | It sees what the brief and the repository give it. This session's corrections are in neither. |
| "I'll describe the contract in my own words" | Two workers paraphrase it two ways. Paste it verbatim. |
| "It's a messaging task, but the agent knows not to send" | Write "never send" in the brief. |
| "Rerun the whole batch to be safe" | Count the valid results and rerun only the gaps. |

## See Also

- subagents(meta) — parent: brief fidelity, the return shape and depth
- subagents/agent-teams(meta) — several agents on one repository
- workflows(meta) — scripted multi-agent runs that use these briefs
- goals(meta) — goal mode and the condition it runs toward
