//! scan/machine.rs — ADR-160 chunked late-interaction matching machine.
//!
//! An alternative to the single-vector semantic gate (scoring.rs). Instead of
//! embedding one reduced surface and thresholding each way's calibrated cosine,
//! the machine:
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
//!   5. **confirms** each survivor by the *mean-of-max* cross-similarity of the
//!      surface chunks against the winning way's body chunks — breadth, which
//!      rejects single-token collisions and covers the softmax's blind spot (it
//!      always hands the winner mass, even when nothing is truly relevant).
//!
//! Peak (lenient, specificity) and mean-confirm (strict, breadth) take opposite
//! stances on purpose — that split is the load-bearing choice (ADR-160).
//!
//! **Operating points are HAND-SET and UNCALIBRATED.** ADR-160 stays Proposed
//! until they are fit against a precision metric (task #5). This module exists to
//! be deployed and *tried*, behind the `matching_machine` config flag.
//!
//! **Fail-safe.** Every stage returns `None` when it cannot run (no binary /
//! corpus / model, a surface too sparse to chunk), and the caller falls back to
//! the single-vector path. The machine never partially decides.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::reduce::split_sentences;
use super::scoring::find_way_embed;

// ── Hand-set operating points (uncalibrated — task #5 fits these) ──
/// Per-chunk softmax temperature. Small τ sharpens the competition so a clear
/// per-chunk winner takes most of the mass.
const SOFTMAX_TAU: f64 = 0.08;
/// Ways entering each chunk's softmax (the rest score ~0 mass anyway).
const TOP_K_PER_CHUNK: usize = 8;
/// Summed-share / n_chunks a way must reach to survive ranking into confirmation.
const SHARE_GATE: f64 = 0.15;
/// Mean-of-max body cross-similarity a survivor must reach to actually fire.
const CONFIRM_GATE: f64 = 0.40;
/// Caps — keep the batched embedding bounded on a pathological surface/body.
const MAX_SURFACE_CHUNKS: usize = 12;
const MAX_BODY_CHUNKS: usize = 8;
const MIN_CHUNK_CHARS: usize = 12;
/// Only the strongest ranking survivors are worth a body-confirm pass.
const MAX_WINNERS_TO_CONFIRM: usize = 6;

/// The machine's decision for a scan: the ways it fired, keyed by corpus id, with
/// a representative score (the summed softmax-share) for telemetry.
pub(crate) struct MachineVerdicts {
    fired: HashMap<String, f64>,
}

impl MachineVerdicts {
    /// The share score if the machine fired `corpus_id`, else `None`.
    pub(crate) fn fired_score(&self, corpus_id: &str) -> Option<f64> {
        self.fired.get(corpus_id).copied()
    }
}

