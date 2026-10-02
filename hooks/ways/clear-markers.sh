#!/usr/bin/env bash
# SessionStart (startup, compact, clear): clear this session's state, as
# `ways session reset` does, and log the session start.

source "$(dirname "$0")/require-ways.sh"
ways_hook session-start
