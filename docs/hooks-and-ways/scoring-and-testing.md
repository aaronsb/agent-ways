# Way Scoring and Testing

How to verify that ways trigger correctly — and only when they should.

For the exact engine semantics this page relies on (the fire rule, calibration,
parent-boost, config defaults with source citations), see
[engine-reference.md](engine-reference.md) — the single source of truth. This
how-to assumes that model and shows you how to work it.

## The Self-Validating Loop

The ways system doesn't just deliver guidance — it instructs its own quality assurance.

When Claude creates or modifies a way, the `meta/knowledge` way has already fired for that session, telling Claude how ways work — including that they use embedding-based semantic scoring against a vocabulary, mapped through a calibrated relevance model. The `/ways-tests` skill is listed in Claude's available tools. The memory system records "always verify new ways against sample prompts before shipping." The way-testing skill's own documentation includes scoring methodology, cross-way isolation checks, and vocabulary gap analysis.

So when Claude finishes writing a way and moves to testing it, that behavior isn't a separate QA step bolted on after the fact. It's the system telling Claude to validate itself, using tools the system provides, against criteria the system defines. The loop looks like this:

```
ways tell Claude how ways work
  → Claude creates a new way
    → ways (+ skills + memory) tell Claude to score it
      → Claude runs the scoring tool the system provides
        → scores reveal vocabulary gaps
          → Claude fixes the vocabulary
            → the improved way is now part of the system
              → that system tells Claude to score the next one
```

This is what makes the testing process reliable without a human manually running a test suite. Claude is both the author and the reviewer, but the *review criteria* come from the system itself — not from Claude's general training. The ways encode what "good" looks like for this specific project, and Claude applies those standards because the ways told it to.

The worked example below shows this loop in action during an actual way creation session.

## The Problem

Ways use embedding-based semantic scoring to decide whether a user's prompt is relevant to a particular domain of guidance. Each way has a vocabulary (terms it cares about) and a description; the prompt is embedded and scored by cosine `s` against that alias, and `s` is mapped through a calibrated logistic `g(s) = σ(a·s + b)` into a relevance probability. A way fires when that probability clears the global semantic bar `τ_s` (0.5), or — on a `pattern:` keyword hit — the lower keyword floor `τ_k` (0.15). There is no per-way threshold to set; the cutoffs are global and live in probability space. See [engine-reference.md](engine-reference.md) for the full rule.

Getting the vocabulary right matters: a way that fires too eagerly drowns the user in irrelevant guidance; a way that never fires is dead weight.

