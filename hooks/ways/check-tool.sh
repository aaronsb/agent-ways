#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-FileCopyrightText: 2026 Clayton Chancey
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# PreToolUse and PostToolUse on the tools settings.json wires: match the tool name against the ways (ADR-704).

source "$(dirname "$0")/require-ways.sh"
ways_hook tool
