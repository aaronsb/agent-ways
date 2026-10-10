import csv, sys
a, b = sys.argv[1], sys.argv[2]
for name in ['tree-sample', 'tree-sample-joined', 'tree-sample-pleasantry']:
    def load(l):
        rows = [r for r in csv.DictReader(open(f'/tmp/kgtopo-a3601/{l}-{name}.tsv'), delimiter='\t') if r.get('stage')]
        return {(r['expected_way'], r['kind'], r['prompt']): r for r in rows}
    A, B = load(a), load(b)
    print('==', name)
    for k in A:
        ra, rb = A[k], B.get(k)
        if rb is None:
            continue
        if ra['pass'] != rb['pass'] or (ra['rank'] == '1') != (rb['rank'] == '1'):
            print(f"{k[0]} {k[1]} pass {ra['pass']}->{rb['pass']} rank {ra['rank']}->{rb['rank']} sib {ra['sibling_over']}->{rb['sibling_over']} stage {ra['stage']}->{rb['stage']} share {ra['share']}->{rb['share']}")
            print('   ', k[2][:110])
