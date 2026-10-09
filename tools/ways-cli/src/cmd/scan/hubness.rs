//! scan/hubness.rs — the hubness penalty (ADR-700 §5, evaluated under ADR-703).
//!
//! A hub is a way whose alias scores high against many prompts, so it crowds
//! the ranking whatever the prompt is about. CSLS corrects for it by lowering
//! each way's score by its mean top-k cosine over a set of prompts. Here the
//! set is the golden prompts the committed probe sets leave out, so the
//! penalty is fitted without labels and never sees an evaluation row.
//!
//! `ways corpus` scores those prompts once, after the body sidecar, and writes
//! each way's highest cosines ([`KEEP`] of them, descending) to [`FILE`]: the
//! alias cosine, and the scaled body-rank score `(alias + w × best section) /
//! (1 + w)` that the single-vector path ranks on by default. A scan picks the
//! list that matches the score it is penalising, so the penalty is fitted on
//! the same quantity it corrects.
//!
//! The penalty is `λ × (hub − mean hub)`, the mean taken over every way in the
//! file ([`Shape::Centred`]). Centring keeps the average score where the
//! calibration was fitted, and it raises the ways below the mean as much as it
//! lowers the hubs. [`Shape::OneSided`] lowers the hubs only; [`Shape::Raw`]
//! subtracts `λ × hub` from every way. The file is used only when its model id
//! is the installed one's ([`super::sidecar::model_id`]).

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file's name in the corpus directory.
pub(crate) const FILE: &str = "ways-hubness-en.json";
/// Cosines kept per way, the largest k a scan can ask for.
pub(crate) const KEEP: usize = 32;
/// Default λ: CSLS's `2·cos − r` halved onto the cosine scale.
pub(crate) const LAMBDA: f64 = 0.5;
/// Default neighbourhood size (ADR-700 §5 measured k = 10).
pub(crate) const K: usize = 10;

/// The stored fit.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Hubs {
    /// The embedder that scored the prompts ([`super::sidecar::model_id`]).
    pub model: String,
    /// Fitting prompts scored.
    pub prompts: usize,
    /// Per way, its top alias cosines over the prompts, descending.
    pub alias: HashMap<String, Vec<f64>>,
    /// Per way, its top scaled body-rank scores, descending. A way with no
    /// body sections carries its alias cosines, as the scan ranks it.
    pub fused: HashMap<String, Vec<f64>>,
}

/// Which stored list a penalty is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fit {
    /// The list matching the score being penalised (fused when fused).
    Matching,
    /// Always the alias list.
    Alias,
}

/// How a way's hub becomes its penalty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// `λ × (hub − mean hub)`: hubs drop, ways below the mean rise.
    Centred,
    /// `λ × max(0, hub − mean hub)`: hubs drop, no way rises.
    OneSided,
    /// `λ × hub`: every way drops.
    Raw,
}

/// Evaluation switches for `ways author probe`. The scan never sets them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tuning {
    pub lambda: f64,
    pub k: usize,
    pub shape: Shape,
    pub fit: Fit,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning { lambda: LAMBDA, k: K, shape: Shape::Centred, fit: Fit::Matching }
    }
}

static TUNING: std::sync::OnceLock<Tuning> = std::sync::OnceLock::new();

/// Set the evaluation switches for this process (probe only; first call wins).
pub(crate) fn set_tuning(t: Tuning) {
    let _ = TUNING.set(t);
}

fn tuning() -> Tuning {
    TUNING.get().copied().unwrap_or_default()
}

impl Hubs {
    /// Each way's penalty for scores that are `fused` or alias cosines.
    pub(crate) fn penalties(&self, fused: bool) -> HashMap<String, f64> {
        let t = tuning();
        let lists = if fused && t.fit == Fit::Matching { &self.fused } else { &self.alias };
        penalties(lists, t.k, t.lambda, t.shape)
    }
}

/// Each way's penalty under `shape`, where hub is the mean of the way's first
/// `k` values. A way with an empty list gets none.
pub(crate) fn penalties(lists: &HashMap<String, Vec<f64>>, k: usize, lambda: f64, shape: Shape) -> HashMap<String, f64> {
    let hubs: HashMap<&str, f64> = lists
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(id, v)| {
            let top = &v[..v.len().min(k.max(1))];
            (id.as_str(), top.iter().sum::<f64>() / top.len() as f64)
        })
        .collect();
    let mean = if shape == Shape::Raw || hubs.is_empty() { 0.0 } else { hubs.values().sum::<f64>() / hubs.len() as f64 };
    let floor = if shape == Shape::OneSided { 0.0 } else { f64::NEG_INFINITY };
    hubs.into_iter().map(|(id, h)| (id.to_string(), lambda * (h - mean).max(floor))).collect()
}

