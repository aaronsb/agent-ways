#!/usr/bin/env python3
"""Metrics for the yes/no gate evaluation. Writes results.jsonl and metrics.json.

Gate rule: a fire is blocked when p_yes < threshold.
noise removed = blocked share of label-0 fires; relevant lost = blocked share of label-1 fires.
"""
import json
import math
import os
import random
import re
from collections import Counter, defaultdict
from pathlib import Path

P = Path(__file__).resolve().parent
DATA = P.parent / "data"
ROOT = Path(os.path.expanduser("~/.claude/hooks/ways"))
THRESHOLDS = [0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5, 0.7]
B = 1000
WVS, CVS = ["A", "B", "C"], ["T1", "T4", "TX", "TXG"]


def load(p):
    return [json.loads(l) for l in open(p) if l.strip()]


# ------------------------------------------------------------------ labels

def labels():
    items = {}
    for it in load(DATA / "eval.jsonl"):
        items[("s1", it["id"])] = {"label": it["label"], "cat": it["category"]}
    for it in load(DATA / "random40.jsonl"):
        items[("s2", it["id"])] = {"label": it["label"], "cat": "random"}
    la = {r["id"]: r["label"] for r in load(DATA / "session_labels.jsonl")}
    lc = {r["id"]: r["label"] for r in load(P / "session_labels_claude.jsonl")}
    for it in load(DATA / "session_unlabelled.jsonl"):
        a, c = la[it["id"]], lc[it["id"]]
        items[("s3", it["id"])] = {"label": a, "label_agent": a, "label_claude": c,
                                   "agree": a == c, "cat": it["lane"]}
    return items


# ------------------------------------------------------------------ stats

def auc(pos, neg):
    if not pos or not neg:
        return None
    allv = sorted([(v, 1) for v in pos] + [(v, 0) for v in neg])
    ranks, i = {}, 0
    rank_sum = 0.0
    while i < len(allv):
        j = i
        while j < len(allv) and allv[j][0] == allv[i][0]:
            j += 1
        r = (i + j + 1) / 2
        rank_sum += r * sum(1 for k in range(i, j) if allv[k][1] == 1)
        i = j
    n1, n0 = len(pos), len(neg)
    return (rank_sum - n1 * (n1 + 1) / 2) / (n1 * n0)


def pct(xs, q):
    xs = sorted(xs)
    if not xs:
        return None
    k = (len(xs) - 1) * q
    f = math.floor(k)
    c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def boot(rows, fn, seed=7):
    """rows: list of (score, label). fn(rows) -> value. Returns (point, lo, hi)."""
    rng = random.Random(seed)
    point = fn(rows)
    vals = []
    for _ in range(B):
        s = [rows[rng.randrange(len(rows))] for _ in rows]
        v = fn(s)
        if v is not None:
            vals.append(v)
    return point, pct(vals, 0.025), pct(vals, 0.975)


def f_auc(rows):
    return auc([s for s, l in rows if l == 1], [s for s, l in rows if l == 0])


def f_removed(t):
    def f(rows):
        neg = [s for s, l in rows if l == 0]
        return sum(s < t for s in neg) / len(neg) if neg else None
    return f


def f_lost(t):
    def f(rows):
        pos = [s for s, l in rows if l == 1]
        return sum(s < t for s in pos) / len(pos) if pos else None
    return f


def paired_auc_diff(sa, sb, labs, seed=11):
    """sa, sb: dict id->score; labs: id->label. Bootstrap CI of AUC(a) - AUC(b)."""
    ids = sorted(set(sa) & set(sb) & set(labs))
    rng = random.Random(seed)

    def d(sample):
        a = f_auc([(sa[i], labs[i]) for i in sample])
        b = f_auc([(sb[i], labs[i]) for i in sample])
        return None if a is None or b is None else a - b
    point = d(ids)
    vals = [v for v in (d([ids[rng.randrange(len(ids))] for _ in ids]) for _ in range(B)) if v is not None]
    return point, pct(vals, 0.025), pct(vals, 0.975)


def shuffled_auc(rows, seed=3):
    rng = random.Random(seed)
    s = [r[0] for r in rows]
    labs = [r[1] for r in rows]
    vals = []
    for _ in range(B):
        rng.shuffle(labs)
        vals.append(f_auc(list(zip(s, labs))))
    return sum(vals) / len(vals), pct(vals, 0.025), pct(vals, 0.975)


