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
  - ADR-161
  - ADR-187
  - ADR-188
  - ADR-190
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

Add a cross-encoder rerank gate as the last stage before a way is injected. Host it, together with the embedder, in a resident per-user daemon. The gate learns from, and is measured against, the ratings defined in ADR-190. ADR-190 owns labels, training and the weight lifecycle; this ADR owns the daemon, the gate, the lanes and model selection.

### 1. Evaluation baseline (prerequisite)

Before the gate suppresses anything, the ADR-160 matcher is measured against the human anchor slice and the per-injection ratings from ADR-190. Every stage below is compared against that baseline.

- **Metric.** Precision and recall of injected ways, reported per lane and per way family.
- **Storage.** Rated contexts contain private session text. They stay under the user's local state directory and are never committed (ADR-190 section 2).

### 2. Resident daemon

A daemon, working name `wayd`, holds the models and session context for one user.

- **Build.** It is built from the `tools/way-embed` tree and links the same llama.cpp submodule, so it ships through the existing release pipeline and adds no Python or ONNX runtime.
- **Transport.** It listens on a Unix socket under `$XDG_RUNTIME_DIR` (falling back to the ways state directory) and speaks line-delimited JSON. The socket is created with mode 0600.
- **Models.** It keeps the English embedder, the multilingual embedder when that lane is enabled (ADR-139), and the reranker resident. Scan requests that today spawn one `way-embed` process per call (single-vector, late-interaction batch, up to six body-confirm calls, and the gate re-check) become calls on the socket.
- **Lifecycle.** The first hook that finds no daemon starts one detached and continues on the fallback path. The daemon exits after 30 minutes idle. A version handshake on connect makes a hook refuse a daemon built from a different release, so an upgrade takes effect without a manual restart.
- **Fallback.** If the socket does not accept a connection within 50 ms, or a rerank call has not answered within its budget, the scan proceeds exactly as ADR-160 specifies and logs `rerank_unavailable`. A missing or slow daemon never blocks a prompt and never changes which ways fire.

### 3. Rerank gate

The gate runs once per scan, after the ADR-160 matcher and the refire engine, and before a fire is recorded. It applies to the lanes whose fires come from semantic matching, since those are the only fires a relevance model can judge.

| Lane | Hook | Query | Gated |
|---|---|---|---|
| Prompt | UserPromptSubmit | Session context from stage 4 | Yes |
| Task (subagent, teammate) | PreToolUse:Task stash, drained at SubagentStart | The delegation prompt | Yes |
| Queued message (ADR-161) | PostToolUse | The queued message, then session context | Yes |
| Tool (`commands:`, `files:`) | PostToolUse (ADR-188) | None | No |

ADR-188 retires the semantic lane on the Bash surface, so tool-lane fires come from `commands:` and `files:` patterns. Their authors chose a deterministic trigger, and the gate leaves them alone. Checks that score through `description` and `vocabulary` are also out of scope here.

The task lane is the first lane the gate turns on for (stage 7). Ways stashed for a subagent are emitted at SubagentStart without marker checks, so a false positive is paid in full on every spawn, into a fresh context where it is a large share of what the subagent reads. The delegation prompt is also the best query any lane produces: it is written to stand alone, and it needs no session context.

- **Placement.** In `scan_prompt_surface`, the scan collects every `PromptMatch::Fired` way, drops the ways the refire engine would suppress (`way_fire_outcome`, ADR-126), and sends the survivors to the daemon in one batch. Only the ways that pass reach `record_way_fire` and body output in `show::way_scored`. A way the gate rejects does not spend its refire budget. In `scan::task`, the gate runs on the matched list before the stash file is written, so a rejected way never reaches the subagent. The queued lane follows the prompt lane.
- **Query.** On the prompt and queued lanes, the session context from stage 4, reduced to 384 tokens with the ADR-130 salience reducer, with the current prompt kept whole when it fits. On the task lane, the delegation prompt reduced to 384 tokens.
- **Document.** The way's `description`, followed by its body with frontmatter and macro output removed, truncated to 256 tokens.
- **Score.** The reranker's raw logit is mapped to a probability with a logistic fit per model, `σ(a·logit + b)`, fitted on ADR-190's rated injections and stored in `embed-manifest.json` beside the existing embedder calibration (ADR-156). A way is injected when the probability is at least `τ_r`.
- **Exemptions.** Ways with `pattern_strict` bypass the gate on every lane, since their authors chose a deterministic trigger.
- **Telemetry.** Every gated candidate logs a `way_reranked` event carrying the raw logit, the probability, `τ_r`, and the outcome. `ways tune` (ADR-134) reads these events to refit `a`, `b` and `τ_r`.

A reranker returns one scalar, and the only outcomes are inject or skip. A middle band that injects a one-line pointer in place of the full body was considered. It stays out of this ADR until the ratings show a population of borderline cases where a pointer is the correct answer.

### 4. Session context

