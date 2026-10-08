#!/usr/bin/env python3
"""Region signals independent of the way scores (follow-up to ADR-700 §9).

§9 found that region-first routing never beats flat competition when the
region score is derived from the way scores, while a domain oracle would lift
top-1 from 0.656 to 0.771. This script scores each region from its own vector
instead, and asks how much of that headroom comes back.

    OUT=/tmp/region-out experiments/content-corpus/run.py GOLDEN.tsv [...]
    OUT=/tmp/region-out experiments/content-corpus/region.py GOLDEN.tsv [...]

Region signals, one vector per region node of the way tree (every domain, and
every intermediate directory such as softwaredev/code):
  desc     the node's description, embedded alone with the same way-embed and
           model as run.py. Resolution as in issue #664: the description of the
           way in that directory; for a node with no way, an authored line from
           region-descriptions.tsv (written from the ways' own descriptions,
           not from the golden prompts).
  alias-c  the normalised mean of the alias vectors of every way under the node.
  body-c   the normalised mean of the body section vectors (run.py `section`
           chunks) of every way under the node.
  mean3    the mean of the three region cosines above.
  <sig>+h  the same signal with a label-free hubness correction at the region
           level: 2 × cosine − the region's mean cosine over its 10 closest
           prompts in the other fold (CSLS k=10, the folds of geometry.py).
           Some region vectors sit close to every prompt; this removes that.

None of them reads a label; the centroids use all ways, which is allowed.

Methods, on cosine way scores and on hubness-corrected ones (CSLS k=10,
two-fold, the folds of geometry.py):
  region-first   rank domains by region score, the top domain's ways first
                 (by way score), then the next domain's
  top-2          the two best domains' ways first, by way score, then the rest
  soft λ         way score + λ × its domain's region score
  path λ         way score + λ × mean region score of all its ancestor nodes
                 (uses the intermediate-directory vectors)
  descent        cosine only: from the root, children compete; a child subtree
                 by its region score, a leaf way or the node's own way by its
                 way score. Mixes two score scales, reported for completeness.
"""

import json
import random
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import geometry  # noqa: E402
import run  # noqa: E402
from subtree import Tree, sign_test  # noqa: E402

O = run.OUT
HERE = Path(__file__).parent
RAW = ("desc", "alias-c", "body-c", "mean3")
SIGNALS = RAW + tuple(f"{s}+h" for s in RAW)
LAMS = (0.1, 0.25, 0.5)


def unit(v):
    return v / np.linalg.norm(v)


def embed_texts(texts, tag):
    raw = O / f"{tag}.raw.jsonl"
    raw.write_text("".join(
        json.dumps({"id": f"n{i}", "description": t, "vocabulary": "",
                    "threshold": 0, "embed_threshold": 0.0}) + "\n"
        for i, t in enumerate(texts)))
    out = O / f"{tag}.jsonl"
    subprocess.run([str(run.EMBED), "generate", "--corpus", str(raw), "--model", str(run.MODEL),
                    "--output", str(out)], check=True, capture_output=True)
    return np.array([json.loads(l)["embedding"] for l in out.read_text().splitlines()])


def region_nodes(tree):
    """Every domain (including a domain that is a single way, such as
    `research`) and every intermediate directory."""
    return sorted(set(tree.children[""]) | {n for n in tree.children if n and tree.children[n]})


# ── region vectors ───────────────────────────────────────────────────────────

