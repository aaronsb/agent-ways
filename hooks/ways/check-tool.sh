#!/usr/bin/env bash
# PreToolUse and PostToolUse on the tools settings.json wires: match the tool name against the ways (ADR-704).

source "$(dirname "$0")/require-ways.sh"
ways_hook tool
