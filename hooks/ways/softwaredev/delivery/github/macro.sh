#!/usr/bin/env bash
# Dynamic context for GitHub way
# Two concerns:
#   1. Project scope (solo vs team) and workflow recommendations
#   2. Repository health — how well-configured is this repo?

# Early exit if not a GitHub repo
gh repo view &>/dev/null || {
  echo "**Note**: Not a GitHub repository - GitHub commands won't work"
  exit 0
}

# --- Parallel API calls ---
# We need: repo details, community profile, labels, branch protection
# Fire them all at once and collect results

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

# Repo details (description, topics, permissions, default branch, full_name for badges)
timeout 3 gh api repos/:owner/:repo \
  --jq '{
    description: .description,
    topics: .topics,
    default_branch: .default_branch,
    permissions: .permissions,
    has_issues: .has_issues,
    has_discussions: .has_discussions,
    full_name: .full_name
  }' >"$WORK_DIR/repo.json" 2>/dev/null &

# Community profile (README, license, CoC, contributing, templates, security)
timeout 3 gh api repos/:owner/:repo/community/profile \
  >"$WORK_DIR/community.json" 2>/dev/null &

# Contributors (save full response for reuse in team path)
timeout 3 gh api repos/:owner/:repo/contributors \
  >"$WORK_DIR/contributors.json" 2>/dev/null &

# Labels — paginate returns per-page jq results, so pipe aggregate through jq
timeout 3 gh api repos/:owner/:repo/labels --paginate \
  >"$WORK_DIR/labels.json" 2>/dev/null &

# Security policy (not in community profile, check both common locations)
(timeout 3 gh api repos/:owner/:repo/contents/SECURITY.md --jq '.name' >"$WORK_DIR/security.txt" 2>/dev/null ||
 timeout 3 gh api repos/:owner/:repo/contents/.github/SECURITY.md --jq '.name' >"$WORK_DIR/security.txt" 2>/dev/null) &

# Current user
timeout 3 gh api user --jq '.login' >"$WORK_DIR/user.txt" 2>/dev/null &

wait

# --- Parse results ---

CONTRIBUTORS=$(jq -r 'length' "$WORK_DIR/contributors.json" 2>/dev/null)
CURRENT_USER=$(cat "$WORK_DIR/user.txt" 2>/dev/null)

# Active contributors — anyone who committed in the last 90 days
# Local git log is faster and more accurate than API for this
ACTIVE_CONTRIBUTORS=$(git log --since="90 days ago" --format='%aN' 2>/dev/null | sort -u | wc -l)
ACTIVE_CONTRIBUTORS=${ACTIVE_CONTRIBUTORS:-0}

# Repo details
DESCRIPTION=$(jq -r '.description // empty' "$WORK_DIR/repo.json" 2>/dev/null)
TOPICS=$(jq -r '.topics | length' "$WORK_DIR/repo.json" 2>/dev/null)
DEFAULT_BRANCH=$(jq -r '.default_branch // "main"' "$WORK_DIR/repo.json" 2>/dev/null)
CAN_PUSH=$(jq -r '.permissions.push // false' "$WORK_DIR/repo.json" 2>/dev/null)
CAN_ADMIN=$(jq -r '.permissions.admin // false' "$WORK_DIR/repo.json" 2>/dev/null)
REPO_FULL_NAME=$(jq -r '.full_name // empty' "$WORK_DIR/repo.json" 2>/dev/null)

# Community profile checks — normalize to "yes" or "" for clean truthiness
HAS_README=$(jq -r 'if .files.readme then "yes" else "" end' "$WORK_DIR/community.json" 2>/dev/null)
HAS_LICENSE=$(jq -r 'if .files.license then "yes" else "" end' "$WORK_DIR/community.json" 2>/dev/null)
HAS_COC=$(jq -r 'if .files.code_of_conduct then "yes" else "" end' "$WORK_DIR/community.json" 2>/dev/null)
HAS_CONTRIBUTING=$(jq -r 'if .files.contributing then "yes" else "" end' "$WORK_DIR/community.json" 2>/dev/null)
HAS_ISSUE_TEMPLATE=$(jq -r 'if .files.issue_template then "yes" else "" end' "$WORK_DIR/community.json" 2>/dev/null)
HAS_PR_TEMPLATE=$(jq -r 'if .files.pull_request_template then "yes" else "" end' "$WORK_DIR/community.json" 2>/dev/null)
# Security policy — populated by parallel call above
HAS_SECURITY_POLICY=$(cat "$WORK_DIR/security.txt" 2>/dev/null)

