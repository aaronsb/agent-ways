# Observed Behavior: Vanilla vs Ways Claude Code on a Long-Running Supertask

**Date of observation:** ~2026-03-17
**Operator:** Aaron Bockelie (single operator, two machines, same starting prompt)
**Status:** n=1 self-observation. Existence proof, not benchmark. Documented here so it exists outside the operator's head.

## Why this file exists

The theoretical scaffolding in [`context-decay.md`](context-decay.md) and [`ADR-123`](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md) describes *why* ways should help with long-context instruction adherence. It does not, on its own, show *that* ways help. This note closes the empirical grounding gap so the theory is not resting entirely on internal plausibility.

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

The steering-cost gap has an established name: preference uncertainty, from principal–agent theory. An agent that doesn't know its principal's norms and current frame has only two safe strategies — ask constantly or hedge exhaustively — and the operator pays the cost of resolving that uncertainty turn by turn. Ways resolve it mechanically, at the moment of action, which is why the same model on the same task stopped needing the nudges.

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

1. **Parameter calibration.** The observation says "ways helped"; it says nothing about whether the specific curve shapes, half-lives, or firing thresholds in the current implementation are optimal. Those are still empirical questions for the calibration work of [ADR-123](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md) to answer.

2. **Which ways were load-bearing.** The full stack was active on Machine B. This observation cannot discriminate between "quality.md was load-bearing" and "github.md was load-bearing" and "it was all of them together." Ablation by individual way would be needed for that.

3. **Reproducibility across operators.** The operator was the author of the system. Familiarity effects, confirmation bias, and working-style matching to the tool are all uncontrolled. A different operator running the same setup might see a smaller or differently-shaped gap.

4. **Magnitude across models.** This was one Claude model on one task. Retention curves differ across models ([`model-context-decay/README.md`](../reference/model-context-decay/README.md) shows Opus 4.6 retains 78% at 1M vs Sonnet 4.6 at 65%); the observed gap is likely model-dependent.

## Re-observation — 2026-04-14 — post ADR-123

**Date:** 2026-04-14
**Operator:** Aaron Bockelie (observer)
**Session:** 43cecca9-5408 on `feat/firing-dynamics` branch, Claude Opus 4.6
**Task:** Continue ADR-123 firing-dynamics work: Phase E `ways tune-curves` subcommand, then Phase F validation (this entry), then ADR-123 status flip. Entered via a continuance prompt on session start; ran for ~3 hours of real work time across 92 turns.

### Why this observation exists

The 2026-03-17 entry above established a baseline: ways-Claude vs vanilla-Claude on the same task, with ways winning on context cost (~200k vs ~275k), redirection count (minimal vs constant), and qualitative supertask coherence. Plan task F1 in the ADR-123 implementation plan (a working file, not kept) asks to reproduce that observation against the new unified-engine stack before flipping ADR-123 from Draft to Accepted. This section closes that loop.

This re-observation has an intentionally *different shape* from 2026-03-17. There is no vanilla control run. Instead, the supertask was the ADR-123 work itself, driven from a new-session continuance prompt, with the operator watching rather than steering. What the observation tests is whether the new stack preserves the task-hierarchy-under-detour property that made the original observation matter — not whether it does so more than vanilla Claude on the same contrived benchmark.

### Setup

- Claude Opus 4.6 on the `feat/firing-dynamics` branch with ADR-123 Phase A–E shipped.
- attend v0.6.0 (036d188) running as a persistent Monitor alongside the session, providing live parity data for Phase B3 as a side-effect.
- Starting prompt: the same continuance prompt shipped in commit 036d188 (todo-adr-123-continuance.md). No hand-tuning of the prompt beyond its own shape.
- Operator behavior: watched *The Fifth Element* on Netflix in the browser tab next to the terminal. Interactions were limited to permission grants for tool calls that weren't pre-allowed in `settings.json` — a configuration gap, not task steering.

