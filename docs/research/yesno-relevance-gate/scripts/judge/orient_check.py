#!/usr/bin/env python3
"""Compare result files against bench_items `expect`: accuracy at 0.5, AUC, per-item p_yes."""
import json, sys
exp = {j["id"]: j["expect"] for j in map(json.loads, open("bench_items.jsonl"))}
for fn in sys.argv[1:]:
    rs = [json.loads(l) for l in open(fn)]
    pos = [r["p_yes"] for r in rs if exp[r["id"]] == "yes"]
    neg = [r["p_yes"] for r in rs if exp[r["id"]] == "no"]
    auc = sum((p > n) + 0.5 * (p == n) for p in pos for n in neg) / (len(pos) * len(neg))
    acc = sum((r["answer"] == exp[r["id"]]) for r in rs) / len(rs)
    print(f"{fn}: acc@0.5={acc:.3f} auc={auc:.3f} mean_p(yes-exp)={sum(pos)/len(pos):.3f} mean_p(no-exp)={sum(neg)/len(neg):.3f}")
