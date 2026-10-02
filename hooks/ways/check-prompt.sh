#!/usr/bin/env bash
# UserPromptSubmit: match the prompt against the ways. The binary keyword-
# matches the prompt and embeds it with Claude's last response, which the
# Stop hook recorded (ADR-155 §3); size bounding is its reducer (ADR-130).

source "$(dirname "$0")/require-ways.sh"
ways_hook prompt
