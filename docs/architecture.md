# Ways System Architecture

Diagrams of the ways trigger system: what runs on each hook event, how a way is matched and admitted, and what state the engine keeps. The prose here is kept to what a diagram needs. The fire rule with its thresholds is stated once in [engine-reference.md](hooks-and-ways/engine-reference.md), the hook table and the switches in [hooks-and-ways.md](hooks-and-ways.md), and the event log fields in [reference/events.md](reference/events.md).

Two names recur below:

- `{SESSIONS_ROOT}` is `$XDG_RUNTIME_DIR/claude-sessions`, or `/tmp/.claude-sessions-{uid}` where that variable is unset (`%LOCALAPPDATA%/claude-ways/sessions` on Windows). `{SESSIONS_ROOT}/{session_id}/` holds one session's state.
- Firing state is kept **per agent**. The main agent's engagement, way tokens, way epochs, epoch counter and check fires sit at the session root. A subagent's sit under `{SESSIONS_ROOT}/{session_id}/agents/{agent_id}/`, and its token position, context window and model are read from its own transcript. Way markers are per agent too: `ways/{way_id}/.marker.{agent_id}`, with `main` for the main agent. A way shown to a subagent therefore does not silence it for the main agent, or the reverse.

## Where the runtime lives

| Root | Path | Holds |
|---|---|---|
| App | `$XDG_DATA_HOME/agent-ways` | The source checkout, the built binaries, and the shipped ways under `hooks/ways/`. Replaced on update. |
| Projection | `~/.claude` (and each extra target) | `hooks/ways`, `skills`, `agents`, `commands` and each binary under `bin/` are linked (or copied) from the app. `ways reconcile` three-way merges the repo's `settings.json` hooks block into the target's `settings.json` (ADR-142, ADR-184). |
| User config | `$XDG_CONFIG_HOME/agent-ways` | `config.yaml`, your own ways under `ways/`, the judge settings in `agent.yaml`, and per-target `targets/<key>/config.yaml`. Survives updates. |
| State | `$XDG_STATE_HOME/agent-ways` | `events.jsonl`, and the per-session subagent switches under `subagent-switch/`. |
| Cache | `$XDG_CACHE_HOME/agent-ways/user` | The embedding corpora (`ways-corpus-en.jsonl`, `ways-corpus-multi.jsonl`), `embed-manifest.json` with the calibration, and the GGUF models. Regenerable. |
| Session | `{SESSIONS_ROOT}/{session_id}` | Markers, per-agent firing state, the subagent stash, the last response. Cleared on SessionStart startup, compact and clear. |

## How a Session Flows

A session from the user's side, showing which lane each injection comes from:

```mermaid
sequenceDiagram
    participant U as User
    participant C as Claude Code
    participant W as ways hook
    participant G as Ways agent (judge)
    participant S as Subagent

    Note over U,S: SessionStart - check-state.sh shows core.md and the session-start ways

    rect rgba(21, 101, 192, 0.15)
        Note over U,G: Prompt lane
        U->>C: "Let's fix the auth bug and add tests"
        C->>W: UserPromptSubmit → ways hook prompt
        W->>W: match (late-interaction, g(s) fallback, keyword floor τ_k)
        W->>G: judge the candidates (at most 8, with the last turn)
        G-->>W: P(yes) per way
        W-->>C: inject ways with P(yes) ≥ 0.3
        Note right of W: a blocked way leaves no marker and keeps its refire budget
    end

    rect rgba(106, 27, 154, 0.15)
        Note over C,W: Tool lanes, before the tool runs
        C->>W: PreToolUse:Bash → ways hook command (git log auth/)
        Note right of W: no way matches, the command proceeds
        C->>W: PreToolUse:Edit → ways hook file (config/auth.yaml)
        W-->>C: config way, before the edit happens
    end

    rect rgba(0, 105, 92, 0.15)
        Note over C,W: Post-tool lanes, after the tool runs
        C->>W: PostToolUse → ways hook post-tool, ways hook queued
        W->>W: postcheck.sh scripts request reactive fires
        W->>G: operator messages queued mid-turn, matched and judged like a prompt
        W-->>C: fired ways
    end

    rect rgba(230, 81, 0, 0.15)
        Note over C,S: Delegation
        C->>W: PreToolUse:Task → ways hook task (match, write stash)
        C->>S: subagent starts
        W-->>S: SubagentStart → ways hook subagent-start injects the stashed ways
        Note right of S: the subagent keeps its own markers and refire state
        S-->>C: findings
    end

    rect rgba(21, 101, 192, 0.15)
        Note over U,W: A later prompt on the same topic
        U->>C: "Now check the tests again"
        Note right of W: testing way inside its refire window → way_suppressed (refire)
        Note right of W: re-disclosed once its refire fraction of the window has passed
    end

    rect rgba(198, 40, 40, 0.15)
        Note over U,S: Auto-compact
        C->>W: SessionStart:compact → ways hook session-start
        W->>W: clear {SESSIONS_ROOT}/{session_id}/
        W-->>C: core.md again from the state scan
        Note right of C: every way can fire again on its next match
    end
```

