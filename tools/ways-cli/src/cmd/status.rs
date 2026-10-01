//! Engine health dashboard — binary, model, corpus, project status.
//! Replaces embed-status.sh (301 lines).

use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn run(json_output: bool) -> Result<()> {
    let xdg_cache = crate::paths::corpus_dir();
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

    // Way counts: core (shipped) + user (operator's own, $XDG_CONFIG) — ADR-143.
    let (global_total, global_semantic) = count_ways(&ways_dir);
    let user_ways_dir = crate::paths::user_ways_root();
    let (user_total, user_semantic) = count_ways(&user_ways_dir);

    // Corpus stats. Entries without an embedding match on keywords only, so
    // report both counts: a corpus whose embedding pass never succeeded looks
    // complete by entry count alone (#645).
    let (corpus_count, corpus_embedded) = if corpus_exists {
        std::fs::read_to_string(&corpus_path)
            .map(|c| {
                let lines: Vec<&str> = c.lines().filter(|l| !l.is_empty()).collect();
                let embedded = lines.iter().filter(|l| l.contains("\"embedding\"")).count();
                (lines.len(), embedded)
            })
            .unwrap_or((0, 0))
    } else {
        (0, 0)
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

    // ADR-156 calibration: the semantic lane fires only when a calibration is
    // present. Surface it so a corpus that predates calibration (semantic lane
    // silently keyword-only) is visible rather than a mystery.
    let calibration = manifest.as_ref().and_then(|m| m.get("calibration"));
    let cal_en_auc = calibration.and_then(|c| c["en"]["auc"].as_f64());
    let cal_multi_auc = calibration.and_then(|c| c["multi"]["auc"].as_f64());

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
            "install": install_json(),
            "engine": {
                "active": engine,
            },
            "gate": gate_json(),
            "binaries": {
                "ways": std::env::current_exe().ok().map(|p| p.display().to_string()),
                "way_embed": way_embed.as_ref().map(|p| p.display().to_string()),
                "ways_agent": crate::cmd::agent::resolve().map(|p| p.display().to_string()),
            },
            "model": {
                "path": model_path.display().to_string(),
                "exists": model_exists,
            },
            "corpus": {
                "path": corpus_path.display().to_string(),
                "exists": corpus_exists,
                "entries": corpus_count,
                "embedded": corpus_embedded,
            },
            "calibration": {
                "present": cal_en_auc.is_some(),
                "en_auc": cal_en_auc,
                "multi_auc": cal_multi_auc,
            },
            "manifest": {
                "exists": manifest.is_some(),
                "global_hash": manifest_global_hash,
            },
            "ways": {
                "global_total": global_total,
                "global_semantic": global_semantic,
                "user_total": user_total,
                "user_semantic": user_semantic,
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
        println!("Install:   {}", install_line());
        println!();

        // Engine & language
        println!("Engine:    {engine}");
        println!("Gate:      {}", gate_line());
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
        println!("  ways-mcp:  {}", mcp_binary_line());
        match crate::cmd::agent::resolve() {
            Some(p) => println!("  ways-agent: {}", p.display()),
            None => println!("  ways-agent: not found — the relevance gate is off until `ways update` installs it"),
        }
        for t in install_targets().0.iter().filter(|t| t.enabled) {
            println!("  mcp:       {} in {}", mcp_registration(&t.dir()), t.path);
        }
        println!();

        // Model
        let model_status = if model_exists { "OK" } else { "MISSING" };
        println!("Model:     {} ({})", model_path.display(), model_status);

        // Corpus
        if corpus_exists {
            let note = if corpus_embedded < corpus_count {
                " — entries without embeddings match on keywords only; run `ways corpus`"
            } else {
                ""
            };
            println!(
                "Corpus:    {} ({} entries, {} embedded){}",
                corpus_path.display(),
                corpus_count,
                corpus_embedded,
                note
            );
            // The last build did not fully embed: the corpus above may be
            // a kept earlier one, so its counts can look healthy (#645).
            let last_embedded = manifest
                .as_ref()
                .and_then(|m| m["embedded"].as_bool())
                .unwrap_or(true);
            if !last_embedded {
                let reason = manifest
                    .as_ref()
                    .and_then(|m| m["reason"].as_str())
                    .unwrap_or("reason not recorded");
                println!("           last build did not fully embed: {reason}; run `ways corpus`");
            }
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

        // Calibration (ADR-156): without it the semantic lane cannot fire.
        if corpus_exists {
            match cal_en_auc {
                Some(auc) => {
                    let multi = cal_multi_auc
                        .map(|a| format!(", multi AUC {a:.3}"))
                        .unwrap_or_default();
                    println!("Calibration: EN AUC {auc:.3}{multi}");
                }
                None => println!(
                    "Calibration: MISSING — semantic lane disabled (keyword-only); run `ways corpus`"
                ),
            }
        }
        println!();

        // Ways
        println!("Core ways: {} total, {} semantic", global_total, global_semantic);
        if user_total > 0 {
            println!("User ways: {} total, {} semantic", user_total, user_semantic);
        }

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
        if !crate::util::has_frontmatter(&content) {
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

use crate::util::home_dir;

/// The install state (ADR-184 item 1): installed and inactive, or active with
/// the targets and their converged state.
fn install_targets() -> (Vec<crate::config::Target>, bool) {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));
    let cfg = crate::config::Config::load(&project_dir);
    (cfg.targets(), cfg.targets_explicit())
}

fn install_line() -> String {
    let (targets, explicit) = install_targets();
    let enabled: Vec<&crate::config::Target> = targets.iter().filter(|t| t.enabled).collect();
    if enabled.is_empty() {
        return "installed, inactive (no enabled target; `ways config target add <dir>` activates one)".to_string();
    }
    let names: Vec<String> = enabled
        .iter()
        .map(|t| format!("{} [{}]", t.path, crate::cmd::config_cmd::target_state(t)))
        .collect();
    format!(
        "active, {} of {} target{} enabled: {}{}",
        enabled.len(),
        targets.len(),
        if targets.len() == 1 { "" } else { "s" },
        names.join(", "),
        if explicit { "" } else { " (implicit)" }
    )
}

fn install_json() -> serde_json::Value {
    let (targets, explicit) = install_targets();
    let any_enabled = targets.iter().any(|t| t.enabled);
    json!({
        "state": if any_enabled { "active" } else { "installed" },
        "explicit": explicit,
        "targets": targets.iter().map(|t| json!({
            "path": t.path,
            "enabled": t.enabled,
            "observe": t.observes(),
            "state": crate::cmd::config_cmd::target_state(t),
            "mcp_command": crate::cmd::mcp_register::status(&t.dir(), &crate::paths::projection_root()),
        })).collect::<Vec<_>>(),
    })
}

/// The installed ways-mcp and its version: what new sessions start. A running
/// session reports its own server's version through `ways_status`.
fn mcp_binary_line() -> String {
    let bin = crate::paths::data_root().join("bin").join("ways-mcp");
    match std::process::Command::new(&bin).arg("--version").output() {
        Ok(out) if out.status.success() => {
            format!("{} ({})", bin.display(), String::from_utf8_lossy(&out.stdout).trim())
        }
        _ => "not installed (`make ways-mcp` in the app directory, or `ways update`)".to_string(),
    }
}

/// Whether a target has the agent-ways MCP server registered.
fn mcp_registration(dir: &std::path::Path) -> String {
    match crate::cmd::mcp_register::status(dir, &crate::paths::projection_root()) {
        Some(cmd) => format!("{} → {cmd}", crate::cmd::mcp_register::SERVER),
        None => format!("{} not registered (`ways reconcile` registers it)", crate::cmd::mcp_register::SERVER),
    }
}

/// The relevance gate's settings (ADR-196), read without touching the key:
/// `ways` only checks where a key would come from.
fn gate_settings() -> Result<Option<(ways_agent::profile::Settings, Option<ways_agent::keys::Source>)>> {
    use ways_agent::{keys, profile};
    let user = profile::UserLayer::load(&profile::user_layer_path())?;
    let settings = profile::resolve(&user, |p| keys::locate(p).is_some())?;
    Ok(settings.map(|s| {
        let source = keys::locate(s.profile.provider);
        (s, source)
    }))
}

fn gate_line() -> String {
    match gate_settings() {
        Err(e) => format!("config error: {e:#}"),
        Ok(None) => "off — no key (`ways agent key add --provider anthropic`)".to_string(),
        Ok(Some((s, source))) => {
            let key = match source {
                Some(src) => format!("key from {src}"),
                None => "no key: fails open".to_string(),
            };
            format!(
                "{} — {} {} at threshold {}, {key}",
                s.mode.as_str(),
                s.profile.provider,
                s.profile.model,
                s.profile.threshold
            )
        }
    }
}

fn gate_json() -> serde_json::Value {
    match gate_settings() {
        Err(e) => json!({ "error": format!("{e:#}") }),
        Ok(None) => json!({ "mode": "off", "reason": "no key" }),
        Ok(Some((s, source))) => json!({
            "mode": s.mode.as_str(),
            "engine": s.engine,
            "provider": s.profile.provider.as_str(),
            "model": s.profile.model,
            "threshold": s.profile.threshold,
            "key_source": source.map(|src| src.to_string()),
        }),
    }
}
