"""Parse the authored graph: frontmatter + See Also + parent/child for each ingested way."""
import json, os, re, sys
W = sys.argv[1]  # worktree
ROOT = os.path.join(W, 'hooks/ways')
docs = json.load(open('/tmp/kgtopo-a3601/docs.json'))


def way_of(name):
    return name[:-3].replace('__', '/')


def file_of(way):
    return os.path.join(ROOT, way, way.split('/')[-1] + '.md')


def parse(way):
    t = open(file_of(way)).read()
    fm = {}
    if t.startswith('---'):
        end = t.index('\n---', 3)
        for line in t[3:end].splitlines():
            m = re.match(r'^([a-z_]+):\s?(.*)$', line)
            if m:
                fm[m.group(1)] = m.group(2)
    see = []
    m = re.search(r'^## See Also\s*$(.*?)(?=^## |\Z)', t, re.M | re.S)
    if m:
        for ref, dom in re.findall(r'^- ([\w./-]+)\((\w[\w-]*)\)', m.group(1), re.M):
            see.append(ref if (ref == dom or ref.startswith(dom + '/')) else f'{dom}/{ref}')
    return fm, see


out = {}
for d in docs:
    w = way_of(d['name'])
    fm, see = parse(w)
    out[w] = {'description': fm.get('description', ''), 'vocabulary': fm.get('vocabulary', ''),
              'pattern': fm.get('pattern', ''), 'see_also': see,
              'parent': '/'.join(w.split('/')[:-1]) or None}
json.dump(out, open('/tmp/kgtopo-a3601/authored.json', 'w'), indent=0)
ways = set(out)
bad = sorted({s for v in out.values() for s in v['see_also'] if not os.path.exists(file_of(s))})
print(len(out), 'see-also refs', sum(len(v['see_also']) for v in out.values()), 'unresolved', bad)
