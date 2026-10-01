import json,re,collections,datetime,sys
sys.path.insert(0,'.')
from labels import L,RR,RI,AM
cat=json.load(open('catalog.json'))
pool={f"C{x['cid']}":x for x in json.load(open('pool.json'))}
rest={f"X{i}":x for i,x in enumerate(json.load(open('rest.json')))}
M={**pool,**rest}
pts=lambda s: datetime.datetime.fromisoformat(s.replace('Z','+00:00')).timestamp()
need={M[r]['session'] for r,*_ in L}
ev=collections.defaultdict(list)
for l in open('/home/aaron/.local/state/agent-ways/events.jsonl'):
    try:e=json.loads(l)
    except: continue
    if e.get('session') in need and e.get('way'): ev[e['session']].append(e)
def window_events(m):
    p=pts(m['prompt_ts']); return [e for e in ev[m['session']] if p-2<=pts(e['ts'])<=p+60]
def clean(t,role):
    t=re.sub(r'</?pasted_content[^>]*>','',t)
    t=re.sub(r'<(system-reminder|agent-message)[^>]*>.*?</\1>','',t,flags=re.S)
    t=re.sub(r'\n{3,}','\n\n',t).strip()
    if len(t)>1200: t='…'+t[-1199:]
    return t
SYS=re.compile(r'(Another Claude session sent a message|\d+ background agents? (were|was) stopped|Background agent .* was stopped|<agent-message)',re.S)
out=[];problems=[]
for n,(ref,wo,catg,lab,rat) in enumerate(L):
    m=M[ref]; way=wo or m['way_id']
    if way not in cat: problems.append((ref,way,'not in catalog')); continue
    we=window_events(m)
    PT=lambda t: t and (t.startswith('semantic:embedding') or t.startswith('semantic:late') or t=='keyword')
    pf=[e for e in we if e['way']==way and e['event'] in('way_fired','way_redisclosed') and PT(e.get('trigger'))]
    later=sorted({e.get('trigger') for e in we if e['way']==way and e['event'] in('way_fired','way_redisclosed') and not PT(e.get('trigger'))})
    real = wo is None or catg in (RR,RI)
    if wo and not real and pf: problems.append((ref,way,catg,'prompt-lane FIRED'))
    if wo and real and not pf: problems.append((ref,way,catg,'claimed real but no prompt fire'))
    src='real_fire' if real or (catg==AM and wo is None) else 'synthetic_pair'
    if wo and real: m=dict(m, trigger=pf[0].get('trigger'), ts=pf[0]['ts'], matched_span=pf[0].get('matched_span'))
    item={'id':f"yn-{n:03d}",'way_id':way,'summary':cat[way]['description'],'vocabulary':cat[way]['vocabulary'],
          'turns':[{'role':t['role'],'text':clean(t['text'],t['role'])} for t in m['turns'] if not (t['role']=='user' and SYS.match(t['text'].strip()))],
          'label':lab,'category':catg,'rationale':rat,'source':src,'project':m['project'],'session':m['session'],'ts':m['prompt_ts'],
          'fire':({'trigger':m['trigger'],'fire_ts':m['ts'],'matched_span':m.get('matched_span')} if src=='real_fire' else None),
          'later_nonprompt_fires':later,
          'gate_events_in_window':sorted({f"{e['event']}" for e in we if e['way']==way and e['event'] not in('way_fired','way_redisclosed')}),
          'moment_ref':ref}
    out.append(item)
print('problems',*problems,sep='\n')
json.dump(out,open('eval_draft.json','w'))
c=collections.Counter((x['category'],x['label']) for x in out); print(c, len(out))
w=collections.Counter(x['way_id'] for x in out); print([k for k in w.items() if k[1]>6])
s=collections.Counter(x['session'] for x in out); print([k for k in s.items() if k[1]>10], len(s), len(w), len({x['project'] for x in out}))
dup=collections.Counter((x['moment_ref'],x['way_id']) for x in out); print([k for k,v in dup.items() if v>1])
