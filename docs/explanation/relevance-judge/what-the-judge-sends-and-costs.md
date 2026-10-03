---
id: 01.021.E
domain: ways
mode: explanation
related:
  - "[[ADR-502]]"
  - "[[ADR-196]]"
  - "[[01.020.E]]"
  - "[[01.022.E]]"
aliases: []
---

# What the judge sends and what it costs

The relevance judge ([the model](relevance-judge-the-model.md)) is the one part of agent-ways that sends conversation text off the machine, and the one part that costs money per prompt. This page says what is sent, where, and when; how the key is held; how the agent process behaves; and how spend is counted. The design is [ADR-502](../../architecture/platform/ADR-502-the-ways-agent-one-resident-daemon-per-user-for-search-judging-and-key-custody.md).

## What leaves the machine

A request goes out only when all of these hold:

- the prompt or queued-message lane produced candidates that the refire curve lets through and that are not `pattern_strict`;
- `gate.mode` is `enforce` or `shadow`;
- the engine's provider has a key that passed its last check.

Then the agent makes one HTTPS request per gated prompt, to `api.anthropic.com/v1/messages` or `openrouter.ai/api/v1/chat/completions`, depending on the engine's provider. The request holds:

- a fixed system instruction and the `record_judgements` tool schema;
- for each candidate, up to `max_candidates`: the way's path through the ways tree, with `›` between segments, and its `description`. The way's body is not sent;
- the conversation: the last `turns` turns, each with whitespace collapsed, `<` replaced by `‹`, and cut to its last `max_turn_chars` characters (1,200 shipped).

The hook offers the agent at most two turns: Claude's last reply and the prompt. The reply is the text blocks of Claude's last message, the first 2,000 bytes, as the Stop hook recorded it; tool calls and their results are not part of it. The prompt is sent lowercased, as the matcher sees it. The shipped profiles set `turns: 1`, so by default only the prompt is sent. Setting `turns` to 2 or more adds Claude's reply, and no setting sends more than these two. On the queued-message lane the turn is the queued messages joined together, with no reply.

The session id, project path, file contents and the rest of the transcript are not sent. OpenRouter requests carry an `X-Title: agent-ways` header.

A key check sends no conversation text and costs nothing. On Anthropic it fetches the model's metadata (`GET /v1/models/<model>`); on OpenRouter it reads the key's limits and the model's endpoints. `ways agent models` fetches the provider's model list, with the key on Anthropic and without one on OpenRouter.

Shadow mode makes the same requests as enforce and is billed the same. To stop them, set `gate.mode off` or remove the key with `ways agent key remove`.

## Key custody

A key is a file, `$XDG_CONFIG_HOME/agent-ways/keys/<provider>`, mode 0600 inside a directory of mode 0700, written atomically. `ways agent key` adds, checks, rotates and removes keys, and `ways agent key status` says where each key comes from. Nothing prints more than a key's last four characters, and the settings screen shows each key only as present or absent.

The hook never reads a key; it only asks whether a key file exists. The agent reads the key to judge. The `ways agent` commands read it to check it (`key add`, `key rotate`, `key check`), to report it (`key status`), and to list Anthropic's models (`models`), and `ANTHROPIC_API_KEY` and `OPENROUTER_API_KEY` override the file for them. Every agent started by a hook or by `ways agent load` gets a cleared environment and reads only the key file; only `ways-agent serve` run directly from a shell inherits the variables. A key exported for Claude Code itself therefore never turns the gate on and is never used for judging.

A key is used for judging only after a check against the profile's model has passed. `key add` and `key rotate` check the key before storing it; `key add --no-check` stores it unchecked. The result is recorded under `$XDG_STATE_HOME/agent-ways/agent/key-check-<provider>.json`. Changing the key file or the profile's model makes the record stale: the next judged prompt starts a new check in the background and falls back until it passes. `ways agent key check` runs the check at once.

## The agent process

`ways-agent` is one process per user. It listens on `$WAYS_AGENT_SOCK`, else `$XDG_RUNTIME_DIR/agent-ways.sock` when that directory is private, else a per-user directory under the temporary directory. The socket is mode 0600, and the agent answers only connections from the same uid.

There is no service unit. A hook starts the agent when none is listening, under a lock so racing hooks leave one agent, and `ways agent load` starts it the same way. It exits after 30 minutes without a request, when its binary is replaced on disk, or on `ways agent unload`. A hook that finds an agent built against a different version of the shared core asks it to stop after the call, so the next prompt starts a current one.

The agent re-reads `agent.yaml` and the key on every request, so a settings or key change applies at the next prompt without a restart. Its `concurrency` setting caps the provider calls in flight across all sessions.

## What it costs

One provider call judges every candidate of a request, so cost belongs to the call. The hook logs each call once as a `judge_call` event with its tokens and cost. The cost is the provider's own figure where it reports one (OpenRouter), else tokens times a price, else unknown. A call of unknown cost is counted apart, never as zero. The [`ways agent cost` reference](../../reference/ways-cli.md#ways-agent-cost) gives the pricing rules and how to set prices for a model agent-ways does not price. The probe measured about $0.0007 per call at the prices of the time.

Three views read these events:

- `ways agent cost` totals spend by day, month, session or project;
- the session screen's spend tab shows the same by day, and `d` switches it to months;
- the session screen's header gives the session's judge calls and tokens (`judge ×12 · 34.5K tokens`), and `$` switches it to cost.
