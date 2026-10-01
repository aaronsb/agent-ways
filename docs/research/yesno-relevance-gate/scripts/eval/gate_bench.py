#!/usr/bin/env python3
"""Drive the relevance gate with synthetic prompts and measure it.

Each prompt runs `ways scan prompt` in a fresh session (gatebench-<run>-<n>),
so refire suppression never hides a candidate. Wall time is measured around
the scan; the gate's own events (way_judged / gate_fallback) are read back
from events.jsonl by session id. Sessions are reset afterwards.

usage: gate_bench.py [--concurrency N] [--repeat R] [--project DIR]
"""
import argparse, json, os, statistics, subprocess, time, uuid
from concurrent.futures import ThreadPoolExecutor

WAYS = os.path.expanduser("~/.claude/bin/ways")
EVENTS = os.path.join(os.environ.get("XDG_STATE_HOME", os.path.expanduser("~/.local/state")),
                      "agent-ways", "events.jsonl")

# (label, prompt). Labels are what a human would expect: "on" = some way
# should plausibly apply, "off" = chit-chat or unrelated to any way.
PROMPTS = [
    ("on",  "write a database migration that adds a nullable column to the orders table"),
    ("on",  "let's write an ADR for choosing between sqlite and postgres"),
    ("on",  "cut a release and tag the new version, then publish the binaries"),
    ("on",  "open a PR for this branch and get it reviewed before merge"),
    ("on",  "the CI job is flaky, it failed on a network timeout, should I retry it"),
    ("on",  "set up an ssh-agent so my key persists across terminals"),
    ("on",  "add unit tests for the parser and make sure they run in CI"),
    ("on",  "scan our lockfile for vulnerable dependencies before installing"),
    ("on",  "draw a mermaid diagram of the request flow"),
    ("on",  "commit these changes with a good conventional commit message"),
    ("on",  "refactor this module, the function is 300 lines long"),
    ("on",  "this repo might be untrusted, is it safe to run its install script"),
    ("off", "thanks, that looks good"),
    ("off", "what's the weather like in seattle today"),
    ("off", "ok continue"),
    ("off", "how many tokens of context do I have left"),
    ("off", "should I stop claude code and resume it?"),
    ("off", "tell me a joke about cats"),
    ("off", "yes, go with the default"),
    ("off", "I'm going to grab coffee, back in ten"),
]


def scan(prompt, session, project):
    t0 = time.monotonic()
    out = subprocess.run(
        [WAYS, "scan", "prompt", f"--query={prompt.lower()}", f"--session={session}",
         f"--project={project}", "--response-context="],
        capture_output=True, text=True, env={**os.environ, "CLAUDE_PROJECT_DIR": project})
    wall = (time.monotonic() - t0) * 1000
    shown = out.stdout.count("<!-- epistemic:")
    return wall, shown, out.returncode


def read_events(sessions):
    want = set(sessions)
    by = {s: [] for s in sessions}
    with open(EVENTS) as f:
        f.seek(max(0, os.path.getsize(EVENTS) - 8_000_000))
        f.readline()
        for line in f:
            if "gatebench-" not in line:
                continue
            try:
                e = json.loads(line)
            except ValueError:
                continue
            if e.get("session") in want:
                by[e["session"]].append(e)
    return by


def pct(xs, p):
    xs = sorted(xs)
    if not xs:
        return float("nan")
    k = (len(xs) - 1) * p / 100
    lo, hi = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--concurrency", type=int, default=1)
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument("--project", default=os.getcwd())
    ap.add_argument("--keep", action="store_true", help="skip the session reset")
    a = ap.parse_args()

    run = uuid.uuid4().hex[:6]
    jobs = [(i, lbl, p, f"gatebench-{run}-{i}")
            for i, (lbl, p) in enumerate(PROMPTS * a.repeat)]

    def one(job):
        i, lbl, p, s = job
        return (i, lbl, p, s, *scan(p, s, a.project))

    t0 = time.monotonic()
    with ThreadPoolExecutor(a.concurrency) as ex:
        rows = list(ex.map(one, jobs))
    total = time.monotonic() - t0
    time.sleep(0.3)
    ev = read_events([r[3] for r in rows])

    print(f"run {run}: {len(rows)} prompts, concurrency {a.concurrency}, {total:.1f}s total\n")
    print(f"{'lbl':3} {'wall':>6} {'gate':>6} {'judge':>6} {'cand':>4} {'pass':>4} {'blk':>4} {'shown':>5}  prompt / verdicts")
    walls_g, walls_n, gate_ms, judge_ms, falls = [], [], [], [], []
    tally = {"on": [0, 0], "off": [0, 0]}  # [passed, blocked]
    for i, lbl, p, s, wall, shown, rc in rows:
        es = ev[s]
        j = [e for e in es if e["event"] == "way_judged"]
        fb = [e for e in es if e["event"] == "gate_fallback"]
        g = int(j[0]["gate_ms"]) if j else (int(fb[0].get("gate_ms", 0)) if fb else None)
        jm = int(j[0]["judge_ms"]) if j else None
        npass = sum(e["verdict"] == "pass" for e in j)
        nblk = sum(e["verdict"] in ("block", "would_block") for e in j)
        tally[lbl][0] += npass
        tally[lbl][1] += nblk
        (walls_g if (j or fb) else walls_n).append(wall)
        if g is not None:
            gate_ms.append(g)
        if jm is not None:
            judge_ms.append(jm)
        falls += [e.get("reason", "?") for e in fb]
        verdicts = " ".join(f"{'+' if e['verdict']=='pass' else '-'}{e['way'].split('/')[-1]}:{float(e['p_yes']):.2f}" for e in j)
        if fb:
            verdicts += f" FALLBACK({fb[0].get('reason')})"
        print(f"{lbl:3} {wall:6.0f} {g if g is not None else '-':>6} {jm if jm is not None else '-':>6} "
              f"{len(j) or (fb and fb[0].get('candidates')) or 0:>4} {npass:>4} {nblk:>4} {shown:>5}  {p[:48]}"
              + (f"\n{'':44}{verdicts}" if verdicts else "") + ("" if rc == 0 else f"  rc={rc}"))

    def line(name, xs):
        if xs:
            print(f"  {name:28} n={len(xs):3}  p50 {pct(xs,50):6.0f}  p95 {pct(xs,95):6.0f}  max {max(xs):6.0f} ms")
    print("\nlatency")
    line("scan wall, gate called", walls_g)
    line("scan wall, no candidates", walls_n)
    line("gate (hook round trip)", gate_ms)
    line("judge (provider call)", judge_ms)
    if gate_ms and judge_ms and len(gate_ms) == len(judge_ms):
        line("gate - judge (overhead)", [g - j for g, j in zip(gate_ms, judge_ms)])
    print(f"\nverdicts  on-prompts: {tally['on'][0]} pass / {tally['on'][1]} block"
          f"   off-prompts: {tally['off'][0]} pass / {tally['off'][1]} block")
    print(f"fallbacks {len(falls)}" + (f": {sorted(set(falls))}" if falls else ""))

    if not a.keep:
        for r in rows:
            subprocess.run([WAYS, "reset", "--session", r[3], "--confirm"], capture_output=True)


if __name__ == "__main__":
    main()
