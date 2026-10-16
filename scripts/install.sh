#!/usr/bin/env bash
# Install agent-ways as an XDG application projected into ~/.claude (1.0, ADR-142/144).
#
# The application lives in $XDG_DATA_HOME/agent-ways; ~/.claude becomes a thin
# projection (symlinks + Claude-Code-owned files). This is a gateway, not a butler:
# it stages the app, builds it, and reconciles the projection.
#
# Usage:
#   scripts/install.sh                           # install from repo root
#   scripts/install.sh --bootstrap               # clone latest + install
#   curl ... | bash -s -- --bootstrap            # self-bootstrap from internet
#   scripts/install.sh --dangerously-clobber     # nuke ~/.claude + app dir, reinstall
#   scripts/install.sh --stage DIR --from URL --rev SHA [--branch B]
#                                                # fetch + verify one commit into DIR, install nothing
#                                                # (`ways manage reinstall` stages with this, ADR-509)
#
# The happy path: stage app → make setup → ways manage repair. Everything else is guardrails.
#
# Exit codes: 0 done; 1 failed; 2 usage; 3 the app is built and on PATH, and the
# repair after the build failed, most often at a real path in a projected root
# (fix the cause it names, then finish with `ways manage repair --commit`).

set -euo pipefail

UPSTREAM_REPO="aaronsb/agent-ways"
UPSTREAM_URL="https://github.com/${UPSTREAM_REPO}"
DEST="${HOME}/.claude"

# The application root (1.0 XDG layout). The repo is staged here; ~/.claude is a
# projection of it. Honours XDG_DATA_HOME so sandbox installs stay contained.
XDG_DATA="${XDG_DATA_HOME:-${HOME}/.local/share}"
APP_DIR="${XDG_DATA}/agent-ways"
XDG_BIN="${HOME}/.local/bin"

# --- Colors and the staged report ---
#
# The report has the shape every `ways manage` verb prints (ADR-511): ruled
# section headers between stages and a mark per outcome (✓ ok, ⚠ warn,
# ✗ fail, → action, · info). Colour when stdout is a terminal, or when
# CLICOLOR_FORCE or FORCE_COLOR is set; plain text in a pipe, under NO_COLOR,
# and with TERM=dumb, empty or unset. The text is the same either way.

forced_color() {
  local v
  for v in "${CLICOLOR_FORCE:-}" "${FORCE_COLOR:-}"; do
    [[ -n "$v" && "$v" != 0 ]] && return 0
  done
  return 1
}

# Real escape bytes, so a heredoc prints colour as well as `echo -e` and
# `printf %b` do. An unset or empty TERM is a terminal with no colour, as
# `ways` reads it.
if [[ -z "${NO_COLOR:-}" && "${TERM:-dumb}" != dumb ]] && { [[ -t 1 ]] || forced_color; }; then
  RED=$'\033[0;31m'
  GREEN=$'\033[0;32m'
  YELLOW=$'\033[1;33m'
  CYAN=$'\033[0;36m'
  BOLD=$'\033[1m'
  DIM=$'\033[2m'
  RESET=$'\033[0m'
else
  RED='' GREEN='' YELLOW='' CYAN='' BOLD='' DIM='' RESET=''
fi

# The width of a section rule, as `ways` draws it.
RULE_WIDTH=64

rule() {
  local n="$1" out=""
  while (( n-- > 0 )); do out+="─"; done
  printf '%s' "$out"
}

# A section header: a blank line, then `── Name ────…`.
section() {
  local name="$1" fill
  fill=$(( RULE_WIDTH - ${#name} - 4 ))
  (( fill < 3 )) && fill=3
  printf '\n%b──%b %b%s%b %b%s%b\n' "$DIM" "$RESET" "$BOLD$CYAN" "$name" "$RESET" "$DIM" "$(rule "$fill")" "$RESET"
}

# One outcome line: `  <mark> text`.
mark() {
  local kind="$1" glyph color
  shift
  case "$kind" in
    ok) glyph="✓" color="$GREEN" ;;
    warn) glyph="⚠" color="$YELLOW" ;;
    fail) glyph="✗" color="$RED" ;;
    act) glyph="→" color="$CYAN" ;;
    *) glyph="·" color="$DIM" ;;
  esac
  printf '  %b%s%b %s\n' "$color" "$glyph" "$RESET" "$*"
}

