#!/usr/bin/env python3
"""Match-signal strength for alias, body and fused scores.

Reuses the corpora run.py built in $OUT. For each scoring method it reports how
well an absolute score separates the right way from the wrong ones, which is
what a judge reading the score would rely on.

    experiments/content-corpus/signal.py GOLDEN.tsv [GOLDEN2.tsv ...]
"""

import statistics as st
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import run  # noqa: E402

TOPK = 5  # candidates a judge would be shown


def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(q * len(xs)))]


def main():
    golden = []
    for path in sys.argv[1:]:
        for line in Path(path).read_text().splitlines():
            p = line.split("\t")
            if len(p) >= 2 and p[0] != "prompt":
                golden.append((p[0], p[1]))
    ids = [l.split('"id":"', 1)[1].split('"', 1)[0]
           for l in (run.OUT / "alias.jsonl").read_text().splitlines()]
    known = set(ids)
    golden = [g for g in golden if g[1] == "none" or g[1] in known]
    prompts = [g[0] for g in golden]

    alias = run.batch_scores(run.OUT / "alias.jsonl", prompts)
    body = run.batch_scores(run.OUT / "content-section.jsonl", prompts)
    fill = lambda d: {w: d.get(w, 0.0) for w in ids}
    alias = [fill(d) for d in alias]
    body = [fill(d) for d in body]
    methods = {
        "alias": alias,
        "body(section)": body,
        "alias+0.25*body": [{w: a[w] + 0.25 * b[w] for w in ids} for a, b in zip(alias, body)],
    }

    print(f"{len(golden)} rows; candidate pool = top {TOPK} per prompt\n")
    hdr = (f"{'method':18} {'right p50':>9} {'wrong#1 p50':>11} {'margin p50':>10} "
           f"{'z p50':>6} {'AUC all':>8} {'AUC top5':>8} {'none top p50':>12} {'right>none':>10}")
    print(hdr + "\n" + "-" * len(hdr))
    for name, scored in methods.items():
        right, wrong1, margin, z = [], [], [], []
        pool_pos, pool_neg, all_pos, all_neg, none_top = [], [], [], [], []
        for (prompt, exp), d in zip(golden, scored):
            ranked = sorted(d, key=d.get, reverse=True)
            vals = list(d.values())
            mu, sd = st.mean(vals), st.pstdev(vals) or 1e-9
            if exp == "none":
                none_top.append(d[ranked[0]])
                continue
            r = d[exp]
            best_wrong = max(d[w] for w in ids if w != exp)
            right.append(r)
            wrong1.append(best_wrong)
            margin.append(r - best_wrong)
            z.append((r - mu) / sd)
            all_pos.append(r)
            all_neg.extend(d[w] for w in ids if w != exp)
            for w in ranked[:TOPK]:
                (pool_pos if w == exp else pool_neg).append(d[w])
        auc_all = run.auroc(all_pos, all_neg)
        auc_pool = run.auroc(pool_pos, pool_neg)
        right_vs_none = run.auroc(right, none_top)
        print(f"{name:18} {pct(right, .5):9.3f} {pct(wrong1, .5):11.3f} {pct(margin, .5):10.3f} "
              f"{pct(z, .5):6.2f} {auc_all:8.3f} {auc_pool:8.3f} {pct(none_top, .5):12.3f} "
              f"{right_vs_none:10.3f}")

    # Score bands a judge would see: how often is a candidate in each band right?
    print("\nprecision by score band among top-5 candidates (n right / n in band)")
    for name, scored in methods.items():
        cands = []
        for (prompt, exp), d in zip(golden, scored):
            if exp == "none":
                continue
            for w in sorted(d, key=d.get, reverse=True)[:TOPK]:
                cands.append((d[w], w == exp))
        cands.sort()
        n = len(cands)
        bands = [cands[i * n // 5:(i + 1) * n // 5] for i in range(5)]
        cells = []
        for b in bands:
            hits = sum(1 for _, ok in b if ok)
            cells.append(f"[{b[0][0]:.2f}-{b[-1][0]:.2f}] {hits / len(b):5.1%}")
        print(f"  {name:18} " + "  ".join(cells))


if __name__ == "__main__":
    main()
