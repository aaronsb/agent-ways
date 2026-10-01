import json,os,glob,re,collections,datetime
W=os.path.dirname(os.path.abspath(__file__))
cat=json.load(open(W+'/catalog.json'))
PROMPT_TRIG=lambda t: t and (t.startswith('semantic:embedding') or t.startswith('semantic:late') or t=='keyword')
fires=collections.defaultdict(list)
for l in open(os.path.expanduser('~/.local/state/agent-ways/events.jsonl')):
    try:e=json.loads(l)
    except: continue
    if e.get('event')!='way_fired' or e.get('scope')!='agent': continue
    if e.get('agent_id') not in (None,'main'): continue
    if not PROMPT_TRIG(e.get('trigger')): continue
    fires[e['session']].append(e)
idx={}
for f in glob.glob(os.path.expanduser('~/.claude/projects/*/*.jsonl')):
    idx[os.path.basename(f)[:-6]]=f
def pts(s): return datetime.datetime.fromisoformat(s.replace('Z','+00:00')).timestamp()
STRIP=re.compile(r'<(system-reminder|command-[a-z-]+|local-command-[a-z-]+|bash-[a-z-]+|task-notification|teammate-message|user-prompt-submit-hook)[^>]*>.*?</\1>',re.S)
def clean(t):
    t=STRIP.sub('',t)
    t=re.sub(r'<persisted-output>.*?</persisted-output>','',t,flags=re.S)
    return t.strip()
def user_text(m):
    c=m.get('content')
    if isinstance(c,str): return c
    if isinstance(c,list):
        if any(x.get('type')=='tool_result' for x in c): return None
        return '\n'.join(x.get('text','') for x in c if x.get('type')=='text')
    return None
out=[];stats=collections.Counter()
for sess,fl in fires.items():
    f=idx.get(sess)
    if not f: stats['no_transcript']+=len(fl); continue
    turns=[]  # (role,text,ts)
    inj=[]    # (ts, content) userprompt hook contexts
    cur_asst=[]
    for l in open(f):
        try:e=json.loads(l)
        except: continue
        if e.get('isSidechain'): continue
        t=e.get('type')
        if t=='user' and not e.get('isMeta'):
            ut=user_text(e.get('message',{}))
            if ut is None: continue
            ut=clean(ut)
            if not ut or ut.startswith('[Request interrupted') or ut.startswith('Caveat:'): continue
            if cur_asst: turns.append(('assistant','\n\n'.join(cur_asst),None)); cur_asst=[]
            turns.append(('user',ut,e.get('timestamp')))
        elif t=='assistant':
            for x in (e.get('message',{}).get('content') or []):
                if isinstance(x,dict) and x.get('type')=='text' and x.get('text','').strip():
                    cur_asst.append(clean(x['text']))
        elif t=='attachment':
            a=e.get('attachment',{})
            if a.get('type')=='queued_command' and a.get('commandMode','prompt')=='prompt' and isinstance(a.get('prompt'),str):
                ut=clean(a['prompt'])
                if ut:
                    if cur_asst: turns.append(('assistant','\n\n'.join(cur_asst),None)); cur_asst=[]
                    turns.append(('user',ut,a.get('timestamp') or e.get('timestamp')))
            if a.get('type')=='hook_additional_context' or a.get('hookEvent')=='UserPromptSubmit':
                inj.append((e.get('timestamp'),json.dumps(a)[:200000]))
    uidx=[i for i,x in enumerate(turns) if x[0]=='user' and x[2]]
    for e in fl:
        ft=pts(e['ts'])
        best=None
        for i in uidx:
            ut=pts(turns[i][2])
            if ut<=ft+2: best=i
            else: break
        if best is None or ft-pts(turns[best][2])>60: stats['no_prompt']+=1; continue
        stats['joined']+=1
        ctx=turns[max(0,best-3):best+1]
        out.append({'way_id':e['way'],'trigger':e['trigger'],'session':sess,'project':e.get('project'),'ts':e['ts'],
            'prompt_ts':turns[best][2],'turn_index':best,'matched_span':e.get('matched_span'),'fire_score':e.get('fire_score'),
            'in_catalog':e['way'] in cat,
            'turns':[{'role':r,'text':tx} for r,tx,_ in ctx]})
json.dump(out,open(W+'/fires_joined.json','w'))
print(stats, len(out), len(set(o['session'] for o in out)), len(set(o['way_id'] for o in out)), sum(o['in_catalog'] for o in out))
