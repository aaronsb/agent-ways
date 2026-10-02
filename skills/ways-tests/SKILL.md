---
name: ways-tests
description: Score way matching via embedding similarity, analyze vocabulary, and validate frontmatter. Use when testing how well a way matches prompts, checking cosine similarity scores, inspecting the embedding engine status, or validating way files.
allowed-tools: Bash, Read, Glob, Grep, Edit
---

# ways-tests: Way Matching & Vocabulary Tool

Measure how a way matches prompts, analyze vocabulary, and validate structure. This
skill is the **interpretation layer** over the `ways` binary's measurement commands
— run the command, read the judgment here. For exact flags, `ways <cmd> --help`.

## Usage

```
/ways-tests score <way> "prompt"          # Score one way against a prompt
/ways-tests score-all "prompt"            # Rank all ways against a prompt
/ways-tests suggest <way>                 # Vocabulary gaps for one way (report only)
/ways-tests lint [<path>]                 # Validate frontmatter
/ways-tests check <check> "context"       # Work out a check's scoring curve (by hand)
/ways-tests tree <path>                   # Progressive-disclosure tree structure
/ways-tests budget <path>                 # Token cost for a way tree
/ways-tests jaccard <tree> | <way1> <way2> # Sibling / pairwise vocabulary overlap
/ways-tests crowding "prompt"             # Vocabulary crowding across all ways
/ways-tests compare <path1> <path2>       # Side-by-side tree metrics
/ways-tests metrics                       # Session disclosure metrics
/ways-tests embed-status                  # Embedding engine health
/ways-tests embed-score[-all] [<way>] "prompt"  # Legacy single-vector cosine score(s)
```

Each mode maps to a `ways` command, or, where no command exists (check, budget,
crowding, compare, metrics), to steps you carry out by reading command output and
the way files. The sections below say which.

## Not for

- Authoring or editing ways themselves — that's the **ways** skill (`/ways`).
- Updating the agent-ways install — that's **ways-update**.
- This skill only *measures and validates*: scoring, vocabulary analysis, frontmatter and tree lint.

## Engine

The live semantic matcher is **late interaction** (ADR-160): the prompt is split into
chunks, each chunk is matched against every way, and a way is admitted on its summed
softmax **share** or its **peak** chunk cosine, then **confirmed** against its own body
prose. `ways match "prompt"` shows those quantities per candidate. The single-vector
gate described next (ADR-156) is the fail-safe fallback, used when a surface is too
sparse to chunk or the late-interaction path cannot run. `ways match` says so and
prints the single-vector view when that happens.

A way has **two lanes** (ADR-156).

- **Keyword lane** — the regex `pattern:` frontmatter field, matched against the prompt.
- **Semantic lane** — the prompt is embedded (`all-MiniLM-L6-v2`, ~20ms batch) and
  scored by cosine `s` against the way's alias (`description` + `vocabulary`) on a
  **0–1** scale. That raw cosine is mapped through a per-model logistic
  `g(s) = σ(a·s + b)` — fit at corpus-generation and stored in `embed-manifest.json`
  — into a **relevance probability**.

A way **fires** when

```
g(s) ≥ τ_s   ∨   (keyword_match ∧ g(s) ≥ τ_k)
```

with **global** thresholds `τ_s = 0.5` (`semantic_fire_probability`) and
`τ_k = 0.15` (`keyword_floor_probability`), independent of each other. The keyword
lane is **floor-gated**: a `pattern:` hit only fires when the semantic probability
already clears `τ_k`, so a keyword can't drag in an unrelated prompt *when calibration is
loaded* (with no calibrated signal the keyword lane fails open and fires unconditionally).
`pattern_strict: true` bypasses the gate (unconditional keyword fire) by design. There is **no
per-way threshold** — the cutoffs are global and live in probability space, not raw
cosine.

Engine health is `ways status` (binary / model / corpus state; if degraded →
`make setup`, stale corpus → `ways corpus`). That command *is* embed-status mode.

## Scoring  (score / score-all / embed-score)

After editing any `description`/`vocabulary`, regenerate so scores reflect it:
`ways corpus`. Then rank the whole corpus in one batch:

```bash
ways match "$prompt"            # live matcher (ADR-160): peak · share · confirm · outcome per way
                                # QUERY is positional. There is no --threshold flag. The keyword
                                # lane is the way's pattern: field, not a CLI option.
```

`ways match` covers project-local ways too (`--project <dir>`, default the current
directory). Its outcome column reads `fired ✓`, `< gate` (admitted by neither share
nor peak) or `< confirm` (admitted, but the way's body did not corroborate the chunk
it won). For a single way, grep its id (path relative to the ways root, e.g.
`softwaredev/security`) from the batch output.

