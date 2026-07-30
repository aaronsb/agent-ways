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
    // Exit 2 for genuine failures, explicitly. Propagating with `?` would surface
    // as exit 1, which this command has already spent on "wrapped prose found" —
    // and a caller that cannot tell an unreadable file from a real finding will
    // act on the wrong one. `postcheck.sh` checks for exactly 1 for this reason.
    let (source, text) = match path {
        Some(p) => {
            let pb = PathBuf::from(&p);
            match std::fs::read_to_string(&pb) {
                Ok(text) => (Source::File(pb), text),
                Err(e) => {
                    eprintln!("ways reflow: reading {}: {e}", pb.display());
                    std::process::exit(2);
                }
            }
        }
        None => {
            let mut buf = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                eprintln!("ways reflow: reading stdin: {e}");
                std::process::exit(2);
            }
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

    // Repair, then run the detector against our own output. A result that still
    // trips the detector would be written and then immediately re-flagged — a
    // file that nags forever — so non-convergence is a finding, not a warning to
    // write through. In practice this settles on the first pass.
    let outcome = reflow::repair(&lines, reflow::MAX_REPAIR_PASSES);
    let repaired = outcome.lines.join("\n");
    let quote_joins = outcome.quote_joins;

    if !outcome.converged {
        let label = match &source {
            Source::File(p) => p.display().to_string(),
            Source::Stdin => "<stdin>".to_string(),
        };
        if json {
            println!(
                "{{\"wrapped\":true,\"repaired\":false,\"reason\":\"did-not-converge\",\"passes\":{}}}",
                outcome.passes
            );
        } else if !quiet {
            eprintln!();
            eprintln!("NOT repaired: {label}");
            eprintln!(
                "  after {} pass(es) the result still reads as hard-wrapped, so the",
                outcome.passes
            );
            eprintln!("  transform cannot reconcile its own output. The file was left");
            eprintln!("  untouched — writing it would produce a file that flags forever.");
        }
        std::process::exit(1);
    }

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
        // Same posture as the structural check below: a finding, not a crash.
        // Words would have been lost, so the file is left alone and the wrapped
        // prose stays flagged for every later caller.
        if json {
            println!(
                "{{\"wrapped\":true,\"repaired\":false,\"reason\":\"tokens-changed\",\
                 \"before\":{before},\"after\":{after}}}"
            );
        } else if !quiet {
            eprintln!();
            eprintln!("NOT repaired: token stream would change ({before} -> {after}).");
            eprintln!("  Words would be lost. The file was left untouched — this is a bug");
            eprintln!("  in the reflow transform and is worth reporting with the file.");
        }
        std::process::exit(1);
    }

    // The structural post-condition, and the one actually worth trusting: reparse
    // and compare. Unlike the classifier it does not depend on having enumerated
    // markdown's constructs correctly.
    //
    // A trip does NOT abort. It is the most informative result this tool
    // produces — it names a construct the classifier misreads — so it is
    // reported, the file is left alone, and the exit code stays 1: the wrapped
    // prose is still there, unrepaired, and every later caller (lint, CI, a
    // sweep over the corpus) keeps flagging it. Refusing silently, or failing
    // hard enough to stop a batch, would lose the finding instead of recording it.
    if let Some((idx, before_ev, after_ev)) = reflow::structure_diff(&text, &repaired) {
        let label = match &source {
            Source::File(p) => p.display().to_string(),
            Source::Stdin => "<stdin>".to_string(),
        };
        if json {
            println!(
                "{{\"wrapped\":true,\"repaired\":false,\"reason\":\"structure-changed\",\
                 \"event\":{idx},\"before\":{},\"after\":{}}}",
                json_str(&before_ev),
                json_str(&after_ev)
            );
        } else if !quiet {
            eprintln!();
            eprintln!("NOT repaired: {label}");
            eprintln!("  reflowing this file would change its structure, not just its line breaks,");
            eprintln!("  so it was left untouched. This means it contains a construct the");
            eprintln!("  classifier read as ordinary prose — that is a bug worth reporting.");
            eprintln!("  first divergence at parse event {idx}:");
            eprintln!("    before: {}", truncate(&before_ev));
            eprintln!("    after:  {}", truncate(&after_ev));
        }
        // Still wrapped, still unfixed — the finding stands.
        std::process::exit(1);
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

fn truncate(s: &str) -> String {
    if s.chars().count() <= 90 {
        return s.to_string();
    }
    format!("{}…", s.chars().take(90).collect::<String>())
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
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
