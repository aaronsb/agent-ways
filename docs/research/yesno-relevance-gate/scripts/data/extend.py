import json,glob,os,re,collections,sys
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
D=os.path.dirname(os.path.dirname(os.path.abspath(__file__)))+'/data/'
items=[json.loads(l) for l in open(D+'eval.v1.jsonl')]
idx={os.path.basename(f)[:-6]:f for f in glob.glob(os.path.expanduser('~/.claude/projects/*/*.jsonl'))}
STRIP=re.compile(r'<(system-reminder|command-[a-z-]+|local-command-[a-z-]+|bash-[a-z-]+|task-notification|teammate-message|user-prompt-submit-hook|agent-message)[^>]*>.*?</\1>',re.S)
SYS=re.compile(r'(Another Claude session sent a message|\d+ background agents? (were|was) stopped|Background agent .* was stopped|<agent-message)',re.S)
def clean(t):
    t=STRIP.sub('',t); t=re.sub(r'<persisted-output>.*?</persisted-output>','',t,flags=re.S)
    t=re.sub(r'</?pasted_content[^>]*>','',t); return re.sub(r'\n{3,}','\n\n',t).strip()
def utext(m):
    c=m.get('content')
    if isinstance(c,str): return c
    if isinstance(c,list):
        if any(x.get('type')=='tool_result' for x in c): return None
        return '\n'.join(x.get('text','') for x in c if x.get('type')=='text')
cache={}
def session(s):
    if s in cache: return cache[s]
    turns=[];cur=[];title=None;first=None
    def push_user(t,ts):
        nonlocal cur,first
        if cur: turns.append(dict(role='assistant',text='\n\n'.join(cur),ts=None,title=title)); cur=[]
        turns.append(dict(role='user',text=t,ts=ts,title=title))
    for l in open(idx[s]):
        try:e=json.loads(l)
        except: continue
        t=e.get('type')
        if t in('ai-title','custom-title'): title=(t,e.get('aiTitle') or e.get('customTitle') or e.get('title')); continue
        if e.get('isSidechain'): continue
        if t=='user' and not e.get('isMeta'):
            u=utext(e.get('message',{}))
            if u is None: continue
            u=clean(u)
            if not u or u.startswith('[Request interrupted') or u.startswith('Caveat:') or SYS.match(u): continue
            push_user(u,e.get('timestamp'))
        elif t=='assistant':
            for x in (e.get('message',{}).get('content') or []):
                if isinstance(x,dict) and x.get('type')=='text' and x.get('text','').strip(): cur.append(clean(x['text']))
        elif t=='attachment':
            a=e.get('attachment',{})
            if a.get('type')=='queued_command' and a.get('commandMode','prompt')=='prompt' and isinstance(a.get('prompt'),str):
                u=clean(a['prompt'])
                if u and not SYS.match(u): push_user(u,a.get('timestamp') or e.get('timestamp'))
    cache[s]=turns; return turns
BUDGET=4000;out=[];stats=collections.Counter();lens=[]
for x in items:
    T=session(x['session'])
    pos=[i for i,t in enumerate(T) if t['role']=='user' and t['ts']==x['ts']]
    assert pos, x['id']
    p=pos[-1]
    assert T[p]['text'][-200:]==x['turns'][-1]['text'][-200:] or x['turns'][-1]['text'].startswith('…'), x['id']
    ext=[];tot=0
    for t in reversed(T[:p+1]):
        txt=t['text']
        if t['role']=='user' and t is not T[p] and False: pass
        room=BUDGET-tot
        if room<=0: break
        if len(txt)>room: txt='…'+txt[-(room-1):]
        ext.insert(0,{'role':t['role'],'text':txt}); tot+=len(txt)
    title=T[p]['title']
    first=next((t['text'] for t in T if t['role']=='user'),None)
    if title and title[1]: gist,src=title[1],'title'; stats[title[0]]+=1
    elif first: gist,src=first[:300],'first_prompt'
    else: gist,src=None,'none'
    x2=dict(x); x2['turns_extended']=ext; x2['session_gist']=gist; x2['session_gist_source']=src
    stats[src]+=1; lens.append(tot); out.append(x2)
with open(D+'eval.jsonl','w') as f:
    for x in out: f.write(json.dumps(x,ensure_ascii=False)+'\n')
print(stats, 'mean',sum(lens)/len(lens),'max',max(lens),'min',min(lens))
print(collections.Counter(len(x['turns_extended']) for x in out).most_common(8))
y=next(x for x in out if x['id']=='yn-067'); print(json.dumps({k:y[k] for k in ('way_id','label','session_gist','session_gist_source')},ensure_ascii=False)); print(len(y['turns_extended']),sum(len(t['text']) for t in y['turns_extended']))
for t in y['turns_extended']: print('['+t['role']+']',t['text'][:160].replace('\n',' '))
