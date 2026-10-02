#!/usr/bin/env bash
# SubagentStart: phase 2 of subagent injection. The binary claims the
# oldest stash check-task-pre.sh wrote and injects its ways, fresh,
# whatever the parent session has already been shown.

source "$(dirname "$0")/require-ways.sh"
ways_hook subagent-start
