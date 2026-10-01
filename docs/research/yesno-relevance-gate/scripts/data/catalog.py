import os,re,json
root=os.path.expanduser('~/.claude/hooks/ways')
cat={}
for dp,dn,fn in os.walk(root,followlinks=True):
    base=os.path.basename(dp)
    if base+'.md' in fn:
        p=os.path.join(dp,base+'.md'); t=open(p).read()
        if not t.startswith('---'): continue
        fm=t.split('---',2)[1]
        def g(k):
            m=re.search(r'^'+k+r':\s*(.*)$',fm,re.M); return m.group(1).strip() if m else ''
        wid=os.path.relpath(dp,root)
        cat[wid]={'description':g('description'),'vocabulary':g('vocabulary'),'path':p}
json.dump(cat,open(os.path.dirname(__file__)+'/catalog.json','w'),indent=1)
print(len(cat), sum(1 for v in cat.values() if v['description']))
