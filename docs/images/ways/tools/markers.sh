#!/usr/bin/env bash
# Fire a few ways through the real scan lanes for the infra project's
# session, so `ways session ways` has session markers to read. shots.sh runs
# it after fixture.py.
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
fixture_env
mkdir -p "$XDG_RUNTIME_DIR"
S=71d3e5a2-9f0b-4c86-a4d7-3e1b8c0f6a29
T="$F/home/.claude/projects/-home-dev-infra/$S.jsonl"
P=/home/dev/infra
scan() { ways scan "$@" --session $S --project $P --transcript "$T" >/dev/null; }
scan file --path $P/docs/architecture/ADR-012-cart.md
scan command --command 'git commit -m "fix cart total"'
scan command --command 'npm install stripe'
scan file --path $P/src/cart.test.ts
scan command --command 'gh pr create --fill'
scan file --path $P/Dockerfile
scan command --command 'cargo test'
