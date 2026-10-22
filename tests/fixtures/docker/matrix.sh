#!/usr/bin/env bash
# Tier 2 across a matrix of arms, models and trials: `make test-live-matrix`.
#
# An arm is a clone of this repo at one branch, so an ablation
# compares the corpus with and without a change. Each run is one tier 2
# invocation of that arm's own test-live.sh, with one model.
#
#   MATRIX_ARMS     space-separated NAME=PATH pairs; NAME matches
#                   [A-Za-z0-9][A-Za-z0-9_-]*. The same PATH may appear
#                   under two names to measure run-to-run noise (A=x A2=x B=y)
#   MATRIX_MODELS   space-separated model ids, no `/` (default: the tier 2
#                   default, keyed `default`)
#   MATRIX_TRIALS   trials per arm and model (default 1); MODEL=N entries in
#                   MATRIX_MODEL_TRIALS override it per model (MODEL is
#                   `default` when MATRIX_MODELS is unset)
#   MATRIX_OUT      root for transcripts; a run writes MATRIX_OUT/<model>/<arm>/t<k>
#   MATRIX_JOBS     runs at once (default 1). Runs start trial-major, every arm
#                   of a model before the next model, and every run of a trial
#                   finishes before the next trial starts, so arms share
#                   conditions
#   TIER2_SCENARIOS, TIER2_MAX_TURNS, ANTHROPIC_API_KEY_FILE, WAYS_BINARIES
#                   passed to each run unchanged
#
# Concurrent runs share the fixture image tag, which each arm builds from its
# own Dockerfile. Arms whose fixture Dockerfiles differ must not run at once
# (MATRIX_JOBS=1); arms that differ only in the corpus are unaffected.
#
# A run that exits 0 or 1 (the tier passed, or a rubric or tier failed) keeps a
# `done` marker holding pass or fail. Any other exit (a preflight problem, a
# signal) leaves an `error` file with the code and no marker. Rerunning the
# same matrix skips the marked runs and reruns the rest from an empty dir.
# Each run's console output is in run.log beside it. The exit is 0 only when
# every run passed.
#
# The container writes as uid 1000. A host user with another uid cannot clear
# the scenario dirs a run left; such a run is reported and not rerun until its
# dir is removed by hand.
set -uo pipefail

[[ -n "${MATRIX_ARMS:-}" ]] || { echo "MATRIX_ARMS is required (NAME=PATH ...)" >&2; exit 2; }
[[ -n "${MATRIX_OUT:-}" ]] || { echo "MATRIX_OUT is required" >&2; exit 2; }
MODELS="${MATRIX_MODELS:-default}"
TRIALS="${MATRIX_TRIALS:-1}"
JOBS="${MATRIX_JOBS:-1}"

count() { [[ "$2" =~ ^[1-9][0-9]*$ ]] || { echo "$1 must be a positive integer, got '$2'" >&2; exit 2; }; }
count MATRIX_TRIALS "$TRIALS"
count MATRIX_JOBS "$JOBS"

declare -A ARM_PATH ARM_SEEN MODEL_SEEN MODEL_TRIALS
ARM_NAMES=()
for pair in $MATRIX_ARMS; do
  name="${pair%%=*}" path="${pair#*=}"
  [[ "$name" != "$pair" && -n "$path" ]] || { echo "bad arm '$pair' (want NAME=PATH)" >&2; exit 2; }
  [[ "$name" =~ ^[A-Za-z0-9][A-Za-z0-9_-]*$ ]] || { echo "bad arm name '$name' (want [A-Za-z0-9][A-Za-z0-9_-]*)" >&2; exit 2; }
  # Case-folded: the compose project name is lowercase, and a macOS volume
  # folds case in the run dirs.
  [[ -z "${ARM_SEEN[${name,,}]:-}" ]] || { echo "arm $name repeats an earlier name" >&2; exit 2; }
  ARM_SEEN[${name,,}]=1
  [[ -f "$path/tests/fixtures/docker/test-live.sh" ]] || { echo "arm $name: no live fixture under $path" >&2; exit 2; }
  # The branch flavor clones the arm inside the container, and a worktree's
  # .git points at a git dir outside the mount.
  [[ -d "$path/.git" ]] || { echo "arm $name: $path is a worktree; use a clone (git clone --branch <b> <repo> <dir>)" >&2; exit 2; }
  ARM_PATH[$name]="$path"
  ARM_NAMES+=("$name")
