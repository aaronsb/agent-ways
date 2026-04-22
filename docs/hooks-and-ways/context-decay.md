# The Context Decay Model: Why Timed Injection Beats Front-Loading

Ways are a progressive disclosure system for LLM context. This document explains why timed injection outperforms monolithic system prompts by modeling the *presentation economics* of long-context inference: how guidance retains or loses its effective influence on generation as context accumulates. A companion document, [Formal Foundations](context-decay-formal-foundations.md), grounds each claim here in published transformer research, control theory, and human operator modeling. The architectural decisions that operationalize this model for firing dynamics live in [ADR-123: Firing dynamics — progression-axis unification for attend and ways](../architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md).

## What this model captures — and what it doesn't

Before the formulas: a note on scope. The decay model below is a pedagogical approximation of *presentation economics*, not a literal description of what happens inside a trained transformer's attention mechanism. It answers the question **"on aggregate, how much effective influence does a piece of guidance retain as context accumulates past it?"** It does not answer "what does attention actually do with this specific token at this specific layer?"

Two separate things can be true:

1. **The baseline positional prior** for attention decays with distance. RoPE (rotary positional embedding) imposes a long-term decay in the inner product of query/key vectors as their positional distance grows. This is a structural property of the encoding, and it's what the model below is approximating.
2. **Trained attention heads override the baseline** for content they've learned is salient. Modern LLMs — Claude included — are explicitly trained for needle-in-a-haystack retrieval. An attention head that has learned a pattern like "retrieve specific rule tokens when generating commit messages" will reach back 100k+ tokens with near-full salience, ignoring the baseline decay prior. The empirical retention curves in [model-context-decay/README.md](../reference/model-context-decay/README.md) show exactly this: Opus 4.6 retains ~78% retrieval accuracy at 1M tokens, which is much better than any smooth exponential would predict.

Both facts matter, but for *firing decisions* — deciding when to re-inject a way so its guidance is freshly available — what we care about is the **aggregate effective retention** across the mix of attention heads that will actually run during generation. That aggregate is close enough to the baseline decay prior that the model below is a useful decision rule. Firing on token-distance is *presentation economics*, not *attention internals*. We inject when the aggregate has faded below useful, regardless of whether some specific head could still retrieve it.

The rest of this document describes the presentation-economics model. Read the formulas as useful approximations, not as direct claims about transformer mechanics.

## The Decay Problem

During inference, a model's adherence to system prompt instructions drops as context accumulates past it. The drop can be decomposed into two granularities of the same underlying thing — accumulated token distance since the guidance was injected:

1. **Turn decay** ($n^{-\alpha}$): Each additional conversation turn pushes the system prompt further from the attention cursor in token distance. Turn count is a coarse proxy for token accumulation — a convenient one because it's easy to count and humans think in turns, but the underlying measure is tokens.

2. **Within-generation decay** ($t_\mathrm{local}^{\ -\beta}$): Within a single generation, each token the model produces advances the distance from the system prompt. Same mechanism as turn decay, just observed at finer granularity.

**Turn count and local token count are not independent axes — they are the same axis at different zoom levels.** A very long turn, by itself, can close the salience window for guidance injected before it, because the token advance that happens *during* that turn is indistinguishable from the token advance that would happen *across several turns* of equivalent total length. One deep-reasoning turn that generates 8k tokens displaces earlier context the same way eight shorter turns of 1k each would. The $n^{-\alpha}$ and $t_\mathrm{local}^{\ -\beta}$ terms in the formula are not physically separate; they are a two-scale approximation of a single process — **monotonic token advance since injection**.

The combined effect:

$$A(t) \approx A_0 \cdot n^{-\alpha} \cdot t_\mathrm{local}^{\ -\beta}$$

Reading this formula:

- $A(t)$ — **effective adherence** at time $t$. How strongly the model's output actually follows the system prompt right now. Not a binary "remembers or doesn't" — it's a continuous weight that determines how much influence those instructions have on the next generated token.
- $A_0$ — **initial adherence strength**. The model's attention to the system prompt at the very start of the conversation, before any decay has occurred. This is the ceiling — adherence only goes down from here.
- $n^{-\alpha}$ — **turn decay**. The conversation turn count $n$ raised to a negative power $\alpha$. This is an inverse power law: doubling the number of turns doesn't halve adherence, it reduces it by a factor of $2^\alpha$. The exponent $\alpha$ controls how aggressively turns erode attention — higher $\alpha$ means faster fade. This term only resets when the conversation itself resets.
- $t_\mathrm{local}^{\ -\beta}$ — **within-generation decay**. The token count $t_\mathrm{local}$ since the last user message, raised to negative $\beta$. Each token the model generates pushes the system prompt further away in positional distance. The exponent $\beta$ controls how fast attention fades within a single response — higher $\beta$ means the model "forgets" faster during long outputs.

