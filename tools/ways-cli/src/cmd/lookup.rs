//! `ways lookup`: the machine interface behind the MCP lookup tools (ADR-701 §5).
//!
//! `ways-mcp` calls this binary and reads one JSON object from stdout, so the
//! tools run the same matcher, renderer and stamping the hooks run, with no
//! second copy of any of them. A failure prints `{"error": "..."}` and exits 1.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::Path;

use crate::cmd::scan::lookup::{route_of, search};
use crate::cmd::siblings::{default_corpus, find_entry, load_embeddings, nearest, CorpusEntry};
use crate::frontmatter::extract_way_refs;
use crate::scanner::{resolve_ref, scan_declared_ways, WayFile};
use crate::session;

/// Cosine floor for a semantic neighbour, the default of `ways author siblings`.
const NEIGHBOUR_THRESHOLD: f64 = 0.3;

/// Print one lookup's result as a JSON object and exit: 0 for a result, 1 for
/// an `{"error": ...}` object.
pub fn emit(result: Result<Value>) -> ! {
    let (mut object, code) = match result {
        Ok(v) => (v, 0),
        Err(e) => (json!({ "error": format!("{e:#}") }), 1),
    };
    object["contract"] = json!(CONTRACT);
    println!("{object}");
    std::process::exit(code);
}

/// The version of this JSON interface. Every object carries it, and `ways-mcp`
/// refuses a `ways` that speaks another, so the two binaries can update apart.
pub const CONTRACT: u64 = 1;

/// The refusal for a project that switched ways off (ADR-184 item 6): a lookup
/// serves nothing there, as the scan lanes inject nothing.
pub fn ensure_enabled() -> Result<()> {
    if crate::cmd::scan::enabled_for(None) {
        Ok(())
    } else {
        bail!("this project has ways switched off: `enabled: false` in its `.claude/ways.yaml` or in the user config")
    }
}

/// `ways_search`: ranked candidates on the prompt lane.
pub fn search_json(query: &str, session_id: Option<&str>, top_n: usize) -> Result<Value> {
    let project_dir = crate::util::project_dir();
    let Some(found) = search(query, session_id, &project_dir, top_n) else {
        bail!("no embedding lane ran (the engine or corpus is missing); run `ways status`");
    };
    let hits: Vec<Value> = found
        .hits
        .iter()
        .map(|h| {
            let round = |v: f64| (v * 10_000.0).round() / 10_000.0;
            let mut o = json!({
                "way": h.way,
                "route": h.route,
                "description": h.description,
                "cosine": round(h.cosine),
                "share": round(h.share),
                "margin": h.margin.map(round),
            });
            if let Some(section) = &h.section {
                o["section"] = json!(section);
            }
            o
        })
        .collect();
    Ok(json!({ "lane": found.lane, "candidates": hits }))
}

/// `ways_read`: a way's body as injection renders it. Nothing is stamped here;
/// the PostToolUse hook does that for the calling agent (`ways hook pull`).
pub fn read_json(id: &str, session_id: Option<&str>) -> Result<Value> {
    let read = crate::cmd::show::pull::read(id, session_id)?;
    let mut o = json!({ "way": id, "route": route_of(id), "scope": read.scope, "body": read.body });
    if let Some(note) = read.note {
        o["note"] = json!(note);
    }
    Ok(o)
}

/// `ways_neighbors`: the way's tree, See Also and semantic neighbours. With a
/// session, a neighbour whose `scope:` would not reach that session is marked.
pub fn neighbors_json(id: &str, session_id: Option<&str>, top_n: usize) -> Result<Value> {
    crate::cmd::show::pull::check_id(id)?;
    let project_dir = crate::util::project_dir();
    let session_scope = session_id.map_or_else(|| "agent".to_string(), session::detect_scope);
    let roots = ways_core::paths::ways_roots(Some(Path::new(&project_dir)));
    let ways = collect_ways(&roots);

    let corpus = default_corpus();
    let entries = if corpus.is_file() { load_embeddings(&corpus.to_string_lossy()).ok() } else { None };
    let prefix = format!("{}/", crate::util::encode_project_key(Path::new(&project_dir)));
    let is_disabled = |way: &str| session::domain_disabled(way.split('/').next().unwrap_or(way)) || session::way_disabled(way);
    let out_of_scope = |way: &str| {
        ways.iter()
            .find(|n| n.id == way)
            .and_then(|n| std::fs::read_to_string(&n.file.path).ok())
            .is_some_and(|c| !session::scope_matches(&crate::frontmatter::field_in(&c, "scope").unwrap_or_default(), &session_scope))
    };
    neighbors_in(id, &ways, entries.as_deref(), &prefix, top_n, &is_disabled, &out_of_scope)
}