/// Subtract each row's penalty in place and re-sort descending (stable, so
/// ties keep their order). True when a row changed.
pub(crate) fn apply(rows: &mut [(String, f64)], penalties: &HashMap<String, f64>) -> bool {
    let mut any = false;
    for (id, score) in rows.iter_mut() {
        if let Some(p) = penalties.get(id) {
            *score -= p;
            any = true;
        }
    }
    rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    any
}

/// The fit in `corpus_dir`, when it was built with the embedder `bin` and the
/// model in `engine_dir` would use.
pub(crate) fn load(corpus_dir: &Path, engine_dir: &Path, bin: &Path) -> Option<Hubs> {
    let hubs: Hubs = serde_json::from_str(&std::fs::read_to_string(corpus_dir.join(FILE)).ok()?).ok()?;
    (super::sidecar::model_id(engine_dir, bin)? == hubs.model).then_some(hubs)
}

/// The golden prompts the fit uses: every way's prompt-lane golden rows
/// except those of the ways the probe sample selects (`golden --probes` and
/// `--joined` draw only from those), and no `none` rows (the unrelated sets).
/// Tool-surface rows are Bash commands and are left out. Deduplicated, in
/// export order.
pub(crate) fn fitting_prompts(rows: &[crate::cmd::golden::Row]) -> Vec<String> {
    let probed: std::collections::HashSet<String> = crate::cmd::golden::sample_probes(rows).into_iter().map(|p| p.way).collect();
    let mut seen = std::collections::HashSet::new();
    rows.iter()
        .filter(|r| r.way != "none" && !r.kind.ends_with("-tool") && !probed.contains(&r.way))
        .filter(|r| seen.insert(r.prompt.clone()))
        .map(|r| r.prompt.clone())
        .collect()
}

/// Fold one prompt's rows into the per-way lists.
fn push_rows(lists: &mut HashMap<String, Vec<f64>>, rows: impl IntoIterator<Item = (String, f64)>) {
    for (id, s) in rows {
        lists.entry(id).or_default().push(s);
    }
}

/// Sort each list descending and keep the first [`KEEP`].
fn truncate(lists: &mut HashMap<String, Vec<f64>>) {
    for v in lists.values_mut() {
        v.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(KEEP);
    }
}

/// Fit from scored prompts: per prompt, the alias rows and that prompt's
/// vector; `sidecar` turns each alias row into the scaled body-rank score.
pub(crate) fn fit(model: String, per_prompt: &[Vec<(String, f64)>], vectors: &[Vec<f32>], sidecar: &super::sidecar::Sidecar) -> Hubs {
    let (mut alias, mut fused) = (HashMap::new(), HashMap::new());
    for (rows, v) in per_prompt.iter().zip(vectors) {
        push_rows(&mut alias, rows.iter().cloned());
        push_rows(
            &mut fused,
            rows.iter().map(|(id, s)| {
                let f = super::late_interaction::fused_score(sidecar, id, v, *s, crate::config::BodyRank::Scaled).map_or(*s, |(f, _)| f);
                (id.clone(), f)
            }),
        );
    }
    truncate(&mut alias);
    truncate(&mut fused);
    Hubs { model, prompts: per_prompt.len(), alias, fused }
}

/// Build the fit for the corpus in `out_dir` from the golden prompts under
/// `ways_root`, write it, and return (prompts, ways), or why not. Needs the
/// alias corpus and the body sidecar already in `out_dir`.
pub(crate) fn build(out_dir: &Path, engine_dir: &Path, bin: &Path, ways_root: &Path) -> Result<(usize, usize), String> {
    let model_id = super::sidecar::model_id(engine_dir, bin).ok_or("English model or way-embed missing")?;
    let sidecar = super::sidecar::read(&out_dir.join(super::sidecar::FILE)).ok_or("no body sidecar")?;
    let rows = crate::cmd::golden::export(ways_root).map_err(|e| e.to_string())?;
    let prompts = fitting_prompts(&rows);
    if prompts.is_empty() {
        return Err("no golden prompts outside the probe sample".into());
    }
    let corpus = out_dir.join("ways-corpus-en.jsonl");
    let model = engine_dir.join(crate::paths::EN_MODEL);
    let (per_prompt, vectors) =
        super::late_interaction::match_with_vectors(bin, &corpus, &model, &prompts).ok_or("way-embed match --vectors failed")?;
    let hubs = fit(model_id, &per_prompt, &vectors, &sidecar);
    let ways = hubs.alias.len();
    let json = serde_json::to_string(&hubs).map_err(|e| e.to_string())?;
    let staged = out_dir.join(format!("{FILE}.{}.tmp", std::process::id()));
    std::fs::write(&staged, json).map_err(|e| e.to_string())?;
    std::fs::rename(&staged, out_dir.join(FILE)).map_err(|e| e.to_string())?;
    Ok((prompts.len(), ways))
}

