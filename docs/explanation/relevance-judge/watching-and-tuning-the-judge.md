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
- `ways agent key check` checks each stored key against the model the gate uses.

A fallback reason names its cause; [how it fails](relevance-judge-the-model.md#how-it-fails) lists them.

## The events

The judge writes four event types to the event log, beside the matcher's own. Each carries `hook`, `scope`, `project` and `session`, so a session's judge work can be read on its own.

| Event | One per | What to read from it |
|---|---|---|
| `way_judged` | judged candidate | `p_yes` against `threshold`, and `verdict`: `pass`, `block` or `would_block`. `judge_ms` shows how close calls run to the deadline. |
| `judge_call` | provider call | Tokens and `cost_usd`, which the spend views sum. `reason` when the call ended in a fallback. |
| `gate_fallback` | failed request | `reason`: why the matcher decided alone. |
| `gate_capped` | request over the cap | `ways`: the candidates that passed unjudged. Frequent caps suggest raising `max_candidates`. |

A `way_judged` block with `reason: ancestor` names the blocked `ancestor` and carries its verdict. The judge never saw that way, so the line has no `judge_ms`, `gate_ms` or `candidates`, and a count of the judge's own verdicts skips it.

A blocked way has no `way_fired` line. A `would_block` way in shadow mode was injected, so it has both.

To read the recent fallbacks:

```
jq -c 'select(.event == "gate_fallback") | {ts, reason}' "$(ways events-log-path)" | tail
```

## The session screen

`ways session replay` shows the judge's work on the timeline. A row the judge blocked is marked `⊘` and shows its P(yes) in the Trigger column, in the frame where it was judged. A way blocked with its ancestor reads `⊘ <way> (with <ancestor>)`. A way the judge would have blocked in shadow mode was injected and is marked `◌`. By default the table shows the injected ways, and `f` widens it to every matched candidate. The header's judge figures are described under [what it costs](what-the-judge-sends-and-costs.md#what-it-costs).

`ways session fires`, `ways session ways` and `ways session dump` take `--matched` to include the ways the judge kept out, each with its P(yes) against the threshold. The keys and the other tabs are in the [`ways session` reference](../../reference/ways-cli.md#ways-session). Reading a session's events by hand is covered in [reading the session data yourself](../how-ways-works/reading-the-session-data.md).

## Tuning

The gate's settings live in `$XDG_CONFIG_HOME/agent-ways/agent.yaml`, the user layer over the profiles agent-ways ships, and survive updates. They go through the typed registry of [ADR-503](../../architecture/platform/ADR-503-settings-are-files-described-by-one-typed-registry-the-cli-and-the-tui-are-two-ways-in.md): `ways settings` reads and writes them from the command line, and `ways agent` alone on a terminal opens the settings screen on its gate tab. `ways settings help gate` lists every key with its default.

The settings a tuner reaches for, and why:

| Setting | Shipped | Why change it |
|---|---|---|
| `gate.mode` | `enforce` | `shadow` to watch the verdicts for a while before trusting them to block. |
| `gate.engine` | unset | To pin a provider. Unset, the first shipped profile whose provider has a key file is used, `anthropic` before `openrouter`, and adding a key never switches it. |
| `gate.profiles.<p>.threshold` | 0.3 | Higher blocks more, at the risk of losing relevant ways. A new model needs its own threshold, since confidence scales differ by model. |
| `gate.profiles.<p>.turns` | 1 | 2 adds Claude's last reply to what the judge reads, and to what is sent ([what leaves the machine](what-the-judge-sends-and-costs.md#what-leaves-the-machine)). |
| `gate.profiles.<p>.max_candidates` | 8 | Higher judges more of a busy prompt's candidates, at about 0.1 s each. |
| `gate.profiles.<p>.price_in_per_mtok`, `price_out_per_mtok` | unset | To price calls to a model whose provider reports no cost. |

A profile name agent-ways does not ship is a profile of your own. It must set `provider` and `model`, and takes the other fields from the shipped profile of that provider. Each profile is its own unit: a change that fails the schema or does not build is dropped, with a line on stderr, and the others load. A shipped profile then runs as shipped, whether or not it is the engine. A profile of your own is left out, and if it is the engine, the gate is off until it is fixed.

```
ways settings set gate.mode shadow
ways settings set gate.profiles.anthropic.threshold 0.4
ways settings list gate --json --effective
```

## The model picker

On the settings screen, Enter on `gate.profiles.<p>.model` opens a picker. It lists the models from a cached copy of the provider's list, `$XDG_CACHE_HOME/agent-ways/agent/models-<provider>.json`, so opening it makes no network call. The recommended model comes first, then Anthropic's models, then the rest. `ways agent models --provider <p>` fetches and caches the list. Until it has run, the key reports "model list not fetched" with that command.

A new model makes the key's last check stale, so the judge falls back until the key is checked against it; [key custody](what-the-judge-sends-and-costs.md#key-custody) explains the check.
