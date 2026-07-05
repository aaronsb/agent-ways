# Ways System Architecture

Visual documentation of the ways trigger system.

## How a Session Flows

A typical session from the user's perspective, showing how events trigger way injections at each step:

```mermaid
sequenceDiagram
    participant U as 👤 User
    participant C as 🤖 Claude
    participant W as ⚡ Ways System
    participant S as 🔧 Subagent

    Note over U,S: Session starts — core guidance loads

    rect rgba(21, 101, 192, 0.15)
        Note over U,C: User describes their task
        U->>C: "Let's fix the auth bug and<br/>add tests for the login flow"
        W-->>C: 🔑 Security way injected (keyword: auth)
        W-->>C: 🧪 Testing way injected (keyword: tests)
        W-->>C: 🐛 Debugging way injected (keyword: bug)
        Note right of C: Claude now has security, testing,<br/>and debugging guidance in context
    end

    rect rgba(106, 27, 154, 0.15)
        Note over C,W: Claude uses tools — ways intercept before execution
        C->>W: about to run: git log --oneline auth/
        Note right of W: No way matches → command proceeds
        C->>W: about to edit: src/auth/login.ts
        W-->>C: ⚙️ Config way injected (PreToolUse: file match)
        Note right of C: Guidance arrives before the edit happens
    end

    rect rgba(0, 105, 92, 0.15)
        Note over C,S: Claude delegates to a subagent
        C->>W: about to spawn: Task("Review auth<br/>for security vulnerabilities")
        W-->>W: Stash matched ways (PreToolUse:Task)
        C->>S: Subagent starts
        W-->>S: 🔑 Security way injected (SubagentStart)
        W-->>S: 🐛 Debugging way injected (SubagentStart)
        Note right of S: Subagent has its own way context
        S-->>C: Review findings
    end

    rect rgba(230, 81, 0, 0.15)
        Note over C,W: Macro tailors guidance to project context
        C->>W: about to run: gh pr create
        W->>W: macro.sh → queries GitHub API
        W-->>C: 🔀 GitHub way injected (PreToolUse:Bash)<br/>"Team project (4 contributors) — PR recommended"
        Note right of C: Claude sees team context before<br/>the command executes
    end

    rect rgba(21, 101, 192, 0.15)
        Note over U,C: User continues — ways stay quiet
        U->>C: "Now let's also check the tests"
        Note right of W: Testing way already shown → silent
        Note right of C: No new injections — markers prevent repeats
    end

    Note over U,S: ↻ This cycle continues until context fills up

    rect rgba(198, 40, 40, 0.15)
        Note over U,S: Auto-compact triggers — all markers cleared, ways reset
        W->>W: clear-markers.sh → rm /tmp/.claude-sessions/{session_id}/*
        W-->>C: Core guidance reloads (fresh session state)
        Note right of C: All ways can fire again on next match
    end
```

## Hook Flow

How ways get triggered during a Claude Code session:

```mermaid
flowchart TB
    classDef event fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef script fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef match fill:#00695C,stroke:#004D40,color:#fff
    classDef gate fill:#E65100,stroke:#BF360C,color:#fff
    classDef output fill:#2E7D32,stroke:#1B5E20,color:#fff
    classDef silent fill:#78909C,stroke:#546E7A,color:#fff

    subgraph Session["Claude Code Session"]
        SS[SessionStart]:::event --> Core["ways show core<br/>Dynamic table + core.md"]:::script

        UP[UserPromptSubmit]:::event --> CP["check-prompt.sh → ways scan<br/>Regex · Embedding"]:::script

        subgraph PreTool["PreToolUse"]
            Bash[Bash tool]:::event --> CB["check-bash-pre.sh"]:::script
            EditW[Edit/Write tool]:::event --> CF["check-file-pre.sh"]:::script
            Task[Task tool]:::event --> CT["check-task-pre.sh"]:::script
        end

        SA[SubagentStart]:::event --> IS["inject-subagent.sh"]:::script
    end

    CP --> Check{Marker?}:::gate
    CB --> Check
    CF --> Check

    Check -->|No| Output["Output way content<br/>Create marker"]:::output
    Check -->|Yes| Silent["No-op"]:::silent

    CT -->|"scope: subagent"| Stash["Write stash file"]:::output
    IS -->|read stash| Emit["Emit way content<br/>(bypass markers)"]:::output
```

