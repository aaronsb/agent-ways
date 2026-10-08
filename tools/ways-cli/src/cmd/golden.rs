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

pub fn run(ways_dir: Option<String>, tsv: bool, probes: bool) -> Result<()> {
    let root = ways_dir.map(PathBuf::from).unwrap_or_else(crate::paths::shipped_ways_root);
    let rows = export(&root)?;
    if probes {
        for p in sample_probes(&rows) {
            println!("{}", p.tsv());
        }
        return Ok(());
    }
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

/// One tree-sampled probe: a prompt, the way that should win, and the semantic
/// siblings that must not out-rank it.
#[derive(Debug, PartialEq, Eq)]
pub struct Probe {
    pub prompt: String,
    pub way: String,
    pub kind: String,
    pub role: &'static str,
    pub must_not: Vec<String>,
}

impl Probe {
    fn tsv(&self) -> String {
        format!("{}\t{}\t{}\t{}\t{}", self.prompt, self.way, self.kind, self.role, self.must_not.join(","))
    }
}

/// FNV-1a, 64-bit, over the UTF-8 bytes of a way id. Stable across runs and
/// platforms, so the leaf a parent contributes depends only on the ids.
fn fnv1a(id: &str) -> u64 {
    id.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn parent_dir(id: &str) -> &str {
    id.rsplit_once('/').map_or("", |(p, _)| p)
}

/// Sample the disclosure tree. The semantic ways are the ids that carry golden
/// rows. A way's semantic children are the semantic ways whose nearest semantic
/// ancestor (by id prefix) it is. Roles: `root` is a depth-1 way, `parent` is a
/// non-root way with semantic children, `leaf` has none. Selected: every root,
/// every parent, and for each root or parent the one leaf child with the least
/// `(fnv1a(id), id)`. Roots and parents emit their situational prompt; a leaf
/// emits both. `must_not` lists the semantic ways sharing the way's parent
/// directory. Nothing is named by id, so a rename moves the sample with it.
pub fn sample_probes(rows: &[Row]) -> Vec<Probe> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut by_way: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.way != "none") {
        by_way.entry(r.way.as_str()).or_default().push(r);
    }
    let ids: Vec<&str> = by_way.keys().copied().collect();
    let nearest_ancestor = |id: &str| -> Option<&str> {
        let mut cur = id;
        while let Some((p, _)) = cur.rsplit_once('/') {
            if by_way.contains_key(p) {
                return by_way.get_key_value(p).map(|(k, _)| *k);
            }
            cur = p;
        }
        None
    };
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for id in &ids {
        if let Some(a) = nearest_ancestor(id) {
            children.entry(a).or_default().push(id);
        }
    }
    let is_parent = |id: &str| children.contains_key(id);
    let mut selected: BTreeSet<&str> = BTreeSet::new();
    for id in &ids {
        if !id.contains('/') || is_parent(id) {
            selected.insert(id);
        }
    }
    for (parent, kids) in &children {
        let _ = parent;
        if let Some(leaf) = kids.iter().filter(|k| !is_parent(k)).min_by_key(|k| (fnv1a(k), **k)) {
            selected.insert(leaf);
        }
    }
    let mut out = Vec::new();
    for id in selected {
        let role = if !id.contains('/') { "root" } else if is_parent(id) { "parent" } else { "leaf" };
        let must_not: Vec<String> = ids
            .iter()
            .filter(|o| **o != id && parent_dir(o) == parent_dir(id))
            .map(|o| o.to_string())
            .collect();
        for r in &by_way[id] {
            if role != "leaf" && !r.kind.starts_with("situational") {
                continue;
            }
            out.push(Probe { prompt: r.prompt.clone(), way: id.to_string(), kind: r.kind.clone(), role, must_not: must_not.clone() });
        }
    }
    out
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

    fn fixture() -> PathBuf {
        let root = scratch("probes");
        let g = |w: &str| format!("{{\"kind\":\"direct\",\"prompt\":\"{w} d\"}}\n{{\"kind\":\"situational\",\"prompt\":\"{w} s\"}}\n");
        for w in ["top", "top/par", "top/par/l1", "top/par/l2", "top/par/l3", "top/lone", "other"] {
            let leaf = w.rsplit('/').next().unwrap();
            put(&root, &format!("{w}/{leaf}.golden.jsonl"), &g(w));
        }
        put(&root, "golden-none.jsonl", "{\"prompt\":\"hello\"}\n");
        root
    }

    #[test]
    fn probes_select_roots_parents_and_one_hashed_leaf_per_parent() {
        let probes = sample_probes(&export(&fixture()).unwrap());
        let got: Vec<(&str, &str, &str)> = probes.iter().map(|p| (p.way.as_str(), p.role, p.kind.as_str())).collect();
        // the hash picks exactly one of l1..l3 under par
        let l = ["top/par/l1", "top/par/l2", "top/par/l3"].into_iter().min_by_key(|i| (fnv1a(i), *i)).unwrap();
        assert_eq!(
            got,
            [
                ("other", "root", "situational"),
                ("top", "root", "situational"),
                ("top/lone", "leaf", "direct"),
                ("top/lone", "leaf", "situational"),
                ("top/par", "parent", "situational"),
                (l, "leaf", "direct"),
                (l, "leaf", "situational"),
            ]
        );
    }

    #[test]
    fn probes_name_semantic_siblings_in_must_not() {
        let probes = sample_probes(&export(&fixture()).unwrap());
        let m = |w: &str, role: &str| probes.iter().find(|p| p.way == w && p.role == role).unwrap().must_not.clone();
        assert_eq!(m("top", "root"), ["other"]);
        assert_eq!(m("top/par", "parent"), ["top/lone"]);
        assert_eq!(m("top/lone", "leaf"), ["top/par"]);
        let leaf = probes.iter().find(|p| p.role == "leaf" && p.way.starts_with("top/par/")).unwrap();
        assert_eq!(leaf.must_not.len(), 2);
        assert!(!leaf.must_not.contains(&leaf.way));
    }

    #[test]
    fn probes_are_deterministic_and_leave_out_none_rows() {
        let root = fixture();
        let a = sample_probes(&export(&root).unwrap());
        assert_eq!(a, sample_probes(&export(&root).unwrap()));
        assert!(a.iter().all(|p| p.way != "none"));
    }

    #[test]
    fn fnv1a_matches_the_reference_vectors() {
        assert_eq!(fnv1a(""), 0xcbf29ce484222325);
        assert_eq!(fnv1a("a"), 0xaf63dc4c8601ec8c);
    }
}
