#!/usr/bin/env python3
"""Judge input A/B (ADR-701 §4, issue #664): does the relevance judge reject more
irrelevant candidates without losing relevant ones when its candidate text
carries the matcher's share, margin and band, the described route, or both?

Arms, varying only each candidate's text (judge.rs `Candidate.text`):
  A  today's way_text: the route `a › b › c`, then the way's description
  B  A, then a line with the band, share and margin (ADR-701 §4)
  C  the described route: the route, one line per ancestor directory with its
     description (issue #664's resolution: the way in that directory; for a
     node with no way, region-descriptions.tsv), then the way's description
  D  C, then B's line

Everything else is the production request (tools/ways-agent/src/net.rs `judge`,
tools/ways-agent-core/src/judge.rs, the shipped `anthropic` profile): model
claude-haiku-4-5, temperature 0, max_tokens 64 + 48n, the SYSTEM prompt, the
strict record_judgements tool forced by tool_choice, one request per prompt
carrying every candidate, the last turn cut to 1,200 characters, `<` neutralised.

Sets:
  golden  `ways author golden --tsv` + tests/routing-golden.tsv;
          candidates are the alias-cosine top 3 (targeted prompt) or top 2
          (`none` prompt), in rank order; relevant = the expected way (strict)
          or the expected way, its parent or a child (family).
  fires   the ADR-195 probe's real prompt-lane fires (set 1 real_relevant and
          real_irrelevant, set 2 random), read from the probe's private data
          directory; its turns never leave that directory except in the API
          request, and nothing from them is written to the committed cache.
          Skipped when the directory is absent.

Share is a softmax at tau 0.08 over a prompt's top 8 ways by alias cosine and
margin the cosine gap to the next-ranked way, as scan/candidate_log.rs computes
them. Band: share >= 0.5 strong, < 0.35 weak, else uncertain (ADR-700 §4).

    judge_ab.py prepare     build the corpus and the candidate groups
    judge_ab.py estimate    count input tokens (free endpoint) and project cost
    judge_ab.py run [--cap USD] [--limit N]   call the judge, cached, with a running tally
    judge_ab.py analyze     tables to stdout (results-judge-ab.md is written from them)

The key is read from $ANTHROPIC_API_KEY, else $WAYS_JUDGE_KEY_FILE, else
~/.config/agent-ways/keys/anthropic (the ways agent's key file). It is never
printed or logged. Results cache: judge-ab-calls.jsonl beside this script.
"""

import hashlib
import json
import math
import os
import random
import shutil
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))
import run  # noqa: E402  (way-embed and model paths, batch_scores)

OUT = Path(os.environ.get("OUT", "/tmp/judge-ab-out"))
CACHE = HERE / "judge-ab-calls.jsonl"
# The synthetic rows are exported from the per-way golden sidecars (ADR-701 §9)
# into OUT at prepare time; the routing rows stay a checked-in file.
GOLDEN = [OUT / "golden-synthetic.tsv", REPO / "tests" / "routing-golden.tsv"]
PROBE = Path.home() / ".local/state/agent-ways/probes/yesno-gate"

# Production request (net.rs, judge.rs, profiles.yaml).
MODEL = "claude-haiku-4-5"
API = "https://api.anthropic.com/v1/messages"
ANTHROPIC_VERSION = "2023-06-01"
THRESHOLD = 0.3
MAX_TURN_CHARS = 1200
SYSTEM = ("You are a relevance gate for a coding assistant's guidance system. You are given "
          "several pieces of guidance and the most recent turns of a conversation. Judge each "
          "piece of guidance on its own. Call the record_judgements tool exactly once, with one "
          "entry per piece of guidance. `relevant` is your yes/no answer; `confidence` is your "
          "probability, from 0 to 1, that this answer is correct.")
INSTRUCTION = ("Decide whether each piece of guidance is relevant to what the conversation is "
               "doing in its most recent turns.")
TOOL_NAME = "record_judgements"
TOOL_DESCRIPTION = "Record, for each piece of guidance, whether it is relevant to the recent conversation."
PRICE_IN, PRICE_OUT = 1.0 / 1e6, 5.0 / 1e6   # Haiku 4.5, USD per token (cost.rs)

