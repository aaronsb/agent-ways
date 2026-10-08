# Region-first routing against flat competition

Follow-up to ADR-700, which left one question open: does choosing a region of the way tree first, then the way inside it, beat competing every way against every other at once? Measured 2026-10-05 on the 137 committed ways with the shipped MiniLM-L6 embedder and the alias corpus (`description + vocabulary`).

## Method

**Data.** The ADR-700 golden set: the golden sidecars (`ways author golden --tsv`) plus `tests/routing-golden.tsv`, 332 rows, 314 targeted and 18 `none`. Each prompt is embedded once; scores are cosine against the alias vectors, plus the hubness-corrected score (CSLS, k = 10, two-fold, the same folds as `geometry.py`).

**Tree.** Way ids are paths. The domain is the first segment; there are 10 domains. A node is any path prefix of a way id. Only 5 domains have a root way (`documentation`, `ea`, `data`, `research`, `writing`); `softwaredev` (68 ways) and `meta` (27) do not, and 35 ways have no parent way.

**Region scores.** `max` is the best way score in the region; `top3` is the mean of its best three (fewer in a smaller region); `root` is the region root way's own score where that way exists, otherwise `max`.

**Methods.**

- `flat`: argmax over all ways.
- `domain-first <agg>`: pick the domain with the highest region score, then argmax inside it.
- `top-k domains <agg>`: argmax over the union of the k best domains, k = 2 and 3.
- `descent <agg>`: from the roots, at each node the node's own way (if any) competes on its own score against each child subtree's region score. Stop at the node when its own score wins; otherwise descend into the winning child.
- `soft <agg> lam=λ`: way score + λ × its domain's region score, λ in 0.1, 0.25, 0.5.
- `oracle domain`: argmax inside the expected way's own domain. An upper bound on any domain-first method; not a method.