## Subagent Injection

Two-phase stash pattern bridges the gap between Task prompt visibility and SubagentStart injection:

```mermaid
sequenceDiagram
    participant A as Main Agent
    participant CT as check-task-pre.sh
    participant S as Stash File
    participant CC as Claude Code
    participant IS as inject-subagent.sh
    participant SA as Subagent

    rect rgba(21, 101, 192, 0.15)
        Note over A,CT: Phase 1: PreToolUse:Task
        A->>CC: Task(prompt: "Review PR for security...")
        CC->>CT: PreToolUse:Task
        CT->>CT: Scan ways with scope: subagent
        CT->>CT: Match prompt against patterns
        CT->>S: Write matched way paths
        Note right of S: /tmp/.claude-subagent-stash-{sid}/{ts}.json
    end

    rect rgba(106, 27, 154, 0.15)
        Note over CC,SA: Phase 2: SubagentStart
        CC->>SA: Spawn subagent
        CC->>IS: SubagentStart
        IS->>S: Read + claim oldest stash
        IS->>IS: Emit way content (no markers)
        IS->>SA: additionalContext
        Note right of SA: Subagent sees way guidance
        IS->>S: Delete consumed stash
    end
```

### Scope Filtering

The `scope` field controls where ways inject:

```mermaid
flowchart LR
    classDef agent fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef sub fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef both fill:#00695C,stroke:#004D40,color:#fff
    classDef skip fill:#78909C,stroke:#546E7A,color:#fff

    Way["{name}.md<br/>scope: ?"]

    Way -->|"scope: agent"| AG["Agent only<br/>check-prompt / bash / file"]:::agent
    Way -->|"scope: subagent"| SB["Subagent only<br/>check-task-pre → inject"]:::sub
    Way -->|"scope: agent, subagent"| BOTH["Both paths<br/>(default for all built-in ways)"]:::both
    Way -->|"no scope field"| DEF["Agent only<br/>(backward compatible)"]:::agent
```

### Parallel Subagent Handling

Multiple Task tools in one message create separate stash files consumed in FIFO order:

```mermaid
sequenceDiagram
    participant CT as check-task-pre.sh
    participant S as Stash Dir
    participant IS as inject-subagent.sh

    rect rgba(21, 101, 192, 0.12)
        CT->>S: Write {ts1}.json (Task A)
        CT->>S: Write {ts2}.json (Task B)
    end

    rect rgba(106, 27, 154, 0.12)
        IS->>S: Read {ts1}.json (oldest) → Subagent A
        IS->>S: Read {ts2}.json (oldest) → Subagent B
    end

    Note over S: Empty after both consumed
```

## Way State Machine

Each (way, session) pair has exactly two states:

```mermaid
stateDiagram-v2
    classDef notShown fill:#C62828,stroke:#B71C1C,color:#fff,font-weight:bold
    classDef shown fill:#2E7D32,stroke:#1B5E20,color:#fff,font-weight:bold

    [*] --> NotShown: Session starts

    NotShown: not_shown
    NotShown: No marker file exists

    Shown: shown
    Shown: Marker file exists

    NotShown --> Shown: Trigger match → output + create marker
    Shown --> Shown: Trigger match → no-op (idempotent)

    Shown --> [*]: Session ends (markers in /tmp)

    state "not_shown" as NotShown:::notShown
    state "shown" as Shown:::shown
```

**Exception**: Subagent injection bypasses this state machine entirely. Ways injected via `inject-subagent.sh` are emitted without marker checks.

## Trigger Matching

How prompts and tool use get matched to ways:

