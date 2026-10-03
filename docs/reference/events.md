---
id: 01.023.R
domain: ways
mode: reference
related:
  - "[[ADR-134]]"
  - "[[ADR-196]]"
aliases: []
---

# The event log

The `ways` binary appends one JSON object per line to `$XDG_STATE_HOME/agent-ways/events.jsonl` (usually `~/.local/state/agent-ways/events.jsonl`; `ways events-log-path` prints it). Nothing reads the file while a session runs. `ways tune stats`, `ways tune precision`, `ways session` and `ways agent cost` read it afterwards.

Every value is a string, numbers included. Every line has `ts` (UTC, ISO 8601) and `event`. A field is left out when it has no value; an empty string means the value was computed and was empty.

When the file passes 32 MiB, the next write cuts it to its most recent 24 MiB at a line boundary. `judge_call` lines from the dropped part are kept, so judge spend history survives the cut.

## Common fields

| Field | Meaning |
|-------|---------|
| `way` | Way id, its path under the ways root: `softwaredev/delivery/commits` |
| `domain` | First segment of the way id |
| `trigger` | The channel that matched; see [Trigger values](#trigger-values) |
| `scope` | Who received it: `agent`, `subagent`, `teammate` |
| `project` | Project directory |
| `session` | Claude Code session id. Subagent hooks report the parent's session. |
| `agent_id` | The agent the event concerns: `main`, or the subagent's id |
| `model` | Model id the receiving agent ran, read from its transcript. `unknown` when none was resolved. |
| `hook` | The hook event that ran the gate: `UserPromptSubmit` |

## Events

### `session_start`

The SessionStart hook ran. Fields: `project`, `session`.

### `way_fired`

A way was shown to the agent for the first time in its session.

| Field | Meaning |
|-------|---------|
| `way`, `domain`, `trigger`, `scope`, `project`, `session` | Common fields |
| `token_position` | Session token count at the fire |
| `model`, `agent_id` | Common fields |
| `fire_score` | Semantic fires only: the calibrated score that fired the way |
| `surface` | Semantic fires only: a snippet of the text that was matched |
| `matched_span` | Keyword, command and file fires only: the text the pattern matched |
| `parent`, `tree_depth`, `epoch_distance` | Ways inside a tree: the parent id, depth, and epochs since the parent fired |
| `team` | Teammate sessions: the team name |

A way delivered to a subagent at dispatch logs `way_fired` with `way`, `domain`, `trigger`, `scope`, `project`, `session` and `team`, with no position or model.

### `way_redisclosed`

A way fired again after its `refire:` cadence let it. Same fields as `way_fired`, except `matched_span`, which is only on first fires.

### `way_suppressed`

A way or check matched and was not shown.

| Field | Meaning |
|-------|---------|
| `kind` | `way` or `check` |
| `reason` | `refire`: the cadence is still holding it back (logged once per way per fire window). `context_cap`: the per-invocation context budget was full. |
| `way`, `domain`, `trigger`, `scope`, `project`, `session`, `agent_id` | Common fields |

### `way_nearmiss`

A semantic score landed within `near_miss_margin` (default 0.05) under the fire threshold. These are the likely false silences.

| Field | Meaning |
|-------|---------|
| `way`, `corpus_id`, `domain`, `trigger`, `scope`, `project`, `session` | `corpus_id` is the way's id in the corpus, prefixed for project ways |
| `prob_en`, `prob_multi` | Calibrated probability from the English and multilingual models; empty when a model did not score |
| `tau_s` | The semantic fire threshold the scores were measured against |
| `margin` | How far under `tau_s` the best score landed |
| `query_tokens` | Approximate size of the matched text |

Rows written before calibration carry `score_en`, `score_multi`, `thr_en` and `thr_multi` instead.

### `way_keyword_gated`

A `pattern:` matched, but the way's semantic score was under the keyword floor on every model, so it did not fire (ADR-155). `pattern_strict` ways skip the floor and never log this.

| Field | Meaning |
|-------|---------|
| `matched_span` | The text the pattern matched |
| `prob_en`, `prob_multi` | Calibrated probabilities |
| `floor` | The keyword floor |
| `token_position` | Session token count |
| `way`, `corpus_id`, `domain`, `trigger`, `scope`, `project`, `session` | As in `way_nearmiss` |

### `check_fired`

A check under a way was shown.

| Field | Meaning |
|-------|---------|
| `check` | Check id |
| `domain`, `trigger`, `scope`, `project`, `session` | Common fields |
| `epoch`, `way_epoch`, `distance` | Current epoch, the epoch the parent way last fired, and the distance between them |
| `fire_count` | Times this check has fired in the session |
| `match_score`, `effective_score` | The raw match score and the score after distance decay |
| `anchored` | `true` when the check's `## anchor` section was included, which happens five or more epochs after the parent way fired |

### `injection_suppressed`

The subagent switch withheld ways from a dispatched agent. Logged once per Task dispatch and once per agent.

| Field | Meaning |
|-------|---------|
| `reason` | `subagents_off` |
| `switch` | `session` (the per-session switch) or `config` (`subagents: false`) |
| `lane` | The hook lane that was suppressed: `task`, `subagent_start`, `prompt`, `state`, `command`, `file`, `post_tool`, `queued` |
| `agent` | The agent id, when known |
| `scope`, `project`, `session` | `scope` is `subagent` |

### `way_judged`

The relevance gate (ADR-196) asked the judge about a way that matched on the prompt lane. One line per way judged.

| Field | Meaning |
|-------|---------|
| `way` | The judged way |
| `p_yes` | The judge's probability that the way is relevant |
| `threshold` | The profile's threshold |
| `verdict` | `pass`; `block` in `enforce` mode; `would_block` in `shadow` mode |
| `mode`, `engine`, `model` | Gate mode, engine profile and judge model |
| `judge_ms`, `gate_ms`, `candidates` | Judge latency, whole-gate latency, ways in the request |
| `reason`, `ancestor` | On a way blocked because an ancestor was blocked: `reason` is `ancestor` and `ancestor` names it. These lines carry no latency or candidate count. |
| `hook`, `scope`, `project`, `session` | Common fields |

### `judge_call`

One provider call made by the judge. `ways agent cost` sums these.

| Field | Meaning |
|-------|---------|
| `outcome` | `judged`, or `fallback` when the call ended in a fallback |
| `reason` | The fallback reason, on `fallback` |
| `engine`, `provider`, `model`, `candidates` | The call |
| `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens` | Usage, when the provider reported it |
| `cost_usd` | Cost, when known. Left out rather than written as zero when unknown. |
| `cost_source` | `provider`, `price_table` or `unknown` |
| `hook`, `scope`, `project`, `session` | Common fields |

### `gate_fallback`

The gate could not judge, so nothing was blocked. The gate fails open.

| Field | Meaning |
|-------|---------|
| `reason` | Why: a timeout, a transport or provider error, an agent error, or `config: …` when the gate settings do not parse (that line has no other fields) |
| `gate_ms`, `candidates` | Gate latency and ways that went unjudged |
| `hook`, `scope`, `project`, `session` | Common fields |

### `gate_capped`

More ways matched than the profile's `max_candidates` (default 8). The rest went unjudged and fire unless an ancestor was blocked.

| Field | Meaning |
|-------|---------|
| `judged`, `unjudged` | Counts |
| `ways` | The unjudged way ids, comma-separated |
| `hook`, `scope`, `project`, `session` | Common fields |

## Trigger values

| `trigger` | Channel |
|-----------|---------|
| `keyword` | A `pattern:` matched the prompt |
| `semantic:embedding:en`, `semantic:embedding:multi` | Prompt matched by the single-vector semantic matcher, English or multilingual model |
| `semantic:late-interaction:en`, `semantic:late-interaction:multi` | Prompt matched by the late-interaction matcher (ADR-160) |
| `semantic:bash:en`, `semantic:bash:multi` | A shell command's text matched a way's description semantically |
| `bash` | A `commands:` pattern matched a shell command |
| `file` | A `files:` pattern matched a file being edited |
| `state` | A `trigger:` condition: `session-start`, `context-threshold`, `file-exists` |
| `task` | The Task lane, scanning a subagent's prompt |
| `prompt` | Near-miss and keyword-gate rows from the prompt lane; subagent dispatch fires with no recorded channel |
| `check-pull` | A check fired before its parent way, so the parent was shown with it |
| `postcheck` | A way requested by a post-tool check script |
| `attend:<signal>` | An attend signal handler |

`ways tune stats` groups these into lanes: `keyword` and the prompt semantic triggers are the prompt lane, `semantic:bash:*` joins `bash`, and the rest group by the text before the first colon.

## Reading it

```bash
# Ways the judge blocked
jq -c 'select(.event=="way_judged" and .verdict=="block") | {ts, way, p_yes}' \
  "$(ways events-log-path)"

# Fires by team
jq -r 'select(.event=="way_fired" and .team) | .team' "$(ways events-log-path)" | sort | uniq -c
```

See [stats.md](../hooks-and-ways/stats.md) for the summary reports built on this log.
