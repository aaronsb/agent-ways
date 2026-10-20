---
description: What a prompt for another agent must carry, since a subagent, workflow agent, or headless Claude Code run inherits no memory, project instructions, or conversation. The user's writing and send rules, the model and skills the user named, the fixed contract verbatim, owned files, absolute paths, small batches for bulk work, and single-paragraph goal statements with passing conditions.
vocabulary: inherits-nothing carry-over house-rules writing-rule read-only-comms never-send named-skills model-pin fixed-contract owned-files absolute-paths confidence caveats bulk small-batches respawn missing-units quota headless claude-p task-prompt goal-paragraph passing-conditions for-an-agent-to-execute standalone
pattern: (^|\s)/goals? (statement|paragraph)\b|goal.?ready|for an(other)? agent to (execute|run)|passing conditions?|headless (claude|cc)\b|claude -p\b|\bbriefs? (for|to) (the |each |every )?(sub)?agents?\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# What a Brief Must Carry

A subagent, a workflow agent, or a headless run starts with nothing from this session. It has no memory, none of the user's corrections, and none of the conversation. Whatever the brief leaves out, the worker does not know. The parent way covers brief fidelity and the return shape. This way covers what to put in.

## Put everything in the brief

- **The user's writing rules, verbatim**, when the worker writes text the user will read. Paste the rule. A summary like "match the user's style" carries nothing.
- **Read-only or drafts-only** for any worker that touches email, chat, or other comms. It never sends. Say it in those words.
- **The model the user named.** It goes on every agent the run starts. Check the pin before launch. A user who named one model version means that version, not the newest in its family.
- **The skills or tools the user named**, by name: "use the comms skill throughout".
- **The fixed contract, verbatim**: the API, data shape, or output format that parallel workers must agree on.
- **The files the worker owns** and the scope it must not leave.
- **"Use absolute paths in shell commands."** The worker's working directory can reset between calls.

## Add confidence and caveats to the return

For research and judgment work, ask for two items beyond the parent's return shape:

- A confidence level for the bottom line, with the reason.
- Caveats, stated before the evidence.

Ask for the bottom line first. A blocked or failed worker still returns every item.

## Bulk work

- Split bulk work into small fixed batches, such as a few pages or files per worker, so a killed worker loses little.
- If a quota limit stops workers partway, parse each result file and count the valid ones. Then start workers for exactly the missing units. Do not rerun the whole set.

## Headless Claude Code

For a headless run in a repo that already has project instructions, write a short, plain task prompt. Rely on the repo's own instructions, the built-in tools, and a generated config or env file. Do not add a custom system prompt, API pseudo-code, hand-rolled tools, artificial timeouts, or turn limits. Each of those overrides behavior the repo already set.

## Goal statements for another agent

When the user asks for a goal statement or a prompt "for an agent to execute", return one imperative paragraph shaped "Do X, Y, and Z until A, B, and C hold."

- State each passing condition so a reader can check it from shown output.
- Name the exact commands and paths.
- Make it stand alone. The receiving agent has none of this conversation.
- Follow any goal notes in the project instructions.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The worker will pick up the project rules." | It sees what the brief and the repo give it. Your session's corrections are not in either. |
| "I'll describe the contract in my own words." | Two workers paraphrase it two ways. Paste it verbatim. |
| "It's a comms task, but the agent knows not to send." | Write "never send" in the brief. |
| "Rerun the whole batch to be safe." | Count the valid results and rerun only the gaps. |

## See Also

- subagents(meta) — parent: brief fidelity, the return shape, and depth
- goals(meta) — goal mode and the condition it runs toward
- workflows(meta) — scripted multi-agent runs that use these briefs
