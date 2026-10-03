#!/usr/bin/env python3
"""A synthetic HOME and XDG tree for the ways-cli reference screenshots.

Project /home/dev/shop has four sessions. Session A fires ways in main and
five subagents (a code-reviewer, a workflow member, two general-purpose
agents and one with no meta.json), with every outcome the timeline marks.
The judge's calls spread over several days for the spend tab. A second and
third project fill the projects screen. No real ids or paths appear.

Usage: fixture.py <dir>   (shots.sh runs it; <dir>/fx is rebuilt)
"""
import json, os, shutil, sys, time
from datetime import datetime, timedelta

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../../.."))
F = os.path.join(os.path.abspath(sys.argv[1]), "fx")
shutil.rmtree(F, ignore_errors=True)
HOME = os.path.join(F, "home")
for d in ["home/.local/state/agent-ways", "home/.config/agent-ways", "home/.local/share", "home/.cache/agent-ways/agent", "run"]:
    os.makedirs(os.path.join(F, d), exist_ok=True)

PROJ = "/home/dev/shop"
SLUG = "-home-dev-shop"
A = "3f6c2a91-4b7e-4d10-9c55-2e8b7a1d0f42"
B = "8a0d5e17-6c2f-4a93-b1e4-7f3c9d2a6b05"
C = "c41b9f62-0e8d-4f37-a2c6-5d1e8b3a9f70"
D = "e92f3a08-7d4c-4b16-8e5a-1c6b0d9f2e33"

events = []

def ev(ts, event, sid=A, proj=PROJ, **kw):
    e = {"ts": ts, "event": event, "project": proj, "session": sid}
    e.update({k: v for k, v in kw.items() if v is not None})
    events.append(e)

def fire(ts, way, trig, agent="main", sid=A, score=None, surface=None):
    ev(ts, "way_fired", sid, way=way, trigger=trig, agent_id=agent,
       fire_score=score, surface=surface)

def judge(ts, way, p, verdict, agent="main", sid=A):
    ev(ts, "way_judged", sid, way=way, p_yes=p, verdict=verdict, agent_id=agent, threshold="0.30",
       mode="shadow" if verdict == "would_block" else "enforce", engine="anthropic",
       model="claude-haiku-4-5", judge_ms="412")

def call(ts, sid, tin, tout, cost, src="provider"):
    ev(ts, "judge_call", sid, input_tokens=str(tin), output_tokens=str(tout),
       cost_usd=cost, cost_source=src)

CR = "a1b2c3d4e5f6a7b8c"   # code-reviewer
WF = "a9e8d7c6b5a4f3e2d"   # workflow member audit:judge
G1 = "a3897f0e1d2c3b4a5"   # general-purpose
G2 = "a5c21e0f9d8c7b6a5"   # general-purpose
UN = "a0123456789abcdef"   # no meta.json

day = "2026-10-01T"
ev(day + "10:00:00Z", "session_start")
fire(day + "10:00:05Z", "softwaredev/code/testing", "semantic:embedding:en", score="0.612", surface="add a unit test for the cart total")
call(day + "10:00:05Z", A, 1850, 12, "0.0021")
fire(day + "10:02:00Z", "softwaredev/docs/adr", "file")
fire(day + "10:04:00Z", "softwaredev/code/quality", "semantic:embedding:en", score="0.438", surface="clean up the checkout module")
call(day + "10:04:00Z", A, 2210, 14, "0.0025")
ev(day + "10:06:00Z", "check_fired", check="softwaredev/docs/adr", trigger="file", agent_id="main")
ev(day + "10:06:01Z", "check_fired", check="softwaredev/docs/adr", trigger="file", agent_id="main")
ev(day + "10:06:02Z", "check_fired", check="softwaredev/docs/adr", trigger="file", agent_id="main")
ev(day + "10:06:03Z", "check_fired", check="softwaredev/docs/adr", trigger="file", agent_id="main")
ev(day + "10:06:04Z", "check_fired", check="softwaredev/docs/adr", trigger="file", agent_id="main")
fire(day + "10:08:00Z", "softwaredev/delivery/review", "keyword", agent=CR)
fire(day + "10:08:01Z", "softwaredev/docs/adr", "file", agent=CR)
fire(day + "10:08:02Z", "softwaredev/code/testing", "keyword", agent=WF)
fire(day + "10:10:00Z", "softwaredev/environment/deps", "bash", agent=G1)
fire(day + "10:10:01Z", "softwaredev/environment/deps", "bash", agent=G2)
fire(day + "10:10:02Z", "softwaredev/code/security", "semantic:embedding:en", agent=UN, score="0.402", surface="check the token handling")
call(day + "10:10:02Z", A, 1990, 11, "0.0022")
fire(day + "10:12:00Z", "softwaredev/architecture/design", "semantic:embedding:en", score="0.507", surface="split the pricing service")
judge(day + "10:12:00Z", "softwaredev/architecture/design", "0.880", "pass")
call(day + "10:12:00Z", A, 2400, 13, "0.0027")
ev(day + "10:14:00Z", "way_redisclosed", way="softwaredev/code/testing", trigger="semantic:embedding:en", agent_id="main",
   fire_score="0.574", surface="the cart total test still fails")
judge(day + "10:14:00Z", "softwaredev/code/performance", "0.140", "would_block")
fire(day + "10:14:00Z", "softwaredev/code/performance", "semantic:embedding:en", score="0.391", surface="the cart page is slow")
judge(day + "10:14:01Z", "itops/incident", "0.050", "block")
call(day + "10:14:01Z", A, 2600, 12, "0.0029")
call(day + "10:14:01Z", A, 0, 0, None, "unknown")
ev(day + "10:14:02Z", "way_suppressed", way="softwaredev/docs/adr", trigger="file", agent_id=CR, kind="way", reason="refire")
ev(day + "10:14:02Z", "way_suppressed", way="meta/knowledge", trigger="keyword", agent_id="main", kind="way", reason="context_cap")
ev(day + "10:14:03Z", "check_fired", check="softwaredev/docs/adr", trigger="file", agent_id=CR)

