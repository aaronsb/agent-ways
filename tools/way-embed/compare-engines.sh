#!/usr/bin/env bash
# Head-to-head comparison: BM25 vs Embedding engine
# Runs test fixtures through both scorers, reports accuracy, timing, and disagreements.

set -euo pipefail

_C="${XDG_CACHE_HOME:-$HOME/.cache}"
if [[ -d "$_C/agent-ways/user" ]]; then XDG_WAY="$_C/agent-ways/user"
elif [[ -d "$_C/claude-ways/user" ]]; then XDG_WAY="$_C/claude-ways/user"
else XDG_WAY="$_C/agent-ways/user"; fi
WAY_EMBED="${XDG_WAY}/way-embed"
[[ ! -x "$WAY_EMBED" ]] && WAY_EMBED="${HOME}/.claude/bin/way-embed"
WAY_MATCH="${HOME}/.claude/bin/way-match"
CORPUS="${XDG_WAY}/ways-corpus.jsonl"
MODEL="${XDG_WAY}/minilm-l6-v2.gguf"
FIXTURES="${HOME}/.claude/tools/way-match/test-fixtures.jsonl"

# Preflight
for bin in "$WAY_EMBED" "$WAY_MATCH"; do
  [[ -x "$bin" ]] || { echo "error: $bin not found" >&2; exit 1; }
done
[[ -f "$MODEL" ]] || { echo "error: model not found at $MODEL" >&2; exit 1; }

bm25_tp=0; bm25_fp=0; bm25_fn=0; bm25_tn=0
emb_tp=0; emb_fp=0; emb_fn=0; emb_tn=0
agree=0; disagree=0; total=0

printf "%-50s  %-8s %-8s  %s\n" "Prompt" "BM25" "Embed" "Verdict"
printf "%-50s  %-8s %-8s  %s\n" "------" "----" "-----" "-------"

while IFS= read -r line; do
  echo "$line" | grep -q '"expected": *\[' && continue  # skip coactivation

  prompt=$(echo "$line" | sed 's/.*"prompt": *"\([^"]*\)".*/\1/')
  expected=$(echo "$line" | sed 's/.*"expected": *"\([^"]*\)".*/\1/')
  should_match=$(echo "$line" | sed 's/.*"match": *\(true\|false\).*/\1/')

  if echo "$line" | grep -q '"expected": *null'; then
    expected="null"
  fi

  # BM25: score mode, check if expected way appears
  bm25_results=$("$WAY_MATCH" score --corpus "$CORPUS" --query "$prompt" 2>/dev/null || true)
  bm25_hit=$(echo "$bm25_results" | grep "^${expected}	" | head -1 || true)

  # Embedding: match mode at threshold 0.0
  emb_results=$("$WAY_EMBED" match --corpus "$CORPUS" --model "$MODEL" --query "$prompt" --threshold 0.0 2>/dev/null || true)
  emb_hit=$(echo "$emb_results" | grep "^${expected}	" | head -1 || true)

  if [[ "$should_match" == "true" ]]; then
    # TP if expected way found, FN if not
    [[ -n "$bm25_hit" ]] && bm25_tp=$((bm25_tp+1)) || bm25_fn=$((bm25_fn+1))
    [[ -n "$emb_hit" ]] && emb_tp=$((emb_tp+1)) || emb_fn=$((emb_fn+1))
    bm25_mark=$([[ -n "$bm25_hit" ]] && echo "TP" || echo "FN")
    emb_mark=$([[ -n "$emb_hit" ]] && echo "TP" || echo "FN")
  else
    # For negatives: check top score against typical threshold
    bm25_top_score=$(echo "$bm25_results" | head -1 | cut -f2 || echo "0")
    emb_top_score=$(echo "$emb_results" | head -1 | cut -f2 || echo "0")

    bm25_neg=$(awk "BEGIN{print ($bm25_top_score < 2.0) ? 1 : 0}" 2>/dev/null || echo 1)
    emb_neg=$(awk "BEGIN{print ($emb_top_score < 0.35) ? 1 : 0}" 2>/dev/null || echo 1)

    [[ "$bm25_neg" == "1" ]] && { bm25_tn=$((bm25_tn+1)); bm25_mark="TN"; } || { bm25_fp=$((bm25_fp+1)); bm25_mark="FP"; }
    [[ "$emb_neg" == "1" ]] && { emb_tn=$((emb_tn+1)); emb_mark="TN"; } || { emb_fp=$((emb_fp+1)); emb_mark="FP"; }
  fi

  if [[ "$bm25_mark" == "$emb_mark" ]]; then
    agree=$((agree+1))
    verdict="agree"
  else
    disagree=$((disagree+1))
    verdict="DISAGREE: BM25=$bm25_mark Embed=$emb_mark"
  fi

  printf "%-50s  %-8s %-8s  %s\n" "${prompt:0:50}" "$bm25_mark" "$emb_mark" "$verdict"
  total=$((total+1))

done < "$FIXTURES"

echo ""
echo "=== Accuracy ==="
bm25_correct=$((bm25_tp + bm25_tn))
emb_correct=$((emb_tp + emb_tn))
printf "BM25:      TP=%d FP=%d TN=%d FN=%d  Accuracy: %d/%d\n" $bm25_tp $bm25_fp $bm25_tn $bm25_fn $bm25_correct $total
printf "Embedding: TP=%d FP=%d TN=%d FN=%d  Accuracy: %d/%d\n" $emb_tp $emb_fp $emb_tn $emb_fn $emb_correct $total
echo ""
echo "Agreement: $agree/$total  Disagreements: $disagree"

echo ""
echo "=== Timing (10 iterations) ==="

# BM25 timing: score all ways in one call
bm25_start=$(date +%s%N)
for i in $(seq 1 10); do
  "$WAY_MATCH" score --corpus "$CORPUS" --query "add unit tests for the auth module" >/dev/null 2>&1
done
bm25_end=$(date +%s%N)
bm25_avg=$(( (bm25_end - bm25_start) / 10000000 ))

# Embedding timing
emb_start=$(date +%s%N)
for i in $(seq 1 10); do
  "$WAY_EMBED" match --corpus "$CORPUS" --model "$MODEL" --query "add unit tests for the auth module" --threshold 0.35 >/dev/null 2>&1
done
emb_end=$(date +%s%N)
emb_avg=$(( (emb_end - emb_start) / 10000000 ))

echo "BM25  (score mode, 1 spawn):  ${bm25_avg}ms avg"
echo "Embed (match mode, 1 spawn):  ${emb_avg}ms avg"
