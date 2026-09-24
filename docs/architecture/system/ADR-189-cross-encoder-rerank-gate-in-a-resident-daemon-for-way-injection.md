---
status: Proposed
date: 2026-09-24
deciders:
  - aaronsb
  - claude
related:
  - ADR-108
  - ADR-126
  - ADR-130
  - ADR-134
  - ADR-155
  - ADR-156
  - ADR-158
  - ADR-160
  - ADR-188
---

# ADR-189: Cross-encoder rerank gate in a resident daemon for way injection

## Context

Ways that do not fit the turn still surface. ADR-160 fixed the worst of the token-collision fires, and it left the operating points hand-set. `late_interaction.rs` labels its constants `HAND-SET and UNCALIBRATED`. Every attempt since then to remove a poorly-matched fire has meant moving one of those constants, and each move is checked against a small labelled set: 41 rows in `tests/routing-golden.tsv` and 143 in `calibration_probes.jsonl`, for a corpus of 161 ways. A move that fixes one fire can break another, and nothing measures the trade. Tuning in this state chases individual fires.

Two limits sit underneath the tuning problem.

1. **The matcher sees thin evidence.** The scan embeds the current prompt plus Claude's last response, reduced to 110 tokens (ADR-130), and compares it against each way's one-line alias (`description` + `vocabulary`). The way's body is used only in the ADR-160 confirm step, again through cosine similarity. Nothing earlier in the session reaches the matcher, so a way can fire on a turn that uses its vocabulary while the session is doing something else.
2. **A bi-encoder asks a topical question.** The query and the way are embedded separately and compared by one cosine. That measures whether the two texts share a topic. It cannot tell a turn that mentions a subject from a turn that is doing the work, and ADR-160 measured that a bi-encoder moves toward a negated topic. The confirm step inherits the same limit, because it is also a cosine between separately embedded texts.

ADR-160 probed a generative reranker (Haiku) and it kept the false positive it was meant to reject. The ADR drew the right lesson from that probe: the reranker was given only the thin alias as evidence, and the leverage is evidence quality. A pairwise model given the way's body and the session's recent context is a different test, and it has not been run.

A cross-encoder is the standard second stage in retrieve-and-rerank systems. It reads the query and the document together in one forward pass, so attention runs across both texts. That lets it represent negation and the difference between mentioning and doing. Small cross-encoders run on CPU and llama.cpp can host them, so they fit the stack that `way-embed` already uses.

ADR-160 deferred a resident daemon as "relevant only if a larger model is later introduced as a reranker." This ADR introduces that model, and a reranker cannot be reloaded on every hook invocation.

## Decision

Add a cross-encoder rerank gate as the last stage before a way is injected. Host it, together with the embedder, in a resident per-user daemon. Measure and calibrate it against a judged eval set built from real fires before it is allowed to suppress anything.

### 1. Judged eval set (prerequisite)

Build the judged eval set that ADR-160 names as its missing follow-up. Every other stage in this ADR is measured against it.

- **Source.** Replay logged `way_fired`, `way_nearmiss` and `way_keyword_gated` events from `events.jsonl`, extending `ways introspect fires`. For each event, rebuild the turn context at its logged token position from the transcript and pair it with the way's body.
- **Labels.** Each pair is labelled `relevant` or `irrelevant`. Labelling runs offline and in batch, using Claude as the judge, and a person reviews a random sample. The agreement rate on that sample is reported with the set, and a set below 90% agreement is not used for calibration.
- **Storage.** Pairs built from transcripts contain private session text. They stay under the user's local state directory and are never committed. Curated, de-identified cases graduate into `calibration_probes.jsonl` as regression rows, as ADR-158 already does with hard negatives.
- **Metric.** Precision and recall of injected ways against the labels, reported per lane (prompt, task, tool) and per way family. ADR-160's current matcher is the baseline every change is compared against.

### 2. Resident daemon

A daemon, working name `wayd`, holds the models and session context for one user.

