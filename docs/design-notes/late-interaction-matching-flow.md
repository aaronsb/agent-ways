# Late-interaction matching — the flow

A visual companion to **ADR-160** (chunked late-interaction matching with
softmax-share gating). It diagrams two things this session settled by watching
the matcher run: the **per-surface evidence pipeline** that decides a semantic
fire, and the **multi-channel fire model** that explains why most fires you see
are *not* semantic.

## The evidence pipeline (one surface → fire / no-fire)

The pipeline is **lenient where it ranks** (a way is admitted on its strongest
single-chunk evidence) and **strict where it confirms** (the winning way's own
body must corroborate the chunk it won). Everything downstream of *Chunk* is the
late-interaction path; a surface too sparse to chunk, or a session with no
embedder, takes the single-vector fail-safe instead.

```mermaid
flowchart TD
    S(["Surface<br>prompt · task · command lookbehind"])
    KW{"Keyword lane<br>pattern: regex match?"}
    FIREKW["FIRE — keyword channel<br>(fires before semantic)"]
    CHUNKABLE{"≥ 2 chunks<br>and embedder present?"}
    FALLBACK["Single-vector fail-safe<br>ADR-156 calibrated gate"]
    CTX["Contextualize<br>enrich sparse surface with intent"]
    CHUNK["Chunk + embed<br>each sub-unit → vector"]
    MATCH["Late interaction<br>each chunk vs every way alias"]
    PEAK["Rank by peak<br>score = max per-chunk similarity"]
    ADMIT{"Admission co-gate<br>share ≥ 0.15  OR  peak ≥ 0.50"}
    CONFIRM{"Body-confirm<br>best winning-way body chunk<br>vs the won chunk ≥ 0.35"}
    FIRE(["FIRE — semantic channel<br>carries score · surface · won chunk"])
    NOPE(["no fire"])

    S --> KW
    KW -- yes --> FIREKW
    KW -- no --> CHUNKABLE
    CHUNKABLE -- no --> FALLBACK
    FALLBACK --> NOPE
    CHUNKABLE -- yes --> CTX
    CTX --> CHUNK --> MATCH --> PEAK --> ADMIT
    ADMIT -- "neither" --> NOPE
    ADMIT -- "admitted" --> CONFIRM
    CONFIRM -- "< 0.35" --> NOPE
    CONFIRM -- "≥ 0.35" --> FIRE

    classDef surface fill:#475569,color:#ffffff,stroke:#94a3b8;
    classDef proc fill:#2d7d9a,color:#ffffff,stroke:#4a5568;
    classDef gate fill:#fbbf24,color:#1a1a1a,stroke:#d97706;
    classDef fire fill:#2d8e5e,color:#ffffff,stroke:#4a5568;
    classDef fallback fill:#f6821f,color:#1a1a1a,stroke:#d97706;
    classDef dead fill:#475569,color:#ffffff,stroke:#94a3b8;

    class S surface;
    class CTX,CHUNK,MATCH,PEAK proc;
    class KW,CHUNKABLE,ADMIT,CONFIRM gate;
    class FIRE,FIREKW fire;
    class FALLBACK fallback;
    class NOPE dead;
```

**Why the co-gate.** `share = Σ softmax-mass / n_chunks` caps a way that owns one
of N topics at ≈1/N, so on a topic-diverse prompt a specific single-chunk match
dilutes below any fixed share gate. Admitting on a decisive `peak ≥ 0.50`
recovers that match; the strict `body-confirm ≥ 0.35` then carries precision so
the looser admission does not leak noise. (`SOFTMAX_TAU = 0.08`, `TOP_K = 8`.)

## The multi-channel fire model

A way can fire through several channels, and **the channel decides how to judge
the fire** — not one rubric fits all. Keyword fires before semantic; a
session-start trigger fires on the session lifecycle, not on intent at all.

```mermaid
flowchart LR
    subgraph intent["judge on intent-relevance"]
        SEM["semantic<br>late-interaction alias match"]
    end
    subgraph pattern["judge on pattern-appropriateness"]
        KWD["keyword<br>pattern: on prompt"]
        BSH["bash<br>command lookbehind"]
        FIL["file<br>glob trigger"]
    end
    subgraph lifecycle["judge on condition-correctness"]
        STA["state<br>session-start / signal"]
    end
    subgraph ride["judge on parent correctness"]
        CHK["check-pull<br>rides its parent way"]
    end

    classDef sem fill:#7c3aed,color:#ffffff,stroke:#8b5cf6;
    classDef pat fill:#2d7d9a,color:#ffffff,stroke:#4a5568;
    classDef life fill:#fbbf24,color:#1a1a1a,stroke:#d97706;
    classDef rid fill:#475569,color:#ffffff,stroke:#94a3b8;

    class SEM sem;
    class KWD,BSH,FIL pat;
    class STA life;
    class CHK rid;
```

The trap a naive relevance classifier falls into: judging a **state** fire
(`meta/localize`, `softwaredev/freshness` at session start) by task-relevance.
Those fire *every* session start to run a gated check; the right question is
"did the gated macro surface — or correctly stay silent?", not "is this relevant
to what we're doing." An automated fire-evaluator has to route each fire to its
channel's rubric before scoring it.

## Reading it live

- `ways introspect fires --session <id>` — semantic fires only, score · surface ·
  way, lowest-score first. The suspect tail is `--max-score 0.55`.
- `ways introspect dump --session <id>` — every fire with its `trigger_channel`,
  the input to the channel-aware evaluation above.
- `ways match "<multi-sentence query>"` — authoring diagnostic: peak / share /
  confirm / outcome / won-chunk per candidate, without writing to the events log.
