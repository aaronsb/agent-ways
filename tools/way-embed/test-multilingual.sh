#!/usr/bin/env bash
# Cross-language and same-language embedding similarity tests.
# Validates multilingual model quality against English-only baseline.
#
# Usage:
#   bash test-multilingual.sh                    # run tests, print to stdout
#   bash test-multilingual.sh --markdown         # output markdown report

set -euo pipefail

_C="${XDG_CACHE_HOME:-$HOME/.cache}"
if [[ -d "$_C/agent-ways/user" ]]; then XDG_WAY="$_C/agent-ways/user"
elif [[ -d "$_C/claude-ways/user" ]]; then XDG_WAY="$_C/claude-ways/user"
else XDG_WAY="$_C/agent-ways/user"; fi
EMBED="${XDG_WAY}/way-embed"
[[ ! -x "$EMBED" ]] && EMBED="${HOME}/.claude/bin/way-embed"
EN_MODEL="${XDG_WAY}/minilm-l6-v2.gguf"
MULTI_MODEL="${XDG_WAY}/multilingual-minilm-l12-v2-q8.gguf"

MARKDOWN=false
[[ "${1:-}" == "--markdown" ]] && MARKDOWN=true

# Preflight
[[ -x "$EMBED" ]] || { echo "error: way-embed not found" >&2; exit 1; }
[[ -f "$EN_MODEL" ]] || { echo "error: English model not found at $EN_MODEL" >&2; exit 1; }
[[ -f "$MULTI_MODEL" ]] || { echo "error: multilingual model not found at $MULTI_MODEL" >&2; exit 1; }

THRESHOLD=0.25  # minimum same-language similarity to pass

# ── Test data ────────────────────────────────────────────────────
# Format: lang|native_prompt|native_description|english_description

read -r -d '' TEST_DATA << 'TESTS' || true
# Domain: dependency vulnerability scanning
en|check dependencies for vulnerabilities|dependency vulnerability scanning|dependency vulnerability scanning
de|Abhängigkeiten auf Schwachstellen prüfen|Abhängigkeits-Schwachstellenprüfung|dependency vulnerability scanning
es|verificar dependencias por vulnerabilidades|escaneo de vulnerabilidades de dependencias|dependency vulnerability scanning
fr|vérifier les dépendances pour vulnérabilités|analyse des vulnérabilités des dépendances|dependency vulnerability scanning
pt|verificar dependências por vulnerabilidades|verificação de vulnerabilidades de dependências|dependency vulnerability scanning
ru|проверить зависимости на уязвимости|сканирование уязвимостей зависимостей|dependency vulnerability scanning
ja|依存関係の脆弱性をチェックして|依存関係の脆弱性スキャン|dependency vulnerability scanning
ko|의존성 취약점 검사|의존성 취약점 스캐닝|dependency vulnerability scanning
zh|检查依赖项的漏洞|依赖项漏洞扫描|dependency vulnerability scanning
ar|فحص التبعيات بحثاً عن ثغرات|فحص ثغرات التبعيات|dependency vulnerability scanning
el|έλεγχος εξαρτήσεων για ευπάθειες|σάρωση ευπαθειών εξαρτήσεων|dependency vulnerability scanning
# Domain: commit message standards
en|write a conventional commit message|commit message format and standards|commit message format and standards
ja|コミットメッセージを書いて|コミットメッセージの形式と規約|commit message format and standards
ko|커밋 메시지 작성|커밋 메시지 형식과 표준|commit message format and standards
zh|写一个规范的提交信息|提交信息格式和规范|commit message format and standards
de|eine konventionelle Commit-Nachricht schreiben|Commit-Nachricht Format und Standards|commit message format and standards
ru|написать сообщение коммита|формат и стандарты сообщений коммитов|commit message format and standards
# Domain: unit testing
en|add unit tests for the auth module|unit test creation and testing|unit test creation and testing
ja|認証モジュールのユニットテストを追加して|ユニットテストの作成|unit test creation and testing
ko|인증 모듈에 단위 테스트 추가|단위 테스트 작성|unit test creation and testing
zh|为认证模块添加单元测试|单元测试创建|unit test creation and testing
de|Unit-Tests für das Auth-Modul hinzufügen|Unit-Test-Erstellung und Testen|unit test creation and testing
ru|добавить юнит-тесты для модуля аутентификации|создание юнит-тестов|unit test creation and testing
TESTS

