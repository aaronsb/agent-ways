"""Batched judge probe (ADR-195 addendum): one Haiku call per prompt carrying every candidate.

Compares AUC with the single-call scores already in results.jsonl (wv=B, cv=T1).
Reads the key from WAYS_JUDGE_KEY_FILE in code only; never prints it."""
import json, os, time, collections, concurrent.futures as cf
import anthropic

P = os.path.expanduser('~/.local/state/agent-ways/probes/yesno-gate')
OUT = P + '/eval/batched_scores.jsonl'
MODEL = 'claude-haiku-4-5-20251001'
SYSTEM = ("You are a relevance gate for a coding assistant's guidance system. You are given "
          "several pieces of guidance and the most recent turns of a conversation. Judge each "
          "piece of guidance on its own. Call the record_judgements tool exactly once, with one "
          "entry per piece of guidance. `relevant` is your yes/no answer; `confidence` is your "
          "probability, from 0 to 1, that this answer is correct.")
INSTR = "Decide whether each piece of guidance is relevant to what the conversation is doing in its most recent turns."
TOOL = {"name": "record_judgements",
        "description": "Record, for each piece of guidance, whether it is relevant to the recent conversation.",
        "strict": True,
        "input_schema": {"type": "object", "additionalProperties": False, "required": ["judgements"],
            "properties": {"judgements": {"type": "array", "items": {"type": "object", "additionalProperties": False,
                "required": ["id", "relevant", "confidence"],
                "properties": {"id": {"type": "string"}, "relevant": {"type": "boolean"}, "confidence": {"type": "number"}}}}}}}

units = [json.loads(l) for l in open(P + '/eval/units.jsonl')]
units = [u for u in units if u['wv'] == 'B' and u['cv'] == 'T1']
groups = collections.defaultdict(list)
for u in units:
    groups[(u['set'], u['query'])].append(u)
done = set()
if os.path.exists(OUT):
    done = {json.loads(l)['gkey'] for l in open(OUT)}
key = open(os.environ['WAYS_JUDGE_KEY_FILE']).read().strip()
client = anthropic.Anthropic(api_key=key, max_retries=2)
del key

def run(gi, k, members):
    guid = "\n\n".join(f'<guidance id="g{i+1}">\n{m["doc"]}\n</guidance>' for i, m in enumerate(members))
    prompt = f"{INSTR}\n\n{guid}\n\n<conversation>\n{k[1]}\n</conversation>"
    t0 = time.perf_counter(); err = None; js = []
    try:
        r = client.messages.create(model=MODEL, max_tokens=64 + 48 * len(members), system=SYSTEM,
                                   tools=[TOOL], tool_choice={"type": "tool", "name": TOOL["name"]},
                                   messages=[{"role": "user", "content": prompt}], extra_body={"temperature": 0})
        b = next((b for b in r.content if b.type == 'tool_use'), None)
        js = b.input['judgements'] if b else []
        if not b: err = f'no tool_use {r.stop_reason}'
    except Exception as e:
        err = type(e).__name__
    ms = (time.perf_counter() - t0) * 1000
    by = {j['id']: j for j in js}
    rows = []
    for i, m in enumerate(members):
        j = by.get(f'g{i+1}')
        p = None if j is None else (min(1, max(0, j['confidence'])) if j['relevant'] else 1 - min(1, max(0, j['confidence'])))
        rows.append({'gkey': gi, 'set': m['set'], 'id': m['id'], 'way_id': m['way_id'], 'n': len(members), 'p_yes': p, 'ms': ms, 'error': err})
    return rows

todo = [(gi, k, v) for gi, (k, v) in enumerate(sorted(groups.items(), key=lambda kv: kv[0])) if gi not in done]
print('groups', len(groups), 'todo', len(todo), 'calls this run', len(todo))
with open(OUT, 'a') as f, cf.ThreadPoolExecutor(4) as ex:
    for rows in ex.map(lambda a: run(*a), todo):
        for r in rows: f.write(json.dumps(r) + '\n')
        f.flush()
print('done')
