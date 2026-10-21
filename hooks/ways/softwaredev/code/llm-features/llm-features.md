---
description: building an app, agent, or MCP server that calls a language model; look up the current model ID instead of guessing it, fail loudly when a key or model call fails instead of returning canned output, wire demos to the real backend, fetch IDs at runtime instead of hard-coding them, keep prompts as a few plain sections, and keep MCP stdout for the protocol only
vocabulary: llm language model model api call api key model id model name sdk agent app chatbot feature mcp server tool resources stdout stderr logging protocol stream polling poll interval fallback canned response hardcoded fixture demo data sample data custom field id prompt template xml sections max tokens output limit native messaging host semantic tools
pattern: mcp.?server|model.?id|max.?tokens|native.?messaging.?host
refire: 0.2
scope: agent, subagent
---
<!-- epistemic: convention -->
# LLM Features and MCP Servers

A feature that calls a model is judged by what the model returns. Most of the ways it goes wrong hide a broken model path behind output that looks fine.

## Model choice

- Look up the current model ID in the provider's docs or SDK reference. Do not guess it from memory, and do not suggest a retired model.
- Use the model the user or the project chose. Swap to a smaller or faster one only when the user names latency or cost as the constraint, or approves the swap.
- Set the output token limit for the job. A low default cuts a long answer off partway.

## No silent fallbacks

| Tempting move | Do this instead |
|---|---|
| A degraded path when the API key is missing | Fail with an error that names the missing key |
| Catch a failed model call and return a canned result | Surface the error and the reason it failed |
| Hard-coded logic standing in for the agentic step | Let the model do the work the feature was designed for |
| Fixture or demo data so the demo looks good | Wire it to the real backend. The demo works only when the feature works |

## No hard-coded values

- Put no sample entities, prescribed value lists, or IDs in prompts or code. Give the model a general framework instead.
- Fetch environment-specific IDs, such as a tracker's custom field IDs, from the authoritative API at runtime. Add a comment that says why. A guessed ID breaks the write that uses it.
- The feature should work across tenants and datasets, not only the one you tested against.

## Prompt shape

Write prompts as a few plain sections, for example `<task>`, `<approach>`, `<avoid>`, and `<output>`. Leave out how-to the model already knows.

## MCP servers and native-messaging hosts

- stdout carries only the protocol. Send every log line to stderr. One stray print corrupts the stream the client parses.
- Use prompt resources and tool descriptions to steer the consuming model. Say which tools chain together, and run auth automatically when a call needs it.
- Set poll intervals for a model that reads every result. A tool that asks to be polled every few seconds spends the model's context on empty updates.
- Offer high-level semantic tools for the common jobs, and keep the low-level API reachable for the rest.

## Spend and accounts

- Prefer free, local options that need no new accounts. If the user must sign up for a service, give the exact steps. Do not create the account yourself.
- When verification makes paid API calls, cap the spend and put an estimate in the plan.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "Fixture data is fine for the demo" | The audience will believe the feature works. Show the real path or say it is not wired yet. |
| "A fallback is friendlier than an error" | A fallback hides that the feature is broken. The error tells someone what to fix. |
| "I know the model ID" | IDs change between releases. Look it up. |
| "One debug print to stdout won't hurt" | It breaks the client's protocol parser. Use stderr. |

## See Also

- code/errors(softwaredev) — a missing required value fails loudly instead of getting a default
- code/security/injection/prompt(softwaredev) — model output and tool results are data, not instructions
- code/security/secrets(softwaredev) — where API keys live and how they stay out of the repo
- architecture/design/prototype(softwaredev) — probe the real API before building on an assumption about it
