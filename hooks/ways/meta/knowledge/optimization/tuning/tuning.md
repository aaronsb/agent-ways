---
description: locale alias tuning — root-anchored fidelity and discrimination audit, the ways tune acceptance gate for adopter-run localization
vocabulary: tune tuning audit fidelity discrimination confuser anchor root gap margin re-author stub locale lang acceptance gate localized
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Locale Alias Audit

`ways tune` is the **acceptance gate** for adopter-run localization (ADR-139). It runs
only in **localized mode** (a non-English `output_language`); in English mode there is
nothing to audit and it returns clean. It measures embedding health against the
multilingual model and does not write thresholds — there are no per-way thresholds to
write; firing is global (τ_s / τ_k on the calibrated g(s), ADR-156 — see
engine-reference.md), and stub quality is fixed by re-authoring, not by moving gates.

English is the **source of truth.** Each way's English frontmatter is embedded into the
multilingual corpus as the per-way **anchor**, and every localized alias is scored
*against the root* — not against sibling translations. A cluster of mutually-agreeing
bad translations cannot self-certify; each stands or falls by its alignment to English.

## Two measurements

The tool runs each locale's `description + vocabulary` as a query against the
multilingual corpus, scoped to the active (or `--lang`) language, and reports:

**Fidelity** — cosine alignment to the **English root anchor**. For a single localized
language the alias's only same-way peer is the root, so `min_peer` *is* root-alignment:
how well the translation tracks the source of truth. Low fidelity → the stub drifted
from the English meaning.

**Discrimination** — `min_peer − top_confuser.score`. The top confuser is the
best-scoring alias on any *other* way. A negative gap means another way outranks this
stub — it collides and will mis-route. Orthogonal to fidelity; both must hold.

## Workflow

```bash
ways tune                       # audit the active localized language
ways tune --lang es             # scope to one language
ways tune --way delivery/commits
ways tune --fidelity-threshold 0.55 --discrimination-threshold 0.05
ways tune --json
```

`ways tune` is the loop the **ways-localize** skill drives: translate → corpus → tune →
re-author flagged stubs → repeat until clean. "Clean" (no flagged entries) is the
*evidence* a localization is done — not an assertion.

## Two failure modes

### 1. Low fidelity — drifted from the root

The translation embeds far from the English original: a mistranslation, or a stub that
misread the way's intent. **Fix:** re-author the offending locale against the English
frontmatter (the root). Translate the *intent* and the objective match words in local
form, not word-for-word.

### 2. Negative discrimination — collides with another way

The stub embeds closer to a *different* way than its own anchor. **Fix** by what the
confuser is:
- **Sibling way** (e.g. `delivery/commits` vs `delivery/branching`) — genuine neighbors;
  sharpen both so each holds a distinct region.
- **Parent/ancestor** (e.g. `delivery/commits` vs `delivery`) — some overlap is
  expected; add child-specific terminology.
- **Unrelated** (e.g. outranked by `code/quality`) — accidental vocabulary overlap,
  often a generic term doing too much work. Remove or specialize it.

A broad-vocabulary way that keeps appearing as the confuser across many stubs is a
signal to trim *its* vocabulary so it stops hoovering traffic meant for specialized
children.

## Output signal, not verdict

Diagnostic, not prescriptive. Read the values:
- Gap > 0 — stub clears its neighbors; ignore a mild fidelity flag.
- Gap between −0.05 and 0 — marginal; watch.
- Gap more negative than −0.10 — real collision; prioritize.
- Fidelity < 0.40 — translation likely wrong regardless of discrimination.

## See Also

- knowledge/optimization(meta) — broader vocabulary tuning and sparsity principles
- knowledge/authoring(meta) — way file format, coordinate-alias model
- the design note `adopter-localization-lifecycle-and-tuning` — the root-anchored model in full
