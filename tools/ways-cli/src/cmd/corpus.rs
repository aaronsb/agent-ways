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
    let default_core = crate::paths::shipped_ways_root();
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

    // Staleness check: skip regen if corpus is fresh. A fresh corpus whose
    // sidecar was built by another engine (a reinstalled or upgraded
    // way-embed, a new model or chunker) rebuilds the sidecar alone.
    let mut sidecar_only = false;
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
                if !bin.as_deref().is_some_and(|b| sidecar_engine_stale(&manifest, &engine_dir, b)) {
                    vlog("corpus is fresh — nothing to do");
                    return Ok(());
                }
                vlog("corpus is fresh, sidecar built by another engine — rebuilding the sidecar");
                sidecar_only = true;
            } else {
                vlog("corpus is stale — rebuilding");
            }
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

    let report = Report { quiet, emit: &|m| eprintln!("{m}") };

    let excluded = crate::util::load_excluded_segments();
    let empty_skip: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Scan USER ways first (ADR-143): the operator's own root in $XDG_CONFIG.
    // Its ids become the skip set for core, so a user way shadows a same-named
    // shipped way (precedence project > user > core).
    let user_dir = crate::paths::user_ways_root();
    vlog(&format!("user ways dir:   {}", user_dir.display()));
    // Every way the corpus holds, by id, with the file it came from: the body
    // sidecar embeds these files and the manifest records their hashes.
    let mut sources: HashMap<String, PathBuf> = HashMap::new();
    let user_count = if user_dir.is_dir() {
        vlog("scanning user ways");
        let c = scan_ways_dir(&user_dir, "", &excluded, &mut w, &empty_skip, &mut sources)?;
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
    vlog("scanning core ways");
    let global_count = scan_ways_dir(&global_dir, "", &excluded, &mut w, &user_shadow, &mut sources)?;
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
    // encode_project_key, so it matches exactly what `ways scan <lane> --project`
    // computes for the same directory (the fix for Bug B).
    if let Some(cpd) = crate::util::env_project_dir() {
        vlog(&format!("current project (CLAUDE_PROJECT_DIR): {cpd}"));
        let proj_root = PathBuf::from(&cpd);
        if let Some(ways_path) = super::ways_roots::project_ways(&proj_root) {
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
                &mut sources,
                &log,
            )?;
        }
    }

    // Every other project Claude Code knows, from its transcript directories.
    for (project_path, ways_path) in super::ways_roots::known_project_ways(&|line| vlog(line)) {
        vlog(&format!("    ways: {}", ways_path.display()));
        // Dedup: multiple encoded dirs (and the current project above) may
        // resolve to the same .claude/ways/. Compare canonical paths.
        let canon = std::fs::canonicalize(&ways_path).unwrap_or_else(|_| ways_path.clone());
        if !seen_ways_dirs.insert(canon) {
            continue;
        }
        // Key off the resolved REAL path, not the lossy encoded dir name, so
        // it matches `ways scan <lane> --project <that project>`.
        let key = crate::util::encode_project_key(Path::new(&project_path));
        project_total += embed_one_project(&ways_path, &key, &project_path, &excluded, &mut w, &mut manifest_projects, &mut sources, &log)?;
    }

    w.flush()?;
    drop(w);

    if sidecar_only {
        if let Some(bin) = crate::paths::way_embed_in(&engine_dir) {
            let generate = |bin: &Path, corpus: &Path, model: &Path, what: &str| {
                run_generate(bin, corpus, model, what, verbose, &vlog)
            };
            if refresh_stale_sidecar(&out_dir, &engine_dir, &bin, &sources, &generate, vector_support(&bin), &report)? {
                let _ = std::fs::remove_file(tmpfile);
                return Ok(());
            }
        }
        vlog("the way files changed since the last build — full rebuild");
    }

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
    let previous_manifest = read_manifest(&manifest_path);
    let previous_calibration: ways_core::calibration::Calibration = previous_manifest
        .as_ref()
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

    // ADR-701 §6: the body sidecar and the per-way content hashes it is checked
    // against. Both describe the alias corpus in place, so a kept corpus keeps
    // the previous build's hashes and sidecar; a promoted one gets new ones.
    let way_hashes = match settled {
        Settled::KeptPrevious => previous_manifest
            .as_ref()
            .and_then(|m| m.get("way_hashes").cloned())
            .unwrap_or_else(|| json!({})),
        Settled::Promoted | Settled::WroteRaw => way_hashes(&sources),
    };
    let body_sidecar = match (&settled, bin.as_deref()) {
        (Settled::Promoted, Some(bin)) => {
            vlog("building body sidecar (ADR-701 §6)");
            let side = refresh_body_sidecar(&out_dir, &engine_dir, bin, &sources, &generate, vector_support(bin));
            report.sidecar(&side, &out_dir, false);
            side
        }
        _ => previous_manifest
            .as_ref()
            .and_then(|m| m.get("body_sidecar").cloned())
            .unwrap_or(serde_json::Value::Null),
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
        "way_hashes": way_hashes,
        "body_sidecar": body_sidecar,
        "embedded": complete,
        "reason": reason,
        "failed_at": if complete && !sidecar_failed(&body_sidecar) { None } else { Some(agent_fmt::when::now_secs()) },
        "engine": engine,
    });
    vlog("writing manifest");
    write_manifest(&manifest_path, &manifest)?;
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

/// Each corpus way's content hash, for the manifest's `way_hashes` (ADR-701
/// §7). A file that cannot be read gets no entry, so a sidecar can never cover
/// it.
fn way_hashes(sources: &HashMap<String, PathBuf>) -> serde_json::Value {
    use crate::cmd::scan::sidecar::{content_hash, hash_hex};
    let mut ids: Vec<&String> = sources.keys().collect();
    ids.sort();
    let map: serde_json::Map<String, serde_json::Value> = ids
        .into_iter()
        .filter_map(|id| {
            let bytes = std::fs::read(&sources[id]).ok()?;
            Some((id.clone(), json!(hash_hex(content_hash(&bytes)))))
        })
        .collect();
    serde_json::Value::Object(map)
}

