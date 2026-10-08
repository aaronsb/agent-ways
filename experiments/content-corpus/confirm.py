#!/usr/bin/env python3
"""Body confirmation: per-call sentence embedding vs precomputed section vectors.

ADR-701 §6 proposes that late-interaction body confirmation read a precomputed
section sidecar instead of embedding each survivor's first 8 body sentences on
every prompt. This script measures whether the sidecar separates real matches
from collisions at least as well, and what it costs.

    OUT=/tmp/confirm-out experiments/content-corpus/run.py GOLDEN.tsv [...]   # corpora first
    OUT=/tmp/confirm-out experiments/content-corpus/confirm.py GOLDEN.tsv [...]
    OUT=/tmp/confirm-out experiments/content-corpus/confirm.py --verify WRAPPER GOLDEN.tsv [...]

Surfaces are pairs and triples of golden prompts joined as sentences, so every
surface has known relevant ways (the expected ways of its prompts) and every
other admitted way is a collision. The late-interaction pipeline in
tools/ways-cli/src/cmd/scan/late_interaction.rs is reimplemented here: sentence
chunks, `way-embed match --batch` against the alias corpus, softmax share over
each chunk's top 8 at tau 0.08, admission on share >= 0.15 or peak >= 0.50, up to
6 survivors by peak. Each survivor is confirmed two ways:

  (a) today: `way-embed similarity --batch` of the won chunk against the way's
      first 8 body sentences (chunk_body rules), one subprocess per survivor,
      exactly as body_confirm calls it; score = max;
  (b) sidecar: dot product of the won chunk's vector against every section
      vector of the way (run.py `section` chunking), loaded once; score = max.

--verify WRAPPER runs `WRAPPER author match --project DIR SURFACE` on a sample
of surfaces and compares peak, share and (a) with the reimplementation. WRAPPER
must run this checkout's bin/ways with HOME/XDG pointed at a cache holding
$OUT/alias.jsonl as ways-corpus-en.jsonl and a ~/.claude/hooks/ways link to this
checkout's hooks/ways (see results-confirm.md).
"""

import json
import math
import random
import re
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import run  # noqa: E402

O = run.OUT
# The engine-dir copy is the binary the scan path resolves first
# (paths::way_embed); run.py's ~/.claude/bin copy can differ by build.
EMBED = run.CACHE / "way-embed"
if not EMBED.is_file():
    EMBED = run.EMBED
MODEL = run.MODEL

# late_interaction.rs operating points
SOFTMAX_TAU = 0.08
TOP_K_PER_CHUNK = 8
SHARE_GATE = 0.15
PEAK_GATE = 0.50
CONFIRM_GATE = 0.35
MAX_SURFACE_CHUNKS = 12
MAX_BODY_CHUNKS = 8
MIN_CHUNK_CHARS = 12
MAX_WINNERS_TO_CONFIRM = 6
BUDGET_PROMPT = 110  # scan/mod.rs; surfaces are kept under it so reduce is identity

SEED = 11


# ── Rust ports ───────────────────────────────────────────────────────────────

ASCII_WS = " \t\n\r\x0c"


def split_sentences(s: str) -> list[str]:
    """reduce.rs split_sentences: break after .!? followed by ASCII whitespace,
    and at a blank line."""
    out, start, i, n = [], 0, 0, len(s)
    while i < n:
        c = s[i]
        if c in ".!?" and i + 1 < n and s[i + 1] in ASCII_WS:
            chunk = s[start:i + 1].strip()
            if chunk:
                out.append(chunk)
            start = i = i + 2
            continue
        if c == "\n" and i + 1 < n and s[i + 1] == "\n":
            chunk = s[start:i].strip()
            if chunk:
                out.append(chunk)
            start = i = i + 2
            continue
        i += 1
    if start < n:
        tail = s[start:].strip()
        if tail:
            out.append(tail)
    return out