# Scores (scan/candidate_log.rs, ADR-700 §4).
TAU, WINDOW = 0.08, 8
STRONG, WEAK = 0.5, 0.35

ARMS = ("A", "B", "C", "D")
TOPK = {"target": 3, "none": 2}


# ── corpus and candidates ────────────────────────────────────────────────────

def build_alias():
    """Alias corpus from this checkout's hooks/ways, as run.py builds it."""
    ways = REPO / "bin" / "ways"
    ways = str(ways) if ways.exists() else shutil.which("ways")
    out = OUT / "alias"
    out.mkdir(parents=True, exist_ok=True)
    subprocess.run([ways, "corpus", "--ways-dir", str(run.WAYS), "--output", str(out), "-q"],
                   check=True, capture_output=True)
    rows = [json.loads(l) for l in (out / "ways-corpus-en.jsonl").read_text().splitlines()]
    rows = [r for r in rows if not r["id"].startswith("-")]
    path = OUT / "alias.jsonl"
    path.write_text("".join(json.dumps(r) + "\n" for r in rows))
    return path, {r["id"]: r["description"] for r in rows}


def cosines(alias, texts):
    """Per text, {way: cosine} for every way: the texts embedded with the same
    way-embed and model, against the stored alias vectors (all unit length).
    `way-embed match --threshold 0.0` omits negative cosines, which the share
    window and the margins need."""
    import numpy as np
    rows = [json.loads(l) for l in alias.read_text().splitlines()]
    W = np.array([r["embedding"] for r in rows])
    raw, out = OUT / "texts.raw.jsonl", OUT / "texts.jsonl"
    raw.write_text("".join(json.dumps({"id": f"q{i}", "description": t, "vocabulary": "",
                                       "threshold": 0, "embed_threshold": 0.0}) + "\n"
                           for i, t in enumerate(texts)))
    subprocess.run([str(run.EMBED), "generate", "--corpus", str(raw), "--model", str(run.MODEL),
                    "--output", str(out)], check=True, capture_output=True)
    Q = np.array([json.loads(l)["embedding"] for l in out.read_text().splitlines()])
    C = Q @ W.T
    return [{r["id"]: float(C[i, j]) for j, r in enumerate(rows)} for i in range(len(texts))]


def authored_nodes():
    out = {}
    for line in (HERE / "region-descriptions.tsv").read_text().splitlines():
        if line.startswith("#") or line.startswith("node\t") or not line.strip():
            continue
        node, text = line.split("\t", 1)
        out[node] = text.strip()
    return out


def route(way_id):
    return " › ".join(s for s in way_id.split("/") if s)


def described_route(way_id, desc, authored):
    parts = way_id.split("/")
    lines = [route(way_id)]
    for i in range(1, len(parts)):
        node = "/".join(parts[:i])
        d = desc.get(node) or authored.get(node)
        if d:
            lines.append(f"within {route(node)}: {d.strip()}")
    lines.append(desc[way_id].strip())
    return "\n".join(lines)


def band(share):
    return "strong" if share >= STRONG else "weak" if share < WEAK else "uncertain"


def score_line(c):
    return (f"matcher: {c['band']} match (share {c['share']:.2f} of the top {WINDOW} ways, "
            f"margin {c['margin']:.3f} over the next way)")


def candidate_text(arm, c, desc, authored):
    wid = c["way"]
    base = (f"{route(wid)}\n{desc[wid].strip()}" if arm in "AB"
            else described_route(wid, desc, authored))
    return base + ("\n" + score_line(c) if arm in "BD" else "")


def scored(cos):
    """Rank by cosine (ties by id), share over the top WINDOW, margin to the next."""
    ranked = sorted(cos.items(), key=lambda kv: (-kv[1], kv[0]))
    denom = sum(math.exp(c / TAU) for _, c in ranked[:WINDOW])
    out = {}
    for i, (w, c) in enumerate(ranked):
        share = math.exp(c / TAU) / denom
        margin = c - ranked[i + 1][1] if i + 1 < len(ranked) else 0.0
        out[w] = {"way": w, "rank": i + 1, "cosine": round(c, 4), "share": round(share, 4),
                  "margin": round(margin, 4), "band": band(share)}
    return ranked, out


