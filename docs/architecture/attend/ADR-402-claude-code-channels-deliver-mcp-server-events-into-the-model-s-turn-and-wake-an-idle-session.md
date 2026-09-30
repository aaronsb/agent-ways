---
contract: adr/v1
kind: evidence
capability: attend
status: accepted
date: 2026-09-30
deciders:
  - aaronsb
related:
  - ADR-172
  - ADR-182
  - ADR-187
---

# ADR-402: Claude Code channels deliver MCP server events into the model's turn and wake an idle session

## What this record is

A finding from a live test on 2026-09-30, Claude Code 2.1.285 on Linux. ADR-187 item 1 held that no MCP path places content into the model's turn, citing the MCP protocol revision of 2026-07-28, and concluded that inbound delivery must stay on the Monitor sensor loop and the Stop-hook drain. That reading covers the MCP protocol. Claude Code adds its own extension on top of it, channels, which the test shows does place content into the turn.

## The mechanism

A channel is an MCP server that declares the experimental capability `claude/channel` (value `{}`) in its initialize result and sends the notification `notifications/claude/channel` with params `content` (string) and `meta` (string map). Claude Code delivers each notification to the model as `<channel source="<server>" key="value" ...>content</channel>`, with each `meta` entry as a tag attribute. The server's `instructions` string reaches the model as context when the server connects. Reference: code.claude.com/docs/en/channels-reference.

During the research preview a channel outside Anthropic's allowlist loads only when the session is launched with `--dangerously-load-development-channels server:<name>`. No setting enables it. A server that declares the capability in a session launched without the flag connects as an ordinary MCP server, and Claude Code drops its channel notifications without an error.

## The test

A one-way channel server of about a hundred lines of Node, with no SDK, speaking newline-delimited JSON-RPC over stdio. It was declared in a project `.mcp.json` and loaded by a session launched with the flag. The server found its session by walking its parent processes to the `claude` process and reading `~/.claude/sessions/<pid>.json`. It pushed one event whenever a request file named for that session id appeared.

| Push | Session state before | Observed |
|---|---|---|
| "reply with one word and use no tools" | idle | status went to busy within one second of the push; the transcript records the `<channel>` event as a user entry and the model replied "Acknowledged." |
| "if attend is not running in this session, start it; otherwise reply with one word" | idle | a turn started; the model replied "Running." while no `attend run` process existed under the session |
| the same event, after the attend way's liveness check was fixed on disk | idle | a turn started; the model again replied "Running." without checking |

## Findings

1. **A channel event starts a turn in an idle interactive session.** No Monitor and no typed prompt is needed. The docs add that events arriving during a busy turn queue and are delivered together at the next turn.
2. **The channel server receives no session id.** It can derive one, because Claude Code spawns it as a child of the session's `claude` process, and the session record keyed by that pid carries the id.
3. **A channel-started turn leaves no entry in `~/.claude/history.jsonl`.** That file records typed prompts only, so it separates operator activity from channel traffic.
4. **The woken model is a poor judge of session state.** Asked whether attend was running, it answered from what its context said: first from a way that reported attend running when it was not, then from its own earlier reply. A decision about session state belongs in the process that pushes the event, which can inspect the state directly, with the event carrying a direct instruction.

## Limits

- One host, one Claude Code version, one session. The channel extension is a research preview; the capability name, the notification method and the flag can change.
- The test did not show a channel-started turn calling Monitor. Finding 4 kept the model from trying.
- Channel permission relay (`claude/channel/permission`) and two-way reply tools were not exercised.
