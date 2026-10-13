#!/usr/bin/env bash
# Dynamic context for GitHub way
# Two concerns:
#   1. Project scope (solo vs team) and workflow recommendations
#   2. Repository health — how well-configured is this repo?

# Time budget: ways kills a macro at 3 s and keeps none of its output. Every
# GitHub call runs in one parallel batch bounded to 2 s per call, so the
# batch ends by about 2.1 s. The batch is six calls: on some hosts more than
# six concurrent `gh` processes stall the extras for seconds (seen: 6 calls
# in 0.4 s, the 7th to 9th in 5.3 s), so the current user is read from gh's
# local config and the security policy is one GraphQL field.
#
# Stock macOS has no `timeout`. There the stand-in below runs each call
# unbounded, and only ways' 3 s macro deadline bounds the macro as a whole.
command -v timeout >/dev/null 2>&1 || timeout() { shift; "$@"; }

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

# One bounded `gh` call: stdout in NAME.out, stderr in NAME.err, exit
# status in NAME.rc. A call killed before it wrote NAME.rc has none.
call() {
  local name=$1; shift
  timeout 2 gh "$@" >"$WORK_DIR/$name.out" 2>"$WORK_DIR/$name.err"
  echo $? >"$WORK_DIR/$name.rc"
}
# The call answered.
ok() { [[ $(cat "$WORK_DIR/$1.rc" 2>/dev/null) == 0 ]]; }
# The API answered that the thing is not there (HTTP 404). A timeout or any
# other failure is neither ok nor absent: the check is unknown.
absent() { ! ok "$1" && [[ -s "$WORK_DIR/$1.rc" ]] && grep -q 'HTTP 404' "$WORK_DIR/$1.err" 2>/dev/null; }
out() { cat "$WORK_DIR/$1.out" 2>/dev/null; }

# The default branch for the protection call, read locally so that call
# can join the batch instead of waiting on the repo details.
PROTECT_BRANCH=$(git symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null)
PROTECT_BRANCH=${PROTECT_BRANCH#origin/}
PROTECT_BRANCH=${PROTECT_BRANCH:-main}

# --- Parallel API calls ---
# Repo details (description, topics, permissions, full_name for badges). Its
# failure also says whether this is a GitHub repo at all.
call repo api repos/:owner/:repo \
  --jq '{
    description: .description,
    topics: .topics,
    permissions: .permissions,
    full_name: .full_name
  }' &
# Community profile (README, license, CoC, contributing, templates)
call community api repos/:owner/:repo/community/profile &
# Contributors (save full response for reuse in team path)
call contributors api repos/:owner/:repo/contributors &
# Labels — paginate returns per-page results, aggregated through jq below
call labels api repos/:owner/:repo/labels --paginate &
# Security policy (not in community profile): GitHub's own answer, which
# covers SECURITY.md at the root, in .github or docs, and an org default
call security api graphql \
  -f query='query($owner:String!,$name:String!){repository(owner:$owner,name:$name){isSecurityPolicyEnabled}}' \
  -F owner='{owner}' -F name='{repo}' --jq '.data.repository.isSecurityPolicyEnabled' &
# Branch protection: a success with a url means protected, a 404 means none
call protection api "repos/:owner/:repo/branches/$PROTECT_BRANCH/protection" --jq '.url' &
wait

# Not a GitHub repo: the repo call failed without a timeout or a network
# error, so gh could not map this directory to a GitHub repo.
if ! ok repo; then
  if [[ ! -s "$WORK_DIR/repo.rc" || $(cat "$WORK_DIR/repo.rc") == 124 ]] \
    || grep -qiE 'error connecting|could not resolve|timeout|timed out|HTTP 5' "$WORK_DIR/repo.err" 2>/dev/null; then
    echo "**Note**: Could not reach GitHub API"
  else
    echo "**Note**: Not a GitHub repository - GitHub commands won't work"
  fi
  exit 0
fi

# --- Parse results ---

CONTRIBUTORS=$(out contributors | jq -r 'length' 2>/dev/null)
# The current user, from gh's local config: no call
CURRENT_USER=$(gh config get user -h github.com 2>/dev/null)

# Active contributors — anyone who committed in the last 90 days
# Local git log is faster and more accurate than API for this
ACTIVE_CONTRIBUTORS=$(git log --since="90 days ago" --format='%aN' 2>/dev/null | sort -u | wc -l)
ACTIVE_CONTRIBUTORS=${ACTIVE_CONTRIBUTORS:-0}

