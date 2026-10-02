#!/usr/bin/env bash
# PostToolUse and PostToolUseFailure: reactive firing (ADR-123 Decision 5).
# The binary runs each way's postcheck.sh with the payload on stdin; a way
# whose postcheck exits 0 fires through the same gate as predictive firing.

source "$(dirname "$0")/require-ways.sh"
ways_hook post-tool