- **Build.** It is built from the `tools/way-embed` tree and links the same llama.cpp submodule, so it ships through the existing release pipeline and adds no Python or ONNX runtime.
- **Transport.** It listens on a Unix socket under `$XDG_RUNTIME_DIR` (falling back to the ways state directory) and speaks line-delimited JSON. The socket is created with mode 0600.
- **Models.** It keeps the English embedder, the multilingual embedder when that lane is enabled (ADR-139), and the reranker resident. Scan requests that today spawn one `way-embed` process per call (single-vector, late-interaction batch, up to six body-confirm calls, and the gate re-check) become calls on the socket.
- **Lifecycle.** The first hook that finds no daemon starts one detached and continues on the fallback path. The daemon exits after 30 minutes idle. A version handshake on connect makes a hook refuse a daemon built from a different release, so an upgrade takes effect without a manual restart.
- **Fallback.** If the socket does not accept a connection within 50 ms, or a rerank call has not answered within its budget, the scan proceeds exactly as ADR-160 specifies and logs `rerank_unavailable`. A missing or slow daemon never blocks a prompt and never changes which ways fire.

### 3. Rerank gate

The gate runs once per scan, after the ADR-160 matcher and the refire engine, and before a fire is recorded.

- **Placement.** `scan_prompt_surface` collects every `PromptMatch::Fired` way, drops the ways the refire engine would suppress (`way_fire_outcome`, ADR-126), and sends the survivors to the daemon in one batch. Only the ways that pass reach `record_way_fire` and body output in `show::way_scored`. A way the gate rejects does not spend its refire budget.
- **Query.** The session context from stage 4, reduced to 384 tokens with the ADR-130 salience reducer. The current prompt is always kept whole when it fits.
- **Document.** The way's `description`, followed by its body with frontmatter and macro output removed, truncated to 256 tokens.
- **Score.** The reranker's raw logit is mapped to a probability with a logistic fit per model, `σ(a·logit + b)`, fitted on the eval set and stored in `embed-manifest.json` beside the existing embedder calibration (ADR-156). A way is injected when the probability is at least `τ_r`.
- **Exemptions.** Ways with `pattern_strict` bypass the gate, since their authors chose a deterministic trigger. Tool-lane ways delivered on PostToolUse (ADR-188) are gated with the tool event as the query.
- **Telemetry.** Every gated candidate logs a `way_reranked` event carrying the raw logit, the probability, `τ_r`, and the outcome. `ways tune` (ADR-134) reads these events to refit `a`, `b` and `τ_r`.

A reranker returns one scalar, and the only outcomes are inject or skip. A middle band that injects a one-line pointer in place of the full body was considered. It stays out of this ADR until the eval set shows a population of borderline cases where a pointer is the correct answer.

### 4. Session context

The daemon keeps a short rolling context for each `session_id`: the last three user prompts and the last two assistant responses, each reduced by the ADR-130 reducer. The prompt hook and the Stop hook append to it through the socket. The context is held in memory only and dropped when the session goes idle or the daemon exits. This is the part of the design that addresses ways firing on vocabulary the current session is not working on.

### 5. Model selection

The model is chosen by measurement on the eval set. The candidate list, in order of cost:

| Model | Parameters | Coverage |
|---|---|---|
| `gte-reranker-modernbert-base` | 149M | English, long context |
| `jina-reranker-v2-base-multilingual` | 278M | Multilingual |
| `bge-reranker-v2-m3` | 568M | Multilingual |
| `Qwen3-Reranker-0.6B` | 600M | Instruction-conditioned |

The smallest model that meets the precision target on the eval set wins. The English lane and the multilingual lane may use different models, matching the embedder split in ADR-139. A candidate is eligible only if the pinned llama.cpp submodule loads its GGUF and returns scores matching the reference implementation to within 1e-3.

The latency budget is 300 ms at p95 for six candidates on the reference CPU, measured warm. A model that exceeds it is not eligible, whatever its accuracy.

### 6. Local fine-tuning