class Data:
    def __init__(self, golden_paths):
        alias = [json.loads(l) for l in (O / "alias.jsonl").read_text().splitlines()]
        self.ids = [a["id"] for a in alias]
        self.desc = {a["id"]: a["description"] for a in alias}
        self.W = np.array([a["embedding"] for a in alias])
        known = set(self.ids)
        self.g = [x for x in geometry.load_golden(golden_paths) if x[1] == "none" or x[1] in known]
        self.Q = geometry.embed_prompts([x[0] for x in self.g])
        col = {w: j for j, w in enumerate(self.ids)}
        self.exp = [None if e == "none" else col[e] for _, e in self.g]

        chunks = [json.loads(l) for l in (O / "content-section.jsonl").read_text().splitlines()]
        self.CV = np.array([c["embedding"] for c in chunks])
        self.owner = np.array([col[c["id"].split("##")[0]] for c in chunks])

        self.authored = {}
        for line in (HERE / "region-descriptions.tsv").read_text().splitlines():
            if line.startswith("#") or line.startswith("node\t") or not line.strip():
                continue
            node, text = line.split("\t", 1)
            self.authored[node] = text

        full = Tree(self.ids)
        nodes = region_nodes(full)
        texts, self.desc_src = [], {}
        for n in nodes:
            if n in self.desc:
                texts.append(self.desc[n])
                self.desc_src[n] = "way"
            elif n in self.authored:
                texts.append(self.authored[n])
                self.desc_src[n] = "authored"
            else:
                raise SystemExit(f"node {n} has no way and no authored description")
        V = embed_texts(texts, "region-desc")
        self.desc_vec = {n: V[k] for k, n in enumerate(nodes)}

        # Hubness-corrected way scores, as subtree.py.
        rng = random.Random(7)
        order = list(range(len(self.g)))
        rng.shuffle(order)
        folds = [set(order[::2]), set(order[1::2])]
        self.folds = [(sorted(f), [i for i in range(len(self.g)) if i not in f]) for f in folds]
        self.C = self.Q @ self.W.T
        self.H = np.zeros_like(self.C)
        for f in folds:
            fit = [i for i in range(len(self.g)) if i not in f]
            test = sorted(f)
            penalty = np.sort(self.Q[fit] @ self.W.T, axis=0)[-10:].mean(0)
            self.H[test] = 2 * (self.Q[test] @ self.W.T) - penalty

    def region_scores(self, tree, keep):
        """signal -> node -> cosine per prompt, for the region nodes of
        `tree`, whose columns are the original ways `keep`."""
        out = {}
        nodes = region_nodes(tree)
        desc = {n: self.Q @ self.desc_vec[n] for n in nodes}
        ac, bc = {}, {}
        for n in nodes:
            cols = [keep[c] for c in tree.under[n]]
            ac[n] = self.Q @ unit(self.W[cols].mean(0))
            m = np.isin(self.owner, cols)
            bc[n] = self.Q @ unit(self.CV[m].mean(0)) if m.any() else ac[n]
        out["desc"], out["alias-c"], out["body-c"] = desc, ac, bc
        out["mean3"] = {n: (desc[n] + ac[n] + bc[n]) / 3 for n in nodes}
        for sig in RAW:
            h = {}
            for n, x in out[sig].items():
                y = np.empty_like(x)
                for test, fit in self.folds:
                    y[test] = 2 * x[test] - np.sort(x[fit])[-10:].mean()
                h[n] = y
            out[f"{sig}+h"] = h
        return out


# ── methods: fn(tree, R, s, i, e) -> (ranking, score row, top region or None) ─

def domains_by(tree, R, i):
    return sorted(tree.children[""], key=lambda d: -R[d][i])


def region_first(tree, R, s, i, e):
    doms = domains_by(tree, R, i)
    out = []
    for d in doms:
        out.extend(sorted(tree.under[d], key=lambda j: -s[j]))
    return out, s, doms[0]


def top_k(k):
    def fn(tree, R, s, i, e):
        doms = domains_by(tree, R, i)
        first = sorted([j for d in doms[:k] for j in tree.under[d]], key=lambda j: -s[j])
        seen = set(first)
        return first + [j for j in np.argsort(-s) if j not in seen], s, doms[0]
    return fn


def soft(lam):
    def fn(tree, R, s, i, e):
        bonus = {d: R[d][i] for d in tree.children[""]}
        ss = np.array([s[j] + lam * bonus[tree.domain_of(j)] for j in range(len(s))])
        return list(np.argsort(-ss)), ss, None
    return fn


def ancestors(w):
    p = w.split("/")
    return ["/".join(p[:k]) for k in range(1, len(p))]


def path(lam):
    def fn(tree, R, s, i, e):
        ss = s.copy()
        for j, w in enumerate(tree.ids):
            a = [R[n][i] for n in ancestors(w) if n in R]
            if a:
                ss[j] += lam * float(np.mean(a))
        return list(np.argsort(-ss)), ss, None
    return fn


def descent(tree, R, s, i, e, node=""):
    items = []
    if node and node in tree.col:
        items.append((s[tree.col[node]], [tree.col[node]], None))
    for ch in tree.children[node]:
        if tree.children[ch]:
            items.append((R[ch][i], None, ch))
        else:
            items.append((s[tree.col[ch]], [tree.col[ch]], None))
    items.sort(key=lambda x: -x[0])
    out = []
    for _, leaf, sub in items:
        out.extend(leaf if leaf is not None else descent(tree, R, s, i, e, sub)[0])
    return out, s, None


def flat(tree, R, s, i, e):
    return list(np.argsort(-s)), s, None


