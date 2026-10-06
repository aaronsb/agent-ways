#!/usr/bin/env python3
"""Late-interaction admission through the built binary, against recall.py's port.

ADR-700 §12 measured, in recall.py's Python port, that admitting each chunk's
top-ranked way (keeping the peak co-gate at 0.50 and the cap at 6) raises recall
after confirm. ADR-701 increment 6 gates that mode on the binary reproducing the
port. This script runs both on the same surfaces and compares them.

    OUT=/tmp/adm-out python3 -c 'import run; run.OUT.mkdir(parents=True, exist_ok=True); run.build_alias_corpus()'
    OUT=/tmp/adm-out experiments/content-corpus/admission_binary.py \
        --ways tools/target/release/ways [--mode share|chunk_top|both] [--text] \
        experiments/content-corpus/golden-synthetic.tsv tests/routing-golden.tsv

Smoke run: add `--quick` to run a small fixed subset of the golden rows (every
multi-chunk row plus every 12th single-chunk row) instead of the full set; it
finishes in a fraction of the time and still compares binary against port on
both the main and the multi-chunk surfaces. Use the full set for any number you
mean to cite.

    OUT=/tmp/adm-out experiments/content-corpus/admission_binary.py --quick \
        --ways tools/target/release/ways tests/routing-golden.tsv

The engine, `way-embed`, is resolved once, before anything runs, as the binary
resolves it (ways-core paths::way_embed): $XDG_CACHE_HOME/agent-ways/user/way-embed
(an empty or relative $XDG_CACHE_HOME counts as unset), else ~/.claude/bin/way-embed,
each a file. `--way-embed PATH` overrides. That one path serves the binary run
(linked into the scratch cache) and the port (confirm.EMBED), so both measure
the same engine; it is printed as `way-embed: PATH` and written to
admission-binary.json. If none exists the script exits 2 naming the paths tried.

Exit 2 also when a result would be a zero that measured nothing: any surface
with 2+ chunks (the only ones the binary returns rows for) comes back empty;
no surface has a confirm score; or no confirm value was compared against the
port. A recall table over such results would read as recall 0.000.

Surfaces are recall.py's: confirm.py's 310 main surfaces (seed 11) and the 93
auxiliary surfaces built around the multi-sentence golden prompts (seed 13).

Python side: recall.evaluate at the shipped point (K 8, share 0.15, peak 0.50,
cap 6, share / n_chunks) for `share`, and at recall's `top1` pick for
`chunk_top`. Confirm is today's per-call body confirm (the won chunk against
the way's chunk_body sentences, max), batched through recall.confirm_scores.

Binary side: `WAYS author match --all --project EMPTY SURFACE`, run under a
scratch HOME and XDG tree in $OUT/binenv: the corpus is $OUT/alias.jsonl, the
ways are this checkout's hooks/ways, there is no body sidecar (so confirm is
per call, as in the port), and config.yaml sets `admission:` for the mode.
`--all` competes every way, as the port does (it masks nothing). `--json`
output is read when the binary has it; `--text` parses the table instead (for
a binary that predates `--json`, which also lists only the top 20 by share).

Reports, per mode: admitted and fired sets per surface, binary vs port, with
every disagreement listed; recall after admission and after confirm; admitted,
fired and irrelevant-fired candidates per surface. With `--mode both`, the
per-surface diffs between the modes as the binary decides them.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import confirm as C  # noqa: E402
import recall as R  # noqa: E402
import run  # noqa: E402

O = run.OUT
PY_POINT = {"share": R.SHIPPED, "chunk_top": R.PICKS["top1"]}
TEXT_ROW = re.compile(r"^\s{2}(\S+)\s+([\d.]+)\s+([\d.]+)\s+(\S+)\s+(fired ✓|< gate|< cap|< confirm)\s")


# ── the binary's environment ─────────────────────────────────────────────────

def die(msg):
    print(f"admission_binary: {msg}", file=sys.stderr)
    sys.exit(2)


def find_way_embed(flag):
    """The way-embed the binary would use (ways-core paths::way_embed): the
    cache dir, then ~/.claude/bin, each a file. A flag overrides."""
    xdg = Path(os.environ.get("XDG_CACHE_HOME", ""))
    cache = (xdg if xdg.is_absolute() else Path.home() / ".cache") / "agent-ways" / "user"
    if flag:  # an explicit choice is not silently replaced by a fallback
        if not Path(flag).exists():
            die(f"--way-embed {flag} does not exist")
        return Path(flag).resolve()
    tried = [cache / "way-embed", Path.home() / ".claude" / "bin" / "way-embed"]
    for c in tried:
        if c.is_file():
            return c.resolve()
    die("way-embed not found; tried " + ", ".join(str(c) for c in tried)
        + ". Build it or pass --way-embed PATH; without it every surface returns no rows.")


def binenv(mode, explicit, embed):
    """A scratch HOME/XDG tree whose corpus is $OUT/alias.jsonl. With
    `explicit` false and mode share, config.yaml names no admission key, as an
    older binary expects."""
    root = O / "binenv"
    home, cache = root / "home", root / "cache" / "agent-ways" / "user"
    for d in (home / ".claude" / "hooks", cache, root / "xdg" / "data",
              root / "xdg" / "config" / "agent-ways", root / "xdg" / "state", root / "empty"):
        d.mkdir(parents=True, exist_ok=True)
    link = home / ".claude" / "hooks" / "ways"
    if link.is_symlink() or link.exists():
        link.unlink()  # OUT may be shared: never trust a link from another checkout
    link.symlink_to(run.WAYS)
    if not run.MODEL.exists():
        die(f"model not found: {run.MODEL}")
    for name, src in (("minilm-l6-v2.gguf", run.MODEL), ("way-embed", embed)):
        dst = cache / name
        if dst.is_symlink() or dst.exists():
            dst.unlink()  # a link left by an earlier run may dangle
        dst.symlink_to(src)
        if not dst.exists():
            die(f"link target missing: {dst} -> {src}")
    shutil.copyfile(O / "alias.jsonl", cache / "ways-corpus-en.jsonl")
    for stale in ("embed-manifest.json", "ways-body-en.bin"):
        (cache / stale).unlink(missing_ok=True)  # no sidecar: confirm per call
    cfg = root / "xdg" / "config" / "agent-ways" / "config.yaml"
    cfg.write_text(f"admission: {mode}\n" if (explicit or mode != "share") else "")
    env = {**os.environ, "HOME": str(home), "XDG_CACHE_HOME": str(root / "cache"),
           "XDG_DATA_HOME": str(root / "xdg" / "data"),
           "XDG_CONFIG_HOME": str(root / "xdg" / "config"),
           "XDG_STATE_HOME": str(root / "xdg" / "state")}
    env.pop("CLAUDE_CONFIG_DIR", None)
    env.pop("CLAUDE_PROJECT_DIR", None)
    return env, root / "empty"


def run_binary(ways, surfaces, env, empty, text):
    def one(surface):
        args = [ways, "author", "match", "--all", "--project", str(empty), surface]
        if not text:
            args.append("--json")
        p = subprocess.run(args, capture_output=True, text=True, env=env, cwd=empty)
        if p.returncode != 0:
            raise RuntimeError(f"{p.stderr}\n{surface}")
        return parse_text(p.stdout) if text else json.loads(p.stdout)["rows"]
    with ThreadPoolExecutor(max_workers=8) as ex:
        return list(ex.map(one, [s["surface"] for s in surfaces]))


def parse_text(out):
    rows = []
    for line in out.splitlines():
        m = TEXT_ROW.match(line)
        if m:
            rows.append({"id": m.group(1), "peak": float(m.group(2)), "share": float(m.group(3)),
                         "confirm": None if m.group(4) == "—" else float(m.group(4)),
                         "admitted": m.group(5) not in ("< gate", "< cap"), "fired": m.group(5) == "fired ✓"})
    return rows


def resolve_ids(rows, ranked):
    """The text table ellipsizes ids past 34 columns; map each row to the
    port's way with that prefix and the closest peak and share."""
    for r in rows:
        if r["id"].endswith("…"):
            key = r["id"][:-1]
            cand = [x for x in ranked if x["id"].startswith(key)]
            cand.sort(key=lambda x: abs(x["peak"] - r["peak"]) + abs(x["share"] - r["share"]))
            r["id"] = cand[0]["id"] if cand else r["id"]
    return rows


