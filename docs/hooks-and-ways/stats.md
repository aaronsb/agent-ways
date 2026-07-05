# Stats and Observability

You can't manage what you can't see. The ways system logs every firing event and provides tools to understand how guidance is actually being applied across sessions, projects, and teams.

## What Gets Logged

Every time a way fires, `log-event.sh` appends a line to `$XDG_STATE/agent-ways/events.jsonl`:

```json
{"ts":"2026-02-05T19:00:34Z","event":"way_fired","way":"softwaredev/delivery/commits","domain":"softwaredev","trigger":"semantic:embedding:en","scope":"agent","project":"/home/you/myproject","session":"abc-123","token_position":"1204","fire_score":"0.6220"}
```

The `fire_score` field is the calibrated relevance probability `g(s)` that cleared the semantic fire threshold τ_s — not a raw cosine. The fire rule and the calibration that produces `g(s)` live in [engine-reference.md](engine-reference.md); this doc only records what the telemetry carries. `fire_score` is recorded on first-fires only (not redisclosures), because tuning is about what gates a way's *entry* to a session. This stream feeds the **deferred** ADR-134 auto-tune; the `g(s)` calibration itself is fit at corpus-generation from the committed `calibration_probes.jsonl`, **not** from this telemetry.

Session start events are also logged. For teammates, the team name is included:

```json
{"ts":"2026-02-05T19:01:12Z","event":"way_fired","way":"collaboration/teams","domain":"collaboration","trigger":"state","scope":"teammate","project":"/home/you/myproject","session":"def-456","team":"my-refactor-team"}
```

A way that *almost* fired is logged too. When a way's best calibrated probability lands within `near_miss_margin` below its effective semantic threshold τ_s but doesn't clear it, a `way_nearmiss` event records the silence:

```json
{"ts":"2026-02-05T19:02:08Z","event":"way_nearmiss","way":"softwaredev/architecture/design","corpus_id":"...","domain":"softwaredev","prob_en":"0.4791","prob_multi":"","tau_s":"0.5000","margin":"0.0209","trigger":"prompt","scope":"agent","project":"/home/you/myproject","session":"abc-123","query_tokens":"42"}
```

These are a recall signal — they measure the likely false silences (ADR-134 Decision 1). `prob_en`/`prob_multi` are the per-model calibrated relevance probabilities `g(s)`; `tau_s` is the single semantic fire threshold both lanes are measured against — one threshold in probability space, not a per-model cosine bar (see [engine-reference.md](engine-reference.md)). `margin` is how far under τ_s the best model landed, and `query_tokens` the size of the prompt that nearly matched. An empty probability field means that model didn't score.

A pattern hit that was vetoed by the keyword floor is logged too. When a `pattern:` regex matches but the way's calibrated probability sits below the keyword floor τ_k on every model lane (ADR-155), a `way_keyword_gated` event records the vetoed lexical coincidence: `matched_span` (the alternation that matched), `prob_en`/`prob_multi`, and `floor` (τ_k), alongside the same `way`/`corpus_id`/`domain`/`trigger`/`scope`/`project`/`session` fields plus `token_position`. `pattern_strict` ways never appear here — they bypass the gate by design.

The log is JSONL — each line is self-contained, and nothing reads the file during normal operation; it's purely for after-the-fact analysis. Growth is bounded: when `events.jsonl` exceeds ~32 MiB, `log_event` tail-compacts it down to the most recent ~24 MiB (cut at a line boundary, written to a temp file and atomically renamed). This is lossy on the oldest events but leaves readers untouched.

## Reading the Stats

Run the stats tool:

```bash
bash ~/.claude/hooks/ways/stats.sh
```

### Sample Output

```
Ways of Working - Usage Stats
==============================
Period: 2026-02-05 → 2026-02-06

Sessions: 11  |  Way fires: 458

Top ways:
  meta/todos                      96  ████████████████████
  meta/memory                     96  ████████████████████
  meta/knowledge                  30  ██████
  softwaredev/delivery/commits    19  ███
  softwaredev/architecture/design 18  ███

By scope:
  agent        319
  unknown       34
  teammate      32
  subagent      27

By team:
  adr-500-spec                    26 fires

By trigger:
  state      197 (43%)
  bash        76 (16%)
  prompt      71 (15%)

By project:
  ~/Projects/ai/knowledge-graph    391 fires (20 sessions)
  ~/.claude                         55 fires (10 sessions)

Last 24h: 11 sessions, 458 way fires
```

### How to Interpret It

**Top ways** tells you which guidance is actually active. If `meta/todos` and `meta/memory` dominate, your sessions are long enough to hit context thresholds regularly — the system is doing its job keeping state persistent. If a domain-specific way never appears, either the trigger patterns don't match your workflow or the domain is disabled.

