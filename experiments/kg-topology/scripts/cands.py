"""Vocabulary candidates per way: words from kg concept labels + search terms, absent from the frontmatter."""
import json, re, collections, math
T = '/tmp/kgtopo-a3601'
docs = json.load(open(T + '/docs.json'))
cons = json.load(open(T + '/concepts.json'))
auth = json.load(open(T + '/authored.json'))
cpw_path = '/home/aaron/Projects/ai/harness/agent-ways/.claude/worktrees/agent-a3601eccd1f79e81b/experiments/kg-topology/'
idx = json.load(open(cpw_path + 'concept-to-ways.json'))
STOP = set('''a an the and or of to in on for with by from at as is are be was were it its this that these those not no
never always only into over under than then when what which who how why where all any each every more most less least
very just also via per so if but do does did done can could should would will may might must one two three first second
new old way ways use using used make makes making get gets set sets run runs running work works working thing things
same other another own before after during while about against between without within across up down out off again
case cases kind kinds part parts point points step steps type types level levels rule rules need needs needed keep keeps
good bad right wrong real full whole small large big long short high low clear plain simple specific general common
time times place places name names person people user users agent agents claude model models session sessions task tasks
check checks checking change changes changed write writes writing read reads reading way's s vs e g etc i.e eg'''.split())
STOP |= set('''what's isn't don't doesn't can't won't i'm you're it's let's there here they them their our we you your
something nothing anything everything someone anyone instead rather already still even ever often'''.split())


def toks(s):
    return [w for w in re.findall(r"[a-z][a-z0-9+#.-]*[a-z0-9+#]|[a-z]", s.lower().replace('/', ' ')) if len(w) > 2]


def stem(w):
    for suf in ('ing', 'es', 's', 'ed'):
        if w.endswith(suf) and len(w) - len(suf) >= 4:
            return w[:-len(suf)]
    return w


way_words = {}
for key, ws in idx.items():
    pass
# concept text per way
w2text = collections.defaultdict(list)
for d in docs:
    pass
c2w = {}
for key, ws in idx.items():
    cid = 'sha256:' + key.rsplit('[', 1)[1][:-1]
    c2w[cid] = ws
for cid, ws in c2w.items():
    v = cons[cid]
    text = ' '.join([v['label']] + (v.get('search_terms') or []))
    for w in ws:
        w2text[w].append(text)
df = collections.Counter()
wcount = {}
for w, texts in w2text.items():
    c = collections.Counter()
    for t in texts:
        for s in set(stem(x) for x in toks(t)):
            c[s] += 1
    wcount[w] = c
    for s in c:
        df[s] += 1
surface = collections.defaultdict(collections.Counter)
for w, texts in w2text.items():
    for t in texts:
        for x in toks(t):
            surface[stem(x)][x] += 1
N = len(w2text)
out = {}
for w in sorted(w2text):
    fm = auth[w]
    have = set(stem(x) for x in toks(fm['description'] + ' ' + fm['vocabulary'] + ' ' + w.replace('-', ' ')))
    have |= set(x for x in toks(fm['vocabulary']))
    cands = []
    for s, n in wcount[w].items():
        sf = surface[s].most_common(1)[0][0]
        if s in have or sf in STOP or s in STOP or sf.isdigit():
            continue
        if df[s] > 6:
            continue
        cands.append((round(n * math.log(N / df[s]), 2), sf, n, df[s]))
    cands.sort(reverse=True)
    out[w] = cands[:14]
json.dump(out, open(T + '/cands.json', 'w'))
for w, c in out.items():
    print(w, '|', ' '.join(f'{sf}({n}/{d})' for _, sf, n, d in c))
