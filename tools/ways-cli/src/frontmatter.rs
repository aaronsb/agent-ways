use anyhow::{anyhow, Context, Result};
use sensor_trait::Curve;
use serde::Deserialize;
use std::path::Path;

/// ADR-126 refire specification: either a numeric fraction of the session
/// context window, or a preset name resolved via the config's
/// `refire_presets` table at fire-evaluation time.
///
/// Untagged deserialization: a YAML scalar parses as `Numeric` if it's a
/// number, otherwise as `Preset`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RefireSpec {
    /// Explicit fraction of the context window. `0.2` means "half-life =
    /// 20% of the current session's context window." Pinned to today's
    /// model by the author's choice to write a number.
    Numeric(f64),
    /// Preset name (e.g., `rare`, `normal`). Resolved per-fire against the
    /// `refire_presets` section of the config file, so operators can re-tune
    /// the whole tree with a single config edit.
    Preset(String),
}

impl RefireSpec {
    /// Resolve to the concrete fraction for this spec. Preset names look up
    /// the active config; unknown names fall back to the built-in `normal`
    /// value and log a stderr warning (fail-soft at fire time; `ways lint`
    /// catches typos at parse time — see ADR-126 Frontmatter/lint changes).
    pub fn fraction(&self) -> f64 {
        match self {
            Self::Numeric(v) => *v,
            Self::Preset(name) => {
                let cfg = crate::config::global();
                match cfg.refire_presets.get(name) {
                    Some(v) => *v,
                    None => {
                        eprintln!(
                            "[ways] unknown refire preset `{}`; falling back to `normal` (0.15)",
                            name
                        );
                        0.15
                    }
                }
            }
        }
    }

    /// Resolve to a concrete `Curve::Exponential` given the session's
    /// context window. Half-life = `fraction × window` (clamped to at least 1
    /// to avoid a zero-half-life degenerate that would cause immediate
    /// re-fire on every check).
    pub fn to_curve(&self, window: u64) -> Curve {
        let half_life = (self.fraction() * window as f64).round() as u64;
        Curve::Exponential {
            half_life: half_life.max(1),
        }
    }
}

/// Parsed YAML frontmatter from a way file.
#[derive(Debug, Deserialize, Default)]
pub struct Frontmatter {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub vocabulary: Option<String>,
    #[serde(default)]
    #[allow(dead_code)] // parsed for serde compat, accessed via scan's own scope field
    pub scope: Option<String>,
    #[serde(default)]
    pub embed_threshold: Option<f64>,
    /// ADR-123 firing-dynamics curve. Required for ways that fire
    /// predictively or reactively; optional during parse for static
    /// consumers (tune/corpus/graph) that don't invoke the engine.
    #[serde(default)]
    pub curve: Option<Curve>,
    /// ADR-126 window-relative refire. When present, takes precedence over
    /// `curve:` at fire-evaluation time (via `resolved_curve`). Holds the
    /// unresolved spec so config edits take effect mid-session — resolution
    /// happens per fire against the then-current window and preset table.
    #[serde(default)]
    pub refire: Option<RefireSpec>,
}

impl Frontmatter {
    /// Resolve the effective curve for fire evaluation, given the session's
    /// current context window. Precedence (ADR-126): `refire:` wins over
    /// `curve:` when both are present (lint warns about the duplication).
    ///
    /// Returns `None` when neither field is set — static consumers like
    /// `ways tune` and `ways corpus` that don't invoke the engine can still
    /// parse a way file without requiring either.
    pub fn resolved_curve(&self, window: u64) -> Option<Curve> {
        if let Some(spec) = &self.refire {
            return Some(spec.to_curve(window));
        }
        self.curve.clone()
    }
}

/// Extract YAML frontmatter from a way file.
pub fn parse(path: &Path) -> Result<Frontmatter> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;

    let yaml_str = extract_frontmatter_str(&content)
        .with_context(|| format!("no frontmatter in {}", path.display()))?;

    detect_legacy_redisclose(&yaml_str)
        .with_context(|| format!("legacy frontmatter in {}", path.display()))?;

    serde_yaml::from_str(&yaml_str)
        .with_context(|| format!("parsing frontmatter in {}", path.display()))
}

/// Extract the raw YAML string between `---` delimiters.
fn extract_frontmatter_str(content: &str) -> Option<String> {
    let mut lines = content.lines();

    if lines.next()? != "---" {
        return None;
    }

    let mut yaml_lines = Vec::new();
    for line in lines {
        if line == "---" {
            return Some(yaml_lines.join("\n"));
        }
        yaml_lines.push(line);
    }
    None
}

