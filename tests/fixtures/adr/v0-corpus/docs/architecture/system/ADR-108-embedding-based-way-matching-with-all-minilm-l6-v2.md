---
status: Accepted
date: 2026-03-21
amended_by: ADR-125
deciders:
  - aaronsb
  - claude
related:
  - ADR-014
  - ADR-107
  - ADR-125
---

# ADR-108: Embedding-Based Way Matching with all-MiniLM-L6-v2

> **Amendment note (2026-04-17):** [ADR-125](ADR-125-authored-disclosure-graph-and-removal-of-bm25.md) removed BM25 entirely. The "BM25 remains as a fallback" decision below is historical — the embedding model is now the sole retrieval tier. Sections describing the engine-selection cascade (`way-embed` → `way-match` → NCD), BM25 fields in the corpus, and the "fallback chain is automatic" behavior no longer apply.

## Context

ADR-014 introduced BM25 for semantic way matching. ADR-107 Phase 1 shipped an external corpus for correct IDF computation across 58 ways. A vocabulary audit removed 7 ambiguous terms and reduced false positives from 8 to 4 on a test prompt ("write about Dwarf Fortress and AI agent simulation").

The remaining 4 false positives expose a fundamental BM25 limitation: bag-of-words matching operates on stems, not meaning. "Agent" in SSH (ssh-agent) and "agent" in AI (autonomous agent) share the same stem. "Document" in docstrings and "document" in "write a document" are identical tokens. No amount of vocabulary tuning can fix this — the model has no concept of word sense.

The ways system now has 62 ways across 5 domains (softwaredev, itops, meta, writing, research). As the corpus grows, BM25 vocabulary collisions will increase. Each new domain introduces common terms that overlap with existing ways.

Meanwhile, the prompt-evaluation loop must remain imperceptible to the user. The current BM25 path spawns 58 processes per prompt (~120ms). Any replacement must be faster, not slower.

### Evidence

Tested 2026-03-21 in an empty directory with a creative writing prompt:

| State | Ways fired | True positives |
|-------|-----------|----------------|
| Before corpus + audit | 8-9 | 1 |
| After corpus + audit | 4 | 1 |

The 3 remaining false positives (docs, docstrings, subagents) all match on stem collisions that BM25 cannot disambiguate.

## Decision

Replace BM25 with embedding-based semantic matching using **all-MiniLM-L6-v2** as the primary way-matching engine. BM25 initially remained as a zero-dependency fallback; [ADR-125](ADR-125-authored-disclosure-graph-and-removal-of-bm25.md) subsequently removed BM25 entirely, making the embedding model the sole retrieval tier.

### Model choice: all-MiniLM-L6-v2

- **Parameters**: 22M (6 transformer layers, 384-dim embeddings)
- **License**: Apache 2.0
- **GGUF size**: ~44MB (F16), ~22MB (Q5_K_M), ~21MB (Q4_K_M)
- **Why this model**: Smallest viable sentence embedding model with strong semantic discrimination. Battle-tested in production search systems. Pre-converted GGUF available on HuggingFace (second-state/All-MiniLM-L6-v2-Embedding-GGUF).

### Architecture: pre-compute + single-spawn runtime

Way embeddings are computed at **authoring time** and stored in `ways-corpus.jsonl`. At runtime, only the user prompt is embedded — one forward pass, then cosine similarity against pre-computed vectors.

**Authoring time** (like `generate-corpus.sh` today):
```
way-embed generate --corpus ways-corpus.jsonl
```
Reads all way.md descriptions, embeds them, writes 384-dim vectors to the JSONL alongside existing BM25 fields.

**Runtime** (called by `match-way.sh`, replaces 58 `way-match pair` calls):
```
way-embed match --corpus ways-corpus.jsonl --query "user prompt"
```
One process spawn. Loads pre-computed vectors from JSONL, embeds the prompt (~3-5ms), computes 58 cosine similarities (<1ms), outputs matches above threshold.

### Timing budget

| Operation | BM25 today (58 spawns) | Embedding (1 spawn) |
|-----------|----------------------|---------------------|
| Process spawns | 58 × ~2ms = ~116ms | 1 × ~2ms |
| Model load | N/A | ~5ms (GGUF mmap) |
| Scoring | <1ms each (in-process) | ~12ms forward pass + <1ms cosine sims |
| **Total** | **~120ms** | **~22ms** |

The embedding path is **5-6x faster** than the current BM25 path while providing dramatically better discrimination. (Measured on Linux x86_64, 2 threads.)

### Binary packaging

Build on llama.cpp's GGML library (pure C tensor computation, no dependencies). Two files:

- `bin/way-embed` — static binary (~5MB), built with cosmocc for cross-platform APE
- Model weights downloaded to `${XDG_CACHE_HOME}/claude-ways/user/` (~44MB F16, ~22MB Q5_K_M)

The binary loads the model via mmap (no full read into RAM). The model is distributed via GitHub Release artifact or direct HuggingFace download, verified against a committed SHA-256 checksum. It lives in the XDG cache directory, not in the git repo (too large at 44MB).

