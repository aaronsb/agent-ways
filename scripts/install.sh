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
#
# The happy path: stage app → make setup → ways reconcile. Everything else is guardrails.

set -euo pipefail

UPSTREAM_REPO="aaronsb/agent-ways"
UPSTREAM_URL="https://github.com/${UPSTREAM_REPO}"
DEST="${HOME}/.claude"

# The application root (1.0 XDG layout). The repo is staged here; ~/.claude is a
# projection of it. Honours XDG_DATA_HOME so sandbox installs stay contained.
XDG_DATA="${XDG_DATA_HOME:-${HOME}/.local/share}"
APP_DIR="${XDG_DATA}/agent-ways"
XDG_BIN="${HOME}/.local/bin"

# --- Colors ---

if [[ -t 1 ]]; then
  RED='\033[0;31m'
  GREEN='\033[0;32m'
  YELLOW='\033[1;33m'
  CYAN='\033[0;36m'
  BOLD='\033[1m'
  DIM='\033[2m'
  RESET='\033[0m'
else
  RED='' GREEN='' YELLOW='' CYAN='' BOLD='' DIM='' RESET=''
fi

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
  4. Reconciles the projection into ~/.claude ('ways reconcile')
     — your Claude Code files (projects/, credentials, settings) are preserved

${CYAN}Already have a pre-1.0 in-place clone at ~/.claude?${RESET}
  Migrate it to the 1.0 layout — see
  ${UPSTREAM_URL}/blob/main/docs/migration-1.0.md

HELP
}

# --- Flag parsing ---

BOOTSTRAP=false
CLOBBER=false

for arg in "$@"; do
  case "$arg" in
    --bootstrap) BOOTSTRAP=true ;;
    --dangerously-clobber) CLOBBER=true ;;
    --help|-h) show_help; exit 0 ;;
  esac
done

# No flags at all → show help
if [[ "$BOOTSTRAP" == "false" ]] && [[ ! -f "hooks/check-config-updates.sh" ]]; then
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

# --- Self-bootstrap ---

if [[ "$BOOTSTRAP" == "true" ]]; then
  check_prereqs

  echo ""
  echo -e "${BOLD}agent-ways bootstrap${RESET}"
  echo -e "Fetching latest from ${CYAN}${UPSTREAM_REPO}${RESET}..."
  echo ""

  BOOTSTRAP_DIR=$(mktemp -d)
  trap 'rm -rf "$BOOTSTRAP_DIR"' EXIT

  if ! git clone --depth 1 "$UPSTREAM_URL" "$BOOTSTRAP_DIR/agent-ways" 2>&1; then
    echo -e "${RED}Failed to clone ${UPSTREAM_URL}${RESET}"
    exit 1
  fi

  CLONE="$BOOTSTRAP_DIR/agent-ways"

  # Verify the clone
  if [[ ! -f "$CLONE/hooks/check-config-updates.sh" ]]; then
    echo -e "${RED}Clone doesn't look like agent-ways.${RESET}"
    exit 1
  fi

  CLONE_HEAD=$(git -C "$CLONE" log --oneline -1 2>/dev/null)
  echo -e "Verified: ${DIM}${CLONE_HEAD}${RESET}"
  echo ""

  # Forward flags (minus --bootstrap) to the verified copy
  FORWARD_ARGS=()
  for arg in "$@"; do
    [[ "$arg" != "--bootstrap" ]] && FORWARD_ARGS+=("$arg")
  done

  cd "$CLONE"
  # Mark the inner run as bootstrapped so its conflict-options reference the
  # re-runnable curl one-liner, not this temp clone (deleted by the EXIT trap).
  AGENT_WAYS_BOOTSTRAPPED=1 bash "$CLONE/scripts/install.sh" "${FORWARD_ARGS[@]}"
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

echo ""
echo -e "${BOLD}agent-ways installer${RESET} ${DIM}(1.0 XDG projection)${RESET}"
echo -e "Source:      ${CYAN}${SRC}${RESET}"
echo -e "Application: ${CYAN}${APP_DIR}${RESET}"
echo -e "Projection:  ${CYAN}${DEST}${RESET}"
echo ""

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

# Put the built binaries on PATH (~/.local/bin). Symlinks into the stable app dir.
# Uses `if` (not `&&`) and an explicit `return 0` so a missing binary in the last
# loop iteration can't make the function return non-zero and trip `set -e`.
link_path_binaries() {
  mkdir -p "$XDG_BIN"
  local b
  for b in ways attend attend-chat; do
    if [[ -e "$APP_DIR/bin/$b" ]]; then
      ln -sf "$APP_DIR/bin/$b" "$XDG_BIN/$b"
    fi
  done
  return 0
}

# Platform triple for messages (linux-x86_64, darwin-arm64, …).
PLATFORM="$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"

# Did setup produce a functional semantic-matching engine? (ADR-146 verify step.)
# A binary that downloaded/built but won't actually embed is treated as "not
# acquired" — `ways status` reports Engine: embedding only when it truly works.
embedding_engine_ok() {
  [[ -x "$APP_DIR/bin/ways" ]] || return 1
  "$APP_DIR/bin/ways" status 2>/dev/null | grep -qiE '^Engine:[[:space:]]*embedding'
}

