//! Engine health dashboard — binary, model, corpus, project status.
//! Replaces embed-status.sh (301 lines).

use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn run(json_output: bool) -> Result<()> {
    let xdg_cache = xdg_cache_dir().join("claude-ways/user");
    let ways_dir = home_dir().join(".claude/hooks/ways");
    // Engine detection
    let way_embed = find_way_embed(&xdg_cache);
    let model_path = xdg_cache.join("minilm-l6-v2.gguf");
    let corpus_path = xdg_cache.join("ways-corpus.jsonl");
    let manifest_path = xdg_cache.join("embed-manifest.json");

    let model_exists = model_path.is_file();
    let corpus_exists = corpus_path.is_file();

    // Post-ADR-125: embedding is the sole engine. "none" means model or corpus missing.
    let engine = if way_embed.is_some() && model_exists && corpus_exists {
        "embedding"
    } else {
        "none"
    };

    // Global way counts
    let (global_total, global_semantic) = count_ways(&ways_dir);

    // Corpus stats
    let corpus_count = if corpus_exists {
        std::fs::read_to_string(&corpus_path)
            .map(|c| c.lines().filter(|l| !l.is_empty()).count())
            .unwrap_or(0)
    } else {
        0
    };

    // Manifest data
    let manifest: Option<serde_json::Value> = if manifest_path.is_file() {
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
    } else {
        None
    };

    let manifest_global_hash = manifest
        .as_ref()
        .and_then(|m| m["global_hash"].as_str())
        .unwrap_or("")
        .to_string();

    // Project data from manifest
    let projects: Vec<serde_json::Value> = manifest
        .as_ref()
        .and_then(|m| m["projects"].as_object())
        .map(|obj| {
            obj.iter()
                .map(|(encoded, data)| {
                    json!({
                        "encoded": encoded,
                        "path": data["path"],
                        "ways_count": data["ways_count"],
                        "ways_hash": data["ways_hash"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Output language
    let output_language = crate::agents::resolve_language();

    // config::global() — future migration: ctx.config.disabled_domains
    let disabled = crate::config::global().disabled_domains.clone();
    // ADR-131: project-scope per-way toggles
    let disabled_ways: Vec<String> = crate::config::global().disabled_ways().to_vec();

    if json_output {
        let output = json!({
            "engine": {
                "active": engine,
            },
            "binaries": {
                "ways": std::env::current_exe().ok().map(|p| p.display().to_string()),
                "way_embed": way_embed.as_ref().map(|p| p.display().to_string()),
            },
            "model": {
                "path": model_path.display().to_string(),
                "exists": model_exists,
            },
            "corpus": {
                "path": corpus_path.display().to_string(),
                "exists": corpus_exists,
                "entries": corpus_count,
            },
            "manifest": {
                "exists": manifest.is_some(),
                "global_hash": manifest_global_hash,
            },
            "ways": {
                "global_total": global_total,
                "global_semantic": global_semantic,
            },
            "projects": projects,
            "output_language": output_language,
            "disabled_domains": disabled,
            "disabled_ways": disabled_ways,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Ways Engine Status");
        println!("==================");
        println!();

        // Engine & language
        println!("Engine:    {engine}");
        println!("Language:  {output_language}");
        println!();

        // Binaries
        println!("Binaries:");
        if let Ok(exe) = std::env::current_exe() {
            println!("  ways:      {}", exe.display());
        }
        if let Some(ref embed) = way_embed {
            println!("  way-embed: {}", embed.display());
        } else {
            println!("  way-embed: not found");
        }
        println!();

        // Model
        let model_status = if model_exists { "OK" } else { "MISSING" };
        println!("Model:     {} ({})", model_path.display(), model_status);

        // Corpus
        if corpus_exists {
            println!("Corpus:    {} ({} entries)", corpus_path.display(), corpus_count);
        } else {
            println!("Corpus:    MISSING — run `ways corpus` to generate");
        }

        // Dual corpus status
        let en_corpus = xdg_cache.join("ways-corpus-en.jsonl");
        let multi_corpus = xdg_cache.join("ways-corpus-multi.jsonl");
        let multi_model_path = xdg_cache.join("multilingual-minilm-l12-v2-q8.gguf");
        let en_count = if en_corpus.is_file() { count_lines(&en_corpus) } else { 0 };
        let multi_count = if multi_corpus.is_file() { count_lines(&multi_corpus) } else { 0 };
        if en_count > 0 || multi_count > 0 {
            println!("  EN corpus:    {} ways", en_count);
            println!("  Multi corpus: {} ways", multi_count);
            if multi_count > 0 && !multi_model_path.is_file() {
                println!("  ⚠ {} multilingual ways but model missing — run: make setup", multi_count);
            }
        }
        println!();

        // Ways
        println!("Global ways: {} total, {} semantic", global_total, global_semantic);

        if !disabled.is_empty() {
            println!("Disabled domains: {}", disabled.join(", "));
        }
        if !disabled_ways.is_empty() {
            println!("Disabled ways:    {} (project scope, ADR-131)", disabled_ways.join(", "));
        }
        println!();

        // Projects
        if !projects.is_empty() {
            println!("Projects:");
            for proj in &projects {
                let path = proj["path"].as_str().unwrap_or("?");
                let count = proj["ways_count"].as_u64().unwrap_or(0);
                // Shorten home prefix
                let display = path.replace(&home_dir().display().to_string(), "~");
                println!("  {display}: {count} ways");
            }
        } else {
            println!("Projects: none in manifest");
        }
    }

    Ok(())
}

fn find_way_embed(xdg_cache: &Path) -> Option<PathBuf> {
    let cache = xdg_cache.join("way-embed");
    if cache.is_file() {
        return Some(cache);
    }
    let bin = home_dir().join(".claude/bin/way-embed");
    if bin.is_file() {
        return Some(bin);
    }
    None
}

fn count_ways(dir: &Path) -> (usize, usize) {
    let mut total = 0;
    let mut semantic = 0;

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.contains(".check.") {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !content.starts_with("---\n") {
            continue;
        }

        total += 1;

        // Check for description + vocabulary (semantic way)
        let has_desc = content.lines().any(|l| l.starts_with("description:"));
        let has_vocab = content.lines().any(|l| l.starts_with("vocabulary:"));
        if has_desc && has_vocab {
            semantic += 1;
        }
    }

    (total, semantic)
}

fn count_lines(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|c| c.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0)
}

use crate::util::{home_dir, xdg_cache_dir};
