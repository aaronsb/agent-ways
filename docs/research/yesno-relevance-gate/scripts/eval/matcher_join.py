#!/usr/bin/env python3
"""Join each real fire to its way_fired telemetry event and keep fire_score, if recorded.

Items carry only trigger, fire_ts and matched_span. The event log carries fire_score on
some way_fired events. Match: same way, same session (sets 1, 2) and |ts - fire_ts| <= 2 s;
for set 3 (no session field) same way and |ts - item ts| <= 2 s.
"""
import json
import os
from collections import defaultdict
from datetime import datetime
from pathlib import Path

P = Path(__file__).resolve().parent
DATA = P.parent / "data"
EVENTS = Path(os.path.expanduser("~/.local/state/agent-ways/events.jsonl"))


def t(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def main():
    by_way = defaultdict(list)
    for line in open(EVENTS):
        try:
            e = json.loads(line)
        except ValueError:
            continue
        if e.get("event") == "way_fired":
            by_way[e["way"]].append((t(e["ts"]), e.get("session"), e.get("fire_score"), e.get("trigger")))
    out = {}
    for name in ("eval.jsonl", "random40.jsonl", "session_unlabelled.jsonl"):
        for line in open(DATA / name):
            it = json.loads(line)
            fire = it.get("fire")
            if name != "session_unlabelled.jsonl" and not fire:
                continue
            ts = t(fire["fire_ts"]) if fire else t(it["ts"])
            sess = it.get("session")
            cands = [c for c in by_way[it["way_id"]]
                     if abs(c[0] - ts) <= 2 and (sess is None or c[1] == sess)]
            scored = [c for c in cands if c[2] is not None]
            out[it["id"]] = {"matched": bool(cands), "fire_score": scored[0][2] if scored else None,
                             "trigger": (cands[0][3] if cands else None)}
    json.dump(out, open(P / "matcher_scores.json", "w"), indent=0)
    n = len(out)
    print(n, "fires;", sum(v["matched"] for v in out.values()), "joined;",
          sum(v["fire_score"] is not None for v in out.values()), "with fire_score")


if __name__ == "__main__":
    main()
