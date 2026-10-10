---
description: a model-oracle evaluative loop where a model fitted to a system's normal behaviour judges new output by the residual between prediction and observation, for when no fixed expected value exists; benchmark timings across commits, telemetry, simulation energy drift, a metric tracked across releases
vocabulary: numbers jitter run to run fixed pass threshold cries wolf misses real slowdowns no fixed expected value normal behaviour expected range benchmark timing timings across commits performance regression slower than usual telemetry metric across releases anomaly outlier drift energy drift long simulation conservation reference window frozen k sigma three sigma control limit band envelope residual threshold regime change new normal re-baseline seasonal decomposition noise floor percentile median mad refit pre-registered family-wise variability
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Model-Oracle Loop

Some outputs have no fixed right answer. A benchmark takes 41.2 ms one run and 43.0 the next. Telemetry moves with the time of day. A long simulation's total energy drifts a little by design. Here the oracle is a model of the system's normal behaviour: it predicts what this run should show, and the residual between prediction and observation decides.

This differs from `evaluative/probe-set`, where a labelled sample is scored by a metric, and from `evaluative/fidelity`, where an external answer key exists. Here the judge is fitted from the system's own history.

## When it applies

A number that is never the same twice, such as response latency, memory use, an error rate or a benchmark timing, and the question is whether a change made it worse or whether this is its usual wobble. A fixed limit either fires on ordinary noise or misses a real shift.

## Fit on a frozen reference window

Choose a window of known-good behaviour (the last N commits, a week of telemetry, a validated simulation run), fit the model on it, and record which window it was. Fits range from a robust band around the median to change-point detection, a seasonal decomposition, or a Kalman filter tracking a slowly moving level. Keep the model as simple as the signal allows.

## Set the threshold before the change

Derive the threshold from the residual's own spread on the reference window and fix it before the change under test is measured. A threshold chosen after seeing the new result can be placed anywhere. Report the residual as a multiple of that spread.

- **Use a robust spread.** Timings and telemetry are skewed and carry outliers, which inflate a standard deviation. Centre on the median and scale by the median absolute deviation (multiplied by about 1.4826 to be comparable to a standard deviation for normal data), or use a percentile band, then set the threshold at k of those robust spreads.
- **Account for how many series are checked.** At three standard deviations a single normal series raises a false alarm about once in 370 checks. A suite of 200 benchmarks checked per commit then flags something most of the time with nothing wrong. With many series, set k for the whole family, not per series, or control the false-discovery rate across them, and rerun a flagged series before acting on it.
- **Measure enough repetitions** to estimate the spread honestly. One timing per commit puts machine noise into the reference window.

## A flagged residual is a finding

A residual past the threshold is investigated: rerun to rule out noise on the measuring host, then bisect or explain. It is not cleared by widening the threshold or refitting on a window that includes the new result.

## A regime change is a baseline change

Sometimes the system has legitimately changed: a deliberate optimisation, new hardware, a new release cadence. Re-fitting the model on the new behaviour is then a change to the oracle. It goes to the operator as an approved baseline change, with the old and new reference windows and the size of the shift, and it is never done silently.

## Keep the measuring host still

Pin the host, the build flags and the input; record them with each run (see environment/hostparity).

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- evaluative/forecast(evaluative) — when the model itself is the product under test
- code/performance(softwaredev) — profiling once a regression is flagged