The two decay factors are independent and multiplicative. Even if within-generation decay is mild ($\beta$ is small), turn decay still erodes the envelope over time. And even in a short response ($t_\mathrm{local}$ is small), many turns still reduce the peak.

Each user message partially resets $t_\mathrm{local}$ (the local factor), creating a brief spike in attention to earlier context. But the peak of each spike is lower than the last, because $n$ has incremented. The result is a **damped sawtooth**: attention spikes at each turn boundary, but the envelope of those spikes always drops.

<img src="../images/context-decay-sawtooth.png" alt="Damped sawtooth: system prompt adherence decays over conversation turns, with each peak lower than the last" width="100%" />

By mid-conversation, the system prompt's effective influence has decayed below the noise floor. The model hasn't "forgotten" the instructions — they're still in the context window — but positional distance has reduced their weight in the attention computation to the point where recent context dominates.

This is why long conversations feel like the model "lost its instructions." It did, in the attention-weighted sense that matters during generation.

## The Implicit Scope Problem

There's a second failure mode that compounds decay. When a user says "commit this code," the intent is typically not just "run git commit." It's "run git commit *following the project's commit conventions, with a well-structured message, avoiding secrets, using conventional commit format*" — an implicitly unbounded scope of substantiating details.

The model follows the locally most probable path from the prompt. Like gradient descent, it finds the nearest minimum: the literal instruction. The substantiating details were supposed to come from the system prompt, but by the time the model is generating a commit command at turn 15, those instructions have decayed below the threshold where they'd influence the output.

The system prompt was the right place for this information. The problem isn't content — it's position.

## How Injection Changes the Topology

Ways change where guidance enters the context. Instead of one large block at position zero, ways deliver small, relevant fragments near the attention cursor at the moment they matter:

$$A(t) \approx \overbrace{A_0 \cdot n^{-\alpha} \cdot t_\mathrm{local}^{\ -\beta}}^{\mathrm{system\ prompt\ (decaying)}} + \overbrace{A_\mathrm{inject} \cdot t_\mathrm{since}^{\ -\beta}}^{\mathrm{injected\ way\ (fresh)}}$$

Reading this formula — there are now two additive terms:

- **Left term** (system prompt): Identical to the original decay formula. The system prompt is still there, still decaying with both turn count and generation distance. By mid-conversation this term approaches zero.
- **Right term** (injected way): The new contribution from a way that was just injected.
  - $A_\mathrm{inject}$ — **injection strength**. The initial salience of the injected guidance. Analogous to $A_0$ but for the way, not the system prompt.
  - $t_\mathrm{since}^{\ -\beta}$ — **time since injection**. The token count since this way was injected, subject to the same within-generation decay exponent $\beta$. Crucially, this is the *only* decay factor. There is no $n^{-\alpha}$ term — the injection doesn't carry the accumulated weight of all prior turns.

The critical difference: the injection term carries only the local decay factor ($t_\mathrm{since}^{\ -\beta}$), not the turn-count envelope ($n^{-\alpha}$). The injection isn't pinned at position zero — it's near the cursor, regardless of how many turns have elapsed. Its positional distance is always small.

Why the addition works: even when the system prompt term has decayed to near-zero, the injection term provides a fresh $A_\mathrm{inject}$ that only needs to survive local decay. The total adherence $A(t)$ gets a floor set by the most recent injection, not by the original system prompt's fading signal.

<img src="../images/context-decay-comparison.png" alt="Side-by-side comparison: without ways (damped to noise floor) vs. with ways (steady-state adherence)" width="100%" />

The original system prompt still fades. But the injected guidance maintains its peak. The sawtooth stops damping and reaches a **steady state** — adherence oscillates around a constant level instead of decaying to zero.

## Progressive Disclosure

This is progressive disclosure applied to the model itself.

In UX design, progressive disclosure means: don't show the user everything at once. Reveal controls and information as they become relevant to the current task. The principle works because humans have limited working memory — front-loading information wastes the budget on things that aren't yet actionable.

Ways apply the same principle to transformer attention. The context window is the working memory. The attention budget is finite. Front-loading a 500-line system prompt wastes most of that budget on guidance that isn't relevant to the current action.

The system prompt is the settings page nobody reads. Ways are the tooltip that appears when you hover over the button.

| UX Progressive Disclosure | Ways Progressive Disclosure |
|---|---|
| Reveal controls when relevant | Inject guidance at state transitions |
| Don't overwhelm with options | Don't fill context with irrelevant rules |
| Match information to task phase | Match guidance to tool being invoked |
| Settings page vs. contextual tooltip | Monolithic prompt vs. timed injection |

The design heuristic follows directly: when authoring a way, the question isn't "what does Claude need to know?" but "what does Claude need to know *right now*?" The first question produces system prompts. The second produces ways.

## The Saturation Constraint

