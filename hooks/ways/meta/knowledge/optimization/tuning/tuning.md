---
description: locale alias tuning — root-anchored fidelity and discrimination audit, the ways tune locale acceptance gate for adopter-run localization
vocabulary: tune tuning audit fidelity discrimination confuser anchor root gap margin re-author stub locale lang acceptance gate localized mis-route mistranslation hoovering broad-vocabulary root-alignment trim
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Locale Alias Audit

`ways tune locale` is the **acceptance gate** for a localization an adopter runs: it decides whether the translated stubs are ready (ADR-139). It runs only in **localized mode** (a non-English `ways.language`); in English mode there is nothing to audit and it returns clean. It measures how well each translated alias embeds in the multilingual model, and it writes no thresholds. No way has a threshold of its own: firing uses the global τ_s and τ_k on the calibrated g(s) (ADR-156, engine-reference.md). A weak stub is fixed by rewriting it.

English is the **source of truth.** The tool embeds each way's English frontmatter into the multilingual corpus as that way's **anchor**, and it scores every localized alias *against the anchor*, never against the other translations. Bad translations that agree with each other therefore cannot pass on that agreement; each one passes only if it aligns with the English.

## Two measurements

The tool runs each locale's `description + vocabulary` as a query against the multilingual corpus, scoped to the active (or `--lang`) language, and reports:

**Fidelity** — cosine alignment to the **English root anchor**. With one localized language, the English root is the alias's only peer within its way, so `min_peer` measures how closely the translation matches the English. Low fidelity means the stub has drifted from the English meaning.

**Discrimination** — `min_peer − top_confuser.score`. The top confuser is the best-scoring alias on any *other* way. A negative gap means another way outranks this stub, so prompts meant for this way reach the other one. Discrimination is independent of fidelity, and both must pass.

## Workflow

```bash
ways tune locale                       # audit the active localized language
ways tune locale --lang es             # scope to one language
ways tune locale --way delivery/commits
ways tune locale --fidelity-threshold 0.55 --discrimination-threshold 0.05
ways tune locale --json
```

`ways tune locale` is the loop the **ways-localize** skill drives: translate → corpus → tune → re-author flagged stubs → repeat until clean. A clean run, with no flagged entries, is the *evidence* that a localization is done.

## Two failure modes

### 1. Low fidelity — drifted from the root

The translation embeds far from the English original: a mistranslation, or a stub that misread the way's intent. **Fix:** re-author the offending locale against the English frontmatter (the root). Translate the *intent* and the objective match words in local form, not word-for-word.

### 2. Negative discrimination — collides with another way

The stub embeds closer to a *different* way than its own anchor. **Fix** by what the confuser is:
- **Sibling way** (e.g. `delivery/commits` vs `delivery/branching`) — genuine neighbors; sharpen both so each holds a distinct region.
- **Parent/ancestor** (e.g. `delivery/commits` vs `delivery`) — some overlap is expected; add child-specific terminology.
- **Unrelated** (e.g. outranked by `code/quality`) — accidental vocabulary overlap, often a generic term that matches too much. Remove the term or replace it with a more specific one.

When one way with a broad vocabulary shows up as the confuser for many stubs, trim *its* vocabulary so it stops taking prompts meant for its more specific children.

## Reading the output

The values are diagnostics. Read them this way:
- Gap > 0 — the stub outranks its neighbors; ignore a mild fidelity flag.
- Gap between −0.05 and 0 — marginal; watch.
- Gap more negative than −0.10 — real collision; prioritize.
- Fidelity < 0.40 — translation likely wrong regardless of discrimination.

## See Also

- knowledge/optimization(meta) — broader vocabulary tuning and sparsity principles
- knowledge/authoring/locale-stubs(meta) — the locale stub file and its coordinate-alias model
- ADR-183 — the root-anchored model in full
