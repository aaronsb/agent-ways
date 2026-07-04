//! Calibration of embedding cosine into a relevance probability (ADR-156).
//!
//! The matcher no longer thresholds raw cosine. Instead each model carries a
//! fitted logistic `g(s) = σ(a·s + b)` mapping cosine `s` to `P(relevant)`, and
//! the fire thresholds are probabilities. This module owns the model
//! (`ModelCalibration`), the offline fit from labelled `(cosine, relevant)`
//! probe samples (`fit`), and the separability measure (`auc`) that gates a fit.
//!
//! The fit runs at corpus-generation time; the result is stored in the corpus
//! manifest, version-stamped, and loaded read-only at scan time.

use serde::{Deserialize, Serialize};

/// A fitted per-model calibration `g(s) = σ(a·s + b)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelCalibration {
    /// Logistic slope.
    pub a: f64,
    /// Logistic intercept.
    pub b: f64,
    /// Area under the ROC curve of the fit's probe set — separability of intent
    /// from noise. A fit below the generation-time floor is rejected.
    pub auc: f64,
    /// Number of probe samples the fit was computed from.
    pub n: usize,
}

impl ModelCalibration {
    /// Calibrated relevance probability for a cosine score.
    pub fn probability(&self, cosine: f64) -> f64 {
        sigmoid(self.a * cosine + self.b)
    }
}

/// Per-model calibrations, as stored in the corpus manifest. A `None` lane means
/// that model was not calibrated (e.g. multilingual on an English-only install).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub en: Option<ModelCalibration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi: Option<ModelCalibration>,
}

#[inline]
fn sigmoid(z: f64) -> f64 {
    if z >= 0.0 {
        1.0 / (1.0 + (-z).exp())
    } else {
        let e = z.exp();
        e / (1.0 + e)
    }
}

/// Fit `g(s) = σ(a·s + b)` to labelled samples `(cosine, relevant)` and measure
/// its separability. Returns `None` when there is nothing to fit (empty, or all
/// one class — no gradient toward a boundary).
pub fn fit(samples: &[(f64, bool)]) -> Option<ModelCalibration> {
    // Drop non-finite cosines (a stray "nan"/"inf" token from the embedder would
    // otherwise poison the fit and the sort in `auc`).
    let samples: Vec<(f64, bool)> = samples.iter().copied().filter(|(c, _)| c.is_finite()).collect();
    if samples.len() < 2 {
        return None;
    }
    let pos = samples.iter().filter(|s| s.1).count();
    if pos == 0 || pos == samples.len() {
        return None; // single-class: no decision boundary to estimate
    }
    let (a, b) = fit_logistic(&samples);
    // Relevance must be monotonically increasing in cosine: a non-positive (or
    // non-finite) slope means the probe data is degenerate or backwards. Reject
    // rather than ship a calibration that maps higher cosine to lower probability.
    if !a.is_finite() || a <= 0.0 {
        return None;
    }
    // Measure separability on the FITTED predictor `a·x + b`, not raw cosine: a
    // near-flat fit (a ≈ 0) scores ≈ 0.5 and fails the caller's AUC ship-gate,
    // where raw-cosine separability alone would have passed it.
    let scored: Vec<(f64, bool)> = samples.iter().map(|(x, l)| (a * x + b, *l)).collect();
    Some(ModelCalibration {
        a,
        b,
        auc: auc(&scored),
        n: samples.len(),
    })
}

/// Newton–Raphson fit of a 1-D logistic. Two parameters `(a, b)`, so each step
/// solves a 2×2 system; converges in a handful of iterations. A small ridge on
/// the Hessian keeps it invertible when the classes are nearly separable (the
/// likelihood is then flat in `a` and the unregularised Hessian degenerates).
fn fit_logistic(samples: &[(f64, bool)]) -> (f64, f64) {
    let (mut a, mut b) = (1.0f64, 0.0f64);
    let ridge = 1e-6;
    for _ in 0..100 {
        // Gradient g = Σ (p - y) · [x, 1]; Hessian H = Σ p(1-p) · [[x², x], [x, 1]].
        let (mut g0, mut g1) = (0.0, 0.0);
        let (mut h00, mut h01, mut h11) = (ridge, 0.0, ridge);
        for &(x, y) in samples {
            let p = sigmoid(a * x + b);
            let d = p - if y { 1.0 } else { 0.0 };
            g0 += d * x;
            g1 += d;
            let w = p * (1.0 - p);
            h00 += w * x * x;
            h01 += w * x;
            h11 += w;
        }
        // Solve H · Δ = g (2×2), then a,b -= Δ.
        let det = h00 * h11 - h01 * h01;
        if det.abs() < 1e-12 {
            break;
        }
        let da = (h11 * g0 - h01 * g1) / det;
        let db = (h00 * g1 - h01 * g0) / det;
        a -= da;
        b -= db;
        if da.abs() < 1e-9 && db.abs() < 1e-9 {
            break;
        }
    }
    (a, b)
}

