#!/usr/bin/env bash
# Install agent-ways into ~/.claude
#
# This is a gateway, not a butler. It checks readiness, detects complexity,
# and either proceeds with a clean install or tells you what to sort out first.
#
# Usage:
#   scripts/install.sh                           # install from repo root
#   scripts/install.sh --bootstrap               # clone latest + install
#   curl ... | bash -s -- --bootstrap            # self-bootstrap from internet
#   scripts/install.sh --dangerously-clobber     # overwrite existing ~/.claude/
#
# The happy path: clone → make setup. Everything else is guardrails.

set -euo pipefail

UPSTREAM_REPO="aaronsb/agent-ways"
UPSTREAM_URL="https://github.com/${UPSTREAM_REPO}"
DEST="${HOME}/.claude"

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
  --dangerously-clobber     Overwrite existing ~/.claude/ (backs up first)
  --help                    Show this help

${CYAN}What it does:${RESET}
  1. Checks prerequisites (git, jq, make)
  2. Detects existing ~/.claude/ state
  3. Clones into ~/.claude/ (or tells you what to sort out first)
  4. Runs 'make setup' for semantic matching engine

${CYAN}If you already have ~/.claude/ files:${RESET}
  See ${UPSTREAM_URL}/blob/main/docs/install-guide.md
  for how to prepare, back up, or merge your existing config.

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
echo -e "${BOLD}agent-ways installer${RESET}"
echo -e "Source: ${CYAN}${SRC}${RESET}"
echo -e "Target: ${CYAN}${DEST}${RESET}"
echo ""

# --- Detect existing ~/.claude/ state ---

if [[ -d "$DEST" ]]; then
  HAS_GIT=false
  HAS_FILES=false
  IS_US=false

  [[ -d "$DEST/.git" ]] && HAS_GIT=true
  [[ -n "$(ls -A "$DEST" 2>/dev/null)" ]] && HAS_FILES=true

  # Check if it's already our repo
  if [[ "$HAS_GIT" == "true" ]]; then
    REMOTE_URL=$(git -C "$DEST" remote get-url origin 2>/dev/null || true)
    OWNER_REPO=$(echo "$REMOTE_URL" | sed -E 's#.*github\.com[:/]##; s/\.git$//')
    if [[ "$OWNER_REPO" == "$UPSTREAM_REPO" ]]; then
      IS_US=true
    fi
    # Also check if it's a fork
    if [[ "$IS_US" == "false" ]] && command -v gh &>/dev/null; then
      PARENT=$(gh api "repos/${OWNER_REPO}" --jq '.parent.full_name' 2>/dev/null || true)
      [[ "$PARENT" == "$UPSTREAM_REPO" ]] && IS_US=true
    fi
  fi

  # --- Already installed: update path ---
  if [[ "$IS_US" == "true" ]]; then
    echo -e "${GREEN}Already installed.${RESET} Updating..."
    echo ""
    echo -e "  ${DIM}cd ~/.claude && git pull${RESET}"
    cd "$DEST"
    git pull --ff-only 2>&1 || {
      echo ""
      echo -e "${YELLOW}git pull failed.${RESET} You may have local changes."
      echo "  cd ~/.claude && git status"
      echo "  Resolve conflicts, then: make setup"
      exit 1
    }

    echo ""
    echo -e "Running ${CYAN}make setup${RESET} for semantic matching..."
    if [[ -f "$DEST/Makefile" ]]; then
      make -C "$DEST" setup || true
    fi

    echo ""
    echo -e "${GREEN}Updated.${RESET} Restart Claude Code for changes to take effect."
    exit 0
  fi

  # --- Existing files: complexity detected ---
  if [[ "$CLOBBER" == "false" ]]; then
    echo -e "${YELLOW}~/.claude/ already exists and isn't an agent-ways install.${RESET}"
    echo ""

    if [[ "$HAS_GIT" == "true" ]]; then
      echo -e "  Found: ${BOLD}.git/${RESET} directory (existing git tracking)"
      echo -e "  Remote: ${DIM}$(git -C "$DEST" remote get-url origin 2>/dev/null || echo 'none')${RESET}"
    fi

    if [[ "$HAS_FILES" == "true" ]]; then
      local_files=$(find "$DEST" -maxdepth 1 -type f | head -5)
      echo -e "  Found: existing files"
      echo "$local_files" | while read -r f; do
        echo -e "    ${DIM}$(basename "$f")${RESET}"
      done
      more_count=$(find "$DEST" -maxdepth 1 -type f | wc -l)
      if (( more_count > 5 )); then
        echo -e "    ${DIM}... and $((more_count - 5)) more${RESET}"
      fi
    fi

    # Re-runnable invocation. Under --bootstrap, $0 is an ephemeral temp clone
    # that the EXIT trap deletes — so the options must reference the curl
    # one-liner that re-fetches, not a path that won't exist when pasted.
    if [[ "${AGENT_WAYS_BOOTSTRAPPED:-}" == "1" ]]; then
      RERUN="curl -sL https://raw.githubusercontent.com/${UPSTREAM_REPO}/main/scripts/install.sh | bash -s -- --bootstrap"
    else
      RERUN="$0"
    fi

    echo ""
    echo -e "${BOLD}You need to decide what to do with these files before installing.${RESET}"
    echo ""
    echo "  Options:"
    echo -e "    1. Install alongside (keeps your config — ${BOLD}recommended${RESET}):"
    echo -e "       ${CYAN}git clone ${UPSTREAM_URL} ~/.claude/agent-ways${RESET}"
    echo -e "       ${CYAN}cd ~/.claude/agent-ways && make setup && make sync-to-home${RESET}"
    echo "       Keeps your sessions, credentials, and settings; projects ways in"
    echo "       (subdirectory topology — update with: git pull && make sync-to-home)."
    echo ""
    echo "    2. Back up and clobber:"
    echo -e "       ${CYAN}${RERUN} --dangerously-clobber${RESET}"
    echo "       (backs up to ~/.claude-backup-YYYYMMDD/ first)"
    echo ""
    echo "    3. Merge manually (if you have custom config):"
    echo -e "       See ${CYAN}${UPSTREAM_URL}/blob/main/docs/install-guide.md${RESET}"
    echo ""
    echo "    4. Start fresh:"
    echo -e "       ${CYAN}mv ~/.claude ~/.claude-old && ${RERUN}${RESET}"
    echo ""
    exit 1
  fi

  # --- Clobber path (--dangerously-clobber) ---
  BACKUP="${DEST}-backup-$(date +%Y%m%d-%H%M%S)"
  echo -e "${YELLOW}Clobber mode.${RESET} Backing up to ${CYAN}${BACKUP}${RESET}"
  echo ""

  # Confirmation gate — interactive requires typing "clobber", non-interactive warns loudly
  if [[ -t 0 ]]; then
    echo -e "  This will ${RED}replace${RESET} everything in ~/.claude/ with a fresh install."
    echo -e "  Your backup will be at: ${BACKUP}"
    echo ""
    read -rp "  Type 'clobber' to confirm: " confirm < /dev/tty
    if [[ "$confirm" != "clobber" ]]; then
      echo -e "  ${GREEN}Aborted.${RESET} Nothing changed."
      exit 1
    fi
    echo ""
  else
    echo -e "  ${YELLOW}WARNING: Non-interactive clobber.${RESET} Backing up and replacing ~/.claude/"
    echo -e "  Backup: ${BACKUP}"
    echo ""
  fi

  mv "$DEST" "$BACKUP"
  echo -e "  Backed up to ${CYAN}${BACKUP}${RESET}"
