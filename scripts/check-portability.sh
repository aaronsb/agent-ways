#!/usr/bin/env bash
# Cross-platform portability lint.
#
# Catches the classes of OS-specific regression this repo has hit before:
#   1. CRLF line endings on files that bash/cargo expect to be LF.
#   2. Non-portable shebangs (`#!/bin/bash` instead of `#!/usr/bin/env bash`).
#   3. Hardcoded absolute home paths (`/home/<user>/...`) in code/config.
#
# Runs in the pre-commit hook and in CI on Windows + macOS + Linux. Vendored
# third-party trees (llama.cpp) and generated build output are excluded.
#
# Exit code: 0 = clean, 1 = one or more failures. Bare `/tmp` literals in shell
# are reported for review but do NOT fail (legitimate Unix-branch fallbacks
# exist alongside their Windows counterparts).

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT" || exit 2

FAIL=0
note()  { printf '  %s\n' "$1"; }
fail()  { printf '\033[0;31m[FAIL] %s\033[0m\n' "$1"; FAIL=1; }
ok()    { printf '\033[0;32m[ ok ] %s\033[0m\n' "$1"; }

# Files we never lint (vendored / generated).
is_excluded() {
  case "$1" in
    */llama.cpp/*|*/build/*|tools/target/*) return 0 ;;
    *) return 1 ;;
  esac
}

# List tracked files of interest, honoring exclusions.
tracked() { git ls-files "$@"; }

# ── 1. CRLF on LF-only files ─────────────────────────────────────
echo "[1/3] line endings"
crlf_hits=0
while IFS= read -r f; do
  is_excluded "$f" && continue
  [ -f "$f" ] || continue
  if grep -lU $'\r' "$f" >/dev/null 2>&1; then
    fail "CRLF line endings: $f"
    crlf_hits=$((crlf_hits + 1))
  fi
done < <(tracked '*.sh' '*.bash' '*.rs' '*.toml' '*.cpp' '*.h' '*.hpp' '*.c' 'Makefile' '*.mk' 'hooks/pre-commit')
[ "$crlf_hits" -eq 0 ] && ok "no CRLF on LF-only files"

# ── 2. Non-portable shebangs ─────────────────────────────────────
echo "[2/3] shebangs"
shebang_hits=0
while IFS= read -r f; do
  is_excluded "$f" && continue
  [ -f "$f" ] || continue
  first="$(head -1 "$f" 2>/dev/null | tr -d '\r\0')"
  case "$first" in
    "#!/bin/bash"|"#!/bin/sh")
      fail "non-portable shebang ($first): $f — use #!/usr/bin/env bash"
      shebang_hits=$((shebang_hits + 1))
      ;;
  esac
done < <(tracked '*.sh' 'hooks/pre-commit' 'scripts/project-pulse')
[ "$shebang_hits" -eq 0 ] && ok "all shebangs portable"

# ── 3. Hardcoded absolute home paths ─────────────────────────────
echo "[3/3] hardcoded home paths"
# Only lint files that are EXECUTED/READ at runtime with real paths: shell
# scripts, Makefiles, and settings.json. Rust sources are excluded — their
# `/home/...` occurrences are hermetic test fixtures and doc comments, not
# real filesystem reads, and flagging them is pure noise.
home_hits=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  fail "hardcoded home path: $line"
  home_hits=$((home_hits + 1))
done < <(
  tracked '*.sh' '*.bash' 'Makefile' '*.mk' 'settings.json' \
    | grep -vE '(/llama\.cpp/|/build/)' \
    | while IFS= read -r f; do is_excluded "$f" && continue; printf '%s\n' "$f"; done \
    | xargs -r grep -nE '/home/[a-z_][a-z0-9_-]*/|/Users/[A-Za-z][A-Za-z0-9 _-]*/' 2>/dev/null
)
[ "$home_hits" -eq 0 ] && ok "no hardcoded home paths in executed code/config"

# ── Informational: bare /tmp in shell (non-failing) ──────────────
echo "[i] bare /tmp in shell scripts (review — Unix fallbacks are expected):"
tracked '*.sh' 'hooks/pre-commit' \
  | grep -vE '(/llama\.cpp/|/build/)' \
  | xargs -r grep -nE '(^|[^A-Za-z0-9_])/tmp/' 2>/dev/null \
  | sed 's/^/  /' || true

echo
if [ "$FAIL" -ne 0 ]; then
  printf '\033[0;31mPortability check FAILED.\033[0m\n'
  exit 1
fi
printf '\033[0;32mPortability check passed.\033[0m\n'