def oracle(tree, R, s, i, e):
    if e is None:
        return flat(tree, R, s, i, e)
    inside = sorted(tree.under[tree.domain_of(e)], key=lambda j: -s[j])
    seen = set(inside)
    return inside + [j for j in np.argsort(-s) if j not in seen], s, None


def method_table():
    """name -> (base, signal or None, fn)"""
    m = {}
    for base in ("cos", "csls"):
        m[f"{base}: flat"] = (base, None, flat)
        m[f"{base}: oracle domain"] = (base, None, oracle)
        for sig in SIGNALS:
            m[f"{base}: {sig} region-first"] = (base, sig, region_first)
            m[f"{base}: {sig} top-2"] = (base, sig, top_k(2))
            for lam in LAMS:
                m[f"{base}: {sig} soft λ={lam}"] = (base, sig, soft(lam))
            for lam in LAMS:
                m[f"{base}: {sig} path λ={lam}"] = (base, sig, path(lam))
            if base == "cos":
                m[f"{base}: {sig} descent"] = (base, sig, descent)
    return m


# ── evaluation ───────────────────────────────────────────────────────────────

def evaluate(fn, tree, R, S, exp, rows=None):
    res = {"hit": [], "rr": [], "right": [], "none_top": [], "dom_ok": [],
           "wrong_region": 0, "wrong_inside": 0}
    for i in (rows if rows is not None else range(len(exp))):
        e = exp[i]
        rk, ss, _ = fn(tree, R, S[i], i, e)
        if e is None:
            res["none_top"].append(ss[rk[0]])
            continue
        idx = rk.index(e)
        res["hit"].append(idx == 0)
        res["rr"].append(1 / (idx + 1))
        res["right"].append(ss[e])
        same = tree.domain_of(rk[0]) == tree.domain_of(e)
        res["dom_ok"].append(same)
        if idx:
            res["wrong_inside" if same else "wrong_region"] += 1
    return res


