#!/usr/bin/env bash
# Inject context budget into the task list checkpoint

# `ways` exports the WAYS_CONTEXT_* budget to a macro that names it.
if [[ -n "${WAYS_CONTEXT_REMAINING:-}" ]]; then
  echo "**Context budget: ~${WAYS_CONTEXT_REMAINING} tokens remaining (${WAYS_CONTEXT_PCT_REMAINING}% of window).**"
  echo ""
fi
