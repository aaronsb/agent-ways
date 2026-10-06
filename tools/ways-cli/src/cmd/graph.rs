use anyhow::Result;
use serde_json::json;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::frontmatter;
use crate::scanner;

pub fn run(ways_dir: Option<String>, output: Option<String>) -> Result<()> {
    let root = ways_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_ways_dir);

    let writer: Box<dyn Write> = match output {
        Some(ref path) => Box::new(std::fs::File::create(path)?),
        None => Box::new(io::stdout()),
    };
    let mut w = BufWriter::new(writer);

    let (node_count, edge_count) = write_graph(&root, &mut w)?;
    w.flush()?;

    eprintln!("{node_count} nodes, {edge_count} edges");
    Ok(())
}

/// Write one JSON line per way and per See Also edge; returns (nodes, edges).
///
/// Every way is a node, including those that fire on files or a trigger and
/// carry no `description:` (their description is null). Edges point at node
/// ids: an entry that names no node (a skill, a subagent) is not an edge.
fn write_graph(root: &Path, w: &mut impl Write) -> Result<(usize, usize)> {
    let ways = scanner::scan_declared_ways(root);
    let mut node_count = 0;
    let mut edge_count = 0;

    for way in &ways {
        let content = std::fs::read_to_string(&way.path)?;
        let fm = frontmatter::parse(&way.path)?;
        let epistemic = frontmatter::extract_epistemic(&content);
        let description = (!fm.description.is_empty()).then_some(fm.description);

        let node = json!({
            "id": way.id,
            "domain": way.domain,
            "epistemic": epistemic,
            "description": description,
        });
        serde_json::to_writer(&mut *w, &node)?;
        w.write_all(b"\n")?;
        node_count += 1;

        for r in frontmatter::extract_way_refs(&content) {
            let Some(target) = scanner::resolve_ref(&ways, &r.name, &r.domain) else { continue };
            let mut edge = json!({
                "source": way.id,
                "target": target.id,
                "type": "see_also",
            });
            if !r.label.is_empty() {
                edge["label"] = json!(r.label);
            }
            serde_json::to_writer(&mut *w, &edge)?;
            w.write_all(b"\n")?;
            edge_count += 1;
        }
    }
    Ok((node_count, edge_count))
}

fn default_ways_dir() -> PathBuf {
    crate::paths::projected_ways_root()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn a_way_without_a_description_is_a_node_with_its_edges() {
        let root = std::env::temp_dir().join(format!("ways-graph-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        put(
            &root,
            "d/described/described.md",
            "---\ndescription: has one\n---\n# D\n\n## See Also\n\n- bare(d) — to the descriptionless way\n- develop (skill) — not an edge\n",
        );
        put(
            &root,
            "d/bare/bare.md",
            "---\nfiles: x\n---\n# B\n\n## See Also\n\n- described(d) — back\n",
        );

        let mut out = Vec::new();
        let (nodes, edges) = write_graph(&root, &mut out).unwrap();
        assert_eq!((nodes, edges), (2, 2));

        let lines: Vec<serde_json::Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let bare = lines.iter().find(|l| l["id"] == "bare").expect("bare is a node");
        assert!(bare["description"].is_null());
        let edge = |s: &str, t: &str| lines.iter().any(|l| l["type"] == "see_also" && l["source"] == s && l["target"] == t);
        assert!(edge("described", "bare"));
        assert!(edge("bare", "described"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
