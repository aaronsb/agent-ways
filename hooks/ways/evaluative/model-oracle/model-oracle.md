---
description: a model-oracle evaluative loop where a model fitted to a system's normal behaviour judges new output by the residual between prediction and observation, for when no fixed expected value exists; benchmark timings across commits, telemetry, simulation energy drift, a metric tracked across releases
vocabulary: numbers jitter run to run fixed pass threshold cries wolf misses real slowdowns no fixed expected value normal behaviour expected range benchmark timing timings across commits performance regression slower than usual telemetry metric across releases anomaly outlier drift energy drift long simulation conservation reference window frozen k sigma three sigma control limit band envelope residual threshold regime change new normal re-baseline seasonal decomposition noise floor
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Model-Oracle Loop

Some outputs have no fixed right answer. A benchmark takes 41.2 ms one run and 43.0 the next. Telemetry moves with the time of day. A long simulation's total energy drifts a little by design. Here the oracle is a model of the system's normal behaviour: it predicts what this run should show, and the residual between prediction and observation decides.

This differs from `evaluative/probe-set`, where a labelled sample is scored by a metric, and from `evaluative/fidelity`, where an external answer key exists. Here the judge is fitted from the system's own history.

## Fit on a frozen reference window

Choose a window of known-good behaviour (the last N commits, a week of telemetry, a validated simulation run), fit the model on it, and record which window it was. Usual fits are an autoregressive model, a seasonal decomposition, or a Kalman filter tracking a slowly moving level. Keep the model as simple as the signal allows.

## Set the threshold before the change

Derive the threshold from the residual's own spread on the reference window, such as k standard deviations, and fix it before the change under test is measured. A threshold chosen after seeing the new result can be placed anywhere. Report the residual as a multiple of that spread.

Measure enough repetitions to estimate the spread honestly. One timing per commit puts machine noise into the reference window.

## A flagged residual is a finding

A residual past the threshold is investigated: rerun to rule out noise on the measuring host, then bisect or explain. It is not cleared by widening the threshold or refitting on a window that includes the new result.

## A regime change is a baseline change

Sometimes the system has legitimately changed: a deliberate optimisation, new hardware, a new release cadence. Re-fitting the model on the new behaviour is then a change to the oracle. It goes to the operator as an approved baseline change, with the old and new reference windows and the size of the shift, and it is never done silently.

## Keep the measuring host still

The residual is only as clean as the measurement. Pin the host, the build flags and the input; record them with each run (see environment/hostparity). A residual that appears only on one machine is a fact about that machine.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- evaluative/forecast(evaluative) — when the model itself is the product under test
- code/performance(softwaredev) — profiling once a regression is flagged