Future option: llamafile-style single binary with model weights concatenated into the executable. Deferred until cosmocc builds of llama.cpp stabilize.

### Corpus format evolution

`ways-corpus.jsonl` gains an `embedding` field:

```json
{
  "id": "writing",
  "description": "Content creation — documents, presentations...",
  "vocabulary": "write draft compose...",
  "threshold": 2.0,
  "embedding": [0.023, -0.041, 0.118, ...]
}
```

BM25 fields (`threshold`, tokenized vocabulary) were retained in the corpus at the time of this ADR to serve the fallback path. [ADR-125](ADR-125-authored-disclosure-graph-and-removal-of-bm25.md) removed the BM25 engine; these fields are no longer read at runtime.

### Configuration (superseded)

> Superseded by ADR-125: embedding is the sole tier; no fallback cascade.

Originally, the scanner detected which engine was available:

1. If `bin/way-embed` exists and model file present → use embedding
2. Else if `bin/way-match` exists → use BM25 with corpus
3. Else if gzip + bc available → use NCD (legacy fallback)

Per ADR-125, the embedding model is a hard dependency. If the embedding engine is unavailable, matching does not degrade — it errors. This surfaces setup problems early rather than silently returning degraded results.

### Security boundary

Same constraint as ADR-107: **runtime scanners never write to `~/.claude/`**. The model file and pre-computed embeddings are authoring-time artifacts committed to the repo. The runtime binary only reads.

The model file is a published, checksummed artifact from HuggingFace. It can be verified against known hashes. It does not execute arbitrary code — GGUF is a tensor format, not an executable format.

## Consequences

### Positive

- Solves the stem-collision problem that BM25 cannot fix. "SSH agent" and "AI agent" will have distant embedding vectors despite sharing a stem.
- 15x faster than current 58-spawn BM25 path. Imperceptible to the user.
- Pre-computed embeddings mean runtime cost is independent of corpus size. 200 ways would cost the same as 58.
- Multilingual potential without per-language stemmers or vocabulary files. MiniLM handles many languages out of the box (trained on multilingual data). ADR-107 Phase 3 locale work becomes simpler.
- BM25 vocabulary tuning becomes unnecessary. New ways need only a good description — no manual keyword curation.

### Negative

- 44MB model file download (F16) or ~22MB (Q5_K_M). Not git-tracked — distributed via GitHub Release or HuggingFace. Users run `make model` to download.
- llama.cpp / GGML dependency for building the binary. More complex build than the current single-file `way-match.c`. Though the binary itself remains dependency-free once compiled.
- Model quality is fixed at MiniLM's training — it may not perfectly capture domain-specific semantics (e.g., "way" as a concept in this system vs. "way" as a path). BM25's explicit vocabulary handles this better for known edge cases.
- Quantization (Q4) trades quality for size. Need to validate that Q4 embeddings still discriminate well enough on the test fixture corpus.

### Neutral

- `way-match` binary and BM25 scoring initially remained in the repo as the fallback. ADR-125 subsequently removed both, as the fallback was rarely exercised once the multilingual model was reliably distributed.
- The `ways-corpus.jsonl` format was initially additive — embedding vectors sat alongside BM25 fields. Per ADR-125, the BM25 fields are no longer read at runtime; the corpus format will drop them when convenient.
- The `/ways-tests` skill needs to learn to score with embeddings (cosine similarity thresholds are on a different scale than BM25 scores).
- Threshold values will need recalibration. BM25 thresholds (1.8-2.5) don't apply to cosine similarity (0.0-1.0). The test fixture corpus provides the calibration data.

## Alternatives Considered

- **Larger embedding models (Snowflake Arctic, Qwen3-0.6B)**: Better quality but 1.2GB+ model files. Overkill for matching 60 short descriptions against short prompts. MiniLM's 22M parameters are sufficient for this task scale.
- **ONNX Runtime instead of GGML/llama.cpp**: Would require shipping a shared library (~15MB onnxruntime.so). Less portable than a static binary. GGML is pure C with no dependencies, matching our existing build pattern.
- **Python-based inference (fastembed, sentence-transformers)**: Adds a Python runtime dependency. Non-starter for a system that runs on bash + coreutils + static binaries.
- **Fine-tuning MiniLM on way-matching data**: Would improve domain-specific discrimination (training data exists: 74 test fixtures + 58 way descriptions). Deferred — try the off-the-shelf model first. Fine-tuning is ~$5-20 on cloud GPU if needed.
- **Hybrid scoring (BM25 + embedding combined)**: Run both engines, combine scores. More complex, harder to calibrate, and the embedding model alone should handle all cases BM25 handles plus the ones it can't. Keep it simple — one engine primary, one fallback.
- **Keep BM25 and accept the false positives**: The vocabulary audit reduced false positives from 8 to 4, but the remaining 3 FPs are structural BM25 limitations. As the corpus grows past 100 ways, vocabulary collisions will worsen. The problem doesn't stabilize — it compounds.