# ── the port ─────────────────────────────────────────────────────────────────

def port(allsurf, bodies):
    out = {}
    pairs = set()
    for mode, st in PY_POINT.items():
        res = [R.evaluate(s["rows"], **st) for s in allsurf]
        out[mode] = res
        pairs.update((si, r["id"], r["chunk"]) for si, (_, _, adm, _) in enumerate(res) for r in adm)
    cache = O / "recall-confirm-cache.json"
    key = lambda k: f"{allsurf[k[0]]['surface']}\t{k[1]}\t{k[2]}"  # noqa: E731
    old = json.loads(cache.read_text()) if cache.is_file() else {}
    todo = {k for k in pairs if key(k) not in old}
    conf = {k: old[key(k)] for k in pairs if key(k) in old}
    if todo:
        conf.update(R.confirm_scores(todo, allsurf, bodies))
        cache.write_text(json.dumps({**old, **{key(k): v for k, v in conf.items()}}))
    return out, conf


# ── metrics ──────────────────────────────────────────────────────────────────

def metrics(decisions, allsurf, which):
    """decisions[si] = (admitted ids, fired ids)."""
    sel = [si for si, s in enumerate(allsurf) if s["set"] == which]
    exp = adm_rel = fired_rel = n_adm = n_fired = irr_adm = irr_fired = 0
    for si in sel:
        s = allsurf[si]
        adm, fired = decisions[si]
        exp += len(s["expected"])
        adm_rel += len(adm & set(s["expected"]))
        fired_rel += len(fired & set(s["expected"]))
        n_adm += len(adm)
        n_fired += len(fired)
        irr_adm += sum(R.label(w, s["expected"]) == "irrelevant" for w in adm)
        irr_fired += sum(R.label(w, s["expected"]) == "irrelevant" for w in fired)
    n = len(sel)
    if not exp:  # no surfaces, or none with an expected way: every ratio is n/a
        return {"n": n, "exp": 0}
    return {"n": n, "exp": exp, "rec_adm": adm_rel / exp, "rec_fired": fired_rel / exp,
            "adm_per": n_adm / n, "fired_per": n_fired / n,
            "irr_adm_per": irr_adm / n, "irr_fired_per": irr_fired / n,
            "fired_rel": fired_rel}


