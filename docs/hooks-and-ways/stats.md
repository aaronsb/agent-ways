# Stats and Observability

The ways system logs every fire, re-disclosure, near miss and relevance-gate verdict, and `ways tune stats` summarizes the log. Use it to see which guidance is actually reaching sessions.

## What gets logged

The `ways` binary appends one JSON line per event to `$XDG_STATE_HOME/agent-ways/events.jsonl`:

```json
{"ts":"2026-10-03T03:52:51Z","event":"way_fired","way":"softwaredev/delivery/commits","domain":"softwaredev","trigger":"semantic:bash:en","scope":"agent","project":"/home/you/myproject","session":"abc-123","token_position":"1204","model":"claude-opus-5-5","agent_id":"main","fire_score":"0.6220","surface":"git commit -m …"}
```

Besides `way_fired` and `way_redisclosed`, the log records ways that matched but were held back (`way_suppressed`), scores that fell just short (`way_nearmiss`), keyword hits vetoed by the semantic floor (`way_keyword_gated`), check fires, subagent suppression, and the relevance gate's verdicts, provider calls and fallbacks. [The event log](../reference/events.md) lists every event and field.

A way that matches is not always shown. With the relevance gate on (`gate.mode=enforce`), the judge can block a prompt-lane match, which logs `way_judged` with `verdict: block` and no `way_fired`. In `shadow` mode the verdict is `would_block` and the way still fires. The gate's design and evidence are in ADR-196 and [the probe report](../research/yesno-relevance-gate/README.md).

## Reading the stats

```bash
ways tune stats                 # this project (CLAUDE_PROJECT_DIR), all time
ways tune stats --days 7        # last 7 days
ways tune stats --project PATH  # one project and the paths under it
ways tune stats --global        # every project
ways tune stats --json          # machine-readable
```

Sample output, `ways tune stats --days 7 --global`, trimmed:

```
Ways of Working — Usage Stats

  Period:  last 7 days

  Sessions: 61  |  Way fires: 4883  |  Re-disclosures: 4381

Top ways:
  meta/deployment                147  ████████████████████
  softwaredev/delivery/github    140  ███████████████████
  documentation/adr               79  ██████████
  softwaredev/delivery/release    79  ██████████
  softwaredev/delivery/commits    77  ██████████

By trigger:
  semantic:bash:en 3170 (64%)
  keyword    543 (11%)
  semantic:embedding:en 400 (8%)
  bash       389 (7%)
  state      138 (2%)
  semantic:late-interaction:en 111 (2%)
  file        92 (1%)
  check-pull  38 (0%)

By model:
  Model           Fires Re-disclosures
  ────────────────────────────────────
  claude-opus-5-5  3952           4380
  (unstamped)       474              0
  unknown           457              1
  (unstamped): rows with no model: written before the field existed, or ways delivered at subagent dispatch.

Ways per hook invocation:
  Channel    Invocations    1   2   3  4+ Max
  ───────────────────────────────────────────
  bash              2107 1334 414 198 161  18
  prompt             529  287 130  45  67  17
  state              116  106   4   1   5   5
  file                69   51  15   2   1   5
  An invocation is the way_fired rows sharing session, agent, second, and channel.

Check fires: 588
  softwaredev/code               199
  softwaredev/environment/makefile 132
  softwaredev/delivery/groundwork  49
```

The full report also has "Top ways by model", the top ten ways with one column per model.

### How to read it

**Top ways** shows which guidance is active. A domain way that never appears either does not match your workflow or sits in a disabled domain.

**By trigger** shows how ways get activated. [Trigger values](../reference/events.md#trigger-values) explains each one. In the sample, most fires come from `semantic:bash:en`: shell commands matched against way descriptions.

**By model** splits fires and re-disclosures by the model that received them. `unknown` means the fire happened before the model could be read from the transcript, for example at subagent launch.

**Ways per hook invocation** shows how many ways one hook call fired, per channel. A long `4+` tail on one channel means a single command or prompt is pulling in many ways at once, which costs context.

**Re-disclosures** in the header count ways shown again after their `refire:` cadence let them. A high ratio of re-disclosures to fires on one way suggests its `refire:` is short for how often its trigger recurs.

**Check fires** counts checks shown, by parent way.

`--json` adds `by_scope`, `by_way_model`, `by_check`, `check_avg_distance` and `check_anchored`, which the text report leaves out. `ways projects` lists sessions per project.

### What the stats do not tell you

The stats show what fired, not whether it helped. A way that fires 140 times may be firing too broadly. A way that never fires may be waiting for work you have not done yet. Use the counts to spot noisy ways, dead ways and empty scopes, then use the audit commands below for precision and recall.

## Auditing the telemetry

`ways tune precision` reads the log and estimates, for each way, how often its fires landed in sessions whose other activity never touched the way's domain (ADR-134). It writes nothing and reports a heuristic flag, not a verdict. It separates two cases that a plain counter cannot: a **mis-targeted** way, a narrow way that keeps firing into the wrong kind of session, which you fix by narrowing its vocabulary or changing its trigger channel; and a **cross-cutting** way, one that fires broadly by design, which you scope by trigger and never narrow automatically. Flags: `--min-sessions` (default 5), `--flag-threshold` (default 0.5), `--project`, `--way`, `--json`.

```bash
ways tune precision
```

`ways session` opens the session screen, with tabs for fires, judge spend, stats and precision. `ways agent cost` sums the judge's provider calls.

Cadence is authored directly as `refire:`. No command tunes it from telemetry yet; that is the deferred part of ADR-134.

The near-miss band is set by `near_miss_margin` (default 0.05) in the ways config, beside the fire thresholds `semantic_fire_probability` (default 0.5) and `keyword_floor_probability` (default 0.15). It only changes what is logged, never what fires.

## Where the data lives

| File | Purpose |
|------|---------|
| `$XDG_STATE_HOME/agent-ways/events.jsonl` | Event log; [the event log](../reference/events.md) gives its fields and size cap |
| `/tmp/.claude-config-update-state-{uid}` | Update check cache |
| `{sessions root}/{session}/ways/{way id}/.marker.{agent}` | Fire markers, per session and agent |
| `{sessions root}/{session}/teammate` | Teammate scope marker, holding the team name |
| `{sessions root}/{session}/tasks-active` | Suppresses the context-threshold task reminder |

`ways sessions-root` prints the sessions root, such as `/run/user/1000/claude-sessions`.
