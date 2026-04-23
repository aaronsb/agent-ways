//! Memory seeding per ADR-128.
//!
//! Claude Code's auto-memory feature loads `MEMORY.md` unconditionally at
//! session start. Left to its own devices, it accumulates as a silo that
//! short-circuits the friction-enforcing artifacts (ways, ADRs, notes,
//! issues, PRs, commit messages) where project knowledge actually belongs.
//!
//! This module seeds `MEMORY.md` with routing guidance and detects drift
//! (from direct edits, or from Anthropic's periodic memory compaction).
//! On drift, the previous content is preserved as a dated diff file and
//! the seed is rewritten. A review prompt is emitted to stdout so the
//! SessionStart hook surfaces it to the session.
//!
//! **Integrity check**: the canonical seed body is embedded in the binary
//! via `include_str!` at compile time. Verification is byte comparison of
//! the extracted body against the embedded constant — no hashing required.
//! The binary itself is the tamper-evident baseline.

use anyhow::Result;
use std::path::{Path, PathBuf};

const SEED_VERSION: u32 = 1;
const SEED_ID: &str = "claude-code-memory";
const USER_CONTEXT_MARKER: &str = "## User Context";
const USER_CONTEXT_STUB: &str = "<!-- user / feedback / reference entries only -->";

/// Canonical seed body for `seed-version: 1`. The binary physically contains
/// this string; integrity of `MEMORY.md`'s seeded portion is checked by byte
/// comparison against it.
const SEED_BODY_V1: &str = include_str!("../../../../hooks/memory-seed/seed-v1.md");

/// Canonical body trimmed of leading/trailing newlines. Using this in both
/// the write path and the verification path means the round-trip through
/// `write_seed` → parse is byte-exact regardless of how the embedded file
/// ships (with or without trailing newline).
fn canonical_body() -> &'static str {
    SEED_BODY_V1.trim_matches('\n')
}

/// Seed or verify `MEMORY.md` for the given project. Idempotent: returns
/// quietly when the seed is intact; only emits stdout when drift is
/// detected (so the SessionStart pipeline doesn't get noisy on every run).
pub fn apply(project_dir: &str) -> Result<()> {
    let memory_dir = project_memory_dir(project_dir);
    let memory_file = memory_dir.join("MEMORY.md");

    std::fs::create_dir_all(&memory_dir)?;

    if !memory_file.is_file() {
        write_seed(&memory_file, None)?;
        return Ok(());
    }

    let content = std::fs::read_to_string(&memory_file)?;
    let parsed = parse_seeded_memory(&content);

    if let Some(p) = &parsed {
        if p.frontmatter_seed.as_deref() == Some(SEED_ID)
            && p.frontmatter_version == Some(SEED_VERSION)
            && p.body_bytes == canonical_body()
        {
            return Ok(()); // seed intact
        }
    }

    // Drift detected: save diff, re-seed (preserving User Context if present).
    let diff_path = write_drift_diff(&memory_dir, &content)?;
    let preserved = parsed.and_then(|p| {
        if p.user_context.trim().is_empty() {
            None
        } else {
            Some(p.user_context)
        }
    });
    write_seed(&memory_file, preserved.as_deref())?;

    emit_review_prompt(&memory_file, &diff_path);
    Ok(())
}

fn project_memory_dir(project_dir: &str) -> PathBuf {
    let normalized: String = project_dir
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".claude/projects")
        .join(normalized)
        .join("memory")
}

fn write_seed(path: &Path, user_context: Option<&str>) -> Result<()> {
    // `trim()` (both ends) — parsed user context begins with a blank line
    // left over from the previous file's `\n\n{marker}\n\n{user}` layout.
    // Trimming both ends lets the format string insert exactly one blank
    // line before the content regardless of the input shape.
    let user_body = user_context.unwrap_or(USER_CONTEXT_STUB).trim();
    let content = format!(
        "---\nseed: {seed}\nseed-version: {ver}\n---\n\n{body}\n\n{marker}\n\n{user}\n",
        seed = SEED_ID,
        ver = SEED_VERSION,
        body = canonical_body(),
        marker = USER_CONTEXT_MARKER,
        user = user_body,
    );
    std::fs::write(path, content)?;
    Ok(())
}

