use anyhow::{anyhow, Context, Result};
use sensor_trait::Curve;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Sensible upper bound on numeric `refire:` values. Well above any realistic
/// cadence; catches typos like `refire: 200000` (legacy raw-tokens pasted
/// into the new field).
const REFIRE_NUMERIC_MAX: f64 = 10.0;

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
    /// Resolve preset names against the supplied table. Unknown names
    /// fail-soft — fall back to the built-in `normal` value and log a
    /// stderr warning. This is the runtime safety net; `ways author lint` and
    /// `ways corpus` both reject unknown preset names upstream so fire-time
    /// typos shouldn't happen in practice.
    pub fn fraction_with(&self, presets: &HashMap<String, f64>) -> f64 {
        match self {
            Self::Numeric(v) => *v,
            Self::Preset(name) => match presets.get(name) {
                Some(v) => *v,
                None => {
                    eprintln!(
                        "[ways] unknown refire preset `{}`; falling back to `normal` (0.15). \
                        Run `ways author lint` to locate the source.",
                        name
                    );
                    0.15
                }
            },
        }
    }

    /// Resolve to a concrete `Curve::Exponential` given the session's
    /// context window. Half-life = `fraction × window` (clamped to at least 1
    /// to avoid a zero-half-life degenerate that would cause immediate
    /// re-fire on every check).
    pub fn to_curve(&self, window: u64) -> Curve {
        self.to_curve_with(window, &crate::config::global().refire_presets)
    }

    /// Same as [`to_curve`] but resolves preset names against the supplied
    /// table.
    pub fn to_curve_with(&self, window: u64, presets: &HashMap<String, f64>) -> Curve {
        let half_life = (self.fraction_with(presets) * window as f64).round() as u64;
        Curve::Exponential {
            half_life: half_life.max(1),
        }
    }

    /// Strict validation for lint and corpus-generation paths. Returns an
    /// error string (ready for a `ways author lint` ERROR line) when the spec is
    /// malformed — numeric out of sane range, or preset name not in the
    /// supplied table. Fail-closed at these upstream gates so fire-time
    /// always sees a spec that resolves cleanly.
    pub fn validate(&self, presets: &HashMap<String, f64>) -> Result<(), String> {
        match self {
            Self::Numeric(v) => {
                if !v.is_finite() {
                    return Err(format!("refire numeric {v} is not a finite number"));
                }
                if *v < 0.0 {
                    return Err(format!(
                        "refire numeric {v} is negative (fractions must be ≥ 0)"
                    ));
                }
                if *v > REFIRE_NUMERIC_MAX {
                    return Err(format!(
                        "refire numeric {v} exceeds the sane upper bound {REFIRE_NUMERIC_MAX} — \
                        values > 1.0 are valid but rare; {v} is almost certainly a raw token \
                        count accidentally pasted into the new field"
                    ));
                }
                Ok(())
            }
            Self::Preset(name) => {
                if presets.contains_key(name) {
                    Ok(())
                } else {
                    let mut valid: Vec<&String> = presets.keys().collect();
                    valid.sort();
                    Err(format!(
                        "refire preset `{name}` is not defined in config.refire_presets (valid: {valid:?})"
                    ))
                }
            }
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
    /// ADR-126 window-relative refire. Holds the unresolved spec so config
    /// edits take effect mid-session — resolution happens per fire against the
    /// then-current window and preset table (via `resolved_curve`). The legacy
    /// ADR-123 `curve:` field was retired in ADR-159; `refire:` is the sole
    /// authored cadence field.
    #[serde(default)]
    pub refire: Option<RefireSpec>,
}

impl Frontmatter {
    /// Resolve the effective curve for fire evaluation, given the session's
    /// current context window. Resolves from `refire:` (ADR-126) — the sole
    /// authored cadence field since ADR-159 retired legacy `curve:`.
    ///
    /// Returns `None` when `refire:` is unset — static consumers like
    /// `ways tune locale` and `ways corpus` that don't invoke the engine can still
    /// parse a way file without requiring it.
    pub fn resolved_curve(&self, window: u64) -> Option<Curve> {
        self.refire.as_ref().map(|spec| spec.to_curve(window))
    }
}

/// Frontmatter fields that wire a way into a runtime dispatch channel.
/// A way carrying any of these participates in firing and therefore needs
/// a `refire:` field (ADR-126). The semantic channel (description +
/// vocabulary) is checked separately by [`fires_on_something`] because it
/// requires both fields together.
///
/// **Adding a channel:** extending this list picks the new field up in
/// `ways author lint`'s fire-eligibility check automatically. The corresponding
/// dispatch entry point in `cmd/scan/` still needs wiring separately — this
/// const is the advisory declaration, not the dispatcher.
pub const FIRE_BEARING_FIELDS: &[&str] = &["pattern", "files", "commands", "trigger"];

/// Does this frontmatter wire the way into any firing channel?
/// Used by `ways author lint` to flag fire-bearing ways that lack a `refire:` field.
///
/// Operates on the raw YAML string rather than the parsed [`Frontmatter`]
/// struct because several channel fields (`pattern`, `files`, `commands`,
/// `trigger`) aren't modeled on the serde type. Keeping the predicate
/// string-based lets it run against partially-valid frontmatter during lint.
pub fn fires_on_something(fm_str: &str) -> bool {
    fn has(fm: &str, name: &str) -> bool {
        let prefix = format!("{name}:");
        fm.lines().any(|l| l.starts_with(&prefix))
    }
    let has_desc_vocab = has(fm_str, "description") && has(fm_str, "vocabulary");
    has_desc_vocab || FIRE_BEARING_FIELDS.iter().any(|f| has(fm_str, f))
}

/// Extract YAML frontmatter from a way file.
pub fn parse(path: &Path) -> Result<Frontmatter> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;

    let (yaml_str, _) = split(&content)
        .with_context(|| format!("no frontmatter in {}", path.display()))?;

    parse_str(&yaml_str)
}

