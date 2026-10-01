"""AUC, ΔAUC vs base (paired bootstrap over prompt groups), operating point at the
shipped threshold, latency and output tokens for each judge variant."""
import collections, json, os, random, statistics

P = os.path.expanduser('~/.local/state/agent-ways/probes/yesno-gate')
THRESHOLD = 0.3
labels = {}
for f in ('eval', 'random40', 'session_labels'):
    for l in open(f'{P}/data/{f}.jsonl'):
        r = json.loads(l)
        labels[r['id']] = r['label']

rows = [json.loads(l) for l in open(P + '/eval/variant_scores.jsonl')]
by = collections.defaultdict(dict)           # variant -> (set, id) -> row
for r in rows:
    by[r['variant']][(r['set'], r['id'])] = r
variants = [v for v in ('base', 'compact', 'structured', 'catalog') if v in by]
assert 'base' in variants, 'score the base variant first: every delta is against it'
SUBSETS = {'s1+s2 (T1)': ('s1', 's2'), 's3 real fires (EX)': ('s3',), 'all': ('s1', 's2', 's3')}


def auc(pairs):
    pos = [p for p, y in pairs if y == 1]
    neg = [p for p, y in pairs if y == 0]
    if not pos or not neg:
        return float('nan')
    wins = sum((p > n) + 0.5 * (p == n) for p in pos for n in neg)
    return wins / (len(pos) * len(neg))


def scored(v, sets):
    return {k: r for k, r in by[v].items() if k[0] in sets and r['p_yes'] is not None and k[1] in labels}


def ci(xs):
    xs = sorted(xs)
    return xs[int(.025 * len(xs))], xs[int(.975 * len(xs)) - 1]


def prec_rec(v, keys, t=THRESHOLD):
    passed = [k for k in keys if by[v][k]['p_yes'] >= t]
    tp = sum(labels[k[1]] for k in passed)
    npos = sum(labels[k[1]] for k in keys)
    return (tp / len(passed) if passed else float('nan'), tp / npos if npos else float('nan'), len(passed) - tp)


random.seed(7)
for name, sets in SUBSETS.items():
    # Sorted, so the seeded bootstrap draws the same groups in every process.
    keys = sorted(set.intersection(*(set(scored(v, sets)) for v in variants)))
    groups = collections.defaultdict(list)
    for k in keys:
        groups[by['base'][k]['gkey']].append(k)
    gl = [groups[g] for g in sorted(groups)]
    npos = sum(labels[k[1]] for k in keys)
    print(f"\n## {name}: {len(keys)} units ({npos} relevant), {len(gl)} prompt groups")
    print(f"{'variant':11} {'AUC':>6} {'95% CI':>15} {'ΔAUC vs base [95% CI]':>28} {'prec':>5} {'rec':>5} {'block%':>6}")
    boots = {v: [] for v in variants}
    prb = {v: [] for v in variants}
    for _ in range(2000):
        sample = [k for g in random.choices(gl, k=len(gl)) for k in g]
        for v in variants:
            boots[v].append(auc([(by[v][k]['p_yes'], labels[k[1]]) for k in sample]))
            prb[v].append(prec_rec(v, sample))
    for v in variants:
        a = auc([(by[v][k]['p_yes'], labels[k[1]]) for k in keys])
        lo, hi = ci(boots[v])
        d = [x - y for x, y in zip(boots[v], boots['base'])]
        dlo, dhi = ci(d)
        delta = '' if v == 'base' else f"{a - auc([(by['base'][k]['p_yes'], labels[k[1]]) for k in keys]):+.3f} [{dlo:+.3f}, {dhi:+.3f}]"
        prec, rec, fp = prec_rec(v, keys)
        blocked = sum(by[v][k]['p_yes'] < THRESHOLD for k in keys)
        print(f"{v:11} {a:6.3f} [{lo:.3f}, {hi:.3f}] {delta:>28} {prec:5.2f} {rec:5.2f} {100 * blocked / len(keys):6.1f}")
    print(f"  at {THRESHOLD}, paired against base:")
    for v in variants[1:]:
        dp = sorted(x[0] - y[0] for x, y in zip(prb[v], prb['base']))
        dr = sorted(x[1] - y[1] for x, y in zip(prb[v], prb['base']))
        a, b = prec_rec(v, keys), prec_rec('base', keys)
        print(f"    {v:11} Δprecision {a[0] - b[0]:+.3f} [{dp[50]:+.3f}, {dp[1949]:+.3f}]  "
              f"Δrecall {a[1] - b[1]:+.3f} [{dr[50]:+.3f}, {dr[1949]:+.3f}]  wrongly passed {b[2]} → {a[2]}")
    print("  precision / recall by threshold:")
    for v in variants:
        print(f"    {v:11} " + "  ".join(f"{t}: {prec_rec(v, keys, t)[0]:.2f}/{prec_rec(v, keys, t)[1]:.2f}"
                                       for t in (0.2, 0.3, 0.5, 0.7)))

print("\n## cost and latency (one call per prompt group, 4 in parallel)")
print(f"{'variant':11} {'calls':>5} {'err':>4} {'p50 ms':>7} {'p95 ms':>7} {'out tok/cand':>12} {'in tok':>7} {'cache_r':>8}")
for v in variants:
    calls = {}
    for r in by[v].values():
        calls[r['gkey']] = r
    ms = sorted(c['ms'] for c in calls.values())
    errs = sum(1 for c in calls.values() if c['error'])
    ok = [c for c in calls.values() if c['usage']]
    per = statistics.mean(c['usage']['output_tokens'] / c['n'] for c in ok)
    inp = statistics.mean(c['usage']['input_tokens'] for c in ok)
    cr = statistics.mean(c['usage'].get('cache_read_input_tokens') or 0 for c in ok)
    print(f"{v:11} {len(calls):5} {errs:4} {ms[len(ms) // 2]:7.0f} {ms[int(.95 * len(ms))]:7.0f} {per:12.1f} {inp:7.0f} {cr:8.0f}")

if 'structured' in by:
    print("\n## structured: `match` field against labels")
    t = collections.Counter((r['match'], labels.get(r['id'])) for r in by['structured'].values() if r['match'])
    for m in ('direct', 'adjacent', 'none'):
        print(f"  {m:8} relevant {t[(m, 1)]:3}  not {t[(m, 0)]:3}")