# Labels — aggregate paginated JSON and count non-defaults
CUSTOM_LABELS=$(jq -s '[.[][] | select(.default == false)] | length' "$WORK_DIR/labels.json" 2>/dev/null)

# Branch protection (separate call - needs the default branch name)
HAS_BRANCH_PROTECTION=""
if [[ -n "$DEFAULT_BRANCH" ]]; then
  timeout 3 gh api "repos/:owner/:repo/branches/$DEFAULT_BRANCH/protection" \
    --jq '.url' >"$WORK_DIR/protection.txt" 2>/dev/null
  if [[ $? -eq 0 ]] && [[ -s "$WORK_DIR/protection.txt" ]]; then
    HAS_BRANCH_PROTECTION="yes"
  fi
fi

# README badges (shields.io)
HAS_BADGES=""
README_PATH=$(git rev-parse --show-toplevel 2>/dev/null)/README.md
if [[ -f "$README_PATH" ]]; then
  if grep -qiE 'img\.shields\.io|badge\.fury\.io|badgen\.net' "$README_PATH" 2>/dev/null; then
    HAS_BADGES="yes"
  fi
fi

# --- Bail if API didn't respond ---
if [[ -z "$CONTRIBUTORS" ]] && [[ ! -s "$WORK_DIR/repo.json" ]]; then
  echo "**Note**: Could not reach GitHub API"
  exit 0
fi

# ============================================================
# SECTION 1: Project scope
# ============================================================

if [[ -n "$CONTRIBUTORS" ]]; then
  # Classify by active contributors (last 90 days), not total
  if [[ "$ACTIVE_CONTRIBUTORS" -le 2 ]]; then
    echo "**Context**: Solo/pair project ($ACTIVE_CONTRIBUTORS active, $CONTRIBUTORS total contributors)"
    echo "- PRs recommended even for solo work — they create history, enable CI, and build good habits"
    echo "- Lightweight PRs are fine: a title and a few bullet points"
  else
    # Reuse saved contributors response instead of re-fetching
    REVIEWERS=$(jq -r '.[0:5][].login' "$WORK_DIR/contributors.json" 2>/dev/null \
      | grep -v "$CURRENT_USER" | head -3 | tr '\n' ', ' | sed 's/,$//')
    echo "**Context**: Team project ($ACTIVE_CONTRIBUTORS active, $CONTRIBUTORS total contributors)"
    echo "- PR required for all changes — review before merge"
    if [[ -n "$REVIEWERS" ]]; then
      echo "- Potential reviewers: $REVIEWERS"
    fi
    echo "- Consider [Claude Code Review](https://claude.com/blog/code-review) for automated multi-agent PR analysis (\$15-25/review, Team/Enterprise plans)"
  fi
fi

# ============================================================
# SECTION 2: Repository health checks
# ============================================================

# Build array of checks: name, status (pass/fail)
declare -a CHECK_NAMES=()
declare -a CHECK_STATUS=()
declare -a CHECK_NEEDS_ADMIN=()

add_check() {
  local name="$1"
  local value="$2"
  local needs_admin="${3:-false}"
  CHECK_NAMES+=("$name")
  CHECK_NEEDS_ADMIN+=("$needs_admin")
  # Guard against empty, "null", "0", and "false" — all mean absent
  if [[ -n "$value" ]] && [[ "$value" != "null" ]] && [[ "$value" != "0" ]] && [[ "$value" != "false" ]]; then
    CHECK_STATUS+=("pass")
  else
    CHECK_STATUS+=("fail")
  fi
}

add_check "README"              "$HAS_README"             "false"
add_check "License"             "$HAS_LICENSE"            "false"
add_check "Description"         "$DESCRIPTION"            "false"
add_check "Topics"              "$TOPICS"                 "false"
add_check "Code of conduct"     "$HAS_COC"                "false"
add_check "Contributing guide"  "$HAS_CONTRIBUTING"       "false"
add_check "Issue templates"     "$HAS_ISSUE_TEMPLATE"     "false"
add_check "PR template"         "$HAS_PR_TEMPLATE"        "false"
add_check "Security policy"     "$HAS_SECURITY_POLICY"    "false"
add_check "Custom labels"       "$CUSTOM_LABELS"          "false"
add_check "Branch protection"   "$HAS_BRANCH_PROTECTION"  "true"
add_check "README badges"       "$HAS_BADGES"             "false"

