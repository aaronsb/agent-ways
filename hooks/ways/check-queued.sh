#!/usr/bin/env bash
# PostToolUse: match operator messages queued mid-turn (ADR-161). They
# never fire UserPromptSubmit; the binary reads them from the transcript.

source "$(dirname "$0")/require-ways.sh"
ways_hook queued