/// Area under the ROC curve: the probability that a random positive scores above
/// a random negative (ties count as half). Computed by rank so it is `O(n log n)`.
pub fn auc(samples: &[(f64, bool)]) -> f64 {
    let n_pos = samples.iter().filter(|s| s.1).count();
    let n_neg = samples.len() - n_pos;
    if n_pos == 0 || n_neg == 0 {
        return 0.5;
    }
    // Rank scores ascending (average rank for ties), sum ranks of positives.
    let mut idx: Vec<usize> = (0..samples.len()).collect();
    // total_cmp is a total order over f64 (NaN-safe), so a stray non-finite
    // score can never panic the sort.
    idx.sort_by(|&i, &j| samples[i].0.total_cmp(&samples[j].0));
    let mut rank = vec![0.0; samples.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && samples[idx[j + 1]].0 == samples[idx[i]].0 {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0; // 1-based average rank
        for &k in &idx[i..=j] {
            rank[k] = avg;
        }
        i = j + 1;
    }
    let sum_pos: f64 = samples
        .iter()
        .zip(&rank)
        .filter(|(s, _)| s.1)
        .map(|(_, r)| *r)
        .sum();
    (sum_pos - n_pos as f64 * (n_pos as f64 + 1.0) / 2.0) / (n_pos as f64 * n_neg as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmoid_is_stable_and_monotone() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-12);
        assert!(sigmoid(50.0) > 0.999);
        assert!(sigmoid(-50.0) < 0.001);
        assert!(sigmoid(1.0) > sigmoid(-1.0));
    }

    #[test]
    fn auc_perfect_separation() {
        let s = [(0.1, false), (0.2, false), (0.8, true), (0.9, true)];
        assert!((auc(&s) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn auc_inverted_is_zero() {
        let s = [(0.8, false), (0.9, false), (0.1, true), (0.2, true)];
        assert!(auc(&s).abs() < 1e-9);
    }

    #[test]
    fn auc_ties_half() {
        let s = [(0.5, true), (0.5, false)];
        assert!((auc(&s) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn fit_separable_orders_probability() {
        // Noise ~0.2, intent ~0.6: fit must map intent above 0.5 and noise below.
        let s = [
            (0.15, false),
            (0.20, false),
            (0.25, false),
            (0.55, true),
            (0.60, true),
            (0.65, true),
        ];
        let c = fit(&s).expect("fit");
        assert!(c.auc > 0.9, "auc {}", c.auc);
        assert!(c.probability(0.60) > 0.5, "intent {}", c.probability(0.60));
        assert!(c.probability(0.20) < 0.5, "noise {}", c.probability(0.20));
        assert!(c.a > 0.0, "slope should be positive");
    }

    #[test]
    fn fit_rejects_single_class() {
        assert!(fit(&[(0.5, true), (0.6, true)]).is_none());
        assert!(fit(&[]).is_none());
    }

    #[test]
    fn fit_drops_non_finite_samples() {
        let s = [
            (0.20, false),
            (f64::NAN, true),
            (0.60, true),
            (0.25, false),
            (f64::INFINITY, false),
            (0.65, true),
        ];
        let c = fit(&s).expect("fit from the finite subset");
        assert!(c.a > 0.0);
        assert_eq!(c.n, 4, "NaN and inf dropped before fitting");
    }

    #[test]
    fn fit_rejects_inverted_slope() {
        // Intent at low cosine, noise at high cosine → non-positive slope.
        let s = [(0.8, false), (0.9, false), (0.1, true), (0.2, true)];
        assert!(fit(&s).is_none(), "backwards probe data must not ship a calibration");
    }

    #[test]
    fn auc_survives_non_finite_score() {
        // total_cmp keeps the sort from panicking even on a NaN.
        let s = [(0.1, false), (f64::NAN, true), (0.9, true)];
        let _ = auc(&s); // must not panic
    }

    #[test]
    fn calibration_serde_roundtrips() {
        let c = Calibration {
            en: Some(ModelCalibration {
                a: 26.8,
                b: -7.9,
                auc: 0.95,
                n: 200,
            }),
            multi: None,
        };
        let j = serde_json::to_string(&c).unwrap();
        assert!(!j.contains("multi"), "None lane omitted: {j}");
        let back: Calibration = serde_json::from_str(&j).unwrap();
        assert_eq!(c, back);
    }
}