```mermaid
flowchart LR
    classDef input fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef scan fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef match fill:#00695C,stroke:#004D40,color:#fff
    classDef output fill:#2E7D32,stroke:#1B5E20,color:#fff

    subgraph Input
        Prompt["User prompt<br/>(lowercased)"]:::input
        Cmd["Bash command"]:::input
        File["File path"]:::input
    end

    subgraph Scan["Recursive Scan"]
        Find["find */{name}.md"]:::scan
        Extract["Extract frontmatter:<br/>pattern, commands, files, scope"]:::scan
    end

    subgraph Match["Regex Match"]
        KW["pattern: regex"]:::match
        CM["commands: pattern"]:::match
        FL["files: pattern"]:::match
    end

    Prompt --> Find
    Cmd --> Find
    File --> Find

    Find --> Extract
    Extract --> KW
    Extract --> CM
    Extract --> FL

    KW -->|match| Out["ways show<br/>(marker-gated output)"]:::output
    CM -->|match| Out
    FL -->|match| Out
```

## Semantic Matching

Ways with `description:` fields use a three-tier scoring engine:

```mermaid
flowchart TB
    classDef input fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef process fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef check fill:#E65100,stroke:#BF360C,color:#fff
    classDef yes fill:#2E7D32,stroke:#1B5E20,color:#fff
    classDef no fill:#C62828,stroke:#B71C1C,color:#fff

    subgraph Input
        Prompt["User prompt"]:::input
        Corpus["ways-corpus.jsonl<br/>(pre-computed embedding vectors)"]:::input
    end

    subgraph Embedding["Embedding (ADR-108, ADR-125)"]
        Embed["way-embed match<br/>all-MiniLM-L6-v2"]:::process
        Cosine["Cosine similarity<br/>vs 384-dim pre-computed vectors"]:::process
        EmbedResult["similarity ≥ embed_threshold?"]:::check
    end

    Prompt --> Embed --> Cosine --> EmbedResult
    EmbedResult -->|Yes| Match["MATCH"]:::yes
    EmbedResult -->|No| NoMatch["No match"]:::no
    Corpus --> Embed
```

| Engine | Accuracy | Timing | Requirements |
|--------|----------|--------|-------------|
| **Embedding** | 98.4% (63/64) | ~20ms | `way-embed` binary + GGUF model (21MB) |

The embedding model is a hard dependency of `ways`. See ADR-125 for the authored disclosure graph model and the single-tier decision.

## Telemetry & Tuning

The matcher computes a score for every way on every prompt. Fires are recorded; so are the *near-misses* — ways that scored just under threshold and stayed silent. Both feed back into how the engine is tuned. This closes the loop ADR-134 opened: hand-set thresholds and half-lives become things the system can revise from its own experience.

```mermaid
flowchart LR
    classDef match fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef log fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef tune fill:#00695C,stroke:#004D40,color:#fff
    classDef apply fill:#E65100,stroke:#BF360C,color:#fff

    Scan["match_prompt<br/>Fired · NearMiss · NoMatch"]:::match

    Scan -->|fired| WF["way_fired<br/>(+ fire_score on first-fires)"]:::log
    Scan -->|"within near_miss_margin"| NM["way_nearmiss<br/>(recall signal)"]:::log

    WF --> EV[("~/.claude/stats/events.jsonl<br/>bounded ~24–32 MiB")]:::log
    NM --> EV

    EV --> TC["ways tune-curves<br/>half_life ≈ median fire delta"]:::tune
    EV --> TP["ways tune-precision<br/>off-class irrelevance audit"]:::tune

    TC -->|--apply| Curve["rewrite curve: block"]:::apply
    TP -->|report-only| Remedy["flag: mis-targeted / cross-cutting"]:::apply
```

### Telemetry events

Two events in `~/.claude/stats/events.jsonl` carry the tuning signal:

- `way_fired` now records `fire_score` — the embedding score that cleared threshold — on **first-fires only** (not on `way_redisclosed`). It feeds future `embed_threshold` tuning.
- `way_nearmiss` is emitted when a way scores within `near_miss_margin` *below* its effective threshold but does not fire. The scores already exist; this is persistence, not new computation. Fields: `score_en`, `score_multi`, `thr_en`, `thr_multi`, `margin`, `trigger`, `query_tokens`. It is a recall signal — the first measure of likely *false silences*, the ways that should have fired and didn't.

