import json, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
OUT = '/tmp/kgtopo-a3601'


def kg(*a):
    r = subprocess.run(['kg', *a, '--json'], capture_output=True, text=True, cwd='/tmp')
    return json.loads(r.stdout)


docs = json.load(open(OUT + '/docs.json'))
cids = sorted({c[0] for d in docs for c in d['concepts']})


def show(cid):
    try:
        j = kg('search', 'show', cid)
    except Exception as e:
        return cid, {'err': str(e)}
    return cid, {
        'label': j.get('label'), 'description': j.get('description'), 'search_terms': j.get('search_terms'),
        'src': sorted({(i.get('source_id') or '') for i in j.get('instances', [])}),
        'rels': [{'from': r.get('from_id'), 'to': r.get('to_id'), 'type': r.get('rel_type'), 'doc': r.get('document_id')}
                 for r in j.get('relationships', [])]}


done = 0
res = {}
with ThreadPoolExecutor(16) as ex:
    for cid, v in ex.map(show, cids):
        res[cid] = v
        done += 1
        if done % 200 == 0:
            print(done, file=sys.stderr, flush=True)
json.dump(res, open(OUT + '/concepts.json', 'w'))
print('done', len(res), file=sys.stderr)
