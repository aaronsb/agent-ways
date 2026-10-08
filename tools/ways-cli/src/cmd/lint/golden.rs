//! ADR-701 §9 golden-prompt sidecars: `{wayname}.golden.jsonl` beside a way,
//! and `golden-none.jsonl` at the top of a ways root.
//!
//! A sidecar line is `{"kind":"direct"|"situational","prompt":"..."}`, with an
//! optional `"surface":"tool"` for a way that fires on a tool call. A
//! `golden-none.jsonl` line is `{"prompt":"..."}`: a prompt no way should win.
//!
//! In a core root every semantic way (a `description:` and no `trigger:`) must
//! carry a sidecar with a non-empty direct and a non-empty situational prompt.
//! In any root, a sidecar that is malformed, or that sits beside no way, is
//! reported. User and project roots have no coverage requirement, and their
//! findings are warnings; the core corpus gets errors.

use std::path::{Path, PathBuf};

use super::helpers::has_field;

const SIDECAR_SUFFIX: &str = ".golden.jsonl";
const NONE_FILE: &str = "golden-none.jsonl";

/// What the golden pass scanned.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct GoldenCounts {
    /// Sidecar files read (`*.golden.jsonl` and `golden-none.jsonl`).
    pub files: usize,
    /// Ways that need a sidecar (core roots only).
    pub required: usize,
}

/// Findings split by whether the root enforces them.
#[derive(Debug, Default)]
struct Report {
    /// (relative path, message)
    findings: Vec<(String, String)>,
    counts: GoldenCounts,
}

pub(super) fn lint_golden(
    dir: &Path,
    ways_dir: &Path,
    project: Option<&Path>,
    errors: &mut u32,
    warnings: &mut u32,
) -> GoldenCounts {
    let core = crate::paths::containing_ways_root(dir, project)
        .is_some_and(|root| crate::paths::is_core_root(&root));
    let report = check(dir, ways_dir, core);
    for (rel, msg) in &report.findings {
        if core {
            eprintln!("  ERROR: {rel} — {msg}");
            *errors += 1;
        } else {
            eprintln!("  WARNING: {rel} — {msg}");
            *warnings += 1;
        }
    }
    report.counts
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(crate::util::path_to_id)
        .unwrap_or_else(|_| path.display().to_string())
}

/// Whether the way file at `path` must carry a sidecar in a core root: a
/// non-check, non-locale `.md` with frontmatter, a `description:`, and no
/// `trigger:` (attend signal and session-state ways are not matched by prompt).
fn needs_golden(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if crate::scanner::is_check(path) || crate::util::extract_locale_from_filename(name).is_some() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(path) else { return false };
    let Some((fm, _)) = crate::frontmatter::split(&content) else { return false };
    crate::frontmatter::field(&fm, "description").is_some() && !has_field(&fm, "trigger")
}

fn check(dir: &Path, ways_dir: &Path, core: bool) -> Report {
    let mut report = Report::default();
    let mut paths: Vec<PathBuf> = crate::scanner::files(dir).collect();
    paths.sort();

    for path in &paths {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let r = rel(ways_dir, path);
        if name == NONE_FILE {
            report.counts.files += 1;
            check_lines(path, &r, true, &mut report.findings);
        } else if let Some(stem) = name.strip_suffix(SIDECAR_SUFFIX) {
            report.counts.files += 1;
            check_lines(path, &r, false, &mut report.findings);
            if !path.with_file_name(format!("{stem}.md")).is_file() {
                report.findings.push((r, format!("golden sidecar has no sibling way file `{stem}.md`")));
            }
        } else if core && name.ends_with(".md") && needs_golden(path) {
            report.counts.required += 1;
            let stem = name.trim_end_matches(".md");
            let sidecar = path.with_file_name(format!("{stem}{SIDECAR_SUFFIX}"));
            if !sidecar.is_file() {
                report.findings.push((
                    r,
                    format!("no `{stem}{SIDECAR_SUFFIX}` (ADR-701 §9: every core way carries a direct and a situational golden prompt)"),
                ));
            } else {
                for kind in ["direct", "situational"] {
                    if !has_prompt(&sidecar, kind) {
                        report.findings.push((
                            rel(ways_dir, &sidecar),
                            format!("no non-empty `{kind}` prompt"),
                        ));
                    }
                }
            }
        }
    }
    report
}

/// Whether the sidecar holds a non-empty prompt of `kind`.
fn has_prompt(sidecar: &Path, kind: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(sidecar) else { return false };
    content.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).any(|v| {
        v.get("kind").and_then(|k| k.as_str()) == Some(kind)
            && v.get("prompt").and_then(|p| p.as_str()).is_some_and(|p| !p.trim().is_empty())
    })
}

