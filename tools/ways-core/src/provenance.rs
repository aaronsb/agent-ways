//! Compliance claim sidecar model and manifest builder (ADR-151 §1, ADR-110).
//!
//! A claim lives in a `provenance.yaml` sidecar beside a way (ADR-110). This
//! module scans the ways tree for those sidecars and builds an in-memory
//! manifest — the cross-referenced view (by policy, by control) that the
//! `ways-audit` compliance surface queries. Nothing is persisted; the manifest
//! is rebuilt at query time.
//!
//! The manifest is currently a loosely-typed `serde_json::Value`. A typed claim
//! schema with an assessability (`satisfied_when`) field is the ADR-151 §3 /
//! ADR-200 §1 direction, layered on in a later change.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::util::{has_frontmatter, home_dir};

/// The typed compliance-claim schema stored in a `provenance.yaml` sidecar
/// (ADR-110, ADR-151 §3). The manifest builder above stays `Value`-based for the
/// query layer; this type is the single-sourced, documented schema and the lens
/// the finding assembler reads through (ADR-201).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Claim {
    #[serde(default)]
    pub policy: Vec<Policy>,
    #[serde(default)]
    pub controls: Vec<Control>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}

/// A source policy document a claim derives from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub uri: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// A control the way *claims* to be designed for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Control {
    pub id: String,
    #[serde(default)]
    pub justifications: Vec<String>,
    /// The determination criterion (ADR-200 §1, ADR-201): the observable session
    /// behavior that would let a classifier decide this control *satisfied* or
    /// *other than satisfied*. Absent → the control is not yet assessable; it can
    /// carry only a process-tier finding (that the way fired), never an outcome one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub satisfied_when: Option<String>,
}

impl Claim {
    /// Build a typed claim from a manifest way-entry's `provenance` object,
    /// tolerating the legacy bare-string control form (`- OWASP A01`) alongside
    /// the structured form. Returns `None` when the entry carries no claim.
    pub fn from_provenance_value(prov: &Value) -> Option<Claim> {
        let obj = prov.as_object()?;
        let policy = obj
            .get("policy")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let uri = p.get("uri")?.as_str()?.to_string();
                        let kind = p.get("type").and_then(|v| v.as_str()).map(str::to_string);
                        Some(Policy { uri, kind })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let controls = obj
            .get("controls")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        if let Some(s) = c.as_str() {
                            return Some(Control {
                                id: s.to_string(),
                                justifications: Vec::new(),
                                satisfied_when: None,
                            });
                        }
                        let o = c.as_object()?;
                        Some(Control {
                            id: o.get("id")?.as_str()?.to_string(),
                            justifications: o
                                .get("justifications")
                                .and_then(|v| v.as_array())
                                .map(|a| a.iter().filter_map(|j| j.as_str().map(str::to_string)).collect())
                                .unwrap_or_default(),
                            satisfied_when: o
                                .get("satisfied_when")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Claim {
            policy,
            controls,
            verified: obj.get("verified").and_then(|v| v.as_str()).map(str::to_string),
            rationale: obj.get("rationale").and_then(|v| v.as_str()).map(str::to_string),
        })
    }
}

/// Build the full compliance-claim manifest as a JSON value.
///
/// Scans `ways_dir` (or the default core ways root) for ways and their
/// `provenance.yaml` sidecars, then indexes coverage by policy and by control.
pub fn generate_manifest(ways_dir: Option<String>) -> Result<Value> {
    let root = ways_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude/hooks/ways"));

    let (ways, with_prov, without_prov) = scan_provenance(&root)?;

    let mut by_policy: HashMap<String, Vec<String>> = HashMap::new();
    let mut by_control: HashMap<String, Vec<String>> = HashMap::new();

    for (way_key, way_data) in &ways {
        if let Some(prov) = way_data.get("provenance").and_then(|v| v.as_object()) {
            if let Some(policies) = prov.get("policy").and_then(|v| v.as_array()) {
                for policy in policies {
                    if let Some(uri) = policy.get("uri").and_then(|v| v.as_str()) {
                        by_policy
                            .entry(uri.to_string())
                            .or_default()
                            .push(way_key.clone());
                    }
                }
            }
            if let Some(controls) = prov.get("controls").and_then(|v| v.as_array()) {
                for control in controls {
                    let cid = control
                        .get("id")
                        .and_then(|v| v.as_str())
                        .or_else(|| control.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !cid.is_empty() {
                        by_control.entry(cid).or_default().push(way_key.clone());
                    }
                }
            }
        }
    }

    Ok(json!({
        "manifest_version": "1.0.0",
        "generator": "ways-audit",
        "ways_scanned": ways.len(),
        "ways_with_provenance": with_prov.len(),
        "ways_without_provenance": without_prov.len(),
        "ways": ways,
        "coverage": {
            "with_provenance": with_prov,
            "without_provenance": without_prov,
            "by_policy": by_policy,
            "by_control": by_control,
        }
    }))
}

#[allow(clippy::type_complexity)]
fn scan_provenance(
    root: &Path,
) -> Result<(HashMap<String, Value>, Vec<String>, Vec<String>)> {
    let mut ways: HashMap<String, Value> = HashMap::new();
    let mut with_prov = Vec::new();
    let mut without_prov = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(".check."))
        {
            continue;
        }

        // Check frontmatter exists
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !has_frontmatter(&content) {
            continue;
        }

        let rel = path.strip_prefix(root).unwrap_or(path);
        let way_key = rel.parent().unwrap_or(Path::new("")).display().to_string();

        if way_key.is_empty() || !way_key.contains('/') {
            continue; // need at least domain/way
        }

        // Check for provenance.yaml sidecar
        let sidecar = path.parent().unwrap_or(path).join("provenance.yaml");
        let prov = if sidecar.is_file() {
            parse_sidecar(&sidecar).ok()
        } else {
            None
        };

        if let Some(ref p) = prov {
            with_prov.push(way_key.clone());
            ways.insert(
                way_key,
                json!({ "path": rel.display().to_string(), "provenance": p }),
            );
        } else {
            without_prov.push(way_key.clone());
            ways.insert(
                way_key,
                json!({ "path": rel.display().to_string(), "provenance": null }),
            );
        }
    }

