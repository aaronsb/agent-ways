# Observed Behavior: Vanilla vs Ways Claude Code on a Long-Running Supertask

**Date of observation:** ~2026-03-17
**Operator:** Aaron Bockelie (single operator, two machines, same starting prompt)
**Status:** n=1 self-observation. Existence proof, not benchmark. Documented here so it exists outside the operator's head.

## Why this file exists

The theoretical scaffolding in [`context-decay.md`](context-decay.md) and [`ADR-123`](../architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md) describes *why* ways should help with long-context instruction adherence. It does not, on its own, show *that* ways help. This note closes the empirical grounding gap so the theory is not resting entirely on internal plausibility.

This is a single observation, not a controlled benchmark. It's n=1, the operator is the author of the system, and there is no blind condition. Treat it as a lab notebook entry, not a study. The value is that the observation is specific enough to be testable by anyone else running the same comparison, and specific enough to discriminate between mechanistic hypotheses.

## Setup

The operator had just started a new job and installed a fresh Claude Code instance on the new-job machine. On the previous machine, the full ways/attend stack had been running for months. Both instances were given the same starting prompt for the same task — a code-review-into-release workflow that involved maintaining a supertask ("publish the new version") while handling a stream of detour work ("fix issues surfaced during review").

- **Machine A — vanilla Claude Code.** Fresh install, no ways, no attend, no custom hooks, no subagent prompts.
- **Machine B — ways Claude Code.** The full stack as of early March 2026 — ways firing on predictive matches, core guidance via SessionStart hooks, the usual subagent set.

Same starting prompt, same task, same Claude model, same operator.

## Observations

The task completed on both machines. The differences were large enough to be unambiguous on a single trial.

| | Vanilla Claude | Ways Claude |
|---|---|---|
| **Context used at task end** | ~275k tokens | ~200k tokens |
| **Redirections required from operator** | Constant — almost every response needed a nudge to stay on the supertask | Minimal — the supertask held |
| **Final output quality** | Many scattered inconsistencies across the work | Coherent and consistent |
| **Operator experience** | Steering cost high; felt like wrestling | Steering cost low; "sailed through" |

The ~27% context-cost reduction is meaningful on its own, but the more telling signal was the qualitative gap in steering cost. Vanilla Claude was burning context on cycles of drift-and-correct. Ways Claude was not.

## The mechanism observation

The operator's direct observation of *what* was going wrong with vanilla Claude, lightly cleaned up from his original description:

> Without steering, Claude kept treating my redirections as the direction to follow — not as detours to resolve inconsistencies I'd surfaced. I think that's because a hint of whatever it was working on — say, resolving issues found during a code review while needing to stay on the supertask of publishing the new version — had drifted far enough from the attention cursor that Claude treated each new instruction as top-of-stack. With ways, the context injection kept the supertask's semantic footprint fresh near the cursor. So when I said something like "fix this inconsistency," Claude could correctly classify it as a detour within the active task rather than a new task to pivot onto. The ways disclosure peaks attention on whatever's semantically related to the current action, which pulls the supertask frame back into effective attention at exactly the moment a detour arrives.

The mechanism in short form: **task hierarchy preservation under redirection.** Vanilla Claude flattens the stack — every new user instruction becomes the new top, because older task context has decayed out of effective attention. Ways Claude maintains the stack — the supertask stays semantically live via re-injection, so new instructions can be correctly framed as detours within a preserved frame rather than replacements for it.

## What this tests (and what it doesn't)

### What the observation supports

1. **Re-injection helps on long-running tasks.** The ~27% context savings and the quality gap are both in the predicted direction. Not proof — compatible with simpler explanations like "more context pressure on vanilla" — but the direction matches theory.

2. **Task-hierarchy preservation is a concrete mechanism, not just a vibe.** The operator could point at *specific* failure modes in vanilla Claude (redirection-as-new-top-of-stack) and *specific* success modes in ways Claude (redirection-as-detour-within-active-task). That specificity is what makes the mechanism falsifiable — anyone running the same comparison can check whether they see the same pattern, not just whether "ways felt better."

3. **Semantically-matched re-injection is doing more than generic reminder spam.** A simpler theory would predict that *any* periodic re-injection — random, rotating, irrelevant — would help roughly equally. This observation is compatible with the stronger claim that ways-style *semantic targeting* matters, because the effect shows up specifically at moments of topic-continuity-vs-topic-change ambiguity. Random re-injection would not be expected to help task classification at those moments.

### What the observation does not test

1. **Parameter calibration.** The observation says "ways helped"; it says nothing about whether the specific curve shapes, half-lives, or firing thresholds in the current implementation are optimal. Those are still empirical questions for [`ways tune`](../architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md) to answer.

2. **Which ways were load-bearing.** The full stack was active on Machine B. This observation cannot discriminate between "quality.md was load-bearing" and "github.md was load-bearing" and "it was all of them together." Ablation by individual way would be needed for that.

3. **Reproducibility across operators.** The operator was the author of the system. Familiarity effects, confirmation bias, and working-style matching to the tool are all uncontrolled. A different operator running the same setup might see a smaller or differently-shaped gap.

4. **Magnitude across models.** This was one Claude model on one task. Retention curves differ across models ([`model-context-decay/README.md`](../reference/model-context-decay/README.md) shows Opus 4.6 retains 78% at 1M vs Sonnet 4.6 at 65%); the observed gap is likely model-dependent.

## Why this is worth keeping

This note is the only place in the project where the empirical grounding for the entire firing-dynamics scaffolding is written down rather than held in the operator's memory. Every time future-us reads [`context-decay.md`](context-decay.md) or [`ADR-123`](../architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md) and wonders whether the theoretical elaboration is justified, the answer should be traceable to a concrete observation with a concrete mechanism — not "I remember noticing once that it helped."

It is also protection against theory-drift. If we later change the implementation in a way that would not have produced the effect observed here, this note is a pre-registered target: the new implementation should still, in principle, pass the same A/B test. If it wouldn't, that's a signal something load-bearing has been lost.

## Related

- [`context-decay.md`](context-decay.md) — the presentation-economics model this observation grounds.
- [`context-decay-formal-foundations.md`](context-decay-formal-foundations.md) — the mathematical scaffolding, tempered to distinguish baseline attention prior from trained retrieval behavior.
- [`ADR-123`](../architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md) — the firing-dynamics architecture informed by this model.
- [`model-context-decay/README.md`](../reference/model-context-decay/README.md) — the empirical retention benchmarks across Claude models.
- **Convergent external work.** As of April 2026, several independent communities are describing the same underlying pattern from different angles — security ("safety heartbeat" constraint re-injection for long-running agents), prompt engineering (strategic repetition to counter the recency bias), agent research (identity stabilization failures in agent-to-agent conversation without human grounding signals), and memory systems (prune-and-decay architectures with selective top-N injection). These are separate discoveries, not one crowd citing each other. Ways sits in the same shape of the design space but earlier in the calibration cycle.
