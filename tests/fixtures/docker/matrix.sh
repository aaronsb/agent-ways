#!/usr/bin/env bash
# Tier 2 across a matrix of arms, models and trials: `make test-live-matrix`.
#
# An arm is a checkout of this repo (a worktree of a branch), so an ablation
# compares the corpus with and without a change. Each run is one tier 2
# invocation of that arm's own test-live.sh, with one model.
#
#   MATRIX_ARMS     space-separated NAME=PATH pairs; the same PATH may appear
#                   under two names to measure run-to-run noise (A=x A2=x B=y)
#   MATRIX_MODELS   space-separated model ids (default: the tier 2 default)
#   MATRIX_TRIALS   trials per arm and model (default 1); MODEL=N entries in
#                   MATRIX_MODEL_TRIALS override it per model
#   MATRIX_OUT      root for transcripts; a run writes MATRIX_OUT/<model>/<arm>/t<k>
#   MATRIX_JOBS     runs at once (default 1); within a trial every arm of a
#                   model runs before the next model, so arms share conditions
#   TIER2_SCENARIOS, TIER2_MAX_TURNS, ANTHROPIC_API_KEY_FILE, WAYS_BINARIES
#                   passed to each run unchanged
#
# A run that finished keeps a `done` marker, so rerunning the same matrix
# resumes where it stopped. Each run's console output is in run.log beside it.
set -uo pipefail

[[ -n "${MATRIX_ARMS:-}" ]] || { echo "MATRIX_ARMS is required (NAME=PATH ...)" >&2; exit 2; }
[[ -n "${MATRIX_OUT:-}" ]] || { echo "MATRIX_OUT is required" >&2; exit 2; }
MODELS="${MATRIX_MODELS:-}"
TRIALS="${MATRIX_TRIALS:-1}"
JOBS="${MATRIX_JOBS:-1}"

declare -A ARM_PATH MODEL_TRIALS
ARM_NAMES=()
for pair in $MATRIX_ARMS; do
  name="${pair%%=*}" path="${pair#*=}"
  [[ "$name" != "$pair" && -n "$path" ]] || { echo "bad arm '$pair' (want NAME=PATH)" >&2; exit 2; }
  [[ -f "$path/tests/fixtures/docker/test-live.sh" ]] || { echo "arm $name: no live fixture under $path" >&2; exit 2; }
  ARM_PATH[$name]="$path"
  ARM_NAMES+=("$name")
done
for pair in ${MATRIX_MODEL_TRIALS:-}; do
  MODEL_TRIALS[${pair%%=*}]="${pair#*=}"
done
[[ -n "$MODELS" ]] || MODELS="default"

trials_for() { echo "${MODEL_TRIALS[$1]:-$TRIALS}"; }

# run_one MODEL ARM TRIAL — one tier 2 run, skipped when its marker exists.
run_one() {
  local model="$1" arm="$2" trial="$3" dir
  dir="$MATRIX_OUT/$model/$arm/t$trial"
  if [[ -f "$dir/done" ]]; then
    echo "skip  $model $arm t$trial (done)"
    return 0
  fi
  mkdir -p "$dir"
  echo "start $model $arm t$trial"
  local env=(TIER=2 "TIER2_OUT=$dir" "COMPOSE_PROJECT_NAME=awmatrix-${arm,,}-t$trial-${model//[^a-z0-9]/}")
  [[ "$model" != "default" ]] && env+=("TIER2_MODEL=$model")
  if env "${env[@]}" bash "${ARM_PATH[$arm]}/tests/fixtures/docker/test-live.sh" > "$dir/run.log" 2>&1; then
    echo "pass" > "$dir/done"
  else
    # A failed rubric still produced transcripts; record it and keep them.
    echo "fail" > "$dir/done"
  fi
  echo "end   $model $arm t$trial ($(cat "$dir/done"))"
}

# Trial-major order: every model and arm gets trial k before any gets k+1, and
# the arm order rotates per trial so no arm always runs first.
max_trials=0
for m in $MODELS; do (( $(trials_for "$m") > max_trials )) && max_trials=$(trials_for "$m"); done
for (( t = 1; t <= max_trials; t++ )); do
  n=${#ARM_NAMES[@]}
  for m in $MODELS; do
    (( t <= $(trials_for "$m") )) || continue
    for (( i = 0; i < n; i++ )); do
      arm="${ARM_NAMES[$(( (i + t - 1) % n ))]}"
      while (( $(jobs -rp | wc -l) >= JOBS )); do wait -n; done
      run_one "$m" "$arm" "$t" &
    done
  done
done
wait

echo "matrix done: $MATRIX_OUT"
find "$MATRIX_OUT" -name 'done' -exec sh -c 'printf "%s %s\n" "$(cat "$1")" "$(dirname "$1")"' _ {} \; | sort -k2