fi

# --- Fresh install ---

echo -e "Cloning into ${CYAN}~/.claude/${RESET}..."
echo ""

if [[ "$SRC" == "$DEST" ]]; then
  # We're already in place (shouldn't happen, but be safe)
  echo -e "${GREEN}Source is already ~/.claude/.${RESET}"
else
  git clone "$SRC" "$DEST" 2>&1
fi

# Set remote to upstream (source might be a temp dir from bootstrap)
git -C "$DEST" remote set-url origin "$UPSTREAM_URL" 2>/dev/null || true

# Make hooks executable
find "$DEST/hooks" -name '*.sh' -exec chmod +x {} + 2>/dev/null || true

echo ""
echo -e "${GREEN}Installed.${RESET}"
echo ""

# --- Post-install: semantic matching setup ---

echo -e "Setting up semantic matching engine..."
echo -e "${DIM}(downloads ~21MB model + pre-built binary on first run)${RESET}"
echo ""

if [[ -f "$DEST/Makefile" ]]; then
  make -C "$DEST" install || {
    echo ""
    echo -e "${YELLOW}Semantic matching setup had issues.${RESET}"
    echo "Ways will only fire on explicit pattern/commands triggers until the"
    echo "embedding engine is installed. Retry later with:"
    echo "  cd ~/.claude && make setup"
    echo ""
  }
fi

echo ""
echo -e "${BOLD}Done.${RESET}"
echo ""
echo "  Restart Claude Code for ways to take effect."
echo "  Review hooks at: ~/.claude/hooks/"
echo ""
echo -e "  ${DIM}Tip: cd ~/.claude && make test${RESET}"
echo ""
