//! scan/late_interaction.rs — ADR-160 chunked late-interaction matcher.
//!
//! An alternative to the single-vector semantic gate (scoring.rs). Instead of
//! embedding one reduced surface and thresholding each way's calibrated cosine,
//! the matcher:
//!
//!   2. **chunks** the surface into sentences and embeds each chunk (one batched
//!      `way-embed match` call — the ADR-160 prerequisite primitive);
//!   3. **ranks** each way by the *peak* of its per-chunk cosines (specificity —
//!      a way matches on its best-matching chunk, not a diluted whole-surface
//!      average);
//!   4. **gates** by per-chunk *softmax-share*: within each chunk the top ways
//!      compete in a zero-sum softmax (which defeats the anisotropic cosine floor
//!      — a way must *win* a chunk, not merely clear an absolute score), and a
//!      way's share is summed across chunks;
//!   5. **confirms** each survivor by cross-similarity of the chunk it *won*
//!      (its peak chunk) against the winning way's body chunks — corroborating
//!      the evidence that fired the way, which rejects single-token collisions
//!      (a collided chunk finds no support in the way's own body) and covers the
//!      softmax's blind spot (it always hands the winner mass, even when nothing
//!      is truly relevant). Confirming the *winning* chunk rather than every
//!      surface chunk avoids diluting a way that legitimately matched only part
//!      of a multi-topic surface.
//!
//! Peak/share are lenient (specificity + competition); the body-confirm is the
//! strict corroboration — the two take opposite stances on purpose (ADR-160).
//!
//! **Body sidecar (ADR-701 §6, §7).** When the corpus build's section sidecar
//! covers every enabled way the alias corpus holds at its content hashes, the match
//! pass also returns each chunk's vector (`way-embed match --vectors`) and
//! confirmation is the max cosine of the won chunk against the way's section
//! vectors, or its alias vector for a way with no sections. No further
//! process runs. Otherwise (no sidecar, an incomplete or stale one, or a
//! way-embed without `--vectors`) confirmation embeds the body per call, as
//! before.
//!
//! **Operating points are HAND-SET and UNCALIBRATED.** ADR-160 stays Proposed
//! until they are fit against a precision metric (task #5). It is the semantic
//! matcher (not opt-in); until calibration lands it stays on its branch.
//!
//! **Fail-safe.** Every stage returns `None` when it cannot run (no binary /
//! corpus / model, a surface too sparse to chunk), and the caller falls back to
//! the single-vector path. The matcher never partially decides.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::reduce::split_sentences;
use super::sidecar::{self, Sidecar};

// ── Hand-set operating points (uncalibrated — task #5 fits these) ──
/// Per-chunk softmax temperature. Small τ sharpens the competition so a clear
/// per-chunk winner takes most of the mass.
pub(super) const SOFTMAX_TAU: f64 = 0.08;
/// Ways entering each chunk's softmax (the rest score ~0 mass anyway).
pub(super) const TOP_K_PER_CHUNK: usize = 8;
/// Summed-share / n_chunks that admits a way into confirmation.
const SHARE_GATE: f64 = 0.15;
/// Peak per-chunk cosine that, on its own, admits a way into confirmation even
/// when its share is diluted (the peak co-gate). On a topic-diverse surface
/// `share = Σmass / n_chunks` caps a way that owns one of N topics at ≈1/N, so a
/// specific, decisive single-chunk match never clears the share gate; admitting on
/// a strong peak and letting the (strict) body-confirm carry precision recovers
/// that case. Set high enough that only a decisive chunk win qualifies.
const PEAK_GATE: f64 = 0.50;
/// Body cross-similarity (winning chunk vs body chunks, max) an admitted way must
/// reach to actually fire.
const CONFIRM_GATE: f64 = 0.35;
/// Caps — keep the batched embedding bounded on a pathological surface/body.
const MAX_SURFACE_CHUNKS: usize = 12;
const MAX_BODY_CHUNKS: usize = 8;
const MIN_CHUNK_CHARS: usize = 12;
/// Only the strongest ranking survivors are worth a body-confirm pass.
const MAX_WINNERS_TO_CONFIRM: usize = 6;

/// The matcher's decision for a scan: the ways it fired, keyed by corpus id, with
/// a representative score (the summed softmax-share) for telemetry.
pub(crate) struct Verdicts {
    fired: HashMap<String, f64>,
    /// Confirmation read the body sidecar (ADR-701 §7's sidecar state).
    sidecar: bool,
}

/// One way's ranking evidence from the chunk-match stage.
struct Ranked {
    id: String,
    /// Max cosine over chunks (specificity — the way's strongest single match).
    peak: f64,
    /// Index of the chunk the way peaked on — its strongest evidence, which the
    /// confirmation stage corroborates against the body.
    peak_chunk: usize,
    /// Summed per-chunk softmax mass / n_chunks (competition / breadth).
    share: f64,
}

impl Verdicts {
    /// The share score if the matcher fired `corpus_id`, else `None`.
    pub(crate) fn fired_score(&self, corpus_id: &str) -> Option<f64> {
        self.fired.get(corpus_id).copied()
    }

    /// True when this scan confirmed against the body sidecar.
    pub(crate) fn used_sidecar(&self) -> bool {
        self.sidecar
    }
}

// ── Authoring diagnostic (task #5 — the late-interaction `ways author match`) ──
// The gates a way must clear to fire, exposed so the diagnostic can annotate
// each candidate's outcome the way an author needs to read it.
pub(crate) const DIAG_SHARE_GATE: f64 = SHARE_GATE;
pub(crate) const DIAG_PEAK_GATE: f64 = PEAK_GATE;
pub(crate) const DIAG_CONFIRM_GATE: f64 = CONFIRM_GATE;

