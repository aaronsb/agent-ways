#!/usr/bin/env python3
"""Admission recall of the late-interaction matcher: where expected ways fall out.

ADR-700 §10 found that late-interaction admission let through 302 of the 620
ways its multi-prompt surfaces were built to match. This script follows every
expected way through the pipeline and records the first stage that drops it,
then sweeps the admission operating points.

    OUT=/tmp/recall-out experiments/content-corpus/run.py GOLDEN.tsv [...]   # corpora first
    OUT=/tmp/recall-out experiments/content-corpus/recall.py GOLDEN.tsv [...]

Surfaces, the pipeline port and the confirm call come from confirm.py (a
verified port of tools/ways-cli/src/cmd/scan/late_interaction.rs). Stages, in
pipeline order:

  topk     the way is in no chunk's top K, so it gets no softmax mass and is
           never ranked (peak alone cannot admit it)
  gate     ranked, but share < SHARE_GATE and peak < PEAK_GATE
  cap      passed the co-gate, but cut by the survivor cap (survivors by peak)
  confirm  survived, but today's body confirm (won chunk vs chunk_body
           sentences, max) is below 0.35
  fired    confirmed

Share modes: `chunks` is shipped (sum of mass / n_chunks). `topics` divides by
the number of distinct per-chunk top-1 ways instead, so chunks that agree on a
winner count as one topic. `max` takes the way's mass in its best single chunk
(one chunk, one topic, no dilution).

Confirm scores depend only on (surface, way): the won chunk is the way's peak
chunk over all match rows, which no operating point changes. They are computed
once, in one batched `way-embed similarity` call, for every pair any setting
admits.
"""

import itertools
import json
import math
import random
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import confirm as C  # noqa: E402
import run  # noqa: E402

O = run.OUT
SEED = 11

SHIPPED = {"K": 8, "S": 0.15, "P": 0.50, "cap": 6, "mode": "chunks"}
SWEEP = {"K": [8, 12, 16], "S": [0.05, 0.10, 0.15], "P": [0.40, 0.45, 0.50],
         "cap": [6, 8, 10], "mode": ["chunks", "topics", "max", "top1", "top2"]}
# `max` lives on a per-chunk scale, where the shipped 0.15 on 2-3 chunks equals
# 0.30-0.45; it also gets those stricter gates. `top1`/`top2` ignore S.
S_BY_MODE = {"max": [0.05, 0.10, 0.15, 0.20, 0.25, 0.30, 0.40], "top1": [1.0], "top2": [1.0]}
STAGES = ["topk", "gate", "cap", "confirm", "fired"]
# the two operating points results-recall.md recommends; chosen after the sweep
PICKS = {"top1": {"K": 8, "S": 1.0, "P": 0.50, "cap": 6, "mode": "top1"},
         "max0.20": {"K": 8, "S": 0.20, "P": 0.50, "cap": 6, "mode": "max"}}


def norm(s):
    return " ".join(s.split())


# ── surfaces with chunk ownership ────────────────────────────────────────────

def own_confirm_surfaces(golden, rng):
    """confirm.py's 310 surfaces; each prompt is exactly one chunk there, so a
    chunk's owner is the golden row whose sentence it is."""
    by_sent = {norm(C.as_sentence(p)): w for p, w in golden}
    out = []
    for s in C.build_surfaces(golden, rng):
        ch = C.chunk_surface(s["surface"])
        owner = [by_sent.get(norm(c)) for c in ch]
        own = {w: [i for i, o in enumerate(owner) if o == w] for w in s["expected"]}
        assert all(own.values()), s["surface"]
        n_prompts = len(s["shape"].split("+"))
        out.append({"set": "main", "shape": s["shape"], "n_prompts": n_prompts,
                    "surface": s["surface"], "chunks": ch, "expected": s["expected"],
                    "own": own})
    return out