Every method yields a full ranking (regional methods rank the chosen region first, then the next), so MRR and recall@3 are defined. The none AUC compares the right way's score on targeted rows against the chosen way's score on `none` rows, in the score the method ranks by. Margin is rank 1 minus rank 2 in that score. Fixed and broken are counted against `cos: flat`, with a two-sided sign test. Each miss is split into "wrong domain" (rank 1 is outside the expected way's domain) and "wrong way in domain".

**A property of `max`.** The region holding the global argmax always has the highest region max, so every hard method aggregated by `max` returns the flat argmax as top-1, at every level of descent. The soft bonus with `max` cannot move the argmax either: the global best way scores s* + λ s*, and every other way scores at most s + λ s*. Those rows are included to confirm this empirically. Only aggregations other than `max` can change top-1.

## Results

### Main table

| method | top-1 | MRR | r@3 | none AUC | softwaredev (n=167) | others (n=147) | fixed / broken | sign p | median margin | misses: wrong domain / wrong way in domain |
|---|---|---|---|---|---|---|---|---|---|---|
| cos: flat | 0.656 | 0.742 | 0.809 | 0.947 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0.068 | 69 / 39 |
| cos: domain-first max | 0.656 | 0.704 | 0.729 | 0.947 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0.103 | 69 / 39 |
| cos: domain-first top3 | 0.618 | 0.675 | 0.710 | 0.951 | 0.731 | 0.490 | 9 / 21 | 0.04 | 0.081 | 78 / 42 |
| cos: domain-first root | 0.599 | 0.639 | 0.659 | 0.967 | 0.784 | 0.388 | 13 / 31 | 0.01 | 0.086 | 90 / 36 |
| cos: top-2 domains max | 0.656 | 0.737 | 0.796 | 0.947 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0.068 | 69 / 39 |
| cos: top-3 domains max | 0.656 | 0.741 | 0.809 | 0.947 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0.068 | 69 / 39 |
| cos: top-2 domains top3 | 0.643 | 0.726 | 0.787 | 0.948 | 0.737 | 0.537 | 1 / 5 | 0.22 | 0.068 | 67 / 45 |
| cos: top-3 domains top3 | 0.653 | 0.739 | 0.806 | 0.947 | 0.731 | 0.565 | 0 / 1 | 1.00 | 0.068 | 69 / 40 |
| cos: descent max | 0.656 | 0.692 | 0.704 | 0.947 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0.162 | 69 / 39 |
| cos: descent top3 | 0.510 | 0.585 | 0.640 | 0.956 | 0.575 | 0.435 | 13 / 59 | 0.00 | 0.068 | 78 / 76 |
| cos: descent root | 0.296 | 0.368 | 0.389 | 0.978 | 0.299 | 0.293 | 11 / 124 | 0.00 | 0.069 | 90 / 131 |
| cos: soft max lam=0.1 | 0.656 | 0.741 | 0.809 | 0.952 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0.071 | 69 / 39 |
| cos: soft max lam=0.25 | 0.656 | 0.741 | 0.812 | 0.957 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0.075 | 69 / 39 |
| cos: soft max lam=0.5 | 0.656 | 0.741 | 0.809 | 0.962 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0.079 | 69 / 39 |
| cos: soft top3 lam=0.1 | 0.659 | 0.742 | 0.809 | 0.953 | 0.737 | 0.571 | 1 / 0 | 1.00 | 0.069 | 68 / 39 |
| cos: soft top3 lam=0.25 | 0.656 | 0.740 | 0.806 | 0.958 | 0.743 | 0.558 | 2 / 2 | 1.00 | 0.069 | 68 / 40 |
| cos: soft top3 lam=0.5 | 0.653 | 0.738 | 0.799 | 0.964 | 0.743 | 0.551 | 4 / 5 | 1.00 | 0.073 | 67 / 42 |
| cos: oracle domain | 0.771 | 0.845 | 0.911 | 0.947 | 0.796 | 0.741 | 36 / 0 | 0.00 | 0.092 | 0 / 72 |
| csls: flat | 0.678 | 0.760 | 0.809 | 0.933 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0.156 | 62 / 39 |
| csls: domain-first max | 0.678 | 0.731 | 0.748 | 0.933 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0.220 | 62 / 39 |
| csls: domain-first top3 | 0.621 | 0.685 | 0.723 | 0.938 | 0.725 | 0.503 | 12 / 23 | 0.09 | 0.166 | 76 / 43 |
| csls: domain-first root | 0.611 | 0.655 | 0.669 | 0.953 | 0.766 | 0.435 | 21 / 35 | 0.08 | 0.179 | 87 / 35 |
| csls: top-2 domains max | 0.678 | 0.755 | 0.809 | 0.933 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0.156 | 62 / 39 |
| csls: top-3 domains max | 0.678 | 0.758 | 0.809 | 0.933 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0.156 | 62 / 39 |
| csls: top-2 domains top3 | 0.672 | 0.747 | 0.806 | 0.936 | 0.731 | 0.605 | 13 / 8 | 0.38 | 0.156 | 60 / 43 |
| csls: top-3 domains top3 | 0.678 | 0.758 | 0.809 | 0.933 | 0.737 | 0.612 | 14 / 7 | 0.19 | 0.156 | 61 / 40 |
| csls: descent max | 0.678 | 0.718 | 0.729 | 0.933 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0.311 | 62 / 39 |
| csls: descent top3 | 0.525 | 0.605 | 0.666 | 0.946 | 0.587 | 0.456 | 13 / 54 | 0.00 | 0.166 | 76 / 73 |
| csls: descent root | 0.306 | 0.374 | 0.379 | 0.970 | 0.269 | 0.347 | 15 / 125 | 0.00 | 0.133 | 87 / 131 |
| csls: soft max lam=0.1 | 0.678 | 0.760 | 0.809 | 0.939 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0.156 | 62 / 39 |
| csls: soft max lam=0.25 | 0.678 | 0.758 | 0.809 | 0.946 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0.159 | 62 / 39 |
| csls: soft max lam=0.5 | 0.678 | 0.758 | 0.809 | 0.953 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0.171 | 62 / 39 |
| csls: soft top3 lam=0.1 | 0.678 | 0.760 | 0.812 | 0.941 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0.156 | 61 / 40 |
| csls: soft top3 lam=0.25 | 0.682 | 0.760 | 0.809 | 0.948 | 0.737 | 0.619 | 14 / 6 | 0.12 | 0.156 | 60 / 40 |
| csls: soft top3 lam=0.5 | 0.678 | 0.757 | 0.803 | 0.956 | 0.737 | 0.612 | 14 / 7 | 0.19 | 0.156 | 60 / 41 |
| csls: oracle domain | 0.768 | 0.846 | 0.901 | 0.933 | 0.796 | 0.735 | 39 / 4 | 0.00 | 0.187 | 0 / 73 |

### Domain choice

How often each region scorer picks the expected way's domain, with log-sum-exp at several temperatures as a family between `max` and a sum:

| region scorer | domain right | top-1 after argmax in chosen domain |
|---|---|---|
| max | 0.780 | 0.656 |
| top3 | 0.752 | 0.618 |
| root | 0.713 | 0.599 |
| lse tau=0.01 | 0.777 | 0.653 |
| lse tau=0.02 | 0.780 | 0.656 |
| lse tau=0.05 | 0.755 | 0.631 |
| lse tau=0.1 | 0.675 | 0.548 |

### Corpus growth

Random subsets of the ways, 20 draws per size (seed 1, as in `geometry.py`), scored on targeted rows whose way survived. The tree is rebuilt on each subset. Cell: top-1 / median margin.

| ways | cos: flat | cos: domain-first top3 | cos: soft top3 lam=0.25 | cos: descent top3 | csls: flat | csls: soft top3 lam=0.25 |
|---|---|---|---|---|---|---|
| 34 | 0.785 / 0.121 | 0.731 / 0.141 | 0.788 / 0.128 | 0.659 / 0.154 | 0.788 / 0.250 | 0.792 / 0.263 |
| 68 | 0.729 / 0.095 | 0.676 / 0.107 | 0.724 / 0.098 | 0.585 / 0.113 | 0.745 / 0.199 | 0.747 / 0.207 |
| 102 | 0.684 / 0.077 | 0.639 / 0.089 | 0.684 / 0.080 | 0.536 / 0.090 | 0.701 / 0.170 | 0.703 / 0.174 |
| 137 | 0.656 / 0.068 | 0.618 / 0.081 | 0.656 / 0.069 | 0.510 / 0.068 | 0.678 / 0.156 | 0.682 / 0.156 |

## Reading the numbers

- **Region-first never beats flat.** Max-aggregated methods tie it exactly by construction (and lose MRR and recall@3 where the ranking turns lexicographic). Every non-max region score loses: domain-first `top3` 9 fixed / 21 broken (p = 0.04), domain-first `root` 13 / 31 (p = 0.01), descent `top3` 13 / 59, descent `root` 11 / 124. Top-2 and top-3 domains with `top3` lose slightly or tie. The best soft variant (`top3`, λ = 0.1) fixes one row.
- **Where the errors are.** Of the 108 flat misses, 69 put a way from the wrong domain at rank 1 and 39 picked the wrong way inside the right domain. The oracle shows the ceiling: with the domain known, top-1 rises to 0.771 (36 fixed, 0 broken), so 33 of the 69 cross-domain misses would still miss inside the right domain. No region scorer approaches the oracle, because the best one available (`max`, or `lse` at τ = 0.02) picks the right domain 78.0% of the time, which is exactly the rate the flat argmax already lands in the right domain, since the domain with the highest max is by definition the argmax's domain. Region choice is the same decision as way choice, made with less information.
- **Why the alternatives lose.** `root` favours domains without a root way, which fall back to `max`: it lifts `softwaredev` to 0.784 and drops the rest to 0.388. `top3` and soft temperatures above 0.02 reward a domain for having several moderately similar ways, which is crowding, the effect ADR-700 found degrades routing. Descent compounds the loss at every level.
- **Hubness correction is the only change that helps here**, as in ADR-700: `csls: flat` 0.678, 14 / 7 (p = 0.19). It cuts cross-domain misses from 69 to 62 and leaves in-domain misses at 39. Adding a soft domain bonus on top gives 0.682, 14 / 6, a one-row difference.
- **Separation.** Soft bonuses raise the none AUC slightly (0.947 to 0.962 at λ = 0.5 with `max`): a prompt with a whole region behind its top way scores higher than an off-topic prompt whose best match is isolated. Descent `root` reaches 0.978 while routing 30% of rows correctly, so the AUC rises here for the wrong reason, and any separation claim needs top-1 held fixed.
- **Margins.** Hard regional methods report larger median margins (descent `max` 0.162 against 0.068) with identical top-1. In their rankings rank 2 is often a weaker way from the same region, not the runner-up overall, so the larger margin does not mean more confident routing.
- **Growth slope.** From 34 to 137 ways flat cosine loses 12.9 points, hubness 11.0, the soft variant 13.2, domain-first `top3` 11.3 and descent `top3` 14.9. Domain-first `top3` trails flat throughout; its deficit narrows from 5.4 to 3.8 points, a difference of five or so rows at full size, which is inside the noise. Descent's deficit widens from 12.6 to 14.6. No region-first method flattens the slope enough to matter across this range.

## Limits

- The golden set is synthetic, model-written, and small: 314 targeted rows, 18 `none`. Differences of one or two points are noise; per-domain cells outside `softwaredev` are 147 rows across nine domains.
- The tree is the shipped path tree. Half the domains have no root way, and the two largest have none, so `root` mostly measures the fallback. A tree with authored root ways, or regions defined by authored edges (ADR-702), may behave differently.
- Region scores come from the same alias vectors as the way scores. A region embedded from its own text (a domain description, a centroid, or a trained classifier) is untested; the oracle bounds what it could gain, about 11 points.
- Subsampling is uniform over ways, as in `geometry.py`, so it measures crowding inside domains as well as domain count. Stratified subsampling (dropping whole domains) was not run.
- Ranking only. Thresholds, the parent-boost rule the engine ships (a fired parent lowers its children's bar), and body fusion combined with regions were not tested.

## Conclusion

On this set, choosing a region first does not beat flat competition, and no variant tested flattens the degradation as the corpus grows. With max-aggregated region scores the hierarchy is mathematically the same as the flat argmax, and every other aggregation (top-3 mean, root way score, softer log-sum-exp, tree descent) loses, four of them at p < 0.05, because the region decision has no information the flat ranking lacks and the alternatives reward crowded regions. The headroom is real: 69 of 108 misses land in the wrong domain, and a domain oracle would lift top-1 from 0.656 to 0.771. Reaching it needs a region signal independent of the way scores, which these experiments did not build; within the alias corpus, hubness correction remains the only change that helps (0.678, p = 0.19), and adding a soft domain bonus on top changes one row.

## Reproduce

```
OUT=/tmp/subtree-out python3 experiments/content-corpus/run.py <(ways author golden --tsv) tests/routing-golden.tsv
OUT=/tmp/subtree-out python3 experiments/content-corpus/subtree.py <(ways author golden --tsv) tests/routing-golden.tsv
```

`run.py` needs `bin/ways` (`make ways`) and `~/.claude/bin/way-embed` with the MiniLM model in the agent-ways cache.
