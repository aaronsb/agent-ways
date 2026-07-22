#!/usr/bin/env bash

# Timeout for all operations (1 second max)
TIMEOUT=1

# Get current directory (basename only for brevity)
DIR=$(basename "$(pwd)")

# Get current time
TIME=$(date +%H:%M)

# Get attend agent name if this session is enrolled (attend running).
# CLI is the contract: read the sanctioned `attend peers` surface rather than
# attend-owned state. The (self) row's Agent column holds our nickname.
AGENT_INFO=""
if command -v attend > /dev/null 2>&1; then
    ESC=$(printf '\033')
    AGENT_NAME=$(timeout $TIMEOUT attend peers 2>/dev/null \
        | sed "s/${ESC}\[[0-9;]*m//g" \
        | awk '/\(self\)/{print $2; exit}')
    if [[ -n "$AGENT_NAME" ]]; then
        AGENT_INFO="🤖 $AGENT_NAME "
    fi
fi

# Get git branch and remote if in a git repo
GIT_INFO=""
REMOTE_INFO=""
if timeout $TIMEOUT git rev-parse --git-dir > /dev/null 2>&1; then
    BRANCH=$(timeout $TIMEOUT git branch --show-current 2>/dev/null || timeout $TIMEOUT git rev-parse --short HEAD 2>/dev/null)
    # Check for uncommitted changes (skip if timeout)
    if timeout $TIMEOUT git status --porcelain 2>/dev/null | grep -q .; then
        GIT_INFO=" 🔀 $BRANCH*"
    else
        GIT_INFO=" 🔀 $BRANCH"
    fi

    # Get remote URL and extract owner/repo
    REMOTE_URL=$(timeout $TIMEOUT git remote get-url origin 2>/dev/null)
    if [[ -n "$REMOTE_URL" ]]; then
        # Extract owner/repo from URL (works for both SSH and HTTPS)
        REMOTE_REPO=$(echo "$REMOTE_URL" | sed -E 's#.*/([^/]+/[^/]+)(\.git)?$#\1#' | sed 's/\.git$//')
        REMOTE_INFO=" 📡 $REMOTE_REPO"
    fi
fi

# Combine all elements
echo "${AGENT_INFO}📁 $DIR$GIT_INFO$REMOTE_INFO | 🕐 $TIME"
