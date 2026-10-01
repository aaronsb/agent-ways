#!/usr/bin/env python3
"""Call the judge exactly as ways-agent does (net.rs / judge.rs), outside the
daemon's 2 s deadline, and record latency, token usage and verdicts.

Variants:
  base     the shipped request: SYSTEM, tool schema with an n-sized id enum,
           candidates + conversation in the user message.
  catalog  a cacheable prefix: a fixed 40-id schema, SYSTEM plus the whole way
           catalog in the system prompt, cache_control on the system block.

The key is read from the key file inside this process and never printed.

usage: latency_probe.py sweep|stability [--variant base|catalog] [--reps N]
"""
import argparse, json, os, statistics, sys, time, urllib.request, urllib.error

KEY_FILE = os.path.expanduser("~/.config/agent-ways/keys/anthropic")
CORPUS = os.path.expanduser("~/.cache/agent-ways/user/ways-corpus.jsonl")
MODEL = "claude-haiku-4-5"

SYSTEM = ("You are a relevance gate for a coding assistant's guidance system. You are given "
          "several pieces of guidance and the most recent turns of a conversation. Judge each piece of guidance on its own. "
          "Call the record_judgements tool exactly once, with one entry per piece of guidance. `relevant` is your yes/no "
          "answer; `confidence` is your probability, from 0 to 1, that this answer is correct.")
INSTRUCTION = "Decide whether each piece of guidance is relevant to what the conversation is doing in its most recent turns."
TOOL_NAME = "record_judgements"
TOOL_DESCRIPTION = "Record, for each piece of guidance, whether it is relevant to the recent conversation."
FIXED_IDS = 40


def schema(n):
    return {"type": "object", "additionalProperties": False, "required": ["judgements"],
            "properties": {"judgements": {"type": "array", "items": {
                "type": "object", "additionalProperties": False, "required": ["id", "relevant", "confidence"],
                "properties": {"id": {"type": "string", "enum": [f"g{i}" for i in range(1, n + 1)]},
                               "relevant": {"type": "boolean"}, "confidence": {"type": "number"}}}}}}


def corpus():
    ways = {}
    with open(CORPUS) as f:
        for line in f:
            w = json.loads(line)
            ways[w["id"]] = w["description"]
    return ways


def way_text(wid, desc):
    return " › ".join(s for s in wid.split("/") if s) + "\n" + desc.strip()


def render_prompt(prompt, cands):
    g = "\n\n".join(f'<guidance id="g{i}">\n{t.replace("<", "‹")}\n</guidance>' for i, t in enumerate(cands, 1))
    return f"{INSTRUCTION}\n\n{g}\n\n<conversation>\nUser: {prompt.replace('<', '‹')}\n</conversation>"


def body(variant, prompt, cands, ways):
    n = len(cands)
    b = {"model": MODEL, "max_tokens": 64 + 48 * n, "temperature": 0,
         "tool_choice": {"type": "tool", "name": TOOL_NAME},
         "messages": [{"role": "user", "content": render_prompt(prompt, cands)}]}
    if variant == "base":
        b["system"] = SYSTEM
        b["tools"] = [{"name": TOOL_NAME, "description": TOOL_DESCRIPTION, "strict": True, "input_schema": schema(n)}]
    else:
        catalog = "\n".join(f"- {way_text(i, d).replace(chr(10), ': ')}" for i, d in sorted(ways.items()))
        b["system"] = [{"type": "text", "text": SYSTEM + "\n\nFor reference, the full catalog of guidance the "
                        "candidates are drawn from (judge only the candidates in the user message):\n" + catalog,
                        "cache_control": {"type": "ephemeral"}}]
        b["tools"] = [{"name": TOOL_NAME, "description": TOOL_DESCRIPTION, "strict": True,
                       "input_schema": schema(FIXED_IDS)}]
    return b


def call(b, key):
    req = urllib.request.Request("https://api.anthropic.com/v1/messages", data=json.dumps(b).encode(),
                                 headers={"x-api-key": key, "anthropic-version": "2023-06-01",
                                          "content-type": "application/json"})
    t0 = time.monotonic()
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            reply = json.load(r)
    except urllib.error.HTTPError as e:
        try:
            msg = json.load(e).get("error", {}).get("message", "?")
        except ValueError:
            msg = "non-JSON error body"
        raise SystemExit(f"provider {e.code}: {msg}")
    ms = (time.monotonic() - t0) * 1000
    tu = next(c for c in reply["content"] if c["type"] == "tool_use")["input"]["judgements"]
    p = {}
    for j in tu:
        c = min(max(j["confidence"], 0), 1)
        p.setdefault(j["id"], c if j["relevant"] else 1 - c)
    return ms, reply["usage"], p