# Count passes and failures
TOTAL=${#CHECK_NAMES[@]}
PASS_COUNT=0
FAIL_COUNT=0
ADMIN_NEEDED=0
declare -a MISSING_NAMES=()
declare -a MISSING_FIXABLE=()

for i in "${!CHECK_STATUS[@]}"; do
  if [[ "${CHECK_STATUS[$i]}" == "pass" ]]; then
    ((PASS_COUNT++))
  else
    ((FAIL_COUNT++))
    MISSING_NAMES+=("${CHECK_NAMES[$i]}")
    # Determine if user can fix this
    if [[ "${CHECK_NEEDS_ADMIN[$i]}" == "true" ]]; then
      if [[ "$CAN_ADMIN" == "true" ]]; then
        MISSING_FIXABLE+=("yes")
      else
        MISSING_FIXABLE+=("needs admin")
        ((ADMIN_NEEDED++))
      fi
    else
      if [[ "$CAN_PUSH" == "true" ]]; then
        MISSING_FIXABLE+=("yes")
      else
        MISSING_FIXABLE+=("read-only")
      fi
    fi
  fi
done

# --- Tiered output ---

if [[ "$FAIL_COUNT" -eq 0 ]]; then
  # Silent — everything configured
  :
elif [[ "$FAIL_COUNT" -le 3 ]]; then
  # Brief hint with per-item fixability awareness
  MISSING_LIST=$(printf '%s' "${MISSING_NAMES[0]}"; printf ', %s' "${MISSING_NAMES[@]:1}")
  if [[ "$CAN_PUSH" == "true" ]]; then
    RIGHTS="you have push access"
  else
    RIGHTS="read-only access"
  fi
  # Note admin-only items separately if present
  if [[ "$ADMIN_NEEDED" -gt 0 ]]; then
    RIGHTS="$RIGHTS; $ADMIN_NEEDED need admin"
  fi
  echo ""
  echo "**Repo health**: $PASS_COUNT/$TOTAL — missing: $MISSING_LIST ($RIGHTS)"
else
  # Full table
  echo ""
  echo "**Repo health**: $PASS_COUNT/$TOTAL checks pass"
  echo ""
  echo "| Check | Status | Can fix |"
  echo "|-------|--------|---------|"
  for i in "${!CHECK_NAMES[@]}"; do
    NAME="${CHECK_NAMES[$i]}"
    if [[ "${CHECK_STATUS[$i]}" == "pass" ]]; then
      STATUS="ok"
      FIXABLE="—"
    else
      STATUS="missing"
      if [[ "${CHECK_NEEDS_ADMIN[$i]}" == "true" ]]; then
        if [[ "$CAN_ADMIN" == "true" ]]; then
          FIXABLE="yes"
        else
          FIXABLE="needs admin"
        fi
      else
        if [[ "$CAN_PUSH" == "true" ]]; then
          FIXABLE="yes"
        else
          FIXABLE="read-only"
        fi
      fi
    fi
    echo "| $NAME | $STATUS | $FIXABLE |"
  done
fi

# --- Badge recommendation when missing ---
if [[ -z "$HAS_BADGES" ]] && [[ -n "$REPO_FULL_NAME" ]] && [[ "$CAN_PUSH" == "true" ]]; then
  echo ""
  echo "**Recommended README badges** (add below the H1 title):"
  echo '```markdown'
  echo "![License](https://img.shields.io/github/license/${REPO_FULL_NAME})"
  echo "![GitHub stars](https://img.shields.io/github/stars/${REPO_FULL_NAME}?style=social)"
  echo "![Latest Release](https://img.shields.io/github/v/release/${REPO_FULL_NAME}?include_prereleases&label=version)"
  echo '```'
fi

# ============================================================
# SECTION 3: Attribution / session-link control surface
# ============================================================
# `attribution` governs what Claude Code appends to commits and PR bodies.
# It is TWO INDEPENDENT CONTROLS, and conflating them is exactly the error
# ADR-167 corrects:
#
#   attribution.sessionUrl  (bool, default TRUE)  -> the `Claude-Session:`
#       transcript link. This is the disclosure control — the link resolves to
#       the full session transcript. Added upstream in v2.1.183; undocumented
#       (upstream #69614), which is how it stayed unset here for a month.
#   attribution.commit / .pr  (string, "" disables) -> the Co-Authored-By and
#       "Generated with Claude Code" FOOTERS. These never governed the session
#       link.
#
# This section previously reported "Session-link trailer" off commit/pr alone
# and never read sessionUrl, so it printed "OFF ... no session trailer" in
# every session — including sessions whose system prompt carried the trailer.
# A surface that asserts a fact it never read is worse than no surface.
#
# Precedence approximates Claude Code's resolution order (project-local >
# project > user-global, highest wins). Each sub-key resolves INDEPENDENTLY
# because Claude Code deep-merges objects: a project file setting .commit does
# not hide a user file setting .sessionUrl. Managed/enterprise policy and
# command-line overrides are not accounted for.

PROJECT_ROOT=$(git rev-parse --show-toplevel 2>/dev/null)
ATTR_FILES=(
  "$PROJECT_ROOT/.claude/settings.local.json"
  "$PROJECT_ROOT/.claude/settings.json"
  "$HOME/.claude/settings.json"
)

# Resolve one attribution sub-key down the precedence chain.
# Echoes "<value>|<source>", or "__UNSET__|" when no file defines it.
# Note the __UNSET__ sentinel comes from jq, not from emptiness: "" is a
# legitimate value for commit/pr and must not read as absent.
resolve_attr_key() {
  local key="$1" f v
  for f in "${ATTR_FILES[@]}"; do
    [[ -f "$f" ]] || continue
    v=$(jq -r --arg k "$key" \
      'if (.attribution? // {} | has($k)) then (.attribution[$k] | tostring) else "__UNSET__" end' \
      "$f" 2>/dev/null) || continue
    [[ "$v" == "__UNSET__" ]] && continue
    printf '%s|%s\n' "$v" "$f"
    return
  done
  printf '__UNSET__|\n'
}

# Shorten $HOME to ~ for display. Don't use ${var/#$HOME/~}: bash
# tilde-expands the replacement, turning ~ back into $HOME (a no-op).
src_label() {
  case "$1" in
    "") echo "no settings file" ;;
    "$HOME"/*) echo "~${1#"$HOME"}" ;;
    *) echo "$1" ;;
  esac
}

SU_RAW="$(resolve_attr_key sessionUrl)"; SU_SRC="${SU_RAW#*|}"; SU_VAL="${SU_RAW%%|*}"
CM_RAW="$(resolve_attr_key commit)";     CM_SRC="${CM_RAW#*|}"; CM_VAL="${CM_RAW%%|*}"
PR_RAW="$(resolve_attr_key pr)";         PR_SRC="${PR_RAW#*|}"; PR_VAL="${PR_RAW%%|*}"

echo ""

# --- the session link (the disclosure control) ---
case "$SU_VAL" in
  false)
    echo "**Session-link trailer**: OFF — \`attribution.sessionUrl: false\` [$(src_label "$SU_SRC")]"
    ;;
  true)
    echo "**Session-link trailer**: **ON** — \`attribution.sessionUrl: true\` [$(src_label "$SU_SRC")]. This publishes a link to the FULL session transcript; set it to \`false\` to suppress (ADR-167)."
    ;;
  __UNSET__)
    echo "**Session-link trailer**: **ON (default)** — \`attribution.sessionUrl\` is unset and defaults to true. This publishes a link to the FULL session transcript; set \`attribution.sessionUrl: false\` in settings.json to suppress (ADR-167)."
    ;;
  *)
    echo "**Session-link trailer**: unrecognized \`attribution.sessionUrl\` value \`$SU_VAL\` (expected true/false) [$(src_label "$SU_SRC")] — treat as ON."
    ;;
esac

# --- the attribution footers (a separate, cosmetic control) ---
describe_footer() {
  case "$1" in
    __UNSET__) echo "default (ON)" ;;
    "")        echo "OFF (empty)" ;;
    *)         echo "custom: \"$1\"" ;;
  esac
}
if [[ "$CM_VAL" == "$PR_VAL" && "$CM_SRC" == "$PR_SRC" ]]; then
  echo "**Attribution footers** (Co-Authored-By / \"Generated with Claude Code\"): $(describe_footer "$CM_VAL") [$(src_label "$CM_SRC")]"
else
  echo "**Attribution footers** (Co-Authored-By / \"Generated with Claude Code\"):"
  echo "- commit: $(describe_footer "$CM_VAL") [$(src_label "$CM_SRC")]"
  echo "- pr: $(describe_footer "$PR_VAL") [$(src_label "$PR_SRC")]"
fi
