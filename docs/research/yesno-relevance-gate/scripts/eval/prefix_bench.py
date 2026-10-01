#!/usr/bin/env python3
"""Gate latency per prompt: k candidates against one context, with and without shared-prefix reuse.

The rendered prompt is system + instruction + Query (turns) + Document (way), so every
candidate for one prompt shares everything up to the Document. With cache_prompt the
single llama-server slot keeps the previous prompt's KV and reprocesses only the suffix.

Modes, per (context, k):
  nocache   k sequential /rerank requests, cache_prompt=false
  cache     k sequential /rerank requests, cache_prompt=true
  batch     one /rerank request holding k documents, cache_prompt=true
Before each trial an unrelated prompt is scored, so the first candidate always pays the full
context (as a new user prompt would). Writes prefix_bench.json.
"""
import json
import random
import statistics
import sys
import time
import urllib.request
from pathlib import Path

P = Path(__file__).resolve().parent
sys.path.insert(0, str(P.parent / "judge"))
import judge  # noqa: E402

URL = "http://127.0.0.1:18341/rerank"
KS = [1, 2, 4, 6]
N_CONTEXTS = 6
REPS = 3


def rerank(docs, cache):
    body = json.dumps({"query": "", "documents": docs, "cache_prompt": cache}).encode()
    req = urllib.request.Request(URL, data=body, headers={"Content-Type": "application/json"})
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=300) as r:
        out = json.load(r)
    ms = (time.perf_counter() - t0) * 1000
    s = [None] * len(docs)
    for res in out["results"]:
        s[res["index"]] = res["relevance_score"]
    return s, ms


def prompt(query, doc):
    return judge.render_qwen({"instruction": judge.INSTRUCTIONS["default"], "turns": query, "summary": doc},
                             "turns-query")


def main():
    rng = random.Random(5)
    units = [json.loads(l) for l in open(P / "units.jsonl")]
    docs_b = sorted({u["doc"] for u in units if u["wv"] == "B"})
    out = {}
    for cv in ("T1", "T4", "TX"):
        queries = sorted({u["query"] for u in units if u["set"] == "s1" and u["cv"] == cv}, key=len)
        mid = len(queries) // 2
        ctxs = queries[mid - N_CONTEXTS // 2: mid + N_CONTEXTS // 2]  # median-length contexts
        res = {m: {k: [] for k in KS} for m in ("nocache", "cache", "batch")}
        maxdiff = 0.0
        for q in ctxs:
            cands = rng.sample(docs_b, max(KS))
            for _ in range(REPS):
                for k in KS:
                    ps = [prompt(q, d) for d in cands[:k]]
                    ref = None
                    for mode in ("nocache", "cache", "batch"):
                        rerank([prompt("unrelated warmup context " + str(rng.random()), "x")], False)
                        if mode == "batch":
                            s, ms = rerank(ps, True)
                        else:
                            s, ms = [], 0.0
                            for p in ps:
                                si, mi = rerank([p], mode == "cache")
                                s += si
                                ms += mi
                        res[mode][k].append(ms)
                        if ref is None:
                            ref = s
                        maxdiff = max(maxdiff, max(abs(a - b) for a, b in zip(ref, s)))
        out[cv] = {"median_chars": statistics.median(len(q) for q in ctxs),
                   "max_abs_p_diff_vs_nocache": maxdiff,
                   **{m: {k: {"p50": statistics.median(v), "p90": sorted(v)[int(0.9 * len(v))]}
                          for k, v in r.items()} for m, r in res.items()}}
        print(cv, json.dumps(out[cv]), flush=True)
    json.dump(out, open(P / "prefix_bench.json", "w"), indent=1)


if __name__ == "__main__":
    main()