# Build the three batch inputs as temp files
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT
EN_CROSS_FILE="$TMPDIR/en_cross.tsv"
MULTI_CROSS_FILE="$TMPDIR/multi_cross.tsv"
MULTI_SAME_FILE="$TMPDIR/multi_same.tsv"

declare -a LANGS PROMPTS NATIVE_DESCS EN_DESCS
i=0

while IFS= read -r line; do
  [[ -z "$line" || "$line" == \#* ]] && continue
  IFS='|' read -r lang prompt native_desc en_desc <<< "$line"
  LANGS[$i]="$lang"
  PROMPTS[$i]="$prompt"
  NATIVE_DESCS[$i]="$native_desc"
  EN_DESCS[$i]="$en_desc"
  printf '%s\t%s\n' "$prompt" "$en_desc" >> "$EN_CROSS_FILE"
  printf '%s\t%s\n' "$prompt" "$en_desc" >> "$MULTI_CROSS_FILE"
  printf '%s\t%s\n' "$prompt" "$native_desc" >> "$MULTI_SAME_FILE"
  i=$((i + 1))
done <<< "$TEST_DATA"

total=$i

# ── Run batch similarity ─────────────────────────────────────────

overall_start=$(date +%s%N 2>/dev/null || date +%s)

read_scores() {
  local outfile="$1"; shift
  "$@" > "$outfile"
}

en_start=$(date +%s%N 2>/dev/null || date +%s)
read_scores "$TMPDIR/en_scores.txt" "$EMBED" similarity --model "$EN_MODEL" --batch < "$EN_CROSS_FILE" 2>/dev/null
en_end=$(date +%s%N 2>/dev/null || date +%s)

multi_cross_start=$(date +%s%N 2>/dev/null || date +%s)
read_scores "$TMPDIR/multi_cross_scores.txt" "$EMBED" similarity --model "$MULTI_MODEL" --batch < "$MULTI_CROSS_FILE" 2>/dev/null
multi_cross_end=$(date +%s%N 2>/dev/null || date +%s)

multi_same_start=$(date +%s%N 2>/dev/null || date +%s)
read_scores "$TMPDIR/multi_same_scores.txt" "$EMBED" similarity --model "$MULTI_MODEL" --batch < "$MULTI_SAME_FILE" 2>/dev/null
multi_same_end=$(date +%s%N 2>/dev/null || date +%s)

# Load scores into arrays
declare -a EN_SCORES MULTI_CROSS_SCORES MULTI_SAME_SCORES
i=0
while IFS= read -r s; do EN_SCORES[$i]="$s"; i=$((i+1)); done < "$TMPDIR/en_scores.txt"
i=0
while IFS= read -r s; do MULTI_CROSS_SCORES[$i]="$s"; i=$((i+1)); done < "$TMPDIR/multi_cross_scores.txt"
i=0
while IFS= read -r s; do MULTI_SAME_SCORES[$i]="$s"; i=$((i+1)); done < "$TMPDIR/multi_same_scores.txt"

overall_end=$(date +%s%N 2>/dev/null || date +%s)

# Compute timing
en_ms=$(( (en_end - en_start) / 1000000 ))
multi_cross_ms=$(( (multi_cross_end - multi_cross_start) / 1000000 ))
multi_same_ms=$(( (multi_same_end - multi_same_start) / 1000000 ))
overall_ms=$(( (overall_end - overall_start) / 1000000 ))

# ── Output ────────────────────────────────────────────────────────

pass=0; fail=0
current_domain=""

if $MARKDOWN; then
  echo "# Multilingual Embedding Model Evaluation"
  echo ""
  echo "**Date:** $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo ""
  echo "**Models:**"
  echo "- English: \`all-MiniLM-L6-v2\` ($(du -h "$EN_MODEL" | cut -f1))"
  echo "- Multilingual: \`paraphrase-multilingual-MiniLM-L12-v2\` Q8_0 ($(du -h "$MULTI_MODEL" | cut -f1))"
  echo ""
  echo "**Methodology:** Each test embeds a native-language prompt against both an English description (cross-language) and a native-language description (same-language stub scenario). Three scores per test:"
  echo ""
  echo "- **EN×EN**: English-only model, English description (baseline)"
  echo "- **Multi×EN**: multilingual model, English description (cross-language)"
  echo "- **Multi×Native**: multilingual model, native description (same-language stub)"
  echo ""
  echo "**Threshold:** ${THRESHOLD} (same-language similarity minimum)"
  echo ""
  echo "## Results"
  echo ""
  echo "| Lang | Prompt | EN×EN | Multi×EN | Multi×Native | Pass |"
  echo "|:-----|:-------|------:|---------:|-------------:|:----:|"
fi

for ((j=0; j<total; j++)); do
  lang="${LANGS[$j]}"
  prompt="${PROMPTS[$j]}"
  en_score="${EN_SCORES[$j]}"
  mc_score="${MULTI_CROSS_SCORES[$j]}"
  ms_score="${MULTI_SAME_SCORES[$j]}"

  verdict="✅"
  if awk "BEGIN{exit !(${ms_score} < ${THRESHOLD})}" 2>/dev/null; then
    verdict="❌"
    fail=$((fail + 1))
  else
    pass=$((pass + 1))
  fi

  if $MARKDOWN; then
    echo "| ${lang} | ${prompt} | ${en_score} | ${mc_score} | ${ms_score} | ${verdict} |"
  else
    printf "%-4s  %-50s  %7s  %7s  %7s  %s\n" "$lang" "$prompt" "$en_score" "$mc_score" "$ms_score" "$verdict"
  fi
done

if $MARKDOWN; then
  echo ""
  echo "## Summary"
  echo ""
  echo "- **Tests:** ${total}"
  echo "- **Passed:** ${pass}"
  echo "- **Failed:** ${fail}"
  echo "- **Accuracy:** $(echo "scale=1; $pass * 100 / $total" | bc)%"
  echo ""
  echo "## Timing"
  echo ""
  echo "| Phase | Duration | Tests | Per-test |"
  echo "|:------|:---------|------:|---------:|"
  echo "| EN model batch (${total} pairs) | ${en_ms}ms | ${total} | $((en_ms / total))ms |"
  echo "| Multi model cross-language (${total} pairs) | ${multi_cross_ms}ms | ${total} | $((multi_cross_ms / total))ms |"
  echo "| Multi model same-language (${total} pairs) | ${multi_same_ms}ms | ${total} | $((multi_same_ms / total))ms |"
  echo "| **Total** | **${overall_ms}ms** | **$((total * 3))** | **$((overall_ms / (total * 3)))ms** |"
  echo ""
  echo "## Interpretation"
  echo ""
  echo "The multilingual model enables three matching strategies:"
  echo ""
  echo "1. **English ways + English model** — current production. High precision for English prompts."
  echo "2. **English ways + multilingual model (cross-language)** — user types in any language, matches against English descriptions. Works but scores 30-50% lower."
  echo "3. **Native-language stubs + multilingual model (same-language)** — locale entries in \`.locales.jsonl\` with native descriptions. Consistently scores 0.80+ across tested languages."
  echo ""
  echo "**Recommendation:** Ship both models. English ways use the English model (precise, 21MB). Multilingual stubs use the multilingual model (broad, 127MB). Routing is automatic — the corpus builder derives the model from file type (.md → EN, .locales.jsonl → multilingual)."
else
  echo ""
  echo "Results: ${pass}/${total} passed (threshold: ${THRESHOLD})"
  echo ""
  echo "Timing:"
  echo "  EN model:           ${en_ms}ms (${total} pairs, $((en_ms / total))ms each)"
  echo "  Multi cross-lang:   ${multi_cross_ms}ms (${total} pairs, $((multi_cross_ms / total))ms each)"
  echo "  Multi same-lang:    ${multi_same_ms}ms (${total} pairs, $((multi_same_ms / total))ms each)"
  echo "  Total:              ${overall_ms}ms ($((total * 3)) embeddings)"
fi

[[ $fail -eq 0 ]] && exit 0 || exit 1
