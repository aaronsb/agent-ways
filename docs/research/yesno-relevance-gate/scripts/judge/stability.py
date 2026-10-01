#!/usr/bin/env python3
"""Repeatability, wording sensitivity, and Haiku latency/agreement, driven through judge.py.

  stability.py --backend local --engine llamacpp|transformers
  stability.py --backend haiku --api-key-file PATH

Item sets (all at --turns 4 --max-turn-chars 600):
  COMPARE  first 4 conversations x 6 ways = 24 items
  REPEAT   10 of COMPARE, run 5 times on byte-identical input
  WORDING  20 of COMPARE, run with --instruction alt (default comes from COMPARE)
Haiku call count: 24 + 12 (two 6-way groups, concurrent) + 50 + 20 = 106.
"""
import argparse
import json
import statistics
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUNS = HERE / "runs" / "stability"
ITEMS = [json.loads(l) for l in open(HERE / "bench_items.jsonl")]
COMPARE = ITEMS[:24]
REPEAT_IDS = [
    "db-migration/migrations", "db-migration/incident", "db-migration/adr",
    "api-versioning/api", "api-versioning/adr", "api-versioning/email",
    "readme-onboarding/readme", "readme-onboarding/commits",
    "calendar-assistant/calendar", "calendar-assistant/email",
]
REPEAT = [it for it in COMPARE if it["id"] in REPEAT_IDS]
WORDING = COMPARE[:20]


def write(name, items):
    p = RUNS / f"{name}.in.jsonl"
    p.write_text("".join(json.dumps(it) + "\n" for it in items))
    return p


def run(tag, items, base, extra=()):
    inp = write(tag, items)
    out = RUNS / f"{tag}.out.jsonl"
    cmd = [str(HERE / "venv/bin/python"), str(HERE / "judge.py"), *base,
           "--turns", "4", "--max-turn-chars", "600", "--in", str(inp), "--out", str(out), *extra]
    subprocess.run(cmd, check=True, stderr=subprocess.DEVNULL)
    return {r["id"]: r for r in map(json.loads, open(out))}


def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(q * (len(xs) - 1))))]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--backend", choices=["local", "haiku"], required=True)
    ap.add_argument("--engine", default="llamacpp")
    ap.add_argument("--api-key-file", default=None)
    a = ap.parse_args()
    RUNS.mkdir(parents=True, exist_ok=True)
    name = a.backend if a.backend == "haiku" else f"local-{a.engine}"
    base = ["--backend", a.backend]
    if a.backend == "local":
        base += ["--engine", a.engine, "--threads", "16"]
    elif a.api_key_file:
        base += ["--api-key-file", a.api_key_file]
    rep = {"backend": name}

    compare = run(f"{name}-compare", COMPARE, base)
    rep["errors_compare"] = sum(r["error"] is not None for r in compare.values())

    # repeatability: 5 runs on identical input
    reps = [run(f"{name}-repeat{k}", REPEAT, base) for k in range(5)]
    spreads, flips = [], 0
    for it in REPEAT:
        ps = [r[it["id"]]["p_yes"] for r in reps]
        if None in ps:
            continue
        spreads.append(max(ps) - min(ps))
        flips += len({p >= 0.5 for p in ps}) > 1
    rep["repeat"] = {"items": len(REPEAT), "runs": 5, "items_with_flip": flips,
                     "max_p_spread": max(spreads), "mean_p_spread": statistics.mean(spreads),
                     "errors": sum(r[i]["error"] is not None for r in reps for i in r)}

    # wording sensitivity: alt instruction vs default on the same items
    alt = run(f"{name}-alt", WORDING, base, ["--instruction", "alt"])
    pairs = [(compare[i["id"]]["p_yes"], alt[i["id"]]["p_yes"]) for i in WORDING]
    pairs = [(x, y) for x, y in pairs if x is not None and y is not None]
    rep["wording"] = {"items": len(pairs),
                      "flips": sum((x >= .5) != (y >= .5) for x, y in pairs),
                      "mean_abs_shift": statistics.mean(abs(x - y) for x, y in pairs),
                      "max_abs_shift": max(abs(x - y) for x, y in pairs)}

    if a.backend == "haiku":
        lat = [r["ms"] for r in compare.values()] + [r[i]["ms"] for r in reps for i in r]
        rep["latency_per_item"] = {"n": len(lat), "p50_ms": round(pct(lat, .5), 1),
                                   "p95_ms": round(pct(lat, .95), 1)}
        # 6-way batch: sequential = sum of the compare run's per-item ms per conversation;
        # concurrent = one judge.py run of the group at --concurrency 6, wall clock.
        groups = [COMPARE[0:6], COMPARE[6:12]]
        rep["batch6_sequential_ms"] = [round(sum(compare[i["id"]]["ms"] for i in g), 1) for g in groups]
        conc = []
        for k, g in enumerate(groups):
            import time
            t0 = time.perf_counter()
            run(f"haiku-conc{k}", g, base, ["--concurrency", "6"])
            conc.append(round((time.perf_counter() - t0) * 1000, 1))
        rep["batch6_concurrent_wall_ms_incl_process"] = conc

    (RUNS / f"{name}-report.json").write_text(json.dumps(rep, indent=2))
    print(json.dumps(rep, indent=2))


if __name__ == "__main__":
    main()
