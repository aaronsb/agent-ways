use anyhow::{bail, Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

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

    // The shipped ways: the projection the scanner reads, or the app itself
    // before the projection exists (a fresh install builds the corpus first).
    let projected = crate::paths::projected_ways_root();
    let default_core = if projected.is_dir() { projected } else { crate::paths::core_ways_root() };
    let global_dir = ways_dir.as_ref().map(PathBuf::from).unwrap_or_else(|| default_core.clone());

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
    // Naming the shipped ways themselves, as setup does, is a canonical build.
    let names_shipped = |d: &PathBuf| {
        let same = |a: &Path, b: &Path| a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(a, b)| a == b);
        same(d, &default_core) || same(d, &crate::paths::core_ways_root())
    };
    if ways_dir.is_some() && output_dir.is_none() && out_dir == engine_dir && !names_shipped(&global_dir) {
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
            let project_dir = crate::util::env_project_dir().unwrap_or_default();
            vlog("staleness check (walks core + user + project ways)");
            let bin = crate::paths::way_embed_in(&engine_dir);
            let engine = engine_fingerprint(bin.as_deref(), &engine_dir);
            if !is_stale(&manifest, &global_dir, &project_dir)
                && !retry_due(&manifest, &engine, agent_fmt::when::now_secs())
            {
                vlog("corpus is fresh — nothing to do");
                return Ok(());
            }
            vlog("corpus is stale — rebuilding");
        }
        // Missing manifest/corpus → always regen
    }
    std::fs::create_dir_all(&out_dir)?;
    let corpus_path = out_dir.join("ways-corpus.jsonl");
    sweep_staging_debris(&out_dir);

    // All three corpus files are written to staging siblings; `settle` moves
    // them into place according to how the embedding passes went.
    let staged = Staged::new(&out_dir);
    let tmpfile = &staged.combined.0;
    let mut w = BufWriter::new(
        std::fs::File::create(tmpfile)
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
    if let Some(cpd) = crate::util::env_project_dir() {
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

    let projects_dir = ways_core::paths::transcripts_root();
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
            let project_path = match claude_sessions::resolve_project_path(&projects_dir, &encoded) {
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

    // A failed pass never replaces an embedded corpus (#645).
    let bin = crate::paths::way_embed_in(&engine_dir);
    let engine = engine_fingerprint(bin.as_deref(), &engine_dir);
    let generate = |bin: &Path, corpus: &Path, model: &Path, what: &str| {
        run_generate(bin, corpus, model, what, verbose, &vlog)
    };
    let outcome = auto_embed(&staged, bin.as_deref(), &engine_dir, &generate, verbose, &vlog, &log)?;
    let complete = matches!(outcome, Embedded::Complete);
    let promote = matches!(outcome, Embedded::Complete | Embedded::Degraded(_));
    let manifest_path = out_dir.join("embed-manifest.json");
    let previous_calibration: ways_core::calibration::Calibration = read_manifest(&manifest_path)
        .and_then(|m| m.get("calibration").cloned())
        .and_then(|c| serde_json::from_value(c).ok())
        .unwrap_or_default();
    vlog(&format!("settling corpus: {}", corpus_path.display()));
    let settled = settle(&staged, promote)?;

    let total = global_count + user_count + project_total;
    log(&format!(
        "Generated {}: {total} ways ({global_count} core, {user_count} user, {project_total} project)",
        corpus_path.display()
    ));

    // Fit per-model calibration g(s)=σ(a·s+b) from the probe corpus (ADR-156).
    // It needs embeddings: a kept corpus keeps its calibration, a raw one has none.
    let calibration = match settled {
        Settled::Promoted => fit_calibration(&out_dir, &engine_dir, verbose, &vlog, &log),
        Settled::KeptPrevious => previous_calibration,
        Settled::WroteRaw => Default::default(),
    };

    // The manifest records whether every lane embedded, why not, when it
    // failed, and which engine built it. `--if-stale` retries a failed build
    // when the engine changes or a day has passed, not on every session start.
    let reason = match &outcome {
        Embedded::Complete => None,
        Embedded::Degraded(why) | Embedded::Unavailable(why) | Embedded::Failed(why) => Some(why.clone()),
    };
    let manifest = json!({
        "global_hash": global_hash,
        "global_count": global_count,
        "user_hash": user_hash,
        "user_count": user_count,
        "total_count": total,
        "projects": manifest_projects,
        "calibration": calibration,
        "embedded": complete,
        "reason": reason,
        "failed_at": if complete { None } else { Some(agent_fmt::when::now_secs()) },
        "engine": engine,
    });
    vlog("writing manifest");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    log(&format!("Manifest written: {}", manifest_path.display()));
    vlog("done");

    let what_happened = match settled {
        Settled::Promoted => String::new(),
        Settled::KeptPrevious => format!("kept the previous corpus at {}", corpus_path.display()),
        Settled::WroteRaw => format!(
            "wrote {} without embeddings, so matching is keyword-only until `ways corpus` succeeds",
            corpus_path.display()
        ),
    };
    match outcome {
        Embedded::Complete => Ok(()),
        // An optional lane failed; the rest was promoted.
        Embedded::Degraded(why) => {
            eprintln!("warning: {why}; kept the previous multilingual corpus");
            Ok(())
        }
        // No engine is an install state: say so, and exit 0 so `make setup`
        // and keyword-only installs carry on.
        Embedded::Unavailable(why) => {
            eprintln!("warning: {why}; {what_happened}");
            Ok(())
        }
        // A pass that ran and failed is an error for a caller who asked for a
        // build. The SessionStart hook (`--if-stale`) reports it and exits 0:
        // hooks must not fail, and `ways status` carries the reason.
        Embedded::Failed(why) if if_stale => {
            eprintln!("warning: {why}; {what_happened}");
            Ok(())
        }
        Embedded::Failed(why) => bail!("{why}; {what_happened}"),
    }
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

/// Run one `way-embed generate` pass, returning the elapsed time on success
/// and the reason on failure.
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
) -> std::result::Result<Instant, String> {
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
        Ok(s) if s.success() => Ok(t),
        Ok(s) => Err(format!("{what} embedding generation failed ({s})")),
        Err(e) => Err(format!("{what} embedding generation could not start: {e}")),
    }
}

/// The three corpus files, each staged beside its final path first:
/// `(staged, final)` pairs for the combined corpus and the two model lanes.
/// Staging names carry the process id, so two concurrent builds never write
/// or promote each other's files.
struct Staged {
    combined: (PathBuf, PathBuf),
    en: (PathBuf, PathBuf),
    multi: (PathBuf, PathBuf),
}

impl Staged {
    fn new(out_dir: &Path) -> Self {
        let pid = std::process::id();
        let pair = |name: &str| (out_dir.join(format!("{name}.{pid}.tmp")), out_dir.join(name));
        Staged {
            combined: pair("ways-corpus.jsonl"),
            en: pair("ways-corpus-en.jsonl"),
            multi: pair("ways-corpus-multi.jsonl"),
        }
    }

    /// Promotion order: the lane splits first, the combined corpus last, so a
    /// rename that fails partway leaves the old combined corpus in place.
    fn pairs(&self) -> [&(PathBuf, PathBuf); 3] {
        [&self.en, &self.multi, &self.combined]
    }

    /// Refresh the staged files' mtimes, so a concurrent build's debris sweep
    /// never takes them for leftovers while a long pass runs.
    fn touch(&self) {
        let now = std::time::SystemTime::now();
        for (tmp, _) in self.pairs() {
            if let Ok(f) = std::fs::File::options().append(true).open(tmp) {
                let _ = f.set_modified(now);
            }
        }
    }
}

/// Remove staging files a killed build left behind: `*.tmp` from this module
/// and `*.tmp.tmp` from `way-embed generate`. Only files older than an hour
/// go, so a build running concurrently keeps its own.
fn sweep_staging_debris(out_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(out_dir) else { return };
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("ways-corpus") && name.contains(".tmp")) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t < hour_ago)
            .unwrap_or(false);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// How the embedding passes went.
