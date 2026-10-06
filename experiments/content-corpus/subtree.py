#!/usr/bin/env python3
"""Region-first routing against flat competition (follow-up to ADR-700).

Asks whether choosing a region of the way tree first (a domain, or a subtree on
the way down from the roots) and then the way inside it beats competing every
way against every other at once. Uses the alias corpus run.py built in $OUT.

    OUT=/tmp/subtree-out experiments/content-corpus/run.py GOLDEN.tsv [...]
    OUT=/tmp/subtree-out experiments/content-corpus/subtree.py GOLDEN.tsv [...]

Way ids are paths. The domain is the first segment. A node of the tree is any
path prefix of a way id; it may or may not be a way itself (5 of 10 domains
have a root way). Every method produces a full ranking, so MRR and recall@3
are defined; regional methods rank the chosen region's ways first, then the
next region's, and so on.

Score aggregations for a region:
  max    best way score in the region
  top3   mean of the region's best 3 way scores (fewer if the region is smaller)
  root   the region root way's own score, where that way exists; else max

`max` has a property worth stating up front: the region holding the global
argmax always has the highest region max, so every max-aggregated hard method
returns the flat argmax as top-1. The same holds for the soft bonus
s + lam * max(domain). These rows are reported to confirm it.
"""

import math
import random
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import geometry  # noqa: E402
import run  # noqa: E402

O = run.OUT


# ── region aggregations ─────────────────────────────────────────────────────

def agg_max(vals, _root):
    return max(vals)


def agg_top3(vals, _root):
    v = sorted(vals, reverse=True)[:3]
    return sum(v) / len(v)


def agg_root(vals, root):
    return root if root is not None else max(vals)


AGGS = {"max": agg_max, "top3": agg_top3, "root": agg_root}


# ── tree ─────────────────────────────────────────────────────────────────────

class Tree:
    """Nodes are path prefixes of the kept way ids; '' is the virtual root."""

    def __init__(self, ids):
        self.ids = ids
        self.col = {w: j for j, w in enumerate(ids)}
        self.children = defaultdict(set)
        self.under = defaultdict(list)  # node -> columns of ways in its subtree
        for j, w in enumerate(ids):
            parts = w.split("/")
            prev = ""
            self.under[""].append(j)
            for k in range(1, len(parts) + 1):
                node = "/".join(parts[:k])
                self.children[prev].add(node)
                self.under[node].append(j)
                prev = node

    def domain_of(self, j):
        return self.ids[j].split("/")[0]


def region_score(tree, s, node, agg):
    vals = [s[j] for j in tree.under[node]]
    root = s[tree.col[node]] if node in tree.col else None
    return agg(vals, root)


def descent_ranking(tree, s, agg, node="", max_depth=None, depth=0):
    """Full ranking by recursive descent. At a node, the node's own way (if
    any) competes on its own score against each child subtree's aggregate;
    the winner is emitted (own) or recursed into (child), then the next.
    With max_depth=1 the tree is cut at the domains: inside a domain all its
    ways compete flat."""
    if max_depth is not None and depth >= max_depth:
        cols = tree.under[node]
        return sorted(cols, key=lambda j: -s[j])
    items = []
    if node and node in tree.col:
        items.append((s[tree.col[node]], 0, node, True))
    for ch in tree.children[node]:
        items.append((region_score(tree, s, ch, agg), 1, ch, False))
    items.sort(key=lambda x: (-x[0], x[1]))
    out = []
    for _, _, n, own in items:
        if own:
            out.append(tree.col[n])
        else:
            out.extend(descent_ranking(tree, s, agg, n, max_depth, depth + 1))
    return out


def topk_ranking(tree, s, agg, k):
    doms = sorted(tree.children[""], key=lambda d: -region_score(tree, s, d, agg))
    union = [j for d in doms[:k] for j in tree.under[d]]
    rest = descent_ranking(tree, s, agg, max_depth=1)
    first = sorted(union, key=lambda j: -s[j])
    seen = set(first)
    return first + [j for j in rest if j not in seen], doms[:k]


def soft_scores(tree, s, agg, lam):
    bonus = {d: region_score(tree, s, d, agg) for d in tree.children[""]}
    return np.array([s[j] + lam * bonus[tree.domain_of(j)] for j in range(len(s))])