# Other sessions: a few fires and judge calls on other days.
for sid, d, n in [(B, "2026-09-27", 6), (C, "2026-09-29", 9), (D, "2026-10-02", 4)]:
    ev(d + "T09:00:00Z", "session_start", sid)
    for i in range(n):
        t = f"{d}T09:{10 + i:02d}:00Z"
        fire(t, ["softwaredev/code/testing", "softwaredev/delivery/commits", "softwaredev/docs/adr"][i % 3],
             "semantic:embedding:en", sid=sid, score=f"{0.42 + i * 0.03:.3f}", surface="fix the failing build")
        call(t, sid, 1700 + 90 * i, 12, f"{0.0019 + 0.0001 * i:.4f}")
# Two older months for the month view.
for d in ["2026-08-12", "2026-08-20", "2026-09-03", "2026-09-15"]:
    call(d + "T12:00:01Z", "b05e7c3d-1a9f-4e62-8d40-6f2c1b8e9a17", 2100, 12, "0.0024")
events.sort(key=lambda e: e["ts"])
with open(os.path.join(F, "home/.local/state/agent-ways/events.jsonl"), "w") as f:
    for e in events:
        f.write(json.dumps(e) + "\n")

# Transcripts: assistant turns carrying usage, for the token gauge.
def transcript(slug, sid, start, turns, cwd, mtime=None, title=None):
    pd = os.path.join(HOME, ".claude/projects", slug)
    os.makedirs(pd, exist_ok=True)
    path = os.path.join(pd, sid + ".jsonl")
    t0 = datetime.fromisoformat(start.replace("Z", "+00:00"))
    with open(path, "w") as f:
        f.write(json.dumps({"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": start,
                            "message": {"role": "user", "content": title or "work on the shop"}}) + "\n")
        for i in range(turns):
            ts = (t0 + timedelta(seconds=60 * i + 30)).strftime("%Y-%m-%dT%H:%M:%S.000Z")
            usage = {"input_tokens": 400, "cache_creation_input_tokens": 2000,
                     "cache_read_input_tokens": 40000 + 1200 * i, "output_tokens": 600}
            f.write(json.dumps({"type": "assistant", "sessionId": sid, "cwd": cwd, "timestamp": ts,
                                "message": {"role": "assistant", "model": "claude-opus-4-5", "usage": usage,
                                            "content": [{"type": "text", "text": "ok"}]}}) + "\n")
    if mtime is not None:
        os.utime(path, (mtime, mtime))
    return path

now = time.time()
transcript(SLUG, A, "2026-10-01T10:00:00Z", 16, PROJ, now - 3 * 86400, "add a unit test for the cart total")
transcript(SLUG, B, "2026-09-27T09:00:00Z", 8, PROJ, now - 6 * 86400, "fix the failing build")
transcript(SLUG, C, "2026-09-29T09:00:00Z", 10, PROJ, now - 4 * 86400, "tidy the release notes")
transcript(SLUG, D, "2026-10-02T09:00:00Z", 5, PROJ, now - 20, "wire the payment webhook")

# Subagent meta for session A.
sub = os.path.join(HOME, ".claude/projects", SLUG, A, "subagents")
os.makedirs(os.path.join(sub, "workflows/wf_audit"), exist_ok=True)
for aid, typ in [(CR, "code-reviewer"), (G1, "general-purpose"), (G2, "general-purpose")]:
    json.dump({"agentType": typ, "description": "a task"}, open(os.path.join(sub, f"agent-{aid}.meta.json"), "w"))
json.dump({"agentType": "workflow-subagent", "description": "audit:judge", "workflowPhase": "Audit"},
          open(os.path.join(sub, f"workflows/wf_audit/agent-{WF}.meta.json"), "w"))

# Two more projects for the projects screen.
transcript("-home-dev-notes", "5b7e1c90-2d4a-4f68-9e13-0a8c6f2b4d71", "2026-09-20T14:00:00Z", 6, "/home/dev/notes",
           now - 13 * 86400, "outline the onboarding guide")
transcript("-home-dev-infra", "71d3e5a2-9f0b-4c86-a4d7-3e1b8c0f6a29", "2026-09-30T08:00:00Z", 12, "/home/dev/infra",
           now - 3.5 * 86400, "rotate the staging certificates")
os.makedirs(os.path.join(HOME, ".claude/projects/-home-dev-scratch"), exist_ok=True)

# The shipped ways, so the why view shows a way's text.
os.makedirs(os.path.join(HOME, ".claude/hooks"), exist_ok=True)
os.symlink(os.path.join(REPO, "hooks/ways"), os.path.join(HOME, ".claude/hooks/ways"))

# A cached model list, so the gate's model key offers a picker.
models = [{"id": i, "name": n, "input_per_mtok": p, "output_per_mtok": q} for i, n, p, q in [
    ("claude-haiku-4-5", "Claude Haiku 4.5", 1.0, 5.0),
    ("claude-sonnet-4-5", "Claude Sonnet 4.5", 3.0, 15.0),
    ("claude-opus-4-5", "Claude Opus 4.5", 5.0, 25.0),
    ("claude-3-5-haiku-latest", "Claude Haiku 3.5", 0.8, 4.0),
]]
json.dump({"provider": "anthropic", "fetched_at": int(now), "models": models},
          open(os.path.join(F, "home/.cache/agent-ways/agent/models-anthropic.json"), "w"), indent=1)
print(len(events), "events")
