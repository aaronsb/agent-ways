#!/usr/bin/env python3
"""Score geometry, domain split and corpus growth for the content-corpus experiment.

Reuses the corpora run.py built in $OUT. Reports:
  1. cosine against mean-centring and a hubness penalty (CSLS), fitted on half
     the prompts without labels and evaluated on the other half;
  2. top-1 by the domain of the expected way;
  3. top-1 and the top-1 margin as the corpus is subsampled.

    experiments/content-corpus/geometry.py GOLDEN.tsv [GOLDEN2.tsv ...]

Vectors from way-embed are L2-normalised, so dot product equals cosine and
Euclidean distance is sqrt(2 - 2 cos); neither is reported separately.
"""

import json
import random
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import run  # noqa: E402

O = run.OUT


def load_golden(paths):
    rows = []
    for p in paths:
        for line in Path(p).read_text().splitlines():
            x = line.split("\t")
            if len(x) >= 2 and x[0] != "prompt":
                rows.append((x[0], x[1]))
    return rows


def unit(m):
    return m / np.linalg.norm(m, axis=1, keepdims=True)


def embed_prompts(prompts):
    raw = O / "prompts.raw.jsonl"
    raw.write_text("".join(
        json.dumps({"id": f"q{i}", "description": p, "vocabulary": "",
                    "threshold": 0, "embed_threshold": 0.0}) + "\n"
        for i, p in enumerate(prompts)))
    out = O / "prompts.jsonl"
    subprocess.run([str(run.EMBED), "generate", "--corpus", str(raw), "--model", str(run.MODEL),
                    "--output", str(out)], check=True, capture_output=True)
    return np.array([json.loads(l)["embedding"] for l in out.read_text().splitlines()])


def main():
    alias = [json.loads(l) for l in (O / "alias.jsonl").read_text().splitlines()]
    ids = [a["id"] for a in alias]
    W = np.array([a["embedding"] for a in alias])
    g = [x for x in load_golden(sys.argv[1:]) if x[1] == "none" or x[1] in set(ids)]
    Q = embed_prompts([x[0] for x in g])
    exp = [x[1] for x in g]

    chunks = [json.loads(l) for l in (O / "content-section.jsonl").read_text().splitlines()]
    CV = np.array([c["embedding"] for c in chunks])
    owner = np.array([ids.index(c["id"].split("##")[0]) for c in chunks])
    CS = Q @ CV.T
    B = np.zeros((len(g), len(ids)))
    for j in range(len(ids)):
        m = owner == j
        if m.any():
            B[:, j] = CS[:, m].max(1)

    rng = random.Random(7)
    order = list(range(len(g)))
    rng.shuffle(order)
    folds = [set(order[::2]), set(order[1::2])]

    def two_fold(fn):
        S = np.zeros((len(g), len(ids)))
        for f in folds:
            fit = [i for i in range(len(g)) if i not in f]
            test = sorted(f)
            S[test] = fn(Q[test], Q[fit])
        return S

    def centred(q, _f):
        mu = W.mean(0)
        return unit(q - mu) @ unit(W - mu).T

    def csls(k):
        def fn(q, f):
            penalty = np.sort(f @ W.T, axis=0)[-k:].mean(0)
            return 2 * (q @ W.T) - penalty
        return fn

    methods = {
        "cosine": Q @ W.T,
        "centred": two_fold(centred),
        "hubness k=10": two_fold(csls(10)),
        "alias+0.25*body": Q @ W.T + 0.25 * B,
    }

    print("1. score geometry")
    for name, S in methods.items():
        top1 = n = 0
        rows, pos, neg, none_top, right = [], [], [], [], []
        for i, e in enumerate(exp):
            s = S[i]
            r = np.argsort(-s)
            if e == "none":
                none_top.append(s[r[0]])
                continue
            j = ids.index(e)
            n += 1
            ok = r[0] == j
            top1 += ok
            rows.append((s[r[0]] - s[r[1]], ok))
            right.append(s[j])
            for k in r[:5]:
                (pos if k == j else neg).append(s[k])
        rows.sort()
        m = len(rows)
        fifths = [rows[i * m // 5:(i + 1) * m // 5] for i in range(5)]
        bands = " ".join(f"{sum(o for _, o in b) / len(b):4.0%}" for b in fifths)
        print(f"  {name:18} top1 {top1 / n:.3f}  AUC top5 {run.auroc(pos, neg):.3f}  "
              f"right>none {run.auroc(right, none_top):.3f}  margin fifths {bands}")

    print("\n2. top-1 by domain of the expected way")
    groups = defaultdict(list)
    for i, e in enumerate(exp):
        if e == "none":
            continue
        d = e.split("/")[0]
        groups[d].append(i)
        if d != "softwaredev":
            groups["(all others)"].append(i)
    for d in sorted(groups, key=lambda k: -len(groups[k])):
        I = groups[d]
        cells = "  ".join(f"{name} {sum(ids[int(np.argmax(S[i]))] == exp[i] for i in I) / len(I):.3f}"
                          for name, S in methods.items())
        print(f"  {d:14} n={len(I):3d}  {cells}")

    print("\n3. corpus growth (20 random subsets per size)")
    rng = random.Random(1)
    for frac in (0.25, 0.5, 0.75, 1.0):
        acc, mar = defaultdict(list), defaultdict(list)
        for _ in range(20 if frac < 1 else 1):
            keep = sorted(rng.sample(range(len(ids)), int(frac * len(ids))))
            kept = set(keep)
            rows = [i for i, e in enumerate(exp) if e != "none" and ids.index(e) in kept]
            for name, S in methods.items():
                sub = S[:, keep]
                ok, mg = 0, []
                for i in rows:
                    o = np.argsort(-sub[i])
                    ok += keep[o[0]] == ids.index(exp[i])
                    mg.append(sub[i][o[0]] - sub[i][o[1]])
                acc[name].append(ok / len(rows))
                mar[name].append(float(np.median(mg)))
        cells = "  ".join(f"{name} {np.mean(acc[name]):.3f}/{np.mean(mar[name]):.3f}" for name in methods)
        print(f"  {int(frac * len(ids)):4d} ways  top1/margin  {cells}")


if __name__ == "__main__":
    main()