def show(name, m):
    if not m["exp"]:
        print(f"  {name:28} n {m['n']:3d}  n/a (0 expected)")
        return
    print(f"  {name:28} n {m['n']:3d}  rec adm {m['rec_adm']:.3f}  rec fired {m['rec_fired']:.3f}"
          f" ({m['fired_rel']}/{m['exp']})  adm/s {m['adm_per']:.2f}  fired/s {m['fired_per']:.2f}"
          f"  irr adm/s {m['irr_adm_per']:.2f}  irr fired/s {m['irr_fired_per']:.2f}")


def quick_subset(golden):
    """Fixed smoke subset: every multi-chunk row, plus every 12th of the rest."""
    multi = [g for g in golden if len(C.chunk_surface(C.as_sentence(g[0]))) >= 2]
    rest = [g for g in golden if g not in multi]
    return multi + rest[::12]


def positive_control(mode, rows, allsurf):
    """A zero must be a measurement. The binary returns rows only for surfaces
    of 2+ chunks; every one of them must have rows, and at least one must carry
    a confirm score (late interaction ran)."""
    multi = [si for si, s in enumerate(allsurf) if len(s["chunks"]) >= 2]
    if not multi:
        die(f"mode {mode}: no surface has 2+ chunks; nothing to measure")
    empty = [si for si in multi if not rows[si]]
    if empty:
        die(f"mode {mode}: the binary returned no rows on {len(empty)} of {len(multi)} "
            f"surfaces with 2+ chunks (first: s{empty[0]}); is way-embed or the corpus "
            "missing? refusing to report recall")
    late = sum(any(x["confirm"] is not None for x in rows[si]) for si in multi)
    if late == 0:
        die(f"mode {mode}: late interaction ran on 0 of {len(multi)} surfaces with 2+ chunks "
            "(no confirm score on any row); refusing to report recall")
    print(f"  control: rows on {len(multi)}/{len(multi)} surfaces with 2+ chunks, "
          f"confirm scored on {late}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ways", required=True)
    ap.add_argument("--way-embed", help="way-embed to link into the scratch cache")
    ap.add_argument("--quick", action="store_true", help="smoke run on a small fixed golden subset")
    ap.add_argument("--mode", default="both", choices=["share", "chunk_top", "both"])
    ap.add_argument("--text", action="store_true", help="parse the table, not --json")
    ap.add_argument("--explicit", action="store_true",
                    help="write `admission: share` for share mode (default: no key)")
    ap.add_argument("golden", nargs="+")
    a = ap.parse_args()
    ways = str(Path(a.ways).resolve())
    embed = find_way_embed(a.way_embed)
    C.EMBED = run.EMBED = embed  # the port measures the engine the binary runs
    print(f"way-embed: {embed}")
    binenv(a.mode if a.mode != "both" else "share", a.explicit, embed)  # fail fast

    alias = [json.loads(l) for l in (O / "alias.jsonl").read_text().splitlines()]
    ids = [x["id"] for x in alias]
    golden = C.load_golden(a.golden, set(ids))
    if a.quick:
        golden = quick_subset(golden)
    import random
    surfaces = R.own_confirm_surfaces(golden, random.Random(R.SEED))
    aux, _, _ = R.multi_chunk_surfaces(golden, random.Random(R.SEED + 2))
    allsurf = surfaces + aux
    chunks, spans = [], []
    for s in allsurf:
        spans.append((len(chunks), len(chunks) + len(s["chunks"])))
        chunks.extend(s["chunks"])
    per = C.match_batch(chunks)
    for s, (lo, hi) in zip(allsurf, spans):
        s["rows"] = per[lo:hi]
    print(f"{len(golden)} golden rows; {len(surfaces)} main surfaces, {len(aux)} auxiliary")
    bodies = {w: C.chunk_body(run.way_file(w).read_text()) for w in ids}
    py, conf = port(allsurf, bodies)

    modes = ["share", "chunk_top"] if a.mode == "both" else [a.mode]
    binary = {}
    report = {}
    for mode in modes:
        env, empty = binenv(mode, a.explicit, embed)
        rows = run_binary(ways, allsurf, env, empty, a.text)
        positive_control(mode, rows, allsurf)
        dec_b, dec_p, diffs, conf_bad, n_conf = [], [], [], [], 0
        for si, (s, rs) in enumerate(zip(allsurf, rows)):
            ranked, _, adm, _ = py[mode][si]
            if a.text:
                rs = resolve_ids(rs, ranked)
            b_adm = {r["id"] for r in rs if r["admitted"]}
            b_fired = {r["id"] for r in rs if r["fired"]}
            p_adm = {r["id"] for r in adm}
            p_fired = {r["id"] for r in adm if conf[(si, r["id"], r["chunk"])] >= C.CONFIRM_GATE}
            dec_b.append((b_adm, b_fired))
            dec_p.append((p_adm, p_fired))
            if b_adm != p_adm or b_fired != p_fired:
                diffs.append((si, sorted(p_adm - b_adm), sorted(b_adm - p_adm),
                              sorted(p_fired - b_fired), sorted(b_fired - p_fired)))
            pc = {r["id"]: conf[(si, r["id"], r["chunk"])] for r in adm}
            for r in rs:
                if r["confirm"] is not None and r["id"] in pc:
                    n_conf += 1
                    if abs(r["confirm"] - pc[r["id"]]) > 0.0011:
                        conf_bad.append((si, r["id"], r["confirm"], pc[r["id"]]))
        if n_conf == 0:
            die(f"mode {mode}: no confirm value was compared against the port; "
                "refusing to report agreement")
        binary[mode] = dec_b
        print(f"\n── mode {mode}: binary vs port ──")
        print(f"  surfaces with identical admitted and fired sets: "
              f"{len(allsurf) - len(diffs)}/{len(allsurf)}; confirm values compared {n_conf}, "
              f"off by > 0.001: {len(conf_bad)}")
        for si, pa, ba, pf, bf in diffs[:20]:
            print(f"  DIFF s{si} port-only adm {pa} binary-only adm {ba} "
                  f"port-only fired {pf} binary-only fired {bf}\n      {allsurf[si]['surface'][:110]}")
        for si, w, b, p in conf_bad[:10]:
            print(f"  CONFIRM s{si} {w}: binary {b:.3f} port {p:.3f}")
        report[mode] = {"diffs": len(diffs), "confirm_bad": len(conf_bad), "confirm_n": n_conf}
        for which in ("main", "multi"):
            mb, mp = metrics(dec_b, allsurf, which), metrics(dec_p, allsurf, which)
            show(f"{which} binary", mb)
            show(f"{which} port", mp)
            report[mode][which] = {"binary": mb, "port": mp}

    if len(modes) == 2:
        print("\n── binary: chunk_top vs share, per surface (main + auxiliary) ──")
        gained, lost = Counter(), Counter()
        examples = []
        for si, s in enumerate(allsurf):
            (_, f0), (_, f1) = binary["share"][si], binary["chunk_top"][si]
            g, l = f1 - f0, f0 - f1
            for w in g:
                gained[R.label(w, s["expected"])] += 1
            for w in l:
                lost[R.label(w, s["expected"])] += 1
            if g or l:
                examples.append((si, sorted(g), sorted(l)))
        print(f"  fired ways gained by label: {dict(gained)}; lost: {dict(lost)}; "
              f"surfaces changed {len(examples)}/{len(allsurf)}")
        def show_ex(si, g, l):
            s = allsurf[si]
            tag = lambda ws: [f"{w} ({R.label(w, s['expected'])})" for w in ws]  # noqa: E731
            print(f"  s{si} [{s['shape']}] gained {tag(g)} lost {tag(l)}\n"
                  f"      expected {s['expected']}\n      {s['surface']}")
        print("  first 12 changed surfaces:")
        for ex in examples[:12]:
            show_ex(*ex)
        print("  every surface that lost a fired way:")
        for ex in examples:
            if ex[2]:
                show_ex(*ex)
        report["mode_diff"] = {"gained": dict(gained), "lost": dict(lost), "changed": len(examples)}
    report["way_embed"] = str(embed)
    (O / "admission-binary.json").write_text(json.dumps(report, indent=1))


if __name__ == "__main__":
    main()
