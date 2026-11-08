#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# PostToolUse on the agent-ways MCP tool ways_read (ADR-701 §5): record the pull
# as a disclosure to the agent that made it. The MCP server serves the body but
# is shared by a session's subagents and cannot tell them apart; this payload
# names the calling agent, so the stamp lands on it.

source "$(dirname "$0")/require-ways.sh"
ways_hook pull
