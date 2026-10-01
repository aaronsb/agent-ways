#!/usr/bin/env python3
"""Score chosen configurations with Haiku 4.5, under a hard cap of 800 API calls in total.

usage: score_haiku.py SET:WV:CV [SET:WV:CV ...]     e.g. s1:A:T4 s2:A:T4
Every attempted call increments haiku_calls.json before it is sent. Resumable.
"""
import json
import os
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

P = Path(__file__).resolve().parent
sys.path.insert(0, str(P.parent / "judge"))
import judge  # noqa: E402

CAP = 800
OUT = P / "haiku_scores.jsonl"
COUNTER = P / "haiku_calls.json"
KEY_FILE = os.environ.get("WAYS_JUDGE_KEY_FILE", os.path.expanduser("~/.config/agent-ways/keys/anthropic"))
lock = threading.Lock()


def take_call():
    with lock:
        n = json.loads(COUNTER.read_text())["calls"] if COUNTER.exists() else 0
        if n >= CAP:
            return False
        COUNTER.write_text(json.dumps({"calls": n + 1}))
        return True


def main():
    configs = [tuple(c.split(":")) for c in sys.argv[1:]]
    units = [json.loads(l) for l in open(P / "units.jsonl")]
    done = {json.loads(l)["key"] for l in open(OUT)} if OUT.exists() else set()
    todo = [u for u in units if (u["set"], u["wv"], u["cv"]) in configs
            and f"{u['set']}|{u['id']}|{u['wv']}|{u['cv']}" not in done]
    used = json.loads(COUNTER.read_text())["calls"] if COUNTER.exists() else 0
    if used + len(todo) > CAP:
        raise SystemExit(f"{len(todo)} calls would exceed the cap ({used} used of {CAP})")
    client = judge.make_haiku_client(KEY_FILE)

    def one(u):
        parts = {"instruction": judge.INSTRUCTIONS["default"], "summary": u["doc"], "turns": u["query"]}
        prompt = judge.render_plain(parts)
        p, err, ms = None, "budget", 0.0
        for _ in range(2):  # one retry on error, each attempt counted
            if not take_call():
                break
            t0 = time.perf_counter()
            p, err = judge.haiku_one(client, prompt)
            ms = (time.perf_counter() - t0) * 1000
            if err is None:
                break
        return {"key": f"{u['set']}|{u['id']}|{u['wv']}|{u['cv']}", "p_yes": p, "ms": round(ms, 1), "error": err}

    with ThreadPoolExecutor(max_workers=6) as pool, open(OUT, "a") as f:
        for row in pool.map(one, todo):
            f.write(json.dumps(row) + "\n")
            f.flush()
    print("calls used:", json.loads(COUNTER.read_text())["calls"])


if __name__ == "__main__":
    main()