/// One candidate's full late-interaction evidence, for the authoring view. Unlike
/// [`Verdicts`] (fired-only, share-only), this carries the peak, the winning chunk,
/// and the body-confirm — everything an author needs to see *why* a way fired or
/// fell short of a gate.
pub(crate) struct DiagRow {
    pub id: String,
    /// Max per-chunk cosine (specificity — the way's strongest single match).
    pub peak: f64,
    /// Summed softmax-share / n_chunks (the ranking + share-gate quantity).
    pub share: f64,
    /// The surface chunk the way peaked on — the evidence that would fire it.
    pub won_chunk: String,
    /// Body cross-similarity of the won chunk against the way's body. `None` when
    /// the way was admitted by neither gate (confirmation is not run for it).
    pub confirm: Option<f64>,
    /// Admitted into confirmation — cleared the share gate or the peak co-gate.
    pub admitted: bool,
    /// Cleared admission AND body-confirm — would fire in production.
    pub fired: bool,
}

/// Run the matcher in diagnostic mode: the same chunk → share → body-confirm
/// pipeline as [`run`], but returning the top `top_n` candidates by share with
/// their full evidence (including those that failed a gate, so an author can see
/// how close a way came). Returns `None` on the same fail-safe conditions as
/// [`run`] — the caller then reports that late-interaction could not run and falls
/// back to the single-vector view, mirroring production.
pub(crate) fn run_diagnostic(
    surface: &str,
    bodies: &HashMap<String, PathBuf>,
    top_n: usize,
) -> Option<Vec<DiagRow>> {
    let bin = crate::paths::way_embed()?;
    let xdg = crate::paths::corpus_dir();
    let corpus = xdg.join("ways-corpus-en.jsonl");
    let model = xdg.join(crate::paths::EN_MODEL);
    if !corpus.is_file() || !model.is_file() {
        return None;
    }
    let chunks = chunk_surface(surface);
    if chunks.len() < 2 {
        return None;
    }
    let sidecar = complete_sidecar(&xdg, &bin, bodies);
    let matched = batch_match(&bin, &corpus, &model, &chunks, sidecar.is_some())?;
    let confirmer = Confirmer::new(&bin, &model, sidecar, matched.vectors);
    let per_chunk = mask_to_enabled(matched.per_chunk, bodies);
    let ranked = aggregate(&per_chunk, chunks.len());

    let mut rows = Vec::new();
    for r in ranked.into_iter().take(top_n) {
        let won_chunk = chunks.get(r.peak_chunk).cloned().unwrap_or_default();
        // Confirmation runs for any admitted candidate — share-gate OR peak co-gate —
        // matching the live matcher's survivor set; a candidate admitted by neither
        // reports `confirm: None`.
        let admitted = r.share >= SHARE_GATE || r.peak >= PEAK_GATE;
        let confirm = if admitted {
            bodies.get(&r.id).and_then(|path| confirmer.confirm(&r.id, r.peak_chunk, r.peak, &chunks, path))
        } else {
            None
        };
        let fired = confirm.is_some_and(|c| c >= CONFIRM_GATE);
        rows.push(DiagRow { id: r.id, peak: r.peak, share: r.share, won_chunk, confirm, admitted, fired });
    }
    Some(rows)
}

/// Run the matcher over `surface`. `bodies` maps a way's corpus id to its `.md`
/// path (for body-confirm). Returns `None` when the matcher cannot run — the
/// caller then falls back to the single-vector semantic gate.
pub(crate) fn run(surface: &str, bodies: &HashMap<String, PathBuf>) -> Option<Verdicts> {
    let dbg = std::env::var("WAYS_LI_DEBUG").is_ok();
    let bin = crate::paths::way_embed()?;
    let xdg = crate::paths::corpus_dir();
    let corpus = xdg.join("ways-corpus-en.jsonl");
    let model = xdg.join(crate::paths::EN_MODEL);
    if !corpus.is_file() || !model.is_file() {
        if dbg { eprintln!("LI: corpus/model missing → fallback"); }
        return None;
    }

    // Stage 2 (chunk). Too few chunks → no competition to run; fall back.
    let chunks = chunk_surface(surface);
    if dbg { eprintln!("LI: {} chunks: {:?}", chunks.len(), chunks); }
    if chunks.len() < 2 {
        if dbg { eprintln!("LI: <2 chunks → fallback"); }
        return None;
    }

    // Stage 2 (match): one batched pass, all chunks against the corpus. When
    // the sidecar is complete the pass also returns the chunks' vectors.
    let t = std::time::Instant::now();
    let sidecar = complete_sidecar(&xdg, &bin, bodies);
    if dbg { eprintln!("LI: body sidecar {} ({:.2} ms)", if sidecar.is_some() { "complete" } else { "absent or incomplete" }, ms(t)); }
    let t = std::time::Instant::now();
    let matched = batch_match(&bin, &corpus, &model, &chunks, sidecar.is_some())?;
    if dbg { eprintln!("LI: match pass {:.1} ms", ms(t)); }
    let confirmer = Confirmer::new(&bin, &model, sidecar, matched.vectors);
    if dbg { eprintln!("LI: confirm via {}", if confirmer.uses_sidecar() { "sidecar" } else { "per-call embedding" }); }
    let per_chunk = mask_to_enabled(matched.per_chunk, bodies);
    if dbg { eprintln!("LI: per_chunk rows: {:?}", per_chunk.iter().map(|c| c.len()).collect::<Vec<_>>()); }

    // Stages 3+4: peak rank + per-chunk softmax-share, sorted by share desc.
    let ranked = aggregate(&per_chunk, chunks.len());

    // Stage 4 gate: admit a way into confirmation on EITHER a sufficient
    // softmax-share OR a decisive peak (the peak co-gate — a specific single-chunk
    // win that share dilution would otherwise suppress on a topic-diverse surface).
    // Body-confirm (stage 5) then carries precision for the peak-admitted case.
    if dbg {
        eprintln!("LI: top ranked (share, peak, chunk, id):");
        for r in ranked.iter().take(8) {
            eprintln!("  {:.3} share  {:.3} peak  c{}  {}", r.share, r.peak, r.peak_chunk, r.id);
        }
    }
    let mut survivors: Vec<Ranked> = ranked
        .into_iter()
        .filter(|r| r.share >= SHARE_GATE || r.peak >= PEAK_GATE)
        .collect();
    // Confirm the strongest evidence first, bounded: peak is the specificity signal,
    // so peak-admitted candidates are not starved by a share-desc order.
    survivors.sort_by(|a, b| b.peak.partial_cmp(&a.peak).unwrap_or(std::cmp::Ordering::Equal));
    survivors.truncate(MAX_WINNERS_TO_CONFIRM);
    if dbg { eprintln!("LI: {} survivors (share ≥ {SHARE_GATE} OR peak ≥ {PEAK_GATE})", survivors.len()); }

    // Stage 5: confirm each survivor against the chunk it WON (its peak chunk),
    // not the whole surface. Confirming against every surface chunk diluted a way
    // that legitimately matched only part of a multi-topic surface (measured — it
    // rejected an ADR way on an ADR-plus-PR-plus-tests prompt). Corroborating the
    // winning evidence against the body still rejects single-token collisions (a
    // collided chunk finds no support in the way's own body) without the dilution.
    let t = std::time::Instant::now();
    let n_survivors = survivors.len();
    let mut fired = HashMap::new();
    for r in survivors {
        let Some(path) = bodies.get(&r.id) else {
            if dbg { eprintln!("LI:   {} → no body path", r.id); }
            continue;
        };
        let confirm = confirmer.confirm(&r.id, r.peak_chunk, r.peak, &chunks, path)?;
        if dbg { eprintln!("LI:   {} confirm={confirm:.3} (gate {CONFIRM_GATE})", r.id); }
        if confirm >= CONFIRM_GATE {
            fired.insert(r.id, r.share);
        }
    }
    if dbg { eprintln!("LI: confirm stage {n_survivors} survivors in {:.2} ms", ms(t)); }
    if dbg { eprintln!("LI: fired {} ways", fired.len()); }
    Some(Verdicts { fired, sidecar: confirmer.uses_sidecar() })
}

