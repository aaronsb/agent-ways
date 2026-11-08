#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# PostToolUse: match operator messages queued mid-turn (ADR-161). They
# never fire UserPromptSubmit; the binary reads them from the transcript.

source "$(dirname "$0")/require-ways.sh"
ways_hook queued