/// The `body_sidecar` manifest entry after rebuilding the sidecar. The old file
/// goes first, so a failed or skipped build never leaves a sidecar that no
/// longer describes the corpus. With a way-embed that cannot return chunk
/// vectors the sidecar is useless to a scan and is not built.
fn refresh_body_sidecar(
    out_dir: &Path,
    engine_dir: &Path,
    bin: &Path,
    sources: &HashMap<String, PathBuf>,
    generate: &GeneratePass<'_>,
    vectors: VectorSupport,
) -> serde_json::Value {
    let _ = std::fs::remove_file(out_dir.join(crate::cmd::scan::sidecar::FILE));
    match vectors {
        VectorSupport::Yes => {}
        VectorSupport::Unsupported => return json!({ "file": null, "reason": NO_VECTORS, "unsupported": true }),
        VectorSupport::Unknown(why) => return json!({ "file": null, "reason": why }),
    }
    match build_body_sidecar(out_dir, engine_dir, bin, sources, generate) {
        Ok((ways, sections)) => json!({
            "file": crate::cmd::scan::sidecar::FILE,
            "ways": ways,
            "sections": sections,
            "vectors": true,
            "model_id": crate::cmd::scan::sidecar::model_id(engine_dir, bin),
        }),
        Err(why) => json!({ "file": null, "reason": why }),
    }
}

/// True when the manifest records a built sidecar whose engine fingerprint
/// (model, way-embed binary, chunker revision; `sidecar::model_id`) differs
/// from the current one: a reinstalled or upgraded way-embed, a new model or
/// a new chunker. Scans then confirm per call until the sidecar is rebuilt.
fn sidecar_engine_stale(manifest: &Path, engine_dir: &Path, bin: &Path) -> bool {
    let Some(m) = read_manifest(manifest) else { return false };
    if !m.get("embedded").and_then(|v| v.as_bool()).unwrap_or(true) {
        return false;
    }
    let side = m.get("body_sidecar");
    if !side.and_then(|s| s.get("file")).is_some_and(|f| f.is_string()) {
        return false;
    }
    let Some(now) = crate::cmd::scan::sidecar::model_id(engine_dir, bin) else { return false };
    side.and_then(|s| s.get("model_id")).and_then(|v| v.as_str()) != Some(now.as_str())
}

/// Rebuild only the sidecar for a corpus that is otherwise current, and record
/// it in the manifest. `Ok(false)`, touching nothing, when the ways no longer
/// hash as the manifest records: the corpus itself needs a rebuild.
fn refresh_stale_sidecar(
    out_dir: &Path,
    engine_dir: &Path,
    bin: &Path,
    sources: &HashMap<String, PathBuf>,
    generate: &GeneratePass<'_>,
    vectors: VectorSupport,
    report: &Report<'_>,
) -> Result<bool> {
    let manifest_path = out_dir.join("embed-manifest.json");
    let Some(mut m) = read_manifest(&manifest_path) else { return Ok(false) };
    if m.get("way_hashes") != Some(&way_hashes(sources)) {
        return Ok(false);
    }
    let side = refresh_body_sidecar(out_dir, engine_dir, bin, sources, generate, vectors);
    report.sidecar(&side, out_dir, true);
    m["failed_at"] = if sidecar_failed(&side) { json!(agent_fmt::when::now_secs()) } else { serde_json::Value::Null };
    m["body_sidecar"] = side;
    write_manifest(&manifest_path, &m)?;
    Ok(true)
}

/// Where a corpus build's messages go. `emit` is stderr in the CLI; tests
/// collect. `quiet` is `--quiet` (`--verbose` already outranks it).
struct Report<'a> {
    quiet: bool,
    emit: &'a dyn Fn(&str),
}

impl Report<'_> {
    /// Say what happened to the sidecar: the warning when none was built
    /// (unless it is the quiet-silenced install state), the success line
    /// (not under `--quiet`) when one was.
    fn sidecar(&self, side: &serde_json::Value, out_dir: &Path, rebuilt: bool) {
        if let Some(warning) = sidecar_warning(side, self.quiet) {
            (self.emit)(&warning);
        } else if side.get("reason").is_none() && !self.quiet {
            let file = out_dir.join(crate::cmd::scan::sidecar::FILE).display().to_string();
            (self.emit)(&if rebuilt {
                format!("Body sidecar rebuilt for the current engine: {file}")
            } else {
                format!("Body sidecar: {file} ({} ways, {} sections)", side["ways"], side["sections"])
            });
        }
    }
}

/// The warning for a sidecar the build did not produce, or None when there is
/// nothing to say. A way-embed without `--vectors` is an install state, not a
/// failure, so `--quiet` silences it; any other reason is always reported.
fn sidecar_warning(side: &serde_json::Value, quiet: bool) -> Option<String> {
    let why = side.get("reason").and_then(|r| r.as_str())?;
    if quiet && side.get("unsupported").and_then(|v| v.as_bool()).unwrap_or(false) {
        return None;
    }
    Some(format!("warning: body sidecar not built: {why}; body confirmation embeds per call"))
}

/// Write the embed manifest through a sibling staging file, so a reader or a
/// crash never sees it half-written.
fn write_manifest(path: &Path, manifest: &serde_json::Value) -> Result<()> {
    agent_settings::writer::write_atomic(path, serde_json::to_string_pretty(manifest)?)
        .with_context(|| format!("writing {}", path.display()))
}

/// Why no sidecar is built for a way-embed older than 1.2.0.
const NO_VECTORS: &str = "way-embed < 1.2.0 lacks --vectors";