# ── methods ──────────────────────────────────────────────────────────────────
# Each method maps (tree, score row, expected column or None) to
# (ranking, score row used for margin / none separation, chosen domains).

def methods_for(base_name):
    m = {f"{base_name}: flat": lambda t, s, e: (list(np.argsort(-s)), s, None)}
    for a in AGGS:
        m[f"{base_name}: domain-first {a}"] = (
            lambda a: lambda t, s, e: (descent_ranking(t, s, AGGS[a], max_depth=1), s, None))(a)
    for a in ("max", "top3"):
        for k in (2, 3):
            m[f"{base_name}: top-{k} domains {a}"] = (
                lambda a, k: lambda t, s, e: (lambda r: (r[0], s, r[1]))(topk_ranking(t, s, AGGS[a], k)))(a, k)
    for a in AGGS:
        m[f"{base_name}: descent {a}"] = (
            lambda a: lambda t, s, e: (descent_ranking(t, s, AGGS[a]), s, None))(a)
    for a in ("max", "top3"):
        for lam in (0.1, 0.25, 0.5):
            def f(t, s, e, a=a, lam=lam):
                ss = soft_scores(t, s, AGGS[a], lam)
                return list(np.argsort(-ss)), ss, None
            m[f"{base_name}: soft {a} lam={lam}"] = f
    m[f"{base_name}: oracle domain"] = oracle
    return m


def oracle(t, s, e):
    """Upper bound: argmax inside the expected way's own domain."""
    if e is None:
        return list(np.argsort(-s)), s, None
    d = t.domain_of(e)
    inside = sorted(t.under[d], key=lambda j: -s[j])
    seen = set(inside)
    return inside + [j for j in np.argsort(-s) if j not in seen], s, None


# ── evaluation ───────────────────────────────────────────────────────────────

def sign_test(a, b):
    n = a + b
    if n == 0:
        return 1.0
    k = min(a, b)
    p = sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n
    return min(1.0, 2 * p)


def evaluate(fn, tree, S, exp_cols):
    """exp_cols[i] is a column index, or None for a `none` row."""
    res = {"hit": [], "rr": [], "r3": [], "margin": [], "right": [], "none_top": [],
           "wrong_region": 0, "wrong_inside": 0}
    for i, e in enumerate(exp_cols):
        rk, ss, _ = fn(tree, S[i], e)
        if e is None:
            res["none_top"].append(ss[rk[0]])
            continue
        idx = rk.index(e)
        res["hit"].append(idx == 0)
        res["rr"].append(1 / (idx + 1))
        res["r3"].append(idx < 3)
        res["margin"].append(ss[rk[0]] - ss[rk[1]])
        res["right"].append(ss[e])
        if idx:
            if tree.domain_of(rk[0]) != tree.domain_of(e):
                res["wrong_region"] += 1
            else:
                res["wrong_inside"] += 1
    return res


