#!/usr/bin/env bash
# PreToolUse Task: phase 1 of subagent injection. The binary matches the
# delegation prompt against subagent-scoped ways and stashes the matches
# for SubagentStart (inject-subagent.sh). A Task naming an agent with its
# own definition (project, user or plugin) is skipped: its .md is its
# constitution.

source "$(dirname "$0")/require-ways.sh"
ways_hook task