/// Apply the penalty to one ranking's rows, when the fit is present.
/// `fused` says which score the rows hold. True when a row changed.
pub(crate) fn penalise(hubs: Option<&Hubs>, rows: &mut [(String, f64)], fused: bool) -> bool {
    hubs.is_some_and(|h| apply(rows, &h.penalties(fused)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists(pairs: &[(&str, &[f64])]) -> HashMap<String, Vec<f64>> {
        pairs.iter().map(|(id, v)| (id.to_string(), v.to_vec())).collect()
    }

    #[test]
    fn centred_penalty_is_lambda_times_hub_less_the_mean_hub() {
        // hubs: a = (0.6 + 0.4) / 2 = 0.5, b = (0.3 + 0.1) / 2 = 0.2; mean 0.35.
        let l = lists(&[("a", &[0.6, 0.4, 0.0]), ("b", &[0.3, 0.1, 0.0])]);
        let p = penalties(&l, 2, 0.5, Shape::Centred);
        assert!((p["a"] - 0.075).abs() < 1e-12, "{p:?}");
        assert!((p["b"] + 0.075).abs() < 1e-12, "{p:?}");
    }

    #[test]
    fn one_sided_penalty_lowers_hubs_and_raises_nothing() {
        let l = lists(&[("a", &[0.6, 0.4, 0.0]), ("b", &[0.3, 0.1, 0.0])]);
        let p = penalties(&l, 2, 0.5, Shape::OneSided);
        assert!((p["a"] - 0.075).abs() < 1e-12, "{p:?}");
        assert_eq!(p["b"], 0.0, "{p:?}");
    }

    #[test]
    fn raw_penalty_is_lambda_times_hub() {
        let l = lists(&[("a", &[0.6, 0.4]), ("b", &[0.3, 0.1])]);
        let p = penalties(&l, 2, 0.5, Shape::Raw);
        assert!((p["a"] - 0.25).abs() < 1e-12);
        assert!((p["b"] - 0.10).abs() < 1e-12);
    }

    #[test]
    fn k_beyond_the_list_uses_the_whole_list_and_an_empty_list_gets_nothing() {
        let l = lists(&[("a", &[0.6, 0.4]), ("e", &[])]);
        let p = penalties(&l, 10, 1.0, Shape::Raw);
        assert!((p["a"] - 0.5).abs() < 1e-12);
        assert!(!p.contains_key("e"));
    }

    #[test]
    fn apply_lowers_a_hub_below_a_specific_way_and_reports_the_change() {
        let mut rows = vec![("hub".to_string(), 0.50), ("specific".to_string(), 0.45)];
        let p: HashMap<String, f64> = [("hub".to_string(), 0.08), ("specific".to_string(), -0.02)].into();
        assert!(apply(&mut rows, &p));
        assert_eq!(rows[0].0, "specific");
        assert!((rows[0].1 - 0.47).abs() < 1e-12);
        assert!((rows[1].1 - 0.42).abs() < 1e-12);
        let mut untouched = vec![("other".to_string(), 0.3)];
        assert!(!apply(&mut untouched, &p));
        assert_eq!(untouched, vec![("other".to_string(), 0.3)]);
    }

    fn row(prompt: &str, way: &str, kind: &str) -> crate::cmd::golden::Row {
        crate::cmd::golden::Row { prompt: prompt.into(), way: way.into(), kind: kind.into() }
    }

    #[test]
    fn fitting_prompts_leave_out_probed_ways_and_none_rows() {
        // One root with two leaves: the sample takes the root (situational)
        // and one hashed leaf (both kinds); the other leaf is the fitting set.
        let rows = vec![
            row("root s", "r", "situational"),
            row("root d", "r", "direct"),
            row("x s", "r/x", "situational"),
            row("x d", "r/x", "direct"),
            row("y s", "r/y", "situational"),
            row("y d", "r/y", "direct"),
            row("git y", "r/y", "direct-tool"),
            row("nothing", "none", "none"),
        ];
        let probed: Vec<String> = crate::cmd::golden::sample_probes(&rows).into_iter().map(|p| p.way).collect();
        let left = if probed.contains(&"r/x".to_string()) { "y" } else { "x" };
        assert_eq!(fitting_prompts(&rows), vec![format!("{left} s"), format!("{left} d")]);
    }

    #[test]
    fn the_file_round_trips_and_is_refused_for_another_model() {
        let dir = std::env::temp_dir().join(format!("hubness-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let hubs = Hubs { model: "m1".into(), prompts: 3, alias: lists(&[("a", &[0.5])]), fused: lists(&[("a", &[0.4])]) };
        std::fs::write(dir.join(FILE), serde_json::to_string(&hubs).unwrap()).unwrap();
        let back: Hubs = serde_json::from_str(&std::fs::read_to_string(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(back, hubs);
        // No engine in the temp dir, so no installed model id: refused.
        assert!(load(&dir, &dir, &dir.join("way-embed")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