With 50+ ways in the system, vocabulary space gets crowded. Adding terms to one way can accidentally create overlap with another. The only way to know is to test. What follows is information-retrieval evaluation in miniature — a test collection with relevance judgments, tuned precision-first; [matching.md](matching.md#what-this-actually-is) traces that lineage.

## How Test Prompts Get Written

The scoring process depends on realistic test prompts — but who writes them?

In this system, Claude generates test prompts by modeling how the human operator would naturally phrase their intent. This is the key mechanism: Claude knows what the way is *for* (from the description and the conversation that led to creating it), and translates that into the variety of ways a human might ask for that thing.

For example, the `meta/project-health` way exists so that when a user wonders about upstream Claude Code changes, the right guidance appears. Claude generates test prompts by thinking: "if I were a human who wanted to know what changed upstream, what would I actually type?"

That produces prompts like:
- "what's new in claude code recently" (casual, direct)
- "have we drifted from upstream claude code" (conceptual, uses domain language)
- "are our ADRs current with what we've shipped" (inward-facing, about self-assessment)
- "run project pulse" (direct tool invocation)

And negative prompts by thinking: "what would a human type that sounds vaguely related but should *not* trigger this way?"

- "how do I create a new way" (meta, but about authoring, not project health)
- "add error handling to the parser function" (code task, nothing to do with upstream)

This matters because **vocabulary gaps hide in the space between how the author thinks about the concept and how the user phrases their need**. The author writes `reconcile drift stale` thinking about ADR status. The user types "are our ADRs current with what we've shipped." Those are the same intent expressed in completely different words. Claude bridges this gap by generating prompts from the user's perspective, not the author's.

This is also why scoring is done iteratively during way creation rather than after the fact. The conversation that produces the way — where the human explains what they want and why — is exactly the context Claude needs to generate authentic test prompts. If scoring is deferred to a separate QA step, that conversational context is lost.

## The Tool

The `ways` binary includes embedding-based semantic scoring as a built-in subcommand (see [ADR-108](../architecture/system/ADR-108-embedding-based-way-matching-with-all-minilm-l6-v2.md) for the embedding engine, [ADR-111](../architecture/system/ADR-111-unified-ways-cli-single-binary-tool-consolidation.md) for the consolidation, and [ADR-125](../architecture/system/ADR-125-authored-disclosure-graph-and-removal-of-bm25.md) for the embedding-only decision). It scores a prompt against the entire way corpus using cosine similarity and ranks the results.

```bash
# Score a prompt against all ways (query is positional; there is no --threshold flag)
ways embed "what's new in claude code recently"

# Output: ranked list with per-model cosine similarity.
# The raw cosine is the INPUT to g(s), not a fire cutoff. A way fires when its
# calibrated probability g(s) clears the global bar τ_s (0.5) on the semantic
# lane, or τ_k (0.15) on a keyword-gated pattern hit. Read the decision off g(s),
# not the raw cosine — see engine-reference.md.
```

The `/ways-tests` skill wraps this with higher-level operations: scoring all ways against a prompt (and surfacing the calibrated `g(s)` alongside the cosine), analyzing vocabulary gaps, checking for cross-way overlap, and validating frontmatter.

## The Process: A Worked Example

This walkthrough shows the loop for the `meta/project-health` way, which provides guidance on managing the project's relationship to upstream Claude Code releases. The scores are illustrative of the calibrated presentation — a **Cosine** column (the embedding input), the **g(s)** probability it maps to under the EN model (`a ≈ 26.7`, `b ≈ −7.8`; see [engine-reference.md](engine-reference.md)), and the **Fire** decision against the global semantic bar `τ_s = 0.5`.

### Step 1: Write the way with initial vocabulary

The vocabulary was chosen by thinking about what a user would say when they want to check upstream changes or review project health. There is no threshold field — firing is decided by the global `τ_s` / `τ_k`, so only the vocabulary and description are tuned:

```yaml
vocabulary: >
  upstream changelog release version claude-code update
  adr status reconcile drift stale dormant
  project pulse health review audit
  what's new recently changed since last
  relevance feature gap opportunity
```

### Step 2: Score against target prompts

These are prompts that *should* trigger the way:

```
── Target Prompts (should match) ──────────────────────────────────

  Prompt                                            Cosine  g(s)   Fire
  "what's new in claude code recently"              0.462   0.988  YES
  "are our ADRs current with what we've shipped"    0.289   0.478  no   ← problem
  "check if upstream features matter for our config"0.371   0.891  YES
  "run project pulse"                               0.330   0.733  YES
```

The second prompt — "are our ADRs current with what we've shipped" — missed. Its cosine mapped to `g(s) = 0.478`, just under the semantic bar `τ_s = 0.5`.

### Step 3: Diagnose the miss

`g(s) = 0.478` sits inside the near-miss band (within `near_miss_margin`, 0.05, below `τ_s`) — a hair short, exactly the kind of false silence the empirical telemetry below is built to surface. The prompt's "current" and "shipped" didn't appear in the vocabulary, and the only overlapping term was "adr" — not enough cosine to push `g(s)` over the bar. The embedding engine (ADR-125) is more forgiving for paraphrase than term-overlap scoring would have been, but the underlying lesson stands: vocabulary tuned for what *users actually say* outperforms vocabulary tuned for the topic in the author's head.

This is the kind of gap that's invisible when you write the vocabulary by thinking about the *topic* — you think "ADR reconciliation" and write `reconcile drift stale`. But a user says "are our ADRs current with what we've shipped" using completely different words for the same concept.

### Step 4: Fix the vocabulary

Added four terms: `shipped`, `implemented`, `current`, `behind`.

### Step 5: Re-score and verify no regressions

```
── Target Prompts (should match) ──────────────────────────────────

  Prompt                                              Cosine  g(s)   Fire
  "what's new in claude code recently"                0.462   0.988  YES
  "are our ADRs current with what we've shipped"      0.351   0.826  YES  ← fixed
  "check if upstream features matter for our config"  0.368   0.885  YES
  "run project pulse"                                 0.326   0.719  YES
  "have we drifted from upstream claude code"         0.455   0.986  YES
  "what claude code releases since our last commit"   0.480   0.993  YES

── Negative Prompts (should NOT match) ─────────────────────────────

  "add error handling to the parser function"         0.071   0.003  no
  "write unit tests for the auth module"              0.089   0.005  no
  "refactor the database connection pool"             0.064   0.003  no
  "how do I create a new way"                         0.243   0.212  no
  "fix the CSS layout on mobile"                      0.038   0.001  no
```

The miss is fixed (`g(s)` 0.478 → 0.826). All other target prompts still fire. All negative prompts still correctly reject. The nearest false-positive candidate — "how do I create a new way" at `g(s) = 0.212` — sits below `τ_s` and would fire only if it also matched a `pattern:` keyword (it's above the keyword floor `τ_k` but there is no pattern here to trip it).

### Step 6: Check cross-way isolation

The final check: does this way compete with other ways for the same prompts? The cross-way ranking shows the calibrated `g(s)` for every way against one prompt, decided against the single global `τ_s = 0.5` — there is no per-way threshold column, because there is no per-way threshold:

```
=== Cross-Way Ranking: "what's new in claude code recently" ===

  Cosine  g(s)   Fire  Way
  ──────  ─────  ────  ───
  0.462   0.988  YES   meta/project-health          ← target (semantic lane)
  0.245   0.223  no    documentation/docstrings
  0.221   0.130  no    softwaredev/code/quality
  0.216   0.116  no    softwaredev/code/supplychain/sourceaudit
  0.198   0.075  no    softwaredev/code/security
  0.205   0.091  no    documentation/standards
  0.212   0.104  no    softwaredev/delivery/github
  ...
```

Clean win. The target way scores `g(s) = 0.988`; the next closest scores 0.223 — well below `τ_s`. No overlap, no competition.

## What to Look For

### Good signs

- **Clean win**: Target way is the clear top scorer with daylight to the next.
- **Correct rejects**: Unrelated prompts map to a low `g(s)`, well under `τ_s`.
- **Score headroom**: Target prompts clear `τ_s` with room to spare, not by a hair.

### Warning signs

- **Narrow miss**: A target prompt lands in the near-miss band — `g(s)` within `near_miss_margin` (0.05) below `τ_s`. It may fail on slightly different phrasing.
- **Overlap cluster**: Two ways both match the same prompt within ~0.05 cosine of each other. They're competing for the same semantic space.
- **False dominance**: Another way scores higher than the target for a prompt the target should own.
- **Vocabulary bleed**: Adding terms to fix one gap creates unexpected matches elsewhere.

### The vocabulary authoring trap

When writing vocabulary, it's natural to think in *your* terms — the terms that describe the concept from the inside. But users don't think about the concept from the inside. They think about their problem:

| You write | User says |
|-----------|-----------|
| `reconcile drift stale` | "are our ADRs current" |
| `epoch mapping feathered window` | "what changed since last time" |
| `upstream tracking` | "what's new in claude code" |

The fix is always the same: write target prompts *before* you write the vocabulary, then add the terms the prompts actually use.

## Sparsity as the Guard Against Overfitting

The natural instinct when a way misses a prompt is to add more vocabulary. When it misses another, add more. This works locally — each fix raises the score for the target prompt — but globally it's overfitting. Every term you add to a vocabulary is a term that could match prompts meant for a *different* way.

The system's defense against this is **sparsity**: each way should occupy a narrow, distinct region of the scoring space with minimal overlap against other ways. The goal isn't to maximize any single way's score. It's to maximize the *distance between ways* — so that for any given prompt, at most one or two ways fire, and it's obvious which one is the right one.

This is why the cross-way ranking check (Step 6 in the worked example) matters more than the individual scores. A way that clears `τ_s` on its target prompt and has clean separation from every other way is healthier than a way that scores high but overlaps with three neighbors.

Concretely:

- **Narrow vocabularies are better than broad ones.** 15 precise terms beat 40 general terms. "upstream", "changelog", "drift" are specific to project-health. "update", "check", "status" are shared by many domains.
- **Don't chase every synonym.** If "shipped" fixes a miss, add it. But don't then add "deployed", "released", "landed", "merged", "delivered" — each one increases the surface area for false matches against delivery/release or delivery/github.
- **The two levers are vocabulary and pattern — both measured through the calibration, neither a per-way threshold.** Sharpen or widen the `vocabulary` to move the semantic lane; add or tighten the `pattern:` regex for the keyword lane. When a way fires correctly but also fires weakly on unrelated prompts, the remedy is to narrow the vocabulary, not to reach for a knob that no longer exists. (A keyword that leaks *globally* is the `τ_k` floor's job, not the way's — see the remedy loop in [authoring-docs-style.md](authoring-docs-style.md).)
- **Accept some misses.** A way that fires for 90% of relevant prompts with zero false positives is better than one that fires for 100% but also fires for 5% of irrelevant prompts. The 0 FP constraint is hard; recall is soft.

The test harness enforces this: it tracks false positive rate as a hard constraint (must be 0) while accuracy can vary. Sparsity is how you maintain 0 FP as the vocabulary grows.

### Intentional co-fire: sparsity's inverse

Sparsity is the default — keep ways apart. But sometimes you *want* two ways to fire together. A project-scoped way and a user-scoped way might both be relevant when someone says "create a PR." A GitHub way and a custom Jira way might both need to fire when someone says "ship this ticket."

Rather than writing a third way that combines both concerns (more content to maintain, more context consumed), you can plant shared vocabulary terms in both ways so that the embedding scorer naturally co-fires them on the same prompt. Two small ways that each contribute their piece is lighter than one large way that tries to cover everything.

This is a deliberate vocabulary manipulation — the opposite of sharpening. You're *reducing* the distance between two ways for specific prompts where both are genuinely needed. The key discipline is that the shared terms should be narrow: "pull request", "ship", "PR" — not broad terms like "code" or "deploy" that would create accidental overlap on unrelated prompts.

The `/ways-tests crowding` command distinguishes these cases. When it reports two ways co-firing, it flags whether the overlap looks accidental (similar scores on a prompt neither should own) or intentional (both score well on a prompt both should serve). The worked example's cross-way ranking shows this: a "healthy co-fire" is when two ways both match but serve complementary purposes.

## Tools Reference

| Command | Purpose |
|---------|---------|
| `/ways-tests score <way> "prompt"` | Score one way, with automatic cross-way context |
| `/ways-tests score-all "prompt"` | Rank all ways against a prompt |
| `/ways-tests suggest <way>` | Analyze vocabulary gaps (body terms missing from vocabulary) |
| `/ways-tests suggest <way> --apply` | Auto-fix vocabulary gaps |
| `/ways-tests crowding "prompt"` | Detect vocabulary overlap across all ways |
| `/ways-tests lint --all` | Validate all way frontmatter |
| `ways tune` | Audit locale alias fidelity + discrimination (per-way, across all languages) |
| `ways tune --way <path>` | Filter the audit to a single way or subtree |
| `ways tune-curves` | Calibrate firing cadence: suggest `half_life` from observed fire deltas (`--apply` rewrites the `curve:` block) — ADR-123 Phase E |
| `ways tune-precision` | Heuristic relevance audit: flag ways firing into off-domain sessions (`--min-sessions`, `--flag-threshold`, `--project`, `--way`, `--json`) — ADR-134 Decision 3 |
| `ways siblings <path>` | Compute vocabulary overlap (Jaccard) between sibling ways |

See the [ways-tests skill](/skills/ways-tests/SKILL.md) for the testing skill and [Locale Alias Audit](../../hooks/ways/meta/knowledge/optimization/tuning/tuning.md) (the `knowledge/optimization/tuning` way) for the `ways tune` workflow in depth.

## Empirical Signals: Tuning From What Actually Fired

The worked example above tunes a way against prompts you write by hand. But once a way ships, the firing engine itself becomes the evidence. [ADR-134](../architecture/system/ADR-134-empirical-auto-tuning-from-fire-and-near-miss-telemetry.md) extends the telemetry in `~/.claude/stats/events.jsonl` so that hand-tuning gets a record to revise from — three report-first signals:

- **Near-misses.** When a way's calibrated probability lands in the band just below the semantic bar — `τ_s − near_miss_margin ≤ g(s) < τ_s`, `near_miss_margin` default 0.05 (a live config key, `config.rs`) — but nothing fires, the matcher logs a `way_nearmiss` event. It carries `prob_en`, `prob_multi`, `tau_s`, `margin`, `trigger`, `query_tokens` (plus `way`, `corpus_id`, `domain`, `scope`, `project`, `session`) — the already-computed probabilities against the *same* global `τ_s` the fire path uses, no per-lane thresholds and no new embedding work (`scan/mod.rs`). These are the false silences the precision-first discipline can't otherwise see — a way that consistently lands just under the bar on prompts whose sessions then do that way's kind of work is a candidate to widen, the recall counterpart to the 0-FP constraint.
- **Fire scores.** A `way_fired` event carries `fire_score`: the calibrated probability `g(s)` that cleared `τ_s`, recorded on first-fires only (not redisclosures, and `None`/absent for deterministic keyword fires) — `show/mod.rs`. This is the fire-score population that the ADR-156 calibration is fit from and that `tune-precision` reads.
- **Gated keywords.** When a `pattern:` hit is vetoed because `g(s)` sat below `τ_k` on every model lane (ADR-155), the matcher logs a `way_keyword_gated` event carrying the `matched_span` — the per-alternation evidence that calibrates `keyword_floor_probability` and drives the pattern-hygiene rework before any tightening.

`ways tune-precision` reads the fire stream and reports, per way, an off-class irrelevance rate — how often its fires landed in sessions whose activity (judged by the parent-family of the ways that co-fired) never touched the way's own domain. It distinguishes **mis-targeted** (a narrow way repeatedly firing into the same wrong kind of session — remedy: narrow the vocabulary or change the trigger channel; there is no per-way threshold to raise) from **cross-cutting** (a way that fires broadly by design, e.g. `meta/tracking` — remedy: scope by trigger, *never* auto-narrow vocabulary). Like `ways tune`'s fidelity audit, these are diagnostic flags, not verdicts.

A practitioner note: `events.jsonl` growth is bounded. `log_event` tail-compacts the file when it exceeds ~32 MiB, retaining the most recent ~24 MiB at a line boundary via atomic temp+rename — lossy on the oldest events, but readers always see a complete file (`session.rs`).
