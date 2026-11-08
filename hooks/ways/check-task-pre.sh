#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-FileCopyrightText: 2026 Jonathan Muse
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# PreToolUse Task: phase 1 of subagent injection. The binary matches the
# delegation prompt against subagent-scoped ways and stashes the matches
# for SubagentStart (inject-subagent.sh). A Task naming an agent with its
# own definition (project, user or plugin) is skipped: its .md is its
# constitution.

source "$(dirname "$0")/require-ways.sh"
ways_hook task
