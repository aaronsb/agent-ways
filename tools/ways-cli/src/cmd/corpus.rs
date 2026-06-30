use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::frontmatter;

pub fn run(
    ways_dir: Option<String>,
    output_dir: Option<String>,
    quiet: bool,
    if_stale: bool,
) -> Result<()> {
    let global_dir = ways_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude/hooks/ways"));

    // The engine dir holds the way-embed binary + GGUF models — always canonical.
    let engine_dir = crate::paths::corpus_dir();
    // Corpus artifacts (jsonl, splits, manifest) go to --output if given, else
    // the canonical engine dir.
    let out_dir = match &output_dir {
        Some(o) => crate::util::normalize_path_sep(&PathBuf::from(o)),
        None => engine_dir.clone(),
    };

    // Bug-C guard: an ad-hoc --ways-dir build that lands on the canonical corpus
    // re-embeds and wipes the global + project ways. Steer it to --output.
    if ways_dir.is_some() && output_dir.is_none() && out_dir == engine_dir {
        eprintln!(
            "[ways corpus] WARNING: --ways-dir regenerates the canonical corpus at {},",
            out_dir.display()
        );
        eprintln!("  replacing global + all project ways. Pass --output <dir> for an isolated build.");
    }

    // Staleness check: skip regen if corpus is fresh
    if if_stale {
        let manifest = out_dir.join("embed-manifest.json");
        let corpus = out_dir.join("ways-corpus.jsonl");
        if manifest.is_file() && corpus.is_file() {
            let project_dir = std::env::var("CLAUDE_PROJECT_DIR").unwrap_or_default();
            if !is_stale(&manifest, &global_dir, &project_dir) {
                return Ok(());
            }
        }
        // Missing manifest/corpus → always regen
    }
    std::fs::create_dir_all(&out_dir)?;
    let corpus_path = out_dir.join("ways-corpus.jsonl");

    let tmpfile = corpus_path.with_extension("jsonl.tmp");
    let mut w = BufWriter::new(
        std::fs::File::create(&tmpfile)
            .with_context(|| format!("creating {}", tmpfile.display()))?,
    );

    let log = |msg: &str| {
        if !quiet {
            eprintln!("{msg}");
        }
    };

    let excluded = crate::util::load_excluded_segments();

    // Scan global ways
    let global_count = scan_ways_dir(&global_dir, "", &excluded, &mut w)?;
    let global_hash = content_hash(&global_dir);
    log(&format!(
        "Global ways: {global_count} (hash: {}...)",
        &global_hash[..16.min(global_hash.len())]
    ));

    // Scan project-local ways
    let mut project_total = 0;
    let mut manifest_projects: HashMap<String, serde_json::Value> = HashMap::new();
    let mut seen_ways_dirs: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    // Current project first, straight from CLAUDE_PROJECT_DIR. This is the
    // Windows-safe path: no lossy decode of the ~/.claude/projects/ dir name.
    // The namespace key is derived from the REAL project root via
    // encode_project_key, so it matches exactly what `ways scan --project`
    // computes for the same directory (the fix for Bug B).
    if let Ok(cpd) = std::env::var("CLAUDE_PROJECT_DIR") {
        if !cpd.is_empty() {
            let proj_root = PathBuf::from(&cpd);
            let ways_path = proj_root.join(".claude/ways");
            if ways_path.is_dir() {
                let canon = std::fs::canonicalize(&ways_path).unwrap_or_else(|_| ways_path.clone());
                seen_ways_dirs.insert(canon);
                let key = crate::util::encode_project_key(&proj_root);
                let real = std::fs::canonicalize(&proj_root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or(cpd);
                project_total += embed_one_project(
                    &ways_path,
                    &key,
                    &real,
                    &excluded,
                    &mut w,
                    &mut manifest_projects,
                    &log,
                )?;
            }
        }
    }

    let projects_dir = home_dir().join(".claude/projects");
    if projects_dir.is_dir() {
        for entry in std::fs::read_dir(&projects_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            let encoded = entry.file_name().to_string_lossy().to_string();
            let project_path = match resolve_project_path(&projects_dir, &encoded) {
                Some(p) => p,
                None => continue,
            };

            // Walk up to find .claude/ways/ (project may be invoked from subdirectory)
            let ways_path = match find_ways_dir(&project_path) {
                Some(p) => p,
                None => continue,
            };

            // Dedup: multiple encoded dirs (and the current project above) may
            // resolve to the same .claude/ways/. Compare canonical paths.
            let canon = std::fs::canonicalize(&ways_path).unwrap_or_else(|_| ways_path.clone());
            if !seen_ways_dirs.insert(canon) {
                continue;
            }

            // Key off the resolved REAL path, not the lossy encoded dir name, so
            // it matches `ways scan --project <that project>`.
            let key = crate::util::encode_project_key(Path::new(&project_path));
            project_total += embed_one_project(
                &ways_path,
                &key,
                &project_path,
                &excluded,
                &mut w,
                &mut manifest_projects,
                &log,
            )?;
        }
    }

    w.flush()?;
    drop(w);

    // Atomic move
    std::fs::rename(&tmpfile, &corpus_path)?;

    let total = global_count + project_total;
    log(&format!(
        "Generated {}: {total} ways ({global_count} global, {project_total} project)",
        corpus_path.display()
    ));

    // Auto-embed if way-embed binary and model are available
    auto_embed(&out_dir, &engine_dir, &corpus_path, &log)?;

    // Write manifest
    let manifest = json!({
        "global_hash": global_hash,
        "global_count": global_count,
        "total_count": total,
        "projects": manifest_projects,
    });
    let manifest_path = out_dir.join("embed-manifest.json");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    log(&format!("Manifest written: {}", manifest_path.display()));

    Ok(())
}

/// Embed one project's `.claude/ways/` under namespace `key`.
///
/// Honors the `.ways-embed` marker (skips on `disinclude`), namespaces every
/// way id as `{key}/{bare_id}`, and records the project in the manifest under
/// `key`. Returns the number of ways embedded.
#[allow(clippy::too_many_arguments)]
fn embed_one_project(
    ways_path: &Path,
    key: &str,
    project_path: &str,
    excluded: &[String],
    w: &mut impl Write,
    manifest_projects: &mut HashMap<String, serde_json::Value>,
    log: &dyn Fn(&str),
) -> Result<usize> {
    // Check .ways-embed marker (skip only on explicit disinclude)
    let marker_dir = ways_path.parent().unwrap_or(Path::new(""));
    let marker = marker_dir.join(".ways-embed");
    if marker.is_file() {
        let state = std::fs::read_to_string(&marker)
            .unwrap_or_default()
            .trim()
            .to_string();
        if state == "disinclude" {
            return Ok(0);
        }
    }

    let prefix = format!("{key}/");
    let local_count = scan_ways_dir(ways_path, &prefix, excluded, w)?;

    if local_count > 0 {
        let local_hash = content_hash(ways_path);
        log(&format!(
            "  {project_path}: {local_count} ways (hash: {}...)",
            &local_hash[..16.min(local_hash.len())]
        ));
        manifest_projects.insert(
            key.to_string(),
            json!({
                "path": project_path,
                "ways_hash": local_hash,
                "ways_count": local_count,
            }),
        );
    }

    Ok(local_count)
}

/// Scan a ways directory for semantic ways (having description + vocabulary).
/// Writes JSONL to the writer. Returns the number of ways found.
fn scan_ways_dir(dir: &Path, id_prefix: &str, excluded: &[String], w: &mut impl Write) -> Result<usize> {
    let mut count = 0;

    let mut md_files: Vec<PathBuf> = Vec::new();
    let mut locale_files: Vec<PathBuf> = Vec::new();
    // Track which (directory, lang) pairs have external .lang.md overrides
    let mut locale_overrides: std::collections::HashSet<(PathBuf, String)> = std::collections::HashSet::new();

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Collect .locales.jsonl files
        if fname.ends_with(".locales.jsonl") {
            if !crate::util::is_excluded_path(path, excluded) {
                locale_files.push(path.to_path_buf());
            }
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if fname.contains(".check.") {
            continue;
        }
        if crate::util::is_excluded_path(path, excluded) {
            continue;
        }

        // Detect locale override files ({name}.{lang}.md)
        if let Some(lang) = crate::util::extract_locale_from_filename(fname) {
            if let Some(parent) = path.parent() {
                locale_overrides.insert((parent.to_path_buf(), lang));
            }
        }

        md_files.push(path.to_path_buf());
    }
    md_files.sort();
    locale_files.sort();

    // Pass 1: process .md files (including any external locale override .lang.md files)
    let presets = &crate::config::global().refire_presets;
    // English roots captured here become the multilingual anchor in Pass 2 (localized mode).
    let mut en_roots: HashMap<String, (String, String)> = HashMap::new();
    for path in &md_files {
        let fm = match frontmatter::parse_if_present(path) {
            Ok(Some(fm)) => fm,
            // No frontmatter at all — a template/catalog/prose file, not a way. Skip
            // it silently, the way it always has been.
            Ok(None) => continue,
            // Frontmatter present but unparseable (e.g. an unquoted value containing
            // ": ") would vanish from matching with no signal. `ways lint` is the hard
            // gate (it now runs this same parse), but warn here too so a runtime
            // rebuild still surfaces it. See ADR-125.
            Err(e) => {
                let rel = path.strip_prefix(dir).unwrap_or(path);
                eprintln!("[ways corpus] WARN: {} — frontmatter present but did not parse, dropped from corpus ({})", rel.display(), e.root_cause());
                continue;
            }
        };

        // ADR-126: surface malformed refire specs at corpus time. Corpus is a
        // frequently-invoked gate (CI, local rebuilds), so typos caught here
        // don't have to wait for a session to misfire. Warnings are
        // stderr-only — `ways lint` is the hard gate and escalates.
        if let Some(spec) = &fm.refire {
            if let Err(msg) = spec.validate(presets) {
                let rel = path.strip_prefix(dir).unwrap_or(path);
                eprintln!("[ways corpus] WARN: {} — {msg}", rel.display());
            }
        }

        // Skip ways without semantic fields (corpus is for matching engines)
        if fm.description.is_empty() || fm.vocabulary.is_none() {
            continue;
        }

        let relpath = path.strip_prefix(dir).unwrap_or(path);
        let id_body = crate::util::path_to_id(relpath.parent().unwrap_or(Path::new("")));
        let id = format!("{id_prefix}{id_body}");

        // Capture the English root for the multilingual anchor (Pass 2, localized mode).
        en_roots.insert(
            id.clone(),
            (fm.description.clone(), fm.vocabulary.clone().unwrap_or_default()),
        );

        // .md ways always use EN model (locale stubs use multilingual)
        let entry = json!({
            "id": id,
            "description": fm.description,
            "vocabulary": fm.vocabulary.unwrap_or_default(),
            "embed_threshold": fm.embed_threshold.unwrap_or(0.35),
            "embed_model": "en",
        });

        serde_json::to_writer(&mut *w, &entry)?;
        w.write_all(b"\n")?;
        count += 1;
    }

    // Pass 2: locale aliases + the English-root anchor — localized mode only (ADR-139).
    // The English frontmatter, embedded with the multilingual model, is the anchor every
    // localized alias is matched and tuned against: the source of truth in multilingual
    // space. English mode builds no multilingual entries at all.
    if crate::config::global().localized_language().is_some() {
        let mut anchored: std::collections::HashSet<String> = std::collections::HashSet::new();
        for path in &locale_files {
            let parent = path.parent().unwrap_or(Path::new(""));
            let relparent = parent.strip_prefix(dir).unwrap_or(parent);
            let id = format!("{}{}", id_prefix, crate::util::path_to_id(relparent));

            let entries = match frontmatter::parse_locales_jsonl(path) {
                Ok(e) => e,
                Err(_) => continue,
            };

            // English-root anchor: once per way, emit its English text as a multilingual
            // entry (lang "en") so localized aliases score against the source of truth.
            if anchored.insert(id.clone()) {
                if let Some((desc, vocab)) = en_roots.get(&id) {
                    let anchor = json!({
                        "id": id,
                        "description": desc.as_str(),
                        "vocabulary": vocab.as_str(),
                        "embed_model": "multilingual",
                        "lang": "en",
                    });
                    serde_json::to_writer(&mut *w, &anchor)?;
                    w.write_all(b"\n")?;
                    count += 1;
                }
            }

            for le in entries {
                // Skip inactive languages
                if !crate::agents::is_language_active(&le.lang) {
                    continue;
                }
                // Skip if an external .lang.md override exists
                if locale_overrides.contains(&(parent.to_path_buf(), le.lang.clone())) {
                    continue;
                }

                let entry = json!({
                    "id": id,
                    "description": le.description,
                    "vocabulary": le.vocabulary.unwrap_or_default(),
                    "embed_model": "multilingual",
                    "lang": le.lang,
                });

                serde_json::to_writer(&mut *w, &entry)?;
                w.write_all(b"\n")?;
                count += 1;
            }
        }
    }

    Ok(count)
}

/// Shell out to way-embed generate for embedding vectors.
/// Generates two corpus files: one with EN model embeddings, one with multilingual.
///
/// `out_dir` receives the split corpora; `engine_dir` (always the canonical XDG
/// cache) supplies the way-embed binary and GGUF models. The two differ only
/// when `ways corpus --output <dir>` redirects an isolated build.
fn auto_embed(out_dir: &Path, engine_dir: &Path, corpus: &Path, log: &dyn Fn(&str)) -> Result<()> {
    let embed_bin = [
        engine_dir.join("way-embed"),
        home_dir().join(".claude/bin/way-embed"),
    ]
    .into_iter()
    .find(|p| p.is_file());

    let bin = match embed_bin {
        Some(b) => b,
        None => {
            log("ERROR: embedding engine required (ADR-125). Run: cd ~/.claude && make setup");
            return Ok(());
        }
    };

    let en_model = engine_dir.join("minilm-l6-v2.gguf");
    let multi_model = engine_dir.join("multilingual-minilm-l12-v2-q8.gguf");

    // Split corpus into EN and multilingual entries
    let corpus_content = std::fs::read_to_string(corpus)?;
    let corpus_en = out_dir.join("ways-corpus-en.jsonl");
    let corpus_multi = out_dir.join("ways-corpus-multi.jsonl");
    let mut en_count = 0usize;
    let mut multi_count = 0usize;

    {
        let mut w_en = std::io::BufWriter::new(std::fs::File::create(&corpus_en)?);
        let mut w_multi = std::io::BufWriter::new(std::fs::File::create(&corpus_multi)?);

        for line in corpus_content.lines() {
            if line.is_empty() { continue; }
            let model_field = serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|v| v.get("embed_model").and_then(|m| m.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| "en".to_string());

            if model_field == "multilingual" {
                writeln!(w_multi, "{line}")?;
                multi_count += 1;
            } else {
                writeln!(w_en, "{line}")?;
                en_count += 1;
            }
        }
    }

    // On Windows, Stdio::null() for the NUL device can cause MSVC C runtime
    // to abort the child process. Use Stdio::inherit() on Windows instead.
    #[cfg(windows)]
    let embed_stderr = || std::process::Stdio::inherit();
    #[cfg(not(windows))]
    let embed_stderr = || std::process::Stdio::null();

    // Embed EN corpus
    if en_model.is_file() && en_count > 0 {
        log(&format!("Embedding {en_count} ways with English model..."));
        let status = std::process::Command::new(&bin)
            .args(["generate", "--corpus"])
            .arg(&corpus_en)
            .args(["--model"])
            .arg(&en_model)
            .stderr(embed_stderr())
            .status();

        match status {
            Ok(s) if s.success() => log(&format!("  EN embeddings: {}", corpus_en.display())),
            _ => eprintln!("WARNING: EN embedding generation failed"),
        }
    }

    // Embed multilingual corpus
    if multi_model.is_file() && multi_count > 0 {
        log(&format!("Embedding {multi_count} ways with multilingual model..."));
        let status = std::process::Command::new(&bin)
            .args(["generate", "--corpus"])
            .arg(&corpus_multi)
            .args(["--model"])
            .arg(&multi_model)
            .stderr(embed_stderr())
            .status();

        match status {
            Ok(s) if s.success() => log(&format!("  Multi embeddings: {}", corpus_multi.display())),
            _ => eprintln!("WARNING: multilingual embedding generation failed"),
        }
    } else if multi_count > 0 && !multi_model.is_file() {
        log(&format!("  {multi_count} multilingual ways found but model not installed"));
        log("  Run: make -C tools/way-embed model-multilingual  (127MB, on-demand per ADR-139)");
    }

    // Also generate combined corpus for backward compatibility
    // (the main ways-corpus.jsonl keeps EN embeddings as before)
    if en_model.is_file() {
        log("Generating combined corpus with English embeddings...");
        let status = std::process::Command::new(&bin)
            .args(["generate", "--corpus"])
            .arg(corpus)
            .args(["--model"])
            .arg(&en_model)
            .stderr(embed_stderr())
            .status();

        match status {
            Ok(s) if s.success() => log(&format!("Combined corpus: {}", corpus.display())),
            _ => eprintln!("WARNING: combined embedding generation failed"),
        }
    }

    Ok(())
}

/// Resolve real project path from Claude Code's encoded directory name.
/// The encoding (/ → -) is lossy, so we try sessions-index.json first,
/// then fall back to greedy filesystem resolution.
fn resolve_project_path(projects_dir: &Path, encoded: &str) -> Option<String> {
    // Try sessions-index.json first
    let idx = projects_dir.join(encoded).join("sessions-index.json");
    if idx.is_file() {
        if let Ok(content) = std::fs::read_to_string(&idx) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(path) = parsed["entries"][0]["projectPath"].as_str() {
                    if !path.is_empty() {
                        return Some(path.to_string());
                    }
                }
            }
        }
    }

    // Fallback: greedy filesystem resolution
    resolve_encoded_path(encoded)
}