/// Run the machine over `surface`. `bodies` maps a way's corpus id to its `.md`
/// path (for body-confirm). Returns `None` when the machine cannot run — the
/// caller then falls back to the single-vector semantic gate.
pub(crate) fn run(surface: &str, bodies: &HashMap<String, PathBuf>) -> Option<MachineVerdicts> {
    let dbg = std::env::var("WAYS_MACHINE_DEBUG").is_ok();
    let bin = find_way_embed()?;
    let xdg = crate::paths::corpus_dir();
    let corpus = xdg.join("ways-corpus-en.jsonl");
    let model = xdg.join("minilm-l6-v2.gguf");
    if !corpus.is_file() || !model.is_file() {
        if dbg { eprintln!("MACHINE: corpus/model missing → fallback"); }
        return None;
    }

    // Stage 2 (chunk). Too few chunks → no competition to run; fall back.
    let chunks = chunk_surface(surface);
    if dbg { eprintln!("MACHINE: {} chunks: {:?}", chunks.len(), chunks); }
    if chunks.len() < 2 {
        if dbg { eprintln!("MACHINE: <2 chunks → fallback"); }
        return None;
    }

    // Stage 2 (match): one batched pass, all chunks against the corpus.
    let per_chunk = batch_match(&bin, &corpus, &model, &chunks)?;
    if dbg { eprintln!("MACHINE: per_chunk rows: {:?}", per_chunk.iter().map(|c| c.len()).collect::<Vec<_>>()); }

    // Stages 3+4: peak rank + per-chunk softmax-share, sorted by share desc.
    let ranked = aggregate(&per_chunk, chunks.len());

    // Stage 4 gate: survivors above the share gate, strongest first.
    if dbg {
        eprintln!("MACHINE: top ranked (id, peak, share):");
        for (id, peak, share) in ranked.iter().take(8) {
            eprintln!("  {share:.3} share  {peak:.3} peak  {id}");
        }
    }
    let survivors: Vec<(String, f64)> = ranked
        .into_iter()
        .filter(|(_, _peak, share)| *share >= SHARE_GATE)
        .take(MAX_WINNERS_TO_CONFIRM)
        .map(|(id, _peak, share)| (id, share))
        .collect();
    if dbg { eprintln!("MACHINE: {} survivors above share gate {SHARE_GATE}", survivors.len()); }

    // Stage 5: body-confirm each survivor (breadth check).
    let mut fired = HashMap::new();
    for (id, share) in survivors {
        let Some(path) = bodies.get(&id) else {
            if dbg { eprintln!("MACHINE:   {id} → no body path"); }
            continue;
        };
        let confirm = body_confirm(&bin, &model, &chunks, path)?;
        if dbg { eprintln!("MACHINE:   {id} confirm={confirm:.3} (gate {CONFIRM_GATE})"); }
        if confirm >= CONFIRM_GATE {
            fired.insert(id, share);
        }
    }
    if dbg { eprintln!("MACHINE: fired {} ways", fired.len()); }
    Some(MachineVerdicts { fired })
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

/// Stage 2 match: run `way-embed match --batch` with the chunks on stdin, return
/// per-chunk `(way_id, cosine)` rows (each inner vec sorted by cosine desc, as
/// way-embed emits). `--threshold 0.0` returns every non-negative cosine.
fn batch_match(
    bin: &Path,
    corpus: &Path,
    model: &Path,
    chunks: &[String],
) -> Option<Vec<Vec<(String, f64)>>> {
    let mut cmd = Command::new(bin);
    cmd.args([
        "match",
        "--corpus",
        corpus.to_str()?,
        "--model",
        model.to_str()?,
        "--batch",
        "--threshold",
        "0.0",
    ]);
    let input = chunks.join("\n");
    let stdout = run_stdin(cmd, &input)?;

    // Lines are `qindex<TAB>id<TAB>cos`, grouped and ordered by qindex.
    let mut per_chunk: Vec<Vec<(String, f64)>> = vec![Vec::new(); chunks.len()];
    for line in stdout.lines() {
        let mut parts = line.split('\t');
        let qi: usize = parts.next()?.parse().ok()?;
        let id = parts.next()?.to_string();
        let cos: f64 = parts.next()?.parse().ok()?;
        if let Some(bucket) = per_chunk.get_mut(qi) {
            bucket.push((id, cos));
        }
    }
    Some(per_chunk)
}

/// Stages 3+4: fold per-chunk scores into `(id, peak, share)` sorted by share.
/// - **peak** = max cosine over chunks (ranking / specificity).
/// - **share** = (summed per-chunk softmax mass) / n_chunks (gate / competition).
fn aggregate(per_chunk: &[Vec<(String, f64)>], n_chunks: usize) -> Vec<(String, f64, f64)> {
    let mut peak: HashMap<&str, f64> = HashMap::new();
    let mut mass: HashMap<&str, f64> = HashMap::new();

    for chunk in per_chunk {
        // Peak over the whole chunk.
        for (id, cos) in chunk {
            let e = peak.entry(id.as_str()).or_insert(f64::MIN);
            if *cos > *e {
                *e = *cos;
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
    let mut out: Vec<(String, f64, f64)> = mass
        .iter()
        .map(|(id, m)| {
            let p = peak.get(id).copied().unwrap_or(0.0);
            ((*id).to_string(), p, m / n)
        })
        .collect();
    out.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
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
    let mut prose = String::new();
    let mut in_frontmatter = false;
    let mut in_fence = false;
    for (i, line) in content.lines().enumerate() {
        let t = line.trim_start();
        // A leading `---` on line 0 opens a frontmatter block.
        if i == 0 && t == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if t == "---" {
                in_frontmatter = false;
            }
            continue;
        }
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

    #[test]
    fn aggregate_peak_and_share() {
        // Two chunks; way "a" wins both strongly, "b" is a weak also-ran.
        let per_chunk = vec![
            vec![("a".to_string(), 0.9), ("b".to_string(), 0.2)],
            vec![("a".to_string(), 0.8), ("b".to_string(), 0.25)],
        ];
        let ranked = aggregate(&per_chunk, 2);
        assert_eq!(ranked[0].0, "a");
        assert!((ranked[0].1 - 0.9).abs() < 1e-9, "peak = max over chunks");
        assert!(ranked[0].2 > ranked[1].2, "a's share dominates b's");
        assert!(ranked[0].2 <= 1.0);
    }
}
