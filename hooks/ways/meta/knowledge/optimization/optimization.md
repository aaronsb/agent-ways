---
description: optimizing way vocabulary, tuning thresholds, reviewing matching quality, analyzing gaps and coverage
vocabulary: optimize vocabulary suggest gaps coverage unused threshold tune scoring health audit sparsity discrimination overlap
macro: prepend
scope: agent
requires: ["Read", "Bash(awk:*)", "Bash(find:*)", "Bash(grep:*)", "Bash(sed:*)", "Bash(sort:*)", "Bash(ways:*)"]
refire: 0.15
---
<!-- epistemic: heuristic -->
# Way Optimization

## Workflow

```
suggest → interpret → apply → test → verify
```

1. **Survey**: `/ways-tests suggest --all` (or `--all --summary` for overview)
2. **Interpret**: Gaps vs intentional unused (see below)
3. **Apply**: `/ways-tests suggest <way> --apply` (git-safe, shows diff)
4. **Test**: `/ways-tests score-all "<sample prompt>"` to verify discrimination
5. **Verify**: `make test-sim` for regression

## Reading Suggest Output

| Section | Meaning | Action |
|---------|---------|--------|
| **GAPS** | Body terms not in vocabulary (freq >= 2) | Add if the term catches user prompts |
| **COVERAGE** | Vocabulary terms found in body | Healthy — these are working |
| **UNUSED** | Vocabulary terms not in body | Often intentional — they catch *user* terms, not body terms |

**Don't blindly add all gaps.** Body text uses terms like "the", "code", "use" that don't discriminate between ways. Good vocabulary terms are *domain-specific* words users would say when asking about this topic.

**Don't remove unused terms.** Terms like `owasp`, `csrf`, `xss` in security vocabulary exist to catch user prompts, not because they appear in the way body.

## Sparsity and Discrimination

The goal isn't to maximize each way's score — it's to maximize the **semantic distance between ways**. Narrow, distinct vocabularies create sparsity: each way occupies its own region of the scoring space with minimal overlap. This means prompts activate exactly the right guidance, not a cluster of partially-relevant ways.

```bash
/ways-tests score-all "the ambiguous prompt"
```

Ideal outcome: one way scores well above threshold, others score well below. If two ways both match the same prompt, their semantic regions overlap — they need sharpening.

**Sharpening strategies:**
- Add discriminating terms unique to each way's domain
- Remove shared generic terms that don't differentiate
- Raise the threshold on the less-specific way
- Don't blindly expand vocabulary — more terms can *reduce* sparsity by creating new overlaps

## Which Ways Use Semantic Matching

Only ways with both `description:` and `vocabulary:` frontmatter fields use semantic matching. Ways with `match: regex`, `files:`, or `commands:` triggers don't need vocabulary optimization — they match on patterns.

## Thresholds

- **Embedding threshold** (frontmatter `embed_threshold:`): Cosine similarity, 0–1 scale. One value per node (English frontmatter); default is `config.default_embed_threshold` (0.35). Locale stubs do NOT carry their own thresholds (ADR-125).
- **Parent-boost**: once an ancestor way has fired in the session, a child's effective threshold is multiplied by `config.parent_threshold_multiplier` (default 0.8). This is how progressive disclosure amplifies in-domain children; see [hooks-and-ways/matching.md](../../../../docs/hooks-and-ways/matching.md).

Lowering the base threshold increases recall but risks false positives. The test harness tracks FP rate — **0 FP is the hard constraint**.

### Locale alias audit with `ways tune`

The tuner does NOT write thresholds (ADR-125). It measures per-locale embedding health so authors know which stubs to re-author:

- **Fidelity** — min cosine against peer aliases on the same way. Low fidelity means one language's stub diverges from the others.
- **Discrimination** — `min_peer − top_confuser.score`. Negative means some other way's alias outranks this locale's own peers.

```bash
ways tune                                    # full audit
ways tune --way delivery/commits             # single way
ways tune --fidelity-threshold 0.55          # looser fidelity gate
ways tune --discrimination-threshold 0.05    # require +0.05 margin
ways tune --json                             # machine-readable
```

The audit names the **top confuser** — which other way's alias is winning against this one in embedding space. Low discrimination means revising the stub vocabulary (or sometimes the confuser's vocabulary if it's hoovering up too much neighborhood). See `knowledge/optimization/tuning(meta)` for failure-mode categories and fix strategies.

## Health Indicators

- **Gap ratio**: gaps / (gaps + coverage). High ratio = vocabulary may be too narrow.
- **Unused ratio**: unused / total vocabulary. High ratio isn't bad — unused terms serve user-facing matching.
- **0 FP**: The test harness must maintain zero false positives. Accuracy can vary but FP cannot.

Stop when vocabulary changes stop changing test outcomes.
