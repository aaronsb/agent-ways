#!/usr/bin/env bash
# Freshness checks. Each one is silent on the happy path and emits at most a
# few lines when there is something worth a look.
#
#   doc_freshness  — README.md / docs/ whose git history lags far behind HEAD
#                    with no local branch carrying an update.
#   adr_lifecycle  — ADRs parked in Draft or Proposed under docs/architecture.
#
# Tunable: WAYS_FRESHNESS_COMMITS (default 25), how far HEAD may advance past
# the last doc-touching commit before doc_freshness fires.
#
# The same shape applies to other derived or descriptive artifacts (lockfile vs
# manifest, generated client vs schema). Add those as further functions here
# rather than as new ways.

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$PWD}"

# ── Documentation history vs HEAD ──────────────────────────────

doc_freshness() {
  git rev-parse --is-inside-work-tree &>/dev/null || return 0

  local THRESHOLD=${WAYS_FRESHNESS_COMMITS:-25}
  local PATHS=(README.md docs)

  # Most recent commit that touched any of PATHS.
  local last
  last=$(git log -1 --format=%H -- "${PATHS[@]}" 2>/dev/null)
  [[ -z "$last" ]] && return 0   # none of these paths are tracked here

  # How far has HEAD advanced since then?
  local behind
  behind=$(git rev-list --count "$last"..HEAD 2>/dev/null)
  [[ -z "$behind" || "$behind" -lt "$THRESHOLD" ]] && return 0   # keeping pace

  # Suppress if a local branch ahead of the current one already touches PATHS.
  # The update is in flight, so no nudge is needed.
  local cur b
  cur=$(git symbolic-ref --short HEAD 2>/dev/null)
  [[ -z "$cur" ]] && cur=HEAD
  while IFS= read -r b; do
    [[ -z "$b" || "$b" == "$cur" ]] && continue
    if [[ -n "$(git rev-list "$cur".."$b" -- "${PATHS[@]}" 2>/dev/null | head -1)" ]]; then
      return 0
    fi
  done < <(git for-each-ref --format='%(refname:short)' refs/heads 2>/dev/null)

  local ts months age=""
  ts=$(git log -1 --format=%ct "$last" 2>/dev/null)
  if [[ -n "$ts" ]]; then
    months=$(( ( $(date +%s) - ts ) / 2592000 ))
    age=" (~${months} mo)"
  fi
  echo "📄 **Freshness:** \`README.md\`/\`docs/\` last had a substantive commit ${behind} commits back${age}, and no local branch carries an update. Worth a pass — keeping in mind this flags the *abandoned* doc, not the *subtly wrong* one (stale counts, dead links — recency can't see those)."
  echo ""
}

# ── ADR lifecycle debt ─────────────────────────────────────────
# An ADR left in Draft or Proposed is a decision the project has stopped
# deciding. Frontmatter is read directly rather than through the ADR tool, so
# the check works in projects that never vendored it and costs one awk pass.
# Archived ADRs are out of the active set and are skipped.

adr_lifecycle() {
  local arch="$PROJECT_DIR/docs/architecture"
  [[ -d "$arch" ]] || return 0

  local rows
  rows=$(find "$arch" -name '*.md' -type f \
           -not -path "$arch/archive/*" -not -name 'INDEX.md' -print0 2>/dev/null \
         | xargs -0 -r awk '
      FNR == 1 { fm = 0; head = 0; emitted = 0; st = ""; dt = "" }
      emitted { next }
      FNR == 1 && $0 ~ /^---[[:space:]]*$/ { fm = 1; next }
      fm && $0 ~ /^---[[:space:]]*$/ { fm = 0; head = 1; next }
      fm && $0 ~ /^status:/ {
        st = $0; sub(/^status:[[:space:]]*/, "", st)
        gsub(/^["'"'"']|["'"'"'][[:space:]]*$|[[:space:]]+$/, "", st)
        next
      }
      fm && $0 ~ /^date:/ {
        dt = $0; sub(/^date:[[:space:]]*/, "", dt)
        gsub(/^["'"'"']|["'"'"'][[:space:]]*$|[[:space:]]+$/, "", dt)
        next
      }
      head && $0 ~ /^#[[:space:]]*ADR-/ {
        line = $0; sub(/^#[[:space:]]*/, "", line)
        num = line; sub(/:.*$/, "", num); sub(/^ADR-/, "", num)
        ttl = line; sub(/^[^:]*:[[:space:]]*/, "", ttl)
        if (dt == "") dt = "9999-99-99"
        if (st != "") print st "\t" dt "\t" num "\t" ttl
        emitted = 1
      }
    ' 2>/dev/null)
  [[ -z "$rows" ]] && return 0

  local draft proposed n_draft n_proposed
  draft=$(printf '%s\n' "$rows" | awk -F'\t' 'tolower($1) == "draft"' | sort -t"$(printf '\t')" -k2,2 -k3,3n)
  proposed=$(printf '%s\n' "$rows" | awk -F'\t' 'tolower($1) == "proposed"' | sort -t"$(printf '\t')" -k2,2 -k3,3n)
  n_draft=$(printf '%s' "$draft" | grep -c . || true)
  n_proposed=$(printf '%s' "$proposed" | grep -c . || true)
  (( n_draft + n_proposed == 0 )) && return 0

  # "1 Draft, 2 Proposed", dropping the zero side.
  local counts=""
  (( n_draft > 0 )) && counts="${n_draft} Draft"
  if (( n_proposed > 0 )); then
    [[ -n "$counts" ]] && counts="${counts}, "
    counts="${counts}${n_proposed} Proposed"
  fi
  echo "📐 **ADR lifecycle:** ${counts} under \`docs/architecture\`."

  local label rows_var line date num title
  for label in Draft Proposed; do
    [[ "$label" == Draft ]] && rows_var="$draft" || rows_var="$proposed"
    [[ -z "$rows_var" ]] && continue
    line=$(printf '%s\n' "$rows_var" | head -1)
    date=$(printf '%s' "$line" | cut -f2)
    num=$(printf '%s' "$line" | cut -f3)
    title=$(printf '%s' "$line" | cut -f4)
    [[ "$date" == "9999-99-99" ]] && date="undated"
    (( ${#title} > 70 )) && title="${title:0:70}..."
    echo "Oldest ${label}: ADR-${num} (${date}) ${title}"
  done
  echo ""
}

doc_freshness
adr_lifecycle