## Hook Flow

Every script under `hooks/ways/` that touches ways is a thin adapter for one `ways hook <event>` call (ADR-504 §11). The other hooks in the same `settings.json` block are shown in grey.

```mermaid
flowchart LR
    classDef event fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef script fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef gate fill:#E65100,stroke:#BF360C,color:#fff
    classDef output fill:#2E7D32,stroke:#1B5E20,color:#fff
    classDef other fill:#78909C,stroke:#546E7A,color:#fff

    SS["SessionStart<br/>startup · compact · resume · clear"]:::event
    UP[UserPromptSubmit]:::event
    PB["PreToolUse<br/>Bash"]:::event
    PF["PreToolUse<br/>Edit, Write"]:::event
    PT["PreToolUse<br/>Task"]:::event
    PC["PreToolUse<br/>TaskCreate"]:::event
    SA[SubagentStart]:::event
    PO["PostToolUse<br/>Edit, Write, Bash, Task"]:::event
    PX["PostToolUseFailure<br/>Edit, Write, Bash, Task"]:::event
    ST[Stop]:::event
    TC[TaskCreated]:::event

    CM["clear-markers.sh → ways hook session-start<br/>(startup, compact, clear)"]:::script
    CS["check-state.sh → ways hook state"]:::script
    CP["check-prompt.sh → ways hook prompt"]:::script
    CB["check-bash-pre.sh → ways hook command"]:::script
    CF["check-file-pre.sh → ways hook file"]:::script
    CT["check-task-pre.sh → ways hook task"]:::script
    MT["mark-tasks-active.sh → ways hook tasks-active"]:::script
    IS["inject-subagent.sh → ways hook subagent-start"]:::script
    PP["check-post.sh → ways hook post-tool"]:::script
    QQ["check-queued.sh → ways hook queued"]:::script
    CR["check-response.sh → ways hook stop"]:::script

    SS --> CM
    SS --> CS
    UP --> CP
    UP --> CS
    PB --> CB
    PF --> CF
    PT --> CT
    PC --> MT
    SA --> IS
    PO --> PP
    PX --> PP
    PO --> QQ
    ST --> CR

    On{"Switched on?<br/>ways.enabled · subagent switches<br/>· defined agent"}:::gate
    CS --> On
    CP --> On
    CB --> On
    CF --> On
    CT --> On
    IS --> On
    PP --> On
    QQ --> On

    On -->|prompt, queued, command, file| Match["Matcher<br/>keyword · semantic"]:::gate
    On -->|state| State["core.md · state triggers"]:::gate
    On -->|post-tool| Post["postcheck.sh exits 0"]:::gate
    On -->|task| TaskM["Matcher on the Task prompt<br/>scope: subagent"]:::gate
    On -->|subagent-start| Claim["claim the oldest stash"]:::gate

    Match -->|prompt, queued| Judge{"Relevance judge<br/>ways agent socket"}:::gate
    Match -->|command, file| Refire
    Judge -->|pass| Refire
    Judge -->|block| Blocked["not shown, no marker"]:::other
    State --> Refire
    Post --> Refire
    Refire{"Inside the refire window,<br/>or over the 10,000-char budget?"}:::gate
    Refire -->|no| Out["inject, stamp the per-agent marker"]:::output
    Refire -->|yes| Supp["way_suppressed"]:::other

    TaskM --> Stash[("{SESSIONS_ROOT}/{sid}/subagent-stash/")]:::output
    Stash -.-> Claim
    Claim --> Emit["emit fresh, record the fire<br/>under the subagent's id"]:::output

    CM --> Clear["clear {SESSIONS_ROOT}/{sid}/,<br/>log session_start"]:::output
    CR --> Rec["record the last response"]:::output
    MT --> TA["write tasks-active (dormant)"]:::other

    SS -.-> O1["check-setup.sh · check-config-updates.sh<br/>· ways init · ways corpus --if-stale"]:::other
    SS -.-> O2["issues-pull.sh"]:::other
    UP -.-> O2
    PO -.->|Bash| O2
    PB -.-> O3["strip-session-link-pre.sh"]:::other
    ST -.-> O4["attend-drain-stop.sh"]:::other
    TC -.-> O5["issues-task-created.sh"]:::other
```

