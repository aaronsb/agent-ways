---
contract: adr/v1
kind: decision
verb: add
capability: matching
basis:
  - operator: aaronsb
    level: directed
    said: "my opinion after letting the discussion stew in my mind for a week or so is that it's overly complicated and the most valuable thing out of it is just something that can evaluate plain text of the way summary sort of like this: \"is this way [summary of the way] contextually related to the last turns of human and|or agent conversation? [plain text stream of turns in scope]\" the answer is literally yes or no, and if no, then the way is not injected"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "we're not looking for perfection - if every porkchop were perfect we couldn't have hotdogs. we're just looking to improve the discrimination of ways injection"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "my hope is that we have a statistically significant improvement in a ways injection, and that on this specific system, a local daemon running and handling responses doesn't add more than 100 msec of latency."
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "let's use haiku for now, leave the ways embedding search system (since we know it at least gets us in the ballpark). I think the daemon still makes sense because it's low latency (not in the mcp server) - unless you disagree. we should provide support for anthropic and a model picker, we'll strongly highly recommend haiku, and store in the profile config the properties that are adjustable, and we'll also support openrouter, again, just pointing at haiku in openrouter too"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "then we need to be able to log but silently degrade to essentially the decision model we have right now"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "the exchange is the same regardless of the judge model"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "I still feel agent-ways has a chance to be a generalized hook injection system for other agent tools (like codex or gemini etc)"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - evidence: ADR-195
  - precedent: ADR-160
agent:
  name: claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "we're not pursuing the local model, we'll use a remote (haiku, or possibly something else compatible) against anthropic or openrouter. we should have a bounded timer that fails open (essentially, as if we had not configured a valid api key or never used the decision). - we should log failures so we can get an idea for latency vs passthrough"
    via: "chat, session 418e1be3, 2026-10-01, answering the probes on PR #667"
    covers: [latency]
  - operator: aaronsb
    said: "a key that works is an implicit approval to use the gate. shadow mode is something we can activate (defeating the gate)"
    via: "chat, session 418e1be3, 2026-10-01, answering the probes on PR #667"
  - operator: aaronsb
    said: "this is a significant improvement"
    via: chat, session 418e1be3, 2026-10-01, after the failure rates at threshold 0.3 were shown
    covers: [quality]
status: accepted
date: 2026-10-01
deciders:
  - aaronsb
related:
  - ADR-160
  - ADR-193
  - ADR-194
  - ADR-195
  - ADR-502
  - ADR-604
---
# ADR-196: A yes/no relevance gate on way injection, judged by a hosted model

## Summary

- **Decided:** Before a way the matcher fired is injected, a judge model is asked whether that way is relevant to the conversation's recent turns, and a way judged irrelevant is not injected. The first engine is a hosted model, Claude Haiku 4.5, reached through the Anthropic API or OpenRouter. The embedding matcher stays as the first stage. The gate runs in the per-user ways agent (ADR-502), logs every decision, and enforces as soon as a working key is configured. Shadow mode is an opt-in that logs without blocking. Any failure, including a judge call past its deadline, fails open to today's behaviour.
- **Trades away:** A network round trip of about 0.8 s on gated prompts, bounded by a deadline past which the ways pass ungated, a paid API key, and conversation text sent to a third-party provider for every judged candidate. The gate has no local engine.
- **One-way?** No. The gate is configuration, off without a working key, and shadow mode restores today's injections.
- **Probes:** *Confident (quality):* blocking about six in ten off-topic injections while losing about two in a hundred relevant ones is the improvement you were after, even though nine in ten injections are noise today. *Not confident (latency):* a second of added wait on prompts that have candidates is acceptable while a local engine is pursued, given the 100 ms north star.
- **Inversion:** Inject everything the matcher picks, or inject only what a model confirms. The decision confirms with a model where one is configured and falls back to the matcher where not.

## Context

The matcher selects ways by embedding similarity with calibrated thresholds (ADR-160). A probe of live fires (ADR-195, docs/research/yesno-relevance-gate) found that about nine injections in ten are off-topic, and that the matcher's score barely separates relevant from irrelevant fires on a random sample (AUC 0.53). Every candidate the matcher picks is topically plausible, so what remains to decide is whether the conversation is doing the work the way guides, or only touches its words.