struct ParsedSeededMemory {
    frontmatter_seed: Option<String>,
    frontmatter_version: Option<u32>,
    /// Body region between frontmatter and `## User Context`, trimmed of
    /// leading/trailing newlines. Byte-compared against `canonical_body()`.
    body_bytes: String,
    /// Raw content from the line after `## User Context` to end of file.
    user_context: String,
}

fn parse_seeded_memory(content: &str) -> Option<ParsedSeededMemory> {
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }

    let mut frontmatter = Vec::new();
    for line in &mut lines {
        if line == "---" {
            break;
        }
        frontmatter.push(line);
    }

    let mut body_lines: Vec<&str> = Vec::new();
    let mut user_context_lines: Vec<&str> = Vec::new();
    let mut in_user_context = false;
    for line in lines {
        if !in_user_context && line == USER_CONTEXT_MARKER {
            in_user_context = true;
            continue;
        }
        if in_user_context {
            user_context_lines.push(line);
        } else {
            body_lines.push(line);
        }
    }

    let body_joined = body_lines.join("\n");
    let body_bytes = body_joined.trim_matches('\n').to_string();

    Some(ParsedSeededMemory {
        frontmatter_seed: frontmatter_value(&frontmatter, "seed"),
        frontmatter_version: frontmatter_value(&frontmatter, "seed-version")
            .and_then(|v| v.parse().ok()),
        body_bytes,
        user_context: user_context_lines.join("\n"),
    })
}

fn frontmatter_value(frontmatter: &[&str], key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    frontmatter.iter().find_map(|line| {
        line.strip_prefix(&prefix)
            .map(|rest| rest.trim().to_string())
    })
}

fn write_drift_diff(memory_dir: &Path, current: &str) -> Result<PathBuf> {
    let today = today_date();
    let serial = next_serial_for_date(memory_dir, &today);
    let path = memory_dir.join(format!("MEMORY.diff.{today}.{serial:03}.md"));

    let content = format!(
        "# Memory Drift Diff — {today} #{serial:03}\n\n\
        Captured at SessionStart per ADR-128. The previous `MEMORY.md` content \
        below is preserved verbatim so entries can be triaged against the \
        routing table in `hooks/ways/meta/memory/memory.md` — convert \
        repo-relevant content to ways / ADRs / design notes / issues, discard \
        entries that don't warrant preservation, and re-add only genuine \
        cross-project user facts under `## User Context`.\n\n\
        ## Previous MEMORY.md (full content)\n\n\
        ```markdown\n{current}\n```\n\n\
        ## Canonical v{ver} seeded body (for comparison)\n\n\
        ```markdown\n{canonical}\n```\n",
        today = today,
        serial = serial,
        current = current.trim_end(),
        ver = SEED_VERSION,
        canonical = canonical_body(),
    );
    std::fs::write(&path, content)?;
    Ok(path)
}