def calibration(rows, nbins=10):
    rows = sorted(rows)
    out = []
    for b in range(nbins):
        chunk = rows[b * len(rows) // nbins:(b + 1) * len(rows) // nbins]
        if chunk:
            out.append({"decile": b + 1, "n": len(chunk), "p_min": chunk[0][0], "p_max": chunk[-1][0],
                        "mean_p": sum(s for s, _ in chunk) / len(chunk),
                        "rate": sum(l for _, l in chunk) / len(chunk)})
    return out


# ------------------------------------------------------------------ word-overlap baseline

STOP = set("the a an and or of to in on for is are be it this that with as at by from i you we was were "
           "not no do does did can will would should so if then than but have has had my your our its "
           "me they them he she what which who how when where why all any just also into out up about".split())


def toks(text):
    return [w for w in re.findall(r"[a-z0-9]+", text.lower()) if len(w) > 1 and w not in STOP]


def way_corpus():
    docs = {}
    for d, _, _ in os.walk(ROOT, followlinks=True):
        rel = os.path.relpath(d, ROOT)
        f = Path(d) / (os.path.basename(d) + ".md")
        if rel == "." or not f.exists():
            continue
        m = re.match(r"---\n(.*?)\n---", f.read_text(), re.S)
        if not m:
            continue
        fm = m.group(1)
        desc = re.search(r"^description:\s*(.*)$", fm, re.M)
        voc = re.search(r"^vocabulary:\s*(.*)$", fm, re.M)
        docs[rel] = (desc.group(1) if desc else "") + " " + (voc.group(1) if voc else "")
    return docs


class BM25:
    def __init__(self, docs, k1=1.2, b=0.75):
        self.k1, self.b = k1, b
        self.tok = {k: toks(v) for k, v in docs.items()}
        self.avg = sum(len(t) for t in self.tok.values()) / len(self.tok)
        df = Counter(w for t in self.tok.values() for w in set(t))
        n = len(self.tok)
        self.idf = {w: math.log(1 + (n - c + 0.5) / (c + 0.5)) for w, c in df.items()}

    def score(self, query, doc_text):
        d = toks(doc_text)
        tf = Counter(d)
        s = 0.0
        for w in set(toks(query)):
            if w in tf:
                f = tf[w]
                s += self.idf.get(w, math.log(1 + len(self.tok))) * f * (self.k1 + 1) / (
                    f + self.k1 * (1 - self.b + self.b * len(d) / self.avg))
        return s


def jaccard(a, b):
    a, b = set(toks(a)), set(toks(b))
    return len(a & b) / len(a | b) if a | b else 0.0


# ------------------------------------------------------------------ main

def main():
    labs = labels()
    units = {f"{u['set']}|{u['id']}|{u['wv']}|{u['cv']}": u for u in load(P / "units.jsonl")}
    meta = json.load(open(P / "ways_meta.json"))["ways"]
    matcher = json.load(open(P / "matcher_scores.json"))
    scores = {"local": {}, "haiku": {}}
    ms = {"local": {}, "haiku": {}}
    for backend, fname in (("local", "local_scores.jsonl"), ("haiku", "haiku_scores.jsonl")):
        if not (P / fname).exists():
            continue
        for r in load(P / fname):
            if r["p_yes"] is None:
                continue
            scores[backend][r["key"]] = r["p_yes"]
            ms[backend][r["key"]] = r["ms"]

    # results.jsonl: every scored row
    with open(P / "results.jsonl", "w") as f:
        for backend in scores:
            for k, p in scores[backend].items():
                u = units[k]
                lab = labs[(u["set"], u["id"])]
                f.write(json.dumps({"backend": backend, "set": u["set"], "id": u["id"], "way_id": u["way_id"],
                                    "wv": u["wv"], "cv": u["cv"], "p_yes": p, "ms": ms[backend][k], **lab}) + "\n")

    # score lookups: cfg -> {(set,id): p}
    by_cfg = defaultdict(dict)
    for backend in scores:
        for k, p in scores[backend].items():
            u = units[k]
            by_cfg[(backend, u["set"], u["wv"], u["cv"])][u["id"]] = p

    # evaluation subsets: name -> (set, id filter, label key)
    def s1_filter(cats):
        return lambda i: labs[("s1", i)]["cat"] in cats
    subsets = {
        "1a": ("s1", s1_filter({"real_relevant", "real_irrelevant"}), "label"),
        "1": ("s1", lambda i: labs[("s1", i)]["cat"] != "ambiguous", "label"),
        "2": ("s2", lambda i: True, "label"),
    }
    for lk in ("label_agent", "label_claude", "agree"):
        for lane in ("all", "prompt", "tool"):
            def flt(i, lane=lane, lk=lk):
                L = labs[("s3", i)]
                return (lane == "all" or L["cat"] == lane) and (lk != "agree" or L["agree"])
            subsets[f"3/{lk}/{lane}"] = ("s3", flt, "label_agent" if lk == "agree" else lk)

    def rows_for(cfg_scores, sub):
        sset, flt, lk = subsets[sub]
        return [(p, labs[(sset, i)][lk]) for i, p in sorted(cfg_scores.items()) if flt(i)]

    M = {"configs": {}, "controls": {}, "diffs": {}, "categories": {}, "calibration": {}, "latency": {},
         "human": [], "counts": {}}
    for sub, (sset, flt, lk) in subsets.items():
        ids = [i for (s, i) in labs if s == sset and flt(i)]
        M["counts"][sub] = {"n": len(ids), "pos": sum(labs[(sset, i)][lk] for i in ids)}

    for (backend, sset, wv, cv), sc in sorted(by_cfg.items()):
        name = f"{backend}:{wv}+{cv}"
        for sub, (ss, _, _) in subsets.items():
            if ss != sset:
                continue
            rows = rows_for(sc, sub)
            if not rows:
                continue
            ent = {"n": len(rows), "auc": boot(rows, f_auc), "sweep": {}}
            for t in THRESHOLDS:
                ent["sweep"][t] = {"removed": boot(rows, f_removed(t)), "lost": boot(rows, f_lost(t))}
            M["configs"][f"{name}|{sub}"] = ent

    # paired AUC differences, local, set 1 non-ambiguous and 1a
    for sub in ("1", "1a", "2"):
        sset, flt, lk = subsets[sub]
        L = {i: labs[(sset, i)][lk] for (s, i) in labs if s == sset and flt(i)}
        ref = by_cfg[("local", sset, "A", "T4")]
        for wv in WVS:
            for cv in CVS:
                if (wv, cv) != ("A", "T4"):
                    M["diffs"][f"local:{wv}+{cv} - A+T4|{sub}"] = paired_auc_diff(
                        by_cfg[("local", sset, wv, cv)], ref, L)
        # marginal: way variants pooled within each context, context pooled within way variant
        for cv in CVS:
            for wv in ("B", "C"):
                M["diffs"][f"local:{wv} - A @ {cv}|{sub}"] = paired_auc_diff(
                    by_cfg[("local", sset, wv, cv)], by_cfg[("local", sset, "A", cv)], L)
        for wv in WVS:
            for cv in ("T1", "TX", "TXG"):
                M["diffs"][f"local:{cv} - T4 @ {wv}|{sub}"] = paired_auc_diff(
                    by_cfg[("local", sset, wv, cv)], by_cfg[("local", sset, wv, "T4")], L)
        for wv, cv in [(w, c) for w in WVS for c in CVS]:
            h = by_cfg.get(("haiku", sset, wv, cv))
            if h:
                M["diffs"][f"haiku - local @ {wv}+{cv}|{sub}"] = paired_auc_diff(
                    h, by_cfg[("local", sset, wv, cv)], L)
    for lk in ("label_agent", "label_claude"):
        sub = f"3/{lk}/all"
        sset, flt, k = subsets[sub]
        L = {i: labs[(sset, i)][k] for (s, i) in labs if s == sset and flt(i)}
        for wv in ("B", "C"):
            M["diffs"][f"local:{wv} - A @ EX|{sub}"] = paired_auc_diff(
                by_cfg[("local", "s3", wv, "EX")], by_cfg[("local", "s3", "A", "EX")], L)
        h = by_cfg.get(("haiku", "s3", "A", "EX"))
        if h:
            M["diffs"][f"haiku - local @ A+EX|{sub}"] = paired_auc_diff(h, by_cfg[("local", "s3", "A", "EX")], L)

    # controls
    bm = BM25(way_corpus())
    items = {("s1", it["id"]): it for it in load(DATA / "eval.jsonl")}
    items.update({("s2", it["id"]): it for it in load(DATA / "random40.jsonl")})
    items.update({("s3", it["id"]): it for it in load(DATA / "session_unlabelled.jsonl")})
    for sub, (sset, flt, lk) in subsets.items():
        cvs = ["EX"] if sset == "s3" else CVS
        ref = by_cfg.get(("local", sset, "A", "EX" if sset == "s3" else "T4"), {})
        rows = rows_for(ref, sub)
        if rows:
            M["controls"][f"shuffled|{sub}"] = shuffled_auc(rows)
        for cv in cvs:
            bmr, jr = [], []
            for (s, i), it in items.items():
                if s != sset or not flt(i):
                    continue
                q = units[f"{sset}|{i}|A|{cv}"]["query"]
                wtext = it["summary"] + " " + (it.get("vocabulary") or meta[it["way_id"]]["vocabulary"])
                bmr.append((bm.score(q, wtext), labs[(s, i)][lk]))
                jr.append((jaccard(q, wtext), labs[(s, i)][lk]))
            M["controls"][f"bm25:{cv}|{sub}"] = boot(bmr, f_auc)
            M["controls"][f"jaccard:{cv}|{sub}"] = boot(jr, f_auc)
        # current matcher's fire_score, on the scored subset, beside the judge on the same subset
        mr, jr = [], []
        for (s, i) in labs:
            if s != sset or not flt(i):
                continue
            fs = matcher.get(i, {}).get("fire_score")
            if fs is None or i not in ref:
                continue
            mr.append((fs, labs[(s, i)][lk]))
            jr.append((ref[i], labs[(s, i)][lk]))
        if mr:
            M["controls"][f"matcher_fire_score|{sub}"] = {"n": len(mr), "pos": sum(l for _, l in mr),
                                                          "matcher": boot(mr, f_auc),
                                                          "judge_ref_same_items": boot(jr, f_auc)}

    # categories at best threshold (Youden on 1a), and calibration
    for (backend, sset, wv, cv), sc in by_cfg.items():
        if sset != "s1":
            continue
        name = f"{backend}:{wv}+{cv}"
        r1a = rows_for(sc, "1a")
        best = max(THRESHOLDS, key=lambda t: f_removed(t)(r1a) - f_lost(t)(r1a))
        cats = defaultdict(lambda: [0, 0])
        for i, p in sc.items():
            L = labs[("s1", i)]
            cats[L["cat"]][0] += int((p >= best) == (L["label"] == 1))
            cats[L["cat"]][1] += 1
        M["categories"][name] = {"best_t": best, "acc": {c: v[0] / v[1] for c, v in cats.items()},
                                 "n": {c: v[1] for c, v in cats.items()}}
        M["calibration"][name] = calibration(rows_for(sc, "1") + rows_for(by_cfg.get((backend, "s2", wv, cv), {}), "2"))

    # latency
    for backend in ms:
        per = defaultdict(list)
        for k, v in ms[backend].items():
            u = units[k]
            per[f"{u['set']}:{u['wv']}+{u['cv']}"].append(v)
        M["latency"][backend] = {c: {"n": len(v), "p50": pct(v, .5), "p95": pct(v, .95), "sum_s": sum(v) / 1000}
                                 for c, v in sorted(per.items())}
        M["latency"][backend + "_total_s"] = sum(ms[backend].values()) / 1000

    # human labels: last row per id counts
    human = {}
    for r in load(P / "human_labels.jsonl"):
        human[r["id"]] = r
    for i, r in human.items():
        row = {"id": i, "human": r["human_label"], "agent": labs[("s1", i)]["label"], "cat": labs[("s1", i)]["cat"]}
        for (backend, sset, wv, cv), sc in by_cfg.items():
            if sset == "s1" and i in sc:
                row[f"{backend}:{wv}+{cv}"] = sc[i]
        M["human"].append(row)

    json.dump(M, open(P / "metrics.json", "w"), indent=1, default=str)
    print("ok", len(M["configs"]), "config-subset entries")


if __name__ == "__main__":
    main()