/// Parse an already-extracted frontmatter YAML block. This is the single
/// strict-parse path the matching pipeline relies on; `ways author lint` calls it too
/// so the gate can't pass a way the corpus/scanner would silently reject (e.g.
/// an unquoted value containing ": ", which is invalid YAML).
pub fn parse_str(yaml_str: &str) -> Result<Frontmatter> {
    detect_legacy_redisclose(yaml_str).context("legacy frontmatter")?;
    serde_yaml::from_str(yaml_str).context("parsing frontmatter")
}

/// Like `parse`, but distinguishes "not a way" from "malformed way" so corpus
/// generation warns only on genuine breakage:
///
/// - `Ok(None)` — no `---` frontmatter block (a template/catalog/prose file that isn't a way; skip silently).
/// - `Ok(Some(_))` — frontmatter present and valid.
/// - `Err(_)` — frontmatter present but unparseable (a real defect to surface, not a silent drop).
pub fn parse_if_present(path: &Path) -> Result<Option<Frontmatter>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    match split(&content) {
        None => Ok(None),
        Some((yaml_str, _)) => parse_str(&yaml_str).map(Some),
    }
}

/// True if `content` opens with a `---` YAML frontmatter delimiter: the cheap
/// gate a walker applies before it reads a file as a way.
///
/// Uses `lines()` (which strips a trailing `\r`) so a way authored on Windows
/// with CRLF endings is recognized. A hard `content.starts_with("---\n")` check
/// fails on `---\r\n` and silently drops the file — on the scan/resolve path
/// that means the way never matches or renders. Every frontmatter gate routes
/// through here so the behavior is uniform across platforms.
pub fn opens_with_fence(content: &str) -> bool {
    content.lines().next() == Some("---")
}

/// Split a way file into its frontmatter YAML and its body. The file must open
/// with a `---` line; the YAML runs to the next `---` line and the body is
/// everything after it, so a `---` in the body (a horizontal rule) stays in the
/// body. Line ends may be `\n` or `\r\n`; the YAML comes back `\n`-joined.
/// `None` when the file has no closed frontmatter block.
pub fn split(content: &str) -> Option<(String, &str)> {
    let mut lines = content.split_inclusive('\n');
    let first = lines.next()?;
    if strip_eol(first) != "---" {
        return None;
    }
    let mut offset = first.len();
    let mut yaml_lines = Vec::new();
    for line in lines {
        offset += line.len();
        let line = strip_eol(line);
        if line == "---" {
            return Some((yaml_lines.join("\n"), &content[offset..]));
        }
        yaml_lines.push(line);
    }
    None
}

/// The markdown body after the frontmatter ([`split`]), `\n`-joined without a
/// trailing newline. Empty when there is no closed frontmatter block.
pub fn body_text(content: &str) -> String {
    split(content).map_or_else(String::new, |(_, body)| body.lines().collect::<Vec<_>>().join("\n"))
}

