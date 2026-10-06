#!/usr/bin/env python3
"""Content-corpus routing experiment (follow-up to ADR-127).

Builds the alias corpus (description + vocabulary, as shipped) and one content
corpus per chunking strategy from hooks/ways in this checkout, scores a golden
TSV against each, and reports how alias-only, content-only and fused rankings
route.

    experiments/content-corpus/run.py GOLDEN.tsv [GOLDEN2.tsv ...]

Golden rows are `prompt<TAB>expected_way[<TAB>style]`; `none` marks a prompt no
way covers. Results go to stdout and to $OUT (default /tmp/content-corpus-out).
"""

import json
import math
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
WAYS = REPO / "hooks" / "ways"
_XDG = Path(os.environ.get("XDG_CACHE_HOME", ""))  # empty or relative counts as unset
CACHE = (_XDG if _XDG.is_absolute() else Path.home() / ".cache") / "agent-ways" / "user"
EMBED = Path.home() / ".claude" / "bin" / "way-embed"
MODEL = CACHE / "minilm-l6-v2.gguf"
OUT = Path(os.environ.get("OUT", "/tmp/content-corpus-out"))

MIN_CHARS = 25          # drop fragments shorter than this
SECTION_MAX_WORDS = 120  # MiniLM truncates past ~256 tokens; split long sections


# ── way bodies ───────────────────────────────────────────────────────────────

def way_file(way_id: str) -> Path:
    return WAYS / way_id / f"{way_id.rsplit('/', 1)[-1]}.md"


def body_of(text: str) -> str:
    if text.startswith("---"):
        parts = text.split("\n---", 1)
        text = parts[1].split("\n", 1)[1] if len(parts) == 2 else ""
    return text


def prose_blocks(body: str):
    """Yield (heading, [prose lines]) per section, without code, tables,
    HTML comments or the See Also section (it names other ways)."""
    heading, lines, in_fence, skip = "", [], False, False
    body = re.sub(r"<!--.*?-->", "", body, flags=re.S)
    for raw in body.splitlines():
        t = raw.strip()
        if t.startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        m = re.match(r"^(#{1,6})\s+(.*)", t)
        if m:
            if lines and not skip:
                yield heading, lines
            heading, lines = m.group(2).strip(), []
            skip = heading.lower().startswith("see also")
            continue
        if skip or t.startswith("|") or not t:
            if not t and lines and lines[-1] != "":
                lines.append("")
            continue
        lines.append(re.sub(r"^([-*>]|\d+\.)\s+", "", t))
    if lines and not skip:
        yield heading, lines


def clean(s: str) -> str:
    s = re.sub(r"`([^`]*)`", r"\1", s)
    s = re.sub(r"\*\*?([^*]+)\*\*?", r"\1", s)
    s = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", s)
    return " ".join(s.split())


def sentences(text: str):
    for s in re.split(r"(?<=[.!?])\s+(?=[A-Z0-9\"'(])|\n\s*\n", text):
        s = clean(s)
        if len(s) >= MIN_CHARS:
            yield s


def chunks(way_id: str, strategy: str):
    blocks = list(prose_blocks(body_of(way_file(way_id).read_text())))
    if strategy == "sentence":
        for _, lines in blocks:
            yield from sentences("\n".join(lines))
    elif strategy == "window3":
        sents = [s for _, lines in blocks for s in sentences("\n".join(lines))]
        if len(sents) <= 3:
            if sents:
                yield " ".join(sents)
            return
        for i in range(0, len(sents) - 2, 2):
            yield " ".join(sents[i:i + 3])
    elif strategy == "section":
        for heading, lines in blocks:
            words = clean(" ".join(l for l in lines if l)).split()
            for i in range(0, len(words), SECTION_MAX_WORDS):
                piece = " ".join(words[i:i + SECTION_MAX_WORDS])
                if len(piece) >= MIN_CHARS:
                    yield f"{heading}. {piece}" if heading else piece
    else:
        raise ValueError(strategy)


# ── corpora ──────────────────────────────────────────────────────────────────

def build_alias_corpus() -> tuple[Path, list[str]]:
    out = OUT / "alias"
    out.mkdir(parents=True, exist_ok=True)
    subprocess.run([str(REPO / "bin" / "ways"), "corpus", "--ways-dir", str(WAYS),
                    "--output", str(out), "-q"], check=True, capture_output=True)
    src = out / "ways-corpus-en.jsonl"
    ids, rows = [], []
    for line in src.read_text().splitlines():
        row = json.loads(line)
        if row["id"].startswith("-"):  # operator's project ways, not committed
            continue
        ids.append(row["id"])
        rows.append(line)
    dst = OUT / "alias.jsonl"
    dst.write_text("\n".join(rows) + "\n")
    return dst, ids


def build_content_corpus(ids: list[str], strategy: str) -> tuple[Path, dict]:
    raw = OUT / f"content-{strategy}.raw.jsonl"
    stats = defaultdict(int)
    with raw.open("w") as f:
        for wid in ids:
            for n, c in enumerate(chunks(wid, strategy)):
                f.write(json.dumps({"id": f"{wid}##{n}", "description": c,
                                    "vocabulary": "", "threshold": 0,
                                    "embed_threshold": 0.0}) + "\n")
                stats[wid] += 1
    dst = OUT / f"content-{strategy}.jsonl"
    subprocess.run([str(EMBED), "generate", "--corpus", str(raw), "--model", str(MODEL),
                    "--output", str(dst)], check=True, capture_output=True)
    return dst, stats


