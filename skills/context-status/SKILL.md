---
name: context-status
description: Check how much context window remains in this session — token budget, usage, and room left before compaction. Use when the user asks how much context/room/budget is left, or wants a context-usage gauge.
allowed-tools: Bash
---

# Context Status

Run the context usage command and capture the JSON output:

```bash
ways context --json
```

Then render a visual gauge using the chart tool. Build the JSON and pipe it:

```bash
# Parse the values from the --json output, then:
echo '{"type":"hbar","data":{"Used (PCT%)":USED,"Free (RPCT%)":REMAINING},"title":"Context: USEDk / TOTALk tokens (MODEL)","width":60,"format":"human"}' \
  | ~/.claude/hooks/ways/softwaredev/visualization/charts/chart-tool
```

Replace `USED`, `REMAINING`, `TOTAL`, `PCT`, `RPCT`, and `MODEL` with actual values from the JSON output. Use `jq` to extract them.

The command resolves the context window from the model in the transcript (it varies by model), and reports how it got there in `window_source` (ADR-166):

| `window_source` | Meaning |
|---|---|
| `model_table` | The model was recognized; `tokens_total` is its real window. |
| `env_override` | `CLAUDE_CONTEXT_WINDOW` was set and took precedence over detection. |
| `default` | **The model was not recognized** and a conservative 200K was assumed. |

A `default` reading means the gauge is a guess, not a measurement — the model is missing from the table, and `tokens_total` is likely wrong. Say so rather than reporting the percentage as fact. Detection cannot see Claude Code's own 1M toggle (only the model id reaches the transcript), so `CLAUDE_CONTEXT_WINDOW` is the way to state the window explicitly; it overrides detection on every model.

If the remaining percentage is below 20%, mention that compaction is approaching and suggest wrapping up or prioritizing remaining work.

## Not for

- Changing the context window size or compaction threshold — that's `CLAUDE_CONTEXT_WINDOW` and the compaction settings, not this skill.
- General session status — it reports the context window only.
- A continuous watch — it's a one-shot snapshot; run it again for a fresh reading.
