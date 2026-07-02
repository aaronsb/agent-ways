//! Shared helpers for the ways-audit report modules.
//!
//! Query-side helpers that operate on the claim manifest. The firing-log reader
//! (`load_events` / `count_fires`) lives in `ways_core::firing`; the calendar
//! conversion lives in `ways_core::util` — both shared engine, not audit-only.

use serde_json::Value;

pub fn obj_len(v: &Value) -> usize {
    v.as_object().map(|m| m.len()).unwrap_or(0)
}

pub fn cutoff_date(days: u32) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff_secs = secs.saturating_sub(days as u64 * 86400);
    let days_since = cutoff_secs / 86400;
    let (y, m, d) = ways_core::util::days_to_ymd(days_since);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn find_stale_ways(manifest: &Value, days: u32) -> Vec<String> {
    let cutoff = cutoff_date(days);
    let mut stale = Vec::new();

    if let Some(ways) = manifest["ways"].as_object() {
        for (way_id, data) in ways {
            if let Some(verified) = data["provenance"]["verified"].as_str() {
                if verified < cutoff.as_str() {
                    stale.push(way_id.clone());
                }
            }
        }
    }
    stale.sort();
    stale
}

pub fn find_incomplete(manifest: &Value) -> Vec<String> {
    let mut incomplete = Vec::new();

    if let Some(ways) = manifest["ways"].as_object() {
        for (way_id, data) in ways {
            let prov = &data["provenance"];
            if prov.is_null() {
                continue;
            }
            let missing_policy = prov["policy"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true);
            let missing_controls = prov["controls"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true);
            let missing_rationale = prov["rationale"].as_str().is_none();

            if missing_policy || missing_controls || missing_rationale {
                incomplete.push(way_id.clone());
            }
        }
    }
    incomplete.sort();
    incomplete
}

/// Detect project-local ways directory from CLAUDE_PROJECT_DIR or cwd.
pub fn detect_project_ways() -> Option<String> {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .or_else(|| {
            let cwd = std::env::current_dir().ok()?;
            let mut dir = cwd.as_path();
            loop {
                let claude_dir = dir.join(".claude");
                if claude_dir.is_dir()
                    && (claude_dir.join("settings.json").exists()
                        || dir.join("CLAUDE.md").exists()
                        || claude_dir.join("settings.local.json").exists())
                {
                    return Some(dir.to_string_lossy().to_string());
                }
                dir = dir.parent()?;
            }
        })?;

    let project_ways = std::path::PathBuf::from(&project_dir).join(".claude/ways");
    if project_ways.is_dir() {
        Some(project_ways.to_string_lossy().to_string())
    } else {
        None // No project-local ways, fall through to global
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cutoff_date_is_iso_formatted() {
        let d = cutoff_date(0);
        assert_eq!(d.len(), 10);
        assert_eq!(d.as_bytes()[4], b'-');
        assert_eq!(d.as_bytes()[7], b'-');
    }

    #[test]
    fn find_incomplete_flags_missing_fields() {
        let manifest = json!({
            "ways": {
                "a/complete": {"provenance": {
                    "policy": [{"uri": "p.md"}],
                    "controls": [{"id": "X"}],
                    "rationale": "why"
                }},
                "a/no-rationale": {"provenance": {
                    "policy": [{"uri": "p.md"}],
                    "controls": [{"id": "X"}]
                }},
                "a/no-claim": {"provenance": null}
            }
        });
        let incomplete = find_incomplete(&manifest);
        assert_eq!(incomplete, vec!["a/no-rationale"]);
    }

    #[test]
    fn find_stale_ways_respects_cutoff() {
        let manifest = json!({
            "ways": {
                "a/old": {"provenance": {"verified": "2000-01-01"}},
                "a/future": {"provenance": {"verified": "2999-01-01"}}
            }
        });
        let stale = find_stale_ways(&manifest, 90);
        assert_eq!(stale, vec!["a/old"]);
    }
}