/// Milliseconds since `t`, for the WAYS_LI_DEBUG trace.
fn ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// ADR-701 §7: the body sidecar in `corpus_dir`, only when it was built with
/// the installed model and covers every way in `enabled` that the alias
/// corpus holds, at its content hashes (the manifest's `way_hashes`).
fn complete_sidecar(corpus_dir: &Path, bin: &Path, enabled: &HashMap<String, PathBuf>) -> Option<Sidecar> {
    sidecar::state(corpus_dir, bin, enabled.keys().map(String::as_str)).ok()
}

/// How stage 5 confirms a survivor: against the body sidecar with the match
/// pass's chunk vectors, or by embedding the body per call.
enum Confirmer<'a> {
    Sidecar { sidecar: Sidecar, vectors: Vec<Vec<f32>> },
    PerCall { bin: &'a Path, model: &'a Path },
}

impl<'a> Confirmer<'a> {
    /// The sidecar state needs both a complete sidecar and the chunk vectors.
    fn new(
        bin: &'a Path,
        model: &'a Path,
        sidecar: Option<Sidecar>,
        vectors: Option<Vec<Vec<f32>>>,
    ) -> Self {
        match (sidecar, vectors) {
            (Some(sidecar), Some(vectors)) => Confirmer::Sidecar { sidecar, vectors },
            _ => Confirmer::PerCall { bin, model },
        }
    }

    fn uses_sidecar(&self) -> bool {
        matches!(self, Confirmer::Sidecar { .. })
    }

    /// Confirmation score of way `id` on chunk `won`, where its alias cosine
    /// was `peak`. `None` only when the per-call subprocess fails.
    fn confirm(&self, id: &str, won: usize, peak: f64, chunks: &[String], body_path: &Path) -> Option<f64> {
        match self {
            Confirmer::Sidecar { sidecar, vectors } => {
                let v = vectors.get(won)?;
                Some(sidecar_confirm(sidecar, id, v, peak))
            }
            Confirmer::PerCall { bin, model } => body_confirm(bin, model, &chunks[won..=won], body_path),
        }
    }
}

/// Stage 5 against the sidecar: the max cosine of the won chunk's vector over
/// the way's section vectors. A way with no sections confirms against its
/// alias vector (ADR-701 §6). Its cosine with the won chunk is the chunk's
/// match score, which is the way's peak, so no alias vector is read.
fn sidecar_confirm(sc: &Sidecar, id: &str, chunk: &[f32], peak: f64) -> f64 {
    sc.max_cosine(id, chunk).unwrap_or(peak)
}

/// Split the surface into sentence chunks, drop trivially short fragments and
/// duplicates, and cap the count. Sentences never contain newlines, so each is a
/// valid single-line `match --batch` query.
fn chunk_surface(surface: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for s in split_sentences(surface) {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if s.chars().count() < MIN_CHUNK_CHARS {
            continue;
        }
        if seen.insert(s.clone()) {
            out.push(s);
            if out.len() >= MAX_SURFACE_CHUNKS {
                break;
            }
        }
    }
    out
}

/// The match pass's output: per-chunk rows and, when asked for and supported,
/// each chunk's embedding.
struct Matched {
    per_chunk: Vec<Vec<(String, f64)>>,
    vectors: Option<Vec<Vec<f32>>>,
}

