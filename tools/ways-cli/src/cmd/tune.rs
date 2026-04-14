//! Tune embed_threshold values for locale stubs.
//!
//! Two modes:
//! - **Tune** (default): compute optimal thresholds per locale entry
//! - **Audit** (`--audit`): surface entries with low discrimination —
//!   where the description doesn't clearly separate this way from others
//!
//! Parallelized: uses all cores minus 4, one way per thread.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;

use crate::frontmatter;
use crate::util::{home_dir, xdg_cache_dir};

/// A confuser: a non-self way that scores close to self.
#[derive(Clone)]
struct Confuser {
    way_id: String,
    score: f64,
}

#[derive(Clone)]
struct TuneResult {
    way_id: String,
    lang: String,
    current: f64,
    optimal: f64,
    best_self: f64,
    best_non_self: Option<f64>,
    /// Gap between self and best non-self (discrimination signal)
    gap: f64,
    changed: bool,
    /// Top 3 closest non-self ways
    confusers: Vec<Confuser>,
}

struct WayTuneResult {
    way_id: String,
    locale_path: PathBuf,
    results: Vec<TuneResult>,
    tuned_entries: Vec<frontmatter::LocaleEntry>,
    original_entries: Vec<frontmatter::LocaleEntry>,
}

pub fn run(
    ways_dir: Option<String>,
    way_filter: Option<String>,
    apply: bool,
    audit: bool,
    audit_threshold: f64,
    margin: f64,
    json_output: bool,
) -> Result<()> {
    let global_dir = ways_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude/hooks/ways"));
    let xdg_way = xdg_cache_dir().join("claude-ways/user");

    let multi_corpus = xdg_way.join("ways-corpus-multi.jsonl");
    let multi_model = xdg_way.join("multilingual-minilm-l12-v2-q8.gguf");

    if !multi_corpus.is_file() {
        bail!("Multilingual corpus not found. Run `ways corpus` first.");
    }
    if !multi_model.is_file() {
        bail!("Multilingual model not found. Run `make setup` first.");
    }

    let embed_bin = find_way_embed()
        .context("way-embed binary not found. Run `make setup` to install.")?;

    let excluded = crate::util::load_excluded_segments();

    // Collect all .locales.jsonl files
    let mut locale_files: Vec<(String, PathBuf)> = Vec::new();
    for entry in WalkDir::new(&global_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !fname.ends_with(".locales.jsonl") {
            continue;
        }
        if crate::util::is_excluded_path(path, &excluded) {
            continue;
        }

        let parent = path.parent().unwrap_or(Path::new(""));
        let rel = parent.strip_prefix(&global_dir).unwrap_or(parent);
        let way_id = rel.display().to_string();

        if let Some(ref filter) = way_filter {
            if !way_id.contains(filter.as_str()) {
                continue;
            }
        }

        locale_files.push((way_id, path.to_path_buf()));
    }
    locale_files.sort_by(|a, b| a.0.cmp(&b.0));

    if locale_files.is_empty() {
        if way_filter.is_some() {
            bail!("No .locales.jsonl files matched filter");
        }
        bail!("No .locales.jsonl files found");
    }

    // Parallelism: all cores minus 4 (leave headroom), minimum 1
    let n_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let n_workers = n_cores.saturating_sub(4).max(1);

    let total_ways = locale_files.len();
    eprintln!(
        "{} {} ways across {} threads...",
        if audit { "Auditing" } else { "Tuning" },
        total_ways,
        n_workers
    );

    // Shared state
    let work_queue: Arc<Mutex<Vec<(String, PathBuf)>>> = Arc::new(Mutex::new(locale_files));
    let completed: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let failed: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let all_results: Arc<Mutex<Vec<WayTuneResult>>> = Arc::new(Mutex::new(Vec::new()));

    // Spawn workers
    let mut handles = Vec::new();
    for _ in 0..n_workers {
        let queue = Arc::clone(&work_queue);
        let results = Arc::clone(&all_results);
        let completed = Arc::clone(&completed);
        let failed = Arc::clone(&failed);
        let embed_bin = embed_bin.clone();
        let multi_corpus = multi_corpus.clone();
        let multi_model = multi_model.clone();

        let handle = std::thread::spawn(move || {
            loop {
                let item = {
                    let mut q = queue.lock().unwrap();
                    q.pop()
                };

                let (way_id, locale_path) = match item {
                    Some(i) => i,
                    None => break,
                };

                match tune_way(&way_id, &locale_path, &embed_bin, &multi_corpus, &multi_model, margin) {
                    Ok(result) => {
                        let mut r = results.lock().unwrap();
                        r.push(result);
                    }
                    Err(e) => {
                        eprintln!("\nERROR tuning {}: {}", way_id, e);
                        let mut f = failed.lock().unwrap();
                        *f += 1;
                    }
                }

                let done = {
                    let mut c = completed.lock().unwrap();
                    *c += 1;
                    *c
                };
                eprint!("\r  {}/{} ways", done, total_ways);
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }
    eprintln!();

    let fail_count = *failed.lock().unwrap();
    if fail_count > 0 {
        eprintln!("WARNING: {} ways failed to tune", fail_count);
    }

    // Collect and sort
    let mut way_results = Arc::try_unwrap(all_results)
        .map_err(|_| anyhow::anyhow!("failed to unwrap results"))
        .unwrap()
        .into_inner()
        .unwrap();
    way_results.sort_by(|a, b| a.way_id.cmp(&b.way_id));

    let mut all_tune_results: Vec<TuneResult> = Vec::new();
    let mut files_to_update: BTreeMap<PathBuf, Vec<frontmatter::LocaleEntry>> = BTreeMap::new();

    for wr in &way_results {
        all_tune_results.extend(wr.results.clone());

        let any_changed = wr.tuned_entries.iter().any(|e| {
            let orig = wr.original_entries.iter().find(|o| o.lang == e.lang);
            orig.is_some_and(|o| o.embed_threshold != e.embed_threshold)
        });
        if any_changed {
            files_to_update.insert(wr.locale_path.clone(), wr.tuned_entries.clone());
        }
    }

    // Output
    if audit {
        output_audit(&all_tune_results, audit_threshold, json_output)?;
    } else {
        output_tune(&all_tune_results, apply, json_output)?;
    }

    // Apply if requested (tune mode only)
    if !audit && apply {
        let changed_count = all_tune_results.iter().filter(|r| r.changed).count();
        if changed_count > 0 {
            for (path, entries) in &files_to_update {
                let mut lines: Vec<String> = Vec::new();
                for entry in entries {
                    lines.push(serde_json::to_string(entry)?);
                }
                lines.sort();
                let content = lines.join("\n") + "\n";
                std::fs::write(path, content)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            eprintln!("Updated {} .locales.jsonl files", files_to_update.len());
            eprintln!("Run `ways corpus` to regenerate the corpus with tuned thresholds.");
        }
    }

    Ok(())
}

/// Standard tune output: threshold table.
fn output_tune(results: &[TuneResult], apply: bool, json_output: bool) -> Result<()> {
    let changed_count = results.iter().filter(|r| r.changed).count();

    if json_output {
        let json_results: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "way": r.way_id,
                    "lang": r.lang,
                    "current": r.current,
                    "optimal": r.optimal,
                    "self_score": r.best_self,
                    "best_non_self": r.best_non_self,
                    "gap": r.gap,
                    "changed": r.changed,
                    "confusers": r.confusers.iter().map(|c| {
                        serde_json::json!({"way": c.way_id, "score": c.score})
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
    } else {
        let mut table =
            agent_fmt::Table::new(&["Way", "Lang", "Current", "Optimal", "Self", "Noise", "Gap", "Δ"]);
        table.max_width(0, 40);

        for r in results {
            let delta = if r.changed {
                format!("{:+.2}", r.optimal - r.current)
            } else {
                "—".to_string()
            };
            let noise = r
                .best_non_self
                .map_or("—".to_string(), |s| format!("{:.4}", s));

            table.add(vec![
                &r.way_id,
                &r.lang,
                &format!("{:.2}", r.current),
                &format!("{:.2}", r.optimal),
                &format!("{:.4}", r.best_self),
                &noise,
                &format!("{:.2}", r.gap),
                &delta,
            ]);
        }
        table.print();

        println!();
        println!("{} entries analyzed, {} would change", results.len(), changed_count);

        if changed_count > 0 && !apply {
            println!();
            println!("Run with --apply to write tuned thresholds to .locales.jsonl files.");
            println!("Then run `ways corpus` to regenerate the corpus.");
        }
    }

    Ok(())
}

/// Audit output: surface low-discrimination entries with confusers.
fn output_audit(results: &[TuneResult], min_gap: f64, json_output: bool) -> Result<()> {
    let mut flagged: Vec<&TuneResult> = results
        .iter()
        .filter(|r| r.gap < min_gap)
        .collect();
    flagged.sort_by(|a, b| a.gap.partial_cmp(&b.gap).unwrap_or(std::cmp::Ordering::Equal));

    if json_output {
        let json_results: Vec<serde_json::Value> = flagged
            .iter()
            .map(|r| {
                serde_json::json!({
                    "way": r.way_id,
                    "lang": r.lang,
                    "gap": r.gap,
                    "self_score": r.best_self,
                    "best_non_self": r.best_non_self,
                    "confusers": r.confusers.iter().map(|c| {
                        serde_json::json!({"way": c.way_id, "score": c.score})
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
        return Ok(());
    }

    if flagged.is_empty() {
        println!("No entries with discrimination gap < {:.2}", min_gap);
        println!("All locale descriptions have clear separation from neighbors.");
        return Ok(());
    }

    println!("Discrimination Audit");
    println!("====================");
    println!();
    println!(
        "{} of {} entries have gap < {:.2} (ambiguous — description doesn't clearly",
        flagged.len(),
        results.len(),
        min_gap,
    );
    println!("separate this way from others. Consider revising description/vocabulary.)");
    println!();

    // Group by way for cleaner output
    let mut current_way = "";
    for r in &flagged {
        if r.way_id != current_way {
            if !current_way.is_empty() {
                println!();
            }
            current_way = &r.way_id;
            println!("  {} ", current_way);
        }

        let confuser_str = r
            .confusers
            .iter()
            .map(|c| format!("{} ({:.2})", c.way_id, c.score))
            .collect::<Vec<_>>()
            .join(", ");

        println!(
            "    {} — gap {:.2}  (self {:.2}, noise {:.2})  confused with: {}",
            r.lang,
            r.gap,
            r.best_self,
            r.best_non_self.unwrap_or(0.0),
            confuser_str,
        );
    }

    println!();
    println!("To fix: revise the description or vocabulary in the .locales.jsonl to");
    println!("better distinguish from confusers. Then re-run `ways tune` to update thresholds.");

    // Summary stats
    let total = results.len();
    let clear = total - flagged.len();
    println!();
    println!(
        "Summary: {} clear, {} ambiguous, {} total",
        clear,
        flagged.len(),
        total
    );

    Ok(())
}

/// Tune all locale entries for a single way.
fn tune_way(
    way_id: &str,
    locale_path: &Path,
    embed_bin: &Path,
    multi_corpus: &Path,
    multi_model: &Path,
    margin: f64,
) -> Result<WayTuneResult> {
    let all_entries = frontmatter::parse_locales_jsonl(locale_path)?;
    // Only tune active languages
    let entries: Vec<frontmatter::LocaleEntry> = all_entries
        .into_iter()
        .filter(|e| crate::agents::is_language_active(&e.lang))
        .collect();
    let mut results: Vec<TuneResult> = Vec::new();
    let mut tuned_entries: Vec<frontmatter::LocaleEntry> = Vec::new();

    for entry in &entries {
        let query = format!(
            "{} {}",
            entry.description,
            entry.vocabulary.as_deref().unwrap_or("")
        );

        let output = Command::new(embed_bin)
            .args([
                "match",
                "--corpus",
                multi_corpus.to_str().unwrap(),
                "--model",
                multi_model.to_str().unwrap(),
                "--query",
                &query,
                "--threshold",
                "0.0",
            ])
            .output()
            .with_context(|| format!("way-embed match for {}/{}", way_id, entry.lang))?;

        if !output.status.success() {
            tuned_entries.push(entry.clone());
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut self_scores: Vec<f64> = Vec::new();
        let mut non_self_entries: Vec<(String, f64)> = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 {
                continue;
            }
            let id = parts[0];
            let score: f64 = match parts[1].parse() {
                Ok(s) => s,
                Err(_) => continue,
            };

            if id == way_id {
                self_scores.push(score);
            } else {
                non_self_entries.push((id.to_string(), score));
            }
        }

        // Sort non-self by score descending, dedup by way_id (keep best)
        non_self_entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut seen = std::collections::HashSet::new();
        let mut top_confusers: Vec<Confuser> = Vec::new();
        for (id, score) in &non_self_entries {
            if seen.insert(id.clone()) {
                top_confusers.push(Confuser {
                    way_id: id.clone(),
                    score: *score,
                });
                if top_confusers.len() >= 3 {
                    break;
                }
            }
        }

        // Treat all same-way stubs as positives (cross-lingual equivalence):
        // the threshold must admit the *tightest* positive, so use min not max.
        let min_self = self_scores
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);
        let best_self = if min_self.is_finite() {
            min_self
        } else {
            f64::NEG_INFINITY
        };
        let best_non_self = non_self_entries
            .first()
            .map(|(_, s)| *s)
            .unwrap_or(f64::NEG_INFINITY);

        let gap = if best_non_self > f64::NEG_INFINITY && best_self > f64::NEG_INFINITY {
            best_self - best_non_self
        } else if best_self > f64::NEG_INFINITY {
            best_self // no confusers at all = perfect discrimination
        } else {
            0.0
        };

        let optimal = if best_self > f64::NEG_INFINITY && best_non_self > f64::NEG_INFINITY {
            // Midpoint if there's a clean gap; otherwise fall back to
            // tightest positive minus margin so we still catch all stubs.
            if best_self > best_non_self + 2.0 * margin {
                (best_self + best_non_self) / 2.0
            } else {
                (best_self - margin).max(best_non_self + margin / 2.0)
            }
        } else if best_self > f64::NEG_INFINITY {
            // No confusers — just below tightest positive.
            best_self - margin
        } else {
            0.15
        };

        let optimal = optimal.clamp(0.10, 0.90);
        let optimal = (optimal * 100.0).round() / 100.0;

        let current = entry.embed_threshold.unwrap_or(0.25);
        let changed = (optimal - current).abs() > 0.005;

        results.push(TuneResult {
            way_id: way_id.to_string(),
            lang: entry.lang.clone(),
            current,
            optimal,
            best_self,
            best_non_self: if best_non_self > f64::NEG_INFINITY {
                Some(best_non_self)
            } else {
                None
            },
            gap,
            changed,
            confusers: top_confusers,
        });

        let mut tuned = entry.clone();
        if changed {
            tuned.embed_threshold = Some(optimal);
        }
        tuned_entries.push(tuned);
    }

    Ok(WayTuneResult {
        way_id: way_id.to_string(),
        locale_path: locale_path.to_path_buf(),
        results,
        tuned_entries,
        original_entries: entries,
    })
}

fn find_way_embed() -> Option<PathBuf> {
    let xdg = xdg_cache_dir().join("claude-ways/user/way-embed");
    if xdg.is_file() {
        return Some(xdg);
    }
    let bin = home_dir().join(".claude/bin/way-embed");
    if bin.is_file() {
        return Some(bin);
    }
    None
}
