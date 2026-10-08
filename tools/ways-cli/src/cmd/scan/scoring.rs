//! Batch scoring and subprocess calls for the embedding matcher (ADR-125).
//!
//! The two models (EN-only 384-dim, multilingual 768-dim) produce cosine
//! scores in different distributions. Scores are NOT comparable across
//! models — each is scored and gated independently, then the scan loop
//! fires a way if either path clears its own threshold. Confidence rises
//! when both paths agree (they're independent confirmations of the same
//! semantic match).

use crate::cmd::show::ContextBudget;

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

/// Where a probe run points the scan: a ways root and the corpus artifacts built
/// from it, in place of the shipped ways and the canonical corpus. Set once per
/// thread by `ways author probe`; the hooks never set it.
pub(crate) struct Isolation {
    pub ways_dir: std::path::PathBuf,
    pub artifacts: std::path::PathBuf,
}

thread_local! {
    // Per thread, so a test that isolates its scan cannot leak into another.
    // A probe run is single-threaded.
    static ISOLATION: std::cell::RefCell<Option<Isolation>> = const { std::cell::RefCell::new(None) };
}

/// Point this thread's scans at `isolation`.
pub(crate) fn isolate(isolation: Isolation) {
    ISOLATION.with(|i| *i.borrow_mut() = Some(isolation));
}

/// The isolated ways root, when a probe run set one.
pub(crate) fn isolated_ways_dir() -> Option<std::path::PathBuf> {
    ISOLATION.with(|i| i.borrow().as_ref().map(|i| i.ways_dir.clone()))
}

/// The directory holding the corpus, manifest and body sidecar the scan reads:
/// the isolated one when set, else the canonical engine dir. Models and the
/// `way-embed` binary always come from the canonical engine dir.
pub(crate) fn artifact_dir() -> std::path::PathBuf {
    ISOLATION
        .with(|i| i.borrow().as_ref().map(|i| i.artifacts.clone()))
        .unwrap_or_else(crate::paths::corpus_dir)
}

/// Whether the multilingual matching lane is enabled.
///
/// The lane runs only in **localized mode** — `output_language` in the user config set
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
    batch_embed_score_with(query, None)
}

/// Score `query` against an explicit corpus JSONL instead of the canonical one.
///
/// `corpus` is the English-lane corpus file. The multilingual lane reads
/// `ways-corpus-multi.jsonl` as its sibling in the same directory, so a corpus
/// built by `ways corpus --output DIR` scores as a unit. Models and calibration
/// still come from the canonical corpus dir. An isolated corpus holds way
/// entries; `make setup` is what puts the weights on disk.
pub(crate) fn batch_embed_score_with(
    query: &str,
    corpus: Option<&std::path::Path>,
) -> EmbedScores {
    let Some(embed_bin) = crate::paths::way_embed() else {
        return EmbedScores { en: None, multi: None, calibration: Default::default() };
    };
    let xdg = crate::paths::corpus_dir();
    let artifacts = artifact_dir();
    let calibration = load_calibration(&artifacts);

    let en_corpus = match corpus {
        Some(p) => p.to_path_buf(),
        None => artifacts.join("ways-corpus-en.jsonl"),
    };
    let en_model = xdg.join(crate::paths::EN_MODEL);
    let en = run_if_ready(&embed_bin, &en_corpus, &en_model, query, "EN");

    // Mode gate (ADR-139): the multilingual lane runs only in localized mode, so
    // English-mode installs never load the heavier 768-dim model on a match —
    // gated on output_language, not on corpus-file presence.
    let multi = if multilingual_enabled(&crate::config::global().language) {
        let multi_corpus = sibling_corpus(corpus, &xdg, "ways-corpus-multi.jsonl");
        let multi_model = xdg.join(crate::paths::MULTI_MODEL);
        run_if_ready(&embed_bin, &multi_corpus, &multi_model, query, "multilingual")
    } else {
        None
    };

    // Legacy fallback: combined corpus + EN model if neither ran. An explicit
    // `--corpus` names the file to score, so it gets no substitute.
    if en.is_none() && multi.is_none() && corpus.is_none() {
        let combined = xdg.join("ways-corpus.jsonl");
        if combined.is_file() && en_model.is_file() {
            let fallback = way_embed_match(&embed_bin, &combined, &en_model, query).ok().flatten();
            return EmbedScores { en: fallback, multi: None, calibration };
        }
    }

    EmbedScores { en, multi, calibration }
}

