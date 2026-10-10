#!/usr/bin/env bash
# usage: gates.sh <worktree> <corpus-dir> <label>
W=$1; C=$2; L=$3
cd "$W" || exit 1
for f in tests/probes/tree-sample.tsv tests/probes/tree-sample-joined.tsv tests/probes/tree-sample-pleasantry.tsv; do
  ways author probe "$f" --ways-dir hooks/ways --corpus "$C" --tsv > "/tmp/kgtopo-a3601/$L-$(basename $f .tsv).tsv" 2>/dev/null
done
for f in tests/probes/unrelated-*.tsv; do
  echo "== $f"; ways author probe "$f" --ways-dir hooks/ways --corpus "$C" --unrelated 2>&1 | grep -E "^unrelated:|fired" | head -5
done
python3 /tmp/kgtopo-a3601/summ.py "$L" | grep -v unrelated