done
for m in $MODELS; do
  # The model id names a run dir, so it may not nest one.
  [[ "$m" != */* && "$m" != .* ]] || { echo "bad model '$m' (no / or leading .)" >&2; exit 2; }
  [[ -z "${MODEL_SEEN[$m]:-}" ]] || { echo "model $m is listed twice" >&2; exit 2; }
  MODEL_SEEN[$m]=1
done
for pair in ${MATRIX_MODEL_TRIALS:-}; do
  m="${pair%%=*}" n="${pair#*=}"
  [[ "$m" != "$pair" && -n "$m" ]] || { echo "bad MATRIX_MODEL_TRIALS entry '$pair' (want MODEL=N)" >&2; exit 2; }
  [[ -n "${MODEL_SEEN[$m]:-}" ]] || { echo "MATRIX_MODEL_TRIALS: $m is not in the matrix (models: $MODELS)" >&2; exit 2; }
  count "MATRIX_MODEL_TRIALS $m" "$n"
  MODEL_TRIALS[$m]="$n"
done

# Compose resolves a relative bind source against the compose file's dir.
mkdir -p "$MATRIX_OUT" || exit 2
MATRIX_OUT="$(cd "$MATRIX_OUT" && pwd -P)" || exit 2

trials_for() { echo "${MODEL_TRIALS[$1]:-$TRIALS}"; }

# project MODEL ARM TRIAL — a compose project name unique to the run. The slug
# folds case and punctuation, so a checksum of the raw names keeps
# claude-x-4.5 and claude-x-4-5 apart.
project() {
  local slug="$2-t$3-$1" sum
  slug="${slug,,}"
  slug="${slug//[^a-z0-9_-]/-}"
  sum="$(printf '%s\n' "$1" "$2" "$3" | cksum | cut -d' ' -f1)"
  echo "awmatrix-$slug-$sum"
}

# run_one MODEL ARM TRIAL — one tier 2 run, skipped when its marker exists.
run_one() {
  local model="$1" arm="$2" trial="$3" dir rc
  dir="$MATRIX_OUT/$model/$arm/t$trial"
  if [[ -f "$dir/done" ]]; then
    echo "skip  $model $arm t$trial (done)"
    return 0
  fi
  # An unmarked dir holds a broken or interrupted run; start it over.
  if [[ -e "$dir" ]] && ! rm -rf "$dir" 2>/dev/null; then
    echo "stale $model $arm t$trial: cannot clear $dir (files from another uid?); remove it by hand" >&2
    return 0
  fi
  mkdir -p "$dir"
  echo "start $model $arm t$trial"
  local env=(TIER=2 "TIER2_OUT=$dir" "COMPOSE_PROJECT_NAME=$(project "$model" "$arm" "$trial")")
  [[ "$model" != "default" ]] && env+=("TIER2_MODEL=$model")
  # Job control leaves stdin on the terminal; a background read would stop.
  env "${env[@]}" bash "${ARM_PATH[$arm]}/tests/fixtures/docker/test-live.sh" < /dev/null > "$dir/run.log" 2>&1
  rc=$?
  case "$rc" in
    0) echo "pass" > "$dir/done" ;;
    # A failed rubric still produced transcripts; record it and keep them.
    1) echo "fail" > "$dir/done" ;;
    *) echo "rc $rc" > "$dir/error" ;;
  esac
  echo "end   $model $arm t$trial ($(cat "$dir/done" 2>/dev/null || cat "$dir/error"))"
}

# Job control puts each run in its own process group, so a signal reaches the
# whole run (test-live.sh and docker compose under it), and the jobs no longer
# ignore SIGINT.
set -m
# shellcheck disable=SC2329  # invoked from the traps below
stop() {
  local pids
  pids="$(jobs -p)"
  for p in $pids; do kill -TERM -- "-$p" 2>/dev/null; done
  wait
  echo "matrix stopped by SIG$1; runs in flight left no marker" >&2
  exit "$2"
}
trap 'stop INT 130' INT
trap 'stop TERM 143' TERM

# Trial-major order: every model and arm gets trial k before any gets k+1, and
# the arm order rotates per trial so no arm always runs first.
max_trials=0
for m in $MODELS; do (( $(trials_for "$m") > max_trials )) && max_trials=$(trials_for "$m"); done
n=${#ARM_NAMES[@]}
for (( t = 1; t <= max_trials; t++ )); do
  for m in $MODELS; do
    (( t <= $(trials_for "$m") )) || continue
    for (( i = 0; i < n; i++ )); do
      arm="${ARM_NAMES[$(( (i + t - 1) % n ))]}"
      while (( $(jobs -rp | wc -l) >= JOBS )); do wait -n; done
      run_one "$m" "$arm" "$t" &
    done
  done
  # The trial barrier: no run of trial k+1 overlaps one of trial k.
  wait
done

echo "matrix done: $MATRIX_OUT"
bad=0
for m in $MODELS; do
  for (( t = 1; t <= $(trials_for "$m"); t++ )); do
    for arm in "${ARM_NAMES[@]}"; do
      dir="$MATRIX_OUT/$m/$arm/t$t"
      if [[ -f "$dir/done" ]]; then s="$(cat "$dir/done")"
      elif [[ -f "$dir/error" ]]; then s="error ($(cat "$dir/error"))"
      else s="missing"
      fi
      [[ "$s" == pass ]] || bad=1
      printf '%s %s\n' "$s" "$dir"
    done
  done
done
exit "$bad"
