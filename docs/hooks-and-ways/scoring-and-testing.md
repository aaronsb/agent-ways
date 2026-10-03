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

```text
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

Each way has a description and a vocabulary, which together form its alias in embedding space. The matcher scores a prompt against every alias, maps the score through a per-model calibration into a relevance probability, and fires the ways that clear the global bar. A `pattern:` hit fires on a lower floor. There is no per-way threshold to set; the only levers are the vocabulary, the description and the pattern. [engine-reference.md](engine-reference.md) gives the rule, and [matching.md](matching.md) the matcher. After the match, the relevance gate (ADR-196), when it is on, asks a small model whether each prompt-lane match is relevant and can withhold it; that shows up as a `way_judged` event with `verdict: block`, not as a low score.

Getting the vocabulary right matters: a way that fires too eagerly drowns the session in irrelevant guidance, and a way that never fires is dead weight. With 150+ ways in the corpus, vocabulary space gets crowded, and adding terms to one way can create overlap with another. The only way to know is to test. What follows is information-retrieval evaluation in miniature: a test collection with relevance judgments, tuned precision-first. [matching.md](matching.md#what-this-actually-is) traces that lineage.

## How Test Prompts Get Written

Claude writes the test prompts by modeling how the operator would phrase the need. It knows what the way is for, from its description and the conversation that led to it, and turns that into the different ways a person would ask.

For `softwaredev/delivery/commits`, which covers commit messages and conventional-commit prefixes, that gives prompts like:

- "write the commit message for this" (direct)
- "what prefix should this commit use, feat or fix" (uses the domain's words)
- "squash these into one commit before I push" (adjacent task, same domain)

And negative prompts, ones that sound related but belong elsewhere:

- "add error handling to the parser function" (code task)
- "how do I create a new way" (meta, about authoring)

**Vocabulary gaps hide between how the author thinks about the concept and how the user phrases the need.** The author writes `conventional prefix scope` thinking about the format; the user types "should this be feat or fix". Generating prompts from the user's side closes that gap. It is also why scoring happens while the way is written: the conversation that produces the way is the context needed to write authentic prompts.

## The Tool

`ways author match "<prompt>"` scores one prompt against every way in the corpus and ranks them. It shows the live late-interaction matcher (ADR-160) per candidate: peak chunk cosine, summed share, body-confirm, and whether the way fired. When the prompt is too sparse to chunk, or late interaction is unavailable, it falls back to the single-vector view: one cosine per model (EN, Multi), ranked. The fire path falls back the same way.

The corpus is what gets scored, so run `ways corpus` after every edit to a description or vocabulary, before matching again.

The `/ways-tests` skill wraps this with higher-level procedures: scoring one way or all ways, vocabulary gaps, tree structure, crowding and budget. Some of its modes map to a command; the rest are steps it carries out over command output.

## The Process: A Worked Example

Scoring `softwaredev/delivery/commits` with `ways author match`, single-vector view, EN cosine. Columns trimmed.

### Step 1: Score the target prompts

```
$ ways author match "write the commit message for this"
  softwaredev/delivery/commits     0.5170  — git commit messages, branch naming, …
  softwaredev/delivery             0.3668  — Shipping code — commits, pull requests, …
  meta/trust/delegation            0.3384  — sending email, posting chat messages, …

$ ways author match "what prefix should this commit use, feat or fix"
  softwaredev/delivery/commits     0.5486  — git commit messages, branch naming, …
  softwaredev/delivery/release     0.3950  — software releases, the changelog, …
  softwaredev/code/quality/versioning  0.3846  — version-numbered identifiers, …

$ ways author match "squash these into one commit before I push"
  softwaredev/delivery/commits     0.5206  — git commit messages, branch naming, …
  softwaredev/delivery             0.3845  — Shipping code — commits, pull requests, …
```

The target ranks first on every prompt, about 0.13 to 0.15 above the next way. That gap matters more than the absolute score: it means the prompt belongs to this way and no other.

### Step 2: Score the negative prompts

```
$ ways author match "add error handling to the parser function"
  softwaredev/code/errors          0.4012  — error handling — exceptions, …
  softwaredev/code/security/injection  0.2840  — injection prevention — …

$ ways author match "how do I create a new way"
  meta/knowledge                   0.2955  — how ways and progressive disclosure work …
  documentation/adr-context        0.2807  — planning how to implement a feature, …
