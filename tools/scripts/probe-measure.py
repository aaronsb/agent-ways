#!/usr/bin/env python3
"""Measure probes through the LIVE ADR-156 calibration.

For each probe, computes cosine(probe, way_alias) via way-embed and maps it to a
relevance probability g(s)=sigmoid(a*s+b) using the deployed calibration in the
embed-manifest. The way alias is "{description} {vocabulary}" — the exact text
the corpus embedded and the calibration was fit against.

Usage:
    probe-measure.py <way-id> --intent "probe" "probe" ... --noise "probe" ...
    probe-measure.py <way-id> --json < probes.json   # {"intent":[...],"noise":[...]}

Emits per-probe cosine, g_en, g_multi, and lane verdict against the global
thresholds tau_s=0.5 (semantic fire) / tau_k=0.15 (keyword floor). Prints a
separation summary (intent vs noise) used to decide remove/anchor/bound.
"""
import argparse, json, math, os, subprocess, sys
from pathlib import Path

HOME = Path.home()
BIN_CANDIDATES = [
    HOME / ".claude/bin/way-embed",
    HOME / ".local/share/agent-ways/bin/way-embed",
    HOME / ".cache/agent-ways/user/way-embed",
]
CACHE = HOME / ".cache/agent-ways/user"
# Override with --cache-dir to score against an isolated corpus build (e.g. a
# `ways corpus --output <dir>` refit of an un-deployed branch). The model always
# lives in the canonical cache.
CORPUS = CACHE / "ways-corpus-en.jsonl"
MANIFEST = CACHE / "embed-manifest.json"
EN_MODEL = CACHE / "minilm-l6-v2.gguf"

TAU_S = 0.5   # semantic fire probability (config: semantic_fire_probability)
TAU_K = 0.15  # keyword floor probability (config: keyword_floor_probability)


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(2)


def find_bin():
    for c in BIN_CANDIDATES:
        if c.exists():
            return c
    die("way-embed binary not found")


def load_alias(way_id):
    if not CORPUS.exists():
        die(f"corpus missing: {CORPUS}")
    for line in CORPUS.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            v = json.loads(line)
        except json.JSONDecodeError:
            continue
        if v.get("id") == way_id:
            desc = v.get("description", "")
            vocab = v.get("vocabulary", "")
            return f"{desc} {vocab}".strip()
    die(f"way id not found in corpus: {way_id}")


def load_calibration():
    if not MANIFEST.exists():
        die(f"manifest missing: {MANIFEST}")
    cal = json.loads(MANIFEST.read_text()).get("calibration") or {}
    if "en" not in cal:
        die("no EN calibration in manifest — corpus not fit")
    return cal


def sigmoid(x):
    if x < -60:
        return 0.0
    if x > 60:
        return 1.0
    return 1.0 / (1.0 + math.exp(-x))


def batch_similarity(bin_path, alias, probes):
    """Return one cosine per probe (order preserved)."""
    lines = []
    for p in probes:
        p = p.replace("\t", " ").replace("\n", " ")
        lines.append(f"{p}\t{alias}")
    proc = subprocess.run(
        [str(bin_path), "similarity", "--model", str(EN_MODEL), "--batch"],
        input="\n".join(lines) + "\n",
        capture_output=True, text=True,
    )
    if proc.returncode != 0:
        die(f"way-embed failed: {proc.stderr}")
    out = [l.strip() for l in proc.stdout.splitlines() if l.strip()]
    if len(out) != len(probes):
        die(f"expected {len(probes)} cosines, got {len(out)}")
    return [float(x) for x in out]


def measure(bin_path, alias, cal, probes, label):
    if not probes:
        return []
    cosines = batch_similarity(bin_path, alias, probes)
    en = cal["en"]
    rows = []
    for p, s in zip(probes, cosines):
        g_en = sigmoid(en["a"] * s + en["b"])
        fires_sem = g_en >= TAU_S
        clears_floor = g_en >= TAU_K
        rows.append({
            "label": label, "probe": p, "cos": s, "g_en": g_en,
            "fires_sem": fires_sem, "clears_floor": clears_floor,
        })
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("way_id")
    ap.add_argument("--intent", nargs="*", default=[])
    ap.add_argument("--noise", nargs="*", default=[])
    ap.add_argument("--json", action="store_true", help="read {intent,noise} from stdin")
    ap.add_argument("--cache-dir", help="dir holding ways-corpus-en.jsonl + embed-manifest.json to use instead of the canonical cache")
    args = ap.parse_args()

    global CORPUS, MANIFEST
    if args.cache_dir:
        d = Path(args.cache_dir)
        CORPUS = d / "ways-corpus-en.jsonl"
        MANIFEST = d / "embed-manifest.json"

    intent, noise = args.intent, args.noise
    if args.json:
        data = json.load(sys.stdin)
        intent = data.get("intent", [])
        noise = data.get("noise", [])

    bin_path = find_bin()
    alias = load_alias(args.way_id)
    cal = load_calibration()

    rows = measure(bin_path, alias, cal, intent, "INTENT") + \
           measure(bin_path, alias, cal, noise, "NOISE")

    a, b = cal["en"]["a"], cal["en"]["b"]
    # cos at which g(s)=tau: solve a*s+b = logit(tau). Guard a<=0 (a degenerate
    # or missing fit that load_calibration let through) rather than dividing by 0.
    def cos_at(tau):
        return (math.log(tau / (1 - tau)) - b) / a if a > 0 else float("nan")
    print(f"# way: {args.way_id}")
    print(f"# alias: {alias[:120]}{'...' if len(alias) > 120 else ''}")
    print(f"# EN calibration: a={a:.3f} b={b:.3f} "
          f"(tau_s={TAU_S} -> cos>={cos_at(TAU_S):.3f}, tau_k={TAU_K} -> cos>={cos_at(TAU_K):.3f})")
    print(f"{'LABEL':7} {'cos':>6} {'g_en':>6} {'sem':>4} {'floor':>5}  probe")
    for r in rows:
        print(f"{r['label']:7} {r['cos']:6.3f} {r['g_en']:6.3f} "
              f"{'Y' if r['fires_sem'] else '·':>4} {'Y' if r['clears_floor'] else '·':>5}  {r['probe'][:70]}")

    ig = [r["g_en"] for r in rows if r["label"] == "INTENT"]
    ng = [r["g_en"] for r in rows if r["label"] == "NOISE"]
    if ig and ng:
        # separation: fraction of intent above tau_s, fraction of noise below tau_k
        intent_semantic_safe = sum(1 for g in ig if g >= TAU_S) / len(ig)
        noise_leaks = sum(1 for g in ng if g >= TAU_K) / len(ng)
        inseparable = (min(ig) <= max(ng))
        print(f"\n# SUMMARY  intent median g={sorted(ig)[len(ig)//2]:.3f}  "
              f"noise median g={sorted(ng)[len(ng)//2]:.3f}")
        print(f"#   intent semantic-safe (g>=tau_s): {intent_semantic_safe:.0%}")
        print(f"#   noise still leaks past floor (g>=tau_k): {noise_leaks:.0%}")
        print(f"#   distributions overlap (min intent <= max noise): {inseparable}")


if __name__ == "__main__":
    main()
