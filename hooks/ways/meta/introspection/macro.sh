#!/usr/bin/env bash
# Inject context budget facts into introspection way

# `ways` exports the WAYS_CONTEXT_* budget to a macro that names it.
if [[ -n "${WAYS_CONTEXT_REMAINING:-}" ]]; then
    REMAINING=$WAYS_CONTEXT_REMAINING
    PCT=$WAYS_CONTEXT_PCT_REMAINING
    USED=$WAYS_CONTEXT_USED

    echo "**Context budget: ~${USED} tokens used, ~${REMAINING} remaining (${PCT}% of window).**"
    if [[ $PCT -le 25 ]]; then
      echo "After compaction, this conversation's history is summarized and details are lost. Any corrections or guidance the human gave this session that aren't captured as ways will not survive. This is the window to act on that."
    elif [[ $PCT -le 50 ]]; then
      echo "There is room to work, but this is a good time to capture session learnings. Doing it now means the introspection gets full context rather than a post-compaction summary."
    fi
    echo ""
fi