/// Greedily resolve an encoded path against the filesystem.
/// Splits on -, accumulates segments, tests filesystem at each step
/// to distinguish / from - in the original path.
/// e.g., "-home-aaron-Projects-app-github-manager" → "/home/aaron/Projects/app/github-manager"
fn resolve_encoded_path(encoded: &str) -> Option<String> {
    let stripped = encoded.strip_prefix('-').unwrap_or(encoded);
    let segments: Vec<&str> = stripped.split('-').collect();

    let mut current = String::new();
    let mut pending = String::new();

    for seg in &segments {
        if pending.is_empty() {
            let try_path = format!("{current}/{seg}");
            if Path::new(&try_path).is_dir() {
                current = try_path;
            } else {
                pending = seg.to_string();
            }
        } else {
            // Try hyphenated: current/pending-seg
            let try_hyphen = format!("{current}/{pending}-{seg}");
            // Try split: current/pending/seg
            let try_split = format!("{current}/{pending}/{seg}");

            if Path::new(&try_hyphen).is_dir() {
                current = try_hyphen;
                pending.clear();
            } else if Path::new(&try_split).is_dir() {
                current = try_split;
                pending.clear();
            } else {
                pending = format!("{pending}-{seg}");
            }
        }
    }

    // Flush pending
    if !pending.is_empty() {
        let try_path = format!("{current}/{pending}");
        if Path::new(&try_path).is_dir() {
            current = try_path;
        } else {
            return None;
        }
    }

    if Path::new(&current).is_dir() {
        Some(current)
    } else {
        None
    }
}

