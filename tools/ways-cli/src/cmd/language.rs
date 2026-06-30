//! Language coverage report — shows multilingual state of ways.
//!
//! Reports: resolved output language, derived embedding model per way,
//! locale stubs (.locales.jsonl + override .lang.md files), and model availability.

use anyhow::Result;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::agents;
use crate::frontmatter;
use agent_fmt::Table;
use crate::util::home_dir;

pub fn run(filter_lang: Option<&str>, audit: bool, json_output: bool) -> Result<()> {
    let ways_dir = home_dir().join(".claude/hooks/ways");
    let xdg_way = crate::paths::corpus_dir();
    let excluded = crate::util::load_excluded_segments();

    // Resolved language
    let resolved = agents::resolve_language();

    // Model availability
    let en_model = xdg_way.join("minilm-l6-v2.gguf").is_file();
    let multi_model = xdg_way.join("multilingual-minilm-l12-v2-q8.gguf").is_file();

    // Corpus stats
    let en_corpus = xdg_way.join("ways-corpus-en.jsonl");
    let multi_corpus = xdg_way.join("ways-corpus-multi.jsonl");
    let en_corpus_count = line_count(&en_corpus);
    let multi_corpus_count = line_count(&multi_corpus);

    // Scan all way directories for language stubs and embed_model settings
    let mut ways: Vec<WayLanguageInfo> = Vec::new();
    let mut all_locales: BTreeSet<String> = BTreeSet::new();

    scan_way_dirs(&ways_dir, "", &excluded, &mut ways, &mut all_locales)?;

    // Apply filter
    if let Some(lang) = filter_lang {
        if lang != "en" {
            ways.retain(|w| w.locales.contains(lang) || w.embed_model == "multilingual");
        }
    }

    if json_output {
        let output = json!({
            "resolved_language": resolved,
            "models": {
                "en": { "available": en_model, "corpus_entries": en_corpus_count },
                "multilingual": { "available": multi_model, "corpus_entries": multi_corpus_count },
            },
            "locales_found": all_locales,
            "ways": ways.iter().map(|w| json!({
                "id": w.id,
                "embed_model": w.embed_model,
                "locales": w.locales,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Language Coverage Report");
        println!("=======================");
        println!();
        println!("Output language:  {resolved}");
        println!("EN model:         {}", if en_model { "available" } else { "MISSING" });
        println!("Multilingual:     {}", if multi_model { "available" } else { "MISSING" });
        println!("EN corpus:        {en_corpus_count} ways");
        println!("Multi corpus:     {multi_corpus_count} ways");
        println!();

        // Language coverage summary (active languages only)
        let all_supported = agents::get_active_languages();
        let non_en: Vec<&str> = all_supported.iter()
            .filter(|c| *c != "en")
            .map(|s| s.as_str())
            .collect();

        if !all_locales.is_empty() {
            println!("Covered ({}/{}):     {} + en",
                all_locales.len(), non_en.len(),
                all_locales.iter().cloned().collect::<Vec<_>>().join(", "),
            );
        }

        let uncovered: Vec<String> = non_en.iter()
            .filter(|code| !all_locales.contains(**code))
            .map(|s| s.to_string())
            .collect();

        if !uncovered.is_empty() {
            println!("Uncovered ({}/{}):   {}",
                uncovered.len(), non_en.len(),
                uncovered.join(", "),
            );
        }
        println!();

        // Summary counts
        let en_count = ways.iter().filter(|w| w.embed_model == "en").count();
        let multi_count = ways.iter().filter(|w| w.embed_model == "multilingual").count();
        println!("Ways: {} total ({} en, {} multilingual)", ways.len(), en_count, multi_count);
        println!();

        // Per-way detail (only in audit mode)
        if audit && !ways.is_empty() {
            let mut t = Table::new(&["Way", "Model", "Locales"]);
            t.max_width(0, 45);
            for w in &ways {
                let locales = if w.locales.is_empty() {
                    "en".to_string()
                } else {
                    let mut l: Vec<&str> = w.locales.iter().map(|s| s.as_str()).collect();
                    l.insert(0, "en");
                    l.join(", ")
                };
                t.add(vec![&w.id, &w.embed_model, &locales]);
            }
            t.print();
        } else if !audit {
            println!("Run `ways language --audit` for per-way detail.");
        }

        // Warnings
        if resolved != "en" && !multi_model {
            println!();
            println!("⚠  Output language is {resolved} but multilingual model is not installed.");
            println!("   Non-English prompts will not match. Run: make setup");
        }
    }

    Ok(())
}

struct WayLanguageInfo {
    id: String,
    embed_model: String,
    locales: BTreeSet<String>,
}

fn scan_way_dirs(
    dir: &Path,
    prefix: &str,
    excluded: &[String],
    ways: &mut Vec<WayLanguageInfo>,
    all_locales: &mut BTreeSet<String>,
) -> Result<()> {
    // Collect way directories (each dir with a .md file containing frontmatter)
    let mut way_dirs: BTreeMap<String, PathBuf> = BTreeMap::new();

    for entry in WalkDir::new(dir).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains(".check.")) {
            continue;
        }
        if crate::util::is_excluded_path(path, excluded) {
            continue;
        }

        let parent = match path.parent() {
            Some(p) => p,
            None => continue,
        };
        let rel = parent.strip_prefix(dir).unwrap_or(parent);
        let id = format!("{}{}", prefix, rel.display());

        way_dirs.entry(id).or_insert_with(|| parent.to_path_buf());
    }

    for (id, dir_path) in &way_dirs {
        let mut locales = BTreeSet::new();

        // Read all files in this directory
        if let Ok(entries) = std::fs::read_dir(dir_path) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                let fname = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };

                // Check .locales.jsonl for packed locale stubs
                if fname.ends_with(".locales.jsonl") {
                    if let Ok(entries) = frontmatter::parse_locales_jsonl(&path) {
                        for le in entries {
                            locales.insert(le.lang.clone());
                            all_locales.insert(le.lang);
                        }
                    }
                    continue;
                }

                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }

                // Check for locale override files: {name}.{lang}.md
                if let Some(locale) = crate::util::extract_locale_from_filename(&fname) {
                    locales.insert(locale.clone());
                    all_locales.insert(locale);
                    continue;
                }

                // embed_model is derived: ways with locale stubs → multilingual
                // (set below after scanning all files in the directory)
            }
        }

        // Derive model: ways with locale stubs use multilingual, others use EN
        let derived_model = if locales.is_empty() { "en" } else { "multilingual" };
        ways.push(WayLanguageInfo {
            id: id.clone(),
            embed_model: derived_model.to_string(),
            locales,
        });
    }

    Ok(())
}

fn line_count(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|c| c.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0)
}