def main():
    D = Data(sys.argv[1:])
    tree = Tree(D.ids)
    keep = list(range(len(D.ids)))
    R = D.region_scores(tree, keep)
    bases = {"cos": D.C, "csls": D.H}
    exp = D.exp
    targeted = [i for i, e in enumerate(exp) if e is not None]
    sw = [k for k, i in enumerate(targeted) if tree.domain_of(exp[i]) == "softwaredev"]
    swset = set(sw)
    ot = [k for k in range(len(targeted)) if k not in swset]
    doms = sorted(tree.children[""])
    print(f"{len(D.g)} golden rows: {len(targeted)} targeted, {len(exp) - len(targeted)} none; "
          f"{len(D.ids)} ways, {len(doms)} domains, {len(D.desc_vec)} region nodes")
    print("authored node descriptions: " +
          ", ".join(n for n, s in sorted(D.desc_src.items()) if s == "authored"))
    singles = [d for d in doms if not tree.children[d]]
    print(f"single-way domains (region vector = that way's own description, alias or body): "
          f"{', '.join(singles)}\n")

    # ── region choice by the signal alone ───────────────────────────────────
    flat_dom = [tree.domain_of(int(np.argmax(D.C[i]))) == tree.domain_of(exp[i]) for i in targeted]
    print("region choice on targeted rows (domain level)\n")
    print("| signal | top region right | expected domain in top 2 | right where flat's domain is wrong "
          f"(n={flat_dom.count(False)}) | wrong where flat's domain is right (n={flat_dom.count(True)}) |")
    print("|---|---|---|---|---|")
    print(f"| flat argmax's domain (cos) | {np.mean(flat_dom):.3f} | | | |")
    for sig in SIGNALS:
        t1, t2, rescue, lose = [], [], [], []
        for k, i in enumerate(targeted):
            order = domains_by(tree, R[sig], i)
            ed = tree.domain_of(exp[i])
            t1.append(order[0] == ed)
            t2.append(ed in order[:2])
            if flat_dom[k]:
                lose.append(order[0] != ed)
            else:
                rescue.append(order[0] == ed)
        print(f"| {sig} | {np.mean(t1):.3f} | {np.mean(t2):.3f} | "
              f"{sum(rescue)} ({np.mean(rescue):.3f}) | {sum(lose)} ({np.mean(lose):.3f}) |")

    # per-domain region accuracy for each signal
    print("\nregion choice by expected domain\n")
    by = defaultdict(list)
    for k, i in enumerate(targeted):
        by[tree.domain_of(exp[i])].append(k)
    print("| domain | n | desc source | flat | " + " | ".join(SIGNALS) + " |")
    print("|---|---|---|---|" + "---|" * len(SIGNALS))
    for d in sorted(by, key=lambda d: -len(by[d])):
        ks = by[d]
        cells = []
        for sig in SIGNALS:
            cells.append(f"{np.mean([domains_by(tree, R[sig], targeted[k])[0] == d for k in ks]):.2f}")
        src = D.desc_src[d]
        print(f"| {d} | {len(ks)} | {src} | {np.mean([flat_dom[k] for k in ks]):.2f} | " +
              " | ".join(cells) + " |")

    # ── main table ──────────────────────────────────────────────────────────
    methods = method_table()
    scored = {}
    for name, (base, sig, fn) in methods.items():
        scored[name] = evaluate(fn, tree, R[sig] if sig else {}, bases[base], exp)
    flat_cos = scored["cos: flat"]["hit"]
    flat_csls = scored["csls: flat"]["hit"]
    ora = np.mean(scored["cos: oracle domain"]["hit"])
    f0 = np.mean(flat_cos)
    print(f"\nmain table (oracle recovery = (top-1 − {f0:.3f}) / ({ora:.3f} − {f0:.3f}))\n")
    print(f"| method | top-1 | MRR | none AUC | softwaredev (n={len(sw)}) | others (n={len(ot)}) | "
          "fixed / broken vs cos flat | p | fixed / broken vs own flat | p | rank-1 domain right | "
          "misses: wrong domain / wrong way in domain | oracle recovery |")
    print("|" + "---|" * 13)
    for name, r in scored.items():
        h = r["hit"]
        own = flat_cos if name.startswith("cos") else flat_csls
        fx = sum(1 for a, b in zip(flat_cos, h) if not a and b)
        br = sum(1 for a, b in zip(flat_cos, h) if a and not b)
        ofx = sum(1 for a, b in zip(own, h) if not a and b)
        obr = sum(1 for a, b in zip(own, h) if a and not b)
        rec = (np.mean(h) - f0) / (ora - f0)
        print(f"| {name} | {np.mean(h):.3f} | {np.mean(r['rr']):.3f} | "
              f"{run.auroc(r['right'], r['none_top']):.3f} | "
              f"{np.mean([h[k] for k in sw]):.3f} | {np.mean([h[k] for k in ot]):.3f} | "
              f"{fx} / {br} | {sign_test(fx, br):.2f} | {ofx} / {obr} | {sign_test(ofx, obr):.2f} | "
              f"{np.mean(r['dom_ok']):.3f} | {r['wrong_region']} / {r['wrong_inside']} | {rec:+.0%} |")

    # ── best method: corpus growth ─────────────────────────────────────────
    cands = [n for n in scored if methods[n][1] is not None]
    best_cos = max((n for n in cands if n.startswith("cos")), key=lambda n: np.mean(scored[n]["hit"]))
    best_csls = max((n for n in cands if n.startswith("csls")), key=lambda n: np.mean(scored[n]["hit"]))
    growth = ["cos: flat", best_cos, "csls: flat", best_csls, "cos: oracle domain"]
    print(f"\ncorpus growth for the best method per base (top-1 / median margin, "
          f"20 random subsets per size; desc vectors fixed, centroids recomputed per subset)\n")
    print("| ways | " + " | ".join(growth) + " |")
    print("|" + "---|" * (len(growth) + 1))
    rng = random.Random(1)
    for frac in (0.25, 0.5, 0.75, 1.0):
        acc, mar = defaultdict(list), defaultdict(list)
        for _ in range(20 if frac < 1 else 1):
            sub = sorted(rng.sample(range(len(D.ids)), int(frac * len(D.ids))))
            remap = {j: k for k, j in enumerate(sub)}
            st = Tree([D.ids[j] for j in sub])
            Rs = D.region_scores(st, sub)
            sexp = [None if e is None else remap.get(e, -1) for e in exp]
            rows = [i for i in targeted if sexp[i] != -1]
            for name in growth:
                base, sig, fn = methods[name]
                S = bases[base][:, sub]
                r = evaluate(fn, st, Rs[sig] if sig else {}, S, sexp, rows)
                acc[name].append(np.mean(r["hit"]))
                mg = []
                for i in rows:
                    rk, ss, _ = fn(st, Rs[sig] if sig else {}, S[i], i, sexp[i])
                    mg.append(ss[rk[0]] - ss[rk[1]])
                mar[name].append(float(np.median(mg)))
        print(f"| {int(frac * len(D.ids))} | " +
              " | ".join(f"{np.mean(acc[n]):.3f} / {np.mean(mar[n]):.3f}" for n in growth) + " |")


if __name__ == "__main__":
    main()