fn today_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Calendar date in UTC — avoids bringing in chrono for this one call.
    let days = secs / 86_400;
    let (y, m, d) = days_to_ymd(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Convert days since 1970-01-01 UTC to (year, month, day). Public-domain
/// algorithm (Howard Hinnant's date routines).
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn next_serial_for_date(memory_dir: &Path, today: &str) -> u32 {
    let prefix = format!("MEMORY.diff.{today}.");
    let count = std::fs::read_dir(memory_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .map(|n| n.starts_with(&prefix) && n.ends_with(".md"))
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);
    (count as u32) + 1
}

fn emit_review_prompt(memory_path: &Path, diff_path: &Path) {
    println!();
    println!("⚠️  **Memory seed drift detected.** `MEMORY.md` was modified since");
    println!("the seed was last applied — likely by Claude Code's periodic memory");
    println!("compaction, or by direct edit. A diff has been preserved at:");
    println!();
    println!("    {}", diff_path.display());
    println!();
    println!("Per ADR-128, triage the diff before substantive work. Apply the");
    println!("routing table in `hooks/ways/meta/memory/memory.md` to each entry —");
    println!("convert repo-relevant content to ways/ADRs/design notes/issues,");
    println!("discard entries that don't warrant preservation, and re-add only");
    println!("genuine cross-project user facts under `## User Context` in");
    println!("{}.", memory_path.display());
    println!();
    println!("The seed has been rewritten to its canonical state; any");
    println!("`## User Context` content below the marker was preserved.");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_body_is_nonempty() {
        assert!(!SEED_BODY_V1.is_empty());
        assert!(!canonical_body().is_empty());
        // Canonical form has no leading/trailing newlines — prevents hash-like
        // drift from trailing-newline-only edits of the source file.
        assert!(!canonical_body().starts_with('\n'));
        assert!(!canonical_body().ends_with('\n'));
    }

    #[test]
    fn parse_matches_freshly_written_seed() {
        let dir = std::env::temp_dir().join(format!("ways-seed-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("MEMORY.md");

        write_seed(&path, None).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed = parse_seeded_memory(&content).expect("should parse");

        assert_eq!(parsed.frontmatter_seed.as_deref(), Some(SEED_ID));
        assert_eq!(parsed.frontmatter_version, Some(SEED_VERSION));
        assert_eq!(parsed.body_bytes, canonical_body());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_returns_none_for_non_seed_markdown() {
        let content = "# Not a seed file\n\nJust a regular markdown doc.\n";
        assert!(parse_seeded_memory(content).is_none());
    }

    #[test]
    fn reseed_normalizes_user_context_spacing() {
        // Regression: a preserved user_context captured with a leading
        // blank line would compound with the format's `\n\n{user}` pattern
        // and produce two blank lines between `## User Context` and the
        // first entry. `trim()` on the user_body prevents it.
        let dir = std::env::temp_dir().join(format!("ways-seed-spacing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("MEMORY.md");

        // Simulate the shape produced by a round-trip: user_context starts
        // with a stray newline from the original file.
        let with_leading_blank = "\n- Entry one.\n- Entry two.";
        write_seed(&path, Some(with_leading_blank)).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();

        // Exactly one blank line between the marker and the first entry.
        assert!(after.contains(&format!("{USER_CONTEXT_MARKER}\n\n- Entry one.\n")));
        assert!(!after.contains(&format!("{USER_CONTEXT_MARKER}\n\n\n")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_context_preserved_across_reseed() {
        let dir = std::env::temp_dir().join(format!("ways-seed-test2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("MEMORY.md");

        // Write a "tampered" seed with user-context content below the marker.
        let user_entries = "- Some user memory about their workflow.\n- Another entry.";
        let tampered = format!(
            "---\nseed: {SEED_ID}\nseed-version: 1\n---\n\n# Tampered body\n\n{USER_CONTEXT_MARKER}\n\n{user_entries}\n"
        );
        std::fs::write(&path, tampered).unwrap();

        // Simulate the apply logic: parse, detect drift, re-seed preserving
        // the user context region.
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed = parse_seeded_memory(&content).unwrap();
        assert_ne!(parsed.body_bytes, canonical_body(), "setup expects drift");
        write_seed(&path, Some(&parsed.user_context)).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains(user_entries), "user context lost");
        assert!(after.contains(canonical_body()), "canonical body not written");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn days_to_ymd_known_dates() {
        // Anchors: 1970-01-01 = day 0, 2000-01-01 = day 10957 (common Unix check).
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        assert_eq!(days_to_ymd(10_957), (2000, 1, 1));
        // 2026-04-22 = 10957 + 26*365 + 7 (leap days 2000..=2024) + 31+28+31+21
        //            = 10957 + 9490 + 7 + 111 = 20565
        assert_eq!(days_to_ymd(20_565), (2026, 4, 22));
    }
}
