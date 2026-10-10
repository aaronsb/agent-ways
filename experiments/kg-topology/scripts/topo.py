"""Readback + topology from the kg dump. Writes compact JSON into experiments/kg-topology."""
import json, itertools, collections, sys, os
OUT = sys.argv[1]
T = '/tmp/kgtopo-a3601'
docs = json.load(open(T + '/docs.json'))
cons = json.load(open(T + '/concepts.json'))
auth = json.load(open(T + '/authored.json'))

way_of = lambda n: n[:-3].replace('__', '/')
pref = {}
for d in docs:
    p = d['id'].split(':')[1][:5]
    assert p not in pref, p
    pref[p] = way_of(d['name'])

c2w = collections.defaultdict(set)
for d in docs:
    for cid, _ in d['concepts']:
        c2w[cid].add(way_of(d['name']))
unmapped = 0
for cid, v in cons.items():
    for s in v.get('src', []):
        p = s.split(':')[-1].split('_')[0][:5]
        if p in pref:
            c2w[cid].add(pref[p])
        else:
            unmapped += 1
w2c = collections.defaultdict(set)
for cid, ws in c2w.items():
    for w in ws:
        w2c[w].add(cid)

# compact readback files
cpw = {w: sorted([[cons[c]['label'], cons[c].get('description') or ''] for c in cs]) for w, cs in sorted(w2c.items())}
json.dump(cpw, open(OUT + '/concepts-per-way.json', 'w'), separators=(',', ':'), ensure_ascii=False)
idx = {cons[c]['label'] + ' [' + c.split(':')[1] + ']': sorted(ws) for c, ws in sorted(c2w.items(), key=lambda x: cons[x[0]]['label'].lower())}
json.dump(idx, open(OUT + '/concept-to-ways.json', 'w'), separators=(',', ':'), ensure_ascii=False)

# (a) shared-concept edges
shared = collections.Counter()
for cid, ws in c2w.items():
    for a, b in itertools.combinations(sorted(ws), 2):
        shared[(a, b)] += 1
# relationship edges crossing ways
rel = collections.Counter()
for cid, v in cons.items():
    for r in v.get('rels', []):
        f, t = r.get('from') or cid, r.get('to') or cid
        for a in c2w.get(f, ()):
            for b in c2w.get(t, ()):
                if a != b:
                    rel[tuple(sorted((a, b)))] += 1
for k in rel:
    rel[k] = rel[k]  # counted from both endpoints' views possibly; keep raw


def jac(a, b):
    A, B = w2c[a], w2c[b]
    return len(A & B) / len(A | B) if A | B else 0


edges = {}
for k in set(shared) | set(rel):
    edges[k] = {'shared': shared.get(k, 0), 'rel': rel.get(k, 0), 'jaccard': round(jac(*k), 3),
                'score': shared.get(k, 0) + 0.5 * rel.get(k, 0)}

# (b) authored edges
seealso = set()
for w, v in auth.items():
    for s in v['see_also']:
        seealso.add(tuple(sorted((w, s))))
parentchild = set()
for w in auth:
    p = auth[w]['parent']
    while p:
        if p in auth:
            parentchild.add(tuple(sorted((w, p))))
            break
        p = '/'.join(p.split('/')[:-1]) or None
siblings = set()
for a, b in itertools.combinations(sorted(auth), 2):
    if auth[a]['parent'] and auth[a]['parent'] == auth[b]['parent']:
        siblings.add((a, b))

ranked = sorted(edges.items(), key=lambda x: (-x[1]['score'], -x[1]['jaccard']))
uncovered = [(k, e, 'parent-child' if k in parentchild else 'sibling' if k in siblings else '') for k, e in ranked
             if k not in seealso]
unsupported = sorted(k for k in seealso if k[0] in w2c and k[1] in w2c and k not in edges)
isolated = sorted(w for w in auth if not any(w in k for k in edges))
dups = [(k, e) for k, e in ranked if e['jaccard'] >= 0.08]

topo = {
    'stats': {'ways': len(w2c), 'concepts': len(c2w), 'concepts_in_2plus_ways': sum(len(ws) > 1 for ws in c2w.values()),
              'shared_edges': len(shared), 'rel_edges': len(rel), 'edges': len(edges), 'see_also_edges': len(seealso),
              'see_also_supported': len(seealso & set(edges)), 'unmapped_sources': unmapped},
    'edges': [[a, b, e['shared'], e['rel'], e['jaccard'], 'see_also' if (a, b) in seealso else c] for (a, b), e, c in
              [(k, e, 'parent-child' if k in parentchild else 'sibling' if k in siblings else '') for k, e in ranked]],
    'uncovered_top': [[a, b, e['shared'], e['rel'], e['jaccard'], c] for (a, b), e, c in uncovered[:40]],
    'see_also_unsupported': [list(k) for k in unsupported],
    'near_duplicates': [[a, b, e['shared'], e['jaccard']] for (a, b), e in dups],
    'isolated': isolated,
    'shared_concepts': sorted([[cons[c]['label'], sorted(ws)] for c, ws in c2w.items() if len(ws) > 1], key=lambda x: -len(x[1])),
}
json.dump(topo, open(OUT + '/topology.json', 'w'), separators=(',', ':'), ensure_ascii=False)
print(json.dumps(topo['stats']))
print('UNCOVERED')
for r in topo['uncovered_top'][:20]: print(r)
print('UNSUPPORTED', len(unsupported))
print('DUPS', topo['near_duplicates'][:15])
print('ISOLATED', len(isolated))
print('SHARED', topo['shared_concepts'][:40])