def main():
    alias = [__import__("json").loads(l) for l in (O / "alias.jsonl").read_text().splitlines()]
    ids = [a["id"] for a in alias]
    W = np.array([a["embedding"] for a in alias])
    known = set(ids)
    g = [x for x in geometry.load_golden(sys.argv[1:]) if x[1] == "none" or x[1] in known]
    Q = geometry.embed_prompts([x[0] for x in g])
    col = {w: j for j, w in enumerate(ids)}
    exp_cols = [None if e == "none" else col[e] for _, e in g]

    # Hubness-corrected scores, CSLS k=10, two-fold, same folds as geometry.py.
    rng = random.Random(7)
    order = list(range(len(g)))
    rng.shuffle(order)
    folds = [set(order[::2]), set(order[1::2])]
    H = np.zeros((len(g), len(ids)))
    for f in folds:
        fit = [i for i in range(len(g)) if i not in f]
        test = sorted(f)
        penalty = np.sort(Q[fit] @ W.T, axis=0)[-10:].mean(0)
        H[test] = 2 * (Q[test] @ W.T) - penalty

    C = Q @ W.T
    tree = Tree(ids)
    scored = {}
    allm = {}
    for base, S in (("cos", C), ("csls", H)):
        for name, fn in methods_for(base).items():
            allm[name] = (fn, S)
            scored[name] = evaluate(fn, tree, S, exp_cols)

    targeted = [i for i, e in enumerate(exp_cols) if e is not None]
    sw = [k for k, i in enumerate(targeted) if tree.domain_of(exp_cols[i]) == "softwaredev"]
    ot = [k for k in range(len(targeted)) if k not in set(sw)]
    n_none = sum(e is None for e in exp_cols)
    print(f"{len(g)} golden rows: {len(targeted)} targeted, {n_none} none; {len(ids)} ways, "
          f"{len(tree.children[''])} domains\n")

    base = scored["cos: flat"]["hit"]
    hdr = (f"| method | top-1 | MRR | r@3 | none AUC | softwaredev (n={len(sw)}) | "
           f"others (n={len(ot)}) | fixed / broken | sign p | median margin | "
           f"misses: wrong domain / wrong way in domain |")
    print(hdr)
    print("|" + "---|" * 11)
    for name, r in scored.items():
        h = r["hit"]
        fixed = sum(1 for a, b in zip(base, h) if not a and b)
        broke = sum(1 for a, b in zip(base, h) if a and not b)
        print(f"| {name} | {np.mean(h):.3f} | {np.mean(r['rr']):.3f} | {np.mean(r['r3']):.3f} | "
              f"{run.auroc(r['right'], r['none_top']):.3f} | "
              f"{np.mean([h[k] for k in sw]):.3f} | {np.mean([h[k] for k in ot]):.3f} | "
              f"{fixed} / {broke} | {sign_test(fixed, broke):.2f} | "
              f"{np.median(r['margin']):.3f} | {r['wrong_region']} / {r['wrong_inside']} |")

    # How often each region scorer picks the expected way's domain, with
    # log-sum-exp at several temperatures as a family between max and sum.
    print("\ndomain-choice accuracy on targeted rows (cosine)\n")
    scorers = dict(AGGS)
    for tau in (0.01, 0.02, 0.05, 0.1):
        scorers[f"lse tau={tau}"] = (
            lambda tau: lambda v, _r: tau * math.log(sum(math.exp(x / tau) for x in v)))(tau)
    print("| region scorer | domain right | top-1 after argmax in chosen domain |")
    print("|---|---|---|")
    for name, agg in scorers.items():
        dok = hit = 0
        for i in targeted:
            doms = tree.children[""]
            best = max(doms, key=lambda d: region_score(tree, C[i], d, agg))
            dok += best == tree.domain_of(exp_cols[i])
            hit += max(tree.under[best], key=lambda j: C[i][j]) == exp_cols[i]
        print(f"| {name} | {dok / len(targeted):.3f} | {hit / len(targeted):.3f} |")

    # Corpus growth, as geometry.py section 3: random way subsets, 20 draws per
    # size, scored on targeted rows whose way survived. The tree is rebuilt on
    # each subset, so a removed root way falls back to the region max.
    growth = ["cos: flat", "cos: domain-first top3", "cos: soft top3 lam=0.25",
              "cos: descent top3", "csls: flat", "csls: soft top3 lam=0.25"]
    print("\ncorpus growth (top-1 / median margin, 20 random subsets per size)\n")
    print("| ways | " + " | ".join(growth) + " |")
    print("|" + "---|" * (len(growth) + 1))
    rng = random.Random(1)
    for frac in (0.25, 0.5, 0.75, 1.0):
        acc, mar = defaultdict(list), defaultdict(list)
        for _ in range(20 if frac < 1 else 1):
            keep = sorted(rng.sample(range(len(ids)), int(frac * len(ids))))
            remap = {j: k for k, j in enumerate(keep)}
            sub_tree = Tree([ids[j] for j in keep])
            rows = [i for i in targeted if exp_cols[i] in remap]
            for name in growth:
                fn, S = allm[name]
                Ssub = S[:, keep]
                r = evaluate(fn, sub_tree, Ssub[rows], [remap[exp_cols[i]] for i in rows])
                acc[name].append(np.mean(r["hit"]))
                mar[name].append(float(np.median(r["margin"])))
        print(f"| {int(frac * len(ids))} | " +
              " | ".join(f"{np.mean(acc[n]):.3f} / {np.mean(mar[n]):.3f}" for n in growth) + " |")


if __name__ == "__main__":
    main()
