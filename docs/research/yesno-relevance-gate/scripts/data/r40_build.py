import json,os,re,math
W=os.path.dirname(os.path.abspath(__file__)); D=W+'/../data/'
src=open(W+'/extend.py').read()
exec(src.split("BUDGET=4000")[0].replace("items=[json.loads(l) for l in open(D+'eval.v1.jsonl')]","items=[]"))
cat=json.load(open(W+'/catalog.json'))
pick=json.load(open(W+'/random40_pick.json'))
L={0:(0,"A game narrative about returning to a text terminal is not about rendering Mermaid diagrams with mmaid."),
1:(0,"The user wants a kernel clock branch forced on; no decision is being presented to them."),
2:(1,"The user is adding reasoning to an architecture decision record being drafted, so structural design guidance applies."),
3:(0,"'Use a subagent' asks for simple delegation, not multi-agent Workflow orchestration."),
4:(0,"Creating a cloud project for a game server involves no credential handling yet."),
5:(0,"Tracking closed tasks in the session task list has nothing to do with timesheets or billing."),
6:(0,"Renaming a tool in prose is not a question of delivery readiness."),
7:(1,"The user asks to resolve three open safety questions, a security review."),
8:(1,"The user asks to fix defects in sensor code, the core craft this way covers."),
9:(0,"The user suggests trying a pattern locally instead of adding it to agent-ways; no delegation is involved."),
10:(0,"Asking whether goals are another tracked item is not about vetting a package."),
11:(0,"A pasted session recap contains no decision to present."),
12:(0,"Adopting a KWin effect into the user's workflow has nothing to do with email drafting."),
13:(0,"Booking an 11 am appointment is not a request for a morning briefing."),
14:(0,"A canonical status line script in the repo is project tooling, not personal shell setup."),
15:(0,"Writing issues and running skills is not about establishing team norms."),
16:(0,"'Temporal AA' is a rendering technique, not a meeting recap."),
17:(0,"The user pastes a repo URL while an issue is being filed; nothing is being released."),
18:(0,"Renaming a tool is not a production change proposal."),
19:(0,"A one-word 'git' answer about committing is not a release."),
20:(0,"Researching a Claude Code capability for GitHub issues is not building the project's CLI tooling."),
21:(0,"Adding a git remote does not call for sharing an onboarding guide."),
22:(0,"Permission to run the develop and ship skills does not raise delivery readiness."),
23:(0,"A pasted terminal fragment ending in /wrap is not running a long or interactive shell command."),
24:(0,"Shuffling prompt-word decks for an image generator has nothing to do with Mermaid rendering."),
25:(1,"The user asks Claude to dispatch agents to preserve context."),
26:(0,"Protecting home server assets is not Node dependency security."),
27:(0,"The user asks to run the wrap skill, not to author a skill."),
28:(0,"The user is investigating which agents receive issue injections; delegating work is not the activity."),
29:(0,"Jira issues and Confluence pages are not the human's personal to-dos."),
30:(0,"A pasted file list containing a path segment matched '/merge'; nothing is being merged."),
31:(0,"'Outstanding issues' means problems with a service, not GitHub issues or PRs."),
32:(0,"Purging a mistaken decision record has nothing to do with setting a /goal."),
33:(0,"Calling the work a precursor to an AUR package is not about tool-agnostic way authoring."),
34:(0,"Running a hardware suspend test is not about the shape of test assertions."),
35:(0,"Choosing a brightness value for a scope is not policy enforcement or risk scoring."),
36:(0,"A pasted session recap is not about consolidating migration history."),
37:(0,"A gray stone house in a terrain renderer does not call for suggesting a diagram."),
38:(0,"Wanting agent skills to manage a ledger is not a call for Workflow orchestration."),
39:(0,"The user remarks on the project's ambition; nothing is being merged.")}
out=[]
for i,x in enumerate(pick):
    T=session(x['session']); p=[j for j,t in enumerate(T) if t['role']=='user' and t['ts']==x['prompt_ts']][-1]
    def cl(t):
        t=re.sub(r'</?pasted_content[^>]*>','',t); return ('…'+t[-1199:]) if len(t)>1200 else t
    turns=[{'role':t['role'],'text':cl(t['text'])} for t in T[max(0,p-3):p+1]]
    ext=[];tot=0
    for t in reversed(T[:p+1]):
        room=4000-tot
        if room<=0: break
        txt=t['text'] if len(t['text'])<=room else '…'+t['text'][-(room-1):]
        ext.insert(0,{'role':t['role'],'text':txt}); tot+=len(txt)
    title=T[p]['title']; first=next((t['text'] for t in T if t['role']=='user'),None)
    gist,gs=(title[1],'title') if title and title[1] else ((first[:300],'first_prompt') if first else (None,'none'))
    lab,rat=L[i]; c=cat.get(x['way_id'],{})
    out.append({'id':f"r40-{i:02d}",'way_id':x['way_id'],'summary':c.get('description',''),'vocabulary':c.get('vocabulary',''),'turns':turns,
      'label':lab,'category':'random','rationale':rat,'source':'real_fire','project':x['project'],'session':x['session'],'ts':x['prompt_ts'],
      'fire':{'trigger':x['trigger'],'fire_ts':x['ts'],'matched_span':x.get('matched_span')},'later_nonprompt_fires':None,'gate_events_in_window':None,
      'turns_extended':ext,'session_gist':gist,'session_gist_source':gs})
with open(D+'random40.jsonl','w') as f:
    for o in out: f.write(json.dumps(o,ensure_ascii=False)+'\n')
k=sum(o['label'] for o in out); n=len(out); z=1.96; ph=k/n
den=1+z*z/n; ctr=(ph+z*z/(2*n))/den; hw=z*math.sqrt(ph*(1-ph)/n+z*z/(4*n*n))/den
print(k,n,ph,round(ctr-hw,4),round(ctr+hw,4))
print(sum(1 for o in out if not o['summary']), [o['way_id'] for o in out if not o['summary']])