# ADR-146 recovery card: shown when the projection is up but semantic matching is
# off (no prebuilt way-embed for this platform / it won't launch / toolchain
# missing). Leads with an agent handoff — Claude Code is, by definition, installed,
# since this is being set up *for* it — including a paste-ready prompt; the exact
# commands are the backstop. This function never builds or installs anything.
print_recovery_card() {
  cat <<CARD

${YELLOW}⚠ Semantic (meaning-based) way-matching is OFF${RESET} — keyword/pattern ways still fire,
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
  # If ~/.claude is itself a pre-1.0 in-place clone, reconciling over it would
  # symlink projection roots on top of a live repo — that's a migration, not an
  # update. Route to the gated migrator instead.
  if is_agent_ways_repo "$DEST"; then
    echo -e "${YELLOW}App is installed at ${APP_DIR}, but ~/.claude is still a pre-1.0 in-place clone.${RESET}"
    echo "  Migrate it rather than reconcile over it (gated, backs up first):"
    echo -e "    ${CYAN}ways migrate --what-if && ways migrate --execute${RESET}"
    echo -e "  Guide: ${CYAN}${UPSTREAM_URL}/blob/main/docs/migration-1.0.md${RESET}"
    exit 1
  fi

  echo -e "${GREEN}Already installed${RESET} (app at ${CYAN}${APP_DIR}${RESET}). Updating..."
  echo ""
  git -C "$APP_DIR" pull --ff-only 2>&1 || {
    echo ""
    echo -e "${YELLOW}git pull failed.${RESET} You may have local changes in the app dir."
    echo "  cd $APP_DIR && git status"
    exit 1
  }
  echo ""
  echo -e "Rebuilding (${CYAN}make setup${RESET})..."
  make -C "$APP_DIR" setup || true
  link_path_binaries
  echo ""
  if [[ -x "$APP_DIR/bin/ways" ]]; then
    echo -e "Reconciling projection → ${CYAN}${DEST}${RESET}..."
    "$APP_DIR/bin/ways" reconcile --source "$APP_DIR" --dest "$DEST" || true
    echo ""
    echo -e "${GREEN}Updated.${RESET} Restart Claude Code for changes to take effect."
    embedding_engine_ok || print_recovery_card
  else
    echo -e "${YELLOW}ways binary not built — projection was NOT reconciled.${RESET}"
    echo "  Fix the build (cd $APP_DIR && make setup), then run: ways reconcile"
  fi
  exit 0
fi

# --- Legacy pre-1.0 in-place clone at ~/.claude? Point to the gated migrator. ---

if is_agent_ways_repo "$DEST"; then
  echo -e "${YELLOW}You have a pre-1.0 in-place agent-ways clone at ~/.claude.${RESET}"
  echo ""
  echo "  1.0 moved agent-ways to an XDG application projected into ~/.claude."
  echo "  Migrate your install to the new layout (gated, backs up first):"
  echo ""
  echo -e "    ${CYAN}cd ~/.claude && git pull && make update${RESET}   # get the 1.0 ways binary"
  echo -e "    ${CYAN}ways migrate --what-if${RESET}                    # preview (read-only)"
  echo -e "    ${CYAN}ways migrate --execute${RESET}                    # convert (backs up first)"
  echo ""
  echo -e "  Guide: ${CYAN}${UPSTREAM_URL}/blob/main/docs/migration-1.0.md${RESET}"
  echo ""
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

echo -e "Staging application → ${CYAN}${APP_DIR}${RESET}..."
if [[ "$SRC" == "$APP_DIR" ]]; then
  echo -e "${GREEN}Source is already the app dir.${RESET}"
else
  mkdir -p "$(dirname "$APP_DIR")"
  git clone "$SRC" "$APP_DIR" 2>&1
  # Only rewrite origin when we cloned from a temp/bootstrap source — never
  # clobber the remote of a checkout we were run from.
  git -C "$APP_DIR" remote set-url origin "$UPSTREAM_URL" 2>/dev/null || true
fi
find "$APP_DIR/hooks" -name '*.sh' -exec chmod +x {} + 2>/dev/null || true

echo ""
echo -e "Building binaries + embedding model (${CYAN}make setup${RESET})..."
echo -e "${DIM}(downloads ~21MB model + pre-built binary on first run)${RESET}"
echo ""
make -C "$APP_DIR" setup || {
  echo ""
  echo -e "${YELLOW}Build had issues.${RESET} Ways will fall back to pattern/keyword matching."
  echo "  Retry with: cd $APP_DIR && make setup"
  echo ""
}

# Bootstrap the binaries onto PATH so 'ways' is callable.
link_path_binaries

echo ""
if [[ -x "$APP_DIR/bin/ways" ]]; then
  echo -e "Reconciling projection → ${CYAN}${DEST}${RESET}..."
  echo -e "${DIM}(symlinks the projected trees; merges settings.json; your Claude Code files stay)${RESET}"
  echo ""
  mkdir -p "$DEST"
  "$APP_DIR/bin/ways" reconcile --source "$APP_DIR" --dest "$DEST"
  echo ""
  echo -e "${BOLD}Done.${RESET} ~/.claude is now a projection of ${DIM}${APP_DIR}${RESET}"
  echo ""
  echo "  Restart Claude Code for ways to take effect."
  echo -e "  If ${CYAN}${XDG_BIN}${RESET} isn't on your PATH, add it to your shell rc."
  echo ""
  # ADR-146 verify: the projection is up, but if semantic matching didn't come
  # up (no prebuilt way-embed for this platform / toolchain missing), guide recovery.
  embedding_engine_ok || print_recovery_card
else
  # No binary → don't create an empty ~/.claude. The app is staged; finish by hand.
  echo -e "${YELLOW}ways binary not built — projection not created.${RESET}"
  echo "  The application is staged at $APP_DIR. To finish:"
  echo -e "    ${CYAN}cd $APP_DIR && make setup${RESET}      # build the binary"
  echo -e "    ${CYAN}\"$APP_DIR/bin/ways\" reconcile${RESET}   # materialize the projection"
  echo ""
  exit 1
fi