    with_prov.sort();
    without_prov.sort();
    Ok((ways, with_prov, without_prov))
}

fn parse_sidecar(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path)?;
    let parsed: serde_yaml::Value = serde_yaml::from_str(&content)?;
    Ok(yaml_to_json(&parsed))
}

fn yaml_to_json(v: &serde_yaml::Value) -> Value {
    match v {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(b) => json!(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                json!(i)
            } else if let Some(f) = n.as_f64() {
                json!(f)
            } else {
                Value::Null
            }
        }
        serde_yaml::Value::String(s) => json!(s),
        serde_yaml::Value::Sequence(seq) => {
            json!(seq.iter().map(yaml_to_json).collect::<Vec<_>>())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in map {
                if let Some(key) = k.as_str() {
                    obj.insert(key.to_string(), yaml_to_json(val));
                }
            }
            Value::Object(obj)
        }
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("ways-prov-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
    }

    const WAY: &str = "---\ndescription: a way\n---\nguidance\n";

    #[test]
    fn indexes_claims_by_policy_and_control() {
        let root = scratch("index");
        write(&root, "sd/commits/commits.md", WAY);
        write(
            &root,
            "sd/commits/provenance.yaml",
            "policy:\n  - uri: governance/policies/code.md\n    type: governance-doc\ncontrols:\n  - id: NIST CM-3\n    justifications:\n      - structured change records\nverified: 2026-02-05\nrationale: because\n",
        );
        // A way with no sidecar → counted as without-provenance.
        write(&root, "meta/todos/todos.md", WAY);

        let m = generate_manifest(Some(root.display().to_string())).unwrap();
        assert_eq!(m["ways_scanned"], 2);
        assert_eq!(m["ways_with_provenance"], 1);
        assert_eq!(m["ways_without_provenance"], 1);
        assert!(m["coverage"]["by_control"]["NIST CM-3"]
            .as_array()
            .unwrap()
            .contains(&json!("sd/commits")));
        assert!(m["coverage"]["by_policy"]["governance/policies/code.md"]
            .as_array()
            .unwrap()
            .contains(&json!("sd/commits")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_string_controls_are_indexed() {
        let root = scratch("legacy");
        write(&root, "sd/w/w.md", WAY);
        write(
            &root,
            "sd/w/provenance.yaml",
            "controls:\n  - OWASP A01\nverified: 2026-01-01\n",
        );
        let m = generate_manifest(Some(root.display().to_string())).unwrap();
        assert!(m["coverage"]["by_control"]["OWASP A01"]
            .as_array()
            .unwrap()
            .contains(&json!("sd/w")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
