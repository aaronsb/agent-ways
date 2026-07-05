## The Tool-Use Channel is a Signal Problem: Lookbehind, Chunk-Spread, and Winner Confirmation

> **Type:** Design note (not an ADR)
> **Status:** Exploratory — prototype findings from a 2026-07-05 probe session, pre-decision
> **Cites:** ADR-125 (embedding as hard dependency), ADR-155 (the keyword gate), ADR-156 (calibrated fire in probability space, `g(s) ≥ τ_s`), and the sibling note [The Lexical Gate as a Conditional Threshold](./lexical-gate-as-conditional-threshold.md)
> **Motivates:** a possible ADR for the tool-use (bash) channel — embedding the *intent behind* a command rather than the command string; a `way-embed match --batch` addition; a per-way body cross-similarity confirmation step; and a project/domain scope gate

## What this note is

A reading of *why* the tool-use channel over- and mis-fires, arrived at empirically. The trigger was a felt symptom — executive-assistant ways (`ea/tasks/time`, `ea/briefing`) injecting into a software-development repo — and a matching diagnostic gap: the `ways rethink` drill-down labels an embedding fire *"semantic fire — matched by embedding; no recoverable term,"* because cosine similarity between two dense vectors has no term-level attribution. The investigation set out to explain the mis-fires and closed on a single reframe:

**We were feeding a competent matcher thin evidence and thresholding it once.** The fix is not a better *scorer* (a reranker, a larger model, a reasoning gate) — it is better *signal* into the scorer we already ship (ADR-125/156), plus a deterministic confirmation that reuses the same embedder. Every stage below is the bi-encoder already in the tree; no reasoning model and no resident daemon are required.

The note records the reading and the measurements that forced it. It decides nothing; the ADRs it motivates would.

## The reading: signal, not scoring

A way carries an *alias* — its `description` plus `vocabulary`, embedded once into a unit vector `a_w` (ADR-125). At match time a surface (a prompt, a bash command, a task) embeds to `q`, and relatedness is `s = cos(q, a_w)`, mapped by the calibrated logistic `g(s)` to a relevance probability and fired when `g(s) ≥ τ_s` (ADR-156). The prompt channel embeds the human's words; the tool-use channel embeds the **command string**.

That last choice is the defect. A shell command is an *action*, not a statement of intent, and its bag of tokens collides with any alias that shares those tokens. The canonical failure: a documentation-freshness check —

    git log -1 --format=%ct -- README.md docs; git rev-list --count HEAD; date +%s; months=…

is wall-to-wall `log`, `entry`, `report`, `count`, `date`, `months`. Those are exactly the vocabulary of `ea/tasks/time` (*"time tracking, logging billable hours, time entries, invoicing, billing reports"*). The matcher is not wrong to notice the overlap; the **input** told it this was about logging time.

Two consequences follow, and the rest of the note is their treatment: the tool-use surface is *signal-starved* (a command is too sparse to carry intent), and where it does carry signal, that signal is often the wrong homonym.

## The tool-use surface is signal-starved

Measured against the live 157-way corpus. Many real commands produce **no** semantic candidate at all — every way scores below way-embed's emission floor:

- `crontab -e`, `psql -c "ALTER TABLE …"`, `date -d @… +%F`, `curl -X POST …/notify`, `tail -f app.log`: nothing above floor.
- The commands that *do* fire tend to be the token-dense ones, and they fire the wrong thing: the freshness command above ranked `ea/tasks/time` at **#2 (cos 0.39)** — a top-two cross-domain false positive in a docs check.

So the channel's problem is bimodal: silence on sparse commands, and confident error on dense ones. Neither is fixable by adjusting `τ_s`; the input has to change.

## The lookbehind: where the intent actually lives

