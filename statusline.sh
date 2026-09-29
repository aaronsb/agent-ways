#!/usr/bin/env bash
# statusline.sh — the agent-ways reference status line for Claude Code.
#
# Use it as is: point settings.json at it.
#   "statusLine": { "type": "command", "command": "${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/statusline.sh" }
#
# Build on it: source it from your own script and call the segments you want.
#   source "${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/statusline.sh"
#   echo "$(sl_agent)$(sl_dir) | my own stuff"
#
# Sourcing defines the sl_* functions and prints nothing. Each segment prints
# its text with a trailing space, or nothing when it has nothing to show, so
# segments concatenate without separators to manage. Segments return 0, so a
# caller under `set -euo pipefail` survives a failed or timed-out command.
# See docs/reference/statusline.md.

SL_TIMEOUT=${SL_TIMEOUT:-1}

# Run a command under SL_TIMEOUT seconds. macOS ships no `timeout`; coreutils
# from Homebrew installs it as `gtimeout`. Without either, run unbounded.
sl__run() {
    if command -v timeout > /dev/null 2>&1; then
        timeout "$SL_TIMEOUT" "$@"
    elif command -v gtimeout > /dev/null 2>&1; then
        gtimeout "$SL_TIMEOUT" "$@"
    else
        "$@"
    fi
}

# This session's attend display name, or nothing when attend is absent.
# CLI is the contract: read sanctioned attend surfaces, never attend-owned
# state. `attend whoami` derives the name from the session record's origin
# path (ADR-171), so it stays stable when the shell's cwd wanders.
# `--display` prints the name alone; attend builds before it get the name
# parsed from the whoami table. Builds before `whoami` show no name. A
# timeout (124) skips the fallback, so a hung attend costs one SL_TIMEOUT.
sl_agent_name() {
    command -v attend > /dev/null 2>&1 || return 0
    local name rc=0
    name=$(sl__run attend whoami --display 2>/dev/null) || rc=$?
    if [[ -z "$name" && $rc -ne 124 ]]; then
        local esc
        esc=$(printf '\033')
        name=$(sl__run attend whoami 2>/dev/null \
            | sed "s/${esc}\[[0-9;]*m//g" \
            | awk '/^ *display/{print $2; exit}') || name=""
    fi
    printf '%s' "$name"
}

sl_agent() {
    local name
    name=$(sl_agent_name)
    [[ -n "$name" ]] && printf '🤖 %s ' "$name"
    return 0
}

sl_dir() {
    printf '📁 %s ' "$(basename "$(pwd)")"
}

# Branch, with `*` when the tree has uncommitted changes.
sl_git() {
    sl__run git rev-parse --git-dir > /dev/null 2>&1 || return 0
    local branch
    branch=$(sl__run git branch --show-current 2>/dev/null) || branch=""
    [[ -n "$branch" ]] || branch=$(sl__run git rev-parse --short HEAD 2>/dev/null) || branch=""
    [[ -n "$branch" ]] || return 0
    if sl__run git status --porcelain 2>/dev/null | grep -q .; then
        branch="$branch*"
    fi
    printf '🔀 %s ' "$branch"
}

# owner/repo of the origin remote, from an SSH or HTTPS URL. Credentials in
# the URL are dropped, and a URL that does not parse shows nothing rather
# than printing raw.
sl_remote() {
    local url repo
    url=$(sl__run git remote get-url origin 2>/dev/null) || return 0
    repo=$(printf '%s' "$url" \
        | sed -E 's#^[A-Za-z0-9+.-]+://[^@/]*@#//#; s#/+$##; s#\.git$##' \
        | sed -nE 's#.*[/:]([^/:@]+/[^/:@]+)$#\1#p')
    [[ -n "$repo" ]] || return 0
    printf '📡 %s ' "$repo"
}

sl_time() {
    printf '🕐 %s ' "$(date +%H:%M)"
}

statusline_render() {
    local right
    right=$(sl_time)
    printf '%s| %s\n' "$(sl_agent)$(sl_dir)$(sl_git)$(sl_remote)" "${right% }"
}

# Render only when executed. Sourcing defines the functions and stops here.
if [[ "${BASH_SOURCE[0]-}" == "$0" ]]; then
    statusline_render
fi
