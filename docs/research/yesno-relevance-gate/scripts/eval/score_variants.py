"""Judge request variants (ADR-195 evidence): one batched Haiku call per prompt group,
scored for AUC against the probe's labels, with latency and token usage.

Variants:
  base        the shipped request (ways-agent 0.1.0): relevant + confidence, id enum g1..gN
  compact     id + p_relevant only, the fewest output tokens
  structured  described fields in evidence-to-verdict order: subject, match, relevant, confidence
  catalog     base schema with a fixed 40-id enum, the way catalog in a cached system prompt

Reads the key from WAYS_JUDGE_KEY_FILE in code only; never prints it.
usage: WAYS_JUDGE_KEY_FILE=... python3 score_variants.py VARIANT [VARIANT...]
"""
import collections, concurrent.futures as cf, json, os, sys, time
import anthropic

P = os.path.expanduser('~/.local/state/agent-ways/probes/yesno-gate')
OUT = P + '/eval/variant_scores.jsonl'
CORPUS = os.path.expanduser('~/.cache/agent-ways/user/ways-corpus.jsonl')
MODEL = 'claude-haiku-4-5'

SYSTEM = ("You are a relevance gate for a coding assistant's guidance system. You are given "
          "several pieces of guidance and the most recent turns of a conversation. Judge each "
          "piece of guidance on its own. Call the record_judgements tool exactly once, with one "
          "entry per piece of guidance. `relevant` is your yes/no answer; `confidence` is your "
          "probability, from 0 to 1, that this answer is correct.")
SYSTEM_COMPACT = ("You are a relevance gate for a coding assistant's guidance system. You are given "
                  "several pieces of guidance and the most recent turns of a conversation. Judge each "
                  "piece of guidance on its own. Call the record_judgements tool exactly once, with one "
                  "entry per piece of guidance. `p_relevant` is your probability, from 0 to 1, that the "
                  "guidance is relevant.")
SYSTEM_STRUCTURED = ("You are a relevance gate for a coding assistant's guidance system. You are given "
                     "several pieces of guidance and the most recent turns of a conversation. Judge each "
                     "piece of guidance on its own. Call the record_judgements tool exactly once, with one "
                     "entry per piece of guidance, filling its fields in order.")
INSTR = "Decide whether each piece of guidance is relevant to what the conversation is doing in its most recent turns."
TOOL_NAME = 'record_judgements'
TOOL_DESC = "Record, for each piece of guidance, whether it is relevant to the recent conversation."


def item_schema(variant, ids):
    idp = {"type": "string", "enum": ids}
    if variant == 'compact':
        props = {"id": idp, "p_relevant": {"type": "number"}}
    elif variant == 'structured':
        props = {
            "id": idp,
            "subject": {"type": "string", "description":
                        "What the conversation's most recent turn is working on, in at most eight words."},
            "match": {"type": "string", "enum": ["direct", "adjacent", "none"], "description":
                      "direct: the guidance covers the work in progress now. adjacent: same area, but not "
                      "the work in progress. none: unrelated."},
            "relevant": {"type": "boolean", "description":
                         "Would this guidance help with what the conversation is doing now?"},
            "confidence": {"type": "number", "description":
                           "Probability, from 0 to 1, that `relevant` is correct."},
        }
    else:
        props = {"id": idp, "relevant": {"type": "boolean"}, "confidence": {"type": "number"}}
    return {"type": "object", "additionalProperties": False, "required": list(props), "properties": props}


def tool(variant, n):
    ids = [f"g{i}" for i in range(1, (40 if variant == 'catalog' else n) + 1)]
    return {"name": TOOL_NAME, "description": TOOL_DESC, "strict": True,
            "input_schema": {"type": "object", "additionalProperties": False, "required": ["judgements"],
                             "properties": {"judgements": {"type": "array", "items": item_schema(variant, ids)}}}}


