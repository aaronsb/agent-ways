import json,random,sys,os
W=os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0,W)
cat=json.load(open(W+'/catalog.json'))
fj=[x for x in json.load(open(W+'/fires_joined.json')) if x['verified']]
print('verified',len(fj))
ev=[json.loads(l) for l in open(W+'/../data/eval.jsonl')]
used={(x['session'],x['fire']['fire_ts'],x['way_id']) for x in ev if x['fire']}
cand=[x for x in fj if (x['session'],x['ts'],x['way_id']) not in used]
print('candidates',len(cand))
rng=random.Random(20260930); pick=rng.sample(cand,40)
json.dump(pick,open(W+'/random40_pick.json','w'))
for i,x in enumerate(pick):
    u=x['turns'][-1]['text'].replace('\n',' ')
    a=[t for t in x['turns'][:-1] if t['role']=='assistant']
    print(f"=== R{i} | {x['way_id']} | {x['trigger']} {x.get('matched_span') or ''} | {x['project'].split('/')[-1]} | incat={x['way_id'] in cat}")
    print('DESC:',cat.get(x['way_id'],{}).get('description','')[:200])
    if a: print('PREV-A: ...'+a[-1]['text'][-300:].replace('\n',' '))
    print('USER:',u[-500:]); print()