```

`softwaredev/delivery/commits` does not appear near the top of either. The negatives went to the ways that own them.

### Step 3: Fix a miss, then re-score

When a target prompt ranks the way low, or a neighbour sits within a few hundredths of it, find the words in the prompt that the vocabulary lacks and add the ones users actually say. Run `ways corpus`, then re-score every target and every negative. A fix that pulls a negative prompt toward the way is a regression.

### Step 4: Check the neighbours

`ways author siblings softwaredev/delivery/commits` scores the way against every other way. A sibling that sits close on many prompts is competing for the same space; sharpen the two vocabularies apart, or confirm the co-fire is intended (below).

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

This is why the neighbour check (Step 4 in the worked example) and the gap to the next way matter more than the individual scores. A way that clears `τ_s` on its target prompt and has clean separation from every other way is healthier than a way that scores high but overlaps with three neighbors.

Concretely:

- **Narrow vocabularies are better than broad ones.** 15 precise terms beat 40 general terms. "conventional", "prefix", "squash" are specific to commits. "update", "check", "status" are shared by many domains.
- **Don't chase every synonym.** If "shipped" fixes a miss, add it. But don't then add "deployed", "released", "landed", "merged", "delivered" — each one increases the surface area for false matches against delivery/release or delivery/github.
- **The two levers are vocabulary and pattern — both measured through the calibration, neither a per-way threshold.** Sharpen or widen the `vocabulary` to move the semantic lane; add or tighten the `pattern:` regex for the keyword lane. When a way fires correctly but also fires weakly on unrelated prompts, the remedy is to narrow the vocabulary, not to reach for a knob that no longer exists. (A keyword that leaks *globally* is the `τ_k` floor's job, not the way's — see the remedy loop in [authoring-docs-style.md](authoring-docs-style.md).)
- **Accept some misses.** A way that fires for 90% of relevant prompts with zero false positives is better than one that fires for 100% but also fires for 5% of irrelevant prompts. The 0 FP constraint is hard; recall is soft.

The test harness enforces this: it tracks false positive rate as a hard constraint (must be 0) while accuracy can vary. Sparsity is how you maintain 0 FP as the vocabulary grows.

### Intentional co-fire: sparsity's inverse

Sparsity is the default — keep ways apart. But sometimes you *want* two ways to fire together. A project-scoped way and a user-scoped way might both be relevant when someone says "create a PR." A GitHub way and a custom Jira way might both need to fire when someone says "ship this ticket."

Rather than writing a third way that combines both concerns (more content to maintain, more context consumed), you can plant shared vocabulary terms in both ways so that the embedding scorer naturally co-fires them on the same prompt. Two small ways that each contribute their piece is lighter than one large way that tries to cover everything.

This is a deliberate vocabulary manipulation — the opposite of sharpening. You're *reducing* the distance between two ways for specific prompts where both are genuinely needed. The key discipline is that the shared terms should be narrow: "pull request", "ship", "PR" — not broad terms like "code" or "deploy" that would create accidental overlap on unrelated prompts.

`/ways-tests crowding "<prompt>"` is a manual procedure in the skill, not a command: run `ways author match` for the prompt, group the ways scoring within 0.05 of each other, and cross-check them with `ways author siblings` and `ways author tree <tree> --jaccard`. Overlap on a prompt neither way should own is accidental; sharpen the vocabularies apart. Two ways scoring well on a prompt both should serve is intentional co-fire.

## Tools Reference

| Command | Purpose |
|---------|---------|
| `ways author match "prompt"` | Rank all ways against a prompt |
| `ways author suggest <way-file>` | Vocabulary candidates: body terms missing from the vocabulary |
| `ways author lint <path>` | Validate way frontmatter (`--check` for CI) |
| `ways corpus` | Rebuild the corpus after editing a description or vocabulary |
| `ways tune locale` | Audit locale alias fidelity + discrimination (per-way, across all languages) |
| `ways tune locale --way <path>` | Filter the audit to a single way or subtree |
| `ways tune precision` | Heuristic relevance audit: flag ways firing into off-domain sessions (`--min-sessions`, `--flag-threshold`, `--project`, `--way`, `--json`) — ADR-134 Decision 3 |
| `ways author tree <path> --jaccard` | Compute vocabulary overlap (Jaccard) between sibling ways |
| `ways author siblings <id>` | Way-vs-way embedding cosine (`all` for the full matrix) |

See the [ways-tests skill](/skills/ways-tests/SKILL.md) for the testing skill and [Locale Alias Audit](../../hooks/ways/meta/knowledge/optimization/tuning/tuning.md) (the `knowledge/optimization/tuning` way) for the `ways tune locale` workflow in depth.

## Empirical Signals: Tuning From What Actually Fired

The worked example tunes a way against prompts written by hand. Once a way ships, the event log is the evidence ([ADR-134](../architecture/ways/ADR-134-empirical-auto-tuning-from-fire-and-near-miss-telemetry.md)). Four events carry the signals; [the event log](../reference/events.md) lists their fields.

- **`way_nearmiss`**: a semantic probability landed within `near_miss_margin` (default 0.05) under the fire bar and nothing fired. These are the false silences a precision-first discipline cannot otherwise see. A way that keeps landing just under the bar on prompts whose sessions then do its kind of work is a candidate to widen.
- **`fire_score`** on `way_fired` and `way_redisclosed`: the calibrated probability that fired a semantic match, with the `surface` it matched. Keyword, command and file fires carry none. `ways tune precision` reads these. The calibration itself is fit at corpus build from the committed `calibration_probes.jsonl`, not from this stream.
- **`way_keyword_gated`**: a `pattern:` hit vetoed by the keyword floor, with the `matched_span`. It shows which alternation of a pattern keeps matching the wrong prompts.
- **`way_judged`** with `verdict: block` or `would_block`: the relevance gate judged a match irrelevant. A way the judge blocks often is matching prompts it should not; narrow its vocabulary rather than relying on the gate.

`ways tune precision` reports, per way, how often its fires landed in sessions whose other activity never touched the way's domain. It separates a **mis-targeted** way (narrow it, or change its trigger channel) from a **cross-cutting** one (scope it by trigger; never narrow it automatically). Its output is a diagnostic flag, not a verdict. [stats.md](stats.md) covers it with `ways tune stats`.

### Plotting the raw score distributions

`scripts/signal-report.py` scores a battery of prompts against every way with the single-vector matcher and plots signal (the expected way) against noise (every other way) per model, with a per-prompt chart of the expected way against its top competitor. It shows the raw cosine bands the calibration is fit against; it does not show the fire decision. It writes `scores.csv` and two PNGs to `--out DIR` (default `./signal-analysis`). Pass `--prompts FILE` for your own battery, one `{"lang", "expected_way", "prompt"}` object per line. For the per-way remedy loop, use `tools/scripts/probe-measure.py` instead.
