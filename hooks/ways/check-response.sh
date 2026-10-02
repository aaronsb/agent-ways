#!/usr/bin/env bash
# Stop: record Claude's last response for the next prompt's embed lane
# (ADR-155 §3), or clear the record when the turn ended without text.

source "$(dirname "$0")/require-ways.sh"
ways_hook stop