`near_miss_margin` (default `0.05`) is parsed from the ways config YAML alongside `default_embed_threshold` and `default_multi_embed_threshold`. It caps near-miss volume: only the band just under threshold logs.

The log grows faster with near-misses, so its growth is bounded. `log_event` tail-compacts `events.jsonl` once it crosses ~32 MiB, keeping the most recent ~24 MiB (cut at a line boundary, written to a temp file and atomically renamed). The oldest events are lost; readers are unaffected — a reader holding the pre-compaction file keeps reading it intact.

### Tuning commands

- **`ways tune-curves`** (ADR-123 Phase E) — cadence calibration. Groups `way_fired` / `way_redisclosed` by `(way, session)`, computes token-position deltas between fires, and suggests `half_life ≈ median delta`. `--apply` rewrites the way's `curve:` block in place.
- **`ways tune-precision`** (ADR-134 Decision 3) — a heuristic relevance audit of fire telemetry, report-only. For each way it estimates how often its fires landed *off-class*: in sessions whose activity — judged by the parent-family of the ways that co-fired — never touched the way's own domain. It reports an irrelevance rate and a flag: **mis-targeted** (a narrow way repeatedly firing into the same wrong kind of session; remedy: raise `embed_threshold`, narrow vocabulary, or change trigger channel) vs **cross-cutting** (a way that fires broadly by design, e.g. `meta/tracking`; remedy: scope by trigger — vocabulary is *never* auto-narrowed). Flags: `--min-sessions` (default 5), `--flag-threshold` (default 0.5), `--project`, `--way`, `--json`.

