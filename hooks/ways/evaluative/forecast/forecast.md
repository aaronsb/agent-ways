---
description: a forecast evaluative loop for a predictive or analytical model of time-ordered data, judged on held-out future observations; walk-forward backtest, beat a naive baseline, check for leakage, scale-free error per horizon, residual diagnostics, and a test window frozen before fitting
vocabulary: forecast forecasting predict prediction time series time-ordered backtest walk-forward rolling origin held-out future horizon naive baseline seasonal naive last value leakage look-ahead lookahead mase mape error per horizon residual autocorrelation ljung-box arima arma autoregressive fourier spectral periodic kalman state-space lstm gru rnn transformer reservoir echo state network takens delay embedding chaotic train test split
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Forecast Loop

The agent writes a model that predicts or explains a time-ordered signal: demand, load, a sensor, a price, a physical quantity. The judge is data the model has not seen, from later in time than anything it was fitted on.

## Freeze the test window first

Choose the held-out period before fitting anything and record it. The agent does not move it, shorten it, or drop an awkward stretch after seeing results; that is the author changing the oracle (see `evaluative`). A new window is a new evaluation, reported beside the old one.

## Walk forward, never a random split

On time-ordered data a random split puts the future in the training set. Use a walk-forward (rolling-origin) backtest: fit on data up to a cut-off, forecast the next horizon, move the cut-off forward, repeat. Report errors across all origins.

## Beat a naive baseline

Score the same backtest for a naive forecast: the last value, the value one season ago, or the mean. A model that does not beat the naive baseline on the frozen window is not adopted, however good its in-sample fit looks.

## Check for leakage

Future information leaks in quietly: a feature computed over the whole series, scaling fitted on all the data, a rolling statistic that includes the target's own period, hyperparameters tuned on the test window. Each preprocessing step is fitted inside each training fold only. A result that looks too good is checked for leakage before it is believed.

## A scale-free metric, per horizon

Use an error measure that compares across series and scales, such as mean absolute scaled error, which scores against the naive forecast. Report it per forecast horizon: a model can win one step ahead and lose at twelve.

## Read the residuals

Forecast residuals on the test window should look like noise. Autocorrelation left in them (visible in an autocorrelation plot or a portmanteau test), a trend, or a periodic pattern means the model missed structure. Report the diagnostics with the error metric.

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
