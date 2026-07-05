# Hooks and Ways System

How contextual guidance gets injected into Claude Code sessions.

## Hook Events

Claude Code hook events drive the system. Each fires shell scripts that scan for matching ways and inject their content.

| Event | When | Scripts |
|-------|------|---------|
| **SessionStart** (startup) | Fresh session | `clear-markers.sh`, `check-config-updates.sh`, `check-state.sh`, `ways init`, `ways corpus` |
| **SessionStart** (compact) | After compaction | `clear-markers.sh`, `check-state.sh` |
| **SessionStart** (resume) | Session resume | `check-state.sh` |
| **UserPromptSubmit** | Every user message | `check-prompt.sh`, `check-state.sh` |
| **PreToolUse** (Edit\|Write) | Before file edit | `check-file-pre.sh` |
| **PreToolUse** (Bash) | Before command | `check-bash-pre.sh` |
| **PreToolUse** (Task) | Before subagent spawn | `check-task-pre.sh` |
| **PreToolUse** (TaskCreate) | Before task creation | `mark-tasks-active.sh` |
| **SubagentStart** | When subagent starts | `inject-subagent.sh` |
| **SessionStart** (clear) | After `/clear` | `clear-markers.sh`, `check-state.sh`, `ways init` |
| **Stop** | After Claude responds | `check-response.sh` |

## What Each Script Does

### Session Lifecycle

