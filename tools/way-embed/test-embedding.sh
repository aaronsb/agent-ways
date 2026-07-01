#!/usr/bin/env bash
# Smoke test for way-embed: validates the embedding engine against key scenarios.
# Tests the claims from ADR-108 — stem collision disambiguation and false positive reduction.

set -euo pipefail

_C="${XDG_CACHE_HOME:-$HOME/.cache}"
if [[ -d "$_C/agent-ways/user" ]]; then XDG_WAY="$_C/agent-ways/user"
elif [[ -d "$_C/claude-ways/user" ]]; then XDG_WAY="$_C/claude-ways/user"
else XDG_WAY="$_C/agent-ways/user"; fi
if [[ -x "${XDG_WAY}/way-embed" ]]; then
  WAY_EMBED="${XDG_WAY}/way-embed"
elif [[ -x "${HOME}/.claude/bin/way-embed" ]]; then
  WAY_EMBED="${HOME}/.claude/bin/way-embed"
else
  WAY_EMBED=""
fi
# Use EN-only corpus — this test validates the English model against English ways.
# Multilingual matching is tested separately by test-multilingual.sh.
CORPUS="${XDG_WAY}/ways-corpus-en.jsonl"
MODEL="${XDG_WAY}/minilm-l6-v2.gguf"

pass=0; fail=0; skip=0

check() {
  local desc="$1" query="$2" expected="$3" should_match="$4"
  local result

  result=$("$WAY_EMBED" match --corpus "$CORPUS" --model "$MODEL" --query "$query" --threshold 0.0 2>/dev/null || true)

  if [[ "$should_match" == "yes" ]]; then
    if echo "$result" | grep -q "^${expected}	"; then
      local score
      score=$(echo "$result" | grep "^${expected}	" | cut -f2)
      printf "  PASS  %-60s → %s (%s)\n" "$desc" "$expected" "$score"
      pass=$((pass + 1))
    else
      local top
      top=$(echo "$result" | head -1)
      printf "  FAIL  %-60s → expected %s, got: %s\n" "$desc" "$expected" "${top:-nothing}"
      fail=$((fail + 1))
    fi
  else
    # Should NOT match: check that expected way is below runtime threshold (0.35)
    local score
    score=$(echo "$result" | grep "^${expected}	" | cut -f2 || echo "0.0000")
    if [[ -z "$score" ]] || awk "BEGIN{exit !($score < 0.35)}" 2>/dev/null; then
      printf "  PASS  %-60s → %s correctly below threshold (%s)\n" "$desc" "$expected" "${score:-absent}"
      pass=$((pass + 1))
    else
      printf "  FAIL  %-60s → %s unexpectedly high (%s)\n" "$desc" "$expected" "$score"
      fail=$((fail + 1))
    fi
  fi
}

# Preflight
if [[ ! -x "$WAY_EMBED" ]]; then echo "SKIP: way-embed binary not found"; exit 0; fi
if [[ ! -f "$MODEL" ]]; then echo "SKIP: model file not found at $MODEL"; exit 0; fi

# Check corpus has embeddings
if ! head -1 "$CORPUS" | grep -q '"embedding"'; then
  echo "SKIP: corpus has no embeddings (run generate-corpus.sh first)"; exit 0
fi

echo "=== ADR-108 Embedding Engine Tests ==="
echo ""

echo "--- Stem collision disambiguation (ADR-108 core claim) ---"
check "SSH agent (not AI agent)"         "ssh agent forwarding to remote server" "softwaredev/environment/ssh" "yes"
check "AI agent (not SSH agent)"         "build an AI agent simulation"          "meta/subagents"              "yes"
check "Document code (not write docs)"   "add docstrings to the module"          "documentation/docstrings" "yes"
check "Write a document (not docstrings)" "write a document about the project"   "writing"                     "yes"

echo ""
echo "--- False positive reduction (Dwarf Fortress test) ---"
check "DF: should not fire docs"         "write about Dwarf Fortress and AI agent simulation" "documentation"           "no"
check "DF: should not fire docstrings"   "write about Dwarf Fortress and AI agent simulation" "documentation/docstrings" "no"

echo ""
echo "--- True positives (basic recall) ---"
check "Unit testing"                     "add unit tests for the auth module"          "softwaredev/code/testing"            "yes"
check "SSH access"                       "ssh into the production server"              "softwaredev/environment/ssh"         "yes"
check "ADR + migration"                  "create a new ADR for the database migration" "documentation/adr"        "yes"
check "Commit messages"                  "write a conventional commit message"         "softwaredev/delivery/commits"        "yes"
check "Performance"                      "profile this function to find the bottleneck" "softwaredev/code/performance"       "yes"
check "Threat modeling"                  "perform a STRIDE analysis on the auth flow"  "softwaredev/architecture/threat-modeling" "yes"

echo ""
echo "--- True negatives (out-of-domain) ---"
check "Weather (no match expected)"      "what's the weather today"                    "writing"                             "no"
check "Joke (no match expected)"         "tell me a joke about programmers"            "meta/skills"                         "no"

echo ""
echo "--- Timing ---"
start_ns=$(date +%s%N 2>/dev/null || echo 0)
"$WAY_EMBED" match --corpus "$CORPUS" --model "$MODEL" --query "test prompt for timing" --threshold 0.3 2>/dev/null >/dev/null
end_ns=$(date +%s%N 2>/dev/null || echo 0)
if [[ "$start_ns" != "0" && "$end_ns" != "0" ]]; then
  elapsed_ms=$(( (end_ns - start_ns) / 1000000 ))
  echo "  Single match invocation: ${elapsed_ms}ms"
  if [[ $elapsed_ms -lt 100 ]]; then
    echo "  PASS: under 100ms budget"
    pass=$((pass + 1))
  else
    echo "  WARN: over 100ms budget (${elapsed_ms}ms)"
  fi
else
  echo "  (timing not available on this platform)"
fi

echo ""
echo "=== Results: $pass passed, $fail failed ==="
[[ $fail -eq 0 ]] && exit 0 || exit 1
