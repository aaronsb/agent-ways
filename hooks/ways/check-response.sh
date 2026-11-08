#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-FileCopyrightText: 2026 Jonathan Muse
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Stop: record Claude's last response for the next prompt's embed lane
# (ADR-155 §3), or clear the record when the turn ended without text.

source "$(dirname "$0")/require-ways.sh"
ways_hook stop
