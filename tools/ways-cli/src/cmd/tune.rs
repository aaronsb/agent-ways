//! Locale alias fidelity + discrimination audit (ADR-125).
//!
//! Two measurements per locale entry, run on the same query (the locale's
//! own description + vocabulary):
//!
//! **Fidelity** — cosine alignment to the English root (ADR-139). The English
//! frontmatter is embedded into the multilingual corpus as the per-way anchor,
//! so for a single localized language an alias's only same-way peer IS the root:
//! `min_peer` measures how well the translation tracks the source of truth.
//! Low fidelity → the stub drifted from the English meaning; fix by re-authoring.
//!
//! **Discrimination** — best score against any *other* way's alias,
//! minus the stub's own self-match. If another way scores higher than
//! this stub's peers, the stub is being outranked by a confuser (the
//! "mocking beats commits in ru" failure mode). Fix by sharpening
//! vocabulary or by splitting/merging neighboring ways.
//!
//! Neither measurement writes thresholds; both inform re-authoring.
//!
//! Parallelized: one way per thread, n_cores - 4 workers.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;

use crate::frontmatter;
use crate::util::{home_dir, xdg_cache_dir};

#[derive(Clone)]
struct FidelityResult {
    way_id: String,
    lang: String,
    /// Min cosine against peer aliases on the same way (tightest cross-lingual positive)
    min_peer: f64,
    /// Mean cosine across peer aliases
    mean_peer: f64,
    /// How many peer aliases scored
    peer_count: usize,
    /// Best cosine against any alias on a *different* way (the top confuser)
    top_confuser: Option<Confuser>,
    /// Gap = min_peer - top_confuser.score. Negative means a confuser outranks
    /// the weakest same-way peer (the locale stub is being dominated).
    discrimination_gap: f64,
}

#[derive(Clone)]
struct Confuser {
    way_id: String,
    score: f64,
}