def catalog_system():
    lines = []
    for l in open(CORPUS):
        w = json.loads(l)
        route = " › ".join(s for s in w['id'].split('/') if s)
        lines.append(f"- {route}: {w['description'].strip()}")
    return [{"type": "text", "cache_control": {"type": "ephemeral"},
             "text": SYSTEM + "\n\nFor reference, the full catalog of guidance the candidates are drawn from "
                     "(judge only the candidates in the user message):\n" + "\n".join(sorted(lines))}]


def p_of(variant, j):
    if variant == 'compact':
        return min(1, max(0, j['p_relevant']))
    c = min(1, max(0, j['confidence']))
    return c if j['relevant'] else 1 - c


def groups():
    units = [json.loads(l) for l in open(P + '/eval/units.jsonl')]
    units = [u for u in units if u['wv'] == 'B' and ((u['set'] in ('s1', 's2') and u['cv'] == 'T1')
                                                     or (u['set'] == 's3' and u['cv'] == 'EX'))]
    g = collections.defaultdict(list)
    for u in units:
        g[(u['set'], u['query'])].append(u)
    return sorted(g.items(), key=lambda kv: kv[0])


def main():
    variants = sys.argv[1:]
    key = open(os.environ['WAYS_JUDGE_KEY_FILE']).read().strip()
    client = anthropic.Anthropic(api_key=key, max_retries=3)
    del key
    sysmap = {'base': SYSTEM, 'compact': SYSTEM_COMPACT, 'structured': SYSTEM_STRUCTURED, 'catalog': catalog_system()}
    gs = groups()
    done = set()
    if os.path.exists(OUT):
        done = {(r['variant'], r['gkey']) for r in map(json.loads, open(OUT))}

    def run(variant, gi, k, members):
        guid = "\n\n".join(f'<guidance id="g{i+1}">\n{m["doc"].replace("<", "‹")}\n</guidance>'
                           for i, m in enumerate(members))
        prompt = f"{INSTR}\n\n{guid}\n\n<conversation>\n{k[1]}\n</conversation>"
        per = {'compact': 24, 'structured': 72}.get(variant, 48)
        t0 = time.perf_counter(); err = None; js = []; usage = {}
        try:
            r = client.messages.create(model=MODEL, max_tokens=64 + per * len(members), extra_body={"temperature": 0},
                                       system=sysmap[variant], tools=[tool(variant, len(members))],
                                       tool_choice={"type": "tool", "name": TOOL_NAME},
                                       messages=[{"role": "user", "content": prompt}])
            usage = r.usage.model_dump()
            b = next((b for b in r.content if b.type == 'tool_use'), None)
            js = b.input['judgements'] if b else []
            if not b:
                err = f'no tool_use {r.stop_reason}'
        except Exception as e:
            err = type(e).__name__
        ms = (time.perf_counter() - t0) * 1000
        by = {}
        for j in js:
            by.setdefault(j.get('id'), j)
        rows = []
        for i, m in enumerate(members):
            j = by.get(f'g{i+1}')
            try:
                p = None if j is None else p_of(variant, j)
            except (KeyError, TypeError):
                p = None
            rows.append({'variant': variant, 'gkey': gi, 'set': m['set'], 'id': m['id'], 'way_id': m['way_id'],
                         'n': len(members), 'p_yes': p, 'ms': ms, 'error': err,
                         'match': (j or {}).get('match'), 'usage': usage})
        return rows

    todo = [(v, gi, k, m) for v in variants for gi, (k, m) in enumerate(gs) if (v, gi) not in done]
    print('groups', len(gs), 'calls this run', len(todo), flush=True)
    with open(OUT, 'a') as f, cf.ThreadPoolExecutor(4) as ex:
        for rows in ex.map(lambda a: run(*a), todo):
            for r in rows:
                f.write(json.dumps(r) + '\n')
            f.flush()
    print('done')


if __name__ == '__main__':
    main()
