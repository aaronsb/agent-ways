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
    judge_ab.py run [--cap USD] [--limit N] [--arm A] [--model M] [--provider anthropic|openrouter]
                    [--sample N] [--max-calls N]
                            call the judge, cached, with a running tally
    judge_ab.py analyze     tables to stdout (results-judge-ab.md is written from them)
    judge_ab.py compare [--model M] [--provider P] [--sample N]
                            paired 4.5 vs a second model on the sampled groups

`--provider openrouter` builds the body as net.rs does for OpenRouter (chat completions, forced
function tool_choice, max_tokens 96 + 64n, temperature 0 only when the model accepts sampling) and
reads $OPENROUTER_API_KEY, else ~/.config/agent-ways/keys/openrouter; it records the provider's
reported cost. `--sample N` draws a seeded stratified sample of golden groups.
$WAYS_BIN overrides the ways binary (default: bin/ways, else PATH).

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
    ways = ways_bin()
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
    """The ways binary: $WAYS_BIN, else this checkout's bin/ways, else PATH."""
    if os.environ.get("WAYS_BIN"):
        return os.environ["WAYS_BIN"]
    b = REPO / "bin" / "ways"
    found = str(b) if b.exists() else shutil.which("ways")
    if not found:
        sys.exit("ways binary not found: build with make ways or put ways on PATH")
    return found


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


def accepts_sampling(model):
    """profile.rs accepts_sampling for the ids used here."""
    import re
    i = model.split(":")[0]
    if "/" in i:
        p, rest = i.split("/", 1)
        if p != "anthropic":
            return True
        i = rest
    if not i.startswith("claude-"):
        return False
    nums = [x for x in re.split(r"[-.]", i[len("claude-"):]) if x and len(x) <= 2 and x.isdigit()]
    if not nums:
        return False
    if nums[0] == "3":
        return True
    return nums[0] == "4" and (len(nums) == 1 or int(nums[1]) <= 6)


def request_body(g, arm, desc, authored, model=MODEL, provider="anthropic"):
    texts = [candidate_text(arm, c, desc, authored) for c in g["cands"]]
    guidance = "\n\n".join(f'<guidance id="g{i + 1}">\n{t.replace("<", "‹")}\n</guidance>'
                           for i, t in enumerate(texts))
    prompt = f"{INSTRUCTION}\n\n{guidance}\n\n<conversation>\n{g['turn']}\n</conversation>"
    n = len(texts)
    if provider == "openrouter":
        body = {"model": model, "max_tokens": 96 + 64 * n,
                "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}],
                "tools": [{"type": "function", "function": {
                    "name": TOOL_NAME, "description": TOOL_DESCRIPTION, "parameters": tool_schema(n)}}],
                "tool_choice": {"type": "function", "function": {"name": TOOL_NAME}}}
        if accepts_sampling(model):
            body["temperature"] = 0
        return body
    body = {"model": model, "max_tokens": 64 + 48 * n, "system": SYSTEM,
            "tools": [{"name": TOOL_NAME, "description": TOOL_DESCRIPTION, "strict": True,
                       "input_schema": tool_schema(n)}],
            "tool_choice": {"type": "tool", "name": TOOL_NAME},
            "messages": [{"role": "user", "content": prompt}]}
    if accepts_sampling(model):
        body["temperature"] = 0
    return body


def api_key(provider="anthropic"):
    if provider == "openrouter":
        if os.environ.get("OPENROUTER_API_KEY"):
            return os.environ["OPENROUTER_API_KEY"].strip()
        return (Path.home() / ".config/agent-ways/keys/openrouter").read_text().strip()
    if os.environ.get("ANTHROPIC_API_KEY"):
        return os.environ["ANTHROPIC_API_KEY"].strip()
    f = os.environ.get("WAYS_JUDGE_KEY_FILE", str(Path.home() / ".config/agent-ways/keys/anthropic"))
    return Path(f).read_text().strip()


