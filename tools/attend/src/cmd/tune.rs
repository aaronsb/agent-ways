//! `attend tune` — survey session history and derive engagement config.

use crate::config;

pub(crate) fn cmd_tune(apply: bool) {
    let home = std::env::var("HOME").unwrap_or_default();
    let projects_root = std::path::PathBuf::from(&home).join(".claude").join("projects");

    // Gather the 10 most-recently-modified project directories.
    let mut proj_dirs: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&projects_root) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    let mt = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    proj_dirs.push((entry.path(), mt));
                }
            }
        }
    }
    proj_dirs.sort_by(|a, b| b.1.cmp(&a.1));
    proj_dirs.truncate(10);

    // For each project, take the 5 most-recent .jsonl files.
    let mut sessions: Vec<std::path::PathBuf> = Vec::new();
    for (proj, _) in &proj_dirs {
        let mut in_proj: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(proj) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let mt = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                in_proj.push((path, mt));
            }
        }
        in_proj.sort_by(|a, b| b.1.cmp(&a.1));
        in_proj.truncate(5);
        sessions.extend(in_proj.into_iter().map(|(p, _)| p));
    }

    eprintln!(
        "[tune] surveying {} sessions across {} projects",
        sessions.len(),
        proj_dirs.len()
    );

    let mut a2u_gaps: Vec<f64> = Vec::new();
    let mut u2u_gaps: Vec<f64> = Vec::new();

    for session in &sessions {
        let content = match std::fs::read_to_string(session) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Parse to (timestamp_secs, kind) where kind is 0=user, 1=assistant.
        //
        // Claude Code's JSONL format has the top-level `"type"` field AFTER
        // a nested `"message"` object, and that nested object contains its
        // own `"type":"message"` marker. A naive first-match extractor for
        // `"type"` picks the wrong one. We match the top-level discriminators
        // directly instead.
        let mut events: Vec<(f64, u8)> = Vec::new();
        for line in content.lines() {
            let is_assistant = line.contains("\"type\":\"assistant\"");
            let is_user = line.contains("\"type\":\"user\"");
            if !is_assistant && !is_user {
                continue;
            }

            let kind: u8 = if is_assistant {
                1
            } else {
                // user — skip tool_result entries (mechanical, not a real turn)
                if line.contains("\"type\":\"tool_result\"") {
                    continue;
                }
                0
            };

            let Some(ts_str) = extract_json_str(line, "timestamp") else {
                continue;
            };
            let Some(ts) = parse_iso8601(&ts_str) else {
                continue;
            };
            events.push((ts, kind));
        }

        // Walk events computing gaps
        let mut last_assistant: Option<f64> = None;
        let mut last_user: Option<f64> = None;
        for (ts, kind) in &events {
            if *kind == 0 {
                // user
                if let Some(la) = last_assistant {
                    let gap = ts - la;
                    if gap > 0.0 && gap < 7200.0 {
                        a2u_gaps.push(gap);
                    }
                    last_assistant = None;
                }
                if let Some(lu) = last_user {
                    let gap = ts - lu;
                    if gap > 1.0 && gap < 7200.0 {
                        u2u_gaps.push(gap);
                    }
                }
                last_user = Some(*ts);
            } else {
                last_assistant = Some(*ts);
            }
        }
    }

    if u2u_gaps.is_empty() {
        eprintln!("[tune] no session data found — keeping defaults");
        return;
    }

    let pct = |data: &[f64], p: f64| -> f64 {
        let mut sorted: Vec<f64> = data.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = (((sorted.len() as f64 - 1.0) * p).round() as usize).min(sorted.len() - 1);
        sorted[idx]
    };

    let a2u_median = pct(&a2u_gaps, 0.5);
    let a2u_p75 = pct(&a2u_gaps, 0.75);
    let a2u_p90 = pct(&a2u_gaps, 0.90);
    let u2u_median = pct(&u2u_gaps, 0.5);
    let u2u_p75 = pct(&u2u_gaps, 0.75);
    let u2u_p90 = pct(&u2u_gaps, 0.90);

    // Derive engagement config from percentiles.
    //
    // burst_window: p90 of the full turn cycle × burst_threshold. This is
    //   the window in which "3 turn cycles" would typically complete.
    //   Clamped to at least 5 minutes so very fast sessions still have a
    //   reasonable floor.
    //
    // absolute_refractory: median assistant→user gap (one "think time"
    //   pause). This is how long the other side typically takes to respond,
    //   so blocking disclosures for that long forces a natural beat.
    //
    // decay_per_minute: chosen so peak multiplier (2.25 at burst 3) decays
    //   back to rest (1.0) over ~2× burst_window minutes. That keeps the
    //   refractory in effect for roughly twice as long as the conversation
    //   that triggered it.
    //
    // peer_activity_window: same as burst_window.

    let burst_threshold = 3.0_f64;
    let step_multiplier = 1.25_f64;
    let peak_multiplier = 1.0 + (1.0 * step_multiplier); // peak at exactly burst_threshold

    let burst_window_s = (u2u_p90 * burst_threshold).clamp(300.0, 3600.0) as u64;
    let abs_refractory_s = a2u_median.clamp(15.0, 300.0) as u64;
    let burst_window_min = burst_window_s as f64 / 60.0;
    let decay_per_minute = (peak_multiplier - 1.0) / (2.0 * burst_window_min);

    println!();
    println!("=== attend tune — session survey ===");
    println!("  projects surveyed:  {}", proj_dirs.len());
    println!("  sessions parsed:    {}", sessions.len());
    println!("  turn samples:       {}", u2u_gaps.len());
    println!();
    println!("  assistant → user (think time):");
    println!("    median={:.0}s  p75={:.0}s  p90={:.0}s", a2u_median, a2u_p75, a2u_p90);
    println!("  user → user (full cycle):");
    println!("    median={:.0}s  p75={:.0}s  p90={:.0}s", u2u_median, u2u_p75, u2u_p90);
    println!();
    println!("=== derived engagement config ===");
    println!("engagement:");
    println!("  burst_threshold: {}", burst_threshold as usize);
    println!("  step_multiplier: {}", step_multiplier);
    println!("  absolute_refractory: {}     # median think time", abs_refractory_s);
    println!(
        "  decay_per_minute: {:.4}     # peak decays over ~2× burst-window equivalent",
        decay_per_minute
    );
    println!(
        "  peer_activity_window: {}    # sized from u2u p90 × burst_threshold",
        burst_window_s
    );
    println!();

    if apply {
        match apply_engagement_tune(burst_window_s, abs_refractory_s, decay_per_minute) {
            Ok(path) => println!("[tune] wrote updated engagement section to {}", path.display()),
            Err(e) => eprintln!("[tune] error writing config: {}", e),
        }
    } else {
        println!("(pass --apply to write these values to your attend config)");
    }
}