**Always include cross-way context.** When scoring one way, also show the top 5–8
ranking, so you can see whether it *wins*, *defers* to a more specific way, or
*overlaps* a competitor. On the late-interaction view, read the outcome column. On the
single-vector fallback view, read the decision off the calibrated probability `g(s)`
against the global semantic bar `τ_s = 0.5` (keyword-lane fires against `τ_k = 0.15`):

```
Cosine  g(s)   Fire  Way
0.62    0.88   YES   softwaredev/environment/makefile  ← target (semantic lane)
0.41    0.57   YES   documentation/standards
0.29    0.34   no    softwaredev/environment/deps      ← below τ_s; keyword hit needed
```

Flag: **overlap** (two ways within 0.05 cosine), **false dominance** (a non-target
outscores the target → tune its vocabulary), **healthy co-fire** (both fire,
complementary).

**Prompt battery** — for a broad evaluation, generate 8–12 diverse prompts: some
that should clearly match one way, some healthy co-fires, some boundary cases, some
that should match nothing. Gives a landscape view of the ecosystem.

### Interpreting scores

The raw cosine is the **input** to `g(s)`, not a fire cutoff. Read the decision off
the calibrated probability:

| `g(s)` | Meaning |
|--------|---------|
| ≥ τ_s (0.5) | Fires on the semantic lane alone |
| τ_k–τ_s (0.15–0.5) | Below the semantic bar; fires **only** with a keyword (`pattern:`) hit, floor-gated |
| < τ_k (0.15) | Below the keyword floor; no fire even on a pattern hit (unless `pattern_strict`) |

`g(s) = σ(a·s + b)` is per-model — the `a`/`b` coefficients come from
`embed-manifest.json`, so the same cosine maps to different probabilities under the EN
and multilingual models. Don't reason about a raw-cosine cutoff; reason about `g(s)`
vs `τ_s`/`τ_k`.

When a way fires too broadly or misses, the remedy is **measure → edit
vocabulary/pattern → re-measure** through `tools/scripts/probe-measure.py` — never
"move a threshold" (there is none per-way to move). Sharpen or widen the vocabulary
for the semantic lane; add or tighten `pattern:` for the keyword lane. For a
load-bearing **common word** the pattern-hygiene lint would strip, add it to
`pattern_keep` — the keyword stays and its off-sense noise is held back by the `τ_k`
floor.

At *scan* time (not here), when an ancestor has fired this session a child's effective
semantic bar is lowered from `τ_s` to `(τ_s × parent_threshold_multiplier).max(parent_boost_floor)`
— by default `max(0.5×0.8, 0.30) = 0.40`, in probability space (the multiplier boosts, the floor
caps) — the progressive-disclosure mechanism (ADR-105/123/126). For multilingual
stubs, `ways tune` reports locale fidelity/discrimination (see the
`knowledge/optimization/tuning` way).

## Resolving way paths

Given a short name like "security": check `$CLAUDE_PROJECT_DIR/.claude/ways/` first,
then `~/.claude/hooks/ways/` recursively for `*/security/security.md`; if multiple
match, list them and ask.

## Suggest — vocabulary gaps

```bash
ways suggest "$wayfile" --min-freq 2
```

Sections: GAPS (body terms missing from vocabulary), COVERAGE, UNUSED, VOCABULARY.
UNUSED is usually *intentional* — vocabulary catches user-query terms that don't
appear in the body, so don't auto-remove. `ways suggest` only reports; it takes one
file and has no flag that writes. Edit the `vocabulary:` line by hand, then
`ways corpus` and re-score. To survey several ways, run it once per file.

## Lint — frontmatter

```bash
ways lint            # project ways inside a project, else global
ways lint --global   # global ways
ways lint <path>     # one file or directory
ways lint --check    # exit non-zero on errors (CI)
ways lint --schema   # full field reference
```

Checks: unknown/typo fields, invalid values, incomplete description↔vocabulary
pairs, `when:` blocks, `*.check.md` structure, pattern hygiene, locale stubs,
provenance. It does **not** flag absent *optional* fields, and it does not check tree
health. For sibling overlap use `ways tree <path> --jaccard` (vocabulary) or `ways siblings <id>` (embedding cosine); for
size and depth read `ways tree <path>` (see Tree and Budget below). `--all` only
widens `--fix` to the whole corpus.

## Tree — `ways tree <path>`

Structural analysis of a disclosure tree (depth, breadth, per-level vocabulary
specificity). There are **no per-level thresholds** — firing uses the global
`τ_s`/`τ_k`, and disclosure is governed by parent-boost (`parent_threshold_multiplier`,
floored at `parent_boost_floor`) plus how much more specific each level's vocabulary is than
its parent's. Flag: **weak narrowing** (a
child no more specific than its parent — nothing for progressive disclosure to earn),
sibling **Jaccard > 0.15** (`ways tree <path> --jaccard`), **orphans** (a way file with no ancestor
way), **depth > 4** / **breadth > 7** (over-decomposed).