def post(url, body, key, timeout=60, provider="anthropic"):
    hdr = ({"Authorization": f"Bearer {key}", "X-Title": "agent-ways"} if provider == "openrouter"
           else {"x-api-key": key, "anthropic-version": ANTHROPIC_VERSION})
    req = urllib.request.Request(url, data=json.dumps(body).encode(), method="POST",
                                 headers={**hdr, "content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def p_yes(relevant, conf):
    c = min(1.0, max(0.0, conf)) if isinstance(conf, (int, float)) and math.isfinite(conf) else 0.5
    return c if relevant else 1.0 - c


def parse(reply, n, provider="anthropic"):
    if provider == "openrouter":
        args = (((reply.get("choices") or [{}])[0].get("message") or {}).get("tool_calls") or [{}])[0] \
            .get("function", {}).get("arguments")
        if not isinstance(args, str):
            raise ValueError("no tool call")
        out = [None] * n
        for j in json.loads(args).get("judgements", []):
            idx = j.get("id", "")[1:]
            if idx.isdigit() and 1 <= int(idx) <= n and out[int(idx) - 1] is None:
                out[int(idx) - 1] = p_yes(j["relevant"], j["confidence"])
        if any(p is None for p in out):
            raise ValueError(f"judged {sum(p is not None for p in out)} of {n}")
        return out
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
                out[(r["req"], r["arm"], r.get("model", MODEL), r.get("provider", "anthropic"))] = r
    return out


HAIKU55_IN, HAIKU55_OUT = 0.10 / 1e6, 0.50 / 1e6   # cost.rs list price, USD per token
TOKENIZER_55 = 1.3                                   # net.rs: Haiku 5.5 counts ~30% more tokens


def estimate_sample(n_sample, arm="A"):
    """Free input-token count of the sampled groups (Anthropic count_tokens, Haiku 4.5
    tokenizer, scaled by TOKENIZER_55), priced at the Haiku 5.5 list price; output is
    bounded by max_tokens (96 + 64n), the worst case for a forced tool call."""
    groups, desc = load_groups()
    authored = authored_nodes()
    key = api_key()
    sample = sample_groups(groups, n_sample)
    tin = tout_max = tout_typ = 0
    for g in sample:
        b = request_body(g, arm, desc, authored)
        b = {k: v for k, v in b.items() if k not in ("max_tokens", "temperature")}
        tin += post(API + "/count_tokens", b, key)["input_tokens"] * TOKENIZER_55
        n = len(g["cands"])
        tout_max += 96 + 64 * n
        tout_typ += 30 + 25 * n
    c = lambda o: tin * HAIKU55_IN + o * HAIKU55_OUT
    print(f"{len(sample)} groups ({sum(g['expected'] == 'none' for g in sample)} none), "
          f"{sum(len(g['cands']) for g in sample)} candidates, arm {arm}")
    print(f"input ~{tin:.0f} tok (x{TOKENIZER_55} for the 5.5 tokenizer); output typical ~{tout_typ}, max {tout_max}")
    print(f"projected cost: typical ${c(tout_typ):.5f}, worst case ${c(tout_max):.5f} (count_tokens is free)")


def list_price(model):
    """USD per token (input, output) for the models this script prices; an unknown
    model is priced at the dearest known rate so a projection errs high."""
    if "haiku-5" in model:
        return HAIKU55_IN, HAIKU55_OUT
    return PRICE_IN, PRICE_OUT


def projected_cost(body, model):
    """A conservative cost of one call: input tokens at 1 per 2.5 characters of the
    request JSON (the real ratio is near 1 per 4), plus max_tokens of output."""
    pin, pout = list_price(model)
    return len(json.dumps(body)) / 2.5 * pin + body["max_tokens"] * pout


def estimate():
    args = sys.argv[2:]
    if "--sample" in args:
        estimate_sample(int(args[args.index("--sample") + 1]),
                        args[args.index("--arm") + 1] if "--arm" in args else "A")
        return
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


def sample_groups(groups, n, seed=20261008):
    """Seeded stratified draw of golden groups: targeted (expected way among the
    candidates), targeted-miss and `none` groups kept in balance, spread over
    candidate counts. Half the quota goes to `none` groups, half to targeted ones."""
    gold = [g for g in groups if g["set"] == "golden"]
    rng = random.Random(seed)
    nones = [g for g in gold if g["expected"] == "none"]
    targ = [g for g in gold if g["expected"] != "none"]

    def spread(pool, k):
        # round-robin over (candidate count, hit/miss) strata, random within each
        strata = defaultdict(list)
        for g in pool:
            hit = any(c["strict"] for c in g["cands"])
            strata[(len(g["cands"]), hit)].append(g)
        for v in strata.values():
            rng.shuffle(v)
        keys = sorted(strata)
        out = []
        while len(out) < k and any(strata[x] for x in keys):
            for x in keys:
                if strata[x] and len(out) < k:
                    out.append(strata[x].pop())
        return out

    # `none` groups are scarce; whatever they cannot fill goes to the targeted ones.
    k_none = min(len(nones), n // 2)
    picked = spread(nones, k_none) + spread(targ, n - k_none)
    return sorted(picked, key=lambda g: g["gid"])


def post_openrouter_cost(reply):
    u = (reply or {}).get("usage") or {}
    return u.get("cost"), u.get("prompt_tokens", 0), u.get("completion_tokens", 0)


def run_calls(cap, limit, arm=None, model=MODEL, provider="anthropic", sample=0, max_calls=0):
    """`cap` is this run's spend limit in USD (cached spend is not counted), or None.
    With a cap the run is sequential and each call is refused up front when the
    spend so far plus the call's conservative projection would cross it."""
    sequential = cap is not None or provider == "openrouter"
    cap = 25.0 if cap is None else cap
    groups, desc = load_groups()
    authored = authored_nodes()
    cache = load_cache()
    # Rows are found by request hash: gids are the index in one `prepare` output
    # and go stale when the golden set changes.
    key_of = lambda g, a: (body_hash(request_body(g, a, desc, authored, model, provider)), a, model, provider)
    spent = sum(r["usd"] for k, r in cache.items() if k[2:] == (model, provider))
    if sample:
        groups = sample_groups(groups, sample)
    arms = (arm,) if arm else ARMS
    todo = [(g, a) for g in groups for a in arms if key_of(g, a) not in cache]
    if limit:
        todo = todo[:limit]
    if max_calls:
        todo = todo[:max_calls]
    print(f"cached {len(cache)} calls; {model} via {provider} so far ${spent:.5f}; "
          f"to run {len(todo)}; cap ${cap:.4f}")
    key = api_key(provider)
    url = "https://openrouter.ai/api/v1/chat/completions" if provider == "openrouter" else API
    lock = threading.Lock()
    state = {"spent": 0.0, "done": 0, "stop": False, "why": ""}
    f = CACHE.open("a")

    def one(item):
        g, a = item
        body = request_body(g, a, desc, authored, model, provider)
        proj = projected_cost(body, model)
        with lock:
            if state["stop"]:
                return
            if state["spent"] + proj > cap:
                state["stop"] = True
                state["why"] = (f"cap: ${state['spent']:.5f} spent + ${proj:.5f} projected "
                                f"for the next call would cross ${cap:.4f}")
                return
        n = len(g["cands"])
        rec = {"set": g["set"], "gid": g["gid"], "arm": a, "model": model, "provider": provider, "n": n,
               "req": body_hash(body), "ways": [c["way"] for c in g["cands"]],
               "strict": [c["strict"] for c in g["cands"]], "family": [c["family"] for c in g["cands"]],
               "share": [c["share"] for c in g["cands"]], "margin": [c["margin"] for c in g["cands"]],
               "band": [c["band"] for c in g["cands"]], "cosine": [c["cosine"] for c in g["cands"]]}
        if g["set"] == "golden":
            rec["expected"] = g["expected"]
        else:
            rec["items"] = [c["item"] for c in g["cands"]]
        err, reply, t0 = None, None, time.perf_counter()
        # A single attempt: the call budget is a hard bound, so no retries.
        try:
            reply = post(url, body, key, provider=provider)
        except urllib.error.HTTPError as e:
            detail = ""
            try:
                detail = e.read().decode()[:300].replace(key, "<key>")
            except Exception:
                pass
            err = f"http_{e.code}: {detail}"
        except Exception as e:  # transport
            err = type(e).__name__
        rec["ms"] = round((time.perf_counter() - t0) * 1000)
        if provider == "openrouter":
            cost, pin, pout = post_openrouter_cost(reply)
            rec["usage"] = {"input_tokens": pin, "output_tokens": pout}
            rec["usd"] = cost if isinstance(cost, (int, float)) else 0.0
            rec["cost_reported"] = isinstance(cost, (int, float))
            if reply is not None and isinstance(reply.get("error"), dict):
                err = f"provider: {json.dumps(reply['error'])[:300]}"
        else:
            usage = (reply or {}).get("usage", {})
            rec["usage"] = {k: usage.get(k, 0) for k in ("input_tokens", "output_tokens")}
            rec["usd"] = usage.get("input_tokens", 0) * PRICE_IN + usage.get("output_tokens", 0) * PRICE_OUT
        if reply is not None and err is None:
            try:
                rec["p_yes"] = [round(p, 4) for p in parse(reply, n, provider)]
            except Exception as e:
                err = f"answer: {e}"
        rec["error"] = err
        with lock:
            f.write(json.dumps(rec) + "\n")
            f.flush()
            state["spent"] += rec["usd"]
            state["done"] += 1
            if state["done"] % 10 == 0:
                print(f"  {state['done']}/{len(todo)} calls, running spend ${state['spent']:.5f}", flush=True)
            if provider == "openrouter" and not rec.get("cost_reported"):
                state["stop"] = True
                state["why"] = f"no provider-reported cost on {g['gid']}: spend is no longer known"

    # Sequential under a cap: no in-flight call can overshoot it.
    workers = 1 if sequential else 4
    with ThreadPoolExecutor(workers) as ex:
        list(ex.map(one, todo))
    f.close()
    print(f"done {state['done']} calls; spend ${state['spent']:.5f}"
          + (f"; STOPPED, {state['why']}" if state["stop"] else ""))


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
    recs = [r for r in recs if r.get("model", MODEL) == MODEL and r.get("provider", "anthropic") == "anthropic"]
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


def compare(model, provider, sample_n, arm="A"):
    """Paired comparison on the sampled groups: the cached claude-haiku-4-5 rows
    against `model` via `provider`."""
    groups, desc = load_groups()
    authored = authored_nodes()
    sampled = sample_groups(groups, sample_n)
    gids = [g["gid"] for g in sampled]
    cache = load_cache()
    # The 4.5 rows were cached under an older gid numbering; the Anthropic request
    # hash identifies the same group (same prompt, candidates and order) in both.
    by_req = {k[0]: r for k, r in cache.items() if k[1:] == (arm, MODEL, "anthropic")}
    old_of = {g["gid"]: by_req.get(body_hash(request_body(g, arm, desc, authored))) for g in sampled}
    by_new = {g["gid"]: cache.get((body_hash(request_body(g, arm, desc, authored, model, provider)),
                                   arm, model, provider)) for g in sampled}
    allrecs = [json.loads(l) for l in CACHE.read_text().splitlines()]
    new_reqs = {body_hash(request_body(g, arm, desc, authored, model, provider)): g["gid"] for g in sampled}
    new_all = [r for r in allrecs if r.get("model") == model and r.get("provider", "anthropic") == provider
               and r["arm"] == arm and r["req"] in new_reqs]
    fails = [r for r in new_all if r.get("error")]
    print(f"{model} via {provider}: {len(new_all)} calls, {len(fails)} failed, "
          f"spend ${sum(r['usd'] for r in new_all):.5f}")
    for r in fails:
        print("  failure:", r["gid"], r["error"])
    old, new = {}, {}
    for g in gids:
        o = old_of[g]
        n = by_new.get(g)
        if o and n:
            assert o["ways"] == n["ways"] and o["strict"] == n["strict"], g
            old[g], new[g] = o, n
    print(f"paired groups: {len(old)} of {len(gids)}")
    lat = sorted(r["ms"] for r in new_all if not r.get("error"))
    if lat:
        print(f"latency {model}: median {lat[len(lat) // 2]} ms, max {lat[-1]} ms; "
              f"4.5 cached median {sorted(r['ms'] for r in old.values())[len(old) // 2]} ms")

    def pct(v, q):
        v = sorted(v)
        return v[min(len(v) - 1, int(q * len(v)))]

    for label in ("strict", "family"):
        print(f"\n## labels {label}")
        res = {}
        for name, d in (("claude-haiku-4-5", old), (model, new)):
            u = units([d[g] for g in sorted(d)], label)
            pos = [p for _, p, y in u if y]
            neg = [p for _, p, y in u if not y]
            res[name] = (u, pos, neg)
            st = stats(u)
            print(f"{name}: {len(pos)} relevant / {len(neg)} irrelevant; AUC {st['auc']:.3f}; "
                  f"@0.3 pass irrelevant {sum(p >= .3 for p in neg)}/{len(neg)}, "
                  f"reject irrelevant {sum(p < .3 for p in neg)}/{len(neg)}, "
                  f"pass relevant {sum(p >= .3 for p in pos)}/{len(pos)}, "
                  f"lose relevant {sum(p < .3 for p in pos)}/{len(pos)}")
            for nm, v in (("relevant", pos), ("irrelevant", neg)):
                print(f"   P(yes) {nm}: min {min(v):.2f} p25 {pct(v, .25):.2f} median {pct(v, .5):.2f} "
                      f"p75 {pct(v, .75):.2f} max {max(v):.2f}; mean {sum(v) / len(v):.3f}")
        (_, pos4, neg4) = res["claude-haiku-4-5"]
        (_, pos5, neg5) = res[model]
        fp4 = sum(p >= .3 for p in neg4) / len(neg4)
        rc4 = sum(p >= .3 for p in pos4) / len(pos4)
        grid = sorted({p for p in pos5 + neg5} | {0.0, 1.0})
        # threshold t: pass iff p >= t. Matching pass-rate on irrelevant (<= 4.5's) and recall (>= 4.5's).
        both = [t for t in grid if sum(p >= t for p in neg5) / len(neg5) <= fp4 + 1e-9
                and sum(p >= t for p in pos5) / len(pos5) >= rc4 - 1e-9]
        by_fp = [t for t in grid if sum(p >= t for p in neg5) / len(neg5) <= fp4 + 1e-9]
        by_rc = [t for t in grid if sum(p >= t for p in pos5) / len(pos5) >= rc4 - 1e-9]
        print(f"4.5 @0.3: irrelevant pass rate {fp4:.1%}, relevant recall {rc4:.1%}")
        print(f"{model} threshold matching irrelevant pass rate (lowest t with rate <= 4.5's): "
              f"{min(by_fp):.2f}" if by_fp else "  none")
        print(f"{model} threshold matching recall (highest t with recall >= 4.5's): "
              f"{max(by_rc):.2f}" if by_rc else "  none")
        print(f"{model} thresholds meeting both: "
              + (f"{min(both):.2f}..{max(both):.2f}" if both else "none"))
        for t in (0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9):
            print(f"   t={t:.1f}: irrelevant pass {sum(p >= t for p in neg5) / len(neg5):.1%}, "
                  f"recall {sum(p >= t for p in pos5) / len(pos5):.1%}")


def compare_more(model, provider, sample_n, arm="A", n_boot=4000, seed=11):
    """Paired bootstrap over groups of AUC(model) - AUC(4.5), and latency percentiles."""
    groups, desc = load_groups()
    authored = authored_nodes()
    sampled = sample_groups(groups, sample_n)
    cache = load_cache()
    by_req = {k[0]: r for k, r in cache.items() if k[1:] == (arm, MODEL, "anthropic")}
    pairs = []
    for g in sampled:
        o = by_req.get(body_hash(request_body(g, arm, desc, authored)))
        n = cache.get((body_hash(request_body(g, arm, desc, authored, model, provider)), arm, model, provider))
        if o and n:
            pairs.append((o, n))
    print(f"\nbootstrap on {len(pairs)} paired groups, {n_boot} resamples (seed {seed})")
    rng = random.Random(seed)
    for label in ("strict", "family"):
        def d(idx):
            u4 = [(i, p, y) for i, k in enumerate(idx) for p, y in zip(pairs[k][0]["p_yes"], pairs[k][0][label])]
            u5 = [(i, p, y) for i, k in enumerate(idx) for p, y in zip(pairs[k][1]["p_yes"], pairs[k][1][label])]
            a4 = stats(u4)["auc"]
            a5 = stats(u5)["auc"]
            return a4, a5
        a4, a5 = d(list(range(len(pairs))))
        diffs = []
        for _ in range(n_boot):
            x, y = d([rng.randrange(len(pairs)) for _ in pairs])
            if not (math.isnan(x) or math.isnan(y)):
                diffs.append(y - x)
        diffs.sort()
        lo, hi = diffs[int(0.025 * len(diffs))], diffs[int(0.975 * len(diffs)) - 1]
        print(f"{label}: AUC 4.5 {a4:.4f}, 5.5 {a5:.4f}, diff {a5 - a4:+.4f}, 95% interval [{lo:+.4f}, {hi:+.4f}]"
              f" -> {'includes 0' if lo <= 0 <= hi else 'excludes 0'}")
    lat = sorted(r["ms"] for r in (json.loads(l) for l in CACHE.read_text().splitlines())
                 if r.get("model") == model and r.get("provider", "anthropic") == provider and not r.get("error"))
    q = lambda f: lat[min(len(lat) - 1, math.ceil(f * len(lat)) - 1)]
    print(f"latency over {len(lat)} calls: p50 {q(.5)} p90 {q(.9)} p95 {q(.95)} p99 {q(.99)} max {lat[-1]} ms; "
          f"over 2000 ms: {sum(x > 2000 for x in lat)} ({sum(x > 2000 for x in lat) / len(lat):.1%})")
    p99 = q(.99)
    print(f"timeout rule: p99 x1.2 = {p99 * 1.2:.0f} -> {math.ceil(p99 * 1.2 / 500) * 500} ms")


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "analyze"
    if cmd == "prepare":
        prepare()
    elif cmd == "estimate":
        estimate()
    elif cmd == "run":
        args = sys.argv[2:]
        opt = lambda k, d: args[args.index(k) + 1] if k in args else d
        run_calls(float(opt("--cap", 0)) if "--cap" in args else None, int(opt("--limit", 0)), opt("--arm", None),
                  opt("--model", MODEL), opt("--provider", "anthropic"), int(opt("--sample", 0)),
                  int(opt("--max-calls", 0)))
    elif cmd == "compare":
        args = sys.argv[2:]
        opt = lambda k, d: args[args.index(k) + 1] if k in args else d
        compare(opt("--model", MODEL), opt("--provider", "anthropic"), int(opt("--sample", 60)))
        compare_more(opt("--model", MODEL), opt("--provider", "anthropic"), int(opt("--sample", 60)))
    elif cmd == "analyze":
        analyze()
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
