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

A write that finds the file past 32 MiB compacts it to its most recent 24 MiB, cut at a line boundary. `judge_call` lines from the dropped part are carried forward, so judge spend history survives. When the carried lines leave too little to free, the compaction is skipped until the file has grown further, so the file can sit above 32 MiB for a while.

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
| `fire_score` | Semantic fires only: the score that decided the fire (the share for a late-interaction fire, the calibrated `g(s)` for a single-vector one; see [hooks-and-ways.md](../hooks-and-ways.md#telemetry)) |
| `surface` | Semantic fires only: a snippet of the text that was matched |
| `matched_span` | Keyword, command and file fires only: the text the pattern matched |
| `parent`, `tree_depth`, `epoch_distance` | Ways inside a tree: the parent id, depth, and epochs since the parent fired |
| `team` | Teammate sessions: the team name |
| `discarded` | Discarded fires only: an object naming what switched the way off. `filter` is `toggle`, `domain` or `project_off` (`enabled: false`); `toggle` is the winning toggle key when the filter is `toggle`; `scope` is the config layer that holds the switch, `user` or `project`, when known |
| `lane` | Discarded fires only: the lane that would have fired the way: `prompt`, `command`, `file`, `tool`, `task`, `state`, `pull` or `subagent` |
| `cosine`, `share`, `margin` | Discarded fires on a semantic channel: the way's scores beside the enabled ways, named as the decision record's candidates |

A way delivered to a subagent at dispatch logs `way_fired` with `way`, `domain`, `trigger`, `scope`, `project`, `session` and `team`, with no position or model.

A `way_fired` event with a `discarded` object is a discarded fire: a switched-off way that would have fired on this surface, refire window included. It was not shown to the agent and recorded no refire state. `ways tune stats` and the session summaries count discarded fires apart from delivered ones, and every reader of delivered fires leaves them out. A discarded check logs `check_fired` with the same `discarded` object and `lane: check`.

### `way_redisclosed`

A way fired again after its `refire:` cadence let it. Same fields as `way_fired`, except `matched_span`, which is only on first fires.

### `way_suppressed`

A way or check matched and was not shown.

| Field | Meaning |
|-------|---------|
| `kind` | `way` or `check` |
| `reason` | `refire`: the cadence is still holding it back (logged once per way per fire window). `context_cap`: the per-invocation context budget was full. `deadline`: the hook was too near its deadline to show the way, so no fire was recorded. |
| `way`, `domain`, `trigger`, `scope`, `project`, `session`, `agent_id` | Common fields |

### `way_nearmiss`

A semantic score landed within `near_miss_margin` under the fire threshold (see [stats.md](../hooks-and-ways/stats.md#auditing-the-telemetry) for the setting). These are the likely false silences.

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

The relevance gate asked the judge about a way that matched, on any lane. One line per way judged.

| Field | Meaning |
|-------|---------|
| `way` | The judged way |
| `p_yes` | The judge's probability that the way is relevant |
| `threshold` | The profile's threshold |
| `verdict` | `pass`; `block` in `enforce` mode; `would_block` in `shadow` mode |
| `mode`, `engine`, `model` | Gate mode, engine profile and judge model |
| `judge_ms`, `gate_ms`, `candidates` | Judge latency, whole-gate latency, ways in the request |
| `reason`, `ancestor` | On a way blocked because an ancestor was blocked: `reason` is `ancestor` and `ancestor` names it. These lines carry no latency or candidate count. |
| `critique` | The judge's reason for its answer, when it gave one. The shipped judge's answer has no field for it, so current lines carry none. |
| `command` | Command lane only: the first 80 characters of the call, with the value of each shell assignment, credential flag and auth header replaced by `…` |
| `lane`, `agent_id` | The lane that asked and the calling agent |
| `hook`, `scope`, `project`, `session` | Common fields |

### `way_unjudged`

A tool lane's candidate the judge gave no verdict, in enforce mode. One line per way. A tool lane sends one judge request per call, so a candidate gets no verdict when it was past the request's cap or its request failed.

| Field | Meaning |
|-------|---------|
| `way` | The way, or `check:<id>` for a check |
| `trigger` | What picked it: `semantic`, `keyword`, `strict` or `declared` |
| `verdict` | `block` for a candidate only the embedding matcher picked, which is not shown; `pass` for any other, shown as the matcher decided |
| `reason` | `overflow` when it was past `tool_max_candidates`, else the request's fallback reason, such as `deadline`, `deadline: unclaimed` or `model_error: aborted` |
| `mode`, `command` | `enforce`, and on the command lane the call as `way_judged` records it |
| `hook`, `lane`, `scope`, `project`, `session`, `agent_id` | Common fields |

### `judge_call`

One provider call made by the judge. `ways agent cost` sums these.

| Field | Meaning |
|-------|---------|
| `outcome` | `judged`, or `fallback` when the call ended in a fallback |
| `reason` | The fallback reason, on `fallback` |
| `engine`, `provider`, `model`, `candidates` | The call |
| `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens` | Usage, when the provider reported it |
| `cost_usd` | Cost, when known. Left out rather than written as zero when unknown. |
| `cost_source` | `provider`, `price_table`, `account` or `unknown`. `account` is a call on the Claude Code account under the `claude-code` engine whose usage reports tokens; it carries no `cost_usd`. A call that reports no tokens is `unknown`: one with no usage, such as one that hit its deadline, or one whose usage is all zeros, such as an aborted call, which still logs its zero token fields. |
| `hook`, `scope`, `project`, `session` | Common fields |

### `gate_fallback`

One judge request could not be judged. On the prompt and state lanes nothing is blocked and the gate fails open. On a tool lane each of its candidates also logs `way_unjudged`, and in enforce mode a semantic one is not shown.

| Field | Meaning |
|-------|---------|
| `reason` | Why: a timeout, a transport or provider error, an agent error, or `config: …` when the gate settings do not parse. A `config:` line carries no latency or candidate count. |
| `gate_ms`, `candidates` | Gate latency and ways that went unjudged |
| `hook`, `scope`, `project`, `session` | Common fields |

### `judge_detach_skipped`

Written once per session, the first time a lane would have started a detached judge (a tool lane in shadow mode, or the `gate.held_sample` share of held candidates) while the session judges on the Claude Code internal engine. That engine's host claims jobs only while a hook runs, so no detached judge starts and those verdicts are not recorded. A key engine starts the detached judge as usual. In `decisions.jsonl` a shadow tool lane's record then carries judge status `detach_skipped` in place of `detached`: no detached record with verdicts follows it.

| Field | Meaning |
|-------|---------|
| `reason` | `claude-code engine has no host outside a hook` |
| `candidates` | Ways the first skipped judge would have judged |
| `lane`, `agent_id` | The lane and the calling agent |
| `session` | Common field |

### `gate_capped`

Written by older releases when more ways matched than the profile's `max_candidates` (default 8) and the rest went unjudged. The gate now judges every candidate in further requests, so a current log has none.

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
| `semantic:late-interaction:en` | Prompt matched by the late-interaction matcher (ADR-160); always recorded with `:en` |
| `semantic:bash:en`, `semantic:bash:multi` | A shell command's text matched a way's description semantically |
| `bash` | A `commands:` pattern matched a shell command |
| `file` | A `files:` pattern matched a file being edited |
| `tool` | A `tool_name:` pattern matched the tool name at an event its `tool_event:` lists |
| `state` | A `trigger:` condition: `session-start`, `context-threshold`, `file-exists` |
| `task` | The Task lane, scanning a subagent's prompt |
| `prompt` | Near-miss and keyword-gate rows from the prompt lane; subagent dispatch fires with no recorded channel |
| `check-pull` | A check fired before its parent way, so the parent was shown with it |
| `postcheck` | A way requested by a post-tool check script |
| `attend:<signal>` | An attend signal handler |
| `unknown` | A way shown by `ways show way` with no `--trigger` |

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

## References

- ADR-712: every candidate in every lane and agent thread is judged, after the refire check and before the fire is recorded. It retired `gate_capped`.
