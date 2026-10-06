# Region signals independent of the way scores

Follow-up to ADR-700 §9 and `results-subtree.md`. That spike found that region-first routing never beats flat competition when the region score is derived from the way scores, while a domain oracle would lift top-1 from 0.656 to 0.771 (11.5 points; 69 of the 108 flat misses land in the wrong domain). This one asks whether a region signal built from its own vector, independent of the way scores, recovers part of that headroom. Measured 2026-10-05 on the 137 committed ways with the shipped MiniLM-L6 embedder, the same `way-embed` binary and model `run.py` uses.

## Method

**Data.** The ADR-700 golden set as before: `golden-synthetic.tsv` plus `tests/routing-golden.tsv`, 332 rows, 314 targeted and 18 `none`. Way scores are cosine against the alias vectors (`description + vocabulary`), and the hubness-corrected score (CSLS, k = 10, two-fold, the folds of `geometry.py`).

**Regions.** The 10 domains (first path segment) and 27 intermediate directories, 37 region nodes in all. `research` and `writing` are single-way domains; their region vector is that way's own.

**Region signals.** Each region node gets one vector; its region score for a prompt is the cosine against it.

- `desc`: the node's description, embedded alone. Resolution follows #664: the `description:` of the way in that directory. Six nodes have no way and take a line authored for this experiment, kept in `region-descriptions.tsv`: **`softwaredev`, `meta`, `itops`, `collaboration`, `workstation` and `meta/knowledge/authoring` are authored**, written from the descriptions of the ways they contain, without reading the golden prompts. The other 31 descriptions are given.
- `alias-c`: the normalised mean of the alias vectors of every way under the node.
- `body-c`: the normalised mean of the body section vectors (`run.py` `section` chunks) of every way under the node.
- `mean3`: the mean of the three region cosines.
- `<signal>+h`: the same signal with a label-free hubness correction at the region level (2 × cosine minus the region's mean cosine over its 10 closest prompts in the other fold). Added after `desc` turned out to pick `collaboration` for 72 of 314 targeted prompts.

No signal reads a label. The centroids are computed over all ways, which uses no labels.

**Methods.** Each runs on cosine and on CSLS way scores.

- `region-first`: rank domains by region score; the top domain's ways first, by way score, then the next domain's.
- `top-2`: the two best domains' ways first, by way score, then the rest in flat order.
- `soft λ`: way score + λ × its domain's region score, λ in 0.1, 0.25, 0.5.
- `path λ`: way score + λ × the mean region score of all the way's ancestor nodes, which brings in the intermediate directories. λ as above.
- `descent` (cosine only): from the root, children compete, a subtree by its region score, a leaf way or the node's own way by its way score. It mixes two score scales and is reported for completeness.

**Measures.** As in `results-subtree.md`: top-1, MRR, none AUC (right way's score on targeted rows against rank 1's score on `none` rows, in the score the method ranks by), top-1 for `softwaredev` and the other nine domains, fixed and broken against `cos: flat` and against the same base's flat (two-sided sign test), the share of rows whose rank-1 way is in the right domain, and misses split into wrong domain and wrong way in the right domain. Oracle recovery is (top-1 − 0.656) / (0.771 − 0.656).

## Results

### Region choice by the signal alone

How often the region signal's top domain is the expected way's domain, against the domain of the flat argmax. The last two columns measure independence: whether the signal is right where flat's domain is wrong, and wrong where flat's is right.

| signal | top region right | expected domain in top 2 | right where flat's domain is wrong (n=69) | wrong where flat's domain is right (n=245) |
|---|---|---|---|---|
| flat argmax's domain (cos) | 0.780 | | | |
| desc | 0.185 | 0.325 | 13 (0.188) | 200 (0.816) |
| alias-c | 0.576 | 0.739 | 16 (0.232) | 80 (0.327) |
| body-c | 0.525 | 0.701 | 16 (0.232) | 96 (0.392) |
| mean3 | 0.490 | 0.704 | 12 (0.174) | 103 (0.420) |
| desc+h | 0.204 | 0.360 | 11 (0.159) | 192 (0.784) |
| alias-c+h | 0.487 | 0.672 | 12 (0.174) | 104 (0.424) |
| body-c+h | 0.433 | 0.627 | 15 (0.217) | 124 (0.506) |
| mean3+h | 0.420 | 0.656 | 11 (0.159) | 124 (0.506) |

By expected domain (share of rows whose top region is the expected domain; `desc source` says whether the domain's description is a way's or authored):

| domain | n | desc source | flat | desc | alias-c | body-c | mean3 | desc+h | alias-c+h | body-c+h | mean3+h |
|---|---|---|---|---|---|---|---|---|---|---|---|
| softwaredev | 167 | authored | 0.87 | 0.04 | 0.57 | 0.50 | 0.44 | 0.07 | 0.43 | 0.34 | 0.32 |
| meta | 57 | authored | 0.63 | 0.16 | 0.47 | 0.30 | 0.33 | 0.16 | 0.49 | 0.30 | 0.30 |
| documentation | 29 | way | 0.59 | 0.34 | 0.45 | 0.41 | 0.41 | 0.34 | 0.28 | 0.34 | 0.38 |
| ea | 20 | way | 0.85 | 0.30 | 0.90 | 0.90 | 0.85 | 0.35 | 0.90 | 0.90 | 0.95 |
| data | 14 | way | 0.93 | 0.79 | 1.00 | 0.93 | 1.00 | 0.79 | 0.93 | 0.93 | 0.93 |
| workstation | 13 | authored | 0.77 | 0.69 | 0.85 | 0.85 | 0.85 | 0.54 | 0.85 | 0.85 | 0.85 |
| itops | 8 | authored | 0.50 | 0.50 | 0.25 | 0.75 | 0.50 | 0.62 | 0.25 | 0.75 | 0.50 |
| collaboration | 2 | authored | 1.00 | 1.00 | 0.50 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| research | 2 | way | 0.00 | 0.00 | 0.00 | 0.50 | 0.00 | 0.00 | 0.00 | 0.50 | 0.00 |
| writing | 2 | way | 0.00 | 0.50 | 0.00 | 0.50 | 0.50 | 0.50 | 0.00 | 0.50 | 0.50 |

### Main table

Condensed; every row the script prints is in the appendix.

| method | top-1 | MRR | none AUC | softwaredev (n=167) | others (n=147) | fixed / broken vs cos flat | p | fixed / broken vs own flat | p | rank-1 domain right | misses: wrong domain / wrong way in domain | oracle recovery |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cos: flat | 0.656 | 0.742 | 0.947 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0 / 0 | 1.00 | 0.780 | 69 / 39 | +0% |
| cos: oracle domain | 0.771 | 0.845 | 0.947 | 0.796 | 0.741 | 36 / 0 | 0.00 | 36 / 0 | 0.00 | 1.000 | 0 / 72 | +100% |
| cos: desc region-first | 0.166 | 0.248 | 0.989 | 0.036 | 0.313 | 10 / 164 | 0.00 | 10 / 164 | 0.00 | 0.185 | 256 / 6 | -428% |
| cos: desc top-2 | 0.258 | 0.357 | 0.985 | 0.144 | 0.388 | 9 / 134 | 0.00 | 9 / 134 | 0.00 | 0.315 | 215 / 18 | -347% |
| cos: desc soft λ=0.1 | 0.669 | 0.749 | 0.950 | 0.725 | 0.605 | 6 / 2 | 0.29 | 6 / 2 | 0.29 | 0.790 | 66 / 38 | +11% |
| cos: desc soft λ=0.25 | 0.650 | 0.737 | 0.954 | 0.707 | 0.585 | 6 / 8 | 0.79 | 6 / 8 | 0.79 | 0.774 | 71 / 39 | -6% |
| cos: desc soft λ=0.5 | 0.631 | 0.718 | 0.959 | 0.677 | 0.578 | 8 / 16 | 0.15 | 8 / 16 | 0.15 | 0.742 | 81 / 35 | -22% |
| cos: desc path λ=0.1 | 0.656 | 0.740 | 0.952 | 0.725 | 0.578 | 5 / 5 | 1.00 | 5 / 5 | 1.00 | 0.790 | 66 / 42 | +0% |
| cos: desc descent | 0.143 | 0.215 | 0.987 | 0.018 | 0.286 | 8 / 169 | 0.00 | 8 / 169 | 0.00 | 0.178 | 258 / 11 | -447% |
| cos: alias-c region-first | 0.459 | 0.533 | 0.963 | 0.455 | 0.463 | 9 / 71 | 0.00 | 9 / 71 | 0.00 | 0.576 | 133 / 37 | -172% |
| cos: alias-c top-2 | 0.532 | 0.610 | 0.961 | 0.551 | 0.510 | 2 / 41 | 0.00 | 2 / 41 | 0.00 | 0.672 | 103 / 44 | -108% |
| cos: alias-c soft λ=0.1 | 0.662 | 0.743 | 0.954 | 0.731 | 0.585 | 3 / 1 | 0.62 | 3 / 1 | 0.62 | 0.793 | 65 / 41 | +6% |
| cos: alias-c soft λ=0.25 | 0.662 | 0.743 | 0.959 | 0.719 | 0.599 | 5 / 3 | 0.73 | 5 / 3 | 0.73 | 0.793 | 65 / 41 | +6% |
| cos: alias-c soft λ=0.5 | 0.650 | 0.734 | 0.965 | 0.707 | 0.585 | 5 / 7 | 0.77 | 5 / 7 | 0.77 | 0.790 | 66 / 44 | -6% |
| cos: alias-c path λ=0.1 | 0.656 | 0.739 | 0.953 | 0.731 | 0.571 | 3 / 3 | 1.00 | 3 / 3 | 1.00 | 0.790 | 66 / 42 | +0% |
| cos: alias-c descent | 0.350 | 0.427 | 0.966 | 0.287 | 0.422 | 9 / 105 | 0.00 | 9 / 105 | 0.00 | 0.576 | 133 / 71 | -267% |
| cos: body-c region-first | 0.414 | 0.493 | 0.967 | 0.413 | 0.415 | 10 / 86 | 0.00 | 10 / 86 | 0.00 | 0.525 | 149 / 35 | -211% |
| cos: body-c top-2 | 0.510 | 0.585 | 0.961 | 0.521 | 0.497 | 8 / 54 | 0.00 | 8 / 54 | 0.00 | 0.650 | 110 / 44 | -128% |
| cos: body-c soft λ=0.1 | 0.662 | 0.745 | 0.957 | 0.731 | 0.585 | 2 / 0 | 0.50 | 2 / 0 | 0.50 | 0.790 | 66 / 40 | +6% |
| cos: body-c soft λ=0.25 | 0.662 | 0.745 | 0.968 | 0.731 | 0.585 | 5 / 3 | 0.73 | 5 / 3 | 0.73 | 0.799 | 63 / 43 | +6% |
| cos: body-c soft λ=0.5 | 0.650 | 0.736 | 0.975 | 0.719 | 0.571 | 8 / 10 | 0.81 | 8 / 10 | 0.81 | 0.787 | 67 / 43 | -6% |
| cos: body-c path λ=0.1 | 0.656 | 0.741 | 0.957 | 0.731 | 0.571 | 3 / 3 | 1.00 | 3 / 3 | 1.00 | 0.787 | 67 / 41 | +0% |
| cos: body-c descent | 0.322 | 0.403 | 0.968 | 0.257 | 0.395 | 10 / 115 | 0.00 | 10 / 115 | 0.00 | 0.529 | 148 / 65 | -292% |
| cos: mean3 region-first | 0.414 | 0.491 | 0.967 | 0.383 | 0.449 | 8 / 84 | 0.00 | 8 / 84 | 0.00 | 0.490 | 160 / 24 | -211% |
| cos: mean3 top-2 | 0.522 | 0.596 | 0.961 | 0.515 | 0.531 | 5 / 47 | 0.00 | 5 / 47 | 0.00 | 0.643 | 112 / 38 | -117% |
| cos: mean3 soft λ=0.1 | 0.669 | 0.748 | 0.954 | 0.731 | 0.599 | 5 / 1 | 0.22 | 5 / 1 | 0.22 | 0.796 | 64 / 40 | +11% |
| cos: mean3 soft λ=0.25 | 0.656 | 0.740 | 0.961 | 0.719 | 0.585 | 6 / 6 | 1.00 | 6 / 6 | 1.00 | 0.787 | 67 / 41 | +0% |
| cos: mean3 soft λ=0.5 | 0.643 | 0.731 | 0.969 | 0.695 | 0.585 | 6 / 10 | 0.45 | 6 / 10 | 0.45 | 0.780 | 69 / 43 | -11% |
| cos: mean3 path λ=0.1 | 0.659 | 0.743 | 0.954 | 0.731 | 0.578 | 4 / 3 | 1.00 | 4 / 3 | 1.00 | 0.793 | 65 / 42 | +3% |
| cos: mean3 descent | 0.309 | 0.390 | 0.967 | 0.216 | 0.415 | 8 / 117 | 0.00 | 8 / 117 | 0.00 | 0.490 | 160 / 57 | -303% |
| cos: desc+h region-first | 0.188 | 0.266 | 0.990 | 0.072 | 0.320 | 10 / 157 | 0.00 | 10 / 157 | 0.00 | 0.204 | 250 / 5 | -408% |
| cos: alias-c+h region-first | 0.389 | 0.478 | 0.972 | 0.347 | 0.435 | 7 / 91 | 0.00 | 7 / 91 | 0.00 | 0.487 | 161 / 31 | -233% |
| cos: alias-c+h top-2 | 0.487 | 0.576 | 0.964 | 0.473 | 0.503 | 6 / 59 | 0.00 | 6 / 59 | 0.00 | 0.618 | 120 / 41 | -147% |
| cos: mean3+h soft λ=0.1 | 0.669 | 0.747 | 0.959 | 0.725 | 0.605 | 6 / 2 | 0.29 | 6 / 2 | 0.29 | 0.796 | 64 / 40 | +11% |
| cos: body-c+h soft λ=0.1 | 0.666 | 0.746 | 0.964 | 0.725 | 0.599 | 6 / 3 | 0.51 | 6 / 3 | 0.51 | 0.796 | 64 / 41 | +8% |
| csls: flat | 0.678 | 0.760 | 0.933 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0 / 0 | 1.00 | 0.803 | 62 / 39 | +19% |
| csls: oracle domain | 0.768 | 0.846 | 0.933 | 0.796 | 0.735 | 39 / 4 | 0.00 | 28 / 0 | 0.00 | 1.000 | 0 / 73 | +97% |
| csls: desc region-first | 0.166 | 0.249 | 0.987 | 0.036 | 0.313 | 10 / 164 | 0.00 | 6 / 167 | 0.00 | 0.185 | 256 / 6 | -428% |
| csls: desc top-2 | 0.252 | 0.357 | 0.984 | 0.138 | 0.381 | 9 / 136 | 0.00 | 5 / 139 | 0.00 | 0.312 | 216 / 19 | -353% |
| csls: desc soft λ=0.1 | 0.669 | 0.754 | 0.936 | 0.713 | 0.619 | 12 / 8 | 0.50 | 0 / 3 | 0.25 | 0.793 | 65 / 39 | +11% |
| csls: desc path λ=0.1 | 0.666 | 0.754 | 0.937 | 0.707 | 0.619 | 11 / 8 | 0.65 | 1 / 5 | 0.22 | 0.799 | 63 / 42 | +8% |
| csls: alias-c region-first | 0.465 | 0.537 | 0.961 | 0.473 | 0.456 | 13 / 73 | 0.00 | 5 / 72 | 0.00 | 0.576 | 133 / 35 | -167% |
| csls: alias-c top-2 | 0.551 | 0.623 | 0.959 | 0.563 | 0.537 | 11 / 44 | 0.00 | 3 / 43 | 0.00 | 0.682 | 100 / 41 | -92% |
| csls: alias-c soft λ=0.1 | 0.672 | 0.756 | 0.939 | 0.719 | 0.619 | 12 / 7 | 0.36 | 0 / 2 | 0.50 | 0.799 | 63 / 40 | +14% |
| csls: alias-c path λ=0.1 | 0.672 | 0.755 | 0.938 | 0.719 | 0.619 | 12 / 7 | 0.36 | 1 / 3 | 0.62 | 0.803 | 62 / 41 | +14% |
| csls: body-c region-first | 0.417 | 0.498 | 0.967 | 0.425 | 0.408 | 12 / 87 | 0.00 | 6 / 88 | 0.00 | 0.525 | 149 / 34 | -208% |
| csls: body-c top-2 | 0.516 | 0.593 | 0.958 | 0.527 | 0.503 | 12 / 56 | 0.00 | 4 / 55 | 0.00 | 0.646 | 111 / 41 | -122% |
| csls: body-c soft λ=0.1 | 0.675 | 0.758 | 0.941 | 0.725 | 0.619 | 13 / 7 | 0.26 | 0 / 1 | 1.00 | 0.803 | 62 / 40 | +17% |
| csls: body-c path λ=0.1 | 0.675 | 0.758 | 0.941 | 0.725 | 0.619 | 13 / 7 | 0.26 | 1 / 2 | 1.00 | 0.806 | 61 / 41 | +17% |
| csls: mean3 region-first | 0.411 | 0.491 | 0.965 | 0.383 | 0.442 | 9 / 86 | 0.00 | 4 / 88 | 0.00 | 0.490 | 160 / 25 | -214% |
| csls: mean3 top-2 | 0.525 | 0.600 | 0.958 | 0.515 | 0.537 | 10 / 51 | 0.00 | 3 / 51 | 0.00 | 0.640 | 113 / 36 | -114% |
| csls: mean3 soft λ=0.1 | 0.672 | 0.756 | 0.939 | 0.719 | 0.619 | 12 / 7 | 0.36 | 0 / 2 | 0.50 | 0.799 | 63 / 40 | +14% |
| csls: mean3 path λ=0.1 | 0.675 | 0.757 | 0.939 | 0.725 | 0.619 | 13 / 7 | 0.26 | 1 / 2 | 1.00 | 0.806 | 61 / 41 | +17% |

### Corpus growth for the best method

The best method on each base, by top-1 at full size, over the same random subsets as `geometry.py` and `subtree.py` (20 draws per size, seed 1), scored on targeted rows whose way survived. Centroids are recomputed on each subset; descriptions stay fixed, since a directory's description does not depend on which ways are present. Cell: top-1 / median margin.

| ways | cos: flat | cos: desc soft λ=0.1 | csls: flat | csls: body-c soft λ=0.1 | cos: oracle domain |
|---|---|---|---|---|---|
| 34 | 0.785 / 0.121 | 0.789 / 0.123 | 0.788 / 0.250 | 0.787 / 0.252 | 0.886 / 0.163 |
| 68 | 0.729 / 0.095 | 0.736 / 0.094 | 0.745 / 0.199 | 0.743 / 0.201 | 0.832 / 0.128 |
| 102 | 0.684 / 0.077 | 0.692 / 0.078 | 0.701 / 0.170 | 0.699 / 0.171 | 0.798 / 0.108 |
| 137 | 0.656 / 0.068 | 0.669 / 0.066 | 0.678 / 0.156 | 0.675 / 0.156 | 0.771 / 0.092 |

## Reading the numbers

- **Every independent region signal chooses the domain worse than the flat argmax already does.** The flat argmax lands in the right domain on 78.0% of targeted rows. The best independent signal, the alias centroid, picks the right domain on 57.6%; body centroids 52.5%, the three combined 49.0%, descriptions 18.5%. A region-level hubness correction lowers every centroid signal further (42 to 49%) and lifts descriptions only to 20.4%.
- **The signals are partly independent, but not where it helps.** On the 69 rows where flat picks the wrong domain, the signals pick the right one 11 to 16 times (16 to 23%). On the 245 rows where flat's domain is right, they pick a wrong domain 80 to 200 times. Any hard use of them loses many more rows than it gains.
- **Hard region choice loses heavily.** Region-first with the best signal reaches 0.459 (alias centroid, 9 fixed / 71 broken), top-2 reaches 0.532 on cosine and 0.551 on CSLS (11 / 44). Every region-first, top-2 and descent row is worse than flat at p < 0.01. Descent is worst (0.10 to 0.35).
- **A soft bonus at λ = 0.1 is the only thing that moves top-1 up, and not significantly.** The best are `desc soft λ=0.1` (0.669, 6 fixed / 2 broken, p = 0.29) and `mean3 soft λ=0.1` (0.669, 5 / 1, p = 0.22): four net rows, 11% of the oracle's 11.5 points. Per signal on cosine, the best row recovers 11% for descriptions, 6% for alias centroids, 6% for body centroids, 11% for the combination, and 0 to 11% for the hubness-corrected variants. At λ = 0.25 every signal is within two rows of flat or below it, and at λ = 0.5 every one is below it.
- **On hubness-corrected way scores nothing helps.** `csls: flat` is 0.678 (19% of the oracle's headroom on its own). The best soft row on CSLS is 0.675 (body centroid, λ = 0.1), one row below `csls: flat`, and every larger λ loses rows against it; at λ = 0.5 the losses reach p = 0.02 to 0.03 for descriptions, alias centroids and body centroids. Whatever the soft bonus fixes on cosine, CSLS already fixes.
- **Where the gain lands.** The best soft rows lift the other nine domains (0.571 to 0.605 for descriptions, 0.599 for the combination) and leave `softwaredev` flat or one row lower. Wrong-domain misses fall from 69 to 64 to 66; wrong-way-in-domain misses stay at 38 to 40.
- **Intermediate directories add nothing.** The `path` bonus, which averages the scores of every ancestor node, never beats the domain-only `soft` bonus at the same λ on cosine, and is within one row of it on CSLS; its best cosine row is `mean3+h path λ=0.1` at 0.662 (+2 rows). Deeper nodes pull their ways toward whichever directory sits near the prompt, which also disturbs choices inside the right domain: in-domain misses rise from 39 to 42 to 57 at λ = 0.25 and 0.5.
- **Descriptions fail at the domain level for a structural reason.** A one- or two-line description of a 68-way domain is too broad to win: the authored `softwaredev` line is the top domain for only 8 of 314 prompts and for 4% of the 167 `softwaredev` rows, while the authored `collaboration` line (one way about sharing an onboarding guide with teammates) is the top domain for 72 prompts. It is not only the authored lines: the given root-way descriptions of `documentation` and `ea` pick their own domain for 34% and 30% of their rows, against 59% and 85% for the flat argmax. Region-level hubness correction does not repair it (20.4%).
- **Separation.** Soft bonuses raise the none AUC (body centroid at λ = 0.5: 0.975 against 0.947) while top-1 drops, and hard methods reach 0.99 while routing under 30% correctly. As in §9, AUC rises here for the wrong reason, and a separation claim needs top-1 held fixed. At λ = 0.1, where top-1 holds, the gain is small (0.950 to 0.964).
- **Growth.** `desc soft λ=0.1` runs 0.4 to 1.3 points above flat cosine at every size and loses 12.0 points from 34 to 137 ways, against 12.9 for flat. On CSLS, the best soft row tracks `csls: flat` within 0.3 points throughout. No signal changes the slope.

## Limits

- The golden set is synthetic, model-written and small: 314 targeted rows, 18 `none`. A four-row difference is noise. Domains other than `softwaredev` and `meta` have 29 rows or fewer, and four of them have 2 to 8 rows.
- Six of the 37 descriptions were authored for this experiment, including the two largest domains. A differently worded `softwaredev` or `collaboration` line would change the `desc` rows. The given descriptions, which nobody wrote for region routing, fail in the same direction, so the conclusion does not rest on the authored text.
- A region vector here is a single point. A domain of 68 ways is not well described by one centroid or one sentence; multi-vector regions (max over a region's ways is the §9 `max` region score, which equals flat), a trained domain classifier, or a reranker reading the described route were not tested.
- λ was swept over three values on the evaluation set without a held-out split. The best row was chosen on the same rows it is reported on, so its 11% is an optimistic estimate.
- Ranking only. Thresholds, the engine's parent-boost rule, and body-score fusion combined with region bonuses were not tested. The relevance-gate use of node descriptions that #664 proposes (input version C for the yes/no probe) is a different question and was not measured here.

## Conclusion

On this set, no region signal independent of the way scores recovers a meaningful part of the oracle's 11.5 points: domain descriptions, alias centroids, body centroids and their combination all pick the right domain less often (18.5 to 57.6%) than the flat argmax already does (78.0%), so every hard region-first, top-2 or descent method loses at p < 0.01. A small soft bonus (λ = 0.1) gains four net rows on cosine, 11% of the headroom at p = 0.22 to 0.29, and nothing on hubness-corrected scores, where `csls: flat` alone recovers 19% and every region bonus is level or worse. The headroom the oracle shows is real, but a single vector per region, whether written or averaged, does not carry the information to claim it, and none of these signals changes how top-1 falls as the corpus grows.

## Reproduce

```
OUT=/tmp/region-out python3 experiments/content-corpus/run.py experiments/content-corpus/golden-synthetic.tsv tests/routing-golden.tsv
OUT=/tmp/region-out python3 experiments/content-corpus/region.py experiments/content-corpus/golden-synthetic.tsv tests/routing-golden.tsv
```

`run.py` needs `bin/ways` (`make ways`) and `~/.claude/bin/way-embed` with the MiniLM model in the agent-ways cache. `region.py` reads the alias and `section` corpora from `$OUT` and the authored descriptions from `region-descriptions.tsv`.

## Appendix: every method

<details>
<summary>Full table, 140 methods</summary>

| method | top-1 | MRR | none AUC | softwaredev (n=167) | others (n=147) | fixed / broken vs cos flat | p | fixed / broken vs own flat | p | rank-1 domain right | misses: wrong domain / wrong way in domain | oracle recovery |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cos: flat | 0.656 | 0.742 | 0.947 | 0.731 | 0.571 | 0 / 0 | 1.00 | 0 / 0 | 1.00 | 0.780 | 69 / 39 | +0% |
| cos: oracle domain | 0.771 | 0.845 | 0.947 | 0.796 | 0.741 | 36 / 0 | 0.00 | 36 / 0 | 0.00 | 1.000 | 0 / 72 | +100% |
| cos: desc region-first | 0.166 | 0.248 | 0.989 | 0.036 | 0.313 | 10 / 164 | 0.00 | 10 / 164 | 0.00 | 0.185 | 256 / 6 | -428% |
| cos: desc top-2 | 0.258 | 0.357 | 0.985 | 0.144 | 0.388 | 9 / 134 | 0.00 | 9 / 134 | 0.00 | 0.315 | 215 / 18 | -347% |
| cos: desc soft λ=0.1 | 0.669 | 0.749 | 0.950 | 0.725 | 0.605 | 6 / 2 | 0.29 | 6 / 2 | 0.29 | 0.790 | 66 / 38 | +11% |
| cos: desc soft λ=0.25 | 0.650 | 0.737 | 0.954 | 0.707 | 0.585 | 6 / 8 | 0.79 | 6 / 8 | 0.79 | 0.774 | 71 / 39 | -6% |
| cos: desc soft λ=0.5 | 0.631 | 0.718 | 0.959 | 0.677 | 0.578 | 8 / 16 | 0.15 | 8 / 16 | 0.15 | 0.742 | 81 / 35 | -22% |
| cos: desc path λ=0.1 | 0.656 | 0.740 | 0.952 | 0.725 | 0.578 | 5 / 5 | 1.00 | 5 / 5 | 1.00 | 0.790 | 66 / 42 | +0% |
| cos: desc path λ=0.25 | 0.637 | 0.729 | 0.957 | 0.695 | 0.571 | 9 / 15 | 0.31 | 9 / 15 | 0.31 | 0.783 | 68 / 46 | -17% |
| cos: desc path λ=0.5 | 0.592 | 0.700 | 0.965 | 0.653 | 0.524 | 9 / 29 | 0.00 | 9 / 29 | 0.00 | 0.761 | 75 / 53 | -56% |
| cos: desc descent | 0.143 | 0.215 | 0.987 | 0.018 | 0.286 | 8 / 169 | 0.00 | 8 / 169 | 0.00 | 0.178 | 258 / 11 | -447% |
| cos: alias-c region-first | 0.459 | 0.533 | 0.963 | 0.455 | 0.463 | 9 / 71 | 0.00 | 9 / 71 | 0.00 | 0.576 | 133 / 37 | -172% |
| cos: alias-c top-2 | 0.532 | 0.610 | 0.961 | 0.551 | 0.510 | 2 / 41 | 0.00 | 2 / 41 | 0.00 | 0.672 | 103 / 44 | -108% |
| cos: alias-c soft λ=0.1 | 0.662 | 0.743 | 0.954 | 0.731 | 0.585 | 3 / 1 | 0.62 | 3 / 1 | 0.62 | 0.793 | 65 / 41 | +6% |
| cos: alias-c soft λ=0.25 | 0.662 | 0.743 | 0.959 | 0.719 | 0.599 | 5 / 3 | 0.73 | 5 / 3 | 0.73 | 0.793 | 65 / 41 | +6% |
| cos: alias-c soft λ=0.5 | 0.650 | 0.734 | 0.965 | 0.707 | 0.585 | 5 / 7 | 0.77 | 5 / 7 | 0.77 | 0.790 | 66 / 44 | -6% |
| cos: alias-c path λ=0.1 | 0.656 | 0.739 | 0.953 | 0.731 | 0.571 | 3 / 3 | 1.00 | 3 / 3 | 1.00 | 0.790 | 66 / 42 | +0% |
| cos: alias-c path λ=0.25 | 0.650 | 0.734 | 0.957 | 0.719 | 0.571 | 6 / 8 | 0.79 | 6 / 8 | 0.79 | 0.790 | 66 / 44 | -6% |
| cos: alias-c path λ=0.5 | 0.624 | 0.716 | 0.961 | 0.695 | 0.544 | 7 / 17 | 0.06 | 7 / 17 | 0.06 | 0.790 | 66 / 52 | -28% |
| cos: alias-c descent | 0.350 | 0.427 | 0.966 | 0.287 | 0.422 | 9 / 105 | 0.00 | 9 / 105 | 0.00 | 0.576 | 133 / 71 | -267% |
| cos: body-c region-first | 0.414 | 0.493 | 0.967 | 0.413 | 0.415 | 10 / 86 | 0.00 | 10 / 86 | 0.00 | 0.525 | 149 / 35 | -211% |
| cos: body-c top-2 | 0.510 | 0.585 | 0.961 | 0.521 | 0.497 | 8 / 54 | 0.00 | 8 / 54 | 0.00 | 0.650 | 110 / 44 | -128% |
| cos: body-c soft λ=0.1 | 0.662 | 0.745 | 0.957 | 0.731 | 0.585 | 2 / 0 | 0.50 | 2 / 0 | 0.50 | 0.790 | 66 / 40 | +6% |
| cos: body-c soft λ=0.25 | 0.662 | 0.745 | 0.968 | 0.731 | 0.585 | 5 / 3 | 0.73 | 5 / 3 | 0.73 | 0.799 | 63 / 43 | +6% |
| cos: body-c soft λ=0.5 | 0.650 | 0.736 | 0.975 | 0.719 | 0.571 | 8 / 10 | 0.81 | 8 / 10 | 0.81 | 0.787 | 67 / 43 | -6% |
| cos: body-c path λ=0.1 | 0.656 | 0.741 | 0.957 | 0.731 | 0.571 | 3 / 3 | 1.00 | 3 / 3 | 1.00 | 0.787 | 67 / 41 | +0% |
| cos: body-c path λ=0.25 | 0.653 | 0.737 | 0.966 | 0.725 | 0.571 | 6 / 7 | 1.00 | 6 / 7 | 1.00 | 0.796 | 64 / 45 | -3% |
| cos: body-c path λ=0.5 | 0.624 | 0.719 | 0.972 | 0.695 | 0.544 | 8 / 18 | 0.08 | 8 / 18 | 0.08 | 0.780 | 69 / 49 | -28% |
| cos: body-c descent | 0.322 | 0.403 | 0.968 | 0.257 | 0.395 | 10 / 115 | 0.00 | 10 / 115 | 0.00 | 0.529 | 148 / 65 | -292% |
| cos: mean3 region-first | 0.414 | 0.491 | 0.967 | 0.383 | 0.449 | 8 / 84 | 0.00 | 8 / 84 | 0.00 | 0.490 | 160 / 24 | -211% |
| cos: mean3 top-2 | 0.522 | 0.596 | 0.961 | 0.515 | 0.531 | 5 / 47 | 0.00 | 5 / 47 | 0.00 | 0.643 | 112 / 38 | -117% |
| cos: mean3 soft λ=0.1 | 0.669 | 0.748 | 0.954 | 0.731 | 0.599 | 5 / 1 | 0.22 | 5 / 1 | 0.22 | 0.796 | 64 / 40 | +11% |
| cos: mean3 soft λ=0.25 | 0.656 | 0.740 | 0.961 | 0.719 | 0.585 | 6 / 6 | 1.00 | 6 / 6 | 1.00 | 0.787 | 67 / 41 | +0% |
| cos: mean3 soft λ=0.5 | 0.643 | 0.731 | 0.969 | 0.695 | 0.585 | 6 / 10 | 0.45 | 6 / 10 | 0.45 | 0.780 | 69 / 43 | -11% |
| cos: mean3 path λ=0.1 | 0.659 | 0.743 | 0.954 | 0.731 | 0.578 | 4 / 3 | 1.00 | 4 / 3 | 1.00 | 0.793 | 65 / 42 | +3% |
| cos: mean3 path λ=0.25 | 0.650 | 0.735 | 0.961 | 0.719 | 0.571 | 7 / 9 | 0.80 | 7 / 9 | 0.80 | 0.793 | 65 / 45 | -6% |
| cos: mean3 path λ=0.5 | 0.631 | 0.721 | 0.967 | 0.689 | 0.565 | 9 / 17 | 0.17 | 9 / 17 | 0.17 | 0.796 | 64 / 52 | -22% |
| cos: mean3 descent | 0.309 | 0.390 | 0.967 | 0.216 | 0.415 | 8 / 117 | 0.00 | 8 / 117 | 0.00 | 0.490 | 160 / 57 | -303% |
| cos: desc+h region-first | 0.188 | 0.266 | 0.990 | 0.072 | 0.320 | 10 / 157 | 0.00 | 10 / 157 | 0.00 | 0.204 | 250 / 5 | -408% |
| cos: desc+h top-2 | 0.287 | 0.377 | 0.978 | 0.210 | 0.374 | 8 / 124 | 0.00 | 8 / 124 | 0.00 | 0.347 | 205 / 19 | -322% |
| cos: desc+h soft λ=0.1 | 0.656 | 0.741 | 0.954 | 0.713 | 0.592 | 6 / 6 | 1.00 | 6 / 6 | 1.00 | 0.771 | 72 / 36 | +0% |
| cos: desc+h soft λ=0.25 | 0.624 | 0.716 | 0.960 | 0.683 | 0.558 | 7 / 17 | 0.06 | 7 / 17 | 0.06 | 0.742 | 81 / 37 | -28% |
| cos: desc+h soft λ=0.5 | 0.576 | 0.671 | 0.954 | 0.635 | 0.510 | 8 / 33 | 0.00 | 8 / 33 | 0.00 | 0.675 | 102 / 31 | -69% |
| cos: desc+h path λ=0.1 | 0.643 | 0.735 | 0.956 | 0.707 | 0.571 | 8 / 12 | 0.50 | 8 / 12 | 0.50 | 0.787 | 67 / 45 | -11% |
| cos: desc+h path λ=0.25 | 0.583 | 0.697 | 0.964 | 0.641 | 0.517 | 9 / 32 | 0.00 | 9 / 32 | 0.00 | 0.761 | 75 / 56 | -64% |
| cos: desc+h path λ=0.5 | 0.529 | 0.646 | 0.961 | 0.581 | 0.469 | 10 / 50 | 0.00 | 10 / 50 | 0.00 | 0.710 | 91 / 57 | -111% |
| cos: desc+h descent | 0.099 | 0.174 | 0.995 | 0.012 | 0.197 | 9 / 184 | 0.00 | 9 / 184 | 0.00 | 0.131 | 273 / 10 | -486% |
| cos: alias-c+h region-first | 0.389 | 0.478 | 0.972 | 0.347 | 0.435 | 7 / 91 | 0.00 | 7 / 91 | 0.00 | 0.487 | 161 / 31 | -233% |
| cos: alias-c+h top-2 | 0.487 | 0.576 | 0.964 | 0.473 | 0.503 | 6 / 59 | 0.00 | 6 / 59 | 0.00 | 0.618 | 120 / 41 | -147% |
| cos: alias-c+h soft λ=0.1 | 0.662 | 0.744 | 0.958 | 0.719 | 0.599 | 5 / 3 | 0.73 | 5 / 3 | 0.73 | 0.787 | 67 / 39 | +6% |
| cos: alias-c+h soft λ=0.25 | 0.643 | 0.731 | 0.964 | 0.689 | 0.592 | 7 / 11 | 0.48 | 7 / 11 | 0.48 | 0.774 | 71 / 41 | -11% |
| cos: alias-c+h soft λ=0.5 | 0.605 | 0.701 | 0.961 | 0.653 | 0.551 | 7 / 23 | 0.01 | 7 / 23 | 0.01 | 0.736 | 83 / 41 | -44% |
| cos: alias-c+h path λ=0.1 | 0.659 | 0.741 | 0.957 | 0.719 | 0.592 | 7 / 6 | 1.00 | 7 / 6 | 1.00 | 0.799 | 63 / 44 | +3% |
| cos: alias-c+h path λ=0.25 | 0.631 | 0.721 | 0.961 | 0.683 | 0.571 | 11 / 19 | 0.20 | 11 / 19 | 0.20 | 0.790 | 66 / 50 | -22% |
| cos: alias-c+h path λ=0.5 | 0.583 | 0.681 | 0.958 | 0.629 | 0.531 | 11 / 34 | 0.00 | 11 / 34 | 0.00 | 0.752 | 78 / 53 | -64% |
| cos: alias-c+h descent | 0.242 | 0.329 | 0.983 | 0.138 | 0.361 | 7 / 137 | 0.00 | 7 / 137 | 0.00 | 0.395 | 190 / 48 | -361% |
| cos: body-c+h region-first | 0.338 | 0.426 | 0.968 | 0.275 | 0.408 | 9 / 109 | 0.00 | 9 / 109 | 0.00 | 0.433 | 178 / 30 | -278% |
| cos: body-c+h top-2 | 0.446 | 0.530 | 0.964 | 0.425 | 0.469 | 9 / 75 | 0.00 | 9 / 75 | 0.00 | 0.583 | 131 / 43 | -183% |
| cos: body-c+h soft λ=0.1 | 0.666 | 0.746 | 0.964 | 0.725 | 0.599 | 6 / 3 | 0.51 | 6 / 3 | 0.51 | 0.796 | 64 / 41 | +8% |
| cos: body-c+h soft λ=0.25 | 0.646 | 0.735 | 0.974 | 0.707 | 0.578 | 7 / 10 | 0.63 | 7 / 10 | 0.63 | 0.783 | 68 / 43 | -8% |
| cos: body-c+h soft λ=0.5 | 0.605 | 0.702 | 0.976 | 0.659 | 0.544 | 7 / 23 | 0.01 | 7 / 23 | 0.01 | 0.736 | 83 / 41 | -44% |
| cos: body-c+h path λ=0.1 | 0.659 | 0.742 | 0.964 | 0.719 | 0.592 | 7 / 6 | 1.00 | 7 / 6 | 1.00 | 0.796 | 64 / 43 | +3% |
| cos: body-c+h path λ=0.25 | 0.621 | 0.720 | 0.972 | 0.689 | 0.544 | 9 / 20 | 0.06 | 9 / 20 | 0.06 | 0.783 | 68 / 51 | -31% |
| cos: body-c+h path λ=0.5 | 0.599 | 0.695 | 0.972 | 0.677 | 0.510 | 8 / 26 | 0.00 | 8 / 26 | 0.00 | 0.761 | 75 / 51 | -50% |
| cos: body-c+h descent | 0.207 | 0.303 | 0.995 | 0.120 | 0.306 | 7 / 148 | 0.00 | 7 / 148 | 0.00 | 0.373 | 197 / 52 | -392% |
| cos: mean3+h region-first | 0.357 | 0.442 | 0.971 | 0.281 | 0.442 | 9 / 103 | 0.00 | 9 / 103 | 0.00 | 0.420 | 182 / 20 | -261% |
| cos: mean3+h top-2 | 0.494 | 0.573 | 0.962 | 0.455 | 0.537 | 7 / 58 | 0.00 | 7 / 58 | 0.00 | 0.602 | 125 / 34 | -142% |
| cos: mean3+h soft λ=0.1 | 0.669 | 0.747 | 0.959 | 0.725 | 0.605 | 6 / 2 | 0.29 | 6 / 2 | 0.29 | 0.796 | 64 / 40 | +11% |
| cos: mean3+h soft λ=0.25 | 0.640 | 0.730 | 0.968 | 0.683 | 0.592 | 7 / 12 | 0.36 | 7 / 12 | 0.36 | 0.774 | 71 / 42 | -14% |
| cos: mean3+h soft λ=0.5 | 0.605 | 0.697 | 0.971 | 0.653 | 0.551 | 7 / 23 | 0.01 | 7 / 23 | 0.01 | 0.726 | 86 / 38 | -44% |
| cos: mean3+h path λ=0.1 | 0.662 | 0.743 | 0.959 | 0.719 | 0.599 | 8 / 6 | 0.79 | 8 / 6 | 0.79 | 0.803 | 62 / 44 | +6% |
| cos: mean3+h path λ=0.25 | 0.634 | 0.725 | 0.968 | 0.689 | 0.571 | 9 / 16 | 0.23 | 9 / 16 | 0.23 | 0.793 | 65 / 50 | -19% |
| cos: mean3+h path λ=0.5 | 0.586 | 0.683 | 0.971 | 0.629 | 0.537 | 12 / 34 | 0.00 | 12 / 34 | 0.00 | 0.755 | 77 / 53 | -61% |
| cos: mean3+h descent | 0.197 | 0.297 | 0.995 | 0.078 | 0.333 | 6 / 150 | 0.00 | 6 / 150 | 0.00 | 0.318 | 214 / 38 | -400% |
| csls: flat | 0.678 | 0.760 | 0.933 | 0.731 | 0.619 | 14 / 7 | 0.19 | 0 / 0 | 1.00 | 0.803 | 62 / 39 | +19% |
| csls: oracle domain | 0.768 | 0.846 | 0.933 | 0.796 | 0.735 | 39 / 4 | 0.00 | 28 / 0 | 0.00 | 1.000 | 0 / 73 | +97% |
| csls: desc region-first | 0.166 | 0.249 | 0.987 | 0.036 | 0.313 | 10 / 164 | 0.00 | 6 / 167 | 0.00 | 0.185 | 256 / 6 | -428% |
| csls: desc top-2 | 0.252 | 0.357 | 0.984 | 0.138 | 0.381 | 9 / 136 | 0.00 | 5 / 139 | 0.00 | 0.312 | 216 / 19 | -353% |
| csls: desc soft λ=0.1 | 0.669 | 0.754 | 0.936 | 0.713 | 0.619 | 12 / 8 | 0.50 | 0 / 3 | 0.25 | 0.793 | 65 / 39 | +11% |
| csls: desc soft λ=0.25 | 0.666 | 0.752 | 0.940 | 0.707 | 0.619 | 13 / 10 | 0.68 | 1 / 5 | 0.22 | 0.790 | 66 / 39 | +8% |
| csls: desc soft λ=0.5 | 0.653 | 0.745 | 0.945 | 0.689 | 0.612 | 11 / 12 | 1.00 | 1 / 9 | 0.02 | 0.777 | 70 / 39 | -3% |
| csls: desc path λ=0.1 | 0.666 | 0.754 | 0.937 | 0.707 | 0.619 | 11 / 8 | 0.65 | 1 / 5 | 0.22 | 0.799 | 63 / 42 | +8% |
| csls: desc path λ=0.25 | 0.656 | 0.748 | 0.941 | 0.701 | 0.605 | 11 / 11 | 1.00 | 3 / 10 | 0.09 | 0.796 | 64 / 44 | +0% |
| csls: desc path λ=0.5 | 0.653 | 0.743 | 0.946 | 0.701 | 0.599 | 13 / 14 | 1.00 | 5 / 13 | 0.10 | 0.799 | 63 / 46 | -3% |
| csls: alias-c region-first | 0.465 | 0.537 | 0.961 | 0.473 | 0.456 | 13 / 73 | 0.00 | 5 / 72 | 0.00 | 0.576 | 133 / 35 | -167% |
| csls: alias-c top-2 | 0.551 | 0.623 | 0.959 | 0.563 | 0.537 | 11 / 44 | 0.00 | 3 / 43 | 0.00 | 0.682 | 100 / 41 | -92% |
| csls: alias-c soft λ=0.1 | 0.672 | 0.756 | 0.939 | 0.719 | 0.619 | 12 / 7 | 0.36 | 0 / 2 | 0.50 | 0.799 | 63 / 40 | +14% |
| csls: alias-c soft λ=0.25 | 0.669 | 0.753 | 0.945 | 0.713 | 0.619 | 12 / 8 | 0.50 | 0 / 3 | 0.25 | 0.799 | 63 / 41 | +11% |
| csls: alias-c soft λ=0.5 | 0.659 | 0.746 | 0.951 | 0.701 | 0.612 | 11 / 10 | 1.00 | 0 / 6 | 0.03 | 0.787 | 67 / 40 | +3% |
| csls: alias-c path λ=0.1 | 0.672 | 0.755 | 0.938 | 0.719 | 0.619 | 12 / 7 | 0.36 | 1 / 3 | 0.62 | 0.803 | 62 / 41 | +14% |
| csls: alias-c path λ=0.25 | 0.662 | 0.751 | 0.943 | 0.713 | 0.605 | 11 / 9 | 0.82 | 1 / 6 | 0.12 | 0.803 | 62 / 44 | +6% |
| csls: alias-c path λ=0.5 | 0.656 | 0.743 | 0.948 | 0.713 | 0.592 | 10 / 10 | 1.00 | 3 / 10 | 0.09 | 0.796 | 64 / 44 | +0% |
| csls: body-c region-first | 0.417 | 0.498 | 0.967 | 0.425 | 0.408 | 12 / 87 | 0.00 | 6 / 88 | 0.00 | 0.525 | 149 / 34 | -208% |
| csls: body-c top-2 | 0.516 | 0.593 | 0.958 | 0.527 | 0.503 | 12 / 56 | 0.00 | 4 / 55 | 0.00 | 0.646 | 111 / 41 | -122% |
| csls: body-c soft λ=0.1 | 0.675 | 0.758 | 0.941 | 0.725 | 0.619 | 13 / 7 | 0.26 | 0 / 1 | 1.00 | 0.803 | 62 / 40 | +17% |
| csls: body-c soft λ=0.25 | 0.669 | 0.753 | 0.950 | 0.713 | 0.619 | 12 / 8 | 0.50 | 0 / 3 | 0.25 | 0.799 | 63 / 41 | +11% |
| csls: body-c soft λ=0.5 | 0.659 | 0.748 | 0.960 | 0.707 | 0.605 | 11 / 10 | 1.00 | 0 / 6 | 0.03 | 0.790 | 66 / 41 | +3% |
| csls: body-c path λ=0.1 | 0.675 | 0.758 | 0.941 | 0.725 | 0.619 | 13 / 7 | 0.26 | 1 / 2 | 1.00 | 0.806 | 61 / 41 | +17% |
| csls: body-c path λ=0.25 | 0.666 | 0.752 | 0.949 | 0.719 | 0.605 | 12 / 9 | 0.66 | 1 / 5 | 0.22 | 0.809 | 60 / 45 | +8% |
| csls: body-c path λ=0.5 | 0.656 | 0.745 | 0.959 | 0.713 | 0.592 | 12 / 12 | 1.00 | 2 / 9 | 0.07 | 0.806 | 61 / 47 | +0% |
| csls: mean3 region-first | 0.411 | 0.491 | 0.965 | 0.383 | 0.442 | 9 / 86 | 0.00 | 4 / 88 | 0.00 | 0.490 | 160 / 25 | -214% |
| csls: mean3 top-2 | 0.525 | 0.600 | 0.958 | 0.515 | 0.537 | 10 / 51 | 0.00 | 3 / 51 | 0.00 | 0.640 | 113 / 36 | -114% |
| csls: mean3 soft λ=0.1 | 0.672 | 0.756 | 0.939 | 0.719 | 0.619 | 12 / 7 | 0.36 | 0 / 2 | 0.50 | 0.799 | 63 / 40 | +14% |
| csls: mean3 soft λ=0.25 | 0.662 | 0.751 | 0.945 | 0.707 | 0.612 | 12 / 10 | 0.83 | 0 / 5 | 0.06 | 0.790 | 66 / 40 | +6% |
| csls: mean3 soft λ=0.5 | 0.662 | 0.748 | 0.952 | 0.701 | 0.619 | 12 / 10 | 0.83 | 1 / 6 | 0.12 | 0.790 | 66 / 40 | +6% |
| csls: mean3 path λ=0.1 | 0.675 | 0.757 | 0.939 | 0.725 | 0.619 | 13 / 7 | 0.26 | 1 / 2 | 1.00 | 0.806 | 61 / 41 | +17% |
| csls: mean3 path λ=0.25 | 0.659 | 0.750 | 0.944 | 0.707 | 0.605 | 11 / 10 | 1.00 | 1 / 7 | 0.07 | 0.803 | 62 / 45 | +3% |
| csls: mean3 path λ=0.5 | 0.662 | 0.748 | 0.952 | 0.713 | 0.605 | 13 / 11 | 0.84 | 4 / 9 | 0.27 | 0.806 | 61 / 45 | +6% |
| csls: desc+h region-first | 0.185 | 0.265 | 0.988 | 0.066 | 0.320 | 10 / 158 | 0.00 | 7 / 162 | 0.00 | 0.204 | 250 / 6 | -411% |
| csls: desc+h top-2 | 0.283 | 0.378 | 0.970 | 0.204 | 0.374 | 9 / 126 | 0.00 | 5 / 129 | 0.00 | 0.347 | 205 / 20 | -325% |
| csls: desc+h soft λ=0.1 | 0.666 | 0.754 | 0.939 | 0.707 | 0.619 | 13 / 10 | 0.68 | 1 / 5 | 0.22 | 0.790 | 66 / 39 | +8% |
| csls: desc+h soft λ=0.25 | 0.656 | 0.748 | 0.946 | 0.695 | 0.612 | 11 / 11 | 1.00 | 2 / 9 | 0.07 | 0.774 | 71 / 37 | +0% |
| csls: desc+h soft λ=0.5 | 0.631 | 0.724 | 0.951 | 0.683 | 0.571 | 10 / 18 | 0.18 | 2 / 17 | 0.00 | 0.745 | 80 / 36 | -22% |
| csls: desc+h path λ=0.1 | 0.659 | 0.752 | 0.940 | 0.695 | 0.619 | 12 / 11 | 1.00 | 1 / 7 | 0.07 | 0.790 | 66 / 41 | +3% |
| csls: desc+h path λ=0.25 | 0.656 | 0.747 | 0.948 | 0.695 | 0.612 | 14 / 14 | 1.00 | 4 / 11 | 0.12 | 0.796 | 64 / 44 | +0% |
| csls: desc+h path λ=0.5 | 0.611 | 0.715 | 0.952 | 0.653 | 0.565 | 13 / 27 | 0.04 | 8 / 29 | 0.00 | 0.761 | 75 / 47 | -39% |
| csls: alias-c+h region-first | 0.389 | 0.480 | 0.971 | 0.353 | 0.429 | 8 / 92 | 0.00 | 3 / 94 | 0.00 | 0.487 | 161 / 31 | -233% |
| csls: alias-c+h top-2 | 0.503 | 0.586 | 0.962 | 0.485 | 0.524 | 14 / 62 | 0.00 | 5 / 60 | 0.00 | 0.627 | 117 / 39 | -133% |
| csls: alias-c+h soft λ=0.1 | 0.666 | 0.753 | 0.943 | 0.707 | 0.619 | 12 / 9 | 0.66 | 0 / 4 | 0.12 | 0.796 | 64 / 41 | +8% |
| csls: alias-c+h soft λ=0.25 | 0.659 | 0.748 | 0.950 | 0.701 | 0.612 | 11 / 10 | 1.00 | 0 / 6 | 0.03 | 0.790 | 66 / 41 | +3% |
| csls: alias-c+h soft λ=0.5 | 0.640 | 0.729 | 0.957 | 0.683 | 0.592 | 11 / 16 | 0.44 | 1 / 13 | 0.00 | 0.764 | 74 / 39 | -14% |
| csls: alias-c+h path λ=0.1 | 0.662 | 0.751 | 0.942 | 0.701 | 0.619 | 12 / 10 | 0.83 | 0 / 5 | 0.06 | 0.796 | 64 / 42 | +6% |
| csls: alias-c+h path λ=0.25 | 0.653 | 0.744 | 0.949 | 0.701 | 0.599 | 11 / 12 | 1.00 | 1 / 9 | 0.02 | 0.793 | 65 / 44 | -3% |
| csls: alias-c+h path λ=0.5 | 0.637 | 0.731 | 0.953 | 0.683 | 0.585 | 12 / 18 | 0.36 | 6 / 19 | 0.01 | 0.777 | 70 / 44 | -17% |
| csls: body-c+h region-first | 0.341 | 0.431 | 0.968 | 0.287 | 0.401 | 10 / 109 | 0.00 | 5 / 111 | 0.00 | 0.433 | 178 / 29 | -275% |
| csls: body-c+h top-2 | 0.452 | 0.537 | 0.961 | 0.437 | 0.469 | 12 / 76 | 0.00 | 4 / 75 | 0.00 | 0.576 | 133 / 39 | -178% |
| csls: body-c+h soft λ=0.1 | 0.669 | 0.755 | 0.946 | 0.713 | 0.619 | 12 / 8 | 0.50 | 0 / 3 | 0.25 | 0.799 | 63 / 41 | +11% |
| csls: body-c+h soft λ=0.25 | 0.659 | 0.748 | 0.959 | 0.701 | 0.612 | 11 / 10 | 1.00 | 0 / 6 | 0.03 | 0.790 | 66 / 41 | +3% |
| csls: body-c+h soft λ=0.5 | 0.646 | 0.737 | 0.969 | 0.689 | 0.599 | 11 / 14 | 0.69 | 1 / 11 | 0.01 | 0.777 | 70 / 41 | -8% |
| csls: body-c+h path λ=0.1 | 0.666 | 0.753 | 0.946 | 0.707 | 0.619 | 12 / 9 | 0.66 | 0 / 4 | 0.12 | 0.799 | 63 / 42 | +8% |
| csls: body-c+h path λ=0.25 | 0.656 | 0.746 | 0.958 | 0.707 | 0.599 | 13 / 13 | 1.00 | 1 / 8 | 0.04 | 0.796 | 64 / 44 | +0% |
| csls: body-c+h path λ=0.5 | 0.640 | 0.737 | 0.967 | 0.689 | 0.585 | 11 / 16 | 0.44 | 3 / 15 | 0.01 | 0.787 | 67 / 46 | -14% |
| csls: mean3+h region-first | 0.354 | 0.441 | 0.970 | 0.287 | 0.429 | 9 / 104 | 0.00 | 4 / 106 | 0.00 | 0.420 | 182 / 21 | -264% |
| csls: mean3+h top-2 | 0.500 | 0.579 | 0.959 | 0.461 | 0.544 | 12 / 61 | 0.00 | 5 / 61 | 0.00 | 0.605 | 124 / 33 | -136% |
| csls: mean3+h soft λ=0.1 | 0.666 | 0.753 | 0.944 | 0.707 | 0.619 | 12 / 9 | 0.66 | 0 / 4 | 0.12 | 0.793 | 65 / 40 | +8% |
| csls: mean3+h soft λ=0.25 | 0.662 | 0.749 | 0.952 | 0.701 | 0.619 | 12 / 10 | 0.83 | 1 / 6 | 0.12 | 0.790 | 66 / 40 | +6% |
| csls: mean3+h soft λ=0.5 | 0.643 | 0.734 | 0.962 | 0.683 | 0.599 | 11 / 15 | 0.56 | 1 / 12 | 0.00 | 0.764 | 74 / 38 | -11% |
| csls: mean3+h path λ=0.1 | 0.662 | 0.752 | 0.942 | 0.701 | 0.619 | 12 / 10 | 0.83 | 0 / 5 | 0.06 | 0.796 | 64 / 42 | +6% |
| csls: mean3+h path λ=0.25 | 0.662 | 0.750 | 0.952 | 0.707 | 0.612 | 14 / 12 | 0.85 | 3 / 8 | 0.23 | 0.806 | 61 / 45 | +6% |
| csls: mean3+h path λ=0.5 | 0.631 | 0.729 | 0.959 | 0.683 | 0.571 | 12 / 20 | 0.22 | 5 / 20 | 0.00 | 0.783 | 68 / 48 | -22% |

</details>