def family(a, b):
    return a == b or a.startswith(b + "/") and "/" not in a[len(b) + 1:] \
        or b.startswith(a + "/") and "/" not in b[len(a) + 1:]


def render_turn(text):
    """judge.rs render_turns for one user turn: whitespace collapsed, `<`
    neutralised, cut to the last MAX_TURN_CHARS characters."""
    t = " ".join(text.split()).replace("<", "‹")
    if len(t) > MAX_TURN_CHARS:
        t = "…" + t[len(t) - MAX_TURN_CHARS + 1:]
    return f"User: {t}"


def ways_bin():
    """The ways binary build_alias uses: this checkout's bin/ways, else PATH."""
    b = REPO / "bin" / "ways"
    return b if b.exists() else shutil.which("ways")


def prepare():
    OUT.mkdir(parents=True, exist_ok=True)
    alias, desc = build_alias()
    known = set(desc)
    groups = []

    GOLDEN[0].write_text(subprocess.run(
        [str(ways_bin()), "author", "golden", "--tsv", "--ways-dir", str(REPO / "hooks" / "ways")],
        check=True, capture_output=True, text=True).stdout)
    golden = []
    for p in GOLDEN:
        for line in p.read_text().splitlines():
            x = line.split("\t")
            if len(x) >= 2 and x[0] != "prompt":
                golden.append((x[0], x[1], p.name))
    dropped = [g for g in golden if g[1] != "none" and g[1] not in known]
    golden = [g for g in golden if g[1] == "none" or g[1] in known]
    cos = cosines(alias, [g[0] for g in golden])
    for i, ((prompt, exp, src), d) in enumerate(zip(golden, cos)):
        ranked, sc = scored(d)
        k = TOPK["none" if exp == "none" else "target"]
        cands = []
        for w, _ in ranked[:k]:
            c = dict(sc[w])
            c["strict"] = int(w == exp)
            c["family"] = int(exp != "none" and family(w, exp))
            cands.append(c)
        groups.append({"set": "golden", "gid": f"g{i:03d}", "src": src, "expected": exp,
                       "turn": render_turn(prompt), "cands": cands})

    fires_note = "absent"
    if (PROBE / "eval" / "units.jsonl").exists():
        labels = {}
        for f in ("eval.jsonl", "random40.jsonl"):
            for l in (PROBE / "data" / f).read_text().splitlines():
                r = json.loads(l)
                labels[r["id"]] = (r["label"], r["category"])
        units = [json.loads(l) for l in (PROBE / "eval" / "units.jsonl").read_text().splitlines()]
        units = [u for u in units if u["wv"] == "B" and u["cv"] == "T1"
                 and labels[u["id"]][1] in ("real_relevant", "real_irrelevant", "random")]
        missing = [u for u in units if u["way_id"] not in known]
        units = [u for u in units if u["way_id"] in known]
        by_q = defaultdict(list)
        for u in units:
            by_q[(u["set"], u["query"])].append(u)
        keys = sorted(by_q)
        # The probe's query is `User: <last turn>`, already one line.
        texts = [k[1].split(": ", 1)[1] for k in keys]
        cos = cosines(alias, texts)
        for j, (k, d) in enumerate(zip(keys, cos)):
            _, sc = scored(d)
            cands = []
            for u in sorted(by_q[k], key=lambda u: sc[u["way_id"]]["rank"]):
                c = dict(sc[u["way_id"]])
                c["strict"] = c["family"] = int(labels[u["id"]][0] == 1)
                c["item"] = u["id"]
                cands.append(c)
            groups.append({"set": "fires", "gid": f"f{j:03d}", "src": k[0], "expected": None,
                           "turn": render_turn(texts[j]), "cands": cands, "private": True})
        fires_note = (f"{len(units)} items in {len(keys)} prompts; "
                      f"{len(missing)} items dropped (way no longer in the corpus)")

    (OUT / "groups.json").write_text(json.dumps({"groups": groups, "desc": desc}, indent=0))
    n = Counter(g["set"] for g in groups)
    print(f"golden: {n['golden']} prompts, {sum(len(g['cands']) for g in groups if g['set'] == 'golden')} "
          f"candidates; {len(dropped)} rows dropped (expected way not in corpus)")
    print(f"fires: {fires_note}")


