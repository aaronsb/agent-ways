#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-FileCopyrightText: 2026 Jonathan Muse
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# PreToolUse TaskCreate: mark the session's task list active, which
# silences the context-threshold nag.

source "$(dirname "$0")/require-ways.sh"
ways_hook tasks-active