#[derive(Debug)]
enum Embedded {
    /// Every pass that had a model ran and succeeded.
    Complete,
    /// The multilingual pass failed. Its model is optional (ADR-139), so the
    /// English lanes are promoted and the previous multilingual corpus kept.
    Degraded(String),
    /// No engine or English model is installed: an install state, not a fault.
    Unavailable(String),
    /// A pass ran and failed; the reason, for the operator.
    Failed(String),
}

/// What reached the corpus paths.
#[derive(Debug, PartialEq)]
enum Settled {
    /// The staged files replaced the corpus.
    Promoted,
    /// Embedding did not complete and a previous corpus exists; it stays,
    /// since its vectors still match and a raw corpus would drop matching to
    /// keywords.
    KeptPrevious,
    /// Embedding did not complete and there was no previous corpus; the raw
    /// one was written, keyword matching being better than none.
    WroteRaw,
}

/// Move the staged corpus into place, or keep what is there (#645).
fn settle(staged: &Staged, complete: bool) -> Result<Settled> {
    if !complete && staged.combined.1.is_file() {
        for (tmp, _) in staged.pairs() {
            let _ = std::fs::remove_file(tmp);
        }
        return Ok(Settled::KeptPrevious);
    }
    for (tmp, dest) in staged.pairs() {
        if tmp.is_file() {
            std::fs::rename(tmp, dest)
                .with_context(|| format!("moving {} into place", dest.display()))?;
        } else if !complete && dest != &staged.combined.1 {
            // A split from an older build must not sit beside a new raw corpus.
            let _ = std::fs::remove_file(dest);
        }
    }
    Ok(if complete { Settled::Promoted } else { Settled::WroteRaw })
}

