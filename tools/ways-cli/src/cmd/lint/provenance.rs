//! ADR-110 provenance sidecar validation.
//!
//! Walks each way directory for `provenance.yaml` files and validates
//! they parse as a YAML mapping containing at least one of `policy:`
//! or `controls:`. Structural only — doesn't verify policy URIs or
//! control identifiers against any external registry.

use anyhow::Result;
use std::path::Path;

pub(super) fn lint_provenance_sidecars(
    dir: &Path,
    ways_dir: &Path,
    errors: &mut u32,
) -> Result<()> {
    for path in crate::scanner::files(dir) {
        let path = path.as_path();
        if path.file_name().and_then(|n| n.to_str()) != Some("provenance.yaml") {
            continue;
        }

        let relpath = path.strip_prefix(ways_dir).unwrap_or(path);
        let content = std::fs::read_to_string(path)?;
        let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&content);

        match parsed {
            Ok(val) => {
                if let Some(map) = val.as_mapping() {
                    if map.get("policy").is_none() && map.get("controls").is_none() {
                        eprintln!(
                            "  ERROR: {} — invalid provenance sidecar (needs policy or controls)",
                            relpath.display()
                        );
                        *errors += 1;
                    }
                } else {
                    eprintln!(
                        "  ERROR: {} — provenance sidecar is not a YAML mapping",
                        relpath.display()
                    );
                    *errors += 1;
                }
            }
            Err(_) => {
                eprintln!(
                    "  ERROR: {} — provenance sidecar has invalid YAML",
                    relpath.display()
                );
                *errors += 1;
            }
        }
    }
    Ok(())
}