/// Resolve `name` next to an explicit corpus file, falling back to the canonical
/// corpus dir when no corpus was given or it has no parent directory.
pub(crate) fn sibling_corpus(
    corpus: Option<&std::path::Path>,
    xdg: &std::path::Path,
    name: &str,
) -> std::path::PathBuf {
    corpus
        .and_then(|p| p.parent())
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(xdg)
        .join(name)
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
    way_embed_match(bin, corpus, model, query).ok().flatten()
}

/// `way-embed match` for one query against one corpus/model pair: every
/// `(way_id, cosine)` row way-embed prints, in its order. The one runner for
/// the single-query mode (the matcher and `ways tune locale`).
///
/// Passes `--threshold 0.0` so way-embed returns every score. Per-way
/// thresholds and parent-boost (ADR-125) are applied in Rust at match time.
/// `Err` when the binary cannot be run; `Ok(None)` when it exits non-zero.
/// A row that does not parse as `id<TAB>score` is skipped.
pub(crate) fn way_embed_match(
    bin: &std::path::Path,
    corpus: &std::path::Path,
    model: &std::path::Path,
    query: &str,
) -> std::io::Result<Option<Vec<(String, f64)>>> {
    let output = std::process::Command::new(bin)
        .arg("match")
        .arg("--corpus")
        .arg(corpus)
        .arg("--model")
        .arg(model)
        .args(["--query", query, "--threshold", "0.0"])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    Ok(Some(parse_match_rows(&String::from_utf8_lossy(&output.stdout))))
}

/// `id<TAB>score` rows from `way-embed match`, skipping any that do not parse.
fn parse_match_rows(stdout: &str) -> Vec<(String, f64)> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let id = parts.next()?.to_string();
            let score: f64 = parts.next()?.parse().ok()?;
            Some((id, score))
        })
        .collect()
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

// ── In-process show capture ───────────────────────────────────

use crate::cmd::show::{ShowOutcome, Shown};

/// [`crate::cmd::show::way_scored`] with its error absorbed: a way that fails on
/// the fire path shows nothing and is recorded as [`ShowOutcome::Error`].
pub(crate) fn capture_show_way(
    id: &str,
    session_id: &str,
    trigger: &str,
    fire_score: Option<f64>,
    matched_span: Option<&str>,
    surface: Option<&str>,
    budget: Option<&mut ContextBudget>,
) -> Shown {
    crate::cmd::show::way_scored(id, session_id, trigger, fire_score, matched_span, surface, budget)
        .unwrap_or(Shown { body: String::new(), outcome: ShowOutcome::Error })
}

pub(crate) fn capture_show_check(
    id: &str,
    session_id: &str,
    trigger: &str,
    score: f64,
    budget: Option<&mut ContextBudget>,
) -> String {
    crate::cmd::show::check_within(id, session_id, trigger, score, budget).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{multilingual_enabled, parse_match_rows, sibling_corpus};

    #[test]
    fn match_rows_parse_and_skip_junk() {
        let rows = parse_match_rows("a/b\t0.5\nnoise\nc\tnan-ish\nd\t0.25\textra\n");
        assert_eq!(rows, vec![("a/b".to_string(), 0.5), ("d".to_string(), 0.25)]);
    }
    use std::path::Path;

    #[test]
    fn sibling_resolves_next_to_an_explicit_corpus() {
        let given = Path::new("/tmp/iso/ways-corpus.jsonl");
        let canonical = Path::new("/home/u/.cache/agent-ways/user");
        assert_eq!(
            sibling_corpus(Some(given), canonical, "ways-corpus-en.jsonl"),
            Path::new("/tmp/iso/ways-corpus-en.jsonl")
        );
    }

    #[test]
    fn sibling_falls_back_to_the_canonical_dir() {
        let canonical = Path::new("/home/u/.cache/agent-ways/user");
        assert_eq!(
            sibling_corpus(None, canonical, "ways-corpus-en.jsonl"),
            canonical.join("ways-corpus-en.jsonl")
        );
        // A bare filename has an empty parent; the canonical dir covers it.
        let bare = Path::new("ways-corpus.jsonl");
        assert_eq!(
            sibling_corpus(Some(bare), canonical, "ways-corpus-en.jsonl"),
            canonical.join("ways-corpus-en.jsonl")
        );
    }

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