# A command's own output, indented under its stage.
indent() {
  local line
  while IFS= read -r line || [[ -n "$line" ]]; do
    printf '    %s\n' "$line"
  done
}

# --- Help ---

show_help() {
  cat <<HELP
${BOLD}agent-ways installer${RESET}

${CYAN}Usage:${RESET}
  scripts/install.sh                           Install from local clone
  scripts/install.sh --bootstrap               Clone latest and install
  curl -sL <raw-url> | bash -s -- --bootstrap  Self-bootstrap from internet

${CYAN}Options:${RESET}
  --bootstrap               Clone latest release to temp, then install
  --dangerously-clobber     Remove ~/.claude AND the app dir, then reinstall (backs up first)
  --help                    Show this help

${CYAN}What it does (1.0 XDG layout):${RESET}
  1. Checks prerequisites (git, jq, make)
  2. Stages the app into \$XDG_DATA_HOME/agent-ways
  3. Builds binaries + embedding model ('make setup')
  4. Reconciles the projection into ~/.claude ('ways manage repair')
     Your Claude Code files (projects/, credentials, settings) are preserved.
     A real skills/, agents/, commands/ or hooks/ways/ directory of your own at
     a projected root stops this step; nothing is deleted.

${CYAN}Already have a pre-1.0 in-place clone at ~/.claude?${RESET}
  Migrate it to the 1.0 layout — see
  ${UPSTREAM_URL}/blob/ways-v1.8.3/docs/migration-1.0.md

HELP
}

# --- Flag parsing ---

BOOTSTRAP=false
CLOBBER=false
STAGE="" STAGE_FROM="" STAGE_REV="" STAGE_BRANCH=""
ARGS=("$@")

while [[ $# -gt 0 ]]; do
  case "$1" in
    --stage|--from|--rev|--branch)
      # A value flag takes the next word, which must be there and not a flag.
      if [[ $# -lt 2 || -z "$2" || "$2" == --* ]]; then
        echo "$1 needs a value." >&2
        exit 2
      fi ;;
  esac
  case "$1" in
    --bootstrap) BOOTSTRAP=true ;;
    --dangerously-clobber) CLOBBER=true ;;
    --stage) STAGE="$2"; shift ;;
    --from) STAGE_FROM="$2"; shift ;;
    --rev) STAGE_REV="$2"; shift ;;
    --branch) STAGE_BRANCH="$2"; shift ;;
    --help|-h) show_help; exit 0 ;;
  esac
  shift
done

# No flags at all → show help
if [[ "$BOOTSTRAP" == "false" && -z "$STAGE" ]] && [[ ! -f "hooks/check-config-updates.sh" ]]; then
  show_help
  exit 0
fi

# --- Prerequisites ---

