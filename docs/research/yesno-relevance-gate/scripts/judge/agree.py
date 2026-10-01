#!/usr/bin/env python3
"""Max |p_yes| difference and answer flips between a reference and candidate result files (paired by id)."""
import json, sys
def load(fn): return {r["id"]: r for r in map(json.loads, open(fn))}
ref, cands = sys.argv[1::2], sys.argv[2::2]
pairs = []
for a, b in zip(ref, cands):
    A, B = load(a), load(b)
    pairs += [(A[k]["p_yes"], B[k]["p_yes"]) for k in A]
d = [abs(x - y) for x, y in pairs]
flips = sum((x >= .5) != (y >= .5) for x, y in pairs)
print(f"n={len(pairs)} max_diff={max(d):.5f} mean_diff={sum(d)/len(d):.6f} answer_flips={flips}")