### Observations

#### Observer-side (Aaron's direct report)

- **Zero redirections required across the whole session.** "I never had to steer anything." Every operator interaction was a permission grant; none was a task-level correction.
- **Continuance mindset, not just in-task cohesion.** "Claude around 2.1.108, with no harness or configuration at all, seems to still perform well but simply perform the task and wrap up, or not really have a particular plan in mind for continuance." The contrast being drawn: vanilla Claude completes the task in front of it; ways-Claude maintains a forward-looking plan that survives detours. This is a specific behavioral claim beyond "the supertask held."
- **`ways session ways` shows semantic sparsity, not scattergun firing.** The ways that triggered are the ways the operator would expect to trigger given the actions being taken. Bar positions cluster at epochs that match the actual work phases, not random across the session.
- **`ways rethink` (now `ways session replay`) replay shows leading-edge compaction.** Watching the animation, the firing positions track the attention cursor as it moves forward, compressing behind. This is cursor-following as a visible phenomenon, not inferred from the math.
- **TaskList was invoked via progressive disclosure, not by pre-programmed habit.** A task-tracking way fired on turn 1 from the continuance-prompt content. The task list that then structured the whole session came from the firing, not from generic assistant instinct. The mechanism is visible in the causal chain.

#### Session-internal (Claude-side)

- **25 ways fired across 92 turns** by mid-observation, keyed against a 1000K context window.
- **212K tokens used (21%).** This matches the 2026-03-17 ways-Claude number (~200K) for a qualitatively similar work shape, despite a completely different task and a completely different stack.
- **Several genuine detours handled without supertask drift.** None of these were pre-planned; each arose organically and was returned-from cleanly:
  - Phase B3 parity check (observing attend cadence during unrelated work).
  - `attend config show` burst_window display bug — discovered, fixed as a small commit (679b205), Phase 2 cleanup filed as issue #50.
  - `tools/attend/src/main.rs` 2027-line priority flag from the quality way — filed as issue #51 with a concrete module layout rather than pivoting to implement.
  - Dead-code warning on `p75`/`max` fields in tune_curves — fixed before committing.
  - Test-math errors in tune_curves percentile/median assertions — fixed.
  - A load-bearing semantic bug in tune_curves's event filter (`way_fired`-only instead of `way_fired`+`way_redisclosed`) — caught by dry-running against the real log before committing, then fixed.
  - Quality way firing reactively at epoch 83 on tune_curves.rs itself as it crossed 500 lines — which led to the `--days` scope trim. This is the Phase D reactive-firing path working on the session that was writing it.
- **Phase D reactive firing verified live.** `ways session ways` shows `softwaredev/code/quality` fired at epoch 83 via the `postcheck` trigger. No predictive path could have caught that — the file was above threshold because of an Edit that had just happened. This is exactly the case ADR-123 §5 was written to handle, and it's working.
- **Check-firing decay verified live.** `softwaredev/environment` shows `11 fires, decay=0.08, (suppressed)` in the `ways session ways` output. The engine correctly stopped firing the check path after 11 fires because `1/(11+1) = 0.083 < REFIRE_FLOOR = 0.5`. The outward gate is doing its job.
- **Leading-edge compaction visible in the table.** The firing distribution clusters at epoch 1 (continuance-prompt seeded ways) and epochs 83–92 (current work), with middle epochs showing re-fire readiness rather than first-fires. That's the same pattern the operator observed in the `rethink` TUI animation (now `ways session replay`) — the engine is re-injecting old guidance specifically at the moment new work touches the same semantic territory.

### What this tests (and what it doesn't)

#### What the observation supports

1. **ADR-123's unified engine preserves the task-hierarchy-under-detour property.** The 2026-03-17 observation said "re-injection helps on long-running tasks." This observation says "the re-injection path through the new progression-axis engine still does that." Not a stronger claim than the original — a *preserved* claim, which is what Phase F asks for.