pub fn run(
    ways_dir: Option<String>,
    way_filter: Option<String>,
    lang: Option<String>,
    fidelity_threshold: f64,
    discrimination_threshold: f64,
    json_output: bool,
) -> Result<()> {
    // Mode gate (ADR-139): tune is a localized-mode concern. The target language is
    // the explicit --lang, else the active localized language. English mode (en /
    // auto) has nothing to audit — return clean, not an error.
    let target_lang = match lang.or_else(|| {
        crate::config::global().localized_language().map(|s| s.to_string())
    }) {
        Some(l) => l,
        None => {
            eprintln!("English mode (output_language = en / auto): no localized language to audit.");
            eprintln!("Localize first via the ways-localize skill; `ways tune` then audits the result.");
            return Ok(());
        }
    };

    let global_dir = ways_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude/hooks/ways"));
    let xdg_way = xdg_cache_dir().join("agent-ways/user");

    let multi_corpus = xdg_way.join("ways-corpus-multi.jsonl");
    let multi_model = xdg_way.join("multilingual-minilm-l12-v2-q8.gguf");

    if !multi_corpus.is_file() {
        eprintln!("No multilingual corpus to audit — run `ways corpus` (localized mode) first.");
        return Ok(());
    }
    if !multi_model.is_file() {
        eprintln!("Multilingual model not installed — run `make -C tools/way-embed model-multilingual`.");
        return Ok(());
    }

    let embed_bin = find_way_embed()
        .context("way-embed binary not found. Run `make setup` to install.")?;

    let excluded = crate::util::load_excluded_segments();
    let locale_files = collect_locale_files(&global_dir, way_filter.as_deref(), &excluded)?;
    if locale_files.is_empty() {
        eprintln!("No localized stubs found for '{target_lang}'. Run the ways-localize workflow.");
        return Ok(());
    }

    let n_cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let n_workers = n_cores.saturating_sub(4).max(1);

    eprintln!(
        "Measuring alias fidelity for {} ways across {} threads...",
        locale_files.len(),
        n_workers
    );

    let total = locale_files.len();
    let queue: Arc<Mutex<Vec<(String, PathBuf)>>> = Arc::new(Mutex::new(locale_files));
    let completed: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let failed: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let results: Arc<Mutex<Vec<FidelityResult>>> = Arc::new(Mutex::new(Vec::new()));

    let mut handles = Vec::new();
    for _ in 0..n_workers {
        let queue = Arc::clone(&queue);
        let results = Arc::clone(&results);
        let completed = Arc::clone(&completed);
        let failed = Arc::clone(&failed);
        let embed_bin = embed_bin.clone();
        let multi_corpus = multi_corpus.clone();
        let multi_model = multi_model.clone();
        let target_lang = target_lang.clone();

        handles.push(std::thread::spawn(move || loop {
            let item = { queue.lock().unwrap().pop() };
            let (way_id, locale_path) = match item {
                Some(x) => x,
                None => break,
            };

            match measure_way(&way_id, &locale_path, &embed_bin, &multi_corpus, &multi_model, &target_lang) {
                Ok(mut ws) => {
                    let mut r = results.lock().unwrap();
                    r.append(&mut ws);
                }
                Err(e) => {
                    eprintln!("\nERROR measuring {way_id}: {e}");
                    *failed.lock().unwrap() += 1;
                }
            }

            let done = {
                let mut c = completed.lock().unwrap();
                *c += 1;
                *c
            };
            eprint!("\r  {done}/{total} ways");
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
    eprintln!();

    let fail_count = *failed.lock().unwrap();
    if fail_count > 0 {
        eprintln!("WARNING: {fail_count} ways failed to measure");
    }

    let mut all_results = Arc::try_unwrap(results)
        .map_err(|_| anyhow::anyhow!("failed to unwrap results"))?
        .into_inner()
        .unwrap();
    all_results.sort_by(|a, b| a.way_id.cmp(&b.way_id).then(a.lang.cmp(&b.lang)));

    if json_output {
        emit_json(&all_results, fidelity_threshold, discrimination_threshold)?;
    } else {
        emit_report(&all_results, fidelity_threshold, discrimination_threshold);
    }

    Ok(())
}

fn collect_locale_files(
    global_dir: &Path,
    way_filter: Option<&str>,
    excluded: &[String],
) -> Result<Vec<(String, PathBuf)>> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for entry in WalkDir::new(global_dir).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".locales.jsonl")) {
            continue;
        }
        if crate::util::is_excluded_path(path, excluded) {
            continue;
        }

        let parent = path.parent().unwrap_or(Path::new(""));
        let rel = parent.strip_prefix(global_dir).unwrap_or(parent);
        let way_id = rel.display().to_string();

        if let Some(filter) = way_filter {
            if !way_id.contains(filter) {
                continue;
            }
        }
        files.push((way_id, path.to_path_buf()));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    // Empty is a clean state (English mode / not yet localized); the caller decides.
    Ok(files)
}

/// Measure fidelity for every locale alias on a single way.
fn measure_way(
    way_id: &str,
    locale_path: &Path,
    embed_bin: &Path,
    multi_corpus: &Path,
    multi_model: &Path,
    target_lang: &str,
) -> Result<Vec<FidelityResult>> {
    let entries: Vec<frontmatter::LocaleEntry> = frontmatter::parse_locales_jsonl(locale_path)?
        .into_iter()
        .filter(|e| e.lang == target_lang)
        .collect();

    let mut out = Vec::with_capacity(entries.len());

    for entry in &entries {
        let query = format!(
            "{} {}",
            entry.description,
            entry.vocabulary.as_deref().unwrap_or("")
        );

        let output = Command::new(embed_bin)
            .args([
                "match",
                "--corpus", multi_corpus.to_str().unwrap(),
                "--model", multi_model.to_str().unwrap(),
                "--query", &query,
                "--threshold", "0.0",
            ])
            .output()
            .with_context(|| format!("way-embed match for {way_id}/{lang}", lang = entry.lang))?;

        if !output.status.success() {
            continue;
        }

        // Collect same-way peer scores (excluding self-row at ~1.0) and
        // best non-self score (the top confuser — another way's alias that
        // competes with this stub in embedding space).
        let mut peer_scores: Vec<f64> = Vec::new();
        let mut top_confuser: Option<Confuser> = None;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.split('\t');
            let id = match parts.next() {
                Some(s) => s,
                None => continue,
            };
            let score: f64 = match parts.next().and_then(|s| s.parse().ok()) {
                Some(s) => s,
                None => continue,
            };

            if id == way_id {
                if score > 0.999 {
                    continue; // self-match
                }
                peer_scores.push(score);
            } else if top_confuser.as_ref().is_none_or(|c| score > c.score) {
                top_confuser = Some(Confuser { way_id: id.to_string(), score });
            }
        }

        let (min_peer, mean_peer) = if peer_scores.is_empty() {
            (f64::NAN, f64::NAN)
        } else {
            let min = peer_scores.iter().cloned().fold(f64::INFINITY, f64::min);
            let mean = peer_scores.iter().sum::<f64>() / peer_scores.len() as f64;
            (min, mean)
        };

        let discrimination_gap = match (&top_confuser, peer_scores.is_empty()) {
            (Some(c), false) => min_peer - c.score,
            _ => f64::NAN,
        };

        out.push(FidelityResult {
            way_id: way_id.to_string(),
            lang: entry.lang.clone(),
            min_peer,
            mean_peer,
            peer_count: peer_scores.len(),
            top_confuser,
            discrimination_gap,
        });
    }

    Ok(out)
}