/// A string that changes when the engine binary or either model changes:
/// path, size and mtime of each, or `absent`.
fn engine_fingerprint(bin: Option<&Path>, engine_dir: &Path) -> String {
    let describe = |p: &Path| match p.metadata() {
        Ok(m) => {
            let secs = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("{}:{}:{secs}", p.display(), m.len())
        }
        Err(_) => format!("{}:absent", p.display()),
    };
    [
        bin.map(describe).unwrap_or_else(|| "way-embed:absent".to_string()),
        describe(&engine_dir.join(crate::paths::EN_MODEL)),
        describe(&engine_dir.join(crate::paths::MULTI_MODEL)),
    ]
    .join("|")
}

fn read_manifest(path: &Path) -> Option<serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
}

/// How long a failed build waits before `--if-stale` retries it on its own.
const RETRY_AFTER_SECS: u64 = 24 * 3600;

/// True when the manifest records a build that did not fully embed and a
/// retry is due: the engine changed since (a repair), or a day has passed (a
/// transient failure). Manifests written before the `embedded` field count as
/// embedded.
fn retry_due(manifest: &Path, engine: &str, now: u64) -> bool {
    let Some(m) = read_manifest(manifest) else { return false };
    if m.get("embedded").and_then(|v| v.as_bool()).unwrap_or(true) {
        return false;
    }
    let engine_changed = m.get("engine").and_then(|v| v.as_str()) != Some(engine);
    let failed_at = m.get("failed_at").and_then(|v| v.as_u64()).unwrap_or(0);
    engine_changed || now.saturating_sub(failed_at) >= RETRY_AFTER_SECS
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

    for path in crate::scanner::files(dir) {
        let path = path.as_path();
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
        if crate::scanner::is_check(path) {
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

/// One `way-embed generate` pass: (binary, corpus, model, pass name) to the
/// elapsed time, or the reason it failed.
type GeneratePass<'a> = dyn Fn(&Path, &Path, &Path, &str) -> std::result::Result<Instant, String> + 'a;

/// Shell out to way-embed generate for embedding vectors, on the staged files.
/// Splits the staged combined corpus into EN and multilingual lanes, embeds
/// each with its model, then embeds the combined corpus with the EN model.
///
/// `bin` is the resolved way-embed binary; `engine_dir` (always the canonical
/// XDG cache) supplies the GGUF models. `generate` runs one pass (bin, corpus,
/// model, pass name) — `run_generate` in production, a fake in tests.
fn auto_embed(
    staged: &Staged,
    bin: Option<&Path>,
    engine_dir: &Path,
    generate: &GeneratePass<'_>,
    verbose: bool,
    vlog: &dyn Fn(&str),
    log: &dyn Fn(&str),
) -> Result<Embedded> {
    let setup_hint = format!("run: cd {} && make setup", crate::paths::data_root().display());
    let Some(bin) = bin else {
        return Ok(Embedded::Unavailable(format!(
            "embedding engine not installed (ADR-125); {setup_hint}"
        )));
    };
    vlog(&format!("way-embed: {}", bin.display()));

    let en_model = engine_dir.join(crate::paths::EN_MODEL);
    let multi_model = engine_dir.join(crate::paths::MULTI_MODEL);
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
    if !en_model.is_file() {
        return Ok(Embedded::Unavailable(format!(
            "English model missing at {}; {setup_hint}",
            en_model.display()
        )));
    }

    // Split corpus into EN and multilingual entries
    vlog("splitting corpus into en / multilingual lanes");
    let corpus = &staged.combined.0;
    let corpus_content = std::fs::read_to_string(corpus)?;
    let corpus_en = &staged.en.0;
    let corpus_multi = &staged.multi.0;
    let mut en_count = 0usize;
    let mut multi_count = 0usize;

    {
        let mut w_en = std::io::BufWriter::new(std::fs::File::create(corpus_en)?);
        let mut w_multi = std::io::BufWriter::new(std::fs::File::create(corpus_multi)?);

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
    if en_count > 0 {
        log(&format!("Embedding {en_count} ways with English model..."));
        staged.touch();
        match generate(bin, corpus_en, &en_model, "EN") {
            Ok(t) => log(&format!(
                "  EN embeddings: {}{}",
                staged.en.1.display(),
                elapsed_suffix(t, verbose)
            )),
            Err(why) => return Ok(Embedded::Failed(why)),
        }
    }

    // Embed multilingual corpus. Its model is installed on demand (ADR-139):
    // an absent model skips the lane, and a failed pass keeps the previous
    // multilingual corpus instead of holding back the English lanes.
    let mut multi_failure = None;
    if multi_model.is_file() && multi_count > 0 {
        log(&format!("Embedding {multi_count} ways with multilingual model..."));
        staged.touch();
        match generate(bin, corpus_multi, &multi_model, "multilingual") {
            Ok(t) => log(&format!(
                "  Multi embeddings: {}{}",
                staged.multi.1.display(),
                elapsed_suffix(t, verbose)
            )),
            Err(why) => {
                let _ = std::fs::remove_file(corpus_multi);
                multi_failure = Some(why);
            }
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
    log("Generating combined corpus with English embeddings...");
    vlog(&format!(
        "re-embedding all {} entries (en {en_count} + multi {multi_count})",
        en_count + multi_count
    ));
    staged.touch();
    match generate(bin, corpus, &en_model, "combined") {
        Ok(t) => log(&format!(
            "Combined corpus: {}{}",
            staged.combined.1.display(),
            elapsed_suffix(t, verbose)
        )),
        Err(why) => return Ok(Embedded::Failed(why)),
    }

    Ok(match multi_failure {
        Some(why) => Embedded::Degraded(why),
        None => Embedded::Complete,
    })
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

/// Content hash of a directory: FNV-1a over the sorted file list (relative
/// paths joined with `/`) and sizes, stable across Rust releases and
/// platforms, so a manifest written by one build or OS matches another's hash
/// of the same tree.
fn content_hash(dir: &Path) -> String {
    let mut entries: Vec<(String, u64)> = crate::scanner::files(dir)
        .map(|path| {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            // `/`-joined on every OS: `display()` gives `\` on Windows, and the
            // same tree must hash the same everywhere.
            let rel = crate::util::path_to_id(path.strip_prefix(dir).unwrap_or(&path));
            (rel, size)
        })
        .collect();
    entries.sort();
    format!("{:016x}", agent_identity::identity::fnv1a_64(&content_hash_input(&entries)))
}

/// The bytes [`content_hash`] hashes: each path, a NUL, its size as 8
/// little-endian bytes. The NUL keeps `a` + size from colliding with `a1`.
fn content_hash_input(entries: &[(String, u64)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (rel, size) in entries {
        bytes.extend_from_slice(rel.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&size.to_le_bytes());
    }
    bytes
}

use crate::util::home_dir;

/// True if a way or locale file (`.md`, `.jsonl`) under `root` is newer than
/// the manifest.
fn any_way_file_newer(root: &Path, manifest: &Path) -> bool {
    crate::scanner::files(root).any(|path| {
        let ext = path.extension().and_then(|e| e.to_str());
        (ext == Some("md") || ext == Some("jsonl")) && is_newer_than(&path, manifest)
    })
}

/// Check if any way file is newer than the manifest.
fn is_stale(manifest: &Path, global_dir: &Path, project_dir: &str) -> bool {
    // Check core + user ways (both unnamespaced roots feed the corpus).
    for root in [global_dir.to_path_buf(), crate::paths::user_ways_root()] {
        if !root.is_dir() {
            continue;
        }
        if any_way_file_newer(&root, manifest) {
            return true;
        }
    }

    // Check project ways
    if !project_dir.is_empty() {
        let project_ways = Path::new(project_dir).join(".claude/ways");
        if project_ways.is_dir() && any_way_file_newer(&project_ways, manifest) {
            return true;
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

    let bin = match crate::paths::way_embed_in(engine_dir) {
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

    let en = fit_lane(crate::paths::EN_MODEL, "en");
    // The multi lane is fit from the ENGLISH probe corpus and aliases. That is
    // correct for the English target; in localized mode the multi model scores
    // translated text, so this calibration is approximate until a localized
    // probe corpus ships (ADR-156 names multilingual calibration a follow-on).
    let multi = fit_lane(crate::paths::MULTI_MODEL, "multi");
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The hash is a fixed function of the tree, not of the Rust release: a
    /// known input pins its value. DefaultHasher made no such promise. The
    /// nested fixture also pins the `/` separator: Windows hashed `a\a.md`.
    #[test]
    fn content_hash_is_pinned_fnv1a() {
        let input = content_hash_input(&[("a/a.md".to_string(), 3)]);
        assert_eq!(input, b"a/a.md\0\x03\0\0\0\0\0\0\0".to_vec());
        let dir = std::env::temp_dir().join(format!("ways-content-hash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("a/a.md"), "abc").unwrap();
        let want = format!("{:016x}", agent_identity::identity::fnv1a_64(&input));
        assert_eq!(content_hash(&dir), want);
        let _ = std::fs::remove_dir_all(&dir);
    }
    use std::cell::RefCell;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ways-corpus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    /// Finals holding `previous` (if any) and every staged file holding "staged".
    fn scene(name: &str, previous: Option<&str>) -> (PathBuf, Staged) {
        let dir = scratch(name);
        let staged = Staged::new(&dir);
        for (tmp, dest) in staged.pairs() {
            if let Some(prev) = previous {
                std::fs::write(dest, prev).unwrap();
            }
            std::fs::write(tmp, "staged").unwrap();
        }
        (dir, staged)
    }

    #[test]
    fn complete_promotes_every_staged_file() {
        let (dir, staged) = scene("complete", Some("previous"));
        assert_eq!(settle(&staged, true).unwrap(), Settled::Promoted);
        for (tmp, dest) in staged.pairs() {
            assert_eq!(read(dest), "staged", "{} not promoted", dest.display());
            assert!(!tmp.exists(), "{} left behind", tmp.display());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn incomplete_keeps_the_previous_corpus() {
        let (dir, staged) = scene("keep", Some("previous"));
        assert_eq!(settle(&staged, false).unwrap(), Settled::KeptPrevious);
        for (tmp, dest) in staged.pairs() {
            assert_eq!(read(dest), "previous", "{} replaced", dest.display());
            assert!(!tmp.exists(), "{} left behind", tmp.display());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn incomplete_with_no_previous_corpus_writes_the_raw_one_and_drops_old_splits() {
        let dir = scratch("raw");
        let staged = Staged::new(&dir);
        // Only the combined corpus was staged (the engine was missing), and a
        // split from an older build is lying around.
        std::fs::write(&staged.combined.0, "raw").unwrap();
        std::fs::write(&staged.en.1, "old split").unwrap();
        assert_eq!(settle(&staged, false).unwrap(), Settled::WroteRaw);
        assert_eq!(read(&staged.combined.1), "raw");
        assert!(!staged.en.1.exists(), "stale EN split survived beside the raw corpus");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn staging_names_are_per_process() {
        let staged = Staged::new(Path::new("/x"));
        let pid = std::process::id().to_string();
        for (tmp, dest) in staged.pairs() {
            assert!(tmp.to_string_lossy().contains(&pid), "{}", tmp.display());
            assert_ne!(tmp, dest);
        }
    }

    #[test]
    fn a_failed_build_is_retried_on_engine_change_or_after_a_day() {
        let dir = scratch("stale");
        let manifest = dir.join("embed-manifest.json");
        let t0 = 1_000_000u64;
        std::fs::write(&manifest, format!(r#"{{"embedded": false, "engine": "A", "failed_at": {t0}}}"#)).unwrap();
        assert!(!retry_due(&manifest, "A", t0 + 60), "same engine retried within the day");
        assert!(retry_due(&manifest, "B", t0 + 60), "changed engine not retried");
        assert!(retry_due(&manifest, "A", t0 + RETRY_AFTER_SECS), "transient failure never retried");
        std::fs::write(&manifest, r#"{"embedded": true, "engine": "A"}"#).unwrap();
        assert!(!retry_due(&manifest, "B", t0 + RETRY_AFTER_SECS), "embedded corpus retried");
        std::fs::write(&manifest, r#"{"global_hash": "x"}"#).unwrap();
        assert!(!retry_due(&manifest, "B", t0), "pre-field manifest treated as failed");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Run auto_embed against a staged corpus with one EN and one multilingual
    /// way, a fake `generate` that fails the named pass, and the given models.
    /// Returns the outcome, the passes called in order, and the staged files.
    fn embed_case(
        name: &str,
        bin: bool,
        en_model: bool,
        multi_model: bool,
        fail: Option<&str>,
    ) -> (Embedded, Vec<String>, Staged, PathBuf) {
        let dir = scratch(name);
        let engine = dir.join("engine");
        std::fs::create_dir_all(&engine).unwrap();
        if en_model {
            std::fs::write(engine.join(crate::paths::EN_MODEL), "m").unwrap();
        }
        if multi_model {
            std::fs::write(engine.join(crate::paths::MULTI_MODEL), "m").unwrap();
        }
        let staged = Staged::new(&dir);
        std::fs::write(
            &staged.combined.0,
            "{\"id\":\"a\"}\n{\"id\":\"b\",\"embed_model\":\"multilingual\"}\n",
        )
        .unwrap();
        let calls = RefCell::new(Vec::new());
        let generate = |_: &Path, _: &Path, _: &Path, what: &str| {
            calls.borrow_mut().push(what.to_string());
            if Some(what) == fail {
                Err(format!("{what} embedding generation failed (signal: 4)"))
            } else {
                Ok(Instant::now())
            }
        };
        let bin_path = dir.join("way-embed");
        let outcome = auto_embed(
            &staged,
            bin.then_some(bin_path.as_path()),
            &engine,
            &generate,
            false,
            &|_| {},
            &|_| {},
        )
        .unwrap();
        (outcome, calls.into_inner(), staged, dir)
    }

    #[test]
    fn auto_embed_outcomes() {
        let (o, calls, _, d) = embed_case("all-ok", true, true, true, None);
        assert!(matches!(o, Embedded::Complete), "{o:?}");
        assert_eq!(calls, ["EN", "multilingual", "combined"]);
        std::fs::remove_dir_all(d).unwrap();

        let (o, calls, _, d) = embed_case("no-bin", false, true, true, None);
        assert!(matches!(o, Embedded::Unavailable(ref w) if w.contains("engine not installed")), "{o:?}");
        assert!(calls.is_empty());
        std::fs::remove_dir_all(d).unwrap();

        let (o, calls, _, d) = embed_case("no-en-model", true, false, true, None);
        assert!(matches!(o, Embedded::Unavailable(ref w) if w.contains("English model missing")), "{o:?}");
        assert!(calls.is_empty());
        std::fs::remove_dir_all(d).unwrap();

        let (o, calls, _, d) = embed_case("en-fails", true, true, true, Some("EN"));
        assert!(matches!(o, Embedded::Failed(ref w) if w.contains("EN embedding")), "{o:?}");
        assert_eq!(calls, ["EN"]);
        std::fs::remove_dir_all(d).unwrap();

        let (o, calls, staged, d) = embed_case("multi-fails", true, true, true, Some("multilingual"));
        assert!(
            matches!(o, Embedded::Degraded(ref w) if w.contains("multilingual")),
            "multilingual failure not reported as degraded: {o:?}"
        );
        assert_eq!(calls, ["EN", "multilingual", "combined"]);
        assert!(!staged.multi.0.exists(), "failed multilingual lane still staged for promotion");
        std::fs::remove_dir_all(d).unwrap();

        let (o, calls, _, d) = embed_case("no-multi-model", true, true, false, None);
        assert!(matches!(o, Embedded::Complete), "{o:?}");
        assert_eq!(calls, ["EN", "combined"]);
        std::fs::remove_dir_all(d).unwrap();

        let (o, calls, _, d) = embed_case("combined-fails", true, true, true, Some("combined"));
        assert!(matches!(o, Embedded::Failed(ref w) if w.contains("combined")), "{o:?}");
        assert_eq!(calls, ["EN", "multilingual", "combined"]);
        std::fs::remove_dir_all(d).unwrap();
    }
}