def approx_tokens(s: str) -> int:
    return max(len(s.split()), len(s) // 4)


def _dedup_cap(sents, cap):
    seen, out = set(), []
    for s in sents:
        s = " ".join(s.split())
        if len(s) < MIN_CHUNK_CHARS or s in seen:
            continue
        seen.add(s)
        out.append(s)
        if len(out) >= cap:
            break
    return out


def chunk_surface(surface: str) -> list[str]:
    return _dedup_cap(split_sentences(surface), MAX_SURFACE_CHUNKS)


def split_frontmatter(content: str):
    lines = content.splitlines(keepends=True)
    if not lines or lines[0].rstrip("\r\n") != "---":
        return None
    off = len(lines[0])
    for line in lines[1:]:
        off += len(line)
        if line.rstrip("\r\n") == "---":
            return content[off:]
    return None


def chunk_body(content: str) -> list[str]:
    """late_interaction.rs chunk_body: drop frontmatter and fenced code, strip
    leading markdown characters per line, sentence-split, dedup, cap 8."""
    first = content.splitlines()[0] if content else None
    if first == "---":
        body = split_frontmatter(content) or ""
    else:
        body = content
    prose, in_fence = [], False
    for line in body.splitlines():
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        prose.append(line.lstrip("#>-* \t"))
    return _dedup_cap(split_sentences("\n".join(prose) + "\n"), MAX_BODY_CHUNKS)


def aggregate(per_chunk: list[list[tuple[str, float]]]):
    peak, mass = {}, defaultdict(float)
    for ci, rows in enumerate(per_chunk):
        for wid, cos in rows:
            if wid not in peak or cos > peak[wid][0]:
                peak[wid] = (cos, ci)
        top = rows[:TOP_K_PER_CHUNK]
        denom = sum(math.exp(c / SOFTMAX_TAU) for _, c in top)
        if denom > 0:
            for wid, cos in top:
                mass[wid] += math.exp(cos / SOFTMAX_TAU) / denom
    n = max(len(per_chunk), 1)
    out = [{"id": w, "peak": peak[w][0], "chunk": peak[w][1], "share": m / n}
           for w, m in mass.items()]
    out.sort(key=lambda r: -r["share"])
    return out


def admit(ranked):
    surv = [r for r in ranked if r["share"] >= SHARE_GATE or r["peak"] >= PEAK_GATE]
    surv.sort(key=lambda r: -r["peak"])
    return surv[:MAX_WINNERS_TO_CONFIRM]


# ── embedding engine ─────────────────────────────────────────────────────────

def match_batch(chunks: list[str]) -> list[list[tuple[str, float]]]:
    p = subprocess.run([str(EMBED), "match", "--corpus", str(O / "alias.jsonl"),
                        "--model", str(MODEL), "--batch", "--threshold", "0.0"],
                       input="\n".join(chunks) + "\n", text=True, capture_output=True, check=True)
    per = [[] for _ in chunks]
    for line in p.stdout.splitlines():
        qi, wid, cos = line.split("\t")
        per[int(qi)].append((wid, float(cos)))
    return per


def similarity_call(won: str, body: list[str]) -> tuple[float, float]:
    """One body_confirm subprocess, as production issues it. Returns (max, seconds)."""
    inp = "".join(f"{won}\t{b}\n" for b in body)
    t = time.perf_counter()
    p = subprocess.run([str(EMBED), "similarity", "--model", str(MODEL), "--batch"],
                       input=inp, text=True, capture_output=True, check=True)
    dt = time.perf_counter() - t
    sims = [float(x) for x in p.stdout.split()]
    assert len(sims) == len(body), (won, len(sims), len(body))
    return max(sims), dt


def generate(texts: list[str], tag: str) -> np.ndarray:
    raw = O / f"confirm-{tag}.raw.jsonl"
    raw.write_text("".join(json.dumps({"id": f"x{i}", "description": t, "vocabulary": "",
                                       "threshold": 0, "embed_threshold": 0.0}) + "\n"
                           for i, t in enumerate(texts)))
    dst = O / f"confirm-{tag}.jsonl"
    subprocess.run([str(EMBED), "generate", "--corpus", str(raw), "--model", str(MODEL),
                    "--output", str(dst)], check=True, capture_output=True)
    return np.array([json.loads(l)["embedding"] for l in dst.read_text().splitlines()],
                    dtype=np.float32)


# ── surfaces ─────────────────────────────────────────────────────────────────

def load_golden(paths, known):
    rows = []
    for p in paths:
        for line in Path(p).read_text().splitlines():
            x = line.split("\t")
            if len(x) >= 2 and x[0] != "prompt" and (x[1] == "none" or x[1] in known):
                rows.append((x[0].strip(), x[1]))
    return rows


def area(wid):
    return "/".join(wid.split("/")[:2])


def related(a, b):
    return a == b or a.startswith(b + "/") or b.startswith(a + "/")


def as_sentence(p):
    p = p.strip()
    p = p[0].upper() + p[1:]
    return p if p[-1] in ".!?" else p + "."


def build_surfaces(golden, rng):
    tgt = [g for g in golden if g[1] != "none"]
    non = [g for g in golden if g[1] == "none"]
    shapes = [("T+T", 2, 0, 110), ("T+N", 1, 1, 60), ("T+T+N", 2, 1, 80), ("T+T+T", 3, 0, 60)]
    out, seen = [], set()
    for name, nt, nn, want in shapes:
        tries = 0
        got = 0
        while got < want and tries < want * 200:
            tries += 1
            ts = rng.sample(tgt, nt)
            ways = [t[1] for t in ts]
            if len({area(w) for w in ways}) < nt:
                continue  # different areas, so each prompt owns its own way
            if any(related(a, b) for i, a in enumerate(ways) for b in ways[i + 1:]):
                continue
            parts = ts + rng.sample(non, nn)
            rng.shuffle(parts)
            surface = " ".join(as_sentence(p[0]) for p in parts)
            if surface in seen or approx_tokens(surface) > BUDGET_PROMPT:
                continue
            if len(chunk_surface(surface)) != len(parts):
                continue  # a prompt that itself splits would blur chunk ownership
            seen.add(surface)
            out.append({"shape": name, "surface": surface, "expected": sorted(set(ways))})
            got += 1
    return out


# ── metrics ──────────────────────────────────────────────────────────────────

def auroc(pos, neg):
    if not pos or not neg:
        return float("nan")
    pos, neg = np.asarray(pos), np.asarray(neg)
    gt = (pos[:, None] > neg[None, :]).sum() + 0.5 * (pos[:, None] == neg[None, :]).sum()
    return gt / (len(pos) * len(neg))


def rates(pos, neg, gate):
    keep = np.mean(np.asarray(pos) >= gate) if pos else float("nan")
    rej = np.mean(np.asarray(neg) < gate) if neg else float("nan")
    return keep, rej


def gate_matching_keep(pos, keep):
    """Highest gate whose keep rate on `pos` is at least `keep`."""
    s = np.sort(np.asarray(pos))[::-1]
    k = max(int(math.ceil(keep * len(s))), 1)
    return float(s[k - 1])


# ── verification against the binary ─────────────────────────────────────────

ROW = re.compile(r"^\s{2}(\S+)\s+([\d.]+)\s+([\d.]+)\s+(\S+)\s+(.*)$")


def verify(wrapper, cases, bodies):
    lines = []
    for c in cases:
        p = subprocess.run([wrapper, "author", "match", "--project", "/tmp/confirm-empty",
                            c["surface"]], capture_output=True, text=True)
        rows = {}
        for l in p.stdout.splitlines():
            m = ROW.match(l)
            if m and m.group(1) != "way":
                rows[m.group(1)] = (float(m.group(2)), float(m.group(3)),
                                    None if m.group(4) == "—" else float(m.group(4)))
        bad = []
        mine = {r["id"]: r for r in c["ranked"][:20]}
        conf = {a["id"]: a["a"] for a in c["admitted"]}
        n = 0
        for wid, (pk, sh, cf) in rows.items():
            # the table ellipsizes long ids; match on prefix
            key = wid.rstrip("…")
            cand = [w for w in mine if w == wid or (wid.endswith("…") and w.startswith(key))]
            if not cand:
                bad.append(f"{wid}: no counterpart")
                continue
            # an ellipsized id can prefix several ways; take the closest numbers
            cand.sort(key=lambda w: abs(mine[w]["peak"] - pk) + abs(mine[w]["share"] - sh))
            r = mine[cand[0]]
            n += 1
            if abs(r["peak"] - pk) > 0.0011 or abs(r["share"] - sh) > 0.0011:
                bad.append(f"{wid}: peak {r['peak']:.3f}/{pk} share {r['share']:.3f}/{sh}")
            if cf is not None and cand[0] in conf and abs(conf[cand[0]] - cf) > 0.0011:
                bad.append(f"{wid}: confirm {conf[cand[0]]:.3f}/{cf}")
        lines.append((c["surface"], n, sum(1 for v in rows.values() if v[2] is not None), bad))
    return lines


# ── main ─────────────────────────────────────────────────────────────────────

def main():
    args = sys.argv[1:]
    wrapper = None
    if args and args[0] == "--verify":
        wrapper, args = args[1], args[2:]

    alias = [json.loads(l) for l in (O / "alias.jsonl").read_text().splitlines()]
    ids = [a["id"] for a in alias]
    known = set(ids)
    A = np.array([a["embedding"] for a in alias], dtype=np.float32)
    golden = load_golden(args, known)
    rng = random.Random(SEED)
    surfaces = build_surfaces(golden, rng)
    print(f"{len(golden)} golden rows, {len(surfaces)} surfaces "
          f"({', '.join(f'{k} {v}' for k, v in Counter(s['shape'] for s in surfaces).items())})")

    # Body chunks per chunk_body, and section vectors (re-embedded with EMBED so
    # every vector in the comparison comes from one binary).
    bodies = {w: chunk_body(run.way_file(w).read_text()) for w in ids}
    sec_rows = [json.loads(l) for l in (O / "content-section.raw.jsonl").read_text().splitlines()]
    t0 = time.perf_counter()
    SV = generate([r["description"] for r in sec_rows], "sections")
    sec_embed_s = time.perf_counter() - t0
    owner = [r["id"].split("##")[0] for r in sec_rows]
    sec_idx = defaultdict(list)
    for i, w in enumerate(owner):
        sec_idx[w].append(i)
    # the sidecar as the hook would hold it: one matrix per way, loaded once
    np.save(O / "confirm-sidecar.npy", SV)
    t0 = time.perf_counter()
    SVl = np.load(O / "confirm-sidecar.npy")
    sidecar = {w: SVl[ix] for w, ix in sec_idx.items()}
    load_s = time.perf_counter() - t0
    nsec = {w: len(sec_idx.get(w, [])) for w in ids}

    # Stage 2: one match pass over every surface chunk (same numbers as one
    # pass per surface; way-embed scores queries independently).
    all_chunks, spans = [], []
    for s in surfaces:
        ch = chunk_surface(s["surface"])
        s["chunks"] = ch
        spans.append((len(all_chunks), len(all_chunks) + len(ch)))
        all_chunks.extend(ch)
    per = match_batch(all_chunks)
    CV = generate(all_chunks, "chunks")

    recs = []
    t_a_surface, t_b_surface = [], []
    for s, (lo, hi) in zip(surfaces, spans):
        s["ranked"] = aggregate(per[lo:hi])
        s["admitted"] = admit(s["ranked"])
        ta = tb = 0.0
        for r in s["admitted"]:
            won = s["chunks"][r["chunk"]]
            body = bodies.get(r["id"]) or []
            if body:
                a, dt = similarity_call(won, body)
                ta += dt
            else:
                a = 0.0
            v = CV[lo + r["chunk"]]
            t = time.perf_counter()
            M = sidecar.get(r["id"])
            b = float((M @ v).max()) if M is not None else 0.0
            tb += time.perf_counter() - t
            r["a"], r["b"] = a, b
            r["b_alias"] = b if nsec[r["id"]] > 0 else float(A[ids.index(r["id"])] @ v)
            if r["id"] in s["expected"]:
                lab = "relevant"
            elif any(related(r["id"], e) for e in s["expected"]):
                lab = "related"
            else:
                lab = "irrelevant"
            recs.append({"shape": s["shape"], "id": r["id"], "label": lab, "a": a, "b": b,
                         "b_alias": r["b_alias"], "nsec": nsec[r["id"]],
                         "nbody": len(body), "peak": r["peak"], "share": r["share"],
                         "admitted_by": "share" if r["share"] >= SHARE_GATE else "peak",
                         "won": won, "surface": s["surface"]})
        t_a_surface.append(ta)
        t_b_surface.append(tb)

    # recall: of each surface's expected ways, how many were admitted at all
    exp_total = sum(len(s["expected"]) for s in surfaces)
    exp_adm = sum(len(set(s["expected"]) & {r["id"] for r in s["admitted"]}) for s in surfaces)

    out = {"n_surfaces": len(surfaces), "exp_total": exp_total, "exp_admitted": exp_adm,
           "sec_embed_s": sec_embed_s, "sidecar_load_s": load_s,
           "t_a_surface": t_a_surface, "t_b_surface": t_b_surface, "records": recs,
           "sections_per_way": nsec, "body_chunks_per_way": {w: len(b) for w, b in bodies.items()}}
    (O / "confirm.json").write_text(json.dumps(out, indent=1))
    report(out)

    if wrapper:
        rng2 = random.Random(SEED + 1)
        sample = rng2.sample(surfaces, 30)
        print("\nverification against `ways author match`:")
        for surf, n, nconf, bad in verify(wrapper, sample, bodies):
            print(f"  rows {n:2d} confirms {nconf} {'OK' if not bad else 'MISMATCH'}  {surf[:70]}")
            for b in bad:
                print(f"      {b}")


def boot_auc_diff(recs, key_a, key_b, labels_pos, n=2000):
    rng = np.random.default_rng(SEED)
    by_surface = defaultdict(list)
    for r in recs:
        by_surface[r["surface"]].append(r)
    keys = list(by_surface)
    diffs = []
    for _ in range(n):
        pick = rng.integers(0, len(keys), len(keys))
        rs = [r for k in pick for r in by_surface[keys[k]]]
        pos = [r for r in rs if r["label"] in labels_pos]
        neg = [r for r in rs if r["label"] == "irrelevant"]
        diffs.append(auroc([r[key_b] for r in pos], [r[key_b] for r in neg]) -
                     auroc([r[key_a] for r in pos], [r[key_a] for r in neg]))
    return np.percentile(diffs, [2.5, 97.5])


def report(out):
    recs = out["records"]
    cnt = defaultdict(int)
    for r in recs:
        cnt[r["label"]] += 1
    print(f"\nadmitted candidates: {len(recs)} ({dict(cnt)}); expected ways admitted "
          f"{out['exp_admitted']}/{out['exp_total']}")

    for title, pos_labels in (("relevant vs irrelevant (related excluded)", {"relevant"}),
                              ("relevant+related vs irrelevant", {"relevant", "related"})):
        pos = [r for r in recs if r["label"] in pos_labels]
        neg = [r for r in recs if r["label"] == "irrelevant"]
        print(f"\n{title}: {len(pos)} pos / {len(neg)} neg")
        print(f"  {'confirm':22} {'AUC':>6} {'keep@.35':>9} {'rej@.35':>8}")
        for key, name in (("a", "(a) 8 sentences"), ("b", "(b) sections"),
                          ("b_alias", "(b) sections|alias")):
            P, N = [r[key] for r in pos], [r[key] for r in neg]
            k, j = rates(P, N, CONFIRM_GATE)
            print(f"  {name:22} {auroc(P, N):6.3f} {k:9.3f} {j:8.3f}")
        Pa = [r["a"] for r in pos]
        ka, ja = rates(Pa, [r["a"] for r in neg], CONFIRM_GATE)
        g = gate_matching_keep([r["b"] for r in pos], ka)
        kb, jb = rates([r["b"] for r in pos], [r["b"] for r in neg], g)
        print(f"  (b) gate matching (a) keep {ka:.3f}: {g:.3f} -> keep {kb:.3f} rej {jb:.3f}"
              f"  [(a) rej {ja:.3f}]")
        lo, hi = boot_auc_diff(recs, "a", "b", pos_labels)
        print(f"  AUC(b) - AUC(a) 95% surface-bootstrap CI: [{lo:+.3f}, {hi:+.3f}]")

    print("\nby section count (relevant / irrelevant, mean score a | b):")
    for lo, hi, name in ((0, 0, "0 sections"), (1, 1, "1 section"), (2, 99, "2+ sections")):
        rs = [r for r in recs if lo <= r["nsec"] <= hi]
        P = [r for r in rs if r["label"] == "relevant"]
        N = [r for r in rs if r["label"] == "irrelevant"]
        def m(xs, k):
            return f"{np.mean([x[k] for x in xs]):.3f}" if xs else "  -  "
        print(f"  {name:12} n={len(rs):4d}  rel {len(P):3d} a {m(P, 'a')} b {m(P, 'b')}"
              f"  irr {len(N):3d} a {m(N, 'a')} b {m(N, 'b')}"
              f"  AUC a {auroc([x['a'] for x in P], [x['a'] for x in N]):.3f}"
              f" b {auroc([x['b'] for x in P], [x['b'] for x in N]):.3f}")

    ta, tb = np.array(out["t_a_surface"]), np.array(out["t_b_surface"])
    n_conf = len([r for r in recs if r["nbody"]])
    print(f"\nlatency per surface (confirm stage only): (a) mean {ta.mean()*1e3:.1f} ms, "
          f"p50 {np.median(ta)*1e3:.1f}, p95 {np.percentile(ta, 95)*1e3:.1f}; "
          f"(b) mean {tb.mean()*1e6:.1f} us, p95 {np.percentile(tb, 95)*1e6:.1f} us")
    print(f"  (a) per similarity call {ta.sum()/max(n_conf,1)*1e3:.1f} ms over {n_conf} calls; "
          f"sidecar load {out['sidecar_load_s']*1e3:.2f} ms; "
          f"section embed (build time) {out['sec_embed_s']:.1f} s")


if __name__ == "__main__":
    main()
