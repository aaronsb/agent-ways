import json,glob,os,re,datetime,collections
W=os.path.dirname(os.path.abspath(__file__))
cat=json.load(open(W+'/catalog.json')); o=json.load(open(W+'/fires_joined.json'))
def title(w):
    if w not in cat: return None
    m=re.search(r'^# (.+)$',open(cat[w]['path']).read(),re.M); return m.group(1).strip() if m else None
idx={os.path.basename(f)[:-6]:f for f in glob.glob(os.path.expanduser('~/.claude/projects/*/*.jsonl'))}
pts=lambda s: datetime.datetime.fromisoformat(s.replace('Z','+00:00')).timestamp()
cache={}
def atts(s):
    if s in cache: return cache[s]
    r=[]
    for l in open(idx[s]):
        try:e=json.loads(l)
        except: continue
        if e.get('type')=='attachment' and e.get('timestamp'):
            a=e['attachment']
            txt=json.dumps(a)
            for p in re.findall(r'saved to: (\S+?\.txt)',txt):
                try: txt+=open(p).read()
                except Exception: pass
            r.append((pts(e['timestamp']),a.get('hookEvent') or a.get('type'),txt))
    cache[s]=r; return r
c=collections.Counter()
for x in o:
    t=title(x['way_id'])
    if not t: x['verified']=None; c['notitle']+=1; continue
    p=pts(x['prompt_ts']); ok=False
    for ts,ev,txt in atts(x['session']):
        if p-1<=ts<=p+40 and ('# '+t) in txt: ok=True;break
    x['verified']=ok; c[ok]+=1
json.dump(o,open(W+'/fires_joined.json','w'))
print(c)