The intent that the command lacks is sitting one step behind it, in the transcript. The tool-use hook fires with the session transcript on disk, and its structure (observed on this session's `.jsonl`) is:

- Each assistant content block is its **own** record — text, thinking, and `tool_use` never share a record. A `Bash` `tool_use` record carries no adjacent prose.
- Thinking text is **empty** under Opus 4.8's default `display: omitted` — not a usable signal.
- The recurring shape is `user prompt → thinking(empty) → assistant text (narration) → tool_use(Bash)`.

So a lookbehind that walks backward from the command to the nearest assistant `text` record (and the originating human prompt) recovers the intent prose the command omits. Scored against the same two candidates, the effect is decisive:

| query embedded | `ea/tasks/time` | `softwaredev/freshness` | winner |
|---|---|---|---|
| bare command | 0.21 | 0.18 | billing (by 0.03 — a coin-flip) |
| narration + command | 0.16 | **0.48** | freshness (by 0.32 — decisive) |

The narration turns a coin-flip into a clean separation: the false positive drops, the true way triples. This is the highest-leverage change in the note, and it is pure signal — no new model, no corpus edit.

Two cautions the real data imposed. First, `last-prompt` transcript records are **truncated** (~200 chars) and raw `user` records are **polluted** with injected hook context, skill dumps, and tool-result XML — extracting clean human text is itself non-trivial; the assistant `text` record is the cleaner source. Second, real narration is weaker than hand-written intent: an assistant often narrates the *investigation* (*"There it is — look at `way_fired`…"*) rather than the *command's* intent, and technical narration is code-token-dense (stripping inline code leaves thin prose; keeping it injects identifier noise). Lookbehind helps most in agentic "let me check X" flows and least in investigation flows. It is an improvement, not a solvent.

## Chunk-and-spread: robustness, and the peak/breadth split

Embedding the whole narration-plus-command as one vector is worse than it sounds: a long string is averaged into a single vector, and MiniLM truncates past its 128-token window. On real pairs the single-concat query frequently collapsed to nothing or to one diluted foreign hit.

Chunking the prose into sentences, embedding each, and aggregating per way is strictly more robust — no single token-dense line can dominate, and cross-domain junk washes out. But the **combiner matters**, and this is the note's first inversion:

- **Max** (a way's best chunk) preserves *specificity* — the single strongest signal wins.
- **Noisy-OR / vote-count / sum** reward *breadth* — a way that matches many chunks weakly outranks a way that matches one chunk strongly.

On the freshness case, `max` puts `softwaredev/freshness` first; noisy-OR promotes generic git ways (`repoaudit`, `commits` — three weak chunks each) and buries `freshness` at #4. The prose genuinely had three git-mechanics sentences and one freshness-intent sentence, so any breadth-favoring combiner mis-ranks the specific target. **Rank by peak.**

A negative result worth recording so it is not re-attempted: writing an explicit boundary into a way's alias — *"Not for git log inspection, computing commit ages…"* — makes the collision **worse** (the freshness-command cosine to `ea/tasks/time` rose 0.31 → 0.49). A bi-encoder cannot represent negation; naming the anti-example places its tokens in the vector and pulls *toward* it. Better alias text means *positive intent phrasing only*; a positive-only rewrite lowered the false-positive cosine (0.21 → 0.17) while holding the true prompt (0.59). Negation belongs only to a stage that can read "not" — never to embedded text.

## The chatter gate: softmax-share through the existing fire gate

The operator's framing — *"multi-chunk softmax where we also remember that we have thresholds to hit"* — is the chatter-reduction mechanism. Per chunk, softmax over that chunk's candidate ways is **zero-sum**: each chunk distributes exactly 1.0 of mass, so a way must *win chunks relative to its competitors*, not merely clear an absolute floor. This directly attacks the flat-cosine floor — everything scores 0.2–0.4, related or not — which is a documented geometry property of sentence-embedding spaces (*anisotropy*), not a quirk of this corpus; normalizing within a chunk converts an uninterpretable absolute cosine into a competitive one.

One hole to record: because softmax is zero-sum, it *always* hands the winner mass — it cannot distinguish "one clearly-relevant way" from "nothing relevant, one least-bad way." The winner confirmation below is what rescues that; dropping it would silently reintroduce chatter.

Measured, on the freshness pair: a baseline single-query gate at a fixed cosine floor fires **five** ways (only one intended); routing per-chunk softmax-share into a gate collapses the chatter. The operator's second half — *remember the thresholds* — is the elegant part: this is **not** a new gate. It is a better-conditioned input to the calibrated fire gate the system already runs (ADR-156, `g(s) ≥ τ_s`). Same thresholds, competition-normalized aggregated evidence instead of one raw cosine.

The qualifier: softmax-**sum** inherits the breadth bias (it ranked `repoaudit` above `freshness`, same failure as noisy-OR), and the operating point is a genuine calibration surface — a hand-set share threshold over-suppressed to zero fires, and it interacts with temperature `τ` and chunk count. So: **peak ranks *which* way; softmax-share gates *whether* to fire; the operating point wants calibration like `τ_s`, not a constant.**

## Winner confirmation: body cross-similarity, and the second inversion

The final stage is winner-only and cheap, and it repairs the one thing the upstream stages cannot: the breadth bias that still lets a generic near-miss outrank the specific target. After a winner is chosen, chunk the winner's **full body** (the richest text a way owns, far richer than its alias) and cross-compare it to the query chunks — for each query chunk, its best cosine among the body chunks — then aggregate. Measured across the true winner and its two rivals:

| candidate | meanBest | maxBest |
|---|---|---|
| `softwaredev/freshness` (true) | **0.462** | 0.561 |
| `softwaredev/code/supplychain/repoaudit` (generic near-miss) | 0.353 | 0.570 |
| `ea/tasks/time` (cross-domain false positive) | 0.329 | 0.398 |

By **meanBest** the ranking is correct and clean: `freshness` > `repoaudit` > `ea`. This is the second inversion, and it is the key result: upstream, **max** discriminated (pick the way that *peaks* on some chunk); here, **mean** discriminates. `maxBest` is fooled — `repoaudit` (0.570) ≈ `freshness` (0.561), because "git log" spikes against "git history audit" on a *single* chunk. `meanBest` requires the winner's whole body to align *across all* query chunks, which a true match does and a collision cannot: `repoaudit` spikes on one chunk and is flat elsewhere. **Mean-of-best is a collision detector** — it separates "broadly relevant" from "shared one token," and it rejects both the generic near-miss and the cross-domain false positive that survived every earlier stage.

This realizes the "better source evidence" idea deterministically: the way's body is the evidence, cross-chunk breadth of alignment is the test, and the whole step is the bi-encoder already shipped, run only on the winner (a handful of similarity calls).

## Two leaks the signal fix does not touch

The signal work fixes the *homonym* class. Two other contributors to the felt over-firing are orthogonal, and honesty requires naming them:

1. **Foreign-project pollution.** Of 157 corpus entries, **~47 are project-local ways from other repositories** (`ai-knowledge-graph-system`, `jason-life-roadmap`, `seattle-research`, and others). They are eligible to fire in any repo. Richer query signal does not remove them — it slightly *worsened* it (a `pytest` narration pulled in another project's `kg/testing`). This wants a **deterministic project/domain scope gate**, not query enrichment, and it is likely the largest quiet contributor to cross-domain injections.

2. **Self-reference.** Discussing a way by name fires it. The query *"whether `ea/tasks/time` and `ea/briefing` keep getting triggered wrongly"* scores `ea` 0.41, `ea/briefing` 0.37, `ea/tasks` 0.35 — above `τ_s`. A session *about* the executive-assistant ways keeps re-firing them; the embedder cannot distinguish "mentioning X to debug it" from "wanting X." Richer context mitigates this but cannot eliminate it — it is a floor property of any embedding channel.

## What this implies

The stages compose into one pipeline, entirely on the existing embedder:

1. **Lookbehind** — from the tool-use hook, pull the pre-command prose (nearest assistant `text` record since the last human turn, plus the human prompt) from the transcript.
2. **Chunk** the query prose and command into sentences / sub-commands.
3. **Batch-embed** the chunks in one process load — the net-new primitive, `way-embed match --batch` (the `similarity --batch` path shows the plumbing exists; a single cold load is ~22ms, so batching, *not* a daemon, is the cost fix).
4. **Rank by peak** (max per-chunk cosine) — specificity.
5. **Chatter gate** — per-chunk softmax-share, fed into the ADR-156 calibrated fire gate.
6. **Winner confirmation** — chunk the winner's body, cross-compare to the query chunks, require `meanBest ≥ τ_c`.
7. **Scope gate** — deterministic project/domain filter over the candidate pool, addressing the foreign-project leak throughout.

The reasoning-model reranker and the resident daemon, which earlier turns treated as the endpoint, drop out. A single-call Haiku reranker was probed (≈$0.0014/call, 8→1 candidates) and *kept* the very false positive it was meant to reject when handed only the thin alias — confirming that the leverage is evidence and gating, not model capability. The daemon remains relevant only if a genuinely larger *reasoning* model is ever introduced; the embedding tier does not need it.

## Where this sits in the literature

A source review (2026-07-05) placed every stage in established information-retrieval practice; the pipeline is a competent re-derivation of the modern *late-interaction + cascade* playbook, not a new method. Naming the lineage so a future reader inherits its tuning wisdom (and its known failure modes):

- **Lookbehind** is *conversational query reformulation / contextualization* (CQR/CDR) — resolving a context-dependent query against conversation history. It is specifically *not* pseudo-relevance feedback, which expands from first-pass *results*; we expand from context that already exists (Lin et al., arXiv:2104.08707; History-Aware Conversational Dense Retrieval, arXiv:2401.16659).
- **Chunk-and-match** is *multi-vector late interaction* — the MaxSim / Chamfer operator of ColBERT — at sentence-chunk rather than token granularity (Khattab & Zaharia, SIGIR 2020; Answer.AI, "A little pooling goes a long way").
- **Softmax-share gate** is *distribution-based score normalization against embedding anisotropy* plus softmax/temperature calibration; the motivation is standard, the specific gating recipe ad-hoc (Kanoulas et al., score-distribution modeling; Kobayashi, ICCV 2025).
- **Winner confirmation** is *two-stage retrieve-and-rerank (cascade / coarse-to-fine)* with a late-interaction reranker; our mean-of-max is *length-normalized* late interaction, a variant of ColBERT's canonical sum-of-max (Lin et al., arXiv:2010.06467).
- **The negation backfire** is a well-documented single-vector bi-encoder limitation — dense embeddings do not represent logical operators — and the literature's remedy is structural (metadata, rule, or contrastive signal), never negated text. This is the independent argument for making the *scope gate*, not alias wording, carry exclusion (LogiCoL, arXiv:2505.19588; CoDeR, arXiv:2606.13204).

Two things are ours to state as rationale rather than borrow as method. The **problem framing** — a sparse shell command matched against one-line guidance aliases, contextualized from the transcript — is the specific setting, not a general result. And the pipeline takes **deliberately opposite stances on breadth at its two ends**, which is the load-bearing design choice: the first pass ranks by *max* over query chunks (specificity — it discards how many chunks agreed) as a lenient recall gate; the confirmation uses *mean* of per-chunk max (it *requires* agreement across chunks) as a strict coverage check. ColBERT's canonical sum-of-max sits between them, rewarding breadth without normalizing for it. The two-stance design is coherent — lenient recall, then strict coverage — *because* the stages sit at opposite ends of the cascade, and the confirmation's breadth requirement is precisely what rescues the zero-sum softmax from surfacing a least-bad winner when nothing is truly relevant.

## What is unmeasured — carry into any ADR

- **Calibration at scale.** Every operating point here (`τ` for softmax temperature, the softmax-share gate, `τ_c` for body confirmation) was read off one or a few cases. The body-confirmation separation (0.46 / 0.35 / 0.33) is real and correctly ordered, but `repoaudit`-vs-`ea` is close; the confirm/reject threshold needs corpus-wide fitting, the same discipline ADR-156 applied to `τ_s` — not a hand-picked constant.
- **Real-narration weakness.** Lookbehind quality is bounded by whether narration states the command's intent; investigation flows and code-dense narration degrade it. The pipeline should tolerate a weak or absent lookbehind (fall back to command-only, or to the scope gate alone).
- **Clean human text.** `last-prompt` truncates and raw `user` records are polluted; a reliable "what the human actually typed" extractor is a prerequisite for using the human prompt as signal.

## Probe log

The load-bearing claims, each measured on the live corpus during the 2026-07-05 session (numbers embedded above; scripts were throwaway):

1. Bare vs narration+command query → false positive 0.21→0.16, true way 0.18→0.48.
2. Corpus sweep (157 ways) → sparse commands below floor; token-dense freshness command ranks `ea/tasks/time` #2 (0.39) bare, gone with narration.
3. Negation-in-alias backfire → 0.31→0.49. Positive-only rewrite → 0.21→0.17, true prompt holds 0.59.
4. Self-reference → naming `ea` ways fires them (0.35–0.41).
5. Chunk-spread combiner → `max`/`mean` rank `freshness` first; noisy-OR/votes promote generic git ways.
6. Real-narration run → chunk-aggregate robust where bare/concat collapse; project self-reference surfaces (`meta/deployment`); the real token-dense pair still floated `ea/tasks/time` to #4, i.e. the signal fix alone is incomplete without the scope gate.
7. `way-embed` cold load ≈ 22ms → `match --batch`, not a daemon.
8. Softmax-share gate → baseline fires 5 ways; softmax-share collapses chatter but softmax-*sum* keeps the breadth bias; operating point needs calibration.
9. Winner body cross-similarity → `meanBest` ranks `freshness` (0.46) > `repoaudit` (0.35) > `ea` (0.33); `maxBest` fooled by single-chunk collision. Mean-of-best is a collision detector.