/// Stage 2 match: run `way-embed match --batch` with the chunks on stdin, return
/// per-chunk `(way_id, cosine)` rows (each inner vec sorted by cosine desc, as
/// way-embed emits). `--threshold 0.0` returns every non-negative cosine.
///
/// With `want_vectors` the pass adds `--vectors` and also returns each chunk's
/// vector. The caller asks only when the sidecar is usable, which means the
/// manifest recorded that this very binary supports the flag; a failed pass is
/// a failed pass and is not run again.
fn batch_match(bin: &Path, corpus: &Path, model: &Path, chunks: &[String], want_vectors: bool) -> Option<Matched> {
    let mut cmd = Command::new(bin);
    cmd.args(["match", "--corpus", corpus.to_str()?, "--model", model.to_str()?, "--batch", "--threshold", "0.0"]);
    if want_vectors {
        cmd.arg("--vectors");
    }
    let matched = parse_match(&run_stdin(cmd, &chunks.join("\n"))?, chunks.len())?;
    Some(Matched { vectors: matched.vectors.filter(|_| want_vectors), ..matched })
}

/// Parse `way-embed match --batch` output. Score lines are
/// `qindex<TAB>id<TAB>cos`, grouped and ordered by qindex; with `--vectors`
/// each query's group is preceded by `v<TAB>qindex<TAB>f,f,...`. Vectors come
/// back only when every chunk has one of a single dimension.
fn parse_match(stdout: &str, n_chunks: usize) -> Option<Matched> {
    let mut per_chunk: Vec<Vec<(String, f64)>> = vec![Vec::new(); n_chunks];
    let mut vectors: Vec<Option<Vec<f32>>> = vec![None; n_chunks];
    for line in stdout.lines() {
        let mut parts = line.split('\t');
        let first = parts.next()?;
        if first == "v" {
            let qi: usize = parts.next()?.parse().ok()?;
            let v: Option<Vec<f32>> = parts.next()?.split(',').map(|x| x.parse().ok()).collect();
            if let Some(slot) = vectors.get_mut(qi) {
                *slot = v;
            }
            continue;
        }
        let qi: usize = first.parse().ok()?;
        let id = parts.next()?.to_string();
        let cos: f64 = parts.next()?.parse().ok()?;
        if let Some(bucket) = per_chunk.get_mut(qi) {
            bucket.push((id, cos));
        }
    }
    let vectors: Option<Vec<Vec<f32>>> = vectors.into_iter().collect();
    let vectors = vectors.filter(|vs| vs.first().is_some_and(|f| !f.is_empty() && vs.iter().all(|v| v.len() == f.len())));
    Some(Matched { per_chunk, vectors })
}

/// ADR-701 §1: keep only rows for ways in the enabled set before any
/// competition. The corpus embeds every way, disabled or not, so a disabled way
/// that wins a chunk would otherwise take softmax mass and a top-K or survivor
/// slot from an enabled way, then be dropped later for want of a body path.
/// `enabled` is the scan's candidate map (`body_map`): candidates are already
/// filtered by the user-scope domain list, the project toggles and ADR-143
/// shadowing, so its keys are exactly the corpus ids allowed to compete. Each
/// chunk stays sorted by cosine descending, so the top-K window refills from
/// enabled ways.
fn mask_to_enabled(
    mut per_chunk: Vec<Vec<(String, f64)>>,
    enabled: &HashMap<String, PathBuf>,
) -> Vec<Vec<(String, f64)>> {
    for chunk in &mut per_chunk {
        chunk.retain(|(id, _)| enabled.contains_key(id));
    }
    per_chunk
}

