use anyhow::Result;
use serde_json::json;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use crate::frontmatter;
use crate::scanner;

pub fn run(ways_dir: Option<String>, output: Option<String>) -> Result<()> {
    let root = ways_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_ways_dir);

    let ways = scanner::scan_ways(&root)?;

    let writer: Box<dyn Write> = match output {
        Some(ref path) => Box::new(std::fs::File::create(path)?),
        None => Box::new(io::stdout()),
    };
    let mut w = BufWriter::new(writer);

    let mut node_count = 0;
    let mut edge_count = 0;

    for way in &ways {
        let content = std::fs::read_to_string(&way.path)?;
        let fm = frontmatter::parse(&way.path)?;
        let epistemic = frontmatter::extract_epistemic(&content);

        let node = json!({
            "id": way.id,
            "domain": way.domain,
            "epistemic": epistemic,
            "description": fm.description,
        });
        serde_json::to_writer(&mut w, &node)?;
        w.write_all(b"\n")?;
        node_count += 1;

        // Edges point at node ids: an entry that names no node (a skill, a
        // subagent, a way without a description) is not an edge.
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
            serde_json::to_writer(&mut w, &edge)?;
            w.write_all(b"\n")?;
            edge_count += 1;
        }
    }

    w.flush()?;

    eprintln!("{node_count} nodes, {edge_count} edges");
    Ok(())
}

fn default_ways_dir() -> PathBuf {
    crate::paths::projected_ways_root()
}