/// Validate each line of one golden file. `none` selects the `golden-none.jsonl`
/// shape (`prompt` only).
fn check_lines(path: &Path, rel: &str, none: bool, out: &mut Vec<(String, String)>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            out.push((rel.to_string(), format!("cannot read golden file: {e}")));
            return;
        }
    };
    let allowed: &[&str] = if none { &["prompt"] } else { &["kind", "prompt", "surface"] };
    for (i, raw) in content.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        let at = format!("{rel} (line {})", i + 1);
        let val: serde_json::Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(_) => {
                out.push((at, "invalid JSON in golden file".into()));
                continue;
            }
        };
        let Some(obj) = val.as_object() else {
            out.push((at, "golden line is not a JSON object".into()));
            continue;
        };
        for key in obj.keys().filter(|k| !allowed.contains(&k.as_str())) {
            out.push((at.clone(), format!("unknown key '{key}' in golden line")));
        }
        match obj.get("prompt").and_then(|p| p.as_str()) {
            Some(p) if !p.trim().is_empty() => {}
            _ => out.push((at.clone(), "`prompt` is missing or empty".into())),
        }
        if none {
            continue;
        }
        match obj.get("kind").and_then(|k| k.as_str()) {
            Some("direct" | "situational") => {}
            Some(other) => out.push((at.clone(), format!("unknown kind '{other}' (expected direct or situational)"))),
            None => out.push((at.clone(), "`kind` is missing".into())),
        }
        if let Some(s) = obj.get("surface") {
            if s.as_str() != Some("tool") {
                out.push((at, "`surface` must be \"tool\" when present".into()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ways-lint-golden-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn way(root: &Path, id: &str, fm: &str) {
        let leaf = id.rsplit('/').next().unwrap();
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{leaf}.md")), format!("---\n{fm}---\n# {leaf}\n")).unwrap();
    }

    fn sidecar(root: &Path, id: &str, body: &str) {
        let leaf = id.rsplit('/').next().unwrap();
        std::fs::write(root.join(id).join(format!("{leaf}.golden.jsonl")), body).unwrap();
    }

    const FM: &str = "description: d\nvocabulary: v\n";
    const GOOD: &str = "{\"kind\":\"direct\",\"prompt\":\"a\"}\n{\"kind\":\"situational\",\"prompt\":\"b\"}\n";

    fn msgs(r: &Report) -> Vec<String> {
        r.findings.iter().map(|(p, m)| format!("{p}: {m}")).collect()
    }

    #[test]
    fn a_complete_sidecar_is_clean() {
        let root = scratch("clean");
        way(&root, "a/one", FM);
        sidecar(&root, "a/one", GOOD);
        let r = check(&root, &root, true);
        assert!(r.findings.is_empty(), "{:?}", msgs(&r));
        assert_eq!(r.counts, GoldenCounts { files: 1, required: 1 });
    }

    #[test]
    fn a_missing_situational_prompt_is_a_finding() {
        let root = scratch("nosit");
        way(&root, "a/one", FM);
        sidecar(&root, "a/one", "{\"kind\":\"direct\",\"prompt\":\"a\"}\n{\"kind\":\"situational\",\"prompt\":\"\"}\n");
        let r = check(&root, &root, true);
        assert!(msgs(&r).iter().any(|m| m.contains("no non-empty `situational` prompt")), "{:?}", msgs(&r));
    }

    #[test]
    fn a_missing_sidecar_is_a_finding_in_core_only() {
        let root = scratch("nofile");
        way(&root, "a/one", FM);
        assert_eq!(check(&root, &root, true).findings.len(), 1);
        assert!(check(&root, &root, false).findings.is_empty());
    }

    #[test]
    fn ways_without_a_prompt_surface_need_no_sidecar() {
        let root = scratch("exempt");
        way(&root, "a/files-only", "files: x\n");
        way(&root, "a/signal", "description: d\ntrigger:\n  type: attend\n");
        std::fs::write(root.join("a/files-only/files-only.check.md"), "---\ndescription: d\n---\n").unwrap();
        std::fs::write(root.join("a/files-only/files-only.ja.md"), "---\ndescription: d\n---\n").unwrap();
        let r = check(&root, &root, true);
        assert!(r.findings.is_empty(), "{:?}", msgs(&r));
        assert_eq!(r.counts.required, 0);
    }

    #[test]
    fn malformed_lines_are_reported_in_any_root() {
        let root = scratch("bad");
        way(&root, "a/one", FM);
        sidecar(
            &root,
            "a/one",
            "not json\n{\"kind\":\"weird\",\"prompt\":\"x\"}\n{\"kind\":\"direct\",\"prompt\":\"x\",\"extra\":1}\n{\"kind\":\"direct\",\"prompt\":\"x\",\"surface\":\"cli\"}\n[1]\n",
        );
        let m = msgs(&check(&root, &root, false)).join("\n");
        for needle in ["invalid JSON", "unknown kind 'weird'", "unknown key 'extra'", "`surface` must be", "not a JSON object"] {
            assert!(m.contains(needle), "{needle} not in:\n{m}");
        }
    }

    #[test]
    fn an_orphan_sidecar_is_reported() {
        let root = scratch("orphan");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("a/gone.golden.jsonl"), GOOD).unwrap();
        let m = msgs(&check(&root, &root, false));
        assert_eq!(m.len(), 1, "{m:?}");
        assert!(m[0].contains("no sibling way file `gone.md`"));
    }

    #[test]
    fn golden_none_takes_only_a_prompt() {
        let root = scratch("none");
        std::fs::write(root.join("golden-none.jsonl"), "{\"prompt\":\"ok\"}\n{\"prompt\":\"\"}\n{\"prompt\":\"x\",\"kind\":\"none\"}\n").unwrap();
        let m = msgs(&check(&root, &root, true)).join("\n");
        assert!(m.contains("line 2") && m.contains("missing or empty"), "{m}");
        assert!(m.contains("line 3") && m.contains("unknown key 'kind'"), "{m}");
        assert!(!m.contains("line 1"), "{m}");
    }
}