The daemon keeps a short rolling context for each `session_id`: the last three user prompts and the last two assistant responses, each reduced by the ADR-130 reducer. The prompt hook and the Stop hook append to it through the socket. The context is held in memory only and dropped when the session goes idle or the daemon exits. This is the part of the design that addresses ways firing on vocabulary the current session is not working on.

### 5. Model selection

Latency decides the field before ranking quality does. A scan scores up to six pairs of about 640 tokens each, roughly 3,800 tokens per call. The one published CPU comparison across these models (the Ettin reranker release, Hugging Face, May 2026, a desktop i7 on short documents) measures 150M-parameter cross-encoders at about 15 pairs per second and `bge-reranker-v2-m3` at 6. Scaled to this input, that projects to about 2 s and 8 s per scan. Only models of about 35M parameters or fewer project inside the budget. These are estimates. Stage 5 replaces them with measurements on the reference CPU before any model is chosen.

| Role | Model | Parameters | License | Pinned llama.cpp (`ec2b787`) |
|---|---|---|---|---|
| Gate | `ettin-reranker-17m-v1`, `ettin-reranker-32m-v1` | 17.6M, 32.8M | Apache-2.0 | Needs a patch, below |
| Gate baseline | `ms-marco-MiniLM-L6-v2`, `jina-reranker-v1-tiny-en` | 22.7M, 33M | Apache-2.0 | Supported |
| Teacher | `Qwen3-Reranker-0.6B`, `Qwen3-Reranker-4B` | 0.6B, 4B | Apache-2.0 | Supported |
| Generator and judge | A Qwen3 instruct model | varies | Apache-2.0 | Supported (Qwen3 architecture) |

The Ettin rerankers are the lead candidates. On MTEB English retrieval, averaged over six first-stage retrievers, `ettin-reranker-32m-v1` scores 0.578 against 0.553 for `bge-reranker-v2-m3`, at about a seventeenth of its size, and it reads up to 8k tokens. The pinned llama.cpp hard-codes mean pooling for ModernBERT on the rank path, to match `gte-reranker-modernbert-base`, while Ettin uses CLS pooling followed by a two-layer head. Running Ettin needs a patch that selects pooling from the GGUF metadata, plus a repack of its Sentence Transformers head. The patch is offered upstream and carried on the submodule until it lands. If it cannot be carried, the baseline models ship, since they run on the pinned commit unchanged.

Qwen3-Reranker is the only small reranker trained to follow a task instruction. It scores +5.4 (0.6B) and +14.8 (4B) on FollowIR, where BERT-family cross-encoders score about zero. It is too slow to gate on the prompt path at this input size, and it serves as the teacher in stage 6. Community GGUFs of it are often broken, so it is converted from source.

Every gate candidate is English-only. The multilingual lane (ADR-139) keeps the ADR-160 matcher without a gate until a multilingual cross-encoder of this size exists.

Models under non-commercial licenses are excluded, because the corpus ships under MIT and adopters use it commercially. That removes `jina-reranker-v2-base-multilingual`, `jina-reranker-v3`, `jina-reranker-v3.5` and `jina-colbert-v2`.

A candidate is eligible only if its converted GGUF loads in `wayd` and returns scores matching the reference implementation to within 1e-3.

The latency budget is 300 ms at p95 for six candidates on the reference CPU, measured warm. A model that exceeds it is not eligible, whatever its accuracy. If no candidate meets the budget at the stage 3 token sizes, the query and document budgets shrink before a larger model is considered.

### 6. Training

Every gate candidate was trained on web-search pairs, and none of them follows an instruction, so the gate model is trained for this task before the gate turns on. How it is trained, how it keeps learning, and how weights ship and are adopted are governed by ADR-190. The first base is distilled from `Qwen3-Reranker-4B` on synthetic pairs built from the public corpus, as ADR-190 section 6 describes. A Qwen3 instruct model generates the turns and settles the pairs the teacher is unsure of.

Every model that generates or labels data for a shipped checkpoint is open-weight, with a licence that places no restriction on training from its output, and the release manifest records that licence chain. Claude-derived labels train only local state (ADR-190 section 7).

### 7. Rollout

1. Measure the ADR-160 baseline (stage 1). ADR-190's per-way offsets can already ship at this step.
2. Ship the daemon with the embedder only. This is a latency change with no behavior change, verified by identical fire sets on the eval replay.
3. Ship the distilled reranker in shadow mode on every gated lane. It scores and logs `way_reranked` and suppresses nothing.
4. Turn the gate on for the task lane once shadow data shows a precision gain on that lane's eval rows with recall loss under 5 points.
5. Turn it on for the prompt and queued lanes against the same criterion, measured on their own rows.
6. Continual local tuning, per ADR-190.

## Consequences

### Positive

- **Precision gains come from evidence the matcher lacks.** The gate reads the way's body and the session's recent turns together, which is the evidence ADR-160 identified as the lever.
- **Tuning becomes measurable.** ADR-190's ratings and anchor slice replace tuning against individual fires. They also calibrate the ADR-160 constants, which remain in the pipeline as the recall stage.
- **Scans get cheaper.** Resident models remove the separate `way-embed` process spawns and model loads a scan makes today, which reach ten on a surface that admits six candidates.
- **A rejected way keeps its refire budget**, so the next turn that does warrant it still gets it.
- **The stack does not grow a new runtime.** The daemon links the llama.cpp submodule already in the tree.

