#!/usr/bin/env python3
"""Prompt-lane ways per prompt, last 30 days, from the event log.

No prompt event is logged, so a prompt is a run of prompt-lane events in one session
with gaps under 3 s. Main agent only. Counts way_fired alone and way_fired + way_redisclosed.
Only prompts with at least one fire are visible, so the distribution is conditional on >= 1.
"""
import json
import os
from collections import defaultdict
from datetime import datetime, timezone

EVENTS = os.path.expanduser("~/.local/state/agent-ways/events.jsonl")
LANE = ("semantic:embedding", "semantic:late-interaction", "keyword")


def t(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(q * len(xs)))]


def main():
    rows = []
    for line in open(EVENTS):
        try:
            e = json.loads(line)
        except ValueError:
            continue
        if e.get("event") not in ("way_fired", "way_redisclosed"):
            continue
        if not str(e.get("trigger", "")).startswith(LANE):
            continue
        if e.get("scope") != "agent" or e.get("agent_id") not in (None, "main"):
            continue
        rows.append((e["session"], t(e["ts"]), e["event"], e.get("way")))
    end = max(r[1] for r in rows)
    start = end - 30 * 86400
    out = {"window": [datetime.fromtimestamp(start, timezone.utc).isoformat(),
                      datetime.fromtimestamp(end, timezone.utc).isoformat()]}
    for name, kinds in (("fired", {"way_fired"}), ("fired+redisclosed", {"way_fired", "way_redisclosed"})):
        by_s = defaultdict(list)
        for s, ts, ev, w in rows:
            if ev in kinds and ts >= start:
                by_s[s].append((ts, w))
        counts = []
        for evs in by_s.values():
            evs.sort()
            cur, last = set(), None
            for ts, w in evs:
                if last is not None and ts - last >= 3:
                    counts.append(len(cur))
                    cur = set()
                cur.add(w)
                last = ts
            counts.append(len(cur))
        out[name] = {"prompts": len(counts), "fires": sum(counts), "p50": pct(counts, .5),
                     "p90": pct(counts, .9), "p99": pct(counts, .99), "max": max(counts),
                     "mean": sum(counts) / len(counts)}
    json.dump(out, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "fires_per_prompt.json"), "w"),
              indent=1)
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
