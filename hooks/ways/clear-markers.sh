#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-FileCopyrightText: 2026 Jonathan Muse
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# SessionStart (startup, compact, clear): clear this session's state, as
# `ways session reset` does, and log the session start.

source "$(dirname "$0")/require-ways.sh"
ways_hook session-start
