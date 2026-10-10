import json, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
OUT = '/tmp/kgtopo-a3601'


def kg(*a):
    r = subprocess.run(['kg', *a, '--json'], capture_output=True, text=True, cwd='/tmp')
    return json.loads(r.stdout)


onts = kg('catalog', 'ls')['nodes']
docs = []
for o in onts:
    for d in kg('catalog', 'ls', o['id'])['nodes']:
        docs.append({'ontology': o['name'], 'id': d['id'], 'name': d['name'], 'child_count': d['child_count']})


def concepts(d):
    d['concepts'] = [(c['id'], c['name']) for c in kg('catalog', 'ls', d['id'])['nodes']]
    return d


with ThreadPoolExecutor(8) as ex:
    docs = list(ex.map(concepts, docs))
json.dump(docs, open(OUT + '/docs.json', 'w'))
cids = sorted({c[0] for d in docs for c in d['concepts']})
print(len(docs), len(cids), file=sys.stderr)


def show(cid):
    try:
        j = kg('search', 'show', cid)
    except Exception as e:
        return cid, {'err': str(e)}
    return cid, {
        'label': j.get('label'), 'description': j.get('description'), 'search_terms': j.get('search_terms'),
        'inst_files': sorted({i.get('filename') or '' for i in j.get('instances', [])}),
        'rels': [{'from': r.get('from_id'), 'to': r.get('to_id'), 'type': r.get('rel_type'), 'doc': r.get('document_id')}
                 for r in j.get('relationships', [])]}


with ThreadPoolExecutor(8) as ex:
    det = dict(ex.map(show, cids))
json.dump(det, open(OUT + '/concepts.json', 'w'))
