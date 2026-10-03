---
id: 01.022.E
domain: ways
mode: explanation
related:
  - "[[ADR-503]]"
  - "[[ADR-196]]"
  - "[[01.020.E]]"
  - "[[01.021.E]]"
  - "[[01.019.E]]"
aliases: []
---

# Watching and tuning the judge

The judge works out of sight: a blocked way leaves no mark in the conversation. Its work shows up in the event log, on the session screen, and in `ways agent status`. Its behaviour is set through the gate settings. For where the judge sits and how it fails, read [the model](relevance-judge-the-model.md).

## Is it on?

Three commands answer this, from the outside in:

- `ways status` prints a `Gate:` line with the mode, provider, model, threshold, key source and the key's last check. Bare `ways` and `ways --help` end with a "Ways is degraded" warning, naming the fix, whenever the judge cannot gate.
- `ways agent status` reports the running agent: its version, engine and whether the engine was set or picked by key order, the mode, requests judged, calls in flight against the concurrency cap, p50 and p95 latency, and fallbacks counted by reason.
- `ways agent key check` checks each stored key against the model the gate uses, with a call that costs nothing.

A fallback reason names its cause. The table in [how it fails](relevance-judge-the-model.md#how-it-fails) lists them.

## The events

The judge writes four event types to `$XDG_STATE_HOME/agent-ways/events.jsonl`, beside the matcher's own. Every value is a string. Each line also carries `hook`, `scope`, `project` and `session`.

| Event | One per | Fields |
|---|---|---|
| `way_judged` | judged candidate | `way`, `p_yes`, `threshold`, `verdict` (`pass`, `block`, `would_block`), `mode`, `engine`, `model`, `judge_ms`, `gate_ms`, `candidates` |
| `judge_call` | provider call | `outcome` (`judged` or `fallback`), `engine`, `provider`, `model`, `candidates`, `cost_source`, token counts and `cost_usd` when known, `reason` on a fallback |
| `gate_fallback` | failed request | `reason`, `gate_ms`, `candidates` |
| `gate_capped` | request over the cap | `judged`, `unjudged`, `ways` (the unjudged ids) |

A `way_judged` block with `reason: ancestor` names the blocked `ancestor` and carries its verdict. The judge never saw that way, so the line has no `judge_ms`, `gate_ms` or `candidates`. A count of the judge's own verdicts skips these lines.

A blocked way has no `way_fired` line. A `would_block` way in shadow mode was injected, so it has both.

To read the recent fallbacks:

```
jq -c 'select(.event == "gate_fallback") | {ts, reason}' ~/.local/state/agent-ways/events.jsonl | tail
```

## The session screen

`ways session replay` shows the judge's work on the timeline. A row the judge blocked is marked `⊘` and shows its P(yes) in the Trigger column, in the frame where it was judged. A way blocked with its ancestor reads `⊘ <way> (with <ancestor>)`. A way the judge would have blocked in shadow mode was injected and is marked `◌`. By default the table shows the injected ways, and `f` widens it to every matched candidate. The header counts the session's judge calls and tokens, and `$` switches it to cost.

`ways session fires`, `ways session ways` and `ways session dump` take `--matched` to include the ways the judge kept out, each with its P(yes) against the threshold. The keys and the other tabs are in the [`ways session` reference](../../reference/ways-cli.md#ways-session). Reading a session's events by hand is covered in [reading the session data yourself](../how-ways-works/reading-the-session-data.md).

## The settings

The gate's settings live in `$XDG_CONFIG_HOME/agent-ways/agent.yaml`, the user layer over the profiles agent-ways ships. The file survives updates. Settings go through the typed registry of [ADR-503](../../architecture/platform/ADR-503-settings-are-files-described-by-one-typed-registry-the-cli-and-the-tui-are-two-ways-in.md), so `ways settings` reads and writes them from the command line, and `ways agent` alone on a terminal opens the settings screen on its gate tab.

| Key | Shipped | Effect |
|---|---|---|
| `gate.engine` | unset | The profile in use. Unset, the first shipped profile whose provider has a key file: `anthropic`, then `openrouter`. Adding a key never switches an engine that is set. |
| `gate.mode` | `enforce` | `enforce`, `shadow` or `off`. |
| `gate.profiles.<p>.provider` | per profile | `anthropic` or `openrouter`. A profile that changes provider must name its model too. |
| `gate.profiles.<p>.model` | `claude-haiku-4-5`, `anthropic/claude-haiku-4.5` | The model judged with. Changing it makes the key's check stale until it is checked against the new model. |
| `gate.profiles.<p>.threshold` | 0.3 | P(yes) below this is blocked in enforce mode. |
| `gate.profiles.<p>.timeout_ms` | 2000 | The agent's deadline for a request. |
| `gate.profiles.<p>.turns` | 1 | Conversation turns sent, counted back from the prompt. The hook offers two at most: Claude's last reply and the prompt. |
| `gate.profiles.<p>.max_turn_chars` | 1200 | Each turn is cut to its last this-many characters. |
| `gate.profiles.<p>.concurrency` | 8 | Provider calls the agent runs at once, across sessions. |
| `gate.profiles.<p>.max_candidates` | 8 | Candidates judged per request, in matcher order. |
| `gate.profiles.<p>.price_in_per_mtok`, `price_out_per_mtok` | unset | USD per million tokens, for pricing calls the provider does not price. They apply as a pair. |
| `gate.keys.<provider>` | absent | Shown as present or absent. `ways agent key` manages keys. |

A profile name agent-ways does not ship is a profile of your own. It must set `provider` and `model`, and takes the other fields from the shipped profile of that provider. Each profile falls back on its own when it fails the schema, so a typo in one does not drop another's tuning.

```
ways settings set gate.mode shadow
ways settings set gate.engine openrouter
ways settings set gate.profiles.anthropic.threshold 0.4
ways settings list gate --effective
```

## The model picker

On the settings screen, Enter on `gate.profiles.<p>.model` opens a picker. It lists the models from a cached copy of the provider's list, `$XDG_CACHE_HOME/agent-ways/agent/models-<provider>.json`, so opening it makes no network call. The recommended model comes first, then Anthropic's models, then the rest. `ways agent models --provider <p>` fetches and caches the list (`--all` lists every OpenRouter model). Until it has run, the key reports "model list not fetched" with that command.

After choosing a model, the next judged prompt starts a key check against it and falls back with `key_unverified: checking` until the check passes. `ways agent key check` runs the check at once.
