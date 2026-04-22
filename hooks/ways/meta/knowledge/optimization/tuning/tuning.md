---
description: locale alias fidelity and discrimination audit, ways tune workflow, fixing ambiguous stubs
vocabulary: tune tuning audit fidelity discrimination confuser peer alias gap margin re-author stub locales
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Locale Alias Audit

`ways tune` measures per-locale embedding health against the multilingual model. It does not write thresholds — per-ADR-125, thresholds are per-node (English frontmatter), and stub quality is fixed by re-authoring, not by adjusting gates.

## Two Measurements

The tool runs each locale's `description + vocabulary` text as a query against the multilingual corpus, and reports two numbers:

**Fidelity** — `min cosine` between this alias and its **same-way peers** (other languages of the same way). High fidelity means all translations of a way embed in the same neighborhood — they share the objective match words in local form. Low fidelity means one of the translations diverges.

**Discrimination** — `min_peer − top_confuser.score`. The top confuser is the best-scoring alias on any *other* way. A negative gap means another way outranks this way's own peers in embedding space — the stub is being dominated by a competitor.

## Workflow

```bash
# Run the audit on all ways
ways tune

# Single-way filter
ways tune --way delivery/commits

# Tune thresholds for flagging
ways tune --fidelity-threshold 0.55 --discrimination-threshold 0.05

# Machine-readable output
ways tune --json
```

## Interpreting Output

Each flagged entry looks like:

```
Way                          Lang  MinPeer MeanPeer     Gap Top confuser
softwaredev/delivery/commits ru     0.4688   0.6255 -0.1791 softwaredev/code/quality (0.648)
```

- `MinPeer 0.47` — tightest cross-lingual agreement with same-way peers. Low means translations disagree.
- `MeanPeer 0.63` — average peer agreement. Calibrates MinPeer against the typical spread.
- `Gap -0.18` — `code/quality` outranks this locale's own peers by 0.18 in cosine. The stub is being dominated.
- `Top confuser` — which way is winning; also why the user's query might mis-route.

## Three Failure Modes

Each flagged entry falls into one category. The fix differs per category.

### 1. Fidelity problem (low MinPeer, Gap OK)

One translation is different from the others. The stub doesn't carry the objective match words in local form.

**Fix:** re-author the offending locale. Look at the sibling stubs (other languages on the same way) to see what vocabulary they share; add local-language equivalents.

### 2. Discrimination problem (MinPeer OK, Gap negative)

Translations agree with each other, but another way's stub embeds closer to this locale's text than its own peers do.

**Fix options** depending on what the confuser is:

- **Confuser is a sibling way** (e.g., `delivery/commits` confused with `delivery/branching`) — the two ways are genuinely neighbors in meaning. Sharpen vocabulary on both so each occupies a distinct region.
- **Confuser is a parent/ancestor** (e.g., `delivery/commits` confused with `delivery`) — expected to some extent (parent has overlapping scope), but strong overlap means the child is too close to its parent's general description. Add child-specific terminology.
- **Confuser is unrelated** (e.g., `delivery/commits` outranked by `code/quality`) — the stub has accidental vocabulary overlap with something semantically far away. Often means a generic term (like "refactor") is doing too much work. Remove or specialize it.

### 3. Both problems (low MinPeer AND negative Gap)

The translation is both divergent AND outranked. Usually means the stub is just bad — short, generic, or a misunderstanding of the way's intent. Re-author from scratch using the English frontmatter and at least two good sibling translations as reference.

## Top Confusers Pattern

Broad-vocabulary ways tend to dominate their neighborhoods. When a domain-general way like `architecture` or `writing` or `environment/deps` keeps appearing as the confuser across many ways, the fix might be on *the confuser's* side: trim its vocabulary so it stops hoovering up traffic meant for specialized children.

## Output Signal, Not Verdict

The audit is diagnostic, not prescriptive. A flagged entry doesn't always mean "re-author now." Read the gap values:

- Gap > 0 — stub clearly wins, ignore any fidelity flag unless MinPeer is catastrophically low
- Gap between -0.05 and 0 — marginal, watch but not urgent
- Gap more negative than -0.10 — real problem, prioritize
- MinPeer < 0.40 — translation likely wrong regardless of discrimination

## See Also

- knowledge/optimization(meta) — broader vocabulary tuning and sparsity principles
- knowledge/authoring(meta) — way file format, coordinate-alias model