ADR-134 is **Accepted**. One slice — the `embed_threshold`-gated `--apply`, driven by accumulated `fire_score` data — is deferred and data-gated (GitHub issue #123).

## Macro Injection

Ways with `macro: prepend|append` run dynamic scripts that query live state:

```mermaid
sequenceDiagram
    participant Hook as check-*.sh
    participant Show as ways show
    participant Macro as macro.sh
    participant Way as {name}.md
    participant Out as Output

    Hook->>Show: waypath, session_id

    rect rgba(198, 40, 40, 0.12)
        Show->>Show: Check marker
        alt Marker exists
            Show-->>Hook: (silent return)
        end
    end

    rect rgba(21, 101, 192, 0.15)
        Note over Show,Way: No marker — first time this session
        Show->>Way: Read frontmatter

        alt macro: prepend
            rect rgba(106, 27, 154, 0.12)
                Show->>Macro: Execute script
                Note right of Macro: e.g. query GitHub API,<br/>scan files, check tooling
                Macro-->>Out: Dynamic context
            end
            Show->>Way: Strip frontmatter
            Way-->>Out: Static guidance
        else macro: append
            Show->>Way: Strip frontmatter
            Way-->>Out: Static guidance
            rect rgba(106, 27, 154, 0.12)
                Show->>Macro: Execute script
                Macro-->>Out: Dynamic context
            end
        else no macro
            Show->>Way: Strip frontmatter
            Way-->>Out: Static guidance
        end
    end

    rect rgba(46, 125, 50, 0.15)
        Show->>Show: Create marker
        Note right of Show: Way won't fire again this session
    end
```

## Directory Structure

```
~/.claude/hooks/ways/
├── core.md                     # Base guidance (loads at startup)
├── macro.sh                    # Generates Available Ways table
│
├── check-prompt.sh             # UserPromptSubmit → dispatches to `ways scan prompt`
├── check-bash-pre.sh           # PreToolUse:Bash → scan commands
├── check-file-pre.sh           # PreToolUse:Edit|Write → scan files
├── check-task-pre.sh           # PreToolUse:Task → stash for subagent
├── check-state.sh              # UserPromptSubmit → state triggers
├── check-response.sh           # Stop → extract topics for next turn
│
├── inject-subagent.sh          # SubagentStart → emit stashed ways (JSON hookSpecificOutput)
├── clear-markers.sh            # SessionStart → reset session state
├── mark-tasks-active.sh        # PreToolUse:TaskCreate → context nag gate
│
├── softwaredev/                # Domain: software development
│   ├── commits/commits.md       #   git commit format
│   ├── testing/testing.md       #   test practices
│   ├── security/security.md     #   auth, secrets, vulnerabilities
│   ├── github/                  #   PR workflow
│   │   ├── github.md
│   │   └── macro.sh             #   detects solo vs team
│   └── ...                      #   18 ways total
├── itops/                       # Domain: IT operations
│   └── ...                      #   4 ways
└── meta/                        # Domain: meta-system
    └── ...                      #   5 ways

$PROJECT/.claude/ways/           # Project-local overrides
└── {domain}/{wayname}/{wayname}.md  # Same structure, takes precedence
```

### Script Relationships

```mermaid
flowchart LR
    classDef trigger fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef shared fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef output fill:#2E7D32,stroke:#1B5E20,color:#fff
    classDef stash fill:#E65100,stroke:#BF360C,color:#fff
    classDef util fill:#00695C,stroke:#004D40,color:#fff

    WAYS["ways binary<br/>(scan + show + session)"]:::shared --> EMB["Embedding<br/>(all-MiniLM-L6-v2)"]:::util

    CP["check-prompt.sh"]:::trigger --> WAYS
    CB["check-bash-pre.sh"]:::trigger --> WAYS
    CF["check-file-pre.sh"]:::trigger --> WAYS
    CS["check-state.sh"]:::trigger --> WAYS

    CT["check-task-pre.sh"]:::trigger --> ST[("stash file")]:::stash
    ST --> IS["inject-subagent.sh"]:::output
```

## Multi-Trigger Semantics

What happens when multiple triggers fire:

```mermaid
flowchart TB
    classDef prompt fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef pattern fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef way fill:#00695C,stroke:#004D40,color:#fff
    classDef gate fill:#E65100,stroke:#BF360C,color:#fff
    classDef output fill:#2E7D32,stroke:#1B5E20,color:#fff
    classDef silent fill:#78909C,stroke:#546E7A,color:#fff

    Prompt["'Let's review the PR and fix the bug'"]:::prompt

    Prompt --> KW1["pattern: github|pr"]:::pattern
    Prompt --> KW2["pattern: debug|bug"]:::pattern
    Prompt --> KW3["pattern: review"]:::pattern

    KW1 -->|match| GH["github way"]:::way
    KW2 -->|match| DB["debugging way"]:::way
    KW3 -->|match| QA["quality way"]:::way

    GH --> M1{Marker?}:::gate
    DB --> M2{Marker?}:::gate
    QA --> M3{Marker?}:::gate

    M1 -->|No| O1["✓ Output"]:::output
    M2 -->|No| O2["✓ Output"]:::output
    M3 -->|No| O3["✓ Output"]:::output

    M1 -->|Yes| S1["✗ Silent"]:::silent
    M2 -->|Yes| S2["✗ Silent"]:::silent
    M3 -->|Yes| S3["✗ Silent"]:::silent
```

Each way has its own marker - multiple ways can fire from one prompt, but each only fires once per session.

## Project-Local Override

```mermaid
flowchart TB
    classDef proj fill:#E65100,stroke:#BF360C,color:#fff
    classDef global fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef marker fill:#00695C,stroke:#004D40,color:#fff
    classDef skip fill:#78909C,stroke:#546E7A,color:#fff

    subgraph Scan["Way Lookup Order"]
        P["1. Project: $PROJECT/.claude/ways/"]:::proj
        G["2. Global: ~/.claude/hooks/ways/"]:::global
    end

    P -->|found| Use["Use project way"]:::proj
    P -->|not found| G
    G -->|found| UseG["Use global way"]:::global
    G -->|not found| Skip["No match"]:::skip

    Use --> Mark["Single marker<br/>(by waypath)"]:::marker
    UseG --> Mark
```

Project ways take precedence. Only one marker per waypath regardless of source.