# Repo details
DESCRIPTION=$(out repo | jq -r '.description // empty' 2>/dev/null)
TOPICS=$(out repo | jq -r '.topics | length' 2>/dev/null)
CAN_PUSH=$(out repo | jq -r '.permissions.push // false' 2>/dev/null)
CAN_ADMIN=$(out repo | jq -r '.permissions.admin // false' 2>/dev/null)
REPO_FULL_NAME=$(out repo | jq -r '.full_name // empty' 2>/dev/null)

# Community profile checks — normalize to "yes" or "" for clean truthiness
community_has() { out community | jq -r "if .files.$1 then \"yes\" else \"\" end" 2>/dev/null; }
HAS_README=$(community_has readme)
HAS_LICENSE=$(community_has license)
HAS_COC=$(community_has code_of_conduct)
HAS_CONTRIBUTING=$(community_has contributing)
HAS_ISSUE_TEMPLATE=$(community_has issue_template)
HAS_PR_TEMPLATE=$(community_has pull_request_template)

# Labels — aggregate paginated JSON and count non-defaults
CUSTOM_LABELS=$(out labels | jq -s '[.[][] | select(.default == false)] | length' 2>/dev/null)

# README badges (shields.io), read locally
HAS_BADGES=""
README_PATH=$(git rev-parse --show-toplevel 2>/dev/null)/README.md
if [[ -f "$README_PATH" ]]; then
  if grep -qiE 'img\.shields\.io|badge\.fury\.io|badgen\.net' "$README_PATH" 2>/dev/null; then
    HAS_BADGES="yes"
  fi
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
    REVIEWERS=$(out contributors | jq -r '.[0:5][].login' 2>/dev/null \
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

# Each check is pass, fail, or unknown. Unknown means the call behind it
# timed out or failed: it is never shown as missing, since the way offers to
# add what is missing, and a file that exists would be offered again.
declare -a CHECK_NAMES=()
declare -a CHECK_STATUS=()
declare -a CHECK_NEEDS_ADMIN=()

add_check() {  # name value needs-admin answered
  local name="$1" value="$2" needs_admin="${3:-false}" answered="${4:-yes}"
  CHECK_NAMES+=("$name")
  CHECK_NEEDS_ADMIN+=("$needs_admin")
  # Guard against empty, "null", "0", and "false" — all mean absent
  if [[ -n "$value" ]] && [[ "$value" != "null" ]] && [[ "$value" != "0" ]] && [[ "$value" != "false" ]]; then
    CHECK_STATUS+=("pass")
  elif [[ "$answered" == "yes" ]]; then
    CHECK_STATUS+=("fail")
  else
    CHECK_STATUS+=("unknown")
  fi
}
answered() { { ok "$1" || absent "$1"; } && echo yes || echo no; }

# Security policy: GitHub's true or false, or unknown when the call failed.
HAS_SECURITY_POLICY=$(ok security && out security)
SECURITY_ANSWERED=$(ok security && [[ $(out security) =~ ^(true|false)$ ]] && echo yes || echo no)
# Branch protection: protected, a 404, or unknown.
HAS_BRANCH_PROTECTION=$(ok protection && [[ -n $(out protection) ]] && echo yes)
PROTECTION_ANSWERED=$(absent protection && echo yes || echo no)

add_check "README"              "$HAS_README"             "false" "$(answered community)"
add_check "License"             "$HAS_LICENSE"            "false" "$(answered community)"
add_check "Description"         "$DESCRIPTION"            "false" "$(answered repo)"
add_check "Topics"              "$TOPICS"                 "false" "$(answered repo)"
add_check "Code of conduct"     "$HAS_COC"                "false" "$(answered community)"
add_check "Contributing guide"  "$HAS_CONTRIBUTING"       "false" "$(answered community)"
add_check "Issue templates"     "$HAS_ISSUE_TEMPLATE"     "false" "$(answered community)"
add_check "PR template"         "$HAS_PR_TEMPLATE"        "false" "$(answered community)"
add_check "Security policy"     "$HAS_SECURITY_POLICY"    "false" "$SECURITY_ANSWERED"
add_check "Custom labels"       "$CUSTOM_LABELS"          "false" "$(answered labels)"
add_check "Branch protection"   "$HAS_BRANCH_PROTECTION"  "true"  "$PROTECTION_ANSWERED"
add_check "README badges"       "$HAS_BADGES"             "false" "yes"

# Count passes, failures and checks that could not be made
TOTAL=${#CHECK_NAMES[@]}
PASS_COUNT=0
FAIL_COUNT=0
ADMIN_NEEDED=0
declare -a MISSING_NAMES=()
declare -a UNKNOWN_NAMES=()

for i in "${!CHECK_STATUS[@]}"; do
  case "${CHECK_STATUS[$i]}" in
    pass) ((PASS_COUNT++)) ;;
    unknown) UNKNOWN_NAMES+=("${CHECK_NAMES[$i]}") ;;
    fail)
      ((FAIL_COUNT++))
      MISSING_NAMES+=("${CHECK_NAMES[$i]}")
      if [[ "${CHECK_NEEDS_ADMIN[$i]}" == "true" && "$CAN_ADMIN" != "true" ]]; then
        ((ADMIN_NEEDED++))
      fi
      ;;
  esac