**By scope** shows who's receiving guidance. `agent` is your main sessions. `teammate` means team members got their coordination norms. `subagent` means delegated tasks received relevant ways. `unknown` is from older events logged before scope tracking existed — it ages out naturally.

**By team** appears only when teams have been used. It shows which teams triggered the most ways. A team with many fires was doing complex, varied work. A team with few fires was focused on something narrow that only matched a couple of ways.

**By trigger** reveals *how* ways are being activated:
- `state` — context-threshold and session-start triggers (the system managing itself)
- `bash` — command matching (git commit, npm install, etc.)
- `prompt` — keyword/semantic matching against what you typed
- `file` — file path matching (editing .env, README.md, etc.)
- `teammate`/`subagent` — injected into spawned agents

**By project** shows guidance distribution across your work. A project with many fires is one where the ways are highly relevant. A project with few fires either doesn't trigger many patterns or has a focused workflow.

### Filtering

```bash
# Last 7 days
bash ~/.claude/hooks/ways/stats.sh --days 7

# Single project
bash ~/.claude/hooks/ways/stats.sh --project /path/to/project

# Machine-readable JSON
bash ~/.claude/hooks/ways/stats.sh --json

# Project overview (sessions, memory, way fires per project)
bash ~/.claude/hooks/ways/stats.sh --projects
```

### JSON Output

The `--json` flag returns structured data for tooling:

```json
{
  "total_events": 458,
  "sessions": 11,
  "way_fires": 412,
  "by_way": {"meta/todos": 96, "meta/memory": 96, ...},
  "by_trigger": {"state": 197, "bash": 76, ...},
  "by_scope": {"agent": 319, "teammate": 32, ...},
  "by_team": {"adr-500-spec": 26},
  "by_project": {"/home/you/project": 391, ...}
}
```

## What the Stats Don't Tell You

The stats show *what fired*, not *whether it helped*. A way that fires 96 times isn't necessarily 96 times useful — it might be triggering too broadly. A way that never fires isn't necessarily broken — it might be waiting for a workflow you haven't hit yet. `stats.sh` reports the counts; the `fire_score` and `way_nearmiss` telemetry it doesn't summarize feeds the audit commands below, which is where the precision and recall questions get answered.

Use the stats to spot patterns: ways that fire too often (noisy triggers), ways that never fire (dead patterns or disabled domains), scopes that are unexpectedly empty (scope gating too aggressive), and teams that generate unusual activity (worth investigating what they were doing).

## Auditing the Telemetry

Two report-only commands read the event log and turn it back on the ways that produced it (ADR-134). Both write nothing — they surface a heuristic flag, not a verdict.

`ways tune-precision` is a relevance audit. For each way it estimates how often its fires landed *off-class* — in sessions whose actual activity (judged by the parent-family of the ways that co-fired) never touched the way's own domain — and reports an irrelevance rate. It separates two failure modes that look identical to a naive counter: **mis-targeted** (a narrow way repeatedly firing into the same wrong kind of session — remedy: narrow the vocabulary or change the trigger channel, then re-measure; there is no per-way threshold to raise) versus **cross-cutting** (a way that fires broadly by design, like `meta/tracking` — remedy: scope by trigger, and *never* auto-narrow its vocabulary). Flags: `--min-sessions` (default 5), `--flag-threshold` (default 0.5), `--project`, `--way`, `--json`.

```bash
ways tune-precision
```

There is no companion cadence-tuning command: `ways tune-curves` was removed in ADR-159. It suggested an absolute ADR-123 `half_life` (a legacy `curve:` block the schema no longer recognizes), a model superseded by `refire:` (a fraction of the context window, ADR-126). Cadence is now authored directly as `refire:` (numeric fraction or a preset like `normal`); telemetry-driven cadence tuning is the deferred ADR-134 auto-tune, which will target `refire:` rather than `half_life`.

The width of the near-miss band these tools consume is set by the `near_miss_margin` config knob (default 0.05), parsed from the ways config alongside the fire thresholds `semantic_fire_probability` (τ_s, default 0.5) and `keyword_floor_probability` (τ_k, default 0.15). It's purely a logging knob — it never changes which ways fire, only how far below τ_s a silence has to land before it's worth recording.

## Where the Data Lives

| File | Purpose |
|------|---------|
| `$XDG_STATE/agent-ways/events.jsonl` | Event log (tail-compacted at ~32 MiB to ~24 MiB) |
| `/tmp/.claude-config-update-state-{uid}` | Update check cache (hourly) |
| `{SESSIONS_ROOT}/{session}/ways/{way_path}/.marker` | Way firing markers (per-session) |
| `{SESSIONS_ROOT}/{session}/teammate` | Teammate scope marker (contains team name) |
| `{SESSIONS_ROOT}/{session}/tasks-active` | Context-threshold nag suppression |