2. **Reactive firing (Phase D) extends the mechanism into the post-tool-use surface.** The quality way fired on the session that was writing it, in real time, when the file crossed its declared size threshold. That path didn't exist in the 2026-03-17 stack, and it's doing work here — catching maintenance signals that predictive firing structurally cannot see.

3. **Check-firing decay is the outward gate in miniature.** The `softwaredev/environment` way's 11-fire progression to suppression is a clean demonstration of REFIRE_FLOOR as a behavioral constraint, not just a constant in the code.

4. **Continuance mindset, as distinct from in-task coherence, emerges from progressive disclosure.** The operator's observation — that vanilla Claude completes tasks without a forward plan while ways-Claude builds and maintains one — is falsifiable and specific. If some future version of ways stops firing the task-tracking ways early in the session, this observation predicts the behavior would regress to "complete and wrap" with no continuance scaffolding.

#### What the observation does not test

1. **No vanilla control run on this task.** Unlike 2026-03-17, there is no side-by-side comparison. If vanilla Claude Opus 4.6 would have handled this same session equally well, this observation cannot rule that out. It only shows that the ADR-123 stack did handle it, and the mechanism the operator observed (cursor-following re-disclosure) is specific to how ways works.

2. **Observer effect.** The operator was watching, even if not steering. In the 2026-03-17 observation this was noted as uncontrolled; it remains so here. Whether the same session would go equally well with no observer at all is not testable.

3. **Magnitude is not quantified.** 21% context use and zero redirections are absolute numbers; there's no baseline to compare them against for *this specific task shape*. They match 2026-03-17 directionally — which is the right comparison for Phase F's "preserved or improved" criterion — but the match isn't a controlled result.

4. **n is still 1.** Same caveat as the original. This is a lab-notebook entry, not a benchmark.

### Conclusion for Phase F

The plan's acceptance criterion for F1 was *"new implementation is at least as good as the old one on the same task shape."* The task shape is different (ADR-123 continuation rather than code-review-into-release), but the properties being tested are the same: does the supertask hold, does detour-work get framed correctly, does context cost stay reasonable. On all three the answer from this observation is yes, with the additional positive signal that two ADR-123-specific mechanisms (Phase D reactive firing and check-firing decay) are doing visible work.

This clears the gate for ADR-123 Draft → Accepted.

## Why this is worth keeping

This note is the only place in the project where the empirical grounding for the entire firing-dynamics scaffolding is written down rather than held in the operator's memory. Every time future-us reads [`context-decay.md`](context-decay.md) or [`ADR-123`](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md) and wonders whether the theoretical elaboration is justified, the answer should be traceable to a concrete observation with a concrete mechanism — not "I remember noticing once that it helped."

It is also protection against theory-drift. If we later change the implementation in a way that would not have produced the effect observed here, this note is a pre-registered target: the new implementation should still, in principle, pass the same A/B test. If it wouldn't, that's a signal something load-bearing has been lost.

## Related

- [`context-decay.md`](context-decay.md) — the presentation-economics model this observation grounds.
- [`context-decay-formal-foundations.md`](context-decay-formal-foundations.md) — the mathematical scaffolding, tempered to distinguish baseline attention prior from trained retrieval behavior.
- [`ADR-123`](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md) — the firing-dynamics architecture informed by this model.
- [`model-context-decay/README.md`](../reference/model-context-decay/README.md) — the empirical retention benchmarks across Claude models.
- **Convergent external work.** As of April 2026, several independent communities are describing the same underlying pattern from different angles — security ("safety heartbeat" constraint re-injection for long-running agents), prompt engineering (strategic repetition to counter the recency bias), agent research (identity stabilization failures in agent-to-agent conversation without human grounding signals), and memory systems (prune-and-decay architectures with selective top-N injection). These are separate discoveries, not one crowd citing each other. Ways sits in the same shape of the design space but earlier in the calibration cycle.