/// One way with its tree id (domain included, as `ways_read` takes it).
struct Node {
    id: String,
    file: WayFile,
}

/// Every way across the roots in precedence order; a higher root shadows a
/// lower root's way of the same id.
fn collect_ways(roots: &[std::path::PathBuf]) -> Vec<Node> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for root in roots {
        for file in scan_declared_ways(root) {
            let id = if file.id == file.domain { file.domain.clone() } else { format!("{}/{}", file.domain, file.id) };
            if seen.insert(id.clone()) {
                out.push(Node { id, file });
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// The nearest ancestor of `id`, by path, that is a way.
fn parent_of<'a>(id: &str, ways: &'a [Node]) -> Option<&'a Node> {
    let mut path = id;
    while let Some(cut) = path.rfind('/') {
        path = &path[..cut];
        if let Some(n) = ways.iter().find(|n| n.id == path) {
            return Some(n);
        }
    }
    None
}

fn neighbors_in(
    id: &str,
    ways: &[Node],
    entries: Option<&[CorpusEntry]>,
    project_prefix: &str,
    top_n: usize,
    is_disabled: &dyn Fn(&str) -> bool,
    out_of_scope: &dyn Fn(&str) -> bool,
) -> Result<Value> {
    let target = ways
        .iter()
        .find(|n| n.id == id)
        .or_else(|| {
            let suffix = format!("/{id}");
            let mut hits = ways.iter().filter(|n| n.id.ends_with(&suffix));
            let first = hits.next()?;
            hits.next().is_none().then_some(first)
        })
        .ok_or_else(|| anyhow!("no way named {id}"))?;

    let mut out: Vec<Value> = Vec::new();
    let mut add = |kind: &str, way: &str, extra: Value| {
        let mut o = json!({ "kind": kind, "way": way, "route": route_of(way) });
        if is_disabled(way) {
            o["enabled"] = json!(false);
        }
        if out_of_scope(way) {
            o["in_scope"] = json!(false);
        }
        if let Value::Object(map) = extra {
            for (k, v) in map {
                o[k] = v;
            }
        }
        out.push(o);
    };

    if let Some(p) = parent_of(&target.id, ways) {
        add("parent", &p.id, json!({}));
    }
    for child in ways.iter().filter(|n| parent_of(&n.id, ways).is_some_and(|p| p.id == target.id)) {
        add("child", &child.id, json!({}));
    }

    let files: Vec<WayFile> = ways.iter().map(|n| n.file.clone()).collect();
    let tree_id = |w: &WayFile| ways.iter().find(|n| n.file.path == w.path).map(|n| n.id.clone());

    let content = std::fs::read_to_string(&target.file.path)?;
    for r in extract_way_refs(&content) {
        let Some(hit) = resolve_ref(&files, &r.name, &r.domain).and_then(tree_id) else { continue };
        add("see_also", &hit, json!({ "label": r.label }));
    }
    for n in ways.iter().filter(|n| n.id != target.id) {
        let Ok(text) = std::fs::read_to_string(&n.file.path) else { continue };
        let points_here = extract_way_refs(&text)
            .into_iter()
            .filter_map(|r| resolve_ref(&files, &r.name, &r.domain).and_then(tree_id).map(|t| (t, r.label)))
            .find(|(t, _)| *t == target.id);
        if let Some((_, label)) = points_here {
            add("see_also_from", &n.id, json!({ "label": label }));
        }
    }

    let mut semantic_note = None;
    match entries {
        None => semantic_note = Some("no corpus on disk: semantic neighbours unavailable (run `ways corpus`)"),
        Some(entries) => {
            let own = find_entry(entries, &format!("{project_prefix}{}", target.id)).or_else(|| find_entry(entries, &target.id));
            match own {
                None => semantic_note = Some("this way is not in the corpus: it has no description and vocabulary, or the corpus predates it"),
                Some(own) => {
                    let near = nearest(entries, own, NEIGHBOUR_THRESHOLD);
                    let mut kept = 0;
                    for (corpus_id, cosine) in near {
                        // Another project's ways carry that project's key; they are no neighbour here.
                        let way = match corpus_id.strip_prefix(project_prefix) {
                            Some(bare) => bare,
                            None if corpus_id.starts_with('-') => continue,
                            None => corpus_id,
                        };
                        if way == target.id || !ways.iter().any(|n| n.id == way) {
                            continue;
                        }
                        add("semantic", way, json!({ "cosine": (cosine * 10_000.0).round() / 10_000.0 }));
                        kept += 1;
                        if kept == top_n {
                            break;
                        }
                    }
                }
            }
        }
    }

    let mut result = json!({ "way": target.id, "route": route_of(&target.id), "neighbors": out });
    if let Some(note) = semantic_note {
        result["note"] = json!(note);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn tree(name: &str) -> (std::path::PathBuf, Vec<Node>) {
        let root = std::env::temp_dir().join(format!("ways-lookup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        put(&root, "d/d.md", "---\ndescription: root\n---\n# D\n");
        put(&root, "d/code/code.md", "---\ndescription: code\n---\n# Code\n\n## See Also\n\n- tests(d) — how to test\n");
        put(&root, "d/code/quality/quality.md", "---\ndescription: quality\n---\n# Q\n");
        put(&root, "d/tests/tests.md", "---\ndescription: tests\n---\n# T\n");
        put(&root, "d/other/other.md", "---\ndescription: other\n---\n# O\n");
        let ways = collect_ways(std::slice::from_ref(&root));
        (root, ways)
    }

    fn entry(id: &str, v: [f32; 3]) -> CorpusEntry {
        CorpusEntry { id: id.into(), embedding: v.to_vec() }
    }

    fn kinds(v: &Value, kind: &str) -> Vec<String> {
        v["neighbors"].as_array().unwrap().iter().filter(|n| n["kind"] == kind).map(|n| n["way"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn ids_carry_the_domain_and_the_parent_is_the_nearest_way_above() {
        let (root, ways) = tree("ids");
        let ids: Vec<&str> = ways.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["d", "d/code", "d/code/quality", "d/other", "d/tests"]);
        assert_eq!(parent_of("d/code/quality", &ways).unwrap().id, "d/code");
        assert_eq!(parent_of("d/code", &ways).unwrap().id, "d");
        assert!(parent_of("d", &ways).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn neighbours_carry_the_tree_see_also_and_semantic_kinds() {
        let (root, ways) = tree("kinds");
        let entries = [
            entry("d/code", [1.0, 0.0, 0.0]),
            entry("d/other", [0.9, 0.43589, 0.0]),
            entry("d/tests", [0.0, 1.0, 0.0]),
            entry("d/code/quality", [0.0, 0.0, 1.0]),
        ];
        let got = neighbors_in("d/code", &ways, Some(&entries), "-p/", 5, &|_| false, &|_| false).unwrap();
        assert_eq!(kinds(&got, "parent"), ["d"]);
        assert_eq!(kinds(&got, "child"), ["d/code/quality"]);
        assert_eq!(kinds(&got, "see_also"), ["d/tests"]);
        let see = got["neighbors"].as_array().unwrap().iter().find(|n| n["kind"] == "see_also").unwrap();
        assert_eq!(see["label"], "how to test");
        // d/other is 0.9 away and carries no See Also between the two.
        assert_eq!(kinds(&got, "semantic"), ["d/other"]);
        let sem = got["neighbors"].as_array().unwrap().iter().find(|n| n["kind"] == "semantic").unwrap();
        assert!((sem["cosine"].as_f64().unwrap() - 0.9).abs() < 1e-3);
        assert_eq!(got["route"], "d > code");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_way_pointed_at_lists_who_points_at_it() {
        let (root, ways) = tree("incoming");
        let got = neighbors_in("d/tests", &ways, None, "-p/", 5, &|_| false, &|_| false).unwrap();
        assert_eq!(kinds(&got, "see_also_from"), ["d/code"]);
        assert!(got["note"].as_str().unwrap().contains("no corpus"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_disabled_neighbour_is_marked_and_another_projects_way_is_skipped() {
        let (root, ways) = tree("disabled");
        let entries = [
            entry("d/code", [1.0, 0.0, 0.0]),
            entry("d/other", [1.0, 0.0, 0.0]),
            entry("-elsewhere/d/tests", [1.0, 0.0, 0.0]),
        ];
        let got = neighbors_in("d/code", &ways, Some(&entries), "-p/", 5, &|w| w == "d/other", &|_| false).unwrap();
        let sem: Vec<&Value> = got["neighbors"].as_array().unwrap().iter().filter(|n| n["kind"] == "semantic").collect();
        assert_eq!(sem.len(), 1);
        assert_eq!(sem[0]["way"], "d/other");
        assert_eq!(sem[0]["enabled"], false);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_unknown_way_is_an_error_and_a_unique_suffix_resolves() {
        let (root, ways) = tree("lookup");
        assert!(neighbors_in("d/missing", &ways, None, "-p/", 5, &|_| false, &|_| false).is_err());
        let got = neighbors_in("quality", &ways, None, "-p/", 5, &|_| false, &|_| false).unwrap();
        assert_eq!(got["way"], "d/code/quality");
        let _ = std::fs::remove_dir_all(root);
    }
}