def pct(xs, q):
    xs = sorted(xs)
    k = (len(xs) - 1) * q
    lo = int(k)
    return xs[lo] + (xs[min(lo + 1, len(xs) - 1)] - xs[lo]) * (k - lo)


SWEEP_PROMPT = "commit these changes with a good conventional commit message and open a PR"
SWEEP_POOL = [  # matcher-plausible for the prompt first, then filler
    "softwaredev/delivery/commits", "softwaredev/delivery/github", "softwaredev/delivery",
    "softwaredev/delivery/merge", "softwaredev/delivery/release", "softwaredev/delivery/patches",
    "softwaredev/code/quality", "softwaredev/code/testing", "documentation/adr", "softwaredev/delivery/implement",
    "softwaredev/code/security", "softwaredev/environment/recovery", "softwaredev/code/performance",
    "data/migrations", "workstation/shell/gitconfig", "meta/skills", "meta/workflows", "itops/runbooks",
    "softwaredev/code/supplychain", "documentation/mermaid",
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["sweep", "stability"])
    ap.add_argument("--variant", default="base", choices=["base", "catalog"])
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--sizes", default="1,3,6,10,15,20")
    a = ap.parse_args()
    with open(KEY_FILE) as f:
        key = f.read().strip()
    ways = corpus()
    pool = [w for w in SWEEP_POOL if w in ways]
    missing = [w for w in SWEEP_POOL if w not in ways]
    if missing:
        print("not in corpus:", missing, file=sys.stderr)
    rows = []

    if a.mode == "sweep":
        print(f"variant {a.variant}  model {MODEL}")
        print(f"{'n':>3} {'p50 ms':>7} {'min':>6} {'max':>6} {'in':>6} {'cache_w':>7} {'cache_r':>7} {'out':>5} {'ms/out':>6}")
        for n in [int(s) for s in a.sizes.split(",")]:
            cands = [way_text(w, ways[w]) for w in pool[:n]]
            res = [call(body(a.variant, SWEEP_PROMPT, cands, ways), key) for _ in range(a.reps)]
            ms = [r[0] for r in res]
            u = res[-1][1]
            outs = [r[1]["output_tokens"] for r in res]
            print(f"{n:>3} {pct(ms,.5):7.0f} {min(ms):6.0f} {max(ms):6.0f} {u['input_tokens']:>6} "
                  f"{u.get('cache_creation_input_tokens',0):>7} {u.get('cache_read_input_tokens',0):>7} "
                  f"{statistics.mean(outs):5.0f} {statistics.mean(ms)/statistics.mean(outs):6.1f}")
            for r in res:
                rows.append({"variant": a.variant, "n": n, "ms": r[0], "usage": r[1]})
    else:
        prompt = "write a database migration that adds a nullable column to the orders table"
        sets = {
            "data+migrations+numbering": ["data", "data/migrations", "data/migrations/numbering"],
            "data+numbering": ["data", "data/migrations/numbering"],
            "numbering alone": ["data/migrations/numbering"],
        }
        print(f"variant {a.variant}: P(yes) over {a.reps} reps, temperature 0")
        for name, ids in sets.items():
            ids = [i for i in ids if i in ways]
            res = [call(body(a.variant, prompt, [way_text(i, ways[i]) for i in ids], ways), key) for _ in range(a.reps)]
            print(f"  {name}  ({pct([r[0] for r in res], .5):.0f} ms p50)")
            for k, wid in enumerate(ids, 1):
                ps = [r[2].get(f"g{k}", float('nan')) for r in res]
                print(f"    {wid:28} " + " ".join(f"{p:.2f}" for p in ps))
            rows.append({"variant": a.variant, "set": name, "p": [r[2] for r in res], "ms": [r[0] for r in res]})

    out = os.path.join(os.path.expanduser('~/.local/state/agent-ways/probes/yesno-gate/eval'), f"latency_probe-{a.mode}-{a.variant}.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w") as f:
        json.dump(rows, f)


if __name__ == "__main__":
    main()
