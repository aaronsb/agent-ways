"""Append proposed terms to each way's vocabulary: line. Usage: apply.py <worktree> [--dry] [--skip way,...] [--drop way:term,...]"""
import sys, re, os, json
sys.path.insert(0, '/tmp/kgtopo-a3601')
from proposal import P
W = sys.argv[1]
dry = '--dry' in sys.argv
skip = set()
drop = set()
for i, a in enumerate(sys.argv):
    if a == '--skip':
        skip = set(sys.argv[i + 1].split(','))
    if a == '--drop':
        drop = set(sys.argv[i + 1].split(','))
applied, novocab = {}, []
for way, terms in P.items():
    if way in skip:
        continue
    f = os.path.join(W, 'hooks/ways', way, way.split('/')[-1] + '.md')
    t = open(f).read()
    end = t.index('\n---', 3)
    fm, body = t[:end], t[end:]
    m = re.search(r'^vocabulary: ?(.*)$', fm, re.M)
    if not m:
        novocab.append(way)
        continue
    have = set(m.group(1).lower().split())
    add = [x for x in terms.split() if x.lower() not in have and f'{way}:{x}' not in drop]
    if not add:
        continue
    applied[way] = add
    newline = 'vocabulary: ' + (m.group(1).rstrip() + ' ' + ' '.join(add)).strip()
    fm = fm[:m.start()] + newline + fm[m.end():]
    if not dry:
        open(f, 'w').write(fm + body)
json.dump(applied, open('/tmp/kgtopo-a3601/applied.json', 'w'), indent=0)
print('ways touched', len(applied), 'terms', sum(len(v) for v in applied.values()))
print('no vocabulary line (skipped):', novocab)