def load_groups():
    data = json.loads((OUT / "groups.json").read_text())
    return data["groups"], data["desc"]


# ── requests ─────────────────────────────────────────────────────────────────

def tool_schema(n):
    return {"type": "object", "additionalProperties": False, "required": ["judgements"],
            "properties": {"judgements": {"type": "array", "items": {
                "type": "object", "additionalProperties": False,
                "required": ["id", "relevant", "confidence"],
                "properties": {"id": {"type": "string", "enum": [f"g{i}" for i in range(1, n + 1)]},
                               "relevant": {"type": "boolean"},
                               "confidence": {"type": "number"}}}}}}


def request_body(g, arm, desc, authored):
    texts = [candidate_text(arm, c, desc, authored) for c in g["cands"]]
    guidance = "\n\n".join(f'<guidance id="g{i + 1}">\n{t.replace("<", "‹")}\n</guidance>'
                           for i, t in enumerate(texts))
    prompt = f"{INSTRUCTION}\n\n{guidance}\n\n<conversation>\n{g['turn']}\n</conversation>"
    n = len(texts)
    return {"model": MODEL, "max_tokens": 64 + 48 * n, "temperature": 0, "system": SYSTEM,
            "tools": [{"name": TOOL_NAME, "description": TOOL_DESCRIPTION, "strict": True,
                       "input_schema": tool_schema(n)}],
            "tool_choice": {"type": "tool", "name": TOOL_NAME},
            "messages": [{"role": "user", "content": prompt}]}


def api_key():
    if os.environ.get("ANTHROPIC_API_KEY"):
        return os.environ["ANTHROPIC_API_KEY"].strip()
    f = os.environ.get("WAYS_JUDGE_KEY_FILE", str(Path.home() / ".config/agent-ways/keys/anthropic"))
    return Path(f).read_text().strip()


