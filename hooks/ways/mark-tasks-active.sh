#!/usr/bin/env bash
# PreToolUse TaskCreate: mark the session's task list active, which
# silences the context-threshold nag.

source "$(dirname "$0")/require-ways.sh"
ways_hook tasks-active