- **`clear-markers.sh`** - Clears session markers from `{SESSIONS_ROOT}/{session_id}/`. Resets session state so ways can fire fresh. Scoped to the current session only.
- **`ways init`** - Creates `$PROJECT/.claude/ways/_template.md` if the project has a `.claude/` or `.git/` dir but no ways directory yet.
- **`ways corpus --if-stale --quiet`** - Regenerates the embedding corpus if way files have changed since last build.
- **`check-config-updates.sh`** - Checks if the config is behind upstream. Detects four install scenarios: direct clones, GitHub forks, renamed clones (via `.claude-upstream` marker file), and plugin installs. Network calls (`git fetch`, `gh api`, `git ls-remote`) are rate-limited to once per hour; update notices fire every session when behind. See the [Updating](#updating) section of the README for scenario details and how to control this behavior.

### Trigger Evaluation

These scripts fire on **PreToolUse** — before the tool executes, not after. This is a critical design choice: guidance must arrive while Claude can still act on it. A commit format reminder after the commit is too late. Security guidance after the file edit is too late. The "Pre" in PreToolUse means Claude sees the way content and can adjust its behavior before the action happens.

- **`check-prompt.sh`** - Thin dispatcher to `ways scan prompt`. Passes the user prompt (plus response topics from the previous turn) and session ID. The `ways` binary handles all matching: file walking, frontmatter extraction, pattern + semantic matching, scope/precondition gating, parent threshold lowering, session markers, macro dispatch, and content output.
- **`check-bash-pre.sh`** - Scans ways for `commands:` patterns. Tests the command about to run. Also checks `pattern:` against the command description.
- **`check-file-pre.sh`** - Scans ways for `files:` patterns. Tests the file path about to be edited.
- **`check-state.sh`** - Evaluates `trigger:` fields (context-threshold, file-exists, session-start). See [State Triggers](#state-triggers).

All trigger evaluation scripts respect the `scope:` frontmatter field - ways without `agent` scope are skipped.

### Subagent Injection

- **`check-task-pre.sh`** - PreToolUse:Task hook (Phase 1). Reads the Task tool's `prompt` parameter, runs inline matching for `scope: subagent` ways. Writes matched way paths to `{SESSIONS_ROOT}/{session_id}/subagent-stash/`. Never blocks Task creation.
- **`inject-subagent.sh`** - SubagentStart hook (Phase 2). Reads the oldest stash file, claims it atomically, emits way content as JSON `hookSpecificOutput.additionalContext`. Bypasses markers entirely - subagents get fresh context regardless of what the parent triggered.

### State Management

- **`mark-tasks-active.sh`** - Creates `{SESSIONS_ROOT}/{session_id}/tasks-active`. Silences the context-threshold nag.
- **`check-response.sh`** - Extracts technical keywords from Claude's last response, writes to `/tmp/claude-response-topics-{session_id}`. These topics feed back into `check-prompt.sh` on the next turn, so ways can trigger based on what Claude discussed (not just what the user asked).

### Way Display

The `ways` binary (`ways scan` / `ways show`) handles all way display: domain disable list, session markers, macro dispatch, content output (stripping frontmatter), and marker creation. Per-way `macro.sh` scripts still run as shell commands, but orchestration is in Rust.

## Session Lifecycle

```mermaid
sequenceDiagram
    participant CC as Claude Code
    participant CM as clear-markers.sh
    participant CS as check-state.sh
    participant WI as ways init
    participant WC as ways corpus
    participant Ctx as Claude Context

    rect rgba(66, 165, 245, 0.15)
        Note over CC,Ctx: Session Start (startup)
        CC->>CM: SessionStart:startup
        CM->>CM: rm {SESSIONS_ROOT}/{session_id}/*
        CC->>CS: SessionStart:startup
        CS->>Ctx: core guidance + state-triggered ways
        CC->>WI: SessionStart:startup
        WI->>WI: create $PROJECT/.claude/ways/_template.md
        CC->>WC: SessionStart:startup
        WC->>WC: regenerate corpus if stale
    end

    rect rgba(255, 152, 0, 0.15)
        Note over CC,Ctx: After Compaction
        CC->>CM: SessionStart:compact
        CM->>CM: rm {SESSIONS_ROOT}/{session_id}/*
        CC->>CS: SessionStart:compact
        CS->>Ctx: core guidance (fresh)
    end
```

## Way Scope

The `scope:` frontmatter field controls where a way fires. There are three scopes, reflecting the three types of Claude Code sessions:

| Scope | Session type | Detection |
|-------|-------------|-----------|
| `agent` | Your main session | Default (no marker file) |
| `teammate` | Named agent in a coordinated team | `{SESSIONS_ROOT}/{session_id}/teammate` exists |
| `subagent` | Quick Task tool delegate | Spawned via Task without `team_name` |

Ways declare which scopes they apply to:

```yaml
scope: agent                     # Main session only (default if omitted)
scope: teammate                  # Team members only
scope: agent, teammate           # Both, but not quick delegates
scope: agent, subagent           # Main + delegates, not teammates
scope: agent, teammate, subagent # Everyone
```

### Scope Detection

Scope detection is handled by the `ways` binary. It checks for a teammate marker file — if one exists, the scope is `teammate`; otherwise `agent`. Subagent scope is determined at injection time by `check-task-pre.sh`, not by the running session itself.

The teammate marker is created by `inject-subagent.sh` during Phase 2 of the two-phase injection. It persists for the teammate's entire session lifetime and contains the team name (used for telemetry).

### What Gets Gated

| Way | Scope | Why |
|-----|-------|-----|
| `meta/memory` | `agent` | Prevents concurrent MEMORY.md writes from multiple teammates |
| `meta/subagents` | `agent` | Delegation guidance is irrelevant to agents that are themselves delegated work |
| `collaboration/teams` | `teammate` | Coordination norms only make sense for team members |

Subagent injection bypasses the marker system entirely. A way can fire for the parent (marker-gated) AND separately for each subagent or teammate (no markers). The parent's way guidance doesn't automatically transfer because the Task prompt is a compact delegation — the scope system bridges this gap.

See [teams.md](hooks-and-ways/teams.md) for the full team coordination model.

## Way Matching Modes

Each way declares how it should be matched in its YAML frontmatter. There are two lanes and they are additive-OR: a way with both a `pattern:` and a `description:` + `vocabulary:` can fire from either.

- **Keyword lane** — the regex `pattern:` (matched against the user prompt), plus the deterministic `commands:` and `files:` triggers on the tool surfaces.
- **Semantic lane** — the prompt is embedded and scored by cosine against the way's alias (`description` + `vocabulary`); the cosine is mapped to a relevance probability by a calibrated logistic and compared to a global threshold.

The exact fire rule, thresholds, and calibration are stated once in
[hooks-and-ways/engine-reference.md](hooks-and-ways/engine-reference.md) — the
single source of truth. The summary below must agree with it.

```mermaid
flowchart TD
    classDef regex fill:#2E7D32,stroke:#1B5E20,color:#fff
    classDef semantic fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef decision fill:#E65100,stroke:#BF360C,color:#fff
    classDef result fill:#00695C,stroke:#004D40,color:#fff

    W["{name}.md frontmatter"]
    W -->|"pattern: / commands: / files:"| R
    W -->|"description: + vocabulary:"| S

    subgraph RX ["Keyword lane"]
        R[Regex Match]:::regex
        R --> RP["pattern: → user prompt"]:::regex
        R --> RC["commands: → bash command"]:::regex
        R --> RF["files: → file path"]:::regex
    end

    subgraph SM ["Semantic lane"]
        S[Embedding Scorer]:::semantic
        S --> BM["ways embed → cosine s<br/>g(s)=σ(a·s+b) → probability"]:::semantic
    end

    RP --> GATE{"g(s) ≥ τ_k ?<br/>floor gate<br/>(fails open / pattern_strict bypasses)"}:::decision
    GATE -->|"yes"| FIRE[Fire Way]:::result
    RC -->|"exact match"| FIRE
    RF -->|"exact match"| FIRE
    BM -->|"g(s) ≥ τ_s"| FIRE
```

Matching is **additive** — the keyword and semantic lanes are OR'd. A gated keyword never shadows a semantic fire: the semantic lane is checked first and the gated verdict is only reported if nothing cleared `τ_s`.

### Pattern

```yaml
pattern: commit|push          # matched against user prompt
commands: git\ commit         # matched against bash commands
files: \.env$|config\.json    # matched against file paths
```

Fast and precise. Most ways use this. Keyword matching is **case-sensitive** against the original-case prompt — only code fences and URLs are stripped, no lowercasing.

The prompt `pattern:` hit is **floor-gated**: it fires only when the way's calibrated probability also clears the keyword floor `τ_k` on at least one model lane, so a lexical coincidence can't drag in an unrelated prompt. Two carve-outs let the author's explicit trigger stand: the gate **fails open** when there is genuinely no calibrated signal (the engine didn't run, the way isn't embeddable, or no calibration is loaded), and `pattern_strict: true` forces an unconditional keyword fire by design (`scan/mod.rs` `match_prompt`).

### Semantic Matching

```yaml
description: "API design, REST endpoints, request handling"
vocabulary: api endpoint route handler middleware
```

There is **no** `embed_threshold` frontmatter field and **no** per-way threshold — firing is decided by global thresholds in probability space (see below).

Embedding-only engine, built into the `ways` binary:

| Model | How it works |
|-------|-------------|
| **EN** | `all-MiniLM-L6-v2` sentence embeddings via the `way-embed` binary + GGUF model. Pre-computed 384-dim vectors in the corpus. Cosine similarity against all ways (~20ms). |
| **Multilingual** | 768-dim model for localized mode. Routes native-language queries through locale-stub aliases without per-language stemmer wiring. |

The embedding model is a hard dependency of `ways`. `make setup` fetches the binary and GGUF model on four supported platforms.

**Embedding engine** (ADR-108, ADR-125): Semantic similarity captures concepts that lexical scoring would miss — "SSH agent" and "AI agent" share the same English stem but have distant embedding vectors, and the multilingual variant routes native-language queries through locale-stub aliases without per-language stemmer wiring.

#### Calibration and the fire rule

A raw cosine is not the firing signal. ADR-156 maps each model's cosine `s` to a
**relevance probability** with a per-model logistic `g(s) = σ(a·s + b)`, fit at
corpus-generation from a committed probe corpus and stored in
`embed-manifest.json` (deployed EN `AUC ≈ 0.955`, multi `AUC ≈ 0.941`). A fit is
rejected unless its slope `a > 0` and it clears an `AUC_FLOOR` of 0.70; a bad fit
is not written, and scan then degrades (keyword fails open, semantic silent)
rather than trust it.

A way **fires** when `g(s) ≥ τ_s ∨ (keyword_match ∧ g(s) ≥ τ_k)`, with global
`τ_s = 0.5` (`semantic_fire_probability`) and `τ_k = 0.15`
(`keyword_floor_probability`). The two thresholds are **independent** — a leaky
keyword is tightened by raising `τ_k` without touching the semantic bar `τ_s`.
Because calibration makes the boundary comparable across ways, one global
probability suffices where per-way cosine thresholds were once needed.

**Parent-boost.** When an ancestor way has already been shown this session, an
in-domain child's semantic bar is lowered from `τ_s` to
`(τ_s × parent_threshold_multiplier).max(parent_boost_floor)` — by default
`max(0.5 × 0.8, 0.30) = 0.40`. Both keys are live: the multiplier (0.8) lowers
the child's bar; the floor (0.30) stops cascading boosts from reaching the noise
band. `τ_k` is not parent-boosted.

The authoritative statement of all of the above, with source line citations, is
[hooks-and-ways/engine-reference.md](hooks-and-ways/engine-reference.md).

#### Setup

```bash
# Install the ways binary and set up corpus + embedding model
make install    # builds ways, downloads model, generates corpus
make test       # smoke tests (lint, match, graph)
make test-sim   # 8 integration scenarios

# Check engine status
ways status
```

Model location: `${XDG_CACHE_HOME:-~/.cache}/agent-ways/user/minilm-l6-v2.gguf`
Corpus: `${XDG_CACHE_HOME:-~/.cache}/agent-ways/user/ways-corpus.jsonl`

Both user-scope (`~/.claude/hooks/ways/`) and project-scope (`.claude/ways/`) corpora are scanned. They share the same model file.

#### Calibrating from telemetry

Once a corpus has been firing for a while, two commands audit and calibrate the embedding match against observed behavior rather than guesswork (ADR-134).

```bash
# Audit fire relevance — flag ways landing in off-domain sessions
ways tune-precision

# Calibrate firing-dynamics curves from observed cadence
ways tune-curves
```

`ways tune-precision` is a report-only relevance audit. For each way it estimates how often its fires landed *off-class* — in sessions whose activity (judged by the parent-family of the ways that co-fired) never touched the way's own domain — and reports an irrelevance rate plus a flag. **mis-targeted** is a narrow way repeatedly firing into the same wrong kind of session (remedy: narrow its vocabulary, tighten its `pattern:`, or change the trigger channel, then re-measure — there is no per-way threshold to move; a globally leaky keyword is tightened by raising `τ_k`); **cross-cutting** is a way that fires broadly by design, e.g. meta/tracking ways (remedy: scope by trigger — never auto-narrow vocabulary). Flags: `--min-sessions` (default 5), `--flag-threshold` (default 0.5), `--project`, `--way`, `--json`.

`ways tune-curves` (ADR-123 Phase E) groups `way_fired`/`way_redisclosed` events by (way, session), computes token-position deltas between consecutive fires, and suggests a `half_life` ≈ the median delta. Dry-run by default; `--apply` rewrites the `curve:` block in place via line surgery. Vocabulary and matching metadata are never auto-applied — they stay authorial.

## State Triggers

Evaluated by `check-state.sh` on every UserPromptSubmit. Unlike pattern-based ways, these fire based on session conditions.

### context-threshold

```yaml
trigger: context-threshold
threshold: 75
```

Estimates transcript size since last compaction (~4 chars/token, ~155K token window = ~620K chars). Fires when `transcript_bytes > 620K * threshold%`.

**Special behavior**: Does not use the standard marker system. Repeats on every prompt until a `{SESSIONS_ROOT}/{session_id}/tasks-active` marker exists (created by `mark-tasks-active.sh` when `TaskCreate` is used).

### file-exists

```yaml
trigger: file-exists
path: .claude/todo-*.md
```

Fires once (standard marker) if the glob pattern matches any file relative to the project directory.

### session-start

```yaml
trigger: session-start
```

Always evaluates true. Uses standard marker, so fires exactly once per session on the first UserPromptSubmit.

## Once-Per-Session Gating

Most ways fire once then go silent for the rest of the session.

```mermaid
stateDiagram-v2
    classDef notShown fill:#C62828,stroke:#B71C1C,color:#fff,font-weight:bold
    classDef shown fill:#2E7D32,stroke:#1B5E20,color:#fff,font-weight:bold

    [*] --> NotShown
    NotShown --> Shown : trigger match → output + create marker
    Shown --> Shown : trigger match → no-op

    state "not_shown (no marker)" as NotShown:::notShown
    state "shown (marker exists)" as Shown:::shown

    note right of NotShown : {SESSIONS_ROOT}/{session_id}/ways/{way_path}/.marker
    note right of Shown : Cleared on SessionStart (startup & compact)
```

**Exceptions**:
- Context-threshold triggers bypass this system entirely - they repeat until the tasks-active marker exists.
- Subagent injection (`inject-subagent.sh`) bypasses markers completely - each subagent gets fresh way content.

## The Context-Threshold Nag

The `meta/todos` way uses context-threshold to ensure task lists exist before compaction.

```mermaid
sequenceDiagram
    participant U as User
    participant CC as Claude Code
    participant CS as check-state.sh
    participant MT as mark-tasks-active.sh
    participant Ctx as Claude Context

    rect rgba(244, 67, 54, 0.12)
        Note over U,Ctx: Context > 75%, no task list
        U->>CC: (any prompt)
        CC->>CS: UserPromptSubmit
        CS->>CS: transcript_bytes > 465K?
        Note right of CS: YES
        CS->>CS: {SESSIONS_ROOT}/{session}/tasks-active exists?
        Note right of CS: NO
        CS->>Ctx: "Context checkpoint. Create tasks now."
    end

    rect rgba(244, 67, 54, 0.12)
        Note over U,Ctx: Still no task list — nags again
        U->>CC: (any prompt)
        CC->>CS: UserPromptSubmit
        CS->>CS: transcript_bytes > 465K?
        Note right of CS: YES
        CS->>CS: {SESSIONS_ROOT}/{session}/tasks-active exists?
        Note right of CS: NO
        CS->>Ctx: "Context checkpoint. Create tasks now."
    end

    rect rgba(76, 175, 80, 0.15)
        Note over U,Ctx: Claude creates tasks — nag stops
        CC->>CC: TaskCreate (tool call)
        CC->>MT: PreToolUse:TaskCreate
        MT->>MT: touch {SESSIONS_ROOT}/{session}/tasks-active
    end

    rect rgba(76, 175, 80, 0.15)
        Note over U,Ctx: Subsequent prompts — silence
        U->>CC: (any prompt)
        CC->>CS: UserPromptSubmit
        CS->>CS: transcript_bytes > 465K?
        Note right of CS: YES
        CS->>CS: {SESSIONS_ROOT}/{session}/tasks-active exists?
        Note right of CS: YES — skip
    end
```

## Full Data Flow

```mermaid
sequenceDiagram
    participant U as User
    participant CC as Claude Code
    participant CP as check-prompt.sh
    participant CS as check-state.sh
    participant CB as check-bash-pre.sh
    participant CF as check-file-pre.sh
    participant CT as check-task-pre.sh
    participant IS as inject-subagent.sh
    participant MA as mark-tasks-active.sh
    participant CR as check-response.sh
    participant Ctx as Claude Context

    rect rgba(21, 101, 192, 0.12)
        Note over U,Ctx: User sends message
        U->>CC: prompt
        par Prompt Triggers
            CC->>CP: UserPromptSubmit
            CP->>CP: ways scan prompt (regex/semantic, scope: agent)
            CP->>Ctx: way content (if not already shown)
        and State Triggers
            CC->>CS: UserPromptSubmit
            CS->>CS: evaluate triggers (scope: agent)
            CS->>Ctx: context-threshold nag (if applicable)
        end
    end

    rect rgba(106, 27, 154, 0.12)
        Note over U,Ctx: Claude uses tools
        alt Bash command
            CC->>CB: PreToolUse:Bash
            CB->>CB: scan commands: patterns (scope: agent)
            CB->>Ctx: matching way content
        else File edit
            CC->>CF: PreToolUse:Edit|Write
            CF->>CF: scan files: patterns (scope: agent)
            CF->>Ctx: matching way content
        else Subagent spawn
            CC->>CT: PreToolUse:Task
            CT->>CT: scan task prompt (scope: subagent)
            CT->>CT: stash matched way paths
        else Task creation
            CC->>MA: PreToolUse:TaskCreate
            MA->>MA: touch tasks-active marker
        end
    end

    rect rgba(0, 105, 92, 0.12)
        Note over U,Ctx: Subagent starts (if Task was used)
        CC->>IS: SubagentStart
        IS->>IS: read stash → emit way content
        IS->>Ctx: additionalContext (subagent sees ways)
    end

    rect rgba(230, 81, 0, 0.12)
        Note over U,Ctx: Claude finishes responding
        CC->>CR: Stop
        CR->>CR: extract keywords from response
        CR->>CR: write /tmp/claude-response-topics-*
        Note right of CR: Topics feed into next check-prompt.sh
    end
```

## Telemetry

Firing activity is logged to `$XDG_STATE/agent-ways/events.jsonl` — one JSON object per line (legacy installs may still read `~/.claude/stats/events.jsonl`). Beyond the `way_fired`/`way_redisclosed` cadence events that `tune-curves` reads, two signals feed the precision and recall tuning above (ADR-134):

- **`fire_score`** — recorded on `way_fired` events for **first-fires only** (not redisclosures): the calibrated probability `g(s)` that cleared the threshold and admitted the way to the session. It is the raw material the ADR-156 calibration is fit from (`show/mod.rs`).
- **`way_nearmiss`** — emitted when a way scored within `near_miss_margin` *below* its effective semantic threshold `τ_s` but did **not** fire (`τ_s - margin ≤ p < τ_s`). Score fields: `prob_en`, `prob_multi`, `tau_s`, `margin`; plus `trigger`, `query_tokens`, and the `way_fired`-convention identity fields (`event`, `way`, `corpus_id`, `domain`, `scope`, `project`, `session`). This is a recall signal — it measures the likely false silences a `τ_s` drop would recover (`scan/mod.rs` `log_near_miss`).

`near_miss_margin` (default `0.05`) is a purely-logging config knob — it never changes firing — parsed from the ways config YAML alongside `semantic_fire_probability` and `keyword_floor_probability` (`config.rs`).

The near-miss and fire-score streams grow the log faster than fires alone, so its size is **bounded**: `log_event` tail-compacts `events.jsonl` once it exceeds `MAX_EVENTS_BYTES` (~32 MiB), keeping the most recent `KEEP_EVENTS_BYTES` (~24 MiB, cut at a line boundary, written atomically via temp + rename). The ~8 MiB gap provides hysteresis so the rewrite is rare, not per-append. Compaction is lossy only on the oldest events; readers are unaffected (`session.rs`).

ADR-134 ("Empirical auto-tuning from fire and near-miss telemetry") is Accepted. One slice — the gated `--apply` that would auto-write a tuned threshold — is deferred until the `fire_score` population accumulates enough to validate against, tracked as GitHub issue #123.

## Macros

Ways can include a `macro.sh` alongside the way file. Frontmatter declares positioning:

```yaml
macro: prepend   # macro output before static content
macro: append    # macro output after static content
```

Macros generate dynamic content. Examples:
- `documentation/adr/macro.sh` - Tri-state detection: no tooling, tooling available, tooling installed
- `softwaredev/code/quality/macro.sh` - Scans for long files in the project, outputs priority list
- `softwaredev/delivery/github/macro.sh` - Detects solo vs team project, adjusts PR guidance

**Security**: Project-local macros only run if the project is listed in `~/.claude/trusted-project-macros`.

## Project-Local Ways

Projects can override or add ways at `$PROJECT/.claude/ways/{domain}/{way}/{way}.md`. Project-local takes precedence over global. Same-path ways share a marker, so only one fires.

```mermaid
flowchart TD
    classDef project fill:#E65100,stroke:#BF360C,color:#fff
    classDef global fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef marker fill:#00695C,stroke:#004D40,color:#fff
    classDef result fill:#2E7D32,stroke:#1B5E20,color:#fff

    T["Trigger fires for softwaredev/delivery/github"] --> PL

    PL{"$PROJECT/.claude/ways/<br/>softwaredev/delivery/github/github.md<br/>exists?"}
    PL -->|yes| USE_P["Use project-local way"]:::project
    PL -->|no| GL{"~/.claude/hooks/ways/<br/>softwaredev/delivery/github/github.md<br/>exists?"}:::global
    GL -->|yes| USE_G["Use global way"]:::global
    GL -->|no| SKIP["No output"]

    USE_P --> MK["Shared marker:<br/>{SESSIONS_ROOT}/{session}/softwaredev-github"]:::marker
    USE_G --> MK
    MK --> OUT["Output way content"]:::result
```

## Domain Enable/Disable

`$XDG_CONFIG_HOME/agent-ways/config.yaml` controls which domains are active (a legacy `$XDG_CONFIG_HOME/ways/config.yaml`, and `~/.claude/ways.json` with `{"disabled": [...]}`, are still honored):

```yaml
disabled_domains:
  - itops
```

Checked by `ways scan` before outputting any way.

## Testing

Three test layers verify the matching and injection pipeline. See [tests/README.md](../tests/README.md) for full details.

| Layer | Command | What it tests |
|-------|---------|---------------|
| **Smoke** | `make test` | Lint (0 errors), match (sample queries), graph (node/edge count) |
| **Simulation** | `make test-sim` | 8 integration scenarios: matching, idempotency, commands, files, checks, disclosure, scope, epochs |
| **Activation** | `read and run the activation test at tests/way-activation-test.md` | Live hook pipeline: regex, embedding semantic match, negative control, subagent injection |

The `/ways-tests` skill provides ad-hoc scoring for vocabulary tuning:

```
/ways-tests "write some unit tests for this module"
```