def post(url, body, key, timeout=60):
    req = urllib.request.Request(url, data=json.dumps(body).encode(), method="POST", headers={
        "x-api-key": key, "anthropic-version": ANTHROPIC_VERSION, "content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def p_yes(relevant, conf):
    c = min(1.0, max(0.0, conf)) if isinstance(conf, (int, float)) and math.isfinite(conf) else 0.5
    return c if relevant else 1.0 - c


def parse(reply, n):
    block = next((b for b in reply.get("content", []) if b.get("type") == "tool_use"), None)
    if not block:
        raise ValueError(f"no tool_use block (stop_reason {reply.get('stop_reason')})")
    out = [None] * n
    for j in block["input"].get("judgements", []):
        idx = j.get("id", "")[1:]
        if idx.isdigit() and 1 <= int(idx) <= n and out[int(idx) - 1] is None:
            out[int(idx) - 1] = p_yes(j["relevant"], j["confidence"])
    if any(p is None for p in out):
        raise ValueError(f"judged {sum(p is not None for p in out)} of {n}")
    return out


def body_hash(body):
    return hashlib.sha256(json.dumps(body, sort_keys=True).encode()).hexdigest()[:16]


def load_cache():
    out = {}
    if CACHE.exists():
        for l in CACHE.read_text().splitlines():
            r = json.loads(l)
            if r.get("error") is None:
                out[(r["gid"], r["arm"])] = r
    return out


def estimate():
    groups, desc = load_groups()
    authored = authored_nodes()
    key = api_key()
    rng = random.Random(1)
    print(f"{'set':7} {'arm':3} {'calls':>5} {'cands':>6} {'in tok/call':>11} {'est in tok':>10} "
          f"{'est out tok':>11} {'est USD':>8}")
    total = 0.0
    for s in ("golden", "fires"):
        gs = [g for g in groups if g["set"] == s]
        if not gs:
            continue
        sample = rng.sample(gs, min(25, len(gs)))
        for arm in ARMS:
            counts = []
            for g in sample:
                b = request_body(g, arm, desc, authored)
                b = {k: v for k, v in b.items() if k not in ("max_tokens", "temperature")}
                counts.append(post(API + "/count_tokens", b, key)["input_tokens"])
            per_char = sum(counts) / sum(len(json.dumps(request_body(g, arm, desc, authored)))
                                         for g in sample)
            tin = per_char * sum(len(json.dumps(request_body(g, arm, desc, authored))) for g in gs)
            ncand = sum(len(g["cands"]) for g in gs)
            tout = 30 * len(gs) + 25 * ncand   # ADR-195 addendum: ~20/candidate plus overhead
            usd = tin * PRICE_IN + tout * PRICE_OUT
            total += usd
            print(f"{s:7} {arm:3} {len(gs):5d} {ncand:6d} {sum(counts) / len(counts):11.0f} "
                  f"{tin:10.0f} {tout:11.0f} {usd:8.3f}")
    print(f"projected total: ${total:.2f} (count_tokens is free)")


def run_calls(cap, limit):
    groups, desc = load_groups()
    authored = authored_nodes()
    cache = load_cache()
    spent = sum(r["usd"] for r in cache.values())
    todo = [(g, arm) for g in groups for arm in ARMS if (g["gid"], arm) not in cache]
    if limit:
        todo = todo[:limit]
    print(f"cached {len(cache)} calls (${spent:.3f}); to run {len(todo)}; cap ${cap:.2f}")
    key = api_key()
    lock = threading.Lock()
    state = {"spent": spent, "done": 0, "stop": False}
    f = CACHE.open("a")

    def one(item):
        g, arm = item
        if state["stop"]:
            return
        body = request_body(g, arm, desc, authored)
        n = len(g["cands"])
        rec = {"set": g["set"], "gid": g["gid"], "arm": arm, "model": MODEL, "n": n,
               "req": body_hash(body), "ways": [c["way"] for c in g["cands"]],
               "strict": [c["strict"] for c in g["cands"]], "family": [c["family"] for c in g["cands"]],
               "share": [c["share"] for c in g["cands"]], "margin": [c["margin"] for c in g["cands"]],
               "band": [c["band"] for c in g["cands"]], "cosine": [c["cosine"] for c in g["cands"]]}
        if g["set"] == "golden":
            rec["expected"] = g["expected"]
        else:
            rec["items"] = [c["item"] for c in g["cands"]]
        err, reply, t0 = None, None, time.perf_counter()
        for attempt in range(5):
            try:
                reply = post(API, body, key)
                break
            except urllib.error.HTTPError as e:
                err = f"http_{e.code}"
                if e.code in (429, 500, 502, 503, 529):
                    time.sleep(2 ** attempt * 2)
                    continue
                break
            except Exception as e:  # transport
                err = type(e).__name__
                time.sleep(2 ** attempt)
        rec["ms"] = round((time.perf_counter() - t0) * 1000)
        usage = (reply or {}).get("usage", {})
        rec["usage"] = {k: usage.get(k, 0) for k in ("input_tokens", "output_tokens")}
        rec["usd"] = usage.get("input_tokens", 0) * PRICE_IN + usage.get("output_tokens", 0) * PRICE_OUT
        if reply is not None:
            err = None
            try:
                rec["p_yes"] = [round(p, 4) for p in parse(reply, n)]
            except Exception as e:
                err = f"answer: {e}"
        rec["error"] = err
        with lock:
            f.write(json.dumps(rec) + "\n")
            f.flush()
            state["spent"] += rec["usd"]
            state["done"] += 1
            if state["done"] % 50 == 0:
                print(f"  {state['done']}/{len(todo)} calls, running spend ${state['spent']:.3f}", flush=True)
            if state["spent"] > cap:
                state["stop"] = True

    with ThreadPoolExecutor(4) as ex:
        list(ex.map(one, todo))
    f.close()
    print(f"done {state['done']} calls; total spend ${state['spent']:.3f}"
          + ("; STOPPED at cap" if state["stop"] else ""))


# ── analysis ─────────────────────────────────────────────────────────────────

def auc(pos, neg):
    if not pos or not neg:
        return float("nan")
    # rank-based Mann-Whitney with ties
    vals = sorted([(v, 1) for v in pos] + [(v, 0) for v in neg])
    ranks, i = {}, 0
    while i < len(vals):
        j = i
        while j < len(vals) and vals[j][0] == vals[i][0]:
            j += 1
        ranks[vals[i][0]] = (i + j + 1) / 2
        i = j
    rsum = sum(ranks[v] for v in pos)
    return (rsum - len(pos) * (len(pos) + 1) / 2) / (len(pos) * len(neg))


def units(recs, label):
    """[(gid, p, y)] for one arm's records."""
    return [(r["gid"], p, y) for r in recs for p, y in zip(r["p_yes"], r[label])]


def stats(u, thr=THRESHOLD):
    pos = [p for _, p, y in u if y]
    neg = [p for _, p, y in u if not y]
    rej = sum(p < thr for p in neg) / len(neg) if neg else float("nan")
    lost = sum(p < thr for p in pos) / len(pos) if pos else float("nan")
    return {"auc": auc(pos, neg), "rej": rej, "lost": lost, "npos": len(pos), "nneg": len(neg)}


def best_threshold(u):
    cands = sorted({p for _, p, _ in u} | {0.0}) + [1.01]
    best = None
    for t in cands:
        s = stats(u, t)
        j = s["rej"] - s["lost"]
        if best is None or j > best[0] + 1e-12:
            best = (j, t, s)
    return best


def bootstrap(by_arm, label, a, b, n=2000, seed=7):
    """Paired over prompts: diff (b - a) in AUC, rejected and lost at THRESHOLD."""
    gids = sorted({r["gid"] for r in by_arm[a]})
    idx = {arm: {r["gid"]: r for r in by_arm[arm]} for arm in (a, b)}
    rng = random.Random(seed)

    def metric(sample):
        out = []
        for arm in (a, b):
            recs = [idx[arm][g] for g in sample]
            out.append(stats(units(recs, label)))
        return {k: out[1][k] - out[0][k] for k in ("auc", "rej", "lost")}

    point = metric(gids)
    draws = defaultdict(list)
    for _ in range(n):
        s = [gids[rng.randrange(len(gids))] for _ in gids]
        for k, v in metric(s).items():
            if not math.isnan(v):
                draws[k].append(v)
    ci = {k: (sorted(v)[int(0.025 * len(v))], sorted(v)[int(0.975 * len(v)) - 1]) for k, v in draws.items()}
    return point, ci


def analyze():
    recs = [json.loads(l) for l in CACHE.read_text().splitlines()]
    ok = [r for r in recs if r.get("error") is None]
    err = [r for r in recs if r.get("error") is not None]
    spent = sum(r["usd"] for r in recs)
    tin = sum(r["usage"]["input_tokens"] for r in recs)
    tout = sum(r["usage"]["output_tokens"] for r in recs)
    print(f"calls {len(recs)} ({len(err)} failed), input {tin} tok, output {tout} tok, spend ${spent:.3f}")
    if err:
        print("  failures:", Counter(r["error"] for r in err))
    for arm in ARMS:
        la = sorted(r["ms"] for r in ok if r["arm"] == arm)
        ti = [r["usage"]["input_tokens"] for r in ok if r["arm"] == arm]
        print(f"  arm {arm}: p50 {la[len(la) // 2]} ms, p95 {la[int(.95 * len(la))]} ms, "
              f"mean input {sum(ti) / len(ti):.0f} tok")

    for s in ("golden", "fires"):
        srecs = [r for r in ok if r["set"] == s]
        if not srecs:
            continue
        # Keep prompts every arm answered, so comparisons are paired.
        full = {g for g, c in Counter(r["gid"] for r in srecs).items() if c == len(ARMS)}
        by_arm = {arm: sorted((r for r in srecs if r["arm"] == arm and r["gid"] in full),
                              key=lambda r: r["gid"]) for arm in ARMS}
        labels = ("strict", "family") if s == "golden" else ("strict",)
        for label in labels:
            u0 = units(by_arm["A"], label)
            print(f"\n## {s}, labels {label}: {len(full)} prompts, "
                  f"{sum(y for _, _, y in u0)} relevant / {sum(1 - y for _, _, y in u0)} irrelevant")
            print(f"| arm | AUC | irrelevant rejected @0.3 | relevant lost @0.3 | best thr (Youden) | "
                  f"rejected / lost at best |")
            print("|---|---|---|---|---|---|")
            for arm in ARMS:
                u = units(by_arm[arm], label)
                st = stats(u)
                j, t, bs = best_threshold(u)
                print(f"| {arm} | {st['auc']:.3f} | {st['rej']:.1%} | {st['lost']:.1%} | {t:.2f} | "
                      f"{bs['rej']:.1%} / {bs['lost']:.1%} |")
            print(f"\n| pair | ΔAUC [95% CI] | Δ rejected [95% CI] | Δ lost [95% CI] |")
            print("|---|---|---|---|")
            for a, b in (("A", "B"), ("A", "C"), ("A", "D"), ("C", "D"), ("B", "D")):
                pt, ci = bootstrap(by_arm, label, a, b)
                cell = lambda k, pct: (f"{pt[k]:+.1%} [{ci[k][0]:+.1%}, {ci[k][1]:+.1%}]" if pct
                                       else f"{pt[k]:+.3f} [{ci[k][0]:+.3f}, {ci[k][1]:+.3f}]")
                print(f"| {b} − {a} | {cell('auc', False)} | {cell('rej', True)} | {cell('lost', True)} |")

        # Anchoring: pass rate (P(yes) >= 0.3) by band, by arm, split by label.
        label = "strict"
        print(f"\n### {s}: pass rate at 0.3 by band (strict labels), n in brackets")
        print("| band, label | A | B | C | D |")
        print("|---|---|---|---|---|")
        for b in ("strong", "uncertain", "weak"):
            for y, name in ((1, "relevant"), (0, "irrelevant")):
                cells = []
                for arm in ARMS:
                    ps = [p for r in by_arm[arm] for p, yy, bb in zip(r["p_yes"], r[label], r["band"])
                          if yy == y and bb == b]
                    cells.append(f"{sum(p >= THRESHOLD for p in ps) / len(ps):.0%} ({len(ps)})" if ps else "–")
                print(f"| {b}, {name} | " + " | ".join(cells) + " |")
        # Verdict-band agreement: share of candidates whose verdict matches the
        # band's suggestion (strong -> pass, weak -> block), and flips vs A.
        for arm in ("B", "D"):
            base = "A" if arm == "B" else "C"
            flips = Counter()
            for ra, rb in zip(by_arm[base], by_arm[arm]):
                for pa, pb, bb, yy in zip(ra["p_yes"], rb["p_yes"], rb["band"], rb[label]):
                    va, vb = pa >= THRESHOLD, pb >= THRESHOLD
                    if va != vb:
                        toward = (vb and bb == "strong") or (not vb and bb == "weak")
                        flips[("toward band" if toward else "against band",
                               "correct" if vb == bool(yy) else "wrong")] += 1
            agree = [((p >= THRESHOLD) == (bb == "strong"))
                     for r in by_arm[arm] for p, bb in zip(r["p_yes"], r["band"]) if bb != "uncertain"]
            agree0 = [((p >= THRESHOLD) == (bb == "strong"))
                      for r in by_arm[base] for p, bb in zip(r["p_yes"], r["band"]) if bb != "uncertain"]
            print(f"\n{s} {arm} vs {base}: verdict agrees with band (strong/weak only) "
                  f"{sum(agree) / len(agree):.1%} vs {sum(agree0) / len(agree0):.1%}; "
                  f"verdict flips: {dict(flips)}")


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "analyze"
    if cmd == "prepare":
        prepare()
    elif cmd == "estimate":
        estimate()
    elif cmd == "run":
        args = sys.argv[2:]
        cap = float(args[args.index("--cap") + 1]) if "--cap" in args else 25.0
        limit = int(args[args.index("--limit") + 1]) if "--limit" in args else 0
        run_calls(cap, limit)
    elif cmd == "analyze":
        analyze()
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