/// Walk up from a project path to find .claude/ways/ directory.
fn find_ways_dir(project_path: &str) -> Option<PathBuf> {
    let home = home_dir();
    let mut check = PathBuf::from(project_path);
    while check != Path::new("/") && check != home {
        let candidate = check.join(".claude/ways");
        if candidate.is_dir() {
            return Some(candidate);
        }
        check = check.parent()?.to_path_buf();
    }
    None
}

/// Content hash of a directory (sorted file list + sizes).
fn content_hash(dir: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    let mut entries: Vec<(String, u64)> = Vec::new();

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path().is_file() {
            let rel = entry.path().strip_prefix(dir).unwrap_or(entry.path());
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            entries.push((rel.display().to_string(), size));
        }
    }
    entries.sort();
    entries.hash(&mut hasher);

    format!("{:016x}", hasher.finish())
}

use crate::util::home_dir;

/// Check if any way file is newer than the manifest.
fn is_stale(manifest: &Path, global_dir: &Path, project_dir: &str) -> bool {
    // Check global ways
    for entry in WalkDir::new(global_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str());
            if (ext == Some("md") || ext == Some("jsonl")) && is_newer_than(path, manifest) {
                return true;
            }
        }
    }

    // Check project ways
    if !project_dir.is_empty() {
        let project_ways = Path::new(project_dir).join(".claude/ways");
        if project_ways.is_dir() {
            for entry in WalkDir::new(&project_ways)
                .follow_links(true)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() {
                    let ext = path.extension().and_then(|e| e.to_str());
                    if (ext == Some("md") || ext == Some("jsonl")) && is_newer_than(path, manifest) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

fn is_newer_than(file: &Path, reference: &Path) -> bool {
    let file_mtime = file.metadata().and_then(|m| m.modified()).ok();
    let ref_mtime = reference.metadata().and_then(|m| m.modified()).ok();
    match (file_mtime, ref_mtime) {
        (Some(f), Some(r)) => f > r,
        _ => false,
    }
}