`check-setup.sh`, `check-config-updates.sh`, `ways init` and `ways corpus --if-stale` run on `startup` only (`ways init` also on `clear`). `check-queued.sh` scans for the main agent only, because operator messages are queued to it.

## Subagent Injection

A Task prompt is visible on PreToolUse:Task, but the subagent only exists at SubagentStart. A stash file bridges the two:

```mermaid
sequenceDiagram
    participant A as Main agent
    participant CC as Claude Code
    participant CT as check-task-pre.sh (ways hook task)
    participant S as Stash dir
    participant IS as inject-subagent.sh (ways hook subagent-start)
    participant SA as Subagent

    rect rgba(21, 101, 192, 0.15)
        Note over A,S: Phase 1 - PreToolUse:Task
        A->>CC: Task(prompt: "Review the PR for security issues")
        CC->>CT: PreToolUse:Task
        alt subagent_type names a defined agent
            Note right of CT: project, user or plugin agents/*.md - no stash
        else ways switched off for subagents (session, project or user)
            CT->>CT: log injection_suppressed, no stash
        else
            CT->>CT: keyword and late-interaction match over the Task prompt (scope: subagent)
            CT->>S: write {ts}.json with the matched way ids
            Note right of S: {SESSIONS_ROOT}/{sid}/subagent-stash/{ts}.json
        end
    end

    rect rgba(106, 27, 154, 0.15)
        Note over CC,SA: Phase 2 - SubagentStart
        CC->>SA: spawn subagent
        CC->>IS: SubagentStart
        IS->>S: claim the oldest stash (rename, read, delete)
        alt ways switched off for subagents
            Note right of IS: the claimed stash is discarded
        else
            IS->>IS: if a teammate, write the teammate marker in its agent dir
            IS->>IS: render each way, record the fire under the subagent's id
            IS->>SA: additionalContext
        end
    end
```

The stashed ways are emitted whatever the parent has already been shown. Each fire is then recorded under the subagent's own id, so the subagent's later hooks follow their own refire windows.

### Scope Filtering

The `scope` field controls where a way can inject:

```mermaid
flowchart LR
    classDef agent fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef sub fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef both fill:#00695C,stroke:#004D40,color:#fff
    classDef team fill:#E65100,stroke:#BF360C,color:#fff

    Way["{name}.md<br/>scope: ?"]

    Way -->|"scope: agent"| AG["Main agent only<br/>prompt, state, command, file, post-tool lanes"]:::agent
    Way -->|"scope: subagent"| SB["Subagents only<br/>task stash → subagent-start"]:::sub
    Way -->|"scope: agent, subagent"| BOTH["Both paths<br/>(most shipped ways)"]:::both
    Way -->|"scope: teammate"| TM["Teammates<br/>task stash with a team name"]:::team
    Way -->|"no scope field"| DEF["ways.default_scope<br/>(agent unless configured)"]:::agent
```

### Parallel Subagent Handling

Several Task calls in one message write separate stash files, consumed oldest first. Each SubagentStart claims its file by renaming it, so two subagents never take the same stash.

```mermaid
sequenceDiagram
    participant CT as check-task-pre.sh
    participant S as Stash dir
    participant IS as inject-subagent.sh

    rect rgba(21, 101, 192, 0.12)
        CT->>S: write {ts1}.json (Task A)
        CT->>S: write {ts2}.json (Task B)
    end

    rect rgba(106, 27, 154, 0.12)
        IS->>S: claim {ts1}.json (oldest) → Subagent A
        IS->>S: claim {ts2}.json (oldest) → Subagent B
    end

    Note over S: empty after both are claimed
```