A model asked that question directly, with the way's path and description against the recent turns, separates the two well when the model reasons about the conversation (Haiku: AUC 0.94 on the random sample) and poorly when it ranks documents (a 0.6B reranker: 0.49). Claude Code offers no hook-side model call that can gate one hook's output (prompt hooks only allow or block; hooks run in parallel; MCP sampling is not implemented), and agent-ways aims to serve other agent tools as well, so the gate belongs to agent-ways.

## Decision

1. **Where it sits.** The gate runs after the matcher and the refire engine, and before a fire is recorded or a way's body is emitted. A way it rejects does not spend its refire budget. It applies to ways the matcher selected semantically. Ways fired by a `commands:` or `files:` pattern, and ways with `pattern_strict`, are not gated in the first release; broad patterns on the tool lane are a separate fix (#660).
2. **The question.** One request per prompt carries the recent turns and every candidate. For each candidate the judge returns P(yes) that the way is relevant to what the conversation is doing. The way text is the way's path through the ways tree followed by its description, the best input in the probe. The context window is configurable; the default is the last turn, with up to about 4,000 characters and a session gist as the tuned alternative.
3. **The contract is engine-independent.** Every engine takes the same request and returns the same response: per candidate, P(yes); plus the engine, the latency and any fallback reason. Each engine's threshold and calibration live in its profile, because engines' scores are not on one scale.
4. **Engines.** Remote engines through two providers: the Anthropic API and OpenRouter (OpenAI-compatible API). Both default to Claude Haiku 4.5, and a model picker lists each provider's models, recommends Haiku strongly, and warns on slower or costlier picks. The gate has no local engine; a local model is outside this decision, and #666 keeps it as later exploration.
5. **Configuration.** Engine profiles ship with tuned defaults: provider, model, key source, threshold, calibration, timeout, context window, concurrency, and mode. A user layer overrides any field and survives updates. `ways agent tune` fits an engine's threshold on a calibration set and writes it to the user layer.
6. **Modes.** A working key is the operator's approval to gate: `mode: enforce` is the default once `key check` passes. `mode: shadow` is an opt-in that scores and logs every candidate while the matcher still decides. `mode: off` disables the judge. With no working key the gate is off.
7. **Failure.** The judge call has a bounded deadline, a profile field defaulting to 2 s; in the probe 2% of single calls ran longer. Any failure fails open to the matcher's decision, as if no key were configured: no key, invalid key, provider error, deadline exceeded, daemon absent. Every judged candidate and every fallback is logged to `events.jsonl` with the engine, P(yes), threshold, verdict, latency and reason, so latency and the fail-open rate can be read from the log.

## Consequences

### Positive

- The noise the matcher lets through drops sharply where a key is configured (ADR-195 operating points), without changing the matcher or any way.
- The judge's interface is independent of the agent tool and of the model, so other tools and later engines reuse it.
- The log gives a live measure of the gate's latency and fail-open rate on real sessions, and shadow mode gives a calibration source for tuning.

### Negative

- Prompts with candidates wait on a network call, about 0.8 s and at most the deadline, well above the 100 ms north star.
- Conversation turns and way descriptions are sent to the provider. Users who cannot send session text off the machine have no gate.
- A paid key, with its management (ADR-502), is required.

### Neutral

- ADR-193 and ADR-194, proposed before this probe, designed a local cross-encoder gate with a training loop. This decision replaces them; both are abandoned with a pointer here.
- The `.description` proposal (#664) did not improve the judge in the probe; its authoring and typing case stands on its own.

## Alternatives Considered

- **A local reranker as the engine (Qwen3-Reranker-0.6B).** Rejected for now: it did not beat a word-overlap baseline and was at chance on random live fires (ADR-195). The operator chose a remote engine; #666 keeps a local model as later exploration.
- **A better embedder in the matcher.** Not rejected; complementary. It reduces noise before the gate, and the probe's labelled sets can rank embedders. It does not answer the doing-versus-mentioning question that separates the remaining candidates.
- **A Claude Code prompt hook as the judge.** Rejected: prompt hooks return only allow or block for the whole event, see only the event JSON, and run in parallel with the hook that injects ways, so they cannot select among candidates.
- **One call per candidate.** Rejected as the default: it repeats the turns in every call and multiplies latency and cost; one call with all candidates sends them once.
- **A cosine-keyed cache of verdicts.** Rejected: near-identical embeddings are exactly the cases a judge exists to separate. An exact-key cache is allowed.