The model implies a practical limit. If too many ways fire simultaneously, they compete for the same local attention budget. Each additional injection dilutes the effective $A_\mathrm{inject}$ of every other active injection:

$$A_\mathrm{eff} \approx \frac{A_\mathrm{inject}}{1 + k \cdot N_\mathrm{concurrent}}$$

Reading this formula:

- $A_\mathrm{eff}$ — **effective adherence per injection**. How much influence each individual way actually exerts when multiple ways fire at the same time. This is less than $A_\mathrm{inject}$ whenever there's competition.
- $A_\mathrm{inject}$ — **single-injection strength**. The adherence a way would achieve if it were the only one firing. This is the numerator — the ideal case.
- $N_\mathrm{concurrent}$ — **number of simultaneously active injections**. How many ways fired in the same hook response.
- $k$ — **competition coefficient**. How aggressively injections dilute each other. A higher $k$ means each additional way costs more attention from all the others. This isn't a fixed constant — it depends on how semantically similar the injections are. Two ways about the same topic compete more than two about unrelated concerns.

The denominator $1 + k \cdot N_\mathrm{concurrent}$ grows linearly with the number of injections, so effective adherence drops hyperbolically. One injection gets nearly full strength. Two split the budget. Five are each fighting for scraps. The "+1" in the denominator ensures that a single injection ($N_\mathrm{concurrent} = 0$ additional competitors) gets the full $A_\mathrm{inject}$.

This is why ways are designed to be small (20-60 lines each), fire once per session (marker-gated deduplication), and trigger selectively (embedding-based semantic scoring, not blanket activation). The goal is high signal-to-noise at the cursor position, not maximum information delivered. Three precisely timed injections outperform twenty simultaneous ones.

<img src="../images/context-decay-saturation.png" alt="Saturation curve: effective adherence per injection drops sharply as concurrent injections increase" width="100%" />

## Steady-State Adherence

The combination of these mechanisms — timed injection, curve-driven re-fire, selective triggering — produces a system that maintains consistent instruction adherence regardless of conversation length:

| Mechanism | What It Controls |
|---|---|
| Timed injection | Resets positional decay ($t_\mathrm{since}$) |
| Selective triggering | Maintains signal-to-noise ratio |
| Per-way refire | Each way declares its own re-fire cadence as a fraction of the session's context window; the engine suppresses re-injection until salience drops below the floor |
| Small injection size | Maximizes per-injection salience |

The "per-way refire" row is how ADR-123's firing engine, refined by ADR-126, replaces the pre-unification "once-per-session gating" heuristic. Each way declares a `refire:` field in its frontmatter — either a numeric fraction (e.g. `refire: 0.15` for ~15% of session window) or a preset name (`rare`, `normal`, `frequent`) that resolves to a fraction via config. At fire time the engine multiplies by the model's actual context window to produce a concrete half-life, consults `current_salience(current_tick) < REFIRE_FLOOR`, and decides whether to re-inject. Way files stop encoding host-specific token counts — the same `refire: 0.15` fires ~3 times per session on a 200k Sonnet window *and* on a 1M Opus window, because the fraction scales. The old "fire once per session then stay silent" behavior is `refire: once` (sigma 1.0, half-life = window, refire delta exceeds session). The legacy `curve:` block for non-Exponential shapes (Flat step, ActionPotential, ProgressiveStaircase) is still parsed but no current way uses it — smooth fade is the usual story.

The system prompt provides the baseline. Ways provide the reinforcement signal that prevents that baseline from decaying. Together, they produce what a monolithic prompt cannot: stable adherence across arbitrarily long conversations.

## Relationship to Other Documentation

This document describes the presentation-economics model — the *how* of context decay and injection topology, framed as useful approximation rather than claims about attention internals.

For the architecture that operationalizes this model for firing dynamics across ways and attend — including the progression-axis framing, curve-as-first-class-parameter, and why ways' tick is token position while attend's is wall-clock — see [ADR-123: Firing dynamics — progression-axis unification](../architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md). That ADR is the canonical source for how this model gets converted into actual firing decisions.

For the formal mathematical grounding — RoPE decay derivations, multi-layer amplification, cascade control theory, McRuer's crossover model applied to human-LLM steering, and steady-state adherence conditions — see [context-decay-formal-foundations.md](context-decay-formal-foundations.md). Note that the RoPE section there should be read alongside the "What this model captures" section above: RoPE gives the baseline positional prior, but trained attention heads can override it for salient retrieval.

For empirical retention benchmarks across models (Opus/Sonnet MRCR v2 and GraphWalks BFS at 128K–1M context) — see [model-context-decay/README.md](../reference/model-context-decay/README.md). These numbers quantify the aggregate effective retention that the formulas above approximate.

For the cognitive science foundations — active inference, predictive processing, situated cognition — see [rationale.md](rationale.md).

For the empirical landscape of steering systems across tools — see [dev-agent-steering-report.md](../research/dev-agent-steering-report.md).