/// Scan raw frontmatter YAML for a top-level `redisclose:` field.
/// Returns a migration-pointing error if present. Called from `parse`
/// so any leftover legacy field errors loudly at load time (ADR-123 C3).
pub fn detect_legacy_redisclose(yaml_str: &str) -> Result<()> {
    for line in yaml_str.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("redisclose:") {
            return Err(anyhow!(
                "legacy `redisclose:` field is no longer supported — \
                migrate to an explicit `curve:` block per ADR-123. See \
                docs/architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md \
                for migration guidance."
            ));
        }
    }
    Ok(())
}

/// A single locale entry from a .locales.jsonl file.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct LocaleEntry {
    pub lang: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocabulary: Option<String>,
}

/// Parse a .locales.jsonl file into locale entries.
pub fn parse_locales_jsonl(path: &Path) -> Result<Vec<LocaleEntry>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let mut entries = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: LocaleEntry = serde_json::from_str(line)
            .with_context(|| format!("parsing locale entry in {}", path.display()))?;
        entries.push(entry);
    }
    Ok(entries)
}

/// Extract the `<!-- epistemic: VALUE -->` comment from the body of a way file.
pub fn extract_epistemic(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<!-- epistemic:") {
            if let Some(value) = rest.strip_suffix("-->") {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// Extract See Also references from the body of a way file.
/// Returns (target_name, target_domain, label) tuples.
pub fn extract_see_also(content: &str) -> Vec<(String, String, String)> {
    let mut refs = Vec::new();
    let mut in_see_also = false;

    for line in content.lines() {
        if line.starts_with("## See Also") {
            in_see_also = true;
            continue;
        }
        if in_see_also && line.starts_with("## ") {
            break;
        }
        if in_see_also && line.starts_with("- ") {
            if let Some(parsed) = parse_see_also_line(line) {
                refs.push(parsed);
            }
        }
    }
    refs
}

/// Parse a See Also line like `- code/testing(softwaredev) — quality requires test coverage`
fn parse_see_also_line(line: &str) -> Option<(String, String, String)> {
    let line = line.strip_prefix("- ")?;

    let paren_open = line.find('(')?;
    let paren_close = line.find(')')?;

    let name = line[..paren_open].trim().to_string();
    let domain = line[paren_open + 1..paren_close].trim().to_string();

    let label = line[paren_close + 1..]
        .trim()
        .strip_prefix('\u{2014}') // em dash
        .or_else(|| line[paren_close + 1..].trim().strip_prefix("--"))
        .unwrap_or("")
        .trim()
        .to_string();

    Some((name, domain, label))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_yaml(yaml: &str) -> Frontmatter {
        serde_yaml::from_str(yaml).expect("frontmatter parse failed")
    }

    #[test]
    fn parses_curve_exponential() {
        let fm = parse_yaml(
            r#"
description: test way
curve:
  type: Exponential
  half_life: 50000
"#,
        );
        match fm.curve {
            Some(Curve::Exponential { half_life }) => assert_eq!(half_life, 50_000),
            other => panic!("expected Exponential, got {:?}", other),
        }
    }

    #[test]
    fn parses_curve_action_potential() {
        let fm = parse_yaml(
            r#"
description: test way
curve:
  type: ActionPotential
  burst_threshold: 3
  peak_multiplier: 2.0
  absolute_refractory: 5000
  multiplier_half_life: 25000
"#,
        );
        match fm.curve {
            Some(Curve::ActionPotential {
                burst_threshold,
                peak_multiplier,
                absolute_refractory,
                multiplier_half_life,
            }) => {
                assert_eq!(burst_threshold, 3);
                assert!((peak_multiplier - 2.0).abs() < 1e-9);
                assert_eq!(absolute_refractory, 5_000);
                assert_eq!(multiplier_half_life, 25_000);
            }
            other => panic!("expected ActionPotential, got {:?}", other),
        }
    }

    #[test]
    fn parses_curve_progressive_staircase() {
        let fm = parse_yaml(
            r#"
description: test way
curve:
  type: ProgressiveStaircase
  steps:
    - [0, 1.0]
    - [15000, 0.5]
    - [40000, 0.2]
"#,
        );
        match fm.curve {
            Some(Curve::ProgressiveStaircase { steps }) => {
                assert_eq!(steps.len(), 3);
                assert_eq!(steps[0], (0, 1.0));
                assert_eq!(steps[1], (15_000, 0.5));
                assert_eq!(steps[2], (40_000, 0.2));
            }
            other => panic!("expected ProgressiveStaircase, got {:?}", other),
        }
    }

    #[test]
    fn parses_curve_flat() {
        let fm = parse_yaml(
            r#"
description: test way
curve:
  type: Flat
  suppression: 15000
"#,
        );
        match fm.curve {
            Some(Curve::Flat { suppression }) => assert_eq!(suppression, 15_000),
            other => panic!("expected Flat, got {:?}", other),
        }
    }

    #[test]
    fn curve_field_is_optional_for_static_consumers() {
        // Static consumers like `ways tune` and `ways corpus` parse way
        // frontmatter but don't invoke the firing engine, so a missing
        // curve: block must not error at parse time. The engine path in
        // session.rs enforces presence at the fire site.
        let fm = parse_yaml("description: no curve\n");
        assert!(fm.curve.is_none());
        assert!(fm.refire.is_none());
    }

    #[test]
    fn parses_refire_numeric() {
        let fm = parse_yaml("description: test\nrefire: 0.2\n");
        match fm.refire {
            Some(RefireSpec::Numeric(v)) => assert!((v - 0.2).abs() < 1e-9),
            other => panic!("expected Numeric(0.2), got {:?}", other),
        }
    }

    #[test]
    fn parses_refire_preset_name() {
        let fm = parse_yaml("description: test\nrefire: rare\n");
        match fm.refire {
            Some(RefireSpec::Preset(name)) => assert_eq!(name, "rare"),
            other => panic!("expected Preset(\"rare\"), got {:?}", other),
        }
    }

    #[test]
    fn refire_numeric_resolves_to_exponential_at_window() {
        // 0.2 of a 1M window = 200k half-life.
        let fm = parse_yaml("description: test\nrefire: 0.2\n");
        let curve = fm.resolved_curve(1_000_000).expect("should resolve");
        match curve {
            Curve::Exponential { half_life } => assert_eq!(half_life, 200_000),
            other => panic!("expected Exponential, got {:?}", other),
        }
    }

    #[test]
    fn refire_numeric_scales_with_window() {
        // Same fraction on a 200k window = 40k half-life.
        let fm = parse_yaml("description: test\nrefire: 0.2\n");
        let curve = fm.resolved_curve(200_000).expect("should resolve");
        match curve {
            Curve::Exponential { half_life } => assert_eq!(half_life, 40_000),
            other => panic!("expected Exponential, got {:?}", other),
        }
    }

    #[test]
    fn refire_wins_over_curve_when_both_present() {
        // ADR-126: `refire:` takes precedence over `curve:`.
        let fm = parse_yaml(
            "description: test\n\
             refire: 0.3\n\
             curve:\n  \
             type: Exponential\n  \
             half_life: 99999\n",
        );
        let curve = fm.resolved_curve(1_000_000).expect("should resolve");
        match curve {
            Curve::Exponential { half_life } => {
                // 0.3 × 1M = 300k, not the 99_999 from the `curve:` block.
                assert_eq!(half_life, 300_000);
            }
            other => panic!("expected Exponential, got {:?}", other),
        }
    }

    #[test]
    fn curve_fallback_when_no_refire() {
        // Non-Exponential shapes still live in `curve:`. If `refire:` is
        // absent, `resolved_curve` returns the raw curve untouched.
        let fm = parse_yaml(
            "description: test\n\
             curve:\n  \
             type: Flat\n  \
             suppression: 15000\n",
        );
        let curve = fm.resolved_curve(1_000_000).expect("should resolve");
        assert!(matches!(curve, Curve::Flat { suppression: 15_000 }));
    }

    #[test]
    fn resolved_curve_is_none_when_neither_field_set() {
        let fm = parse_yaml("description: static consumer only\n");
        assert!(fm.resolved_curve(1_000_000).is_none());
    }

    #[test]
    fn refire_half_life_clamped_to_one() {
        // Defensive: zero fraction → zero half_life would degenerate
        // Curve::salience_at to 0.0 at delta=0, causing immediate re-fire.
        // Clamp to 1 so the curve is well-defined even on pathological input.
        let spec = RefireSpec::Numeric(0.0);
        let curve = spec.to_curve(1_000_000);
        match curve {
            Curve::Exponential { half_life } => assert_eq!(half_life, 1),
            other => panic!("expected Exponential, got {:?}", other),
        }
    }

    #[test]
    fn detect_legacy_redisclose_flags_top_level_field() {
        let yaml = "description: test way\nredisclose: 25\n";
        let err = detect_legacy_redisclose(yaml).expect_err("should reject");
        let msg = err.to_string();
        assert!(msg.contains("redisclose"), "error message: {}", msg);
        assert!(msg.contains("curve"), "error message: {}", msg);
    }

    #[test]
    fn detect_legacy_redisclose_passes_clean_frontmatter() {
        let yaml = r#"
description: test way
curve:
  type: Exponential
  half_life: 50000
"#;
        detect_legacy_redisclose(yaml).expect("clean yaml should pass");
    }

    #[test]
    fn detect_legacy_redisclose_flags_indented_top_level() {
        // Top-level field can have leading whitespace in some yaml styles.
        let yaml = "description: test\n  redisclose: 25\n";
        let err = detect_legacy_redisclose(yaml).expect_err("should reject indented");
        assert!(err.to_string().contains("redisclose"));
    }
}
