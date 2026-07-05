# Long-Context Performance: Model Compatibility Reference

Empirical benchmarks for context degradation across models. These numbers inform the ways system's token-gated re-disclosure intervals.

## Source Data

Benchmarks from Anthropic's Claude 4.6 model card (March 2026).

- **GraphWalks BFS** — Long-context reasoning (multi-hop graph traversal)
- **MRCR v2, 8-needle** — Long-context retrieval (find 8 specific facts in a long context)

## Retrieval Degradation (MRCR v2, 8-needle)

![MRCR v2 Retrieval](mrcr-v2-retrieval.png)

| Model | 128K | 256K | 512K | 1M | Drop (256K→1M) |
|-------|------|------|------|-----|-----------------|
| **Opus 4.6** | — | 91.9% | ~85% | 78.3% | **-14.8%** |
| **Sonnet 4.6** | — | 90.6% | ~75% | 65.1% | **-28.1%** |
| Sonnet 4.5 | — | 10.8% | — | 18.5% | n/a (poor baseline) |
| GPT-5.4 | 79.3 | — | ~40% | 36.6% | — |
| Gemini 3.1 Pro | 71.9 | 59.1% | 39.4% | 25.9% | — |

**Key takeaway**: Opus retains ~78% retrieval accuracy at 1M — strong but not lossless. Sonnet degrades to 65%, making re-disclosure more critical.

## Reasoning Degradation (GraphWalks BFS)

![GraphWalks BFS Reasoning](graphwalks-bfs-reasoning.png)

| Model | 256K | 1M | Drop |
|-------|------|-----|------|
| **Opus 4.6** | 72.8% | 68.4% | **-6.0%** |
| **Sonnet 4.6** | 61.5% | 41.2% | **-33.0%** |
| Sonnet 4.5 | 44.9% | 25.6% | -43.0% |

**Key takeaway**: Opus reasoning is remarkably stable across context length. Sonnet's reasoning degrades sharply — by 1M it's lost a third of its reasoning capacity.

## Implications for Ways System

### The Problem

Ways originally disclosed once per session — a marker-file rule designed for 200K context windows where the entire conversation fit within a single effective attention span. That rule is retired: the engine now re-discloses on a token-distance axis (ADR-104 → ADR-123 → ADR-126). The motivating problem is unchanged — at 1M tokens:

- A way disclosed at token 50K has measurably degraded influence at token 500K
- Retrieval accuracy for that disclosure drops ~15-20% (Opus) or ~30%+ (Sonnet)
- Reasoning quality about that domain's rules degrades further
- The guidance is not *gone* — it's *faded*

### The Model

Ways system behavior should adapt to empirically measured context degradation:

```
Token position →   50K          250K          500K          750K          1M
                    │             │             │             │             │
Opus retrieval:   ~92%          ~87%          ~83%          ~80%          ~78%
Sonnet retrieval: ~91%          ~82%          ~73%          ~69%          ~65%
                    │             │             │             │             │
Way influence:    STRONG ────── WARM ────────── COOL ──────── COLD ──────── FADED
```

### Re-Disclosure Interval

The interval is **window-relative and per-way**, not a single global threshold. Each way's `refire:` preset is a fraction of the session context window; at fire evaluation the preset is multiplied by the operator's current window, so intervals scale automatically to new context tiers with no code change (ADR-126). The shipped presets:

| Preset | Fraction of window | Cadence |
|--------|-------------------|---------|
| `once` | 1.0 | effectively once per session |
| `rare` | 0.4 | ~2 re-fires across a full window |
| `normal` | 0.15 | the standard load-bearing cadence |
| `frequent` | 0.05 | re-fires on each fresh occurrence of its trigger |

A way needing finer shaping declares an explicit `curve:` block instead (ADR-123); `refire:` wins when both are present. An early design proposed a flat 25%-of-window global constant (retired ADR-104); it was superseded by these per-way presets. See `docs/hooks-and-ways/engine-reference.md` and ADR-126.

### Token Budget Consideration

Re-disclosure has a cost: each way injection is ~200-500 tokens. At the `normal` preset (~15% of the window), that's a handful of re-disclosures per way per session. For a session that triggers 5 ways, that's ~6-10K tokens total — well under 1% of even a 200K budget.

## How This Connects to Epochs

The epoch counter tracks **event distance** — turns / tool actions since a way fired. Token distance is a separate axis, and both are already live:

| Metric | What it measures | Good for |
|--------|-----------------|----------|
| **Epoch distance** | How many tool actions since way fired | Check decay (is the model still thinking about this domain?) |
| **Token distance** | How much context has accumulated since way fired | Re-disclosure (has the way faded from retrievable memory?) |

Both are useful. Epoch distance drives check scoring (ADR-103). Token distance drives way re-disclosure — the window-relative `refire:` mechanism (ADR-126; see `docs/hooks-and-ways/engine-reference.md`). They complement each other.
