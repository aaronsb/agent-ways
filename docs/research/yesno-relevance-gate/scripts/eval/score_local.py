#!/usr/bin/env python3
"""Score every unit with the resident llama-server (Qwen3-Reranker-0.6B f16, -t 8).

One item per request, turns-query orientation, default instruction. Resumable:
units already in local_scores.jsonl are skipped.
"""
import json
import sys
import time
from pathlib import Path

P = Path(__file__).resolve().parent
sys.path.insert(0, str(P.parent / "judge"))
import judge  # noqa: E402

URL = "http://127.0.0.1:18341"
OUT = P / "local_scores.jsonl"


def key(u):
    return f"{u['set']}|{u['id']}|{u['wv']}|{u['cv']}"


def main():
    units = [json.loads(l) for l in open(P / "units.jsonl")]
    done = set()
    if OUT.exists():
        done = {json.loads(l)["key"] for l in open(OUT)}
    j = judge.LlamaCppJudge(url=URL)
    j.p_yes([judge.render_qwen({"instruction": judge.INSTRUCTION, "turns": "warm", "summary": "up"},
                               "turns-query")])
    with open(OUT, "a") as f:
        for n, u in enumerate(units):
            if key(u) in done:
                continue
            parts = {"instruction": judge.INSTRUCTIONS["default"], "summary": u["doc"], "turns": u["query"]}
            prompt = judge.render_qwen(parts, "turns-query")
            t0 = time.perf_counter()
            try:
                p, err = j.p_yes([prompt])[0], None
            except Exception as e:
                p, err = None, f"{type(e).__name__}: {e}"
            ms = (time.perf_counter() - t0) * 1000
            f.write(json.dumps({"key": key(u), "p_yes": p, "ms": round(ms, 2), "error": err}) + "\n")
            f.flush()
            if n % 200 == 0:
                print(n, len(units), file=sys.stderr, flush=True)


if __name__ == "__main__":
    main()
