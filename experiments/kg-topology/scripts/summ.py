import csv, sys, glob
label = sys.argv[1]
for f in sorted(glob.glob(f'/tmp/kgtopo-a3601/{label}-*.tsv')):
    rows = list(csv.DictReader(open(f), delimiter='\t'))
    name = f.split('/')[-1]
    if rows and 'pass' in rows[0]:
        sc = [r for r in rows if r.get('stage') and not r['stage'].startswith('skipped')]
        print(name, len(sc), 'pass', sum(r['pass'] == 'true' for r in sc), 'top1', sum(r['rank'] == '1' for r in sc))
    else:
        keys = list(rows[0].keys()) if rows else []
        fired = [r for r in rows if r.get('fires') == 'true' or r.get('fired') == 'true']
        print(name, len(rows), 'fired', len(fired), keys if not fired and 'fires' not in keys else '')
