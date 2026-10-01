import json,re,os,collections,glob
W=os.path.dirname(os.path.abspath(__file__)); D=W+'/../data/'
cat=json.load(open(W+'/catalog.json'))
title2id=collections.defaultdict(list)
root=os.path.expanduser('~/.claude/hooks/ways')
for dp,dn,fn in os.walk(root,followlinks=True):
    b=os.path.basename(dp)
    if b+'.md' in fn:
        m=re.search(r'^# (.+)$',open(os.path.join(dp,b+'.md')).read(),re.M)
        if m: title2id[m.group(1).strip()].append(os.path.relpath(dp,root))
F=os.path.expanduser('~/.claude/projects/-home-aaron-Projects-ai-harness-agent-ways/418e1be3-ce8d-4493-b2c8-31fc97f133a7.jsonl')
STRIP=re.compile(r'<(system-reminder|command-[a-z-]+|local-command-[a-z-]+|task-notification|teammate-message)[^>]*>.*?</\1>',re.S)
def full(c):
    s='\n'.join(c) if isinstance(c,list) else str(c)
    for p in re.findall(r'saved to: (\S+?\.txt)',s):
        try: s=s.replace(s,s+'\n'+open(p).read())
        except Exception: pass
    return s
tool_in={};last_prompt=None;rows=[];skipped=collections.Counter()
for l in open(F):
    e=json.loads(l); t=e.get('type')
    if e.get('isSidechain'): continue
    if t=='assistant':
        for x in e['message'].get('content') or []:
            if isinstance(x,dict) and x.get('type')=='tool_use':
                i=x.get('input',{}); tool_in[x['id']]=(x['name'], i.get('command') or i.get('file_path') or i.get('pattern') or json.dumps(i)[:200])
    elif t=='user' and not e.get('isMeta'):
        c=e['message'].get('content')
        if isinstance(c,str) or (isinstance(c,list) and not any(y.get('type')=='tool_result' for y in c)):
            txt=c if isinstance(c,str) else '\n'.join(y.get('text','') for y in c if y.get('type')=='text')
            txt=STRIP.sub('',txt).strip()
            if txt: last_prompt=txt
    elif t=='attachment':
        a=e['attachment']
        if a.get('type')=='queued_command' and isinstance(a.get('prompt'),str): last_prompt=STRIP.sub('',a['prompt']).strip() or last_prompt
        if a.get('type')!='hook_additional_context': continue
        ev=a.get('hookEvent'); s=full(a.get('content'))
        # drop preview copies inside persisted-output wrapper, keep the full file text
        s=re.sub(r'<persisted-output>.*?</persisted-output>','',s,flags=re.S)
        lane={'UserPromptSubmit':'prompt','PreToolUse':'tool','PostToolUse':'tool','SessionStart':'session_start'}.get(ev,ev)
        if lane=='prompt': trig=(last_prompt or '')[-200:]
        elif lane=='tool':
            nm,inp=tool_in.get(a.get('toolUseID'),('?','?')); trig=f"{a.get('hookName')} {str(inp)[:200-len(a.get('hookName',''))-1]}"
        else: trig='SessionStart'
        chunks=re.split(r'(?=<!-- epistemic:)',s)
        for ch in chunks:
            m=re.search(r'^# (.+)$',ch,re.M)
            if not ch.startswith('<!-- epistemic:'):
                if ch.strip(): skipped[(lane,(ch.strip().splitlines()[0])[:60])]+=1
                continue
            title=m.group(1).strip() if m else None
            ids=title2id.get(title,[])
            if not ids: skipped[(lane,'untitled/unmapped:'+str(title))]+=1; continue
            wid=ids[0]
            if wid=='core.md' or title in('Available Ways',): skipped[(lane,'core')]+=1; continue
            rows.append({'lane':lane,'way_id':wid,'summary':cat.get(wid,{}).get('description',''),'trigger_excerpt':trig,'ts':e.get('timestamp'),'_ambig_title':len(ids)>1})
for i,r in enumerate(rows): r['id']=f"s-{i:03d}"
json.dump(rows,open(W+'/session_rows.json','w'))
print(len(rows), collections.Counter(r['lane'] for r in rows))
print('ambiguous titles',[r['way_id'] for r in rows if r['_ambig_title']])
for k,v in skipped.most_common(40): print(v,k)
