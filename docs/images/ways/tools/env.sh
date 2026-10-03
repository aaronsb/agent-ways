# Shared setup for the screenshot scripts beside it. Sourced, not run.
#
# Paths: REPO is the checkout; BIN holds the debug `ways`, `ways-agent` and
# `tui-harness` (build them with
#   cargo build --manifest-path tools/Cargo.toml -p ways -p ways-agent -p tui-harness
# and set CARGO_TARGET_DIR if you build elsewhere); WORK is the scratch tree,
# under the git-ignored target/ unless SHOTS_DIR names another.
TOOLS=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$TOOLS/../../../.." && pwd)
BIN=${CARGO_TARGET_DIR:-$REPO/tools/target}/debug
WORK=${SHOTS_DIR:-$REPO/target/doc-shots}
F=$WORK/fx
REALHOME=$HOME
mkdir -p "$WORK"

# The app sees only the fixture. HOME is the relative path `~`, a link to
# the fixture home in $F, so paths under it print as ~/... on screen; config,
# state, cache and data fall back under it.
fixture_env() {
  cd "$F"
  ln -sfn home "$F/~"
  export HOME='~'
  unset XDG_STATE_HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME
  export XDG_RUNTIME_DIR=$F/run
  export PATH=$BIN:/usr/bin:/bin
  export TERM=xterm-256color COLORTERM=truecolor
  unset CLAUDE_CONFIG_DIR CLAUDE_PROJECT_DIR CLAUDE_CODE_SESSION_ID OPENROUTER_API_KEY ANTHROPIC_API_KEY
}

# tui-harness with this tree's state root. A shot runs with the real HOME so
# fontconfig finds the user's fonts.
TH="$BIN/tui-harness --dir $WORK/th"