After the stock model is gated and measured, `ways tune rerank` fine-tunes the selected reranker on the user's own labels and writes a local GGUF that the daemon prefers when present. Fine-tuning is opt-in and runs on the user's machine. Shipping a checkpoint tuned on the maintainer's sessions is a separate decision, because weights can retain training text.

### 7. Rollout

1. Build the eval set and publish the ADR-160 baseline.
2. Ship the daemon with the embedder only. This is a latency change with no behavior change, verified by identical fire sets on the eval replay.
3. Ship the reranker in shadow mode. It scores and logs `way_reranked` and suppresses nothing.
4. Turn the gate on once shadow data shows a precision gain on the eval set with recall loss under 5 points.
5. Offer local fine-tuning.

## Consequences

### Positive

- **Precision gains come from evidence the matcher lacks.** The gate reads the way's body and the session's recent turns together, which is the evidence ADR-160 identified as the lever.
- **Tuning becomes measurable.** A judged eval set replaces tuning against individual fires. It also finally calibrates the ADR-160 constants, which remain in the pipeline as the recall stage.
- **Scans get cheaper.** Resident models remove the separate `way-embed` process spawns and model loads a scan makes today, which reach ten on a surface that admits six candidates.
- **A rejected way keeps its refire budget**, so the next turn that does warrant it still gets it.
- **The stack does not grow a new runtime.** The daemon links the llama.cpp submodule already in the tree.

### Negative

- **A resident process.** It holds roughly 0.3 to 1.2 GB of RAM while running, depending on the model chosen, and adds lifecycle code: startup, idle exit, version handshake, stale socket cleanup.
- **Added latency on the prompt path.** Up to 300 ms at p95 per scan once the gate is on, against tens of milliseconds today.
- **A labelling pipeline to maintain.** The eval set drifts as ways are added and rewritten, so it has to be refreshed, and its labels depend on a judge model whose agreement with a person must be rechecked.
- **Another calibrated threshold.** `τ_r` joins τ_s and τ_k and needs the same telemetry discipline.

### Neutral

- The ADR-160 matcher stays in place as the recall stage and as the complete fallback. This ADR adds a stage behind it and changes none of its stages.
- ADR-160's deferral of a resident daemon is resolved by this ADR, on the condition that ADR stated.
- ADR-160's rejection of a generative reranker stands.

## Alternatives Considered

- **Keep tuning the ADR-160 operating points.** Rejected as the main fix. Without a judged eval set each change is unmeasured, and the bi-encoder cannot represent the mention-versus-doing distinction at any threshold. The eval set from stage 1 is still worth building for this purpose alone.
- **Small generative judge** (Qwen3-1.7B, Gemma-3-1B), reading the probability of a yes token. Rejected as too compute-heavy for the prompt path, at an estimated 0.5 to 1.5 s per scan on CPU. It is also nondeterministic across sampling settings and runtime versions.
- **Laya typed-decision model** (Convai Innovations, 421M, Apache 2.0). Rejected. Its published zero-shot accuracy on typed decisions is 0.362 against a 0.318 random baseline. Its README reports that negated requests select the negated action, which is the failure this gate exists to catch, and that the act/escalate head carries no usable signal. It runs 193 to 464 ms warm on CPU and needs Python with torch or an ONNX Runtime. Fine-tuned, it would do the same job as a fine-tuned cross-encoder at about three times the size, outside the existing stack.
- **Logistic head on existing MiniLM embeddings.** Kept as a baseline in the eval, and rejected as the gate. It is cheap and needs no daemon, and it inherits the bi-encoder's inability to model the interaction between query and document.
- **Remote reranker (Haiku or an API reranker).** Rejected. The requirement is local operation, and ADR-160's probe already showed that a remote model given thin evidence does not help.
- **Upstream `llama-server --reranking` as the daemon.** Rejected as the shipped form. It needs no new code, but it cannot hold session context, embedder calls and version checks in one process. It is a reasonable way to prototype model selection in stage 5.
