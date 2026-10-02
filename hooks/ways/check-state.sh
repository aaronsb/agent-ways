#!/usr/bin/env bash
# SessionStart and UserPromptSubmit: state triggers (context-threshold,
# file-exists, session-start) and the core guidance safety net.

source "$(dirname "$0")/require-ways.sh"
ways_hook state