### Negative

- **A resident process.** It holds the embedders and a reranker of 35M parameters or fewer, on the order of a few hundred MB, and adds lifecycle code: startup, idle exit, version handshake, stale socket cleanup.
- **Added latency on the prompt path.** Up to 300 ms at p95 per scan once the gate is on, against tens of milliseconds today.
- **A training dependency.** The gate needs a trained base before it can turn on, and ADR-190's loop to stay current.
- **A carried llama.cpp patch** for Ettin's pooling, until upstream accepts it.
- **An English-only gate.** The multilingual lane gets no reranker from this ADR.
- **Another calibrated threshold.** `τ_r` joins τ_s and τ_k and needs the same telemetry discipline.

### Neutral

- The ADR-160 matcher stays in place as the recall stage and as the complete fallback. This ADR adds a stage behind it and changes none of its stages.
- ADR-160's deferral of a resident daemon is resolved by this ADR, on the condition that ADR stated.
- ADR-160's rejection of a generative reranker stands.

## Alternatives Considered

- **Keep tuning the ADR-160 operating points.** Rejected as the main fix. Without judged labels each change is unmeasured, and the bi-encoder cannot represent the mention-versus-doing distinction at any threshold. ADR-190's per-way offsets tune the ADR-160 thresholds from ratings, which is the measured version of this alternative.
- **Small generative judge** (Qwen3-1.7B, Gemma-3-1B), reading the probability of a yes token. Rejected as too compute-heavy for the prompt path, at an estimated 0.5 to 1.5 s per scan on CPU. It is also nondeterministic across sampling settings and runtime versions.
- **Laya typed-decision model** (Convai Innovations, 421M, Apache 2.0). Rejected. Its published zero-shot accuracy on typed decisions is 0.362 against a 0.318 random baseline. Its README reports that negated requests select the negated action, which is the failure this gate exists to catch, and that the act/escalate head carries no usable signal. It runs 193 to 464 ms warm on CPU and needs Python with torch or an ONNX Runtime. Fine-tuned, it would do the same job as a fine-tuned cross-encoder at about three times the size, outside the existing stack. Used zero-shot as a reranker on NFCorpus (323 queries), both of its checkpoints ranked below plain BM25, and it ran 27 to 31 times slower per pair on CPU than a 22M MiniLM cross-encoder.
- **Logistic head on existing MiniLM embeddings.** Kept as a baseline in the eval, and rejected as the gate. It is cheap and needs no daemon, and it inherits the bi-encoder's inability to model the interaction between query and document.
- **Jev** (TypeSafe, hosted System One decision model). Rejected as the gate. Its request shape fits this gate well: one state holding the context and every candidate, one yes/no question per way, and a probability per way. In that shape it beat `bge-reranker-v2-m3` and monoT5-3B by 4 to 8 nDCG@10 points on three BEIR sets (S1Rank), and a per-turn skill router built on it loaded the correct skill 96% of the time. It fails the requirements on four counts. Weights are not released, so transcript text would leave the machine on every prompt, and no retention window is published below the enterprise tier. It takes no seed, and about half of all probabilities changed across byte-identical requests, so a way near the threshold would flip between identical prompts. Its client-side p95 is 1 to 2 s, and one run saw 5% of calls fail with HTTP 503. Its calibration also shifts with how often relevant items occur. Its one-call-for-all-candidates shape is adopted locally in stage 3's single batch.
- **Larger local cross-encoders as the gate** (`bge-reranker-v2-m3`, `gte-reranker-modernbert-base`, `Qwen3-Reranker-0.6B`). Rejected for the prompt path on projected latency, at about 1 to 8 s per scan on CPU. Qwen3-Reranker is kept as the stage 6 teacher.
- **Late interaction over pre-encoded way bodies** (`mxbai-edge-colbert-v0`, `answerai-colbert-small-v1`). Held as the fallback. Way bodies are static, so they can be encoded once at corpus build time, and a scan then encodes only the query, which should cost well under 100 ms. It is not the first choice because it scores token similarity without cross-attention, which is the limit this ADR is trying to get past, and because MaxSim scoring would run outside llama.cpp. It becomes the design if no cross-encoder meets the budget after the token budgets shrink.
- **A classifier head per way on a frozen encoder** (the stuntd pattern). Rejected. It needs about 300 labelled examples per question, which is about 48,000 across 161 ways, and every new way needs its own head.
- **Remote reranker (Haiku or an API reranker).** Rejected. The requirement is local operation, and ADR-160's probe already showed that a remote model given thin evidence does not help.
- **Upstream `llama-server --reranking` as the daemon.** Rejected as the shipped form. It needs no new code, but it cannot hold session context, embedder calls and version checks in one process. It is a reasonable way to prototype model selection in stage 5.
