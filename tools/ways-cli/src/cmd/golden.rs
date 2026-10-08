//! `ways author golden` — export the golden-prompt sidecars (ADR-701 §9) as the
//! `prompt<TAB>expected_way<TAB>kind` rows the content-corpus experiments read.
//!
//! Rows come from `{wayname}.golden.jsonl` beside each way and from
//! `golden-none.jsonl` at the root. The kind is `direct` or `situational`, with
//! a `-tool` suffix when the line carries `"surface":"tool"`; a none row is
//! `prompt<TAB>none<TAB>none`. Output is sorted by way id, so it is stable.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    kind: String,
    prompt: String,
    #[serde(default)]
    surface: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoneLine {
    prompt: String,
}

/// One export row.
#[derive(Debug, PartialEq, Eq)]
pub struct Row {
    pub prompt: String,
    pub way: String,
    pub kind: String,
}

impl Row {
    fn tsv(&self) -> String {
        format!("{}\t{}\t{}", self.prompt, self.way, self.kind)
    }
}

pub fn run(ways_dir: Option<String>, tsv: bool) -> Result<()> {
    let root = ways_dir.map(PathBuf::from).unwrap_or_else(crate::paths::shipped_ways_root);
    let rows = export(&root)?;
    if tsv {
        for r in &rows {
            println!("{}", r.tsv());
        }
        return Ok(());
    }
    let mut ways: Vec<&str> = rows.iter().filter(|r| r.way != "none").map(|r| r.way.as_str()).collect();
    ways.dedup();
    let none = rows.iter().filter(|r| r.way == "none").count();
    println!("Golden prompts under {}", root.display());
    println!("  covered ways: {}", ways.len());
    println!("  rows: {} ({} way, {} none)", rows.len(), rows.len() - none, none);
    println!("  `ways author golden --tsv` prints the rows as prompt, expected_way, kind");
    Ok(())
}

/// The way id a sidecar at `sidecar` names: its directory relative to `root`,
/// as the corpus names a way (`corpus.rs` takes the parent of the way file).
/// Two way files in one directory therefore share an id.
fn way_id(root: &Path, sidecar: &Path, stem: &str) -> String {
    let dir = sidecar.parent().unwrap_or(root);
    let rel = dir.strip_prefix(root).map(crate::util::path_to_id).unwrap_or_default();
    if rel.is_empty() { stem.to_string() } else { rel }
}

fn check_cell(s: &str, at: &Path, line: usize) -> Result<()> {
    if s.trim().is_empty() {
        bail!("{} line {line}: the prompt is empty", at.display());
    }
    if s.contains(['\t', '\n', '\r']) {
        bail!("{} line {line}: a prompt holds a tab or newline, which the TSV export cannot carry", at.display());
    }
    Ok(())
}

/// Every golden row under `root`, way rows sorted by way id and then the
/// file's own order, followed by the none rows in file order.
pub fn export(root: &Path) -> Result<Vec<Row>> {
    let mut paths: Vec<PathBuf> = crate::scanner::files(root).collect();
    paths.sort();
    let mut rows: Vec<Row> = Vec::new();
    let mut none: Vec<Row> = Vec::new();

    for path in &paths {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let text = |p: &Path| std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()));
        if name == "golden-none.jsonl" {
            for (i, l) in text(path)?.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
                let n: NoneLine = serde_json::from_str(l)
                    .with_context(|| format!("{} line {}", path.display(), i + 1))?;
                check_cell(&n.prompt, path, i + 1)?;
                none.push(Row { prompt: n.prompt, way: "none".into(), kind: "none".into() });
            }
        } else if let Some(stem) = name.strip_suffix(".golden.jsonl") {
            let id = way_id(root, path, stem);
            for (i, l) in text(path)?.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
                let g: Line = serde_json::from_str(l)
                    .with_context(|| format!("{} line {}", path.display(), i + 1))?;
                if g.kind != "direct" && g.kind != "situational" {
                    bail!("{} line {}: unknown kind '{}'", path.display(), i + 1, g.kind);
                }
                check_cell(&g.prompt, path, i + 1)?;
                let kind = if g.surface.as_deref() == Some("tool") { format!("{}-tool", g.kind) } else { g.kind };
                rows.push(Row { prompt: g.prompt, way: id.clone(), kind });
            }
        }
    }
    rows.sort_by(|a, b| a.way.cmp(&b.way)); // stable: keeps direct before situational
    rows.extend(none);
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ways-golden-export-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn ids_follow_the_directory_as_the_corpus_does() {
        let r = Path::new("/r");
        assert_eq!(way_id(r, Path::new("/r/a/b/b.golden.jsonl"), "b"), "a/b");
        assert_eq!(way_id(r, Path::new("/r/core.golden.jsonl"), "core"), "core");
        assert_eq!(way_id(r, Path::new("/r/m/think/strategies/react.golden.jsonl"), "react"), "m/think/strategies");
    }

    #[test]
    fn export_sorts_by_way_and_marks_tool_and_none_rows() {
        let root = scratch("rows");
        put(&root, "z/z.golden.jsonl", "{\"kind\":\"direct\",\"prompt\":\"zd\"}\n{\"kind\":\"situational\",\"prompt\":\"zs\"}\n");
        put(
            &root,
            "a/a.golden.jsonl",
            "{\"kind\":\"direct\",\"prompt\":\"ad\",\"surface\":\"tool\"}\n{\"kind\":\"situational\",\"prompt\":\"as\"}\n",
        );
        put(&root, "golden-none.jsonl", "{\"prompt\":\"hello\"}\n");
        let tsv: Vec<String> = export(&root).unwrap().iter().map(Row::tsv).collect();
        assert_eq!(tsv, ["ad\ta\tdirect-tool", "as\ta\tsituational", "zd\tz\tdirect", "zs\tz\tsituational", "hello\tnone\tnone"]);
    }

    #[test]
    fn export_is_deterministic() {
        let root = scratch("det");
        put(&root, "a/a.golden.jsonl", "{\"kind\":\"direct\",\"prompt\":\"x\"}\n");
        put(&root, "b/b.golden.jsonl", "{\"kind\":\"direct\",\"prompt\":\"y\"}\n");
        assert_eq!(export(&root).unwrap(), export(&root).unwrap());
    }

    #[test]
    fn export_refuses_a_prompt_the_tsv_cannot_carry() {
        let root = scratch("tab");
        put(&root, "a/a.golden.jsonl", "{\"kind\":\"direct\",\"prompt\":\"a\\tb\"}\n");
        assert!(export(&root).is_err());
    }

    #[test]
    fn export_refuses_a_newline_and_names_the_line() {
        let root = scratch("nl");
        put(&root, "a/a.golden.jsonl", "{\"kind\":\"direct\",\"prompt\":\"ok\"}\n{\"kind\":\"situational\",\"prompt\":\"a\\nb\"}\n");
        let e = export(&root).unwrap_err().to_string();
        assert!(e.contains("a.golden.jsonl line 2") && e.contains("tab or newline"), "{e}");
    }

    #[test]
    fn export_refuses_an_empty_prompt_and_names_the_line() {
        let root = scratch("empty");
        put(&root, "a/a.golden.jsonl", "{\"kind\":\"direct\",\"prompt\":\"ok\"}\n{\"kind\":\"situational\",\"prompt\":\"  \"}\n");
        let e = export(&root).unwrap_err().to_string();
        assert!(e.contains("a.golden.jsonl line 2") && e.contains("empty"), "{e}");
    }
}
