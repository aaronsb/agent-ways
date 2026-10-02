#!/usr/bin/env bash
# Multilingual way matching test — validates locale stubs route correctly.
# Runs each prompt through `ways author match --json` and checks expected way is top-N.
#
# Usage: tests/test-multilingual.sh [--verbose]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WAYS_BIN="${SCRIPT_DIR}/../bin/ways"

VERBOSE=false
[[ "${1:-}" == "--verbose" ]] && VERBOSE=true

if [[ ! -x "$WAYS_BIN" ]]; then
  echo "ERROR: ways binary not found at $WAYS_BIN (run 'make setup')"
  exit 1
fi

# Dormant per ADR-139: localization data is adopter-generated via ways-localize.
# With no locale stubs present there is nothing to validate — skip cleanly so
# run-all.sh and manual invocation stay green until a language is localized.
if [[ -z "$(find "$SCRIPT_DIR/../hooks/ways" -name '*.locales.jsonl' -print -quit 2>/dev/null)" ]]; then
  echo "SKIP: no locale data present — multilingual matching is dormant (ADR-139)"
  exit 0
fi

PASS=0
FAIL=0
TOTAL=0

check() {
  local lang="$1" expected="$2" prompt="$3" rank_limit="${4:-3}"
  TOTAL=$((TOTAL + 1))

  # Get matches (stderr separated), strip ANSI, skip header lines
  local raw output
  raw=$("$WAYS_BIN" author match "$prompt" 2>/dev/null) || true
  output=$(echo "$raw" | sed 's/\x1b\[[0-9;]*m//g' | awk 'NR>3 && NF>0 {if(++n<=20)print}')

  if [[ -z "$output" ]]; then
    echo "  FAIL [$lang] no matches above threshold"
    FAIL=$((FAIL + 1))
    return
  fi

  # Parse: each line is "  way_id  score  description..."
  # Find rank of expected way in output
  local rank=0 line_num=0 match_score=""
  local top3=""
  while IFS= read -r line; do
    # Extract way ID (first non-space field)
    local way_id
    way_id=$(echo "$line" | awk '{print $1}')
    [[ -z "$way_id" ]] && continue
    line_num=$((line_num + 1))

    if [[ $line_num -le 3 ]]; then
      [[ -n "$top3" ]] && top3="$top3, "
      top3="$top3$way_id"
    fi

    if [[ "$way_id" == *"$expected"* && $rank -eq 0 ]]; then
      rank=$line_num
      match_score=$(echo "$line" | awk '{print $2}')
    fi
  done <<< "$output"

  if $VERBOSE; then
    echo "  query: \"$prompt\""
  fi

  if [[ "$rank" -ge 1 && "$rank" -le "$rank_limit" ]]; then
    PASS=$((PASS + 1))
    if $VERBOSE; then
      echo "  PASS [$lang] $expected (rank #$rank, score $match_score)"
      # Show top 5 matches
      local n=0
      while IFS= read -r line; do
        local wid wscore
        wid=$(echo "$line" | awk '{print $1}')
        wscore=$(echo "$line" | awk '{print $2}')
        [[ -z "$wid" ]] && continue
        n=$((n + 1))
        [[ $n -gt 5 ]] && break
        local marker="  "
        [[ "$wid" == *"$expected"* ]] && marker=">>"
        echo "    $marker #$n $wid  $wscore"
      done <<< "$output"
      echo ""
    else
      echo "  PASS [$lang] $expected (#$rank)"
    fi
  else
    FAIL=$((FAIL + 1))
    echo "  FAIL [$lang] expected $expected in top $rank_limit, got: $top3"
    if $VERBOSE; then
      local n=0
      while IFS= read -r line; do
        local wid wscore
        wid=$(echo "$line" | awk '{print $1}')
        wscore=$(echo "$line" | awk '{print $2}')
        [[ -z "$wid" ]] && continue
        n=$((n + 1))
        [[ $n -gt 5 ]] && break
        echo "       #$n $wid  $wscore"
      done <<< "$output"
      echo ""
    fi
  fi
}

echo "Multilingual Way Matching Test"
echo "=============================="
echo ""

echo "Part A: Latin-script languages"
check "de" "architecture" \
  "ADR Architekturentscheidung Entwurf dokumentieren Muster"
check "es" "code/testing" \
  "necesito escribir pruebas unitarias para este modulo"
check "fr" "environment/deps" \
  "il faut installer les dependances et mettre a jour les paquets"
check "pt-br" "delivery/github" \
  "pull request GitHub review merge branch erstellen"
check "it" "code/testing" \
  "devo scrivere test unitari con mock per il livello database"
check "pl" "environment/debugging" \
  "muszę zdebugować ten błąd i sprawdzić stack trace"
check "vi" "delivery/commits" \
  "tôi cần commit các thay đổi và push lên remote"
check "tr" "code/quality" \
  "kodu refactor etmem ve kaliteyi artırmam gerekiyor"
echo ""

echo "Part B: CJK languages (require embedding engine)"
check "ja" "environment/debugging" \
  "デバッグ バグ 調査 スタックトレース breakpoint" 5
check "ko" "code/security" \
  "코드 보안 검토가 필요합니다, 취약점을 확인해야 합니다"
check "zh" "code/performance" \
  "优化 性能 瓶颈 延迟 profile benchmark" 5
echo ""

echo "Part C: Cyrillic"
check "ru" "delivery/commits" \
  "нужно сделать коммит с правильным сообщением и запушить" 15
check "uk" "softwaredev/environment" \
  "потрібно налаштувати середовище розробки та встановити залежності"
echo ""

echo "Part D: Arabic script"
check "ar" "ea/tasks" \
  "أحتاج إلى إدارة المهام ومتابعة بنود العمل المعلقة"
echo ""

echo "Part E: Thai and Devanagari"
check "th" "docs" \
  "ต้องเขียนเอกสารประกอบโค้ดและคู่มือเริ่มต้นใช้งาน"
check "hi" "code/supplychain" \
  "इस रिपॉजिटरी की सप्लाई चेन सुरक्षा ऑडिट करनी है"
echo ""

echo "Part F: Cross-language consistency"
# Same concept in English and Spanish — both should hit testing
check "en" "code/testing" \
  "I need to write unit tests with mocks for the database layer"
check "es" "code/testing" \
  "necesito escribir pruebas unitarias con mocks para la capa de base de datos"
echo ""

echo "=============================="
echo "Results: $PASS/$TOTAL passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