## Disclosure Cadence

Each (way, agent) pair in a session moves through these states. A way's `refire:` value is a fraction of the agent's context window (ADR-126): `once` 1.0, `rare` 0.4, `normal` 0.15, `frequent` 0.05, or a number.

```mermaid
stateDiagram-v2
    classDef notShown fill:#C62828,stroke:#B71C1C,color:#fff,font-weight:bold
    classDef shown fill:#2E7D32,stroke:#1B5E20,color:#fff,font-weight:bold
    classDef eligible fill:#E65100,stroke:#BF360C,color:#fff,font-weight:bold

    state "NotShown (no marker)" as NotShown
    state "Shown (marker holds token_pos)" as Shown
    state "Eligible again" as Eligible

    [*] --> NotShown
    NotShown --> Shown: match, judge pass, fits the budget
    NotShown --> NotShown: judge block (way_judged), no marker
    NotShown --> NotShown: no room (way_suppressed context_cap)
    Shown --> Shown: match inside the window (way_suppressed refire)
    Shown --> Eligible: refire × window tokens consumed
    Eligible --> Shown: match, judge pass, fits (way_redisclosed)
    Eligible --> Eligible: judge block or no room
    Shown --> NotShown: SessionStart compact or clear
    Eligible --> NotShown: SessionStart compact or clear

    class NotShown notShown
    class Shown shown
    class Eligible eligible
```

The marker is `{SESSIONS_ROOT}/{session_id}/ways/{way_id}/.marker.{agent_id}`. The judge runs on the prompt and queued lanes only, so the judge-block edges apply there. A `session-start` state way shows once per marker reset rather than on a refire window.

## Trigger Matching

How prompts and tool input reach a way:

```mermaid
flowchart LR
    classDef input fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef scan fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef match fill:#00695C,stroke:#004D40,color:#fff
    classDef gate fill:#E65100,stroke:#BF360C,color:#fff
    classDef output fill:#2E7D32,stroke:#1B5E20,color:#fff
    classDef silent fill:#78909C,stroke:#546E7A,color:#fff

    subgraph Input
        Prompt["User prompt<br/>(fences and URLs masked)"]:::input
        Queued["Queued operator messages"]:::input
        Cmd["Bash command + description<br/>+ prose since the last human turn"]:::input
        File["File path"]:::input
        TaskP["Task prompt"]:::input
    end

    Cand["collect_candidates<br/>project .claude/ways<br/>> user $XDG_CONFIG_HOME/agent-ways/ways<br/>> core ~/.claude/hooks/ways<br/>(a higher root shadows the same id, tree order)"]:::scan
    Pre["scope · when: preconditions"]:::scan

    Prompt --> Cand
    Queued --> Cand
    Cmd --> Cand
    File --> Cand
    TaskP --> Cand
    Cand --> Pre

    Pre --> KW["pattern: on prompt, queued, task"]:::match
    Pre --> DIR["commands: on the command<br/>pattern: on the description<br/>files: on the path"]:::match
    Pre --> SEM["semantic<br/>late-interaction or g(s) fallback"]:::match

    KW --> Floor{"g(s) ≥ τ_k?<br/>fails open · pattern_strict bypasses"}:::gate
    Floor -->|no| KG["way_keyword_gated"]:::silent
    Floor -->|yes| Hits
    DIR --> Hits
    SEM --> Hits

    Hits["order_hits"]:::scan
    Hits -->|task| Stash[("subagent stash")]:::output
    Hits -->|prompt, queued| Judge{"relevance judge"}:::gate
    Hits -->|command, file| Show
    Judge -->|pass| Show
    Judge -->|block| JB["way_judged block"]:::silent
    Show{"disabled? refire window?<br/>context budget?"}:::gate
    Show -->|admitted| Out["additionalContext"]:::output
    Show -->|held| WS["way_suppressed"]:::silent
```

The bash semantic lane uses the single-vector scores only. The file lane is regex only. Disabled domains and ways are checked when a way is shown.

## Semantic Matching

The semantic channel on the prompt, queued and task surfaces is the ADR-160 late-interaction matcher. The single-vector calibrated gate (ADR-156) decides only when late-interaction cannot run. Its probabilities are computed on every scan, because the keyword floor and near-miss logging read them on both paths.

