---
description: adding a feature, agent or MCP server that calls a language model such as Claude, like a summarize or draft button, or exposing a tool or data over MCP; a failed model call or missing API key returns an error that names the cause, MCP stdout carries only protocol messages with logs on stderr, model IDs come from current docs, and demos run against the real model path
vocabulary: language model llm model api call claude api openai api key model id sdk ai feature summarize classify generate button chatbot assistant agent app mcp server mcp tool expose over mcp stdio stdout stderr logging json-rpc protocol fallback canned response stub sample output demo data fixture hardcoded ids output token limit
pattern: (build|writ\w+|creat\w+|implement\w*) an? mcp server|\bmodel ids?\b
refire: 0.2
scope: agent, subagent
---
<!-- epistemic: convention -->
# LLM Features and MCP Servers

A feature that calls a model is judged by what the model returns. The common failures hide a broken model path behind output that looks fine.

## Fail loudly

| Tempting move | Do this instead |
|---|---|
| Return canned or sample output when the key is missing or the call fails | Return an error that names the cause: the missing key, the status code, the provider's message |
| Use fixture data so the demo looks good | Run the demo against the real model path, or say the path is not wired yet |

A missing required value fails at startup rather than getting a default (code/errors).

## Model, limits and IDs

- Look up the current model ID in the provider's docs or SDK reference instead of recalling it. Use the model the user or the project chose, and change it only when they ask or approve.
- Set the output token limit for the job. A low default cuts a long answer off partway.
- Fetch environment-specific IDs, such as a tracker's custom field IDs, from the authoritative API at runtime. Keep sample entities and fixed value lists out of prompts and code, so the feature works on other tenants and datasets.

## MCP servers

- stdout carries only protocol messages. Every log line goes to stderr. One stray print corrupts the stream the client parses.
- Tool descriptions say what each tool is for and which tools chain together.
- A tool that asks to be polled spends the consuming model's context on empty results. Set poll intervals for a reader that sees every result.
- Offer high-level tools for the common jobs and keep the low-level calls reachable.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "A fallback is friendlier than an error" | A fallback hides that the feature is broken. The error tells someone what to fix. |
| "Fixture data is fine for the demo" | The audience will believe the feature works. Show the real path or say it is not wired. |
| "I know the model ID" | IDs change between releases. Look it up. |
| "One debug print to stdout won't hurt" | It breaks the client's protocol parser. Use stderr. |

## See Also

- code/errors(softwaredev) — a missing required value fails loudly instead of getting a default
- code/security/injection/prompt(softwaredev) — model output and tool results are data, not instructions
- code/security/secrets(softwaredev) — where API keys live and how they stay out of the repo
- architecture/design/prototype(softwaredev) — probe the real API before building on an assumption about it
- evaluative/rubric(evaluative) — judging a model's answer when it varies run to run
