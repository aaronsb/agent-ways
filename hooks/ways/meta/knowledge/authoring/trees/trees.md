---
description: decomposing a large way into a progressive disclosure tree of parent and child ways — when to split, the parent boost, sibling vocabulary isolation, token budgets, and anti-rationalization tables in leaf ways
vocabulary: tree child parent split decompose subway sub-way nest leaf sibling jaccard isolation boost cascade budget worst-case rationalization counter
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Progressive Disclosure Trees

When a way covers multiple distinct concerns (>80 lines, >2 sub-topics, language/tool-specific variants), decompose into a tree. The supply chain tree (`softwaredev/code/supplychain/`) is the reference implementation. A way whose delivered body is over the 10,000-character hook context cap must be split: `ways lint` reports it as an error.

**How disclosure works now** (ADR-125): ways are nodes in a DAG. When a parent fires, a session marker is set. Whenever any ancestor has a marker, an in-domain child's semantic bar is lowered from `τ_s` to `(τ_s × config.parent_threshold_multiplier).max(config.parent_boost_floor)` — by default `max(0.5 × 0.8, 0.30) = 0.40` — so children fire on weaker signal once their domain is active. The multiplier (0.8) is the boost; the floor (0.30) stops cascading boosts from reaching the noise band; both operate in probability space (ADR-156). This is the mechanism behind "progressive disclosure": children are always candidates, but the boost makes in-domain children easier to fire. Full model in [hooks-and-ways/matching.md](../../../../../../docs/hooks-and-ways/matching.md).

**Cross-firing between a child and its root** — thresholds are global (`τ_s` / `τ_k`), not per-way, so there is no threshold to raise on the child. When a child cross-fires with the root (or a sibling), sharpen the child's own signal instead: add discriminating vocabulary, tighten the `pattern:`, then verify with `tools/scripts/probe-measure.py`. The remedy loop is always **measure → edit vocabulary/pattern → re-measure** — never move a threshold (there is none to move).

**Vocabulary isolation** — sibling ways MUST NOT share vocabulary:
- Target Jaccard similarity < 0.15 between siblings
- Each child owns its own keyword space
- Use `ways tree <path> --jaccard` to verify; use `ways siblings <way-id>` for embedding similarity, and `ways tune --way <path>` to surface cross-way confusers in multilingual space

**Token awareness** — aim for:
- Realistic path (root→leaf): ~1200 tokens
- Worst case (all fire): ~4000 tokens
- Use `/ways-tests budget <tree>` to measure

**When NOT to tree**: Leave flat if <80 lines, single cohesive concern, or all content is needed together.

## Tree Validation

- `ways tree <path>` — structural analysis: depth, vocabulary size, and tokens per way
- `/ways-tests tree <path>` — structural analysis (depth, breadth, disclosure boost)
- `/ways-tests budget <path>` — token cost per way, per path, worst-case
- `/ways-tests crowding "prompt"` — vocabulary overlap detection
- `/ways-tests metrics` — session disclosure tracking (after live use)

## Anti-Rationalization Patterns

For high-stakes ways where the agent is tempted to skip steps (testing, security, supply chain), add a "Common Rationalizations" table:

```markdown
## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "This is simple, tests aren't needed" | If it's simple, the test is trivial. Write it. |
| "I'll add tests later" | Later never comes. Tests verify understanding NOW. |
```

**Placement**: In the specific leaf/mid-tier node, not the root. The table should only appear when the agent is actively doing the thing it might skip.

**Tone**: Direct, not preachy. State the fact. 5-7 rows max.

## See Also

- knowledge/authoring(meta) — parent: way format and matching
- knowledge/optimization(meta) — vocabulary tuning, sparsity, discrimination
