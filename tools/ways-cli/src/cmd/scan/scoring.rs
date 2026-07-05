//! Batch scoring and subprocess calls for the embedding matcher (ADR-125).
//!
//! The two models (EN-only 384-dim, multilingual 768-dim) produce cosine
//! scores in different distributions. Scores are NOT comparable across
//! models — each is scored and gated independently, then the scan loop
//! fires a way if either path clears its own threshold. Confidence rises
//! when both paths agree (they're independent confirmations of the same
//! semantic match).

pub(crate) struct EmbedScores {
    /// Scores from the English model × English corpus.
    /// `None` means the engine/corpus/model is unavailable.
    pub(crate) en: Option<Vec<(String, f64)>>,
    /// Scores from the multilingual model × multilingual corpus.
    /// `None` means the engine/corpus/model is unavailable.
    pub(crate) multi: Option<Vec<(String, f64)>>,
    /// Per-model calibration `g(s) = σ(a·s + b)` loaded from the corpus manifest
    /// (ADR-156). Empty lanes when the corpus predates calibration — the matcher
    /// then treats semantic scores as absent (degraded: keyword fires open,
    /// semantic silent), never the retired raw-cosine path.
    pub(crate) calibration: ways_core::calibration::Calibration,
}

impl EmbedScores {
    /// True if at least one model produced scores.
    pub(crate) fn any_ran(&self) -> bool {
        self.en.is_some() || self.multi.is_some()
    }

    /// Calibrated relevance probability `g_en(cos)` for `way_id`, or `None` when
    /// there is no calibrated EN signal: the lane did not run, the way is not
    /// embeddable (absent from the corpus), or no EN calibration is loaded. An
    /// embeddable way that ran but is missing from the results scores as cosine
    /// 0.0 (negative cosine → strongest "unrelated"), i.e. `g_en(0.0)`.
    pub(crate) fn prob_en(&self, way_id: &str, embeddable: bool) -> Option<f64> {
        Self::prob(self.en.as_deref(), self.calibration.en.as_ref(), way_id, embeddable)
    }

    /// Calibrated relevance probability `g_multi(cos)` for `way_id`. See [`prob_en`].
    pub(crate) fn prob_multi(&self, way_id: &str, embeddable: bool) -> Option<f64> {
        Self::prob(self.multi.as_deref(), self.calibration.multi.as_ref(), way_id, embeddable)
    }

    fn prob(
        rows: Option<&[(String, f64)]>,
        cal: Option<&ways_core::calibration::ModelCalibration>,
        way_id: &str,
        embeddable: bool,
    ) -> Option<f64> {
        if !embeddable {
            return None; // not in corpus → no signal → keyword fails open
        }
        let rows = rows?; // lane did not run
        let cal = cal?; // no calibration loaded → degraded to no signal
        let cos = best_score(Some(rows), way_id).unwrap_or(0.0);
        Some(cal.probability(cos))
    }
}

fn best_score(rows: Option<&[(String, f64)]>, way_id: &str) -> Option<f64> {
    rows?
        .iter()
        .filter(|(id, _)| id == way_id)
        .map(|(_, s)| *s)
        .fold(None, |acc, s| Some(acc.map_or(s, |a: f64| a.max(s))))
}

/// Whether the multilingual matching lane is enabled.
///
/// The lane runs only in **localized mode** — `output_language` in ways.json set
/// to a specific non-English language. English mode (`en` / `auto` / unset, the
/// default) never loads the 768-dim multilingual model, regardless of whether a
/// multi corpus is present (ADR-139). Both modes still match by embedding cosine;
/// English mode just runs the 384-dim English lane alone.
pub(crate) fn multilingual_enabled(output_language: &str) -> bool {
    !matches!(output_language, "en" | "auto" | "")
}

/// Run both models against `query` and return per-model scores independently.
/// Either or both may be None if their engine/model is unavailable.
pub(crate) fn batch_embed_score(query: &str) -> EmbedScores {
    let Some(embed_bin) = find_way_embed() else {
        return EmbedScores { en: None, multi: None, calibration: Default::default() };
    };
    let xdg = crate::paths::corpus_dir();
    let calibration = load_calibration(&xdg);

    let en_corpus = xdg.join("ways-corpus-en.jsonl");
    let en_model = xdg.join("minilm-l6-v2.gguf");
    let en = run_if_ready(&embed_bin, &en_corpus, &en_model, query, "EN");

    // Mode gate (ADR-139): the multilingual lane runs only in localized mode, so
    // English-mode installs never load the heavier 768-dim model on a match —
    // gated on output_language, not on corpus-file presence.
    let multi = if multilingual_enabled(&crate::config::global().language) {
        let multi_corpus = xdg.join("ways-corpus-multi.jsonl");
        let multi_model = xdg.join("multilingual-minilm-l12-v2-q8.gguf");
        run_if_ready(&embed_bin, &multi_corpus, &multi_model, query, "multilingual")
    } else {
        None
    };

    // Legacy fallback: combined corpus + EN model if neither ran.
    if en.is_none() && multi.is_none() {
        let combined = xdg.join("ways-corpus.jsonl");
        if combined.is_file() && en_model.is_file() {
            let fallback = run_embed_match(&embed_bin, &combined, &en_model, query);
            return EmbedScores { en: fallback, multi: None, calibration };
        }
    }

    EmbedScores { en, multi, calibration }
}