check_prereqs() {
  local missing=()

  command -v git &>/dev/null  || missing+=("git")
  command -v jq &>/dev/null   || missing+=("jq")
  command -v make &>/dev/null  || missing+=("make")

  if [[ ${#missing[@]} -gt 0 ]]; then
    echo -e "${RED}Missing prerequisites:${RESET} ${missing[*]}"
    echo ""
    echo "Install them first. Platform guides:"
    echo "  macOS:        ${UPSTREAM_URL}/blob/main/docs/prerequisites-macos.md"
    echo "  Arch Linux:   ${UPSTREAM_URL}/blob/main/docs/prerequisites-arch.md"
    echo "  Debian/Ubuntu:${UPSTREAM_URL}/blob/main/docs/prerequisites-debian.md"
    echo "  Fedora/RHEL:  ${UPSTREAM_URL}/blob/main/docs/prerequisites-fedora.md"
    exit 1
  fi
}

# --- Fetch and verify ---

# Clone URL into DEST and check that it is agent-ways. With REV, check out that
# one commit, and fail if the source does not have it; with BRANCH as well, the
# branch name is kept on it, tracking origin, so a later pull works. Both the
# bootstrap and `ways manage reinstall` fetch through here.
fetch_verified() {
  local url="$1" dest="$2" rev="${3:-}" branch="${4:-}"
  if [[ -z "$rev" ]]; then
    git clone --depth 1 "$url" "$dest" 2>&1 || { echo -e "${RED}Failed to clone ${url}${RESET}"; return 1; }
  else
    git clone --no-checkout "$url" "$dest" 2>&1 || { echo -e "${RED}Failed to clone ${url}${RESET}"; return 1; }
    if [[ -n "$branch" ]]; then
      git -C "$dest" checkout -q -B "$branch" "$rev" 2>&1 &&
        { git -C "$dest" branch -q --set-upstream-to="origin/$branch" 2>/dev/null || true; }
    else
      git -C "$dest" -c advice.detachedHead=false checkout -q --detach "$rev" 2>&1
    fi || { echo -e "${RED}${url} does not have ${rev}${RESET}"; return 1; }
  fi
  if [[ ! -f "$dest/hooks/check-config-updates.sh" ]]; then
    echo -e "${RED}Clone doesn't look like agent-ways.${RESET}"
    return 1
  fi
  echo -e "Verified: ${DIM}$(git -C "$dest" log --oneline -1 2>/dev/null)${RESET}"
}

# --- Stage: fetch and verify one commit, install nothing ---

if [[ -n "$STAGE" ]]; then
  if [[ -z "$STAGE_FROM" || -z "$STAGE_REV" ]]; then
    echo -e "${RED}--stage needs --from URL and --rev SHA.${RESET}"
    exit 2
  fi
  if [[ -e "$STAGE" ]]; then
    echo -e "${RED}${STAGE} already exists.${RESET}"
    exit 1
  fi
  check_prereqs
  fetch_verified "$STAGE_FROM" "$STAGE" "$STAGE_REV" "$STAGE_BRANCH" || { rm -rf "$STAGE"; exit 1; }
  exit 0
fi

# --- Self-bootstrap ---

if [[ "$BOOTSTRAP" == "true" ]]; then
  check_prereqs

  echo ""
  echo -e "${BOLD}agent-ways bootstrap${RESET}"
  echo -e "Fetching latest from ${CYAN}${UPSTREAM_REPO}${RESET}..."
  echo ""

  BOOTSTRAP_DIR=$(mktemp -d)
  trap 'rm -rf "$BOOTSTRAP_DIR"' EXIT

  CLONE="$BOOTSTRAP_DIR/agent-ways"
  # AGENT_WAYS_BOOTSTRAP_URL names another source to clone: a mirror, or
  # the stand-in the installer tests use.
  fetch_verified "${AGENT_WAYS_BOOTSTRAP_URL:-$UPSTREAM_URL}" "$CLONE" || exit 1
  echo ""

  # Forward flags (minus --bootstrap) to the verified copy
  FORWARD_ARGS=()
  for arg in "${ARGS[@]}"; do
    [[ "$arg" != "--bootstrap" ]] && FORWARD_ARGS+=("$arg")
  done

  cd "$CLONE"
  # Mark the inner run as bootstrapped so its conflict-options reference the
  # re-runnable curl one-liner, not this temp clone (deleted by the EXIT trap).
  # `${a[@]+"${a[@]}"}`: bash before 4.4 calls an empty array unbound
  # under `set -u`, and the documented `bash -s -- --bootstrap` forwards none.
  AGENT_WAYS_BOOTSTRAPPED=1 bash "$CLONE/scripts/install.sh" ${FORWARD_ARGS[@]+"${FORWARD_ARGS[@]}"}
  exit $?
fi

# --- Detect source ---

# If we're running from a repo, use it as source
SRC="$(cd "$(dirname "$0")/.." && pwd)"

if [[ ! -f "$SRC/hooks/check-config-updates.sh" ]]; then
  echo -e "${RED}Not running from an agent-ways repo.${RESET}"
  echo "Use --bootstrap to clone and install, or cd to the repo first."
  exit 1
fi

check_prereqs

echo -e "${BOLD}agent-ways installer${RESET}"
echo -e "${DIM}source ${SRC} · app ${APP_DIR} · projection ${DEST}${RESET}"

# --- Helpers ---

# Is a directory an agent-ways git clone (ours or a fork of ours)?
is_agent_ways_repo() {
  local dir="$1"
  [[ -d "$dir/.git" ]] || return 1
  local remote owner_repo parent
  remote=$(git -C "$dir" remote get-url origin 2>/dev/null || true)
  owner_repo=$(echo "$remote" | sed -E 's#.*github\.com[:/]##; s/\.git$//')
  [[ "$owner_repo" == "$UPSTREAM_REPO" ]] && return 0
  if command -v gh &>/dev/null; then
    parent=$(gh api "repos/${owner_repo}" --jq '.parent.full_name' 2>/dev/null || true)
    [[ "$parent" == "$UPSTREAM_REPO" ]] && return 0
  fi
  return 1
}

# The repair command a hint names: with --commit when the app's ways has it.
# `make setup` installs the latest release, which can predate the flag
# (ADR-511); that ways repairs with the bare form. The probe asks the fixed
# name, which takes the same flags as `manage repair` and which every
# release has (ADR-509 §4).
repair_cmd() {
  if "$APP_DIR/bin/ways" reconcile --help 2>/dev/null | grep -q -- '--commit'; then
    echo "ways manage repair --commit"
  else
    echo "ways manage repair"
  fi
}

# Shown when the repair after the build fails, most often because a real path
# sits at a projected root. The caller exits 3, so `ways manage reinstall` keeps
# the built app.
print_projection_stopped() {
  mark fail "Projection stopped. The app is staged in ${APP_DIR}; the repair of ${DEST} did not finish."
  echo "    Read the error above. When it lists real paths at projected roots, move those"
  echo "    aside (or copy what you want to keep into a project's .claude/skills/)."
  local repair
  repair="$(repair_cmd)"
  echo "    Then run:"
  echo -e "      ${CYAN}${repair}${RESET}            # link the projected roots"
  echo -e "      ${CYAN}${repair} --force${RESET}    # or: rename each real path to <name>.ways-backup-<seconds> first"
  echo "    If ~/.claude is itself a git clone of agent-ways (pre-1.0), do not use --force;"
  echo "    see ${UPSTREAM_URL}/blob/ways-v1.8.3/docs/migration-1.0.md"
}

# Put the built binaries on PATH (~/.local/bin). Symlinks into the stable app dir.
# Uses `if` (not `&&`) and an explicit `return 0` so a missing binary in the last
# loop iteration can't make the function return non-zero and trip `set -e`.
link_path_binaries() {
  mkdir -p "$XDG_BIN"
  local b
  LINKED=0
  for b in $(suite_bins); do
    if [[ -e "$APP_DIR/bin/$b" ]]; then
      ln -sf "$APP_DIR/bin/$b" "$XDG_BIN/$b"
      LINKED=$((LINKED + 1))
    fi
  done
  # claude-projects was a script linked from tools/; `ways projects` replaced
  # it (ADR-504). Remove the link an earlier install made, and only that link.
  if [[ -L "$XDG_BIN/claude-projects" && "$(readlink "$XDG_BIN/claude-projects")" == "$APP_DIR/tools/claude-projects" ]]; then
    rm -f "$XDG_BIN/claude-projects"
  fi
  return 0
}

# The Link stage: link what was built, and say how much that was.
link_stage() {
  section "Link"
  link_path_binaries
  if (( LINKED > 0 )); then
    mark ok "suite linked onto PATH in ${XDG_BIN} (${LINKED} binaries)"
  else
    mark warn "no suite binary was built, so nothing was linked onto PATH"
  fi
}

# Echo the suite binaries from the app's tools/suite-bins, the list the
# Makefile builds and links from.
suite_bins() {
  awk '/^[a-z]/ { print $1 }' "$APP_DIR/tools/suite-bins"
}

# Platform triple for messages (linux-x86_64, darwin-arm64, …).
PLATFORM="$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"

# Did setup produce a functional semantic-matching engine? (ADR-146 verify step.)
# A binary that downloaded/built but won't actually embed is treated as "not
# acquired" — `ways status` reports Engine: embedding only when it truly works.
embedding_engine_ok() {
  [[ -x "$APP_DIR/bin/ways" ]] || return 1
  # Capture first: `grep -q` stops reading at its match, and `ways status`
  # writing on into the closed pipe aborts, which pipefail reports as failure.
  local status
  status="$("$APP_DIR/bin/ways" status 2>/dev/null)" || true
  grep -qiE '^Engine:[[:space:]]*embedding' <<<"$status"
}

# ADR-146 recovery card: shown when the projection is up but semantic matching is
# off (no prebuilt way-embed for this platform / it won't launch / toolchain
# missing). Leads with an agent handoff — Claude Code is, by definition, installed,
# since this is being set up *for* it — including a paste-ready prompt; the exact
# commands are the backstop. This function never builds or installs anything.
print_recovery_card() {
  cat <<CARD
  ${YELLOW}⚠${RESET} Semantic (meaning-based) way-matching is OFF — keyword/pattern ways still fire,
  but the embedding engine (way-embed) isn't working on this platform (${PLATFORM}).

  ${BOLD}Easiest fix — let the agent you're installing this for finish it.${RESET}
  Open Claude Code in ${CYAN}${APP_DIR}${RESET} and paste this:
  ${DIM}────────────────────────────────────────────────────────────────${RESET}
  agent-ways installed but semantic way-matching is off — way-embed isn't
  working on this machine (${PLATFORM}). The app source is at ${APP_DIR}.
  Please finish setup per docs/finish-install.md: check for a C++ toolchain
  (cmake + compiler); if it's missing run \`make deps\` (it uses sudo — ask me
  first); then \`make setup\`; then verify \`ways status\` shows Engine: embedding.
  ${DIM}────────────────────────────────────────────────────────────────${RESET}

  ${BOLD}Or finish it yourself:${RESET}
    cd ${APP_DIR}
    make deps      # only if cmake/compiler is missing — installs the toolchain (sudo)
    make setup     # build way-embed + fetch model + regenerate corpus
  Details: ${CYAN}${APP_DIR}/docs/finish-install.md${RESET}
CARD
}

# The semantic-matching check of ADR-146 as a stage of the report.
check_engine() {
  section "Semantic matching"
  if embedding_engine_ok; then
    mark ok "the embedding engine runs"
  else
    print_recovery_card
  fi
}

# The relevance judge (ADR-196): `ways judge-setup` checks the stored keys and,
# when none passes, says ways is degraded and offers to add one. The installer
# often runs from a pipe, so the offer reads /dev/tty when it can be opened;
# without one the command warns and never prompts.
judge_setup() {
  local ways="$APP_DIR/bin/ways"
  if ! "$ways" judge-setup --help >/dev/null 2>&1; then
    # A ways binary from before judge-setup: warn when no key file exists.
    local keys="${XDG_CONFIG_HOME:-$HOME/.config}/agent-ways/keys"
    if [[ ! -e "$keys/anthropic" && ! -e "$keys/openrouter" ]]; then
      mark warn "Ways is degraded: the relevance judge is off, so every matched way is injected."
      echo -e "    Fix: ${CYAN}ways agent key add --provider anthropic${RESET} (or openrouter)"
    fi
    return 0
  fi
  if { : </dev/tty >/dev/tty; } 2>/dev/null; then
    # Ctrl-C at the prompt skips the offer; it never fails the install.
    ( trap 'exit 0' INT; "$ways" judge-setup </dev/tty >/dev/tty ) || true
  else
    "$ways" judge-setup || true
  fi
}

# --- Clobber: remove the app dir AND ~/.claude (backs ~/.claude up first) ---

if [[ "$CLOBBER" == "true" ]]; then
  BACKUP="${DEST}-backup-$(date +%Y%m%d-%H%M%S)"
  echo -e "${YELLOW}Clobber mode.${RESET} This removes the app dir and ~/.claude."
  if [[ -t 0 ]]; then
    echo -e "  ~/.claude will be backed up to: ${CYAN}${BACKUP}${RESET}"
    echo ""
    read -rp "  Type 'clobber' to confirm: " confirm < /dev/tty
    [[ "$confirm" == "clobber" ]] || { echo -e "  ${GREEN}Aborted.${RESET} Nothing changed."; exit 1; }
    echo ""
  else
    echo -e "  ${YELLOW}Non-interactive clobber.${RESET} Backup: ${BACKUP}"
  fi
  [[ -e "$DEST" ]] && mv "$DEST" "$BACKUP" && echo -e "  Backed up ~/.claude → ${CYAN}${BACKUP}${RESET}"
  [[ -e "$APP_DIR" ]] && rm -rf "$APP_DIR" && echo -e "  Removed app dir ${CYAN}${APP_DIR}${RESET}"
  echo ""
fi

# --- Already installed (native XDG)? Update in place. ---

if is_agent_ways_repo "$APP_DIR"; then
  section "Pull"
  mark info "already installed: the app at ${APP_DIR} updates in place"
  git -C "$APP_DIR" pull --ff-only 2>&1 | indent || {
    mark fail "git pull failed. You may have local changes in the app dir."
    echo "    cd $APP_DIR && git status"
    exit 1
  }
  mark ok "pulled"
  section "Build"
  mark act "make setup"
  if make -C "$APP_DIR" setup 2>&1 | indent; then
    mark ok "built"
  else
    mark warn "the build had issues; ways falls back to pattern and keyword matching"
  fi
  link_stage
  section "Reproject"
  if [[ -x "$APP_DIR/bin/ways" ]]; then
    mark act "reconciling the projection into ${DEST}"
    # The fixed name: `make setup` may install the latest release, which can
    # predate `ways manage` (ADR-509 §4).
    if ! "$APP_DIR/bin/ways" reconcile --source "$APP_DIR" --dest "$DEST" 2>&1 | indent; then
      print_projection_stopped
      exit 3
    fi
    mark ok "projection reconciled"
    check_engine
    section "Judge"
    judge_setup
    section "Done"
    mark ok "Updated. Restart Claude Code for changes to take effect."
  else
    mark warn "ways binary not built, so the projection was NOT reconciled."
    echo "    Fix the build (cd $APP_DIR && make setup), then run: $(repair_cmd)"
  fi
  exit 0
fi

# --- App dir occupied by something that isn't our repo? Stop rather than clobber. ---
# Exception: if we're running FROM the app dir (SRC == APP_DIR), it IS an
# agent-ways checkout (just an unrecognized origin — local clone / fork with gh
# unauthed); fall through to the fresh path, which skips staging in that case.

if [[ -e "$APP_DIR" && "$SRC" != "$APP_DIR" ]]; then
  echo -e "${YELLOW}${APP_DIR} exists but is not an agent-ways clone.${RESET}"
  echo "  Move it aside, or re-run with --dangerously-clobber."
  exit 1
fi

# --- Fresh native install: stage app → build → reconcile projection ---

section "Stage"
if [[ "$SRC" == "$APP_DIR" ]]; then
  mark ok "the source is already the app dir"
else
  mark act "staging the app into ${APP_DIR}"
  mkdir -p "$(dirname "$APP_DIR")"
  git clone "$SRC" "$APP_DIR" 2>&1 | indent || { mark fail "could not stage the app from ${SRC}"; exit 1; }
  # Only rewrite origin when we cloned from a temp/bootstrap source — never
  # clobber the remote of a checkout we were run from. A reinstall names the
  # source its install came from in AGENT_WAYS_ORIGIN (ADR-509).
  git -C "$APP_DIR" remote set-url origin "${AGENT_WAYS_ORIGIN:-$UPSTREAM_URL}" 2>/dev/null || true
  mark ok "staged"
fi
find "$APP_DIR/hooks" -name '*.sh' -exec chmod +x {} + 2>/dev/null || true

section "Build"
mark act "make setup: binaries and the embedding model (about 21MB on first run)"
# Create the engine cache dir first, so the Makefile and the binary resolve the
# same dir.
mkdir -p "${XDG_CACHE_HOME:-$HOME/.cache}/agent-ways/user"
if make -C "$APP_DIR" setup 2>&1 | indent; then
  mark ok "built"
else
  mark warn "the build had issues; ways falls back to pattern and keyword matching"
  echo "    Retry with: cd $APP_DIR && make setup"
fi

# Bootstrap the binaries onto PATH so 'ways' is callable.
link_stage

section "Reproject"
if [[ -x "$APP_DIR/bin/ways" ]]; then
  mark act "reconciling the projection into ${DEST}"
  echo -e "    ${DIM}(symlinks the projected roots; merges settings.json; your files stay; a real directory at a projected root stops it, nothing is deleted)${RESET}"
  mkdir -p "$DEST"
  # The fixed name: `make setup` may install the latest release, which can
  # predate `ways manage` (ADR-509 §4).
  if ! "$APP_DIR/bin/ways" reconcile --source "$APP_DIR" --dest "$DEST" 2>&1 | indent; then
    print_projection_stopped
    exit 3
  fi
  mark ok "projection reconciled"
  # ADR-146 verify: the projection is up, but if semantic matching didn't come
  # up (no prebuilt way-embed for this platform / toolchain missing), guide recovery.
  check_engine
  section "Judge"
  judge_setup
  section "Done"
  mark ok "${DEST} is now a projection of ${APP_DIR}"
  echo "    Restart Claude Code for ways to take effect."
  echo -e "    If ${CYAN}${XDG_BIN}${RESET} isn't on your PATH, add it to your shell rc."
else
  # No binary → don't create an empty ~/.claude. The app is staged; finish by hand.
  mark fail "ways binary not built, so the projection was not created."
  echo "    The application is staged at $APP_DIR. To finish:"
  echo -e "      ${CYAN}cd $APP_DIR && make setup${RESET}      # build the binary"
  echo -e "      ${CYAN}\"$APP_DIR/bin/ways\" reconcile${RESET}   # materialize the projection"
  exit 1
fi