/// Whether the installed way-embed can return chunk vectors (`match
/// --vectors`, 1.2.0 and later). Checked once per corpus build.
#[derive(Debug, PartialEq)]
enum VectorSupport {
    Yes,
    /// A version was read and it is below 1.2.0: an install state.
    Unsupported,
    /// The version could not be read (the binary failed or printed something
    /// unrecognised): a failure, with its reason.
    Unknown(String),
}

fn vector_support(bin: &Path) -> VectorSupport {
    match retry_if_busy(|| std::process::Command::new(bin).arg("--version").output()) {
        Err(e) => VectorSupport::Unknown(format!("way-embed --version could not run: {e}")),
        Ok(o) if !o.status.success() => VectorSupport::Unknown(format!("way-embed --version failed ({})", o.status)),
        Ok(o) => classify_version(&String::from_utf8_lossy(&o.stdout)),
    }
}

/// Run `spawn`, trying again a few times while the binary is busy (ETXTBSY: a
/// concurrent install or a forked child still holds it open for writing).
fn retry_if_busy<T>(mut spawn: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    const TRIES: u32 = 5;
    let mut tries = 1;
    loop {
        match spawn() {
            Err(e) if tries < TRIES && is_text_file_busy(&e) => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

fn is_text_file_busy(e: &std::io::Error) -> bool {
    cfg!(unix) && e.raw_os_error() == Some(26)
}

fn classify_version(out: &str) -> VectorSupport {
    match parse_way_embed_version(out) {
        Some(v) if v >= (1, 2, 0) => VectorSupport::Yes,
        Some(_) => VectorSupport::Unsupported,
        None => VectorSupport::Unknown(format!("way-embed --version printed {:?}, not a version", out.trim())),
    }
}

/// `way-embed X.Y.Z[-pre][+build]` to `(X, Y, Z)`; a suffix is ignored.
fn parse_way_embed_version(out: &str) -> Option<(u32, u32, u32)> {
    let v = out.trim().strip_prefix("way-embed ")?;
    let v = v.split(['-', '+']).next()?;
    let mut parts = v.split('.').map(|p| p.trim().parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// Build the body sidecar (ADR-701 §6) for every way in `sources`, disabled or
/// not: split each way into sections, embed them all in one `way-embed
/// generate` pass with the English model, and write the binary sidecar into
/// `out_dir`. Returns (ways, section vectors), or why it could not be built.
fn build_body_sidecar(
    out_dir: &Path,
    engine_dir: &Path,
    bin: &Path,
    sources: &HashMap<String, PathBuf>,
    generate: &GeneratePass<'_>,
) -> std::result::Result<(usize, usize), String> {
    use crate::cmd::scan::sidecar;
    let model = engine_dir.join(crate::paths::EN_MODEL);
    let model_id = sidecar::model_id(engine_dir, bin).ok_or("English model or way-embed missing")?;

    let mut ids: Vec<&String> = sources.keys().collect();
    ids.sort();
    // (id, hash, sections) per way, read once so hash and sections agree.
    let mut ways = Vec::with_capacity(ids.len());
    for id in ids {
        let Ok(bytes) = std::fs::read(&sources[id]) else { continue };
        let sections = crate::cmd::scan::chunk_sections(&String::from_utf8_lossy(&bytes));
        ways.push((id.clone(), sidecar::content_hash(&bytes), sections));
    }

    // One generate pass over every section, rewritten in place with vectors.
    // Rows are keyed `{way index}#{n}`, so no way id needs escaping. generate
    // embeds `description + " " + vocabulary`, the text the measurement used.
    let staged = out_dir.join(format!("ways-corpus-sections.{}.tmp", std::process::id()));
    let result = embed_sections(&ways, &staged, bin, &model, generate).and_then(|(dim, mut vectors)| {
        let mut records = Vec::with_capacity(ways.len());
        for (wi, (id, hash, sections)) in ways.iter().enumerate() {
            let vs = (0..sections.len())
                .map(|n| vectors.remove(&format!("{wi}#{n}")))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| format!("way-embed returned no vector for a section of {id}"))?;
            records.push(sidecar::WaySections { id: id.clone(), hash: *hash, vectors: vs });
        }
        let bytes = sidecar::encode(&model_id, dim, &records).ok_or("section vectors of unequal length")?;
        sidecar::write(&out_dir.join(sidecar::FILE), &bytes).map_err(|e| e.to_string())?;
        Ok((records.len(), records.iter().map(|r| r.vectors.len()).sum()))
    });
    let _ = std::fs::remove_file(&staged);
    result
}

/// Embed every section of `ways` through `staged`, returning the dimension and
/// the vectors keyed `{way index}#{n}`. No sections, no pass.
fn embed_sections(
    ways: &[(String, u64, Vec<String>)],
    staged: &Path,
    bin: &Path,
    model: &Path,
    generate: &GeneratePass<'_>,
) -> std::result::Result<(usize, HashMap<String, Vec<f32>>), String> {
    let mut raw = String::new();
    for (wi, (_, _, sections)) in ways.iter().enumerate() {
        for (n, text) in sections.iter().enumerate() {
            raw.push_str(&json!({ "id": format!("{wi}#{n}"), "description": text, "vocabulary": "" }).to_string());
            raw.push('\n');
        }
    }
    let mut vectors = HashMap::new();
    if raw.is_empty() {
        return Ok((0, vectors));
    }
    std::fs::write(staged, raw).map_err(|e| format!("writing {}: {e}", staged.display()))?;
    generate(bin, staged, model, "body sections")?;
    let embedded = std::fs::read_to_string(staged).map_err(|e| e.to_string())?;
    for line in embedded.lines().filter(|l| !l.is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let id = row.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let v: Vec<f32> = row
            .get("embedding")
            .and_then(|e| e.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_f64()).map(|f| f as f32).collect())
            .unwrap_or_default();
        if !v.is_empty() {
            vectors.insert(id, v);
        }
    }
    let dim = vectors.values().next().map_or(0, Vec::len);
    Ok((dim, vectors))
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

/// Remove staging files a killed build left behind: `*.tmp` from this module,
/// the dot-prefixed ones `write_atomic` stages the sidecar and manifest
/// through, and `*.tmp.tmp` from `way-embed generate`. Only files older than an hour
/// go, so a build running concurrently keeps its own.
fn sweep_staging_debris(out_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(out_dir) else { return };
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // write_atomic stages as `.<name>.<pid>.<n>.tmp`.
        let bare = name.strip_prefix('.').unwrap_or(&name);
        let ours = bare.starts_with("ways-corpus")
            || bare.starts_with(crate::cmd::scan::sidecar::FILE)
            || bare.starts_with("embed-manifest.json");
        if !(ours && name.contains(".tmp")) {
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
    let embedded = m.get("embedded").and_then(|v| v.as_bool()).unwrap_or(true);
    let side = m.get("body_sidecar").cloned().unwrap_or(serde_json::Value::Null);
    let side_failed = sidecar_failed(&side);
    // A way-embed without --vectors is an install state: only a new engine
    // can change the answer, so it does not retry by the day.
    let side_unsupported = side.get("unsupported").and_then(|v| v.as_bool()).unwrap_or(false);
    if embedded && !side_failed && !side_unsupported {
        return false;
    }
    let engine_changed = m.get("engine").and_then(|v| v.as_str()) != Some(engine);
    let failed_at = m.get("failed_at").and_then(|v| v.as_u64()).unwrap_or(0);
    let day_passed = (!embedded || side_failed) && now.saturating_sub(failed_at) >= RETRY_AFTER_SECS;
    engine_changed || day_passed
}

/// True when the manifest's `body_sidecar` records a build that failed, as
/// opposed to one skipped because way-embed lacks `--vectors`.
fn sidecar_failed(side: &serde_json::Value) -> bool {
    side.get("reason").is_some_and(|r| r.is_string())
        && !side.get("unsupported").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Embed one project's `.claude/ways/` under namespace `key`.
///
/// Honors the `.ways-embed` marker (skips on `disinclude`), namespaces every
/// way id as `{key}/{bare_id}`, and records the project in the manifest under
/// `key`. Each way written is added to `sources`. Returns the number of ways
/// embedded.
#[allow(clippy::too_many_arguments)]
fn embed_one_project(
    ways_path: &Path,
    key: &str,
    project_path: &str,
    excluded: &[String],
    w: &mut impl Write,
    manifest_projects: &mut HashMap<String, serde_json::Value>,
    sources: &mut HashMap<String, PathBuf>,
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
    // pass a fresh skip set.
    let skip = std::collections::HashSet::new();
    let local_count = scan_ways_dir(ways_path, &prefix, excluded, w, &skip, sources)?;

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
/// written is recorded in `written` with the way's own file (not a `.lang.md`
/// override), which the body sidecar embeds.
fn scan_ways_dir(
    dir: &Path,
    id_prefix: &str,
    excluded: &[String],
    w: &mut impl Write,
    skip: &std::collections::HashSet<String>,
    written: &mut HashMap<String, PathBuf>,
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
            // ": ") would vanish from matching with no signal. `ways author lint` is the hard
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
        // stderr-only — `ways author lint` is the hard gate and escalates.
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
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if crate::util::extract_locale_from_filename(fname).is_some() {
            written.entry(id.clone()).or_insert_with(|| path.clone());
        } else {
            written.insert(id.clone(), path.clone());
        }

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

    /// ADR-701 §6: the sidecar holds a record for every corpus way at its file's
    /// hash, the vectors `generate` produced for its sections in order, and no
    /// vectors for a way with no sections.
    #[test]
    fn body_sidecar_build_writes_every_way_at_its_hash() {
        use crate::cmd::scan::sidecar;
        let dir = scratch("sidecar-build");
        let engine = dir.join("engine");
        std::fs::create_dir_all(&engine).unwrap();
        std::fs::write(engine.join(crate::paths::EN_MODEL), "model").unwrap();
        let prose = "---\ndescription: d\nvocabulary: v\n---\n# One\n\nThe first section has enough words in it.\n\n## Two\n\nThe second section also has enough words.\n";
        let table = "---\ndescription: d\nvocabulary: v\n---\n# Policy\n\n| a | b |\n";
        std::fs::write(dir.join("prose.md"), prose).unwrap();
        std::fs::write(dir.join("table.md"), table).unwrap();
        let sources: HashMap<String, PathBuf> = [("a/prose", "prose.md"), ("b/table", "table.md")]
            .iter()
            .map(|(id, f)| (id.to_string(), dir.join(f)))
            .collect();

        // A fake generate: embed row k as the unit vector on axis k of 3, the
        // way way-embed rewrites the corpus in place.
        let seen = RefCell::new(Vec::new());
        let generate = |_: &Path, corpus: &Path, _: &Path, _: &str| {
            let rows: Vec<String> = read(corpus)
                .lines()
                .enumerate()
                .map(|(k, l)| {
                    let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                    seen.borrow_mut().push(v["description"].as_str().unwrap().to_string());
                    let mut e = vec![0.0; 3];
                    e[k % 3] = 1.0;
                    v["embedding"] = json!(e);
                    v.to_string()
                })
                .collect();
            std::fs::write(corpus, rows.join("\n") + "\n").unwrap();
            Ok(Instant::now())
        };
        let bin = engine.join("way-embed");
        std::fs::write(&bin, "binary").unwrap();
        let model_id = sidecar::model_id(&engine, &bin).unwrap();
        let got = build_body_sidecar(&dir, &engine, &bin, &sources, &generate).unwrap();
        assert_eq!(got, (2, 2));
        assert_eq!(
            seen.into_inner(),
            vec!["One. The first section has enough words in it.", "Two. The second section also has enough words."]
        );

        let sc = sidecar::read(&dir.join(sidecar::FILE)).unwrap();
        assert_eq!(sc.model, model_id);
        assert_eq!(sc.dim, 3);
        assert!((sc.max_cosine("a/prose", &[0.0, 1.0, 0.0]).unwrap() - 1.0).abs() < 1e-9);
        assert!((sc.max_cosine("a/prose", &[0.0, 0.0, 1.0]).unwrap()).abs() < 1e-9, "only two sections");
        assert!(sc.max_cosine("b/table", &[1.0, 0.0, 0.0]).is_none(), "a sectionless way has no vectors");

        let hashes = way_hashes(&sources);
        assert_eq!(hashes["a/prose"], json!(sidecar::hash_hex(sidecar::content_hash(prose.as_bytes()))));
        let alias = sidecar::alias_hashes_from(&json!({ "way_hashes": hashes }));
        assert!(sc.check(&model_id, ["a/prose", "b/table"], &alias).is_ok());
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "staging left behind: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn way_embed_versions_parse_and_gate_vectors() {
        assert_eq!(parse_way_embed_version("way-embed 1.2.0\n"), Some((1, 2, 0)));
        assert_eq!(parse_way_embed_version("way-embed 1.10.3"), Some((1, 10, 3)));
        assert_eq!(parse_way_embed_version("way-embed 0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_way_embed_version("error: unknown"), None);
        assert!(parse_way_embed_version("way-embed 1.1.2").unwrap() < (1, 2, 0));
        assert!(parse_way_embed_version("way-embed 1.10.0").unwrap() >= (1, 2, 0));
        assert_eq!(parse_way_embed_version("way-embed 1.2.0-rc1\n"), Some((1, 2, 0)));
        assert_eq!(parse_way_embed_version("way-embed 1.3.0+build5"), Some((1, 3, 0)));
        assert_eq!(parse_way_embed_version("way-embed 1.2.0-rc1+b2"), Some((1, 2, 0)));
    }

    /// Only a version read and found below 1.2.0 is the install state that
    /// `--quiet` hides; a binary that cannot say its version is a failure.
    #[test]
    fn only_a_parsed_old_version_is_unsupported() {
        assert_eq!(classify_version("way-embed 1.1.2\n"), VectorSupport::Unsupported);
        assert_eq!(classify_version("way-embed 1.2.0"), VectorSupport::Yes);
        assert_eq!(classify_version("way-embed 1.2.0-rc1"), VectorSupport::Yes);
        assert_eq!(classify_version("way-embed 2.0.0+abc"), VectorSupport::Yes);
        assert!(matches!(classify_version(""), VectorSupport::Unknown(_)));
        assert!(matches!(classify_version("Segmentation fault"), VectorSupport::Unknown(_)));
        assert!(matches!(classify_version("way-embed dev"), VectorSupport::Unknown(_)));
    }

    /// A busy binary is tried again a bounded number of times.
    #[cfg(unix)]
    #[test]
    fn a_busy_binary_is_retried_a_few_times() {
        let busy = || std::io::Error::from_raw_os_error(26);
        let mut n = 0;
        let r = retry_if_busy(|| {
            n += 1;
            if n < 3 { Err(busy()) } else { Ok(n) }
        });
        assert_eq!(r.unwrap(), 3);
        let mut n = 0;
        let r: std::io::Result<()> = retry_if_busy(|| {
            n += 1;
            Err(busy())
        });
        assert!(r.is_err());
        assert_eq!(n, 5, "unbounded or no retry");
        let mut n = 0;
        let r: std::io::Result<()> = retry_if_busy(|| {
            n += 1;
            Err(std::io::ErrorKind::NotFound.into())
        });
        assert!(r.is_err());
        assert_eq!(n, 1, "retried an error that is not busy");
    }

    /// What `--version` of a real executable decides.
    #[cfg(unix)]
    #[test]
    fn the_version_probe_reads_a_stub_way_embed() {
        let dir = scratch("vector-support");
        let stub = |name: &str, body: &str| {
            let p = dir.join(name);
            std::fs::write(&p, body).unwrap();
            std::fs::set_permissions(&p, <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755)).unwrap();
            p
        };
        assert_eq!(vector_support(&stub("old", "#!/bin/sh\necho 'way-embed 1.1.2'\n")), VectorSupport::Unsupported);
        assert_eq!(vector_support(&stub("new", "#!/bin/sh\necho 'way-embed 1.2.0-rc1'\n")), VectorSupport::Yes);
        assert!(matches!(vector_support(&stub("crash", "#!/bin/sh\necho 'way-embed 1.1.2'; exit 3\n")), VectorSupport::Unknown(_)));
        assert!(matches!(vector_support(&stub("junk", "#!/bin/sh\necho nonsense\n")), VectorSupport::Unknown(_)));
        assert!(matches!(vector_support(&dir.join("absent")), VectorSupport::Unknown(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A rebuild removes the previous sidecar first: a failed or skipped build
    /// leaves no sidecar describing an older corpus, and records why.
    #[test]
    fn a_failed_or_skipped_sidecar_build_leaves_no_old_sidecar() {
        use crate::cmd::scan::sidecar;
        let dir = scratch("sidecar-refresh");
        let engine = dir.join("engine");
        std::fs::create_dir_all(&engine).unwrap();
        std::fs::write(engine.join(crate::paths::EN_MODEL), "model").unwrap();
        let bin = engine.join("way-embed");
        std::fs::write(&bin, "binary").unwrap();
        std::fs::write(dir.join("w.md"), "---\ndescription: d\nvocabulary: v\n---\n# T\n\nA section long enough to embed here.\n").unwrap();
        let sources: HashMap<String, PathBuf> = [("w".to_string(), dir.join("w.md"))].into_iter().collect();
        let failing = |_: &Path, _: &Path, _: &Path, _: &str| Err("body sections embedding generation failed".to_string());
        let old = dir.join(sidecar::FILE);

        std::fs::write(&old, "previous sidecar").unwrap();
        let side = refresh_body_sidecar(&dir, &engine, &bin, &sources, &failing, VectorSupport::Yes);
        assert!(!old.exists(), "old sidecar left after a failed build");
        assert_eq!(side["reason"], json!("body sections embedding generation failed"));
        assert!(sidecar_failed(&side));

        std::fs::write(&old, "previous sidecar").unwrap();
        let side = refresh_body_sidecar(&dir, &engine, &bin, &sources, &failing, VectorSupport::Unsupported);
        assert!(!old.exists(), "old sidecar left when way-embed lacks --vectors");
        assert_eq!(side, json!({ "file": null, "reason": NO_VECTORS, "unsupported": true }));
        assert!(!sidecar_failed(&side));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A reinstalled or upgraded way-embed invalidates the sidecar. The stale
    /// check sees the engine fingerprint differ from the one recorded at build
    /// and rebuilds only the sidecar, after which a scan uses it again.
    #[test]
    fn a_changed_way_embed_rebuilds_the_sidecar_on_the_stale_check() {
        use crate::cmd::scan::sidecar;
        let dir = scratch("sidecar-engine");
        let engine = dir.join("engine");
        std::fs::create_dir_all(&engine).unwrap();
        std::fs::write(engine.join(crate::paths::EN_MODEL), "model").unwrap();
        let bin = engine.join("way-embed");
        std::fs::write(&bin, "binary").unwrap();
        let day_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400);
        std::fs::File::options().append(true).open(&bin).unwrap().set_modified(day_ago).unwrap();
        let way = dir.join("w.md");
        std::fs::write(&way, "---\ndescription: d\nvocabulary: v\n---\n# T\n\nA section long enough to embed here.\n").unwrap();
        let sources: HashMap<String, PathBuf> = [("w".to_string(), way)].into_iter().collect();
        let calls = RefCell::new(0);
        let generate = |_: &Path, corpus: &Path, _: &Path, _: &str| {
            *calls.borrow_mut() += 1;
            let rows: Vec<String> = read(corpus)
                .lines()
                .map(|l| {
                    let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                    v["embedding"] = json!([1.0, 0.0]);
                    v.to_string()
                })
                .collect();
            std::fs::write(corpus, rows.join("\n") + "\n").unwrap();
            Ok(Instant::now())
        };

        // A recorded build: alias hashes and a sidecar at this engine.
        let manifest = dir.join("embed-manifest.json");
        let side = refresh_body_sidecar(&dir, &engine, &bin, &sources, &generate, VectorSupport::Yes);
        std::fs::write(&manifest, json!({ "embedded": true, "way_hashes": way_hashes(&sources), "body_sidecar": side }).to_string()).unwrap();
        // The corpus dir is both the engine dir and the output dir in production.
        let state = || sidecar::state(&dir, &bin, ["w"]).map(|s| (s.way_count(), s.vector_count()));
        std::fs::copy(engine.join(crate::paths::EN_MODEL), dir.join(crate::paths::EN_MODEL)).unwrap();
        let state_ok = |s: &Result<(usize, usize), sidecar::Fallback>| s.is_ok();
        assert!(!sidecar_engine_stale(&manifest, &engine, &bin), "fresh build counted stale");
        assert_eq!(*calls.borrow(), 1);

        // way-embed reinstalled: same bytes, new mtime.
        std::fs::File::options().append(true).open(&bin).unwrap().set_modified(std::time::SystemTime::now()).unwrap();
        assert!(sidecar_engine_stale(&manifest, &engine, &bin), "changed engine not seen");

        assert!(refresh_stale_sidecar(&dir, &engine, &bin, &sources, &generate, VectorSupport::Yes, &Report { quiet: false, emit: &|_| {} }).unwrap());
        assert_eq!(*calls.borrow(), 2, "sidecar not rebuilt");
        assert!(!sidecar_engine_stale(&manifest, &engine, &bin));
        let s = state();
        assert!(state_ok(&s), "{s:?}");
        assert_eq!(crate::cmd::status::sidecar_line(&s), "Body sidecar: in use (1 ways, 1 sections)");

        // Ways changed under an unchanged mtime check: not a sidecar-only job.
        std::fs::write(dir.join("w.md"), "---\ndescription: d\nvocabulary: v\n---\n# T\n\nEdited text of the way body here.\n").unwrap();
        std::fs::File::options().append(true).open(&bin).unwrap().set_modified(day_ago).unwrap();
        assert!(!refresh_stale_sidecar(&dir, &engine, &bin, &sources, &generate, VectorSupport::Yes, &Report { quiet: false, emit: &|_| {} }).unwrap(), "rebuilt a sidecar for a stale corpus");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--if-stale` retries a failed sidecar build like a failed corpus build;
    /// a way-embed without --vectors waits for a new engine, not a new day.
    #[test]
    fn a_failed_sidecar_build_is_retried() {
        let dir = scratch("stale-sidecar");
        let manifest = dir.join("embed-manifest.json");
        let t0 = 1_000_000u64;
        std::fs::write(
            &manifest,
            format!(r#"{{"embedded": true, "engine": "A", "failed_at": {t0}, "body_sidecar": {{"file": null, "reason": "boom"}}}}"#),
        )
        .unwrap();
        assert!(!retry_due(&manifest, "A", t0 + 60));
        assert!(retry_due(&manifest, "B", t0 + 60), "changed engine");
        assert!(retry_due(&manifest, "A", t0 + RETRY_AFTER_SECS), "a day later");
        std::fs::write(
            &manifest,
            format!(r#"{{"embedded": true, "engine": "A", "body_sidecar": {{"file": null, "reason": "{NO_VECTORS}", "unsupported": true}}}}"#),
        )
        .unwrap();
        assert!(!retry_due(&manifest, "A", t0 + RETRY_AFTER_SECS), "unsupported retried by the day");
        assert!(retry_due(&manifest, "B", t0), "unsupported not retried on a new engine");
        std::fs::write(&manifest, r#"{"embedded": true, "engine": "A", "body_sidecar": {"file": "ways-body-en.bin", "vectors": true}}"#).unwrap();
        assert!(!retry_due(&manifest, "B", t0 + RETRY_AFTER_SECS), "a built sidecar retried");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Staging debris from the sidecar build is swept like the corpus's own.
    #[test]
    fn stale_sidecar_staging_files_are_swept() {
        let dir = scratch("sweep-sidecar");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        // The sidecar and manifest stage through write_atomic: `.<name>.<pid>.<n>.tmp`.
        let names = [
            "ways-corpus-sections.123.tmp",
            "ways-corpus-sections.123.tmp.tmp",
            ".ways-body-en.bin.123.0.tmp",
            ".embed-manifest.json.123.1.tmp",
        ];
        for n in names {
            let p = dir.join(n);
            std::fs::write(&p, "x").unwrap();
            std::fs::File::options().append(true).open(&p).unwrap().set_modified(old).unwrap();
        }
        std::fs::write(dir.join(".ways-body-en.bin.456.0.tmp"), "fresh").unwrap();
        std::fs::write(dir.join("ways-body-en.bin"), "live").unwrap();
        std::fs::write(dir.join("embed-manifest.json"), "live").unwrap();
        sweep_staging_debris(&dir);
        for n in names {
            assert!(!dir.join(n).exists(), "{n} not swept");
        }
        assert!(dir.join(".ways-body-en.bin.456.0.tmp").exists(), "a fresh staging file swept");
        assert!(dir.join("ways-body-en.bin").exists(), "the live sidecar swept");
        assert!(dir.join("embed-manifest.json").exists(), "the live manifest swept");
        let _ = std::fs::remove_dir_all(&dir);
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

    fn unsupported() -> serde_json::Value {
        json!({ "file": null, "reason": NO_VECTORS, "unsupported": true })
    }

    #[test]
    fn a_way_embed_without_vectors_is_silent_under_quiet() {
        assert_eq!(sidecar_warning(&unsupported(), true), None);
    }

    #[test]
    fn a_way_embed_without_vectors_still_warns_without_quiet() {
        let w = sidecar_warning(&unsupported(), false).unwrap();
        assert!(w.starts_with("warning: body sidecar not built: way-embed < 1.2.0"), "{w}");
    }

    #[test]
    fn a_real_failure_warns_even_under_quiet() {
        let side = json!({ "file": null, "reason": "body sections embedding generation failed" });
        let w = sidecar_warning(&side, true).unwrap();
        assert!(w.contains("body sections embedding generation failed"), "{w}");
        assert_eq!(sidecar_warning(&json!({ "file": "x", "vectors": true }), true), None);
    }

    #[test]
    fn the_manifest_is_complete_after_write_and_leaves_no_staging_file() {
        let dir = scratch("manifest-atomic");
        let path = dir.join("embed-manifest.json");
        write_manifest(&path, &json!({ "v": 1 })).unwrap();
        write_manifest(&path, &json!({ "v": 2, "pad": "x".repeat(10_000) })).unwrap();
        let back: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();
        assert_eq!(back["v"], json!(2));
        let names: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["embed-manifest.json".to_string()], "staging file left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A write that cannot stage leaves the old manifest as it was.
    #[cfg(unix)]
    #[test]
    fn a_failed_manifest_write_leaves_the_old_manifest_intact() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("manifest-fail");
        let path = dir.join("embed-manifest.json");
        write_manifest(&path, &json!({ "v": 1 })).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let res = write_manifest(&path, &json!({ "v": 2 }));
        let can_stage = std::fs::File::create(dir.join("probe")).is_ok();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        if can_stage {
            eprintln!("SKIPPED a_failed_manifest_write_leaves_the_old_manifest_intact: directory modes do not bind this user (root?)");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        assert!(res.is_err(), "the manifest was rewritten in place");
        let back: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();
        assert_eq!(back["v"], json!(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A manifest replaced through write_manifest is a new file: a symlink at
    /// the path is replaced, and what it pointed at is untouched.
    #[cfg(unix)]
    #[test]
    fn write_manifest_replaces_the_path_instead_of_writing_through_it() {
        let dir = scratch("manifest-link");
        let target = dir.join("target.json");
        std::fs::write(&target, "old").unwrap();
        let path = dir.join("embed-manifest.json");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        write_manifest(&path, &json!({ "v": 1 })).unwrap();
        assert_eq!(read(&target), "old");
        assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Fixture for the sidecar-only path: a manifest whose way hashes match
    /// `sources`, and a way-embed binary.
    fn sidecar_only_fixture(name: &str) -> (PathBuf, PathBuf, PathBuf, HashMap<String, PathBuf>) {
        let dir = scratch(name);
        let engine = dir.join("engine");
        std::fs::create_dir_all(&engine).unwrap();
        std::fs::write(engine.join(crate::paths::EN_MODEL), "model").unwrap();
        let bin = engine.join("way-embed");
        std::fs::write(&bin, "binary").unwrap();
        let way = dir.join("w.md");
        std::fs::write(&way, "---\ndescription: d\nvocabulary: v\n---\n# T\n\nA section long enough to embed here.\n").unwrap();
        let sources: HashMap<String, PathBuf> = [("w".to_string(), way)].into_iter().collect();
        let manifest = json!({ "way_hashes": way_hashes(&sources), "embedded": true, "body_sidecar": { "file": "ways-body-en.bin", "model_id": "old" } });
        std::fs::write(dir.join("embed-manifest.json"), manifest.to_string()).unwrap();
        (dir, engine, bin, sources)
    }

    fn embedding_ok(_: &Path, corpus: &Path, _: &Path, _: &str) -> std::result::Result<Instant, String> {
        let rows: Vec<String> = read(corpus)
            .lines()
            .map(|l| {
                let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                v["embedding"] = json!([1.0, 0.0]);
                v.to_string()
            })
            .collect();
        std::fs::write(corpus, rows.join("\n") + "\n").unwrap();
        Ok(Instant::now())
    }

    /// What the sidecar-only path says, for a vector-support answer and a
    /// generate pass.
    fn sidecar_only_says(name: &str, quiet: bool, vectors: VectorSupport, generate: &GeneratePass<'_>) -> Vec<String> {
        let (dir, engine, bin, sources) = sidecar_only_fixture(name);
        let said = RefCell::new(Vec::new());
        let report = Report { quiet, emit: &|m| said.borrow_mut().push(m.to_string()) };
        assert!(refresh_stale_sidecar(&dir, &engine, &bin, &sources, generate, vectors, &report).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
        said.into_inner()
    }

    #[test]
    fn the_sidecar_only_path_is_silent_under_quiet_for_an_old_way_embed() {
        assert!(sidecar_only_says("so-quiet", true, VectorSupport::Unsupported, &embedding_ok).is_empty());
    }

    /// The warning, and no "rebuilt" claim for a sidecar that was just removed.
    #[test]
    fn the_sidecar_only_path_warns_once_and_claims_no_rebuild_when_none_was_built() {
        let said = sidecar_only_says("so-loud", false, VectorSupport::Unsupported, &embedding_ok);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].starts_with("warning: body sidecar not built"), "{said:?}");
        let said = sidecar_only_says("so-fail", false, VectorSupport::Yes, &|_, _, _, _| Err("boom".to_string()));
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("boom") && !said[0].contains("rebuilt"), "{said:?}");
    }

    #[test]
    fn the_sidecar_only_path_warns_under_quiet_for_a_real_failure() {
        let said = sidecar_only_says("so-real", true, VectorSupport::Unknown("way-embed --version failed".into()), &embedding_ok);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("way-embed --version failed"), "{said:?}");
        let said = sidecar_only_says("so-boom", true, VectorSupport::Yes, &|_, _, _, _| Err("boom".to_string()));
        assert_eq!(said.len(), 1, "{said:?}");
    }

    #[test]
    fn the_sidecar_only_path_says_rebuilt_when_it_wrote_the_sidecar() {
        let said = sidecar_only_says("so-built", false, VectorSupport::Yes, &embedding_ok);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].starts_with("Body sidecar rebuilt for the current engine: "), "{said:?}");
        assert!(said[0].ends_with(crate::cmd::scan::sidecar::FILE), "{said:?}");
        assert!(sidecar_only_says("so-built-quiet", true, VectorSupport::Yes, &embedding_ok).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_sidecar_only_path_replaces_the_manifest_instead_of_writing_through_it() {
        let (dir, engine, bin, sources) = sidecar_only_fixture("so-atomic");
        let manifest = dir.join("embed-manifest.json");
        let target = dir.join("target.json");
        std::fs::rename(&manifest, &target).unwrap();
        std::os::unix::fs::symlink(&target, &manifest).unwrap();
        let before = read(&target);
        let report = Report { quiet: true, emit: &|_| {} };
        assert!(refresh_stale_sidecar(&dir, &engine, &bin, &sources, &embedding_ok, VectorSupport::Yes, &report).unwrap());
        assert_eq!(read(&target), before, "the manifest was written through its link");
        assert!(std::fs::symlink_metadata(&manifest).unwrap().file_type().is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A read-only output directory fails the sidecar-only refresh and leaves
    /// the old manifest as it was.
    #[cfg(unix)]
    #[test]
    fn a_failed_sidecar_only_manifest_write_leaves_the_old_manifest_intact() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, engine, bin, sources) = sidecar_only_fixture("so-readonly");
        let manifest = dir.join("embed-manifest.json");
        let before = read(&manifest);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let can_stage = std::fs::File::create(dir.join("probe")).is_ok();
        let report = Report { quiet: true, emit: &|_| {} };
        let res = refresh_stale_sidecar(&dir, &engine, &bin, &sources, &embedding_ok, VectorSupport::Unsupported, &report);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        if can_stage {
            eprintln!("SKIPPED a_failed_sidecar_only_manifest_write_leaves_the_old_manifest_intact: directory modes do not bind this user (root?)");
        } else {
            assert!(res.is_err(), "the manifest was rewritten in place");
            assert_eq!(read(&manifest), before);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The full path's own line for a built sidecar.
    #[test]
    fn the_full_path_names_a_built_sidecar_and_is_quiet_under_quiet() {
        let side = json!({ "file": "ways-body-en.bin", "ways": 3, "sections": 9, "vectors": true });
        let said = RefCell::new(Vec::new());
        Report { quiet: false, emit: &|m| said.borrow_mut().push(m.to_string()) }.sidecar(&side, Path::new("/o"), false);
        assert_eq!(said.take(), vec!["Body sidecar: /o/ways-body-en.bin (3 ways, 9 sections)".to_string()]);
        Report { quiet: true, emit: &|m| said.borrow_mut().push(m.to_string()) }.sidecar(&side, Path::new("/o"), false);
        assert!(said.take().is_empty());
    }
}