done
join_names() { local s; printf -v s ', %s' "$@"; printf '%s' "${s:2}"; }

# --- Tiered output ---

if [[ "$FAIL_COUNT" -eq 0 ]]; then
  # Silent — everything checked is configured
  :
elif [[ "$FAIL_COUNT" -le 3 ]]; then
  # Brief hint with per-item fixability awareness
  MISSING_LIST=$(join_names "${MISSING_NAMES[@]}")
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
    case "${CHECK_STATUS[$i]}" in
      pass) STATUS="ok"; FIXABLE="—" ;;
      unknown) STATUS="couldn't check"; FIXABLE="—" ;;
      fail)
        STATUS="missing"
        if [[ "${CHECK_NEEDS_ADMIN[$i]}" == "true" ]]; then
          if [[ "$CAN_ADMIN" == "true" ]]; then FIXABLE="yes"; else FIXABLE="needs admin"; fi
        else
          if [[ "$CAN_PUSH" == "true" ]]; then FIXABLE="yes"; else FIXABLE="read-only"; fi
        fi
        ;;
    esac
    echo "| $NAME | $STATUS | $FIXABLE |"
  done
fi
# Outside the table, a check that could not be made gets its own line.
if [[ ${#UNKNOWN_NAMES[@]} -gt 0 && "$FAIL_COUNT" -le 3 ]]; then
  [[ "$FAIL_COUNT" -eq 0 ]] && echo ""
  echo "**Repo health**: couldn't check $(join_names "${UNKNOWN_NAMES[@]}") (the GitHub call failed or timed out)"
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
# Precedence follows Claude Code's documented order for these three files
# (project-local > project > user, highest wins; "Settings files and
# precedence", https://code.claude.com/docs/en/settings). That page states that
# a key set at a higher level overrides the same key lower down, and that list
# keys such as permissions.allow combine across files. It does not state
# whether an object key such as `attribution` merges sub-key by sub-key across
# files. This section resolves each sub-key INDEPENDENTLY — so a project file
# setting .commit does not hide a user file setting .sessionUrl — which is an
# assumption, not verified behaviour. If Claude Code instead takes the whole
# object from the highest-precedence file, per-key resolution reports the wrong
# source at that seam (the common single-file case is unaffected).
# Managed/enterprise policy and command-line overrides are not accounted for.

PROJECT_ROOT=$(git rev-parse --show-toplevel 2>/dev/null)
ATTR_FILES=(
  "$PROJECT_ROOT/.claude/settings.local.json"
  "$PROJECT_ROOT/.claude/settings.json"
  "$HOME/.claude/settings.json"
)

# Settings files that exist but don't parse, and files whose `attribution` is
# present but isn't an object. Both are reported, never silently skipped: such a
# file may be the one meant to say `sessionUrl: false`, and staying quiet about
# it is how a control surface lies. Valid JSON with a mistyped `attribution`
# passes the parse-guard, so it needs its own check.
ATTR_BAD=()
ATTR_MISTYPED=()
for f in "${ATTR_FILES[@]}"; do
  [[ -f "$f" ]] || continue
  if ! jq -e . "$f" >/dev/null 2>&1; then
    ATTR_BAD+=("$f")
  elif jq -e 'has("attribution") and ((.attribution | type) != "object")' "$f" >/dev/null 2>&1; then
    ATTR_MISTYPED+=("$f")
  fi
done

# Resolve one attribution sub-key down the precedence chain, into the globals
# ATTR_VAL (the value, JSON-ENCODED) and ATTR_SRC (the file that set it).
# ATTR_VAL="" means no readable file defines the key.
#
# Three deliberate choices, each fixing a way this surface previously lied:
#   * Globals, not a packed "value|source" string — a value may legitimately
#     contain any delimiter (`attribution.commit: "Co-Authored-By: X | Y"`).
#   * `jq -e .` parse-guard — jq on an empty file prints nothing and exits 0, so
#     an empty settings.json would otherwise read as "key present, value empty"
#     and mask every lower-precedence file. -e exits non-zero on empty/malformed.
#   * `tojson`, not `tostring` — tostring erases the type, laundering the STRING
#     "false" into a bool-looking `false` and reporting a confident OFF for a key
#     Claude Code reads as a boolean (i.e. trailer still live). tojson keeps
#     `false` and `"false"` distinct, and can never emit an empty string, which
#     is what makes the ATTR_VAL="" absence test safe even though "" is itself a
#     legitimate commit/pr value.
resolve_attr_key() {
  local key="$1" f raw
  ATTR_VAL=""; ATTR_SRC=""
  for f in "${ATTR_FILES[@]}"; do
    [[ -f "$f" ]] || continue
    jq -e . "$f" >/dev/null 2>&1 || continue
    raw=$(jq -r --arg k "$key" '
      if ((.attribution? | type) == "object") and (.attribution | has($k))
      then (.attribution[$k] | tojson)
      else empty end' "$f" 2>/dev/null) || continue
    [[ -z "$raw" ]] && continue
    ATTR_VAL="$raw"; ATTR_SRC="$f"
    return
  done
}

# Shorten $HOME to ~ for display. Don't use ${var/#$HOME/~}: bash
# tilde-expands the replacement, turning ~ back into $HOME (a no-op).
src_label() {
  case "$1" in
    "") echo "not set in any settings file" ;;
    "$HOME"/*) echo "~${1#"$HOME"}" ;;
    *) echo "$1" ;;
  esac
}

# Decode a JSON-encoded scalar back to its display form.
json_decode() { printf '%s' "$1" | jq -r . 2>/dev/null; }

echo ""

# --- the session link (the disclosure control) ---
# Fail-safe direction is ON: anything we cannot read as an explicit boolean
# false is reported as live. A false OFF is the failure mode that matters.
resolve_attr_key sessionUrl
case "$ATTR_VAL" in
  false)
    echo "**Session-link trailer**: OFF — \`attribution.sessionUrl: false\` [$(src_label "$ATTR_SRC")]"
    ;;
  true)
    echo "**Session-link trailer**: **ON** — \`attribution.sessionUrl: true\` [$(src_label "$ATTR_SRC")]. This publishes a link to the FULL session transcript; set it to \`false\` to suppress (ADR-167)."
    ;;
  "")
    echo "**Session-link trailer**: **ON (default)** — \`attribution.sessionUrl\` is unset and defaults to true. This publishes a link to the FULL session transcript; set \`attribution.sessionUrl: false\` in settings.json to suppress (ADR-167)."
    ;;
  *)
    echo "**Session-link trailer**: **ON** — \`attribution.sessionUrl\` is \`$ATTR_VAL\` [$(src_label "$ATTR_SRC")], which is not the boolean Claude Code expects, so the key does not take effect. Set \`attribution.sessionUrl: false\` (unquoted) to suppress (ADR-167)."
    ;;
esac

# --- the attribution footers (a separate, cosmetic control) ---
describe_footer() {
  case "$1" in
    "")     echo "default (ON)" ;;
    '""')   echo "OFF (empty)" ;;
    '"'*)   echo "custom: \"$(json_decode "$1")\"" ;;
    *)      echo "unexpected non-string value \`$1\` (expected a string)" ;;
  esac
}
resolve_attr_key commit; CM_VAL="$ATTR_VAL"; CM_SRC="$ATTR_SRC"
resolve_attr_key pr;     PR_VAL="$ATTR_VAL"; PR_SRC="$ATTR_SRC"
if [[ "$CM_VAL" == "$PR_VAL" && "$CM_SRC" == "$PR_SRC" ]]; then
  echo "**Attribution footers** (Co-Authored-By / \"Generated with Claude Code\"): $(describe_footer "$CM_VAL") [$(src_label "$CM_SRC")]"
else
  echo "**Attribution footers** (Co-Authored-By / \"Generated with Claude Code\"):"
  echo "- commit: $(describe_footer "$CM_VAL") [$(src_label "$CM_SRC")]"
  echo "- pr: $(describe_footer "$PR_VAL") [$(src_label "$PR_SRC")]"
fi

for f in "${ATTR_BAD[@]}"; do
  echo "- ⚠ \`$(src_label "$f")\` exists but is not valid JSON — it was skipped above, so the values reported may not be what Claude Code uses."
done
for f in "${ATTR_MISTYPED[@]}"; do
  echo "- ⚠ \`$(src_label "$f")\` has an \`attribution\` that is not an object — it sets nothing and was skipped above."
done