## Jaccard — `ways tree <tree> --jaccard`

Vocabulary isolation between siblings — structural overlap, independent of any
prompt. Flag **> 0.15** (siblings compete; move shared terms up to the parent or
pick one owner) and **> 0.25** (collision; merge or split harder); show the shared
terms so the author knows what to relocate. 0.00 across all pairs is perfect
isolation — report it as a positive. (For one specific pair, diff the two
vocabulary sets directly. `ways siblings <id>` is the embedding-space counterpart: way-vs-way cosine, default floor 0.3.)

## Crowding — corpus-wide contention

No single subcommand. Steps: run `ways match` for the prompt, cluster candidates
whose peak is within 0.05 of each other, and cross-check `ways siblings` /
vocabularies. Matters at 50+ ways, where
embedding space gets contested. Flag: clusters of 3+ ways matching with overlapping
*purpose*, Jaccard > 0.25 pairs, and terms appearing in 4+ vocabularies (too
generic). Distinguish **accidental** overlap (sharpen vocabularies apart) from
**intentional** co-fire (mark healthy).

## Budget — token cost

No subcommand computes the totals. `ways tree <path>` prints each way's Tokens
(frontmatter-stripped body bytes ÷ 4). Steps: sum that column along each root→leaf
path for the realistic cost, and over the whole tree for the worst case. Flag: per-way > 500 tokens (consider splitting), path > 1500,
worst-case (all fire) > 5000, or one way accounting for > 40% of a tree's total.

## Compare — two trees side by side

No subcommand. Steps: run `ways tree <path> --jaccard` on each tree, then present
depth, total ways, vocabulary-specificity narrowing, worst-case/avg tokens (from the
Tokens column), and max sibling Jaccard for each, then assess which is more
mature and whether the simpler one has room to grow. Useful for judging whether a
refactor helped.

## Metrics — session disclosure

No subcommand computes these. Steps: read the session's firings with `ways list`
(epoch per fired way; `--json` for the raw rows) or `ways introspect dump` (turns,
fired ways and their criteria), then group them by tree. Report per-tree coverage (which children fired, epoch distance) and
parent-activated bar lowering (`parent_threshold_multiplier`, floored at `parent_boost_floor`). Flag: **orphaned roots** (root
fires, no children), **instant cascades** (parent+child same epoch = co-disclosed,
not progressive), **never-fire children** (vocabulary too narrow), **parent-only**
sessions (fine — the root sufficed).

## Check — scoring curve

No command simulates the curve: `ways show check` fires the check for real against
a session. Compute it by hand from the check file. The rule is `check_within` in
`tools/ways-cli/src/cmd/show/mod.rs` under the app source
(`${XDG_DATA_HOME:-~/.local/share}/agent-ways`):

1. `distance` = epochs since the parent way fired, capped at 30 (30 if it has not fired).
2. `fires` = times this check has already fired this session.
3. `effective = match_score × (ln(distance + 1) + 1) × 1 / (fires + 1)`.
4. The check fires when `effective ≥ threshold` (the check file's `threshold:`,
   default 2.0). Its anchor section is included when `distance ≥ 5`.

Tabulate `effective` over a few `distance` / `fires` values to show the curve.

## Evaluation Guidelines

Always close with an **assessment** that interprets the numbers, not just the
numbers: *clean win* (clear top scorer with daylight), *healthy co-fire*
(complementary roles), *overlap concern* (competing at similar scores), *false
negative* (should fire, doesn't — vocabulary gap), *false positive* (fires too
broadly).

## Authoring Techniques

**Intentional co-fire.** Default to sparsity — one prompt, one right way. When two
ways *should* fire together (a project way + a user way for "create a PR"), plant a
few *narrow* shared terms ("pull request", "PR", "ship") in both rather than writing
a third combined way — two small ways beat one large one. Keep shared terms narrow
(never "code"/"deploy"); verify with crowding that the co-fire happens only on
intended prompts.

**Sparsity as overfitting guard.** Every added vocabulary term is a surface for
false matches. 15 precise terms beat 40 general ones; one term per concept (don't
add "released/landed/merged" when "shipped" fixes the miss). The two levers are
**vocabulary** (the semantic lane) and **pattern** (the regex keyword lane) — both
measured through the calibration, neither a per-way threshold. Accept some misses —
90% recall at 0 false-positives beats 100% recall at 5%.

## Notes

- Scores are cosine on the 0–1 scale, mapped through `g(s) = σ(a·s + b)` to a
  probability; the fire cutoffs are the **global** `τ_s = 0.5` / `τ_k = 0.15`, not a
  per-way value.
- The `way-embed` binary + model live under `~/.cache/agent-ways/user/` (via `make setup`); `ways status` checks them.
- After editing any `description`/`vocabulary`, run `ways corpus` so embedding scores reflect the change.
- Present results human-readably, not raw machine output.
