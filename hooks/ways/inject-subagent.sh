#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-FileCopyrightText: 2026 Jonathan Muse
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# SubagentStart: phase 2 of subagent injection. The binary claims the
# oldest stash check-task-pre.sh wrote and injects its ways, fresh,
# whatever the parent session has already been shown.

source "$(dirname "$0")/require-ways.sh"
ways_hook subagent-start
