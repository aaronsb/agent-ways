#!/usr/bin/env python3
"""
Signal analysis for the ways embedding matcher.

For a representative battery of prompts, runs each through way-embed against
both the EN corpus (minilm-l6-v2) and the multi corpus
(paraphrase-multilingual-MiniLM-L12-v2). Plots score distributions so the
tuning team can see the signal/noise bands per model and judge where the
thresholds should live.

Usage:
    scripts/signal-report.py                   # default prompt battery
    scripts/signal-report.py --prompts FILE    # one JSONL prompt per line
    scripts/signal-report.py --out DIR         # where to write PNGs + CSV

Requires: matplotlib, numpy. Relies on way-embed and the multilingual
corpora being installed (make setup).
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

try:
    import matplotlib.pyplot as plt
    import numpy as np
except ImportError:
    print("ERROR: matplotlib and numpy are required. pip install matplotlib numpy", file=sys.stderr)
    sys.exit(1)


DEFAULT_BATTERY: list[tuple[str, str, str]] = [
    # (language, expected_way, prompt) — a small, mixed set
    ("en", "softwaredev/delivery/commits", "let me make an atomic commit with a conventional prefix"),
    ("en", "softwaredev/code/testing", "i need to write unit tests for this module"),
    ("en", "softwaredev/environment/debugging", "help me debug this stack trace and narrow down the bug"),
    ("en", "workstation/shell/shellrc", "configure zsh prompt and aliases in .zshrc"),
    ("en", "workstation/shell/prompt", "set up a starship prompt with nerd font glyphs"),
    ("en", "softwaredev/architecture/adr", "draft an ADR for switching to the new auth system"),
    ("en", "softwaredev/code/security/injection", "sanitize this SQL so it doesn't allow injection"),
    ("en", "meta/knowledge/authoring", "help me write a new way for shell history search"),
    ("ru", "softwaredev/delivery/commits", "закоммитить изменения и запушить в origin"),
    ("ja", "softwaredev/environment/debugging", "デバッグ バグ 調査 スタックトレース"),
    ("zh", "softwaredev/code/testing", "需要写单元测试覆盖这个模块"),
    ("de", "softwaredev/architecture", "eine ADR-Entscheidung für die Architektur dokumentieren"),
    ("es", "softwaredev/code/testing", "necesito escribir pruebas unitarias"),
    # Adversarial prompts that should fire NOTHING
    ("en", None, "what is the weather today"),
    ("en", None, "tell me a joke"),
    ("en", None, "hello"),
]


@dataclass
class Score:
    prompt: str
    lang: str
    expected_way: str | None
    way_id: str
    model: str  # "en" or "multi"
    score: float


def _engine_dir() -> Path:
    # Prefer the 1.0 cache name; fall back to legacy for un-migrated installs.
    # Matches paths::cache_root() in the binary.
    cache = Path(os.environ.get("XDG_CACHE_HOME") or (Path.home() / ".cache"))
    new = cache / "agent-ways/user"
    if new.is_dir():
        return new
    legacy = cache / "claude-ways/user"
    if legacy.is_dir():
        return legacy
    return new


def way_embed_path() -> Path:
    xdg = _engine_dir() / "way-embed"
    if xdg.is_file():
        return xdg
    fallback = Path.home() / ".claude/bin/way-embed"
    if fallback.is_file():
        return fallback
    raise FileNotFoundError("way-embed binary not found — run make setup")


def run_match(bin_path: Path, corpus: Path, model: Path, query: str) -> list[tuple[str, float]]:
    """Run way-embed match with threshold 0 and return (way_id, score) tuples."""
    result = subprocess.run(
        [str(bin_path), "match",
         "--corpus", str(corpus),
         "--model", str(model),
         "--query", query,
         "--threshold", "0.0"],
        capture_output=True, text=True, check=False,
    )
    if result.returncode != 0:
        return []
    out = []
    for line in result.stdout.splitlines():
        parts = line.split("\t")
        if len(parts) < 2:
            continue
        try:
            out.append((parts[0], float(parts[1])))
        except ValueError:
            continue
    return out


def gather(battery: list[tuple[str, str | None, str]]) -> list[Score]:
    bin_path = way_embed_path()
    xdg = _engine_dir()
    en_corpus = xdg / "ways-corpus-en.jsonl"
    en_model = xdg / "minilm-l6-v2.gguf"
    mu_corpus = xdg / "ways-corpus-multi.jsonl"
    mu_model = xdg / "multilingual-minilm-l12-v2-q8.gguf"

    all_scores: list[Score] = []
    for lang, expected, prompt in battery:
        if en_corpus.is_file() and en_model.is_file():
            for way_id, s in run_match(bin_path, en_corpus, en_model, prompt):
                all_scores.append(Score(prompt, lang, expected, way_id, "en", s))
        if mu_corpus.is_file() and mu_model.is_file():
            # multi corpus has duplicate way_ids (one per lang) — keep max per way
            best: dict[str, float] = {}
            for way_id, s in run_match(bin_path, mu_corpus, mu_model, prompt):
                if s > best.get(way_id, -1):
                    best[way_id] = s
            for way_id, s in best.items():
                all_scores.append(Score(prompt, lang, expected, way_id, "multi", s))
    return all_scores


def plot_distributions(scores: list[Score], outdir: Path, en_thr: float, mu_thr: float):
    """Histogram of signal (expected way) vs noise (other ways) per model."""
    fig, axes = plt.subplots(1, 2, figsize=(14, 5), sharey=True)
    for ax, model, thr in [(axes[0], "en", en_thr), (axes[1], "multi", mu_thr)]:
        signal = [s.score for s in scores if s.model == model and s.expected_way and s.way_id == s.expected_way]
        noise = [s.score for s in scores if s.model == model and (not s.expected_way or s.way_id != s.expected_way)]
        bins = np.linspace(-0.2, 1.0, 50)
        ax.hist(noise, bins=bins, color="#d62728", alpha=0.55, label=f"noise (n={len(noise)})")
        ax.hist(signal, bins=bins, color="#2ca02c", alpha=0.75, label=f"signal (n={len(signal)})")
        ax.axvline(thr, color="black", linestyle="--", linewidth=1, label=f"threshold {thr:.2f}")
        ax.set_title(f"{model} model — signal vs noise")
        ax.set_xlabel("cosine similarity")
        ax.set_ylabel("count")
        ax.legend()
        ax.grid(True, alpha=0.3)
    fig.suptitle("Ways matcher — score distributions across prompt battery")
    fig.tight_layout()
    path = outdir / "score-distributions.png"
    fig.savefig(path, dpi=110)
    plt.close(fig)
    return path


def plot_per_prompt_top(scores: list[Score], outdir: Path, en_thr: float, mu_thr: float):
    """Bar chart per prompt: expected way's score vs top competing way's score."""
    prompts = sorted({s.prompt for s in scores})
    fig, axes = plt.subplots(len(prompts), 1, figsize=(12, 1.2 * len(prompts)), sharex=True)
    if len(prompts) == 1:
        axes = [axes]
    for ax, prompt in zip(axes, prompts):
        rows = [s for s in scores if s.prompt == prompt]
        expected = rows[0].expected_way
        for model, thr, color in [("en", en_thr, "#1f77b4"), ("multi", mu_thr, "#ff7f0e")]:
            ms = [s for s in rows if s.model == model]
            if not ms:
                continue
            exp_s = max((s.score for s in ms if s.way_id == expected), default=None)
            top_other = max((s for s in ms if s.way_id != expected), key=lambda s: s.score, default=None)
            xs = []
            labels = []
            cs = []
            if exp_s is not None:
                xs.append(exp_s)
                labels.append(f"{model}: {expected or '(none expected)'}")
                cs.append(color)
            if top_other:
                xs.append(top_other.score)
                labels.append(f"{model}: {top_other.way_id} (top other)")
                cs.append(color)
            y = np.arange(len(xs))
            ax.barh(y - (0 if model == "en" else 0.4), xs, height=0.35, color=cs, alpha=0.75, label=f"{model}")
        ax.axvline(en_thr, color="#1f77b4", linestyle=":", linewidth=1)
        ax.axvline(mu_thr, color="#ff7f0e", linestyle=":", linewidth=1)
        ax.set_xlim(0, 1.0)
        ax.set_title(f"{prompt[:70]}", fontsize=9, loc="left")
        ax.grid(True, axis="x", alpha=0.3)
    fig.suptitle("Per-prompt: expected vs top competing (dotted = each model's threshold)")
    fig.tight_layout()
    path = outdir / "per-prompt-gap.png"
    fig.savefig(path, dpi=110)
    plt.close(fig)
    return path


def write_csv(scores: list[Score], outdir: Path):
    path = outdir / "scores.csv"
    with path.open("w") as f:
        f.write("prompt,lang,expected_way,way_id,model,score\n")
        for s in scores:
            exp = s.expected_way or ""
            prompt_safe = s.prompt.replace('"', '""')
            f.write(f'"{prompt_safe}",{s.lang},{exp},{s.way_id},{s.model},{s.score:.4f}\n')
    return path


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--prompts", type=Path, help="JSONL with {lang, expected_way?, prompt} per line")
    ap.add_argument("--out", type=Path, default=Path.home() / ".claude/docs/signal-analysis")
    ap.add_argument("--en-threshold", type=float, default=0.40)
    ap.add_argument("--multi-threshold", type=float, default=0.55)
    args = ap.parse_args()

    if args.prompts:
        battery = []
        with args.prompts.open() as f:
            for line in f:
                d = json.loads(line)
                battery.append((d.get("lang", "en"), d.get("expected_way"), d["prompt"]))
    else:
        battery = DEFAULT_BATTERY

    args.out.mkdir(parents=True, exist_ok=True)
    print(f"Gathering scores for {len(battery)} prompts across both models...")
    scores = gather(battery)
    print(f"  collected {len(scores)} (way, model, score) data points")

    csv_path = write_csv(scores, args.out)
    print(f"  CSV → {csv_path}")

    dist_path = plot_distributions(scores, args.out, args.en_threshold, args.multi_threshold)
    print(f"  distribution plot → {dist_path}")

    prompt_path = plot_per_prompt_top(scores, args.out, args.en_threshold, args.multi_threshold)
    print(f"  per-prompt plot → {prompt_path}")


if __name__ == "__main__":
    main()
