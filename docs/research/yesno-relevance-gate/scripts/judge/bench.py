#!/usr/bin/env python3
"""Latency benchmark for the local backend. Writes bench_results.json and prints a table.

  bench.py [--threads 16] [--reps 3] [--skip-transformers] [--gpu]   (the ROCm stage segfaults; off by default)
"""
import argparse
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

import judge

HERE = Path(__file__).resolve().parent
ITEMS = [json.loads(l) for l in open(HERE / "bench_items.jsonl")]
SCOPES = [1, 2, 4]
CAP = 600


def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(q * (len(xs) - 1))))]


def prompts_for(items, n):
    return [judge.render_qwen(judge.render_parts(it, n, CAP), "turns-query") for it in items]


def bench_engine(j, reps):
    out = {}
    for n in SCOPES:
        ps = prompts_for(ITEMS, n)
        j.p_yes(ps[:2])  # warm-up
        lat = []
        for _ in range(reps):
            for p in ps:
                t0 = time.perf_counter()
                j.p_yes([p])
                lat.append((time.perf_counter() - t0) * 1000)
        groups = judge.group_by_turns(ITEMS, n, CAP)
        batch_ms, seq_ms = [], []
        for _ in range(reps):
            for g in groups:
                gp = prompts_for(g, n)
                t0 = time.perf_counter()
                j.p_yes(gp)
                batch_ms.append((time.perf_counter() - t0) * 1000)
                t0 = time.perf_counter()
                for p in gp:
                    j.p_yes([p])
                seq_ms.append((time.perf_counter() - t0) * 1000)
        ntok = [len(p) for p in ps]
        out[f"turns={n}"] = {
            "item_p50_ms": round(pct(lat, .5), 1), "item_p95_ms": round(pct(lat, .95), 1),
            "batch6_one_call_p50_ms": round(pct(batch_ms, .5), 1),
            "batch6_one_call_p95_ms": round(pct(batch_ms, .95), 1),
            "batch6_sequential_p50_ms": round(pct(seq_ms, .5), 1),
            "prompt_chars_mean": round(statistics.mean(ntok)),
            "n_item_samples": len(lat), "n_batch_samples": len(batch_ms),
        }
        print(f"  turns={n}: {out[f'turns={n}']}", file=sys.stderr)
    return out


def time_cmd(cmd, reps=3):
    ts = []
    for _ in range(reps):
        t0 = time.perf_counter()
        subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        ts.append((time.perf_counter() - t0) * 1000)
    return round(statistics.median(ts), 1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--threads", type=int, default=16)
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--skip-transformers", action="store_true")
    ap.add_argument("--gpu", action="store_true")
    a = ap.parse_args()
    res = {"threads": a.threads, "turn_cap_chars": CAP, "n_items": len(ITEMS)}

    # one-shot costs: a fresh process per hook call
    six = HERE / "runs" / "six.jsonl"
    six.write_text("".join(json.dumps(it) + "\n" for it in ITEMS[:6]))
    py = str(HERE / "venv" / "bin" / "python")
    res["oneshot"] = {
        "python_import_torch_transformers_ms": time_cmd([py, "-c", "import torch, transformers.models.qwen3"]),
        "judge_cli_transformers_6items_ms": time_cmd([py, str(HERE / "judge.py"), "--backend", "local",
            "--engine", "transformers", "--threads", str(a.threads), "--turns", "2", "--max-turn-chars", str(CAP),
            "--batch", "--in", str(six), "--out", "/dev/null"]),
        "judge_cli_llamacpp_6items_ms": time_cmd([py, str(HERE / "judge.py"), "--backend", "local",
            "--engine", "llamacpp", "--threads", str(a.threads), "--turns", "2", "--max-turn-chars", str(CAP),
            "--batch", "--in", str(six), "--out", "/dev/null"]),
        "llama_embedding_1item_ms": time_cmd([str(HERE / "build-cpu" / "bin" / "llama-embedding"),
            "-m", str(judge.DEFAULT_GGUF), "--pooling", "rank", "-t", str(a.threads), "--embd-normalize", "-1",
            "-c", "4096", "-b", "4096", "-ub", "4096",
            "-p", prompts_for(ITEMS[:1], 2)[0]]),
    }
    print("oneshot", res["oneshot"], file=sys.stderr)

    if not a.skip_transformers:
        t0 = time.perf_counter()
        import torch, transformers  # noqa: F401
        imp = (time.perf_counter() - t0) * 1000
        tj = judge.TransformersJudge(judge.DEFAULT_HF, a.threads)
        print("transformers", file=sys.stderr)
        res["transformers_cpu_fp32"] = {"import_ms": round(imp, 1), "load_ms": round(tj.load_ms, 1),
                                        **bench_engine(tj, a.reps)}
        del tj

    for name, gguf, ngl, binary in [
        ("llamacpp_cpu_f16", HERE / "gguf/qwen3-reranker-0.6b-f16-raw.gguf", 0, HERE / "build-cpu/bin/llama-server"),
        ("llamacpp_cpu_f32", HERE / "gguf/qwen3-reranker-0.6b-f32-raw.gguf", 0, HERE / "build-cpu/bin/llama-server"),
        ("llamacpp_rocm_f16", HERE / "gguf/qwen3-reranker-0.6b-f16-raw.gguf", 99, HERE / "build-rocm/bin/llama-server"),
    ]:
        if ngl and not a.gpu:
            continue
        print(name, file=sys.stderr)
        lj = judge.LlamaCppJudge(None, gguf, binary, a.threads, ngl)
        try:
            res[name] = {"spawn_to_ready_ms": round(lj.load_ms, 1), **bench_engine(lj, a.reps)}
        finally:
            lj.close()

    (HERE / "bench_results.json").write_text(json.dumps(res, indent=2))
    print(json.dumps(res, indent=2))


if __name__ == "__main__":
    main()
