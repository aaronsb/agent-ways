use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

use crate::frontmatter;

pub fn run(
    ways_dir: Option<String>,
    output_dir: Option<String>,
    quiet: bool,
    verbose: bool,
    if_stale: bool,
) -> Result<()> {
    // --verbose outranks --quiet: the reason to reach for it is that a quiet
    // build appeared to hang, and a diagnostic you have to un-silence twice is
    // not a diagnostic.
    let quiet = quiet && !verbose;

    // Every trace line is stamped with elapsed-since-start and emitted *before*
    // the step it announces. eprintln! is unbuffered, so when the process wedges
    // the last line on screen names the step that wedged it.
    let started = Instant::now();
    let vlog = |msg: &str| {
        if verbose {
            eprintln!("[ways corpus +{:6.2}s] {msg}", started.elapsed().as_secs_f64());
        }
    };

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

    vlog(&format!("core ways dir:   {}", global_dir.display()));
    vlog(&format!("engine dir:      {}", engine_dir.display()));
    vlog(&format!("output dir:      {}", out_dir.display()));

    // Staleness check: skip regen if corpus is fresh
    if if_stale {
        let manifest = out_dir.join("embed-manifest.json");
        let corpus = out_dir.join("ways-corpus.jsonl");
        if manifest.is_file() && corpus.is_file() {
            let project_dir = std::env::var("CLAUDE_PROJECT_DIR").unwrap_or_default();
            vlog("staleness check (walks core + user + project ways)");
            if !is_stale(&manifest, &global_dir, &project_dir) {
                vlog("corpus is fresh — nothing to do");
                return Ok(());
            }
            vlog("corpus is stale — rebuilding");
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
    let empty_skip: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Scan USER ways first (ADR-143): the operator's own root in $XDG_CONFIG.
    // Its ids become the skip set for core, so a user way shadows a same-named
    // shipped way (precedence project > user > core).
    let user_dir = crate::paths::user_ways_root();
    vlog(&format!("user ways dir:   {}", user_dir.display()));
    let mut user_sink: std::collections::HashSet<String> = std::collections::HashSet::new();
    let user_count = if user_dir.is_dir() {
        vlog("scanning user ways");
        let c = scan_ways_dir(&user_dir, "", &excluded, &mut w, &empty_skip, &mut user_sink)?;
        if c > 0 {
            log(&format!("User ways: {c} ({})", user_dir.display()));
        }
        c
    } else {
        vlog("no user ways dir — skipping");
        0
    };
    vlog("hashing user ways");
    let user_hash = content_hash(&user_dir);

    // Scan CORE (shipped) ways, dropping any id a user way claimed. The shadow
    // set is ALL user way ids by directory (crate::cmd::scan::candidates::way_ids)
    // — incl. non-semantic ones — so it matches the predictive scanner's dedup
    // and a pattern-only user override still suppresses the core way.
    vlog("collecting user way ids (shadow set)");
    let user_shadow = crate::cmd::scan::candidates::way_ids(&user_dir);
    let mut core_sink: std::collections::HashSet<String> = std::collections::HashSet::new();
    vlog("scanning core ways");
    let global_count = scan_ways_dir(&global_dir, "", &excluded, &mut w, &user_shadow, &mut core_sink)?;
    vlog("hashing core ways");
    let global_hash = content_hash(&global_dir);
    log(&format!(
        "Core ways: {global_count} (hash: {}...)",
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
            vlog(&format!("current project (CLAUDE_PROJECT_DIR): {cpd}"));
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
        vlog(&format!("enumerating projects: {}", projects_dir.display()));
        for entry in std::fs::read_dir(&projects_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            let encoded = entry.file_name().to_string_lossy().to_string();
            // Announce before resolving: resolve_project_path falls back to
            // probing is_dir() across every candidate split of the encoded name,
            // so an unreachable mount stalls here, under this project's name.
            vlog(&format!("  resolving {encoded}"));
            let project_path = match resolve_project_path(&projects_dir, &encoded) {
                Some(p) => p,
                None => {
                    vlog("    unresolved — skipped");
                    continue;
                }
            };

            // Walk up to find .claude/ways/ (project may be invoked from subdirectory)
            let ways_path = match find_ways_dir(&project_path) {
                Some(p) => p,
                None => continue,
            };
            vlog(&format!("    ways: {}", ways_path.display()));

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
    vlog(&format!("writing corpus: {}", corpus_path.display()));
    std::fs::rename(&tmpfile, &corpus_path)?;

    let total = global_count + user_count + project_total;
    log(&format!(
        "Generated {}: {total} ways ({global_count} core, {user_count} user, {project_total} project)",
        corpus_path.display()
    ));

    // Auto-embed if way-embed binary and model are available
    auto_embed(&out_dir, &engine_dir, &corpus_path, verbose, &vlog, &log)?;

    // Fit per-model calibration g(s)=σ(a·s+b) from the probe corpus (ADR-156).
    let calibration = fit_calibration(&out_dir, &engine_dir, verbose, &vlog, &log);

    // Write manifest
    let manifest = json!({
        "global_hash": global_hash,
        "global_count": global_count,
        "user_hash": user_hash,
        "user_count": user_count,
        "total_count": total,
        "projects": manifest_projects,
        "calibration": calibration,
    });
    let manifest_path = out_dir.join("embed-manifest.json");
    vlog("writing manifest");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    log(&format!("Manifest written: {}", manifest_path.display()));
    vlog("done");

    Ok(())
}

/// Child stderr policy for a `way-embed` subprocess.
///
/// Quiet builds discard it. Under `--verbose` it is inherited, which is the
/// whole point of the flag: `way-embed generate` already prints `[n/total] <id>`
/// per way, and swallowing that is what turns a slow pass into an apparent hang.
fn embed_stderr(verbose: bool) -> std::process::Stdio {
    if verbose {
        return std::process::Stdio::inherit();
    }
    // On Windows, Stdio::null() for the NUL device can cause MSVC C runtime
    // to abort the child process. Use Stdio::inherit() on Windows instead.
    #[cfg(windows)]
    {
        std::process::Stdio::inherit()
    }
    #[cfg(not(windows))]
    {
        std::process::Stdio::null()
    }
}

/// Elapsed suffix, rendered only under `--verbose` so a quiet build's output is
/// unchanged.
fn elapsed_suffix(t: Instant, verbose: bool) -> String {
    if verbose {
        format!(" ({:.2}s)", t.elapsed().as_secs_f64())
    } else {
        String::new()
    }
}

/// Run one `way-embed generate` pass, returning the elapsed time on success.
///
/// `what` names the pass in diagnostics. Under `--verbose` the exact argv is
/// echoed, so a stalled pass can be rerun standalone outside the corpus build.
fn run_generate(
    bin: &Path,
    corpus: &Path,
    model: &Path,
    what: &str,
    verbose: bool,
    vlog: &dyn Fn(&str),
) -> Option<Instant> {
    vlog(&format!(
        "exec: {} generate --corpus {} --model {}",
        bin.display(),
        corpus.display(),
        model.display()
    ));
    let t = Instant::now();
    let status = std::process::Command::new(bin)
        .args(["generate", "--corpus"])
        .arg(corpus)
        .args(["--model"])
        .arg(model)
        .stderr(embed_stderr(verbose))
        .status();

    match status {
        Ok(s) if s.success() => Some(t),
        Ok(s) => {
            eprintln!("WARNING: {what} embedding generation failed ({s})");
            None
        }
        Err(e) => {
            eprintln!("WARNING: {what} embedding generation could not start: {e}");
            None
        }
    }
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
    // Project ids are namespaced ({key}/…), so they can't collide with core/user;
    // pass fresh dedup sets.
    let skip = std::collections::HashSet::new();
    let mut written = std::collections::HashSet::new();
    let local_count = scan_ways_dir(ways_path, &prefix, excluded, w, &skip, &mut written)?;

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
///
/// `skip` holds ids already claimed by a higher-precedence root — a matching way
/// here is shadowed and dropped (ADR-143 dedup-by-name). Every id actually
/// written is recorded in `written` so the caller can build the next root's skip
/// set (precedence: project > user > core).
fn scan_ways_dir(
    dir: &Path,
    id_prefix: &str,
    excluded: &[String],
    w: &mut impl Write,
    skip: &std::collections::HashSet<String>,
    written: &mut std::collections::HashSet<String>,
) -> Result<usize> {
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

        // Dedup-by-name (ADR-143): a higher-precedence root already claimed this
        // id, so this shadowed way is dropped from the corpus. Otherwise record it
        // so lower-precedence roots skip it.
        if skip.contains(&id) {
            continue;
        }
        written.insert(id.clone());

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
            // Same dedup as Pass 1: don't emit locale aliases for a shadowed id.
            if skip.contains(&id) {
                continue;
            }

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

/// Resolve the `way-embed` binary: prefer the engine dir, then the projected
/// `~/.claude/bin`. `auto_embed` and `fit_calibration` both need it and must
/// resolve it identically — on a projection install the binary lives in
/// `~/.claude/bin`, not the engine/cache dir, so a resolver that only checks the
/// engine dir silently no-ops (this is how per-model calibration went missing).
fn resolve_embed_bin(engine_dir: &Path) -> Option<PathBuf> {
    [
        engine_dir.join("way-embed"),
        home_dir().join(".claude/bin/way-embed"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// Shell out to way-embed generate for embedding vectors.
/// Generates two corpus files: one with EN model embeddings, one with multilingual.
///
/// `out_dir` receives the split corpora; `engine_dir` (always the canonical XDG
/// cache) supplies the way-embed binary and GGUF models. The two differ only
/// when `ways corpus --output <dir>` redirects an isolated build.
fn auto_embed(
    out_dir: &Path,
    engine_dir: &Path,
    corpus: &Path,
    verbose: bool,
    vlog: &dyn Fn(&str),
    log: &dyn Fn(&str),
) -> Result<()> {
    let bin = match resolve_embed_bin(engine_dir) {
        Some(b) => b,
        None => {
            log(&format!(
                "ERROR: embedding engine required (ADR-125). Run: cd {} && make setup",
                crate::paths::data_root().display()
            ));
            return Ok(());
        }
    };
    vlog(&format!("way-embed: {}", bin.display()));

    let en_model = engine_dir.join("minilm-l6-v2.gguf");
    let multi_model = engine_dir.join("multilingual-minilm-l12-v2-q8.gguf");
    vlog(&format!(
        "en model:    {} ({})",
        en_model.display(),
        if en_model.is_file() { "present" } else { "MISSING" }
    ));
    vlog(&format!(
        "multi model: {} ({})",
        multi_model.display(),
        if multi_model.is_file() { "present" } else { "absent" }
    ));

    // Split corpus into EN and multilingual entries
    vlog("splitting corpus into en / multilingual lanes");
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

    // Embed EN corpus
    if en_model.is_file() && en_count > 0 {
        log(&format!("Embedding {en_count} ways with English model..."));
        if let Some(t) = run_generate(&bin, &corpus_en, &en_model, "EN", verbose, vlog) {
            log(&format!(
                "  EN embeddings: {}{}",
                corpus_en.display(),
                elapsed_suffix(t, verbose)
            ));
        }
    }

    // Embed multilingual corpus
    if multi_model.is_file() && multi_count > 0 {
        log(&format!("Embedding {multi_count} ways with multilingual model..."));
        if let Some(t) = run_generate(&bin, &corpus_multi, &multi_model, "multilingual", verbose, vlog) {
            log(&format!(
                "  Multi embeddings: {}{}",
                corpus_multi.display(),
                elapsed_suffix(t, verbose)
            ));
        }
    } else if multi_count > 0 && !multi_model.is_file() {
        log(&format!("  {multi_count} multilingual ways found but model not installed"));
        log("  Run: make -C tools/way-embed model-multilingual  (127MB, on-demand per ADR-139)");
    }

    // Also generate combined corpus for backward compatibility
    // (the main ways-corpus.jsonl keeps EN embeddings as before)
    //
    // This re-embeds every entry the two passes above already embedded — the
    // longest pass, and the last, which is why a silent build looks like it hung
    // right here.
    if en_model.is_file() {
        log("Generating combined corpus with English embeddings...");
        vlog(&format!(
            "re-embedding all {} entries (en {en_count} + multi {multi_count})",
            en_count + multi_count
        ));
        if let Some(t) = run_generate(&bin, corpus, &en_model, "combined", verbose, vlog) {
            log(&format!(
                "Combined corpus: {}{}",
                corpus.display(),
                elapsed_suffix(t, verbose)
            ));
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
    // Check core + user ways (both unnamespaced roots feed the corpus).
    for root in [global_dir.to_path_buf(), crate::paths::user_ways_root()] {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&root)
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

// ── ADR-156 calibration fit ─────────────────────────────────────

/// Fit per-model calibration `g(s) = σ(a·s + b)` from the committed probe corpus
/// against the freshly generated aliases, gated on AUC separability. Returns
/// empty lanes when the engine, probes, or aliases are unavailable, or a lane's
/// fit is below the floor — the scan then degrades rather than trust a bad fit.
fn fit_calibration(
    out_dir: &Path,
    engine_dir: &Path,
    verbose: bool,
    vlog: &dyn Fn(&str),
    log: &dyn Fn(&str),
) -> ways_core::calibration::Calibration {
    use ways_core::calibration::Calibration;
    const PROBES: &str = include_str!("calibration_probes.jsonl");
    const AUC_FLOOR: f64 = 0.70;

    vlog("fitting calibration (ADR-156)");

    let bin = match resolve_embed_bin(engine_dir) {
        Some(b) => b,
        None => {
            vlog("  no way-embed — left uncalibrated");
            return Calibration::default();
        }
    };

    let aliases = match load_aliases(&out_dir.join("ways-corpus-en.jsonl")) {
        Some(m) if !m.is_empty() => m,
        _ => {
            vlog("  no en aliases — left uncalibrated");
            return Calibration::default();
        }
    };

    // Parse probes (skip `#` comments and blank lines).
    let probes: Vec<(String, String, bool)> = PROBES
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            Some((
                v["prompt"].as_str()?.to_string(),
                v["way"].as_str()?.to_string(),
                v["label"].as_i64()? == 1,
            ))
        })
        .collect();

    // Build (prompt, alias) pairs once — they are model-independent. A probe
    // whose `way` has no corpus alias (a renamed/removed way) is dropped; log the
    // count so path drift that would starve the fit is visible, not silent.
    let mut pairs = Vec::new();
    let mut labels = Vec::new();
    for (prompt, way, lbl) in &probes {
        if let Some(alias) = aliases.get(way) {
            // Guard the TSV framing: a tab/newline in a prompt or alias would
            // mispair the similarity input.
            let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
            pairs.push(format!("{}\t{}", clean(prompt), clean(alias)));
            labels.push(*lbl);
        }
    }
    let dropped = probes.len() - pairs.len();
    if dropped > 0 {
        log(&format!(
            "  calibration: {dropped}/{} probes reference a way not in the corpus (dropped)",
            probes.len()
        ));
    }
    if pairs.len() < 2 {
        log("  calibration: too few usable probes — left uncalibrated");
        return Calibration::default();
    }

    vlog(&format!("  {} usable probe pairs", pairs.len()));

    let fit_lane = |model_name: &str, label: &str| -> Option<ways_core::calibration::ModelCalibration> {
        let model = engine_dir.join(model_name);
        if !model.is_file() {
            return None;
        }
        vlog(&format!(
            "  lane[{label}]: {} similarity pairs via {}",
            pairs.len(),
            model.display()
        ));
        let cosines = batch_similarity(&bin, &model, &pairs, verbose)?;
        if cosines.len() != labels.len() {
            log(&format!(
                "  calibration[{label}]: score count {} != probe count {} — lane left uncalibrated",
                cosines.len(),
                labels.len()
            ));
            return None;
        }
        let samples: Vec<(f64, bool)> = cosines.into_iter().zip(labels.iter().copied()).collect();
        let cal = ways_core::calibration::fit(&samples)?;
        if cal.auc < AUC_FLOOR {
            log(&format!(
                "  calibration[{label}] REJECTED: AUC {:.3} < {AUC_FLOOR:.2} — lane left uncalibrated",
                cal.auc
            ));
            return None;
        }
        log(&format!(
            "  calibration[{label}]: a={:.2} b={:.2} AUC={:.3} (n={})",
            cal.a, cal.b, cal.auc, cal.n
        ));
        Some(cal)
    };

    let en = fit_lane("minilm-l6-v2.gguf", "en");
    // The multi lane is fit from the ENGLISH probe corpus and aliases. That is
    // correct for the English target; in localized mode the multi model scores
    // translated text, so this calibration is approximate until a localized
    // probe corpus ships (ADR-156 names multilingual calibration a follow-on).
    let multi = fit_lane("multilingual-minilm-l12-v2-q8.gguf", "multi");
    Calibration { en, multi }
}

/// Build `id -> "description vocabulary"` from a generated corpus JSONL, so the
/// fit scores each probe against the same alias text the corpus embedded.
fn load_aliases(corpus: &Path) -> Option<HashMap<String, String>> {
    let content = std::fs::read_to_string(corpus).ok()?;
    let mut m = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        // Skip blanks and any non-JSON line rather than abandoning the whole
        // map (and therefore all calibration) on a single malformed line.
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(id) = v["id"].as_str() {
            let desc = v["description"].as_str().unwrap_or("");
            let vocab = v["vocabulary"].as_str().unwrap_or("");
            m.insert(id.to_string(), format!("{desc} {vocab}").trim().to_string());
        }
    }
    Some(m)
}

/// Run `way-embed similarity --batch` over `prompt\talias` pairs on stdin,
/// returning one cosine per pair (order preserved).
fn batch_similarity(bin: &Path, model: &Path, pairs: &[String], verbose: bool) -> Option<Vec<f64>> {
    use std::process::{Command, Stdio};
    // Not `embed_stderr` — that carries a Windows-only NUL workaround this call
    // site has never used. Quiet stays `null()` on every platform, as before;
    // `--verbose` only adds the inherit case.
    let stderr = if verbose { Stdio::inherit() } else { Stdio::null() };
    let mut child = Command::new(bin)
        .args(["similarity", "--model", model.to_str()?, "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .ok()?;
    {
        let mut stdin = child.stdin.take()?;
        stdin.write_all(pairs.join("\n").as_bytes()).ok()?;
        stdin.write_all(b"\n").ok()?;
    } // stdin dropped here → EOF, so way-embed can finish
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect(),
    )
}