def batch_scores(corpus: Path, prompts: list[str]) -> list[dict]:
    """Per prompt: {way_id: best cosine}, chunk ids folded to their way (max)."""
    p = subprocess.run([str(EMBED), "match", "--corpus", str(corpus), "--model", str(MODEL),
                        "--batch", "--threshold", "0.0"],
                       input="\n".join(prompts) + "\n", text=True,
                       capture_output=True, check=True)
    out = [dict() for _ in prompts]
    for line in p.stdout.splitlines():
        qi, cid, cos = line.split("\t")
        wid = cid.split("##", 1)[0]
        d = out[int(qi)]
        c = float(cos)
        if c > d.get(wid, -9):
            d[wid] = c
    return out


# ── scoring ──────────────────────────────────────────────────────────────────

def rank(d: dict) -> list[str]:
    return sorted(d, key=d.get, reverse=True)


def rrf(*dicts, k=60) -> dict:
    out = defaultdict(float)
    for d in dicts:
        for r, wid in enumerate(rank(d)):
            out[wid] += 1.0 / (k + r + 1)
    return out


def auroc(pos: list[float], neg: list[float]) -> float:
    if not pos or not neg:
        return float("nan")
    wins = sum((p > n) + 0.5 * (p == n) for p in pos for n in neg)
    return wins / (len(pos) * len(neg))


def main():
    golden = []
    for path in sys.argv[1:]:
        for line in Path(path).read_text().splitlines():
            parts = line.split("\t")
            if len(parts) < 2 or parts[0] == "prompt":
                continue
            style = parts[2] if len(parts) > 2 else "legacy"
            golden.append((parts[0], parts[1], style))
    OUT.mkdir(parents=True, exist_ok=True)

    alias_path, ids = build_alias_corpus()
    known = set(ids)
    golden = [g for g in golden if g[1] == "none" or g[1] in known]
    prompts = [g[0] for g in golden]
    print(f"{len(golden)} golden rows, {len(ids)} ways")

    alias = batch_scores(alias_path, prompts)
    methods = {"alias": alias}
    for strategy in ("sentence", "window3", "section"):
        cpath, stats = build_content_corpus(ids, strategy)
        n = sum(stats.values())
        print(f"content[{strategy}]: {n} chunks, {n / len(ids):.1f}/way, "
              f"{sum(1 for i in ids if not stats[i])} ways with none")
        content = batch_scores(cpath, prompts)
        methods[f"content:{strategy}"] = content
        for lam in (0.25, 0.5, 1.0):
            methods[f"alias+{lam}*{strategy}"] = [
                {w: a.get(w, 0) + lam * c.get(w, 0) for w in set(a) | set(c)}
                for a, c in zip(alias, content)]
        methods[f"max(alias,{strategy})"] = [
            {w: max(a.get(w, -1), c.get(w, -1)) for w in set(a) | set(c)}
            for a, c in zip(alias, content)]
        methods[f"rrf(alias,{strategy})"] = [rrf(a, c) for a, c in zip(alias, content)]

    styles = sorted({g[2] for g in golden if g[1] != "none"})
    header = f"{'method':28} {'top1':>6} {'mrr':>6} {'r@3':>6} {'r@5':>6} " + \
             " ".join(f"{s[:10]:>10}" for s in styles) + f" {'noneAUC':>8}"
    print("\n" + header + "\n" + "-" * len(header))
    report = {}
    for name, scored in methods.items():
        top1 = mrr = r3 = r5 = 0
        per_style = defaultdict(lambda: [0, 0])
        pos, neg, misses = [], [], []
        for (prompt, exp, style), d in zip(golden, scored):
            r = rank(d)
            top = d[r[0]] if r else 0
            if exp == "none":
                neg.append(top)
                continue
            pos.append(top)
            idx = r.index(exp) if exp in r else 999
            hit = idx == 0
            top1 += hit
            mrr += 1 / (idx + 1) if idx < 999 else 0
            r3 += idx < 3
            r5 += idx < 5
            per_style[style][0] += hit
            per_style[style][1] += 1
            if not hit:
                misses.append((prompt, exp, r[0] if r else None))
        n = len(pos)
        row = (f"{name:28} {top1 / n:6.3f} {mrr / n:6.3f} {r3 / n:6.3f} {r5 / n:6.3f} " +
               " ".join(f"{per_style[s][0] / max(per_style[s][1], 1):10.3f}" for s in styles) +
               f" {auroc(pos, neg):8.3f}")
        print(row)
        report[name] = {"top1": top1, "n": n, "misses": misses}

    # Which rows does each content method fix or break relative to alias?
    base = {m[0] for m in report["alias"]["misses"]}
    print("\nflips vs alias (fixed / broken):")
    for name in methods:
        if name == "alias":
            continue
        mine = {m[0] for m in report[name]["misses"]}
        print(f"  {name:28} +{len(base - mine):3d} / -{len(mine - base):3d}")
    (OUT / "report.json").write_text(json.dumps(report, indent=1))


if __name__ == "__main__":
    main()
