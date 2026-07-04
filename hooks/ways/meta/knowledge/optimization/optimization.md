---
description: optimizing way vocabulary, reviewing matching quality, analyzing gaps and coverage, sharpening discrimination
vocabulary: optimize vocabulary suggest gaps coverage unused tune scoring health audit sparsity discrimination overlap
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

Ideal outcome: one way clears the fire probability (`g(s) ≥ τ_s`) while others fall below it. If two ways both match the same prompt, their semantic regions overlap — they need sharpening.

**Sharpening strategies:**
- Add discriminating terms unique to each way's domain
- Remove shared generic terms that don't differentiate
- Remedy overlap by editing the vocabulary/pattern content — add discriminating terms, drop shared generic terms, anchor or bound the pattern alternations — then measure the change through the calibration (`tools/scripts/probe-measure.py`). There is no per-way threshold to move.
- Don't blindly expand vocabulary — more terms can *reduce* sparsity by creating new overlaps

## Which Ways Use Semantic Matching

Only ways with both `description:` and `vocabulary:` frontmatter fields use semantic matching. Ways with `match: regex`, `files:`, or `commands:` triggers don't need vocabulary optimization — they match on patterns.

## How Firing Is Scored

Firing is decided globally, not by a per-way threshold (ADR-156):

- The semantic lane embeds the prompt and takes cosine `s` against the way's alias (`description` + `vocabulary`). A per-model logistic maps that cosine to a **relevance probability**, `g(s) = σ(a·s + b)`, fit at corpus-generation and stored in `embed-manifest.json`.
- A way fires when `g(s) ≥ τ_s`, or when a keyword pattern hits and `g(s) ≥ τ_k`. The bars are **global and independent**: `τ_s = 0.5` (`semantic_fire_probability`), `τ_k = 0.15` (`keyword_floor_probability`).
- **Parent-boost**: once an ancestor way has fired in the session, an in-domain child's probability is lifted toward `parent_boost_floor` (0.30), applied in probability space. This is how progressive disclosure amplifies in-domain children; see [hooks-and-ways/matching.md](../../../../docs/hooks-and-ways/matching.md).

Recall and precision are shaped by the **content** of a way's vocabulary and pattern, measured through the calibration — not by moving a threshold. A way that misses in-domain prompts needs more discriminating vocabulary; a way that leaks needs its shared generic terms removed or its pattern bounded. The test harness tracks the false-positive rate — **0 FP is the hard constraint** and holds as a test invariant regardless of what content changes.

### The remedy loop

When a way mis-fires, the fix is **measure → edit vocabulary/pattern → re-measure**, never "move a threshold." Measure through `tools/scripts/probe-measure.py`, which scores candidate probes against the calibrated `g(s)`. For a keyword that is a common word and would otherwise trip the pattern-hygiene lint, list it in `pattern_keep` (ADR-155 §5): the keep is a *measured* exemption — the keyword is load-bearing and its off-sense noise stays floor-gated by `τ_k`.

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