/// Load per-model calibration (ADR-156) from the corpus manifest
/// (`embed-manifest.json`, `calibration` block). Returns empty lanes when the
/// manifest is absent, unparseable, or predates calibration — the matcher then
/// degrades (keyword fires open, semantic silent) rather than falling back to
/// the retired raw-cosine path.
fn load_calibration(xdg: &std::path::Path) -> ways_core::calibration::Calibration {
    #[derive(serde::Deserialize)]
    struct Manifest {
        #[serde(default)]
        calibration: ways_core::calibration::Calibration,
    }
    std::fs::read_to_string(xdg.join("embed-manifest.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Manifest>(&s).ok())
        .map(|m| m.calibration)
        .unwrap_or_default()
}

fn run_if_ready(
    bin: &std::path::Path,
    corpus: &std::path::Path,
    model: &std::path::Path,
    query: &str,
    label: &str,
) -> Option<Vec<(String, f64)>> {
    if !corpus.is_file() || !has_entries(corpus) {
        return None;
    }
    if !model.is_file() {
        eprintln!(
            "WARNING: {} {} ways in corpus but model missing ({})",
            line_count(corpus),
            label,
            model.display()
        );
        eprintln!("  Run: make setup");
        return None;
    }
    run_embed_match(bin, corpus, model, query)
}

/// Run way-embed match against a single corpus/model pair.
///
/// Passes `--threshold 0.0` so way-embed returns every score. Per-way
/// thresholds and parent-boost (ADR-125) are applied in Rust at match time.
fn run_embed_match(
    bin: &std::path::Path,
    corpus: &std::path::Path,
    model: &std::path::Path,
    query: &str,
) -> Option<Vec<(String, f64)>> {
    let output = std::process::Command::new(bin)
        .args([
            "match",
            "--corpus", corpus.to_str()?,
            "--model", model.to_str()?,
            "--query", query,
            "--threshold", "0.0",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let id = parts.next()?.to_string();
                let score: f64 = parts.next()?.parse().ok()?;
                Some((id, score))
            })
            .collect(),
    )
}

fn has_entries(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path)
        .map(|c| c.lines().any(|l| !l.is_empty()))
        .unwrap_or(false)
}

fn line_count(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path)
        .map(|c| c.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0)
}

pub(crate) fn find_way_embed() -> Option<std::path::PathBuf> {
    let xdg = crate::paths::corpus_dir().join("way-embed");
    if xdg.is_file() { return Some(xdg); }
    let bin = crate::util::normalize_path_sep(&home_dir().join(".claude/bin/way-embed"));
    if bin.is_file() { return Some(bin); }
    None
}

// ── In-process show capture ───────────────────────────────────

pub(crate) fn capture_show_way(
    id: &str,
    session_id: &str,
    trigger: &str,
    fire_score: Option<f64>,
    matched_span: Option<&str>,
) -> String {
    crate::cmd::show::way_scored(id, session_id, trigger, fire_score, matched_span)
        .unwrap_or_default()
}

pub(crate) fn capture_show_check(id: &str, session_id: &str, trigger: &str, score: f64) -> String {
    crate::cmd::show::check(id, session_id, trigger, score).unwrap_or_default()
}

// ── Path helpers ───────────────────────────────────────────────

pub(crate) fn default_project() -> String {
    std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()))
}

pub(crate) use crate::util::home_dir;

#[cfg(test)]
mod tests {
    use super::multilingual_enabled;

    #[test]
    fn english_mode_disables_multilingual_lane() {
        // en / auto / unset are all English mode — the 768-dim model never loads.
        assert!(!multilingual_enabled("en"));
        assert!(!multilingual_enabled("auto"));
        assert!(!multilingual_enabled(""));
    }

    #[test]
    fn localized_mode_enables_multilingual_lane() {
        assert!(multilingual_enabled("es"));
        assert!(multilingual_enabled("zh"));
        assert!(multilingual_enabled("pt-br"));
    }
}
