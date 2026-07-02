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
/ways-tests suggest <way> [--apply]       # Vocabulary gaps (optionally update in place)
/ways-tests suggest --all [--apply]       # Analyze/update all ways
/ways-tests lint <way> | --all            # Validate frontmatter (+ tree health)
/ways-tests check <check> "context"       # Test check scoring curve
/ways-tests tree <path>                   # Progressive-disclosure tree structure
/ways-tests budget <path>                 # Token cost for a way tree
/ways-tests jaccard <tree> | <way1> <way2> # Sibling / pairwise vocabulary overlap
/ways-tests crowding "prompt"             # Vocabulary crowding across all ways
/ways-tests compare <path1> <path2>       # Side-by-side tree metrics
/ways-tests metrics                       # Session disclosure metrics
/ways-tests embed-status                  # Embedding engine health
/ways-tests embed-score[-all] [<way>] "prompt"  # Cosine score(s)
```

## Not for

- Authoring or editing ways themselves — that's the **ways** skill (`/ways`).
- Updating the agent-ways install — that's **ways-update**.
- This skill only *measures and validates*: scoring, vocabulary analysis, frontmatter and tree lint.

## Engine

Embedding-based semantic scoring is the sole retrieval tier (`all-MiniLM-L6-v2`,
~20ms batch). Cosine similarity on a **0–1** scale; a way fires when
`cosine(query, way) >= embed_threshold` (per-way, default **0.35**). Engine health
is `ways status` (binary / model / corpus state; if degraded → `make setup`, stale
corpus → `ways corpus`). That command *is* embed-status mode.

## Scoring  (score / score-all / embed-score)

After editing any `description`/`vocabulary`, regenerate so scores reflect it:
`ways corpus`. Then rank the whole corpus in one batch:

```bash
ways embed "$prompt"    # full corpus ranking by cosine — read down the list to debug a miss
                        # QUERY is positional; embed takes no --threshold (that's the per-way embed_threshold frontmatter field, not a CLI flag)
```

For a single way, grep its id (path relative to the ways root, e.g.
`softwaredev/security`) from the batch output.

**Always include cross-way context.** When scoring one way, also show the top 5–8
ranking, so you can see whether it *wins*, *defers* to a more specific way, or
*overlaps* a competitor:

```
Cosine  Thr   Match  Way
0.62    0.35  YES    softwaredev/environment/makefile  ← target
0.41    0.35  YES    documentation/standards
0.29    0.35  no     softwaredev/environment/deps
```

Flag: **overlap** (two ways within 0.05 cosine), **false dominance** (a non-target
outscores the target → tune its vocabulary), **healthy co-fire** (both fire,
complementary).

**Prompt battery** — for a broad evaluation, generate 8–12 diverse prompts: some
that should clearly match one way, some healthy co-fires, some boundary cases, some
that should match nothing. Gives a landscape view of the ecosystem.

### Interpreting cosine scores

| Range | Meaning |
|-------|---------|
| ≥ 0.7 | Strong — semantically close |
| 0.5–0.7 | Moderate — related domain |
| 0.35–0.5 | Weak — at/near default threshold |
| < 0.35 | Below default — no match |

Raise a way's `embed_threshold` to suppress weak false positives; lower toward 0.25
for broader catch on niche topics. At *scan* time (not here), a child's effective
threshold is ×0.8 when an ancestor has fired this session — the progressive-
disclosure mechanism (ADR-125). For multilingual stubs, `ways tune` reports locale
fidelity/discrimination (see the `knowledge/optimization/tuning` way).

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
appear in the body, so don't auto-remove. `--apply` rewrites the vocabulary line in
place (git-safety: refuses on untracked files unless `--force`); `--all --apply`
processes every way with gaps.

## Lint — frontmatter + tree health

```bash
ways lint            # all ways (global + project-local)
ways lint <dir>      # one directory
ways lint --check    # exit non-zero on errors (CI)
ways lint --schema   # full field reference
```

Checks: unknown/typo fields, invalid values, incomplete description↔vocabulary
pairs, `when:` blocks, `*.check.md` structure, sibling Jaccard (>0.15 warn, >0.25
error). It does **not** flag absent *optional* fields. With `--all` it also checks
tree health: threshold progression, orphans, >500-token ways, depth > 4.

## Tree — `ways tree <path>`

Structural analysis of a disclosure tree (depth, breadth, per-level thresholds).
Flag: **threshold inversion** (child ≤ parent — breaks progressive disclosure),
**flat thresholds** (no narrowing), sibling **Jaccard > 0.15** (`ways siblings`),
**orphans** (a way file with no ancestor way), **depth > 4** / **breadth > 7**
(over-decomposed).

## Jaccard — `ways siblings <tree>`

Vocabulary isolation between siblings — structural overlap, independent of any
prompt. Flag **> 0.15** (siblings compete; move shared terms up to the parent or
pick one owner) and **> 0.25** (collision; merge or split harder); show the shared
terms so the author knows what to relocate. 0.00 across all pairs is perfect
isolation — report it as a positive. (For one specific pair, diff the two
vocabulary sets directly.)

## Crowding — corpus-wide contention

No single subcommand: run `ways embed` for the prompt, cluster results within 0.05
cosine, and cross-check `ways siblings` / vocabularies. Matters at 50+ ways, where
embedding space gets contested. Flag: clusters of 3+ ways matching with overlapping
*purpose*, Jaccard > 0.25 pairs, and terms appearing in 4+ vocabularies (too
generic). Distinguish **accidental** overlap (sharpen vocabularies apart) from
**intentional** co-fire (mark healthy).

## Budget — token cost

No subcommand: estimate each way as frontmatter-stripped bytes ÷ 4, summed along
each root→leaf path. Flag: per-way > 500 tokens (consider splitting), path > 1500,
worst-case (all fire) > 5000, or one way accounting for > 40% of a tree's total.

## Compare — two trees side by side

No subcommand: present depth, total ways, threshold range, worst-case/avg tokens,
and max sibling Jaccard for each, then assess which is more mature and whether the
simpler one has room to grow. Useful for judging whether a refactor helped.

## Metrics — session disclosure

Read the session's disclosure metrics (`ways list`, or the metrics JSONL under the
sessions root). Reports per-tree coverage (which children fired, epoch distance) and
parent-activated threshold lowering. Flag: **orphaned roots** (root fires, no
children), **instant cascades** (parent+child same epoch = co-disclosed, not
progressive), **never-fire children** (vocabulary too narrow), **parent-only**
sessions (fine — the root sufficed).

## Check — scoring curve

Simulate a check's match / distance / decay curve over successive firings:

```bash
/ways-tests check design "editing architecture file" --distance 20 --fires 0
```

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
add "released/landed/merged" when "shipped" fixes the miss); threshold is a second
lever; accept some misses — 90% recall at 0 false-positives beats 100% recall at 5%.

## Notes

- Scores are cosine on the 0–1 scale; `embed_threshold` (default 0.35) is the per-way cutoff.
- The `way-embed` binary + model live under `~/.cache/claude-ways/user/` (via `make setup`); `ways status` checks them.
- After editing any `description`/`vocabulary`, run `ways corpus` so embedding scores reflect the change.
- Present results human-readably, not raw machine output.
