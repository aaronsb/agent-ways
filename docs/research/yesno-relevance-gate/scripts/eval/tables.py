#!/usr/bin/env python3
"""Render metrics.json as markdown tables (stdout). REPORT.md is assembled from these."""
import json
import sys
from pathlib import Path

P = Path(__file__).resolve().parent
M = json.load(open(P / "metrics.json"))
T = ["0.0005", "0.001", "0.002", "0.005", "0.01", "0.02", "0.05", "0.1", "0.2", "0.3", "0.5", "0.7"]


def ci(v, pct=False):
    if v is None or v[0] is None:
        return "n/a"
    f = (lambda x: f"{100 * x:.0f}") if pct else (lambda x: f"{x:.3f}")
    lo = f(v[1]) if v[1] is not None else "?"
    hi = f(v[2]) if v[2] is not None else "?"
    return f"{f(v[0])} [{lo}–{hi}]"


def auc_table(subs):
    cfgs = sorted({k.split("|")[0] for k in M["configs"]})
    print("| Config | " + " | ".join(subs) + " |")
    print("|---|" + "---|" * len(subs))
    for c in cfgs:
        cells = [ci(M["configs"].get(f"{c}|{s}", {}).get("auc")) for s in subs]
        if any(x != "n/a" for x in cells):
            print(f"| {c} | " + " | ".join(cells) + " |")


def sweep_table(cfg, sub):
    e = M["configs"].get(f"{cfg}|{sub}")
    if not e:
        return
    print(f"\n**{cfg} on {sub}** (n={e['n']}; noise removed % / relevant lost %)\n")
    print("| t | noise removed | relevant lost |")
    print("|---|---|---|")
    for t in T:
        s = e["sweep"][t]
        print(f"| {t} | {ci(s['removed'], True)} | {ci(s['lost'], True)} |")


def headline(cfgs, subs, ts):
    print("| Config | t | " + " | ".join(f"{s} removed / lost" for s in subs) + " |")
    print("|---|---|" + "---|" * len(subs))
    for c in cfgs:
        for t in ts:
            cells = []
            for s in subs:
                e = M["configs"].get(f"{c}|{s}")
                cells.append("n/a" if not e else
                             f"{ci(e['sweep'][t]['removed'], True)} / {ci(e['sweep'][t]['lost'], True)}")
            print(f"| {c} | {t} | " + " | ".join(cells) + " |")


def controls():
    print("| Control | Subset | AUC [95% CI] |")
    print("|---|---|---|")
    for k, v in M["controls"].items():
        name, sub = k.split("|")
        if name.startswith("matcher"):
            print(f"| matcher fire_score (n={v['n']}, pos={v['pos']}) | {sub} | {ci(v['matcher'])} "
                  f"(judge A+ref on same items {ci(v['judge_ref_same_items'])}) |")
        elif name == "shuffled":
            print(f"| shuffled labels (mean, 2.5–97.5%) | {sub} | {ci(v)} |")
        else:
            print(f"| {name} | {sub} | {ci(v)} |")


def diffs(prefix=""):
    print("| Paired ΔAUC | Subset | Δ [95% CI] | outside 0? |")
    print("|---|---|---|---|")
    for k, v in M["diffs"].items():
        if not k.startswith(prefix) or v[0] is None:
            continue
        name, sub = k.split("|")
        out = "yes" if (v[1] > 0 or v[2] < 0) else "no"
        print(f"| {name} | {sub} | {ci(v)} | {out} |")


def categories(cfgs):
    cats = ["real_relevant", "real_irrelevant", "missed_relevant", "synthetic_hard_neg", "synthetic_easy_neg",
            "ambiguous"]
    print("| Config | best t | " + " | ".join(cats) + " |")
    print("|---|---|" + "---|" * len(cats))
    for c in cfgs:
        e = M["categories"].get(c)
        if e:
            print(f"| {c} | {e['best_t']} | " + " | ".join(
                f"{100 * e['acc'][k]:.0f}% (n={e['n'][k]})" for k in cats) + " |")


def calibration(c):
    print(f"\n**{c}**, sets 1 (non-ambiguous) + 2 pooled\n")
    print("| Decile | n | P(yes) range | mean P(yes) | observed relevant rate |")
    print("|---|---|---|---|---|")
    for r in M["calibration"][c]:
        print(f"| {r['decile']} | {r['n']} | {r['p_min']:.4f}–{r['p_max']:.4f} | {r['mean_p']:.3f} | {r['rate']:.2f} |")


def latency():
    for b in ("local", "haiku"):
        print(f"\n**{b}** (total {M['latency'].get(b + '_total_s', 0):.0f} s of judge time)\n")
        print("| Set:config | n | p50 ms | p95 ms |")
        print("|---|---|---|---|")
        for c, v in M["latency"].get(b, {}).items():
            print(f"| {c} | {v['n']} | {v['p50']:.0f} | {v['p95']:.0f} |")


def human():
    rows = M["human"]
    keys = sorted({k for r in rows for k in r if ":" in k})
    print("| id | category | human | agent | " + " | ".join(keys) + " |")
    print("|---|---|---|---|" + "---|" * len(keys))
    for r in rows:
        print(f"| {r['id']} | {r['cat']} | {r['human']} | {r['agent']} | " +
              " | ".join(f"{r[k]:.3f}" if k in r else "" for k in keys) + " |")


if __name__ == "__main__":
    globals()[sys.argv[1]](*[json.loads(a) for a in sys.argv[2:]])