```mermaid
flowchart TB
    classDef input fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef process fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef check fill:#E65100,stroke:#BF360C,color:#fff
    classDef yes fill:#2E7D32,stroke:#1B5E20,color:#fff
    classDef side fill:#78909C,stroke:#546E7A,color:#fff

    Red["reduce_for_embed<br/>prompt + last response, sentence salience<br/>(ADR-130, ADR-155)"]:::input
    Chunk{"≥ 2 sentence chunks<br/>and the EN engine present?"}:::check
    Red --> Chunk

    subgraph LI["Late-interaction (ADR-160), EN corpus only"]
        Batch["embed every chunk in one way-embed batch<br/>vs ways-corpus-en.jsonl"]:::process
        Rank["per way: peak cosine over chunks<br/>per chunk: softmax share over the top 8 (τ 0.08)"]:::process
        Admit{"summed share ≥ 0.15<br/>or peak ≥ 0.50?"}:::check
        Confirm{"won chunk vs the way's body<br/>≥ 0.35?"}:::check
        Batch --> Rank --> Admit -->|yes| Confirm
    end

    subgraph SV["Single vector (ADR-156)"]
        Vec["one vector per model<br/>EN, plus multilingual when localized"]:::process
        Cal["g(s) = σ(a·s + b)<br/>fit stored in embed-manifest.json"]:::process
        Tau{"g(s) ≥ τ_s?<br/>0.5, or 0.40 with parent boost"}:::check
        Vec --> Cal --> Tau
    end

    Chunk -->|yes| Batch
    Chunk -->|"no (fallback)"| Tau
    Red --> Vec
    Confirm -->|yes| F1["FIRE<br/>semantic:late-interaction:en"]:::yes
    Tau -->|yes| F2["FIRE<br/>semantic:embedding:en or :multi"]:::yes
    Cal -.->|"g(s) within 0.05 below τ_s"| NM["way_nearmiss"]:::side
    Cal -.-> KF["keyword floor τ_k"]:::side
```

The late-interaction operating points are hand-set and uncalibrated. Parent boost lowers `τ_s`, so it has no effect when late-interaction decides. On a localized install the multilingual lane can fire only on the fallback path. [engine-reference.md](hooks-and-ways/engine-reference.md) states the rule with its sources.

## Relevance Gate and the Ways Agent

On the prompt and queued lanes, the ways the matcher would show are sent in one request to a hosted yes/no judge before any fire is recorded (ADR-196). The judge runs in the **ways agent**, one resident daemon per user on a Unix socket of mode 0600 (ADR-502). A hook starts it on demand. It exits when idle, and when its binary is replaced. It holds the provider key, so hooks never read one, and it does judging and key custody only. Matching stays in the `ways` process. `ways agent status` reports the running agent, `ways agent key` manages keys, and `ways settings` sets `gate.mode` (`enforce`, `shadow`, `off`) and `gate.engine`. No key file means no gate.

What the judge sees, its threshold, cap, timeout, cost and failure behaviour are explained in [the relevance judge](explanation/relevance-judge/relevance-judge-the-model.md).

## Telemetry & Tuning

```mermaid
flowchart LR
    classDef match fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef log fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef read fill:#00695C,stroke:#004D40,color:#fff

    Scan["scan lanes"]:::match
    Gate["relevance gate"]:::match
    Show["way_scored / subagent-start"]:::match
    Hook["ways hook"]:::match

    Scan -->|"way_nearmiss · way_keyword_gated"| EV
    Gate -->|"way_judged · judge_call<br/>gate_capped · gate_fallback"| EV
    Show -->|"way_fired (+ fire_score) · way_redisclosed<br/>way_suppressed · check_fired"| EV
    Hook -->|"session_start · injection_suppressed"| EV

    EV[("$XDG_STATE_HOME/agent-ways/events.jsonl<br/>tail-compacted at ~32 MiB")]:::log

    EV --> R1["ways session<br/>ways · fires · replay · live · dump"]:::read
    EV --> R2["ways tune precision · ways tune stats"]:::read
    EV --> R3["ways agent cost"]:::read
```

