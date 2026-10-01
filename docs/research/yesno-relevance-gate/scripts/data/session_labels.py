import json,os,collections
W=os.path.dirname(os.path.abspath(__file__)); D=W+'/../data/'
rows=json.load(open(W+'/session_rows.json'))
YES={
'002':"The prompt is about cutting releases from merged PRs, so the merge gate applies.",
'004':"The command checks recent tags before cutting releases, part of shipping.",
'006':"The command lists release tags right before cutting three releases.",
'013':"The command inspects how the Makefile drives a component's release download, so Makefile guidance applies.",
'030':"The command inspects the project's own `ways update` CLI code before changing it.",
'032':"Claude is reading update.rs in order to fix it, so the change should integrate into the file.",
'037':"The command runs the test suite after a change.",
'038':"The command runs the tests and reads CI status before merging, a verification gate.",
'054':"The user challenges whether code review is happening, which is code-quality practice.",
'055':"The user asks whether code is being reviewed, which this way's review standards cover.",
'087':"The command creates a new branch for a docs change.",
'090':"The change being made exempts version bumps from the review gate, which loosens a control.",
'104':"The command reads a PR body before merging it.",
'105':"The command watches PR checks with a blocking --watch, a long-running shell command.",
'107':"The command merges a PR and cleans up, which is delivery.",
'113':"The command starts an ADR on a new branch with the adr tool.",
'114':"The command creates a new ADR document.",
'115':"A new ADR has just been created, so the consider step comes next.",
'117':"The command reads the new ADR and a precedent ADR to understand existing decisions.",
'119':"Claude is writing an ADR markdown file.",
'120':"The user adds reasoning to an architecture decision being drafted.",
'168':"The command surveys way descriptions per directory for a change to the ways tree structure.",
'173':"The command files a tasklist GitHub issue, which this way's issue conventions cover.",
}
out=[]
for r in rows:
    n=r['id'][2:]
    if n in YES: lab,rat=1,YES[n]
    else:
        trig=r['trigger_excerpt'].replace('\n',' ')
        what={'prompt':'the user prompt','tool':'the tool call','session_start':'session start'}[r['lane']]
        lab,rat=0,f"Nothing in {what} (`{trig[:70].strip()}`) involves this way's subject: {r['summary'][:90].rstrip()}."
    out.append({'id':r['id'],'label':lab,'rationale':rat})
with open(D+'session_labels.jsonl','w') as f:
    for o in out: f.write(json.dumps(o,ensure_ascii=False)+'\n')
with open(D+'session_unlabelled.jsonl','w') as f:
    for r in rows: f.write(json.dumps({k:r[k] for k in ('id','lane','way_id','summary','trigger_excerpt','ts')},ensure_ascii=False)+'\n')
lab={o['id']:o['label'] for o in out}
for lane in ('prompt','tool','session_start'):
    ids=[r['id'] for r in rows if r['lane']==lane]; print(lane,len(ids),sum(lab[i] for i in ids))
c=collections.Counter(r['way_id'] for r in rows)
for w,n in c.most_common(7): print(w,n,sum(lab[r['id']] for r in rows if r['way_id']==w))
# duplicates: same way, same trigger
g=collections.defaultdict(list)
for r in rows: g[(r['way_id'],r['trigger_excerpt'][:80])].append(r['id'])
print('dup same trigger',[v for v in g.values() if len(v)>1])
# triggers that fired many ways at once
t=collections.Counter(r['trigger_excerpt'][:80] for r in rows); print('triggers',len(t),'max ways per trigger',t.most_common(1)[0][1])
print('summaries empty',sum(1 for r in rows if not r['summary']))