fn emit_report(results: &[FidelityResult], fidelity_threshold: f64, discrimination_threshold: f64) {
    use agent_fmt::{Align, Table};

    // Entries to flag: low fidelity OR low/negative discrimination.
    let flagged: Vec<&FidelityResult> = results
        .iter()
        .filter(|r| {
            r.peer_count > 0
                && (r.min_peer < fidelity_threshold
                    || (!r.discrimination_gap.is_nan()
                        && r.discrimination_gap < discrimination_threshold))
        })
        .collect();

    println!("Locale Alias Fidelity + Discrimination");
    println!("======================================");
    println!();

    let total = results.iter().filter(|r| r.peer_count > 0).count();
    println!(
        "{}/{} entries flagged (fidelity < {:.2} or discrimination gap < {:.2})",
        flagged.len(),
        total,
        fidelity_threshold,
        discrimination_threshold
    );
    println!();
    println!("Fidelity     = cosine alignment to the English root anchor (how well the translation tracks the source of truth).");
    println!("Discrimination = min_peer − top_confuser.score (how clearly this stub outranks other ways).");
    println!("A negative discrimination gap means another way outranks this locale's own peers.");
    println!();

    if flagged.is_empty() {
        println!("All aliases pass both checks. No re-authoring needed.");
        return;
    }

    let mut t = Table::new(&["Way", "Lang", "MinPeer", "MeanPeer", "Gap", "Top confuser"]);
    t.align(2, Align::Right);
    t.align(3, Align::Right);
    t.align(4, Align::Right);

    for r in &flagged {
        let gap_str = if r.discrimination_gap.is_nan() {
            "—".to_string()
        } else {
            format!("{:+.4}", r.discrimination_gap)
        };
        let confuser_str = match &r.top_confuser {
            Some(c) => format!("{} ({:.4})", c.way_id, c.score),
            None => "—".to_string(),
        };
        t.add_owned(vec![
            r.way_id.clone(),
            r.lang.clone(),
            format!("{:.4}", r.min_peer),
            format!("{:.4}", r.mean_peer),
            gap_str,
            confuser_str,
        ]);
    }

    t.print();
    println!();
}

fn emit_json(
    results: &[FidelityResult],
    fidelity_threshold: f64,
    discrimination_threshold: f64,
) -> Result<()> {
    let rows: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            let below_fidelity = r.peer_count > 0 && r.min_peer < fidelity_threshold;
            let below_discrimination = !r.discrimination_gap.is_nan()
                && r.discrimination_gap < discrimination_threshold;
            serde_json::json!({
                "way": r.way_id,
                "lang": r.lang,
                "min_peer": nan_to_null(r.min_peer),
                "mean_peer": nan_to_null(r.mean_peer),
                "peer_count": r.peer_count,
                "top_confuser": r.top_confuser.as_ref().map(|c| serde_json::json!({
                    "way": c.way_id,
                    "score": c.score,
                })),
                "discrimination_gap": nan_to_null(r.discrimination_gap),
                "below_fidelity": below_fidelity,
                "below_discrimination": below_discrimination,
                "flagged": below_fidelity || below_discrimination,
            })
        })
        .collect();
    let out = serde_json::json!({
        "fidelity_threshold": fidelity_threshold,
        "discrimination_threshold": discrimination_threshold,
        "entries": rows,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn nan_to_null(x: f64) -> serde_json::Value {
    if x.is_nan() {
        serde_json::Value::Null
    } else {
        serde_json::json!(x)
    }
}

fn find_way_embed() -> Option<PathBuf> {
    let xdg = xdg_cache_dir().join("agent-ways/user/way-embed");
    if xdg.is_file() {
        return Some(xdg);
    }
    let bin = home_dir().join(".claude/bin/way-embed");
    if bin.is_file() {
        return Some(bin);
    }
    None
}
