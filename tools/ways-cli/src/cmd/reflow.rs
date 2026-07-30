//! `ways reflow` — detect and repair hard-wrapped markdown prose (#415).
//!
//! The thin caller over [`ways_core::reflow`]. The detector itself lives in the
//! library because three callers share it: this command, the markdown way's
//! `postcheck.sh`, and the `ways lint` rule. A separate implementation in any
//! one of them would drift, and the failure mode is the ugly one — the way
//! fires and then the tool reports the file clean.
//!
//! # Exit codes
//!
//! Lint convention: **0 = clean, 1 = wrapped prose found**, 2 = error. Note
//! that `postcheck.sh` wants the opposite (exit 0 means "fire"), so the
//! postcheck wrapper inverts this. One flag cannot serve both conventions, and
//! the wrapper exists anyway.

use anyhow::{Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};

use ways_core::reflow::{self, TokenVerdict};

/// Where the text came from — decides whether repair can write anywhere.
enum Source {
    File(PathBuf),
    Stdin,
}

pub fn run(path: Option<String>, fix: bool, json: bool, quiet: bool) -> Result<()> {
    let (source, text) = match path {
        Some(p) => {
            let pb = PathBuf::from(&p);
            let text = std::fs::read_to_string(&pb)
                .with_context(|| format!("reading {}", pb.display()))?;
            (Source::File(pb), text)
        }
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading stdin")?;
            (Source::Stdin, buf)
        }
    };

    let lines: Vec<&str> = text.split('\n').collect();
    let hits = reflow::detect(&lines);

    if hits.is_empty() {
        if json {
            println!("{{\"wrapped\":false,\"paragraphs\":[]}}");
        } else if !quiet {
            eprintln!("clean — no hard-wrapped prose detected");
        }
        return Ok(());
    }

    if json {
        let items: Vec<String> = hits
            .iter()
            .map(|w| {
                format!(
                    "{{\"start\":{},\"end\":{},\"trigger_line\":{},\"mid_breaks\":{}}}",
                    w.start + 1,
                    w.end + 1,
                    w.trigger_start + 1,
                    w.mid_breaks
                )
            })
            .collect();
        println!(
            "{{\"wrapped\":true,\"paragraphs\":[{}]}}",
            items.join(",")
        );
    } else if !quiet {
        let label = match &source {
            Source::File(p) => p.display().to_string(),
            Source::Stdin => "<stdin>".to_string(),
        };
        eprintln!(
            "{label}: {} hard-wrapped paragraph(s)",
            hits.len()
        );
        for w in &hits {
            eprintln!("  lines {}-{}", w.start + 1, w.end + 1);
        }
    }

    if !fix {
        // Found. Lint convention, so this is the non-zero case.
        std::process::exit(1);
    }

    let (repaired_lines, quote_joins) = reflow::reflow_with_stats(&lines);
    let repaired = repaired_lines.join("\n");

    // The invariant is a cheap total check against dropped words. It is not
    // sufficient on its own — it cannot see a wrong join, and leading
    // whitespace is not a token — so a clean verdict is evidence, not proof,
    // and the backup below is what makes it reviewable.
    //
    // Use the *accounted* form: the number of dropped `>` markers has to equal
    // the number of blockquote continuations the transform actually joined.
    // Otherwise a bare `>` swallowed from between two quoted paragraphs reads as
    // clean, since the difference is `>`-only either way.
    let verdict = reflow::token_verdict_accounted(&text, &repaired, quote_joins);
    if let TokenVerdict::Mismatch { before, after } = verdict {
        anyhow::bail!(
            "refusing to write: token stream changed ({before} -> {after}). \
             This is a bug in the reflow transform; nothing was modified."
        );
    }

    // The structural post-condition, and the one actually worth trusting: reparse
    // and compare. Unlike the classifier it does not depend on having enumerated
    // markdown's constructs correctly, so an enumeration gap becomes a refusal to
    // write rather than a corrupted document. This is how a live corruption was
    // caught that the token check above passed — a multi-line HTML comment whose
    // body was being joined.
    if !reflow::structure_preserved(&text, &repaired) {
        anyhow::bail!(
            "refusing to write {}: reflowing changed the document structure, not \
             just its line breaks. Nothing was modified. This means the file \
             contains a construct the classifier treated as ordinary prose — \
             please report it with the file attached.",
            match &source {
                Source::File(p) => p.display().to_string(),
                Source::Stdin => "<stdin>".to_string(),
            }
        );
    }

    match source {
        Source::Stdin => {
            print!("{repaired}");
            Ok(())
        }
        Source::File(p) => {
            let backup = write_backup(&p, &text)?;
            std::fs::write(&p, &repaired)
                .with_context(|| format!("writing {}", p.display()))?;
            if !quiet {
                eprintln!();
                eprintln!("repaired {}", p.display());
                eprintln!(
                    "  tokens: {}",
                    match verdict {
                        TokenVerdict::Identical => "identical".to_string(),
                        TokenVerdict::QuoteMarkersDropped { dropped } =>
                            format!("identical ({dropped} bare '>' marker(s) dropped)"),
                        TokenVerdict::Mismatch { .. } => unreachable!("bailed above"),
                    }
                );
                eprintln!("  backup: {}", backup.display());
                if is_tracked(&p) {
                    eprintln!("  review: git diff -- {}", p.display());
                } else {
                    eprintln!("  review: diff {} {}", backup.display(), p.display());
                }
            }
            Ok(())
        }
    }
}

/// Copy the original into a fresh temp directory before overwriting.
///
/// Not a predictable path: a fixed `<tmp>/<name>.bak` is a collision between two
/// sessions and a symlink-attack surface on a shared machine. `create_dir` (not
/// `create_dir_all`) is what makes this safe — it fails if the directory already
/// exists, so an attacker-planted symlink can't be followed. The basename is
/// preserved inside so the path identifies itself when read back out of a log.
///
/// `std::env::temp_dir()` rather than shelling out to `mktemp`: it matches the
/// rest of the Rust tree, drops a process spawn from the repair path, and has no
/// PATH dependency — which a native Windows build would not satisfy.
fn write_backup(path: &Path, original: &str) -> Result<PathBuf> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "document.md".to_string());

    let base = std::env::temp_dir();
    let pid = std::process::id();
    let mut dir = PathBuf::new();
    for attempt in 0..64u32 {
        let candidate = base.join(format!("ways-reflow.{pid}.{attempt}"));
        match std::fs::create_dir(&candidate) {
            Ok(()) => {
                dir = candidate;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("creating {}", candidate.display()))
            }
        }
    }
    if dir.as_os_str().is_empty() {
        anyhow::bail!("could not create a backup directory under {}", base.display());
    }

    let backup = dir.join(name);
    std::fs::write(&backup, original)
        .with_context(|| format!("writing backup {}", backup.display()))?;
    Ok(backup)
}

/// Whether git tracks this path — decides which review command to suggest.
fn is_tracked(path: &Path) -> bool {
    std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch"])
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