`fire_score` is the deciding score of the semantic channel that fired: the summed share for late-interaction, `g(s)` for the single-vector path. The `trigger` field says which. The tuning loop is described in [hooks-and-ways.md](hooks-and-ways.md#telemetry), and every event's fields in [reference/events.md](reference/events.md).

## Macro Injection

Ways with `macro: prepend|append` run a script that queries live state when the way is shown:

```mermaid
sequenceDiagram
    participant Lane as scan lane
    participant Show as way_scored
    participant Macro as macro.sh
    participant Out as additionalContext

    Lane->>Show: way id, session, trigger, budget

    rect rgba(198, 40, 40, 0.12)
        Show->>Show: refire check for this agent
        alt inside the refire window
            Show-->>Lane: nothing (way_suppressed refire)
        else static body does not fit the budget
            Show-->>Lane: nothing (way_suppressed context_cap)
        end
    end

    rect rgba(21, 101, 192, 0.15)
        alt macro: prepend
            Show->>Macro: run (project macros only in trusted projects)
            Macro-->>Out: dynamic context
            Show-->>Out: static body
        else macro: append
            Show-->>Out: static body
            Show->>Macro: run
            Macro-->>Out: dynamic context
        else no macro
            Show-->>Out: static body
        end
    end

    rect rgba(46, 125, 50, 0.15)
        Show->>Show: lock, re-check refire, admit to the budget
        Show->>Show: record the fire, stamp the marker and token position
        Note right of Show: eligible again after the refire fraction of the window
    end
```

A project-local macro runs only if the project is listed in `~/.claude/trusted-project-macros`.

## Directory Structure

```
$XDG_DATA_HOME/agent-ways/hooks/ways/      # shipped ways, projected as ~/.claude/hooks/ways/
├── core.md                     # base guidance, shown by the state scan
├── macro.sh                    # core.md's macro: the Available Ways table
├── require-ways.sh             # shared by the adapters: runs ~/.claude/bin/ways hook <event>
│
├── clear-markers.sh            # SessionStart → ways hook session-start
├── check-state.sh              # SessionStart, UserPromptSubmit → ways hook state
├── check-prompt.sh             # UserPromptSubmit → ways hook prompt
├── check-bash-pre.sh           # PreToolUse:Bash → ways hook command
├── check-file-pre.sh           # PreToolUse:Edit|Write → ways hook file
├── check-task-pre.sh           # PreToolUse:Task → ways hook task
├── mark-tasks-active.sh        # PreToolUse:TaskCreate → ways hook tasks-active (dormant marker)
├── inject-subagent.sh          # SubagentStart → ways hook subagent-start
├── check-post.sh               # PostToolUse, PostToolUseFailure → ways hook post-tool
├── check-queued.sh             # PostToolUse → ways hook queued
├── check-response.sh           # Stop → ways hook stop
│
├── check-setup.sh              # SessionStart:startup → notice when the ways binary is missing
├── strip-session-link-pre.sh   # PreToolUse:Bash → deny publishing a session link (ADR-167)
├── attend-drain-stop.sh        # Stop → attend inbox --drain (ADR-172)
├── issues-pull.sh              # SessionStart, UserPromptSubmit, PostToolUse:Bash → gh-tasks pull (ADR-180)
├── issues-task-created.sh      # TaskCreated → reject unprefixed duplicates of mirrored issues (ADR-180)
├── check-bash-bound.py         # Bash guard (ADR-181), shipped but not wired
│
└── {domain}/{way}/{way}.md     # softwaredev, meta, documentation, ea, workstation, data, itops, ...
    ├── macro.sh                #   optional dynamic content
    ├── postcheck.sh            #   optional reactive firing on PostToolUse
    └── {child}/{child}.md      #   ways nest for progressive disclosure

$XDG_CONFIG_HOME/agent-ways/ways/          # your own ways, same layout, survive updates
$PROJECT/.claude/ways/                     # project ways, same layout, highest precedence
```

`check-config-updates.sh` sits one level up, in `hooks/`.

### Script Relationships

```mermaid
flowchart LR
    classDef trigger fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef shared fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef util fill:#00695C,stroke:#004D40,color:#fff
    classDef ext fill:#E65100,stroke:#BF360C,color:#fff
    classDef other fill:#78909C,stroke:#546E7A,color:#fff

    AD["check-*.sh · clear-markers.sh<br/>inject-subagent.sh · mark-tasks-active.sh"]:::trigger
    RW["require-ways.sh<br/>ways hook &lt;event&gt;"]:::shared
    AD --> RW
    RW --> HOOK["ways hook"]:::shared

    HOOK --> SCAN["scan lanes"]:::shared
    HOOK --> SHOW["show"]:::shared
    HOOK --> SESS["session state<br/>{SESSIONS_ROOT}"]:::shared
    SCAN --> EMB["way-embed<br/>(subprocess, EN and multilingual GGUF)"]:::util
    SCAN --> AGENT["ways agent<br/>(Unix socket, judge + key custody)"]:::util
    AGENT --> API["provider API<br/>Anthropic or OpenRouter"]:::ext
    SHOW --> MAC["macro.sh · postcheck.sh"]:::util

    IP["issues-pull.sh · issues-task-created.sh"]:::other --> GT["gh-tasks"]:::other
    AS["attend-drain-stop.sh"]:::other --> AT["attend inbox"]:::other
    SL["strip-session-link-pre.sh"]:::other --> DENY["PreToolUse deny"]:::other
```

## Multi-Trigger Semantics

What happens when one prompt matches several ways:

```mermaid
flowchart TB
    classDef prompt fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef pattern fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef gate fill:#E65100,stroke:#BF360C,color:#fff
    classDef output fill:#2E7D32,stroke:#1B5E20,color:#fff
    classDef silent fill:#78909C,stroke:#546E7A,color:#fff

    Prompt["'Let's review the PR and fix the bug'"]:::prompt

    Prompt --> KW1["github: pattern match"]:::pattern
    Prompt --> KW2["debugging: pattern match"]:::pattern
    Prompt --> KW3["quality: semantic fire"]:::pattern

    KW1 --> F1{"g(s) ≥ τ_k?"}:::gate
    KW2 --> F2{"g(s) ≥ τ_k?"}:::gate
    F1 -->|no| G1["way_keyword_gated"]:::silent

    F1 -->|yes| ORD["order_hits<br/>fixed admission order"]:::gate
    F2 -->|yes| ORD
    KW3 --> ORD

    ORD --> J{"relevance judge<br/>one request, at most 8"}:::gate
    J -->|block| B["way_judged block"]:::silent
    J -->|pass| R{"per way: inside the refire window?"}:::gate
    R -->|yes| S["way_suppressed refire"]:::silent
    R -->|no| C{"fits the 10,000-char budget?"}:::gate
    C -->|no| S2["way_suppressed context_cap"]:::silent
    C -->|yes| O["shown"]:::output
```

Each way keeps its own marker, so several ways can fire from one prompt and each re-discloses on its own `refire:` cadence. A child that fired only because its parent fired earlier in the same scan is withheld when that parent is not shown.

## Project-Local Override

```mermaid
flowchart TB
    classDef proj fill:#E65100,stroke:#BF360C,color:#fff
    classDef user fill:#6A1B9A,stroke:#4A148C,color:#fff
    classDef core fill:#1565C0,stroke:#0D47A1,color:#fff
    classDef marker fill:#00695C,stroke:#004D40,color:#fff
    classDef skip fill:#78909C,stroke:#546E7A,color:#fff

    T["way id softwaredev/delivery/github"] --> P
    P{"1. Project<br/>$PROJECT/.claude/ways/"}:::proj
    P -->|found| UseP["project way"]:::proj
    P -->|not found| U{"2. User<br/>$XDG_CONFIG_HOME/agent-ways/ways/"}:::user
    U -->|found| UseU["user way"]:::user
    U -->|not found| C{"3. Core<br/>~/.claude/hooks/ways/<br/>→ $XDG_DATA_HOME/agent-ways/hooks/ways/"}:::core
    C -->|found| UseC["shipped way"]:::core
    C -->|not found| Skip["no way"]:::skip

    UseP --> Mark["one marker per id and agent<br/>{SESSIONS_ROOT}/{sid}/ways/softwaredev/delivery/github/.marker.{agent_id}"]:::marker
    UseU --> Mark
    UseC --> Mark
```

The first root that has the id shadows it in every root below (ADR-143), for matching and for rendering alike. Project macros and postchecks run only if the project is listed in `~/.claude/trusted-project-macros`.