fn apply_engagement_tune(
    peer_activity_window_s: u64,
    abs_refractory_s: u64,
    decay_per_minute: f64,
) -> std::io::Result<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", home));
    let path = std::path::PathBuf::from(config_dir)
        .join("attend")
        .join("config.yaml");

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let existing =
        std::fs::read_to_string(&path).unwrap_or_else(|_| config::Config::default_yaml());

    let new_section = format!(
        "engagement:\n  burst_threshold: 3\n  step_multiplier: 1.25\n  absolute_refractory: {}\n  decay_per_minute: {:.4}\n  peer_activity_window: {}\n",
        abs_refractory_s, decay_per_minute, peer_activity_window_s,
    );

    let updated = replace_engagement_section(&existing, &new_section);
    std::fs::write(&path, updated)?;
    Ok(path)
}

/// Replace (or insert) the `engagement:` section in a YAML config string.
fn replace_engagement_section(existing: &str, new_section: &str) -> String {
    let mut result = String::new();
    let mut skipping = false;
    let mut found = false;

    for line in existing.lines() {
        let is_top_level =
            !line.is_empty() && !line.starts_with(' ') && !line.starts_with('\t');

        if is_top_level && line.starts_with("engagement:") {
            skipping = true;
            found = true;
            result.push_str(new_section);
            continue;
        }

        if skipping {
            // Stay in skip mode until we hit another top-level, non-comment line.
            if is_top_level && !line.starts_with('#') {
                skipping = false;
                // fall through to emit this line
            } else {
                continue;
            }
        }

        result.push_str(line);
        result.push('\n');
    }

    if !found {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(new_section);
    }

    result
}

/// Extract a "key":"value" string from a single JSON line (naive, fast).
fn extract_json_str(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let mut end = None;
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            end = Some(i);
            break;
        }
        i += 1;
    }
    Some(rest[..end?].to_string())
}

/// Parse an ISO 8601 timestamp (YYYY-MM-DDTHH:MM:SS[.fff][Z|±HH:MM])
/// into seconds since the Unix epoch. Assumes UTC if a Z suffix or no
/// offset is present.
fn parse_iso8601(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let year: i32 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: u32 = s.get(11..13)?.parse().ok()?;
    let minute: u32 = s.get(14..16)?.parse().ok()?;
    let second: u32 = s.get(17..19)?.parse().ok()?;

    let mut fraction: f64 = 0.0;
    if s.len() > 20 && s.as_bytes()[19] == b'.' {
        let rest = &s[20..];
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        let frac_str = &rest[..end];
        if !frac_str.is_empty() {
            if let Ok(v) = frac_str.parse::<f64>() {
                fraction = v / 10f64.powi(frac_str.len() as i32);
            }
        }
    }

    let days = days_from_civil(year, month, day);
    let seconds =
        days * 86400 + (hour as i64) * 3600 + (minute as i64) * 60 + (second as i64);
    Some(seconds as f64 + fraction)
}

/// Days since 1970-01-01 (UTC) for a given civil date.
/// Howard Hinnant's algorithm — exact, no dependencies.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u32;
    let m_adj = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era as i64) * 146097 + (doe as i64) - 719468
}
