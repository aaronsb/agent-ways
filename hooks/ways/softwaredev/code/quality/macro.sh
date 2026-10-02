#!/usr/bin/env bash
# Scan for files exceeding quality thresholds
# Runs when quality way triggers - appends file list to way output

# Must be in a git repo
git rev-parse --is-inside-work-tree &>/dev/null || exit 0

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Find the way file in this directory (any .md with frontmatter)
WAY_FILE=""
for _f in "${SCRIPT_DIR}"/*.md; do
  [[ -f "$_f" ]] && [[ "$_f" != *.check.md ]] && head -1 "$_f" 2>/dev/null | grep -q '^---$' && { WAY_FILE="$_f"; break; }
done
[[ -z "$WAY_FILE" ]] && exit 0

# Read exclusion pattern from way frontmatter, or use default
DEFAULT_EXCLUDE='\.md$|\.lock$|\.min\.(js|css)$|\.generated\.|\.bundle\.|vendor/|node_modules/|dist/|build/|__pycache__/'
EXCLUDE_PATTERN=$(awk '/^scan_exclude:/{print $2; exit}' "$WAY_FILE" 2>/dev/null)
EXCLUDE_PATTERN="${EXCLUDE_PATTERN:-$DEFAULT_EXCLUDE}"

THRESHOLD=500
PRIORITY_THRESHOLD=800

# Collect files over threshold. The spawn count stays fixed whatever the
# repo size: one `git grep -c` counts the lines of every tracked file, and
# `file` and `wc` then run once each over the few files past the threshold.
# Spawning `file`, `grep` and `wc` per tracked file cost about 2,700 execs
# and 3.5 s on a 1,600-file repo, paid on each fire and each SubagentStart
# (#705). `git grep -c` counts an unterminated last line where `wc -l` does
# not, so it never drops a file `wc` would keep; `wc` gives the exact count.
# `git grep` reads assume-unchanged and skip-worktree files from the index,
# not from disk. core.quotePath=false keeps non-ASCII names unquoted, so
# they reach the scan; names with control characters stay quoted and skipped.
tracked=$(git -c core.quotePath=false ls-files 2>/dev/null | grep -Ev "$EXCLUDE_PATTERN")
[[ -z "$tracked" ]] && exit 0

# --no-recurse-submodules: a submodule is one gitlink path here, and
# submodule.recurse would otherwise scan its files past scan_exclude.
candidates=()
while IFS=$'\t' read -r f n; do
  ((n > THRESHOLD)) && [[ -f "$f" && -r "$f" ]] && candidates+=("$f")
done < <(printf '%s\n' "$tracked" | tr '\n' '\0' \
  | GIT_LITERAL_PATHSPECS=1 xargs -0 git -c grep.fullName=false \
      grep --no-recurse-submodules --no-color -c --null -e '' -- 2>/dev/null \
  | tr '\0' '\t')
((${#candidates[@]})) || exit 0

# `file -b` and `wc -l` both report in argument order, one line per file.
mimes=()
while IFS= read -r m; do mimes+=("$m"); done \
  < <(file -b --mime -- "${candidates[@]}" 2>/dev/null)
counts=()
while read -r n _; do counts+=("$n"); done \
  < <(wc -l -- "${candidates[@]}" 2>/dev/null)
# `wc` prints no line for a file it cannot read (one removed since the -f
# check, mid-checkout), which would pair later counts with the wrong files.
# It adds a `total` line for more than one file. On a mismatch, say nothing.
expect=${#candidates[@]}
((expect > 1)) && ((expect++))
((${#counts[@]} == expect)) || exit 0

results=$(for i in "${!candidates[@]}"; do
  # Skip binary files
  [[ "${mimes[$i]}" == *text/* ]] || continue
  ((counts[i] > THRESHOLD)) && printf "%5d  %s\n" "${counts[$i]}" "${candidates[$i]}"
done | sort -rn)

[[ -z "$results" ]] && exit 0

# Split into priority and review
priority=$(echo "$results" | awk -v t="$PRIORITY_THRESHOLD" '$1 > t')
review=$(echo "$results" | awk -v t="$PRIORITY_THRESHOLD" '$1 <= t')

echo ""
echo "## File Length Scan"

if [[ -n "$priority" ]]; then
  echo ""
  echo "**Priority (>${PRIORITY_THRESHOLD} lines):**"
  echo '```'
  echo "$priority" | head -10
  echo '```'
fi

if [[ -n "$review" ]]; then
  echo ""
  echo "**Review (>${THRESHOLD} lines):**"
  echo '```'
  echo "$review" | head -15
  echo '```'
fi
