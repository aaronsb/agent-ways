#!/usr/bin/env bash
# Calibrate embedding thresholds against test fixtures
#
# Runs each test fixture through way-embed match at threshold 0.0,
# then analyzes score distributions to find optimal thresholds.

set -euo pipefail

_C="${XDG_CACHE_HOME:-$HOME/.cache}"
if [[ -d "$_C/agent-ways/user" ]]; then XDG_WAY="$_C/agent-ways/user"
elif [[ -d "$_C/claude-ways/user" ]]; then XDG_WAY="$_C/claude-ways/user"
else XDG_WAY="$_C/agent-ways/user"; fi
WAY_EMBED="${XDG_WAY}/way-embed"
[[ ! -x "$WAY_EMBED" ]] && WAY_EMBED="${HOME}/.claude/bin/way-embed"
CORPUS="${XDG_WAY}/ways-corpus.jsonl"
MODEL="${XDG_WAY}/minilm-l6-v2.gguf"
FIXTURES="${HOME}/.claude/tools/way-match/test-fixtures.jsonl"

if [[ ! -x "$WAY_EMBED" ]]; then echo "error: way-embed not found at $WAY_EMBED" >&2; exit 1; fi
if [[ ! -f "$MODEL" ]]; then echo "error: model not found at $MODEL" >&2; exit 1; fi
if [[ ! -f "$CORPUS" ]]; then echo "error: corpus not found at $CORPUS" >&2; exit 1; fi

TP=0; FP=0; TN=0; FN=0
total=0

echo "=== Embedding Threshold Calibration ==="
echo ""
echo "Format: [category] prompt → expected | top_match (score) | verdict"
echo ""

while IFS= read -r line; do
    prompt=$(echo "$line" | sed 's/.*"prompt": *"\([^"]*\)".*/\1/')
    expected=$(echo "$line" | sed 's/.*"expected": *"\([^"]*\)".*/\1/')
    should_match=$(echo "$line" | sed 's/.*"match": *\(true\|false\).*/\1/')
    category=$(echo "$line" | sed 's/.*"category": *"\([^"]*\)".*/\1/')

    # Handle null expected (negative cases)
    if echo "$line" | grep -q '"expected": *null'; then
        expected="null"
    fi

    # Handle array expected (coactivation cases)
    if echo "$line" | grep -q '"expected": *\['; then
        expected=$(echo "$line" | sed 's/.*"expected": *\[\([^]]*\)\].*/\1/' | tr -d '"' | tr ',' ' ')
    fi

    # Get all matches at threshold 0.0
    all_scores=$("$WAY_EMBED" match --corpus "$CORPUS" --model "$MODEL" --query "$prompt" --threshold 0.0 2>/dev/null)

    # Get top match
    top_id=$(echo "$all_scores" | head -1 | cut -f1)
    top_score=$(echo "$all_scores" | head -1 | cut -f2)

    # Check if expected way is in results
    if [[ "$should_match" == "true" ]]; then
        if [[ "$expected" == *" "* ]]; then
            # Coactivation: check each expected way
            all_found=true
            found_list=""
            for exp in $expected; do
                match_score=$(echo "$all_scores" | grep "^${exp}	" | cut -f2 || true)
                if [[ -n "$match_score" ]]; then
                    found_list+=" ${exp}(${match_score})"
                else
                    all_found=false
                    found_list+=" ${exp}(MISS)"
                fi
            done
            if $all_found; then
                verdict="TP"
                TP=$((TP + 1))
            else
                verdict="FN"
                FN=$((FN + 1))
            fi
            printf "[%-12s] %-55s → %s | %s | %s\n" "$category" "$prompt" "$expected" "$found_list" "$verdict"
        else
            match_score=$(echo "$all_scores" | grep "^${expected}	" | cut -f2 || true)
            if [[ -n "$match_score" ]]; then
                verdict="TP"
                TP=$((TP + 1))
                printf "[%-12s] %-55s → %-35s | %-20s (%.4f) | %s\n" "$category" "$prompt" "$expected" "$top_id" "${top_score:-0}" "$verdict"
            else
                verdict="FN"
                FN=$((FN + 1))
                printf "[%-12s] %-55s → %-35s | %-20s (%.4f) | %s\n" "$category" "$prompt" "$expected" "$top_id" "${top_score:-0}" "$verdict"
            fi
        fi
    else
        # Negative case: should not match anything
        if [[ -z "$top_id" || -z "$top_score" ]]; then
            verdict="TN"
            TN=$((TN + 1))
            printf "[%-12s] %-55s → (none expected) | (no match) | %s\n" "$category" "$prompt" "$verdict"
        else
            # Check score — below a threshold is effectively TN
            printf "[%-12s] %-55s → (none expected) | %-20s (%.4f) | (score check)\n" "$category" "$prompt" "$top_id" "$top_score"
            TN=$((TN + 1))  # Will recalibrate based on threshold
        fi
    fi

    total=$((total + 1))

done < "$FIXTURES"

echo ""
echo "=== Summary ==="
echo "Total: $total  TP: $TP  FP: $FP  TN: $TN  FN: $FN"
echo ""

# Now dump all scores for positive test cases to find threshold
echo "=== Score Distribution (positive cases, expected way's score) ==="
while IFS= read -r line; do
    should_match=$(echo "$line" | sed 's/.*"match": *\(true\|false\).*/\1/')
    [[ "$should_match" != "true" ]] && continue

    prompt=$(echo "$line" | sed 's/.*"prompt": *"\([^"]*\)".*/\1/')
    expected=$(echo "$line" | sed 's/.*"expected": *"\([^"]*\)".*/\1/')
    category=$(echo "$line" | sed 's/.*"category": *"\([^"]*\)".*/\1/')

    # Skip coactivation for simplicity
    echo "$line" | grep -q '"expected": *\[' && continue

    all_scores=$("$WAY_EMBED" match --corpus "$CORPUS" --model "$MODEL" --query "$prompt" --threshold 0.0 2>/dev/null)
    match_score=$(echo "$all_scores" | grep "^${expected}	" | cut -f2 || echo "MISS")

    printf "%s\t%s\t%s\n" "$match_score" "$category" "$prompt"
done < "$FIXTURES" | sort -t$'\t' -k1 -n

echo ""
echo "=== Score Distribution (negative cases, top score) ==="
while IFS= read -r line; do
    should_match=$(echo "$line" | sed 's/.*"match": *\(true\|false\).*/\1/')
    [[ "$should_match" != "false" ]] && continue

    prompt=$(echo "$line" | sed 's/.*"prompt": *"\([^"]*\)".*/\1/')
    all_scores=$("$WAY_EMBED" match --corpus "$CORPUS" --model "$MODEL" --query "$prompt" --threshold 0.0 2>/dev/null)
    top_score=$(echo "$all_scores" | head -1 | cut -f2)
    top_id=$(echo "$all_scores" | head -1 | cut -f1)

    printf "%s\t%s\t%s\n" "${top_score:-0.0000}" "$top_id" "$prompt"
done < "$FIXTURES" | sort -t$'\t' -k1 -rn
