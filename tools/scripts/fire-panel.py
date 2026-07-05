#!/usr/bin/env python3
"""Measure semantic fire-breadth: one prompt scored against ALL ways (ADR-158).

The inverse of probe-measure.py. probe-measure scores many probes against ONE
way; this scores ONE prompt against EVERY corpus alias and counts how many clear
the semantic fire bar g(cos) >= tau_s. That count is the fire-breadth — the thing
that felt wrong as "nearly every way triggered."

For each panel prompt it reports the fire count and the top ways by probability,
grouped by bucket with the bucket's expectation, so a calibration refit can be
judged: off_topic must stay ~0, narrow/adjacent must come DOWN, broad may stay
high. Point --cache-dir at an un-deployed corpus build to compare before/after.

    fire-panel.py                          # default panel, canonical corpus
    fire-panel.py --panel path.json        # custom panel
    fire-panel.py --cache-dir DIR          # score against a refit corpus build
    fire-panel.py --prompt "..." --prompt "..."   # ad-hoc prompts (bucket "adhoc")
    fire-panel.py --top 8                   # show more ways per prompt
    fire-panel.py --json                    # machine-readable summary

Caveat: measures RAW semantic fires at tau_s. It does NOT model parent-boost
(which only LOWERS a child's bar when its parent fired), so real fire counts are
>= these. That makes it a sound LOWER BOUND and a consistent before/after proxy.
"""
import argparse, json, math, subprocess, sys
from pathlib import Path

HOME = Path.home()
BIN_CANDIDATES = [
    HOME / ".claude/bin/way-embed",
    HOME / ".local/share/agent-ways/bin/way-embed",
    HOME / ".cache/agent-ways/user/way-embed",
]
CACHE = HOME / ".cache/agent-ways/user"
CORPUS = CACHE / "ways-corpus-en.jsonl"
MANIFEST = CACHE / "embed-manifest.json"
EN_MODEL = CACHE / "minilm-l6-v2.gguf"
TAU_S = 0.5
DEFAULT_PANEL = Path(__file__).with_name("fire-panel.json")


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(2)


def find_bin():
    for c in BIN_CANDIDATES:
        if c.exists():
            return c
    die("way-embed binary not found")


def load_aliases():
    if not CORPUS.exists():
        die(f"corpus missing: {CORPUS}")
    aliases = []
    for line in CORPUS.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            v = json.loads(line)
        except json.JSONDecodeError:
            continue
        wid = v.get("id")
        if not wid:
            continue
        alias = f"{v.get('description', '')} {v.get('vocabulary', '')}".strip()
        aliases.append((wid, alias))
    if not aliases:
        die("no aliases in corpus")
    return aliases


def load_en_calibration():
    if not MANIFEST.exists():
        die(f"manifest missing: {MANIFEST}")
    cal = json.loads(MANIFEST.read_text()).get("calibration") or {}
    if "en" not in cal:
        die("no EN calibration in manifest — corpus not fit")
    return cal["en"]


def sigmoid(x):
    if x < -60:
        return 0.0
    if x > 60:
        return 1.0
    return 1.0 / (1.0 + math.exp(-x))


def batch_similarity(bin_path, prompt, aliases):
    """One cosine per alias for a single prompt (order preserved)."""
    clean = lambda s: s.replace("\t", " ").replace("\n", " ")
    lines = [f"{clean(prompt)}\t{clean(a)}" for _, a in aliases]
    proc = subprocess.run(
        [str(bin_path), "similarity", "--model", str(EN_MODEL), "--batch"],
        input="\n".join(lines) + "\n", capture_output=True, text=True,
    )
    if proc.returncode != 0:
        die(f"way-embed failed: {proc.stderr}")
    out = [l.strip() for l in proc.stdout.splitlines() if l.strip()]
    if len(out) != len(aliases):
        die(f"expected {len(aliases)} cosines, got {len(out)}")
    return [float(x) for x in out]


def score_prompt(bin_path, prompt, aliases, en):
    cosines = batch_similarity(bin_path, prompt, aliases)
    fired = []
    for (wid, _), s in zip(aliases, cosines):
        g = sigmoid(en["a"] * s + en["b"])
        if g >= TAU_S:
            fired.append((wid, s, g))
    fired.sort(key=lambda t: t[2], reverse=True)
    return fired


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--panel", default=str(DEFAULT_PANEL))
    ap.add_argument("--prompt", action="append", default=[], help="ad-hoc prompt (repeatable)")
    ap.add_argument("--cache-dir", help="dir with ways-corpus-en.jsonl + embed-manifest.json")
    ap.add_argument("--top", type=int, default=6)
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    global CORPUS, MANIFEST
    if args.cache_dir:
        d = Path(args.cache_dir)
        CORPUS, MANIFEST = d / "ways-corpus-en.jsonl", d / "embed-manifest.json"

    if args.prompt:
        buckets = {"adhoc": args.prompt}
    else:
        buckets = json.loads(Path(args.panel).read_text()).get("buckets", {})

    bin_path, aliases, en = find_bin(), load_aliases(), load_en_calibration()
    a, b = en["a"], en["b"]
    cos_bar = (math.log(TAU_S / (1 - TAU_S)) - b) / a if a > 0 else float("nan")

    results = {}
    for bucket, prompts in buckets.items():
        results[bucket] = []
        for p in prompts:
            fired = score_prompt(bin_path, p, aliases, en)
            results[bucket].append({"prompt": p, "count": len(fired),
                                    "ways": [(w, round(s, 3), round(g, 4)) for w, s, g in fired]})

    if args.json:
        print(json.dumps({"calibration": {"a": a, "b": b, "cos_bar_tau_s": cos_bar},
                          "total_ways": len(aliases), "results": results}, indent=1))
        return

    print(f"# corpus: {len(aliases)} ways | EN a={a:.3f} b={b:.3f} | "
          f"tau_s={TAU_S} fires at cos>={cos_bar:.3f}")
    for bucket, rows in results.items():
        counts = [r["count"] for r in rows]
        avg = sum(counts) / len(counts) if counts else 0
        print(f"\n=== {bucket}  (avg {avg:.1f} ways/prompt, max {max(counts) if counts else 0}) ===")
        for r in rows:
            print(f"  [{r['count']:2}] {r['prompt'][:64]}")
            for w, s, g in r["ways"][:args.top]:
                print(f"        {g:6.4f}  cos={s:.3f}  {w}")
            if len(r["ways"]) > args.top:
                print(f"        … +{len(r['ways']) - args.top} more")


if __name__ == "__main__":
    main()