/// Stages 3+4: fold per-chunk scores into `(id, peak, share)` sorted by share.
/// - **peak** = max cosine over chunks (ranking / specificity).
/// - **share** = (summed per-chunk softmax mass) / n_chunks (gate / competition).
fn aggregate(per_chunk: &[Vec<(String, f64)>], n_chunks: usize) -> Vec<Ranked> {
    // Track each way's peak cosine AND which chunk it peaked on — the chunk that
    // is the way's strongest evidence, which confirmation then corroborates.
    let mut peak: HashMap<&str, (f64, usize)> = HashMap::new();
    let mut mass: HashMap<&str, f64> = HashMap::new();

    for (ci, chunk) in per_chunk.iter().enumerate() {
        for (id, cos) in chunk {
            let e = peak.entry(id.as_str()).or_insert((f64::MIN, 0));
            if *cos > e.0 {
                *e = (*cos, ci);
            }
        }
        // Softmax over the chunk's top-K (rows are already sorted desc).
        let top = &chunk[..chunk.len().min(TOP_K_PER_CHUNK)];
        let denom: f64 = top.iter().map(|(_, c)| (c / SOFTMAX_TAU).exp()).sum();
        if denom > 0.0 {
            for (id, cos) in top {
                *mass.entry(id.as_str()).or_insert(0.0) += (cos / SOFTMAX_TAU).exp() / denom;
            }
        }
    }

    let n = n_chunks.max(1) as f64;
    let mut out: Vec<Ranked> = mass
        .iter()
        .map(|(id, m)| {
            let (p, idx) = peak.get(id).copied().unwrap_or((0.0, 0));
            Ranked { id: (*id).to_string(), peak: p, peak_chunk: idx, share: m / n }
        })
        .collect();
    out.sort_by(|a, b| b.share.partial_cmp(&a.share).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Stage 5: mean-of-max body cross-similarity. For each surface chunk, take its
/// best similarity to any of the winning way's body chunks; average those bests.
/// Returns `Some(0.0)` (never fires) when the body has no usable chunks, `None`
/// only on a subprocess failure.
fn body_confirm(bin: &Path, model: &Path, surface: &[String], body_path: &Path) -> Option<f64> {
    let body = std::fs::read_to_string(body_path).ok()?;
    let body_chunks = chunk_body(&body);
    if body_chunks.is_empty() {
        return Some(0.0);
    }

    // Surface-major pairs: sim(surface[i], body[j]) lands at i*n_body + j.
    let mut input = String::new();
    for s in surface {
        for b in &body_chunks {
            input.push_str(s);
            input.push('\t');
            input.push_str(b);
            input.push('\n');
        }
    }

    let mut cmd = Command::new(bin);
    cmd.args(["similarity", "--model", model.to_str()?, "--batch"]);
    let stdout = run_stdin(cmd, &input)?;

    let sims: Vec<f64> = stdout.lines().filter_map(|l| l.trim().parse().ok()).collect();
    let n_body = body_chunks.len();
    if sims.len() != surface.len() * n_body {
        return None; // shape mismatch — don't guess
    }

    let mut sum_best = 0.0;
    for i in 0..surface.len() {
        let best = sims[i * n_body..(i + 1) * n_body]
            .iter()
            .copied()
            .fold(f64::MIN, f64::max);
        sum_best += best;
    }
    Some(sum_best / surface.len() as f64)
}

/// Chunk a way's `.md` body for confirmation: drop YAML frontmatter and fenced
/// code, then sentence-split the prose and cap the count.
fn chunk_body(content: &str) -> Vec<String> {
    // An unclosed frontmatter block is not body: nothing after its fence is prose.
    let body = if crate::frontmatter::opens_with_fence(content) {
        crate::frontmatter::split(content).map_or("", |(_, body)| body)
    } else {
        content
    };
    let mut prose = String::new();
    let mut in_fence = false;
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Strip common markdown lead characters; keep the words.
        let cleaned = line.trim_start_matches(['#', '>', '-', '*', ' ', '\t']);
        prose.push_str(cleaned);
        prose.push('\n');
    }

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for s in split_sentences(&prose) {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if s.chars().count() < MIN_CHUNK_CHARS {
            continue;
        }
        if seen.insert(s.clone()) {
            out.push(s);
            if out.len() >= MAX_BODY_CHUNKS {
                break;
            }
        }
    }
    out
}

/// Words per section chunk: MiniLM truncates past about 256 tokens, so a long
/// section is split into pieces of this many words (ADR-701 §6).
const SECTION_MAX_WORDS: usize = 120;
/// A section piece shorter than this many characters is dropped.
const SECTION_MIN_CHARS: usize = 25;

/// Chunk a way's `.md` into heading sections for the body sidecar (ADR-701 §6),
/// the `section` strategy of `experiments/content-corpus/run.py`: drop the
/// frontmatter, HTML comments, fenced code, table rows and the See Also
/// section; split at headings; split a section longer than
/// [`SECTION_MAX_WORDS`] words; prefix each piece with its heading.
pub(crate) fn chunk_sections(content: &str) -> Vec<String> {
    use std::sync::OnceLock;
    static RE: OnceLock<[regex::Regex; 6]> = OnceLock::new();
    let [comment, heading_re, lead, code, emph, link] = RE.get_or_init(|| {
        [
            regex::Regex::new(r"(?s)<!--.*?-->").expect("static regex"),
            regex::Regex::new(r"^(#{1,6})\s+(.*)").expect("static regex"),
            regex::Regex::new(r"^([-*>]|\d+\.)\s+").expect("static regex"),
            regex::Regex::new(r"`([^`]*)`").expect("static regex"),
            regex::Regex::new(r"\*\*?([^*]+)\*\*?").expect("static regex"),
            regex::Regex::new(r"\[([^\]]+)\]\([^)]*\)").expect("static regex"),
        ]
    });

    let body = if crate::frontmatter::opens_with_fence(content) {
        crate::frontmatter::split(content).map_or("", |(_, body)| body)
    } else {
        content
    };
    let body = comment.replace_all(body, "");

    // (heading, prose lines) per section; "" marks a paragraph break.
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    let (mut heading, mut lines, mut in_fence, mut skip) = (String::new(), Vec::<String>::new(), false, false);
    for raw in body.lines() {
        let t = raw.trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(m) = heading_re.captures(t) {
            if !lines.is_empty() && !skip {
                blocks.push((std::mem::take(&mut heading), std::mem::take(&mut lines)));
            }
            heading = m[2].trim().to_string();
            lines.clear();
            skip = heading.to_lowercase().starts_with("see also");
            continue;
        }
        if skip || t.starts_with('|') || t.is_empty() {
            if t.is_empty() && lines.last().is_some_and(|l| !l.is_empty()) {
                lines.push(String::new());
            }
            continue;
        }
        lines.push(lead.replace(t, "").into_owned());
    }
    if !lines.is_empty() && !skip {
        blocks.push((heading, lines));
    }

    let clean = |s: &str| {
        let s = code.replace_all(s, "$1");
        let s = emph.replace_all(&s, "$1");
        let s = link.replace_all(&s, "$1");
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let mut out = Vec::new();
    for (heading, lines) in blocks {
        let joined = lines.iter().filter(|l| !l.is_empty()).map(String::as_str).collect::<Vec<_>>().join(" ");
        let cleaned = clean(&joined);
        let words: Vec<&str> = cleaned.split_whitespace().collect();
        for piece in words.chunks(SECTION_MAX_WORDS) {
            let piece = piece.join(" ");
            if piece.chars().count() < SECTION_MIN_CHARS {
                continue;
            }
            out.push(if heading.is_empty() { piece } else { format!("{heading}. {piece}") });
        }
    }
    out
}

/// Run `cmd` feeding `input` on stdin, return stdout on success. stdin is small
/// (chunks/pairs), so writing it fully before draining stdout cannot deadlock.
fn run_stdin(mut cmd: Command, input: &str) -> Option<String> {
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    child.stdin.take()?.write_all(input.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Share per way after masking to `enabled`, for tests in sibling modules.
#[cfg(test)]
pub(super) fn shares_for_test(per_chunk: Vec<Vec<(String, f64)>>, enabled: &HashMap<String, PathBuf>) -> Vec<(String, f64)> {
    let n = per_chunk.len();
    aggregate(&mask_to_enabled(per_chunk, enabled), n).into_iter().map(|r| (r.id, r.share)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_surface_splits_dedups_and_caps() {
        let s = "Write an ADR for this. Write an ADR for this. Run the test suite now please.";
        let chunks = chunk_surface(s);
        assert_eq!(chunks.len(), 2, "dupes collapse: {chunks:?}");
        assert!(chunks[0].starts_with("Write an ADR"));
    }

    #[test]
    fn chunk_surface_drops_short_fragments() {
        // "Yes." is below MIN_CHUNK_CHARS and must not survive as a chunk.
        let chunks = chunk_surface("Yes. Please write the architecture decision record now.");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].starts_with("Please write"));
    }

    #[test]
    fn chunk_body_strips_frontmatter_and_fences() {
        let body = "---\nkey: val\n---\n# Heading\nThis is the real guidance sentence.\n```\ncode not prose here\n```\nAnother genuine sentence of body text.\n";
        let chunks = chunk_body(body);
        assert!(chunks.iter().all(|c| !c.contains("code not prose")), "{chunks:?}");
        assert!(chunks.iter().any(|c| c.contains("real guidance")));
        assert!(chunks.iter().any(|c| c.contains("genuine sentence")));
    }

    /// The frontmatter closes only on a bare `---` line ([`crate::frontmatter::split`]).
    /// The old chunker trimmed the line first, so an indented `  ---` inside the
    /// YAML closed the block early and the rest of the YAML was read as prose.
    #[test]
    fn chunk_body_closes_frontmatter_only_on_a_bare_fence() {
        let body = "---\nvocabulary: a\n  ---\nleaked: yaml value that is long enough\n---\nThis is the real guidance sentence.\n";
        let chunks = chunk_body(body);
        assert!(chunks.iter().all(|c| !c.contains("leaked")), "{chunks:?}");
        assert!(chunks.iter().any(|c| c.contains("real guidance")));
    }

    /// An unclosed frontmatter block is not body: nothing after its opening
    /// fence is read as prose, the rule `frontmatter::split` holds elsewhere.
    #[test]
    fn chunk_body_reads_nothing_after_an_unclosed_fence() {
        let body = "---\nvocabulary: leaked yaml words that are long enough\nmore leaked yaml text here.\n";
        let chunks = chunk_body(body);
        assert!(chunks.iter().all(|c| !c.contains("leaked")), "{chunks:?}");
    }

    /// ADR-701 §6: the sidecar's section chunks are exactly run.py's `section`
    /// strategy. The expected chunks were produced by run.py on this fixture.
    #[test]
    fn chunk_sections_matches_the_measured_section_strategy() {
        let long: Vec<String> = (0..120).map(|i| format!("w{i}")).collect();
        let fixture = format!(
            "---\ndescription: a fixture way\nvocabulary: fixture words\n---\n\
             <!-- epistemic: heuristic -->\n# Fixture Way\n\n\
             Lead paragraph with **bold words** and `inline code` and a [link](http://x.y).\n\
             - A bullet item that continues the lead.\n1. A numbered item.\n\nShort.\n\n\
             ## Commands\n\n```bash\necho \"code is not prose\"\n```\n\n\
             | col | col |\n|-----|-----|\n| table | row |\n\n\
             > A quoted line in the commands section, long enough to keep.\n\n\
             ## Tiny\n\nok\n\n## Long Section\n\n\
             {} tail words one two three four five six seven eight nine.\n\n\
             ## See Also\n\n- other/way(domain) — never embedded\n",
            long.join(" ")
        );
        let want = vec![
            "Fixture Way. Lead paragraph with bold words and inline code and a link. A bullet item that continues the lead. A numbered item. Short.".to_string(),
            "Commands. A quoted line in the commands section, long enough to keep.".to_string(),
            format!("Long Section. {}", long.join(" ")),
            "Long Section. tail words one two three four five six seven eight nine.".to_string(),
        ];
        assert_eq!(chunk_sections(&fixture), want);
    }

    /// A way whose prose is all tables and comments has no sections; the alias
    /// vector stands in for it at confirm time.
    #[test]
    fn chunk_sections_is_empty_for_a_table_only_body() {
        let body = "---\ndescription: d\nvocabulary: v\n---\n# Policy\n\n| a | b |\n|---|---|\n| c | d |\n<!-- note -->\n";
        assert!(chunk_sections(body).is_empty());
    }

    fn unit(v: &[f32]) -> Vec<f32> {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    /// A sidecar with way "multi" (three sections) and "bare" (none).
    fn sample_sidecar() -> Sidecar {
        let ways = vec![
            sidecar::WaySections {
                id: "multi".into(),
                hash: 1,
                vectors: vec![unit(&[1.0, 0.0, 0.0]), unit(&[1.0, 1.0, 0.0]), unit(&[0.0, 0.0, 1.0])],
            },
            sidecar::WaySections { id: "bare".into(), hash: 2, vectors: vec![] },
        ];
        sidecar::decode(&sidecar::encode("m", 3, &ways).unwrap()).unwrap()
    }

    /// Confirmation against the sidecar is the max cosine of the won chunk over
    /// the way's sections. Hand-computed for chunk (0.6, 0.8, 0): the sections
    /// give 0.6, (0.6 + 0.8) / √2 = 0.98995, and 0.
    #[test]
    fn sidecar_confirm_is_the_max_cosine_over_sections() {
        let sc = sample_sidecar();
        let chunk = [0.6_f32, 0.8, 0.0];
        let got = sidecar_confirm(&sc, "multi", &chunk, 0.123);
        assert!((got - 1.4 / 2f64.sqrt()).abs() < 1e-6, "{got}");
        assert!(got >= CONFIRM_GATE);
    }

    /// ADR-701 §6: a way with no sections confirms against its alias vector,
    /// whose cosine with the won chunk is the way's peak.
    #[test]
    fn sidecar_confirm_falls_back_to_the_peak_for_a_sectionless_way() {
        let p = Path::new("/nonexistent");
        let c = Confirmer::new(p, p, Some(sample_sidecar()), Some(vec![vec![0.6, 0.8, 0.0]]));
        let got = c.confirm("bare", 0, 0.42, &["c0".to_string()], p).unwrap();
        assert!((got - 0.42).abs() < 1e-12, "{got}");
    }

    #[test]
    fn parse_match_reads_vectors_and_scores() {
        let out = "v\t0\t0.5,0.25\n0\ta\t0.9000\n0\tb\t0.1000\nv\t1\t1,0\n1\tb\t0.7000\n";
        let Matched { per_chunk: rows, vectors } = parse_match(out, 2).unwrap();
        assert_eq!(rows[0], vec![("a".to_string(), 0.9), ("b".to_string(), 0.1)]);
        assert_eq!(rows[1], vec![("b".to_string(), 0.7)]);
        assert_eq!(vectors, Some(vec![vec![0.5, 0.25], vec![1.0, 0.0]]));
    }

    /// Output without vector lines (way-embed without `--vectors`), or with a
    /// chunk's vector missing, gives no vectors and the scores unchanged.
    #[test]
    fn parse_match_without_every_vector_returns_none() {
        let Matched { per_chunk: rows, vectors } = parse_match("0\ta\t0.9000\n1\tb\t0.7000\n", 2).unwrap();
        assert_eq!(rows[1], vec![("b".to_string(), 0.7)]);
        assert!(vectors.is_none());
        let vectors = parse_match("v\t0\t0.5,0.25\n0\ta\t0.9000\n1\tb\t0.7000\n", 2).unwrap().vectors;
        assert!(vectors.is_none(), "chunk 1 has no vector");
    }

    /// Confirmation reads the vector of the chunk the way won, not a neighbour.
    /// Chunk 0 matches section 3 exactly; chunk 1 is 45° off section 2.
    #[test]
    fn confirm_scores_the_won_chunks_vector() {
        let p = Path::new("x");
        let vectors = vec![vec![0.0, 0.0, 1.0], vec![0.0, 1.0, 0.0], vec![-1.0, 0.0, 0.0]];
        let c = Confirmer::new(p, p, Some(sample_sidecar()), Some(vectors));
        let chunks = vec!["c0".to_string(), "c1".to_string(), "c2".to_string()];
        let on = |won| c.confirm("multi", won, 0.0, &chunks, p).unwrap();
        assert!((on(0) - 1.0).abs() < 1e-6);
        assert!((on(1) - 0.5f64.sqrt()).abs() < 1e-6);
        assert!(on(2).abs() < 1e-6, "best of -1, -0.707, 0");
    }

    /// A match pass that fails is not run again: the model loads once per scan
    /// even when vectors were asked for.
    #[cfg(unix)]
    #[test]
    fn a_failed_match_runs_the_embedder_once() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ways-li-once-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("way-embed");
        let count = dir.join("calls");
        std::fs::write(&bin, format!("#!/bin/sh\necho x >> {}\nexit 1\n", count.display())).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let chunks = vec!["one chunk here".to_string(), "another chunk".to_string()];
        assert!(batch_match(&bin, &dir.join("c"), &dir.join("m"), &chunks, true).is_none());
        assert_eq!(std::fs::read_to_string(&count).unwrap().lines().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The sidecar state needs a complete sidecar and the chunk vectors; either
    /// missing is today's per-call confirmation.
    #[test]
    fn confirmer_uses_the_sidecar_only_with_vectors() {
        let p = Path::new("x");
        assert!(Confirmer::new(p, p, Some(sample_sidecar()), Some(vec![vec![1.0, 0.0, 0.0]])).uses_sidecar());
        assert!(!Confirmer::new(p, p, Some(sample_sidecar()), None).uses_sidecar());
        assert!(!Confirmer::new(p, p, None, Some(vec![vec![1.0, 0.0, 0.0]])).uses_sidecar());
    }

    /// ADR-701 §7 through the files a scan reads: the sidecar is used when the
    /// manifest records `--vectors` support, it was built for this model and
    /// way-embed, and it covers every enabled way the alias corpus holds at the
    /// manifest's hashes. Each other state names its reason.
    #[test]
    fn complete_sidecar_falls_back_with_a_reason() {
        use sidecar::Fallback;
        let dir = std::env::temp_dir().join(format!("ways-li-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::paths::EN_MODEL), "model").unwrap();
        let bin = dir.join("way-embed");
        std::fs::write(&bin, "binary").unwrap();
        let model = sidecar::model_id(&dir, &bin).unwrap();
        let write_sidecar = |model: &str, ways: &[(&str, u64)]| {
            let ways: Vec<_> = ways
                .iter()
                .map(|(id, h)| sidecar::WaySections { id: id.to_string(), hash: *h, vectors: vec![vec![1.0, 0.0]] })
                .collect();
            sidecar::write(&dir.join(sidecar::FILE), &sidecar::encode(model, 2, &ways).unwrap()).unwrap();
        };
        let manifest = |side: &str| {
            std::fs::write(
                dir.join("embed-manifest.json"),
                format!(r#"{{"way_hashes":{{"a":"000000000000000a","b":"000000000000000b","off":"00000000000000ff"}},"body_sidecar":{side}}}"#),
            )
            .unwrap();
        };
        let state = |enabled: &[&str]| sidecar::state(&dir, &bin, enabled.iter().copied()).map(|_| ());
        manifest(r#"{"file":"ways-body-en.bin","vectors":true}"#);
        let enabled = bodies_of(&["a", "b"]);

        write_sidecar(&model, &[("a", 0xa), ("b", 0xb), ("off", 0xff)]);
        assert!(complete_sidecar(&dir, &bin, &enabled).is_some(), "complete");
        assert_eq!(state(&["a", "b", "-proj/unregistered"]), Ok(()), "a way the alias corpus lacks does not count");

        write_sidecar(&model, &[("a", 0xa), ("b", 0xb)]);
        assert_eq!(state(&["a", "b"]), Ok(()), "a missing disabled way does not matter");

        write_sidecar(&model, &[("a", 0xa), ("off", 0xff)]);
        assert_eq!(state(&["a", "b"]), Err(Fallback::Incomplete { missing: vec!["b".into()], stale: vec![] }));
        assert!(complete_sidecar(&dir, &bin, &enabled).is_none());

        write_sidecar(&model, &[("a", 0xa), ("b", 0xbb), ("off", 0xff)]);
        assert_eq!(state(&["a", "b"]), Err(Fallback::Incomplete { missing: vec![], stale: vec!["b".into()] }));

        write_sidecar("other-model:1", &[("a", 0xa), ("b", 0xb)]);
        assert_eq!(state(&["a", "b"]), Err(Fallback::ModelMismatch), "built for another model");

        write_sidecar(&model, &[("a", 0xa), ("b", 0xb)]);
        std::fs::write(&bin, "a replaced way-embed binary").unwrap();
        assert_eq!(state(&["a", "b"]), Err(Fallback::ModelMismatch), "way-embed replaced since the build");
        std::fs::write(&bin, "binary").unwrap();

        manifest(r#"{"file":"ways-body-en.bin"}"#);
        assert_eq!(state(&["a", "b"]), Err(Fallback::NoVectors), "manifest does not record --vectors");

        manifest(r#"{"file":null,"reason":"way-embed < 1.2.0 lacks --vectors"}"#);
        assert_eq!(state(&["a", "b"]), Err(Fallback::BuildFailed("way-embed < 1.2.0 lacks --vectors".into())));

        manifest(r#"{"file":"ways-body-en.bin","vectors":true}"#);
        std::fs::remove_file(dir.join(sidecar::FILE)).unwrap();
        assert_eq!(state(&["a", "b"]), Err(Fallback::Absent), "no sidecar");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn bodies_of(ids: &[&str]) -> HashMap<String, PathBuf> {
        ids.iter().map(|i| (i.to_string(), PathBuf::from(format!("/{i}.md")))).collect()
    }

    /// ADR-701 §1: a disabled way that wins a chunk must not take softmax mass
    /// or a survivor slot. With it masked, every enabled way scores exactly as
    /// if the disabled way were absent from the corpus.
    #[test]
    fn masking_leaves_enabled_ways_as_if_the_disabled_way_were_absent() {
        let row = |pairs: &[(&str, f64)]| pairs.iter().map(|(i, c)| (i.to_string(), *c)).collect::<Vec<_>>();
        // "off" is disabled and beats everything on both chunks.
        let with_off = vec![
            row(&[("off", 0.9), ("a", 0.6), ("b", 0.5), ("c", 0.1)]),
            row(&[("off", 0.8), ("b", 0.55), ("a", 0.3), ("c", 0.1)]),
        ];
        let absent = vec![
            row(&[("a", 0.6), ("b", 0.5), ("c", 0.1)]),
            row(&[("b", 0.55), ("a", 0.3), ("c", 0.1)]),
        ];
        let enabled = bodies_of(&["a", "b", "c"]);

        // Unmasked, the disabled way takes share and a ranking slot.
        let unmasked = aggregate(&with_off, 2);
        assert_eq!(unmasked[0].id, "off");

        let masked = aggregate(&mask_to_enabled(with_off, &enabled), 2);
        let want = aggregate(&absent, 2);
        assert_eq!(masked.len(), want.len());
        for (m, w) in masked.iter().zip(&want) {
            assert_eq!(m.id, w.id);
            assert!((m.share - w.share).abs() < 1e-12, "{}: share {} vs {}", m.id, m.share, w.share);
            assert!((m.peak - w.peak).abs() < 1e-12);
            assert_eq!(m.peak_chunk, w.peak_chunk);
            // Admission is the same decision.
            assert_eq!(m.share >= SHARE_GATE || m.peak >= PEAK_GATE, w.share >= SHARE_GATE || w.peak >= PEAK_GATE);
        }
        assert!(masked.iter().all(|r| r.id != "off"));
    }

    /// The eight-way softmax window refills from enabled ways: a disabled way
    /// inside the top 8 must not shrink the competition to seven.
    #[test]
    fn masking_refills_the_top_k_window_with_enabled_ways() {
        let mut chunk: Vec<(String, f64)> = vec![("off".to_string(), 0.95)];
        let ids: Vec<String> = (0..9).map(|i| format!("w{i}")).collect();
        for (i, id) in ids.iter().enumerate() {
            chunk.push((id.clone(), 0.5 - i as f64 * 0.01));
        }
        let enabled = bodies_of(&ids.iter().map(String::as_str).collect::<Vec<_>>());
        let masked = mask_to_enabled(vec![chunk.clone(), chunk], &enabled);
        let ranked = aggregate(&masked, 2);
        let scored = ranked.iter().filter(|r| r.share > 0.0).count();
        assert_eq!(scored, TOP_K_PER_CHUNK, "eight enabled ways compete");
    }

    #[test]
    fn aggregate_peak_and_share() {
        // Two chunks; way "a" wins both strongly, "b" is a weak also-ran.
        let per_chunk = vec![
            vec![("a".to_string(), 0.9), ("b".to_string(), 0.2)],
            vec![("a".to_string(), 0.8), ("b".to_string(), 0.25)],
        ];
        let ranked = aggregate(&per_chunk, 2);
        assert_eq!(ranked[0].id, "a");
        assert!((ranked[0].peak - 0.9).abs() < 1e-9, "peak = max over chunks");
        assert_eq!(ranked[0].peak_chunk, 0, "a peaks on chunk 0 (0.9 > 0.8)");
        assert!(ranked[0].share > ranked[1].share, "a's share dominates b's");
        assert!(ranked[0].share <= 1.0);
    }
}

