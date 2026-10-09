---
description: a forecast evaluative loop for a predictive model only ever scored on the data it was trained on or on history it has already seen; judge it on held-out future data with a walk-forward backtest against a naive baseline, check for leakage, report scale-free error per horizon and residual diagnostics
vocabulary: prediction model predictive model accuracy only scored on training data fits history perfectly looks amazing on past data predicting the future churn sales demand delays overfit forecast forecasting time series backtest walk-forward rolling origin held-out future horizon naive baseline seasonal naive leakage look-ahead mase residual autocorrelation arima fourier kalman lstm transformer echo state takens train test split
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Forecast Loop

The agent writes a model that predicts or explains a time-ordered signal: demand, load, a sensor, a price, a physical quantity. The judge is data the model has not seen, from later in time than anything it was fitted on.

## When it applies

A model that predicts something, such as churn, demand, sales or delays, scores well, but only on the data it was trained on or on a past it has already seen. How it does on data from after its training period is unknown.

## Freeze the test window first

Choose the held-out period before fitting anything and record it. The agent does not move it, shorten it, or drop an awkward stretch after seeing results; that is the author changing the oracle (see `evaluative`). A new window is a new evaluation, reported beside the old one.

## Walk forward, never a random split

On time-ordered data a random split puts the future in the training set. Use a walk-forward (rolling-origin) backtest: fit on data up to a cut-off, forecast the next horizon, move the cut-off forward, repeat. Report errors across all origins.

## Beat a naive baseline

Score the same backtest for a naive forecast: the last value, the value one season ago, or the mean. A model that does not beat the naive baseline on the frozen window is not adopted, however good its in-sample fit looks.

## Check for leakage

Future information leaks in quietly: a feature computed over the whole series, scaling fitted on all the data, a rolling statistic that includes the target's own period, hyperparameters tuned on the test window. Each preprocessing step is fitted inside each training fold only. A result that looks too good is checked for leakage before it is believed.

## A scale-free metric, per horizon

Use an error measure that compares across series and scales, such as mean absolute scaled error: the forecast's mean absolute error divided by the in-sample mean absolute error of a one-step naive forecast (seasonal naive for seasonal data) on the training data. Below 1 beats that naive forecast. Report it per forecast horizon: a model can win one step ahead and lose at twelve.

## Read the residuals

Check the one-step-ahead residuals: from the fit on the training data, and from one-step forecasts in the backtest. They should look like noise. Autocorrelation left in them (visible in an autocorrelation plot or a portmanteau test), a trend, or a periodic pattern means the model missed structure. Errors of forecasts more than one step ahead are autocorrelated even for a correct model, because neighbouring forecasts share most of their unknown future, so they are not tested this way. Report the diagnostics with the error metric.

## Match the family to the signal

| Family | Suits |
|---|---|
| AR, ARMA, ARIMA | Linear, autocorrelated series; differencing handles trend |
| Fourier, spectral methods | Periodic or cyclical signals |
| State-space, Kalman filter | A hidden state seen through noisy observations, updated online |
| RNN, LSTM, GRU, Transformer | Nonlinear structure, given enough data |
| Reservoir computing, echo state networks | Chaotic or wave-like dynamics at low training cost; only the readout is trained |
| Delay embedding (Takens) | Reconstructing a system's dynamics from one observed signal |

Start with the simplest family the signal allows. A more complex model earns its place by beating a simpler one on the same frozen backtest.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- evaluative/model-oracle(evaluative) — a fitted model used as the judge of other output
- evaluative/probe-set(evaluative) — a metric over a labelled sample, for non-temporal changes