/// The index, in `content.lines()`, of the line that closes the frontmatter
/// block: the fence rule of [`split`] for the rewriters that edit a way file
/// line by line (`ways author lint --fix`). `None` when there is no closed block.
pub fn closing_fence_line(content: &str) -> Option<usize> {
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    lines.position(|l| l == "---").map(|i| i + 1)
}

/// The value of a top-level `name:` line in frontmatter YAML, trimmed: the
/// first such line with a non-empty value. A line scan, not a YAML parse, so it
/// reads partially-valid frontmatter (lint) and costs nothing on the hook path.
pub fn field(yaml: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    yaml.lines().find_map(|line| {
        let val = line.strip_prefix(&prefix)?.trim();
        (!val.is_empty()).then(|| val.to_string())
    })
}

/// [`field`] over a whole way file: looks only inside its closed frontmatter
/// block ([`split`]), so a body line never answers for a field.
pub fn field_in(content: &str, name: &str) -> Option<String> {
    split(content).and_then(|(yaml, _)| field(&yaml, name))
}

/// A line from `split_inclusive('\n')` without its `\n` or `\r\n`, as
/// `str::lines` would yield it.
fn strip_eol(line: &str) -> &str {
    match line.strip_suffix('\n') {
        Some(l) => l.strip_suffix('\r').unwrap_or(l),
        None => line,
    }
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
                docs/architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md \
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

/// See Also targets that name a way: `- name(domain) — label` lines where
/// `name` is a path (`code/quality`, `trust`) and `domain` a single word, with
/// the parenthesis touching the name. Returns `(name, domain)`.
///
/// A See Also section also carries references to things that are not ways:
/// skills (`develop (skill)`), subagents, ADRs, doc paths, URLs, and prose.
/// None of those has the touching `name(domain)` form with a path-shaped name,
/// so they are skipped here and left to read as plain text. The heading
/// matches `## See Also` in any letter case.
pub fn extract_way_refs(content: &str) -> Vec<(String, String)> {
    fn token(s: &str, allow_slash: bool) -> bool {
        !s.is_empty()
            && !s.starts_with('/')
            && !s.ends_with('/')
            && !s.contains("//")
            && s.chars().all(|c| {
                c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || c == '-'
                    || c == '_'
                    || (allow_slash && c == '/')
            })
    }

    let mut refs = Vec::new();
    let mut in_see_also = false;
    for line in content.lines() {
        if line.len() >= 11 && line.is_char_boundary(11) && line[..11].eq_ignore_ascii_case("## See Also") {
            in_see_also = true;
            continue;
        }
        if in_see_also && line.starts_with("## ") {
            break;
        }
        if !in_see_also {
            continue;
        }
        let Some(rest) = line.strip_prefix("- ") else { continue };
        let Some(open) = rest.find('(') else { continue };
        let Some(close) = rest[open..].find(')') else { continue };
        let name = &rest[..open];
        let domain = &rest[open + 1..open + close];
        if token(name, true) && token(domain, false) {
            refs.push((name.to_string(), domain.to_string()));
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

    #[test]
    fn way_refs_keep_only_name_domain_entries() {
        let body = "# W\n\n## See also\n\n\
            - code/quality(softwaredev) \u{2014} a way\n\
            - trust(meta) \u{2014} a domain way\n\
            - develop (skill) \u{2014} a skill\n\
            - code-reviewer (subagent, `agents/code-reviewer.md`) \u{2014} an agent\n\
            - `docs/development.md` \u{2014} a doc\n\
            - https://example.com/x \u{2014} a url\n\
            - ea / email / comms(ea) \u{2014} several\n\
            - ADR-183 \u{2014} an adr\n\
            \n## Other\n- late(meta) \u{2014} not in See Also\n";
        assert_eq!(
            extract_way_refs(body),
            vec![
                ("code/quality".to_string(), "softwaredev".to_string()),
                ("trust".to_string(), "meta".to_string()),
            ]
        );
    }

    #[test]
    fn opens_with_fence_tolerates_crlf() {
        assert!(opens_with_fence("---\ndescription: x\n---\n"));
        assert!(opens_with_fence("---\r\ndescription: x\r\n---\r\n"));
        assert!(!opens_with_fence("no frontmatter\n"));
        assert!(!opens_with_fence(""));
    }

    #[test]
    fn body_text_keeps_horizontal_rules() {
        let content = "---\ndescription: d\n---\n# Way\n\nabove\n\n---\n\nbelow\n";
        assert_eq!(body_text(content), "# Way\n\nabove\n\n---\n\nbelow");
    }

    #[test]
    fn body_text_reads_crlf_frontmatter() {
        assert_eq!(body_text("---\r\ndescription: d\r\n---\r\n# Way\r\n"), "# Way");
    }

    /// An unclosed block is not frontmatter (the parser rejects it), so no
    /// field is read from it. The show and tree scanners used to read on to
    /// the end of the file and found `scope:` here.
    #[test]
    fn field_in_reads_nothing_from_an_unclosed_block() {
        assert_eq!(field_in("---\nscope: agent\n# body\n", "scope"), None);
        assert_eq!(field_in("---\nscope: agent\n---\n# body\n", "scope").as_deref(), Some("agent"));
    }

    #[test]
    fn field_in_never_reads_the_body() {
        let content = "---\ndescription: d\n---\nscope: agent\n";
        assert_eq!(field_in(content, "scope"), None);
    }

    #[test]
    fn field_skips_empty_values_and_trims() {
        assert_eq!(field("a:\na:  x  \n", "a").as_deref(), Some("x"));
        assert_eq!(field("ab: x\n", "a"), None);
    }

    #[test]
    fn closing_fence_line_follows_split() {
        assert_eq!(closing_fence_line("---\na: 1\n---\nbody\n---\n"), Some(2));
        assert_eq!(closing_fence_line("---\r\na: 1\r\n---\r\n"), Some(2));
        assert_eq!(closing_fence_line("---\n---\n"), Some(1));
        assert_eq!(closing_fence_line("---\na: 1\n"), None);
        assert_eq!(closing_fence_line("a: 1\n---\n"), None);
    }

    fn parse_yaml(yaml: &str) -> Frontmatter {
        serde_yaml::from_str(yaml).expect("frontmatter parse failed")
    }

    #[test]
    fn parse_str_accepts_valid_frontmatter() {
        assert!(parse_str("description: a way\nvocabulary: a b c\n").is_ok());
    }

    #[test]
    fn parse_str_rejects_unquoted_colon_space() {
        // The exact shape that silently dropped a way from the corpus: a colon-space
        // in an unquoted scalar is invalid YAML. lint runs this so it can't pass.
        assert!(parse_str("description: a way about X: the thing\nvocabulary: a b\n").is_err());
    }

    #[test]
    fn parse_str_accepts_colon_when_quoted() {
        assert!(parse_str("description: \"a way about X: the thing\"\nvocabulary: a b\n").is_ok());
    }

    #[test]
    fn parse_if_present_distinguishes_absent_from_malformed() {
        let dir = std::env::temp_dir().join(format!("ways-fm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // No frontmatter delimiters → not a way → Ok(None) (silent skip).
        let prose = dir.join("prose.md");
        std::fs::write(&prose, "# Just prose\nno frontmatter here\n").unwrap();
        assert!(matches!(parse_if_present(&prose), Ok(None)));

        // Present but malformed → Err (a real defect to surface).
        let bad = dir.join("bad.md");
        std::fs::write(&bad, "---\ndescription: broken: yaml\n---\nbody\n").unwrap();
        assert!(parse_if_present(&bad).is_err());

        // Present and valid → Ok(Some).
        let good = dir.join("good.md");
        std::fs::write(&good, "---\ndescription: fine\nvocabulary: a b\n---\nbody\n").unwrap();
        assert!(matches!(parse_if_present(&good), Ok(Some(_))));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refire_is_optional_for_static_consumers() {
        // Static consumers like `ways tune locale` and `ways corpus` parse way
        // frontmatter but don't invoke the firing engine, so a missing
        // refire: field must not error at parse time. The engine path in
        // session.rs enforces presence at the fire site.
        let fm = parse_yaml("description: no refire\n");
        assert!(fm.refire.is_none());
        assert!(fm.resolved_curve(200_000).is_none());
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
    fn resolved_curve_is_none_when_refire_absent() {
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

    fn preset_table() -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("once".to_string(), 1.0);
        m.insert("rare".to_string(), 0.4);
        m.insert("normal".to_string(), 0.15);
        m.insert("frequent".to_string(), 0.05);
        m
    }

    #[test]
    fn refire_preset_resolves_via_table() {
        let presets = preset_table();
        let spec = RefireSpec::Preset("rare".to_string());
        assert!((spec.fraction_with(&presets) - 0.4).abs() < 1e-9);

        // to_curve_with uses the same resolution
        let curve = spec.to_curve_with(1_000_000, &presets);
        match curve {
            Curve::Exponential { half_life } => assert_eq!(half_life, 400_000),
            other => panic!("expected Exponential, got {:?}", other),
        }
    }

    #[test]
    fn refire_preset_unknown_falls_back_to_normal() {
        // Fire-time path: unknown preset shouldn't panic. Falls back to 0.15
        // (normal-equivalent) so the session keeps working. Stderr warning
        // is emitted but not asserted on here.
        let presets = preset_table();
        let spec = RefireSpec::Preset("nonexistent".to_string());
        assert!((spec.fraction_with(&presets) - 0.15).abs() < 1e-9);
    }

    #[test]
    fn validate_accepts_known_preset() {
        let presets = preset_table();
        let spec = RefireSpec::Preset("normal".to_string());
        assert!(spec.validate(&presets).is_ok());
    }

    #[test]
    fn validate_rejects_unknown_preset() {
        let presets = preset_table();
        let spec = RefireSpec::Preset("nonexistent".to_string());
        let err = spec.validate(&presets).unwrap_err();
        assert!(err.contains("nonexistent"));
        assert!(err.contains("valid:"));
    }

    #[test]
    fn validate_accepts_numeric_in_range() {
        let presets = preset_table();
        for v in [0.0_f64, 0.05, 0.15, 0.4, 1.0, 2.0] {
            let spec = RefireSpec::Numeric(v);
            assert!(
                spec.validate(&presets).is_ok(),
                "expected {v} to validate"
            );
        }
    }

    #[test]
    fn validate_rejects_numeric_out_of_range() {
        let presets = preset_table();
        // Negative
        assert!(RefireSpec::Numeric(-0.1).validate(&presets).is_err());
        // Way above the cap (e.g., raw tokens pasted into new field)
        assert!(RefireSpec::Numeric(30_000.0).validate(&presets).is_err());
        // Non-finite
        assert!(RefireSpec::Numeric(f64::NAN).validate(&presets).is_err());
        assert!(RefireSpec::Numeric(f64::INFINITY).validate(&presets).is_err());
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
    fn fires_on_semantic_channel() {
        assert!(fires_on_something("description: x\nvocabulary: y\n"));
    }

    #[test]
    fn does_not_fire_on_description_alone() {
        // Semantic channel needs both — description without vocabulary is
        // flagged by a separate lint rule, not by fire-eligibility.
        assert!(!fires_on_something("description: x\n"));
    }

    #[test]
    fn does_not_fire_on_vocabulary_alone() {
        assert!(!fires_on_something("vocabulary: x\n"));
    }

    #[test]
    fn fires_on_each_declared_channel() {
        // Every field in FIRE_BEARING_FIELDS must flip the predicate on its
        // own — this test catches a new channel being added to the const but
        // the iteration logic drifting away from it.
        for field in FIRE_BEARING_FIELDS {
            let fm = format!("{field}: something\n");
            assert!(
                fires_on_something(&fm),
                "expected fire on {field}-only frontmatter"
            );
        }
    }

    #[test]
    fn does_not_fire_on_empty_or_non_channel_fields() {
        assert!(!fires_on_something(""));
        assert!(!fires_on_something("scope: user\n"));
        // curve: and refire: are cadence fields, not firing channels
        assert!(!fires_on_something("curve:\n  type: Flat\n"));
        assert!(!fires_on_something("refire: 0.2\n"));
    }

    #[test]
    fn fires_on_combined_channels() {
        // A typical way with semantic + pattern — both channels wired.
        let fm = "description: x\nvocabulary: y\npattern: '^foo'\n";
        assert!(fires_on_something(fm));
    }

    #[test]
    fn detect_legacy_redisclose_flags_indented_top_level() {
        // Top-level field can have leading whitespace in some yaml styles.
        let yaml = "description: test\n  redisclose: 25\n";
        let err = detect_legacy_redisclose(yaml).expect_err("should reject indented");
        assert!(err.to_string().contains("redisclose"));
    }
}