def multi_chunk_surfaces(golden, rng, per=15):
    """Auxiliary set: every golden prompt that splits into 2+ chunks on its own,
    alone and joined with 1 or 2 single-chunk targeted prompts from other areas.
    Ownership comes from chunking each part separately."""
    tgt = [g for g in golden if g[1] != "none"]
    multi = [g for g in tgt if len(C.chunk_surface(C.as_sentence(g[0]))) >= 2]
    single = [g for g in tgt if len(C.chunk_surface(C.as_sentence(g[0]))) == 1]
    out, seen = [], set()

    def add(parts, shape):
        surface = " ".join(C.as_sentence(p[0]) for p in parts)
        if surface in seen or C.approx_tokens(surface) > C.BUDGET_PROMPT:
            return False
        own, flat = defaultdict(list), []
        for p in parts:
            pc = C.chunk_surface(C.as_sentence(p[0]))
            own[p[1]].extend(range(len(flat), len(flat) + len(pc)))
            flat.extend(pc)
        ch = C.chunk_surface(surface)
        if ch != flat:
            return False
        seen.add(surface)
        out.append({"set": "multi", "shape": shape, "n_prompts": len(parts),
                    "surface": surface, "chunks": ch,
                    "expected": sorted({p[1] for p in parts}), "own": dict(own)})
        return True

    for m in multi:
        add([m], "M")
        for extra, shape in ((1, "M+T"), (2, "M+T+T")):
            got = tries = 0
            while got < per and tries < per * 200:
                tries += 1
                ts = rng.sample(single, extra)
                ways = [m[1]] + [t[1] for t in ts]
                if len({C.area(w) for w in ways}) < len(ways):
                    continue
                if any(C.related(a, b) for i, a in enumerate(ways) for b in ways[i + 1:]):
                    continue
                parts = [m] + ts
                rng.shuffle(parts)
                got += add(parts, shape)
    return out, multi, single


# ── the pipeline, parameterised ──────────────────────────────────────────────

def aggregate(rows, K, mode):
    """late_interaction.rs aggregate with K and the share denominator exposed.
    Also returns the way's mass per chunk for the stage diagnostics."""
    peak, mass, per_chunk_mass = {}, defaultdict(float), defaultdict(dict)
    top1 = set()
    for ci, r in enumerate(rows):
        for wid, cos in r:
            if wid not in peak or cos > peak[wid][0]:
                peak[wid] = (cos, ci)
        top = r[:K]
        if top:
            top1.add(top[0][0])
        denom = sum(math.exp(c / C.SOFTMAX_TAU) for _, c in top)
        if denom > 0:
            for wid, cos in top:
                m = math.exp(cos / C.SOFTMAX_TAU) / denom
                mass[wid] += m
                per_chunk_mass[wid][ci] = m
    if mode == "chunks":
        n = max(len(rows), 1)
        share = {w: m / n for w, m in mass.items()}
    elif mode == "topics":
        n = max(len(top1), 1)
        share = {w: m / n for w, m in mass.items()}
    elif mode == "max":
        share = {w: max(per_chunk_mass[w].values()) for w in mass}
    elif mode in ("top1", "top2"):
        n = int(mode[-1])
        win = {x for r in rows for x, _ in r[:min(n, K)]}
        share = {w: 1.0 if w in win else 0.0 for w in mass}
    else:
        raise ValueError(mode)
    return [{"id": w, "peak": peak[w][0], "chunk": peak[w][1], "share": share[w]}
            for w in mass], peak


def evaluate(rows, K, S, P, cap, mode):
    ranked, peak = aggregate(rows, K, mode)
    passed = [r for r in ranked if r["share"] >= S or r["peak"] >= P]
    passed.sort(key=lambda r: -r["peak"])
    return ranked, passed, passed[:cap], peak


def stage_of(way, ranked, passed, admitted, conf):
    if way not in {r["id"] for r in ranked}:
        return "topk"
    if way not in {r["id"] for r in passed}:
        return "gate"
    if way not in {r["id"] for r in admitted}:
        return "cap"
    return "fired" if conf >= C.CONFIRM_GATE else "confirm"


def label(way, expected):
    if way in expected:
        return "relevant"
    if any(C.related(way, e) for e in expected):
        return "related"
    return "irrelevant"


# ── confirm scores, one batched call ─────────────────────────────────────────

def confirm_scores(pairs, surfaces, bodies):
    """(surface index, way, won chunk index) -> today's body_confirm score."""
    lines, spans, out = [], [], {}
    for key in sorted(pairs):
        si, wid, ci = key
        body = bodies.get(wid) or []
        if not body:
            out[key] = 0.0
            continue
        won = surfaces[si]["chunks"][ci]
        spans.append((key, len(lines), len(lines) + len(body)))
        lines.extend(f"{won}\t{b}" for b in body)
    p = subprocess.run([str(C.EMBED), "similarity", "--model", str(C.MODEL), "--batch"],
                       input="\n".join(lines) + "\n", text=True, capture_output=True,
                       check=True)
    sims = [float(x) for x in p.stdout.split()]
    assert len(sims) == len(lines), (len(sims), len(lines))
    for key, lo, hi in spans:
        out[key] = max(sims[lo:hi])
    return out


# ── main ─────────────────────────────────────────────────────────────────────

def main():
    alias = [json.loads(l) for l in (O / "alias.jsonl").read_text().splitlines()]
    ids = [a["id"] for a in alias]
    golden = C.load_golden(sys.argv[1:], set(ids))
    rng = random.Random(SEED)
    surfaces = own_confirm_surfaces(golden, rng)
    aux, multi, single = multi_chunk_surfaces(golden, random.Random(SEED + 2))
    n_one_chunk = sum(1 for p, _ in golden if len(C.chunk_surface(C.as_sentence(p))) == 1)
    print(f"{len(golden)} golden rows: {n_one_chunk} are one chunk alone (single-vector "
          f"path, late interaction never runs), {len(golden) - n_one_chunk} split into 2+")
    print(f"main surfaces {len(surfaces)} "
          f"({dict(Counter(s['shape'] for s in surfaces))}); "
          f"multi-chunk auxiliary surfaces {len(aux)} ({dict(Counter(s['shape'] for s in aux))})")
    allsurf = surfaces + aux

    chunks, spans = [], []
    for s in allsurf:
        spans.append((len(chunks), len(chunks) + len(s["chunks"])))
        chunks.extend(s["chunks"])
    per = C.match_batch(chunks)
    print(f"match rows per chunk: min {min(map(len, per))} max {max(map(len, per))}")
    for s, (lo, hi) in zip(allsurf, spans):
        s["rows"] = per[lo:hi]

    bodies = {w: C.chunk_body(run.way_file(w).read_text()) for w in ids}
    settings = [st for st in (dict(zip(SWEEP, v)) for v in itertools.product(*SWEEP.values()))
                if st["mode"] not in S_BY_MODE]
    for mode, svals in S_BY_MODE.items():
        settings += [{**dict(zip(SWEEP, v)), "mode": mode}
                     for v in itertools.product(SWEEP["K"], svals, SWEEP["P"], SWEEP["cap"],
                                                [mode])]
    assert SHIPPED in settings

    # pass 1: every (surface, way, won chunk) any setting admits
    pairs = set()
    results = {}
    for si, s in enumerate(allsurf):
        for st in settings:
            ranked, passed, adm, peak = evaluate(s["rows"], **st)
            results[(si, tuple(st.values()))] = (ranked, passed, adm, peak)
            pairs.update((si, r["id"], r["chunk"]) for r in adm)
    cache = O / "recall-confirm-cache.json"
    ck = lambda k: f"{allsurf[k[0]]['surface']}\t{k[1]}\t{k[2]}"  # noqa: E731
    old = json.loads(cache.read_text()) if cache.is_file() else {}
    todo = {k for k in pairs if ck(k) not in old}
    conf = {k: old[ck(k)] for k in pairs if ck(k) in old}
    if todo:
        conf.update(confirm_scores(todo, allsurf, bodies))
        cache.write_text(json.dumps({**old, **{ck(k): v for k, v in conf.items()}}))
    print(f"confirm pairs scored: {len(pairs)}")

    def cscore(si, r):
        return conf[(si, r["id"], r["chunk"])]

    def trace(st):
        key = tuple(st.values())
        traces = []
        for si, s in enumerate(allsurf):
            ranked, passed, adm, peak = results[(si, key)]
            rk = {r["id"]: r for r in ranked}
            for w in s["expected"]:
                c = cscore(si, rk[w]) if w in {r["id"] for r in adm} else None
                stg = stage_of(w, ranked, passed, adm, c if c is not None else 0)
                own = s["own"][w]
                own_rank = min(next((i for i, (x, _) in enumerate(s["rows"][ci]) if x == w), 999)
                               for ci in own) + 1
                pk, pci = peak.get(w, (0.0, -1))
                traces.append({"set": s["set"], "shape": s["shape"], "n_prompts": s["n_prompts"],
                               "own_chunks": len(own), "n_chunks": len(s["chunks"]),
                               "way": w, "stage": stg, "own_rank": own_rank, "peak": pk,
                               "peak_on_own": pci in own,
                               "share": rk[w]["share"] if w in rk else 0.0, "confirm": c})
        return traces

    def stage_rows(sel, title):
        print(f"\n{title}")
        print(f"  {'group':28} {'n':>4} " + " ".join(f"{x:>8}" for x in STAGES))
        groups = defaultdict(list)
        for t in sel:
            for g in t["_groups"]:
                groups[g].append(t)
        for g, ts in groups.items():
            c = Counter(t["stage"] for t in ts)
            print(f"  {g:28} {len(ts):4d} " +
                  " ".join(f"{c[x]:4d} {c[x]/len(ts):3.0%}".rjust(8) for x in STAGES))

    def stage_tables(traces, name):
        for t in traces:
            t["_groups"] = ["all", f"{t['n_prompts']} prompts", f"shape {t['shape']}"]
        stage_rows([t for t in traces if t["set"] == "main"],
                   f"stage where each expected way stops, {name}, main surfaces "
                   "(every prompt one chunk)")
        for t in traces:
            t["_groups"] = ["all",
                            f"own prompt {'1 chunk' if t['own_chunks'] == 1 else '2+ chunks'}",
                            f"shape {t['shape']}"]
        stage_rows([t for t in traces if t["set"] == "multi"],
                   f"{name}, auxiliary surfaces (contain one multi-chunk prompt)")

    traces = trace(SHIPPED)
    stage_tables(traces, "shipped point")

    main_tr = [t for t in traces if t["set"] == "main"]
    print("\nmain surfaces, expected way's rank in its own chunk vs stage:")
    bands = [(1, 1, "rank 1"), (2, 3, "rank 2-3"), (4, 8, "rank 4-8"), (9, 999, "rank 9+")]
    for lo, hi, name in bands:
        ts = [t for t in main_tr if lo <= t["own_rank"] <= hi]
        c = Counter(t["stage"] for t in ts)
        print(f"  {name:10} n={len(ts):4d}  " + "  ".join(f"{x} {c[x]}" for x in STAGES))
    for stg in ("gate", "cap", "confirm"):
        ts = [t for t in main_tr if t["stage"] == stg]
        if ts:
            print(f"  {stg}: mean peak {sum(t['peak'] for t in ts)/len(ts):.3f}, "
                  f"mean share {sum(t['share'] for t in ts)/len(ts):.3f}, "
                  f"peak on own chunk {sum(t['peak_on_own'] for t in ts)}/{len(ts)}")

    # ── sweep ──
    def metrics(st, which):
        key = tuple(st.values())
        exp = adm_rel = fired_rel = 0
        n_adm = n_fired = irr_adm = irr_fired = rel_adm = rel_fired = 0
        sel = [si for si, s in enumerate(allsurf) if s["set"] == which]
        for si in sel:
            s = allsurf[si]
            _, _, adm, _ = results[(si, key)]
            exp += len(s["expected"])
            for r in adm:
                lab = label(r["id"], s["expected"])
                f = cscore(si, r) >= C.CONFIRM_GATE
                n_adm += 1
                n_fired += f
                if lab == "relevant":
                    adm_rel += 1
                    fired_rel += f
                elif lab == "related":
                    rel_adm += 1
                    rel_fired += f
                else:
                    irr_adm += 1
                    irr_fired += f
        n = len(sel)
        return {**st, "exp": exp, "adm_rel": adm_rel, "fired_rel": fired_rel,
                "recall_adm": adm_rel / exp, "recall_fired": fired_rel / exp,
                "adm_per": n_adm / n, "fired_per": n_fired / n,
                "irr_adm_per": irr_adm / n, "irr_fired_per": irr_fired / n,
                "related_adm_per": rel_adm / n, "related_fired_per": rel_fired / n}

    rows = [metrics(st, "main") for st in settings]
    aux_rows = {tuple(st.values()): metrics(st, "multi") for st in settings}
    cols = list(SWEEP) + ["recall_adm", "recall_fired", "adm_per", "fired_per",
                          "irr_adm_per", "irr_fired_per", "related_adm_per", "related_fired_per"]
    with open(O / "recall-sweep.tsv", "w") as f:
        f.write("\t".join(cols + ["aux_recall_adm", "aux_recall_fired"]) + "\n")
        for r in rows:
            a = aux_rows[tuple(r[k] for k in SWEEP)]
            f.write("\t".join(f"{r[c]:.4f}" if isinstance(r[c], float) else str(r[c])
                              for c in cols) +
                    f"\t{a['recall_adm']:.4f}\t{a['recall_fired']:.4f}\n")

    def show(r, tag=""):
        a = aux_rows[tuple(r[k] for k in SWEEP)]
        sv = "S  -  " if r["mode"] in ("top1", "top2") else f"S{r['S']:.2f}"
        print(f"  K{r['K']:<3d} {sv} P{r['P']:.2f} cap{r['cap']:<3d} {r['mode']:7} "
              f"rec adm {r['recall_adm']:.3f} fired {r['recall_fired']:.3f}  "
              f"adm/surf {r['adm_per']:.2f} irr {r['irr_adm_per']:.2f}  "
              f"fired/surf {r['fired_per']:.2f} irr {r['irr_fired_per']:.2f} "
              f"related {r['related_fired_per']:.2f}  | aux {a['recall_adm']:.3f}/"
              f"{a['recall_fired']:.3f} {tag}")

    base = next(r for r in rows if all(r[k] == v for k, v in SHIPPED.items()))
    print(f"\nshipped point, main surfaces: admitted {base['adm_rel']}/{base['exp']}, "
          f"fired {base['fired_rel']}/{base['exp']}")
    show(base, "(shipped)")

    print("\none factor at a time from the shipped point:")
    for k, vals in SWEEP.items():
        for v in vals:
            if k == "mode" and v in ("top1", "top2"):
                continue
            if v == SHIPPED[k]:
                continue
            st = {**SHIPPED, k: v}
            show(next(r for r in rows if all(r[x] == st[x] for x in SWEEP)), f"({k}={v})")
    for mode, svals in S_BY_MODE.items():
        for v in svals:
            st = {**SHIPPED, "mode": mode, "S": v}
            show(next(r for r in rows if all(r[x] == st[x] for x in SWEEP)), f"(mode={mode})")

    print("\nPareto frontier, recall after confirm vs fired candidates per surface:")
    front = []
    for r in sorted(rows, key=lambda r: (r["fired_per"], -r["recall_fired"])):
        if not front or r["recall_fired"] > front[-1]["recall_fired"] + 1e-9:
            front.append(r)
    for r in front:
        show(r)

    print("\nPareto frontier, recall after confirm vs irrelevant fired per surface:")
    front = []
    for r in sorted(rows, key=lambda r: (r["irr_fired_per"], -r["recall_fired"])):
        if not front or r["recall_fired"] > front[-1]["recall_fired"] + 1e-9:
            front.append(r)
    for r in front:
        show(r)

    print("\nbest recall after confirm per share mode:")
    for mode in SWEEP["mode"]:
        r = max((r for r in rows if r["mode"] == mode),
                key=lambda r: (round(r["recall_fired"], 4), -r["irr_fired_per"]))
        show(r)

    print("\nrecommended operating points vs shipped, paired surface bootstrap (2000, 95%):")
    main_idx = [si for si, s in enumerate(allsurf) if s["set"] == "main"]

    def per_surface(st):
        key = tuple(st.values())
        out = []
        for si in main_idx:
            s = allsurf[si]
            adm = results[(si, key)][2]
            f = [r for r in adm if cscore(si, r) >= C.CONFIRM_GATE]
            out.append((sum(r["id"] in s["expected"] for r in f),
                        sum(label(r["id"], s["expected"]) == "irrelevant" for r in f),
                        len(s["expected"])))
        return np.array(out, dtype=float)

    rng = np.random.default_rng(SEED)
    b0 = per_surface(SHIPPED)
    for name, st in PICKS.items():
        b1 = per_surface(st)
        dr, di = [], []
        for _ in range(2000):
            ix = rng.integers(0, len(main_idx), len(main_idx))
            dr.append((b1[ix, 0].sum() - b0[ix, 0].sum()) / b0[ix, 2].sum())
            di.append((b1[ix, 1].sum() - b0[ix, 1].sum()) / len(ix))
        show(next(r for r in rows if all(r[x] == st[x] for x in SWEEP)), f"({name})")
        print(f"    recall after confirm {np.mean(dr):+.3f} "
              f"[{np.percentile(dr, 2.5):+.3f}, {np.percentile(dr, 97.5):+.3f}]; "
              f"irrelevant fired per surface {np.mean(di):+.2f} "
              f"[{np.percentile(di, 2.5):+.2f}, {np.percentile(di, 97.5):+.2f}]")
        stage_tables(trace(st), name)

    (O / "recall.json").write_text(json.dumps(
        {"traces": [{k: v for k, v in t.items() if k != "_groups"} for t in traces],
         "sweep": rows}, indent=1))


if __name__ == "__main__":
    main()
