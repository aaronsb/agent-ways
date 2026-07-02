//! Assembled compliance findings — the classifier-ready dataset (ADR-201).
//!
//! A **finding** is one row of a would-be supervised-classification dataset: the
//! *features* (a claim's `satisfied_when` criterion + the firing evidence) sit
//! beside an **empty label** (the `determination`) and empty provenance slots for
//! whatever eventually fills them. This module only *assembles* rows; it never
//! writes a determination. The classifying system is deliberately out of scope
//! (ADR-201 §3) — the guarantee here is purely structural: a clean, accessible,
//! unambiguous-to-label record.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::provenance::Claim;

/// The evidence tier a finding sits in (ADR-200 §3). Set by whether the claim
/// carries an assessable criterion — never a determination of anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// "Control-aligned guidance surfaced at the point of work." The firing log
    /// alone speaks to this; no criterion needed.
    Process,
    /// "And the work actually took the claimed shape." Needs the criterion judged
    /// against the transcript — the harder, model-judged label.
    Outcome,
}

/// The firing evidence for a way — the observable, deterministic feature column.
/// `sessions` are transcript pointers (a session id names its transcript).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FiringEvidence {
    pub total: u64,
    pub sessions: Vec<String>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
}

/// One assembled finding — a dataset row with an empty label.
///
/// Every `Option` field serializes explicitly (as `null` when empty) rather than
/// being omitted, so every row carries an identical column set — a uniform,
/// unambiguous-to-label dataset (ADR-201 §2), not a ragged one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub way: String,
    pub control: String,
    /// The determination criterion (feature). `null` → not yet assessable.
    pub criterion: Option<String>,
    pub tier: Tier,
    pub evidence: FiringEvidence,

    // --- the empty label + its provenance slots (ADR-201 §1–2) ---
    // ways-audit writes these `null` and never fills them; a classifier does.
    /// `satisfied` / `other-than-satisfied` — the label. Always `null` at assembly.
    pub determination: Option<String>,
    pub assessed_by: Option<String>,
    pub assessed_at: Option<String>,
    pub basis: Option<String>,

    // --- assembly provenance (who built the row, when) ---
    pub assembled_at: String,
    pub assembled_by: String,
}

/// Per-way firing aggregate, folded from the raw event stream once.
#[derive(Default)]
struct WayFiring {
    total: u64,
    sessions: Vec<String>,
    first_seen: Option<String>,
    last_seen: Option<String>,
}

fn index_firing(events: &[Value]) -> BTreeMap<String, WayFiring> {
    let mut idx: BTreeMap<String, WayFiring> = BTreeMap::new();
    for ev in events {
        if ev["event"].as_str() != Some("way_fired") {
            continue;
        }
        let Some(way) = ev["way"].as_str() else { continue };
        let entry = idx.entry(way.to_string()).or_default();
        entry.total += 1;
        if let Some(sess) = ev["session"].as_str() {
            if !entry.sessions.iter().any(|s| s == sess) {
                entry.sessions.push(sess.to_string());
            }
        }
        if let Some(ts) = ev["ts"].as_str() {
            // Lexicographic compare is chronological here: every event timestamp is
            // written by the same RFC3339 `…Z` writer, so string order == time order.
            if entry.first_seen.as_deref().map(|f| ts < f).unwrap_or(true) {
                entry.first_seen = Some(ts.to_string());
            }
            if entry.last_seen.as_deref().map(|l| ts > l).unwrap_or(true) {
                entry.last_seen = Some(ts.to_string());
            }
        }
    }
    idx
}

/// Assemble the finding dataset from the claim manifest and the firing events.
///
/// One row per (claimed way, control). The determination and its provenance are
/// left empty — assembly never labels (ADR-201 §1). `assembled_at` is injected so
/// this stays a pure function (the binary supplies `util::now_utc()`).
pub fn assemble(manifest: &Value, events: &[Value], assembled_at: &str) -> Vec<Finding> {
    let firing = index_firing(events);
    let mut findings = Vec::new();

    let Some(ways) = manifest["ways"].as_object() else {
        return findings;
    };

    for (way, entry) in ways {
        let Some(claim) = Claim::from_provenance_value(&entry["provenance"]) else {
            continue; // no claim → no finding
        };
        let ev = firing.get(way);
        let evidence = FiringEvidence {
            total: ev.map(|f| f.total).unwrap_or(0),
            sessions: ev.map(|f| f.sessions.clone()).unwrap_or_default(),
            first_seen: ev.and_then(|f| f.first_seen.clone()),
            last_seen: ev.and_then(|f| f.last_seen.clone()),
        };

        for control in claim.controls {
            let tier = if control.satisfied_when.is_some() {
                Tier::Outcome
            } else {
                Tier::Process
            };
            findings.push(Finding {
                way: way.clone(),
                control: control.id,
                criterion: control.satisfied_when,
                tier,
                evidence: evidence.clone(),
                determination: None,
                assessed_by: None,
                assessed_at: None,
                basis: None,
                assembled_at: assembled_at.to_string(),
                assembled_by: "ways-audit".to_string(),
            });
        }
    }

    findings.sort_by(|a, b| (&a.way, &a.control).cmp(&(&b.way, &b.control)));
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> Value {
        json!({
            "ways": {
                "sd/commits": {"provenance": {
                    "controls": [
                        {"id": "NIST CM-3", "justifications": ["x"],
                         "satisfied_when": "commits follow conventional format"},
                        {"id": "SOC 2 CC8.1", "justifications": ["y"]}
                    ]
                }},
                "meta/todos": {"provenance": null}
            }
        })
    }

    fn events() -> Vec<Value> {
        vec![
            json!({"event": "way_fired", "way": "sd/commits", "session": "s1", "ts": "2026-06-01T10:00:00Z"}),
            json!({"event": "way_fired", "way": "sd/commits", "session": "s1", "ts": "2026-06-02T10:00:00Z"}),
            json!({"event": "way_fired", "way": "sd/commits", "session": "s2", "ts": "2026-05-30T10:00:00Z"}),
        ]
    }

    #[test]
    fn assembles_one_row_per_control_with_empty_label() {
        let f = assemble(&manifest(), &events(), "2026-07-02T00:00:00Z");
        assert_eq!(f.len(), 2); // two controls on the one claimed way
        for row in &f {
            assert!(row.determination.is_none(), "assembler must never label");
            assert!(row.assessed_by.is_none());
            assert_eq!(row.assembled_by, "ways-audit");
        }
    }

    #[test]
    fn tier_follows_criterion_presence() {
        let f = assemble(&manifest(), &events(), "t");
        let cm3 = f.iter().find(|r| r.control == "NIST CM-3").unwrap();
        let cc81 = f.iter().find(|r| r.control == "SOC 2 CC8.1").unwrap();
        assert_eq!(cm3.tier, Tier::Outcome);
        assert_eq!(cm3.criterion.as_deref(), Some("commits follow conventional format"));
        assert_eq!(cc81.tier, Tier::Process);
        assert!(cc81.criterion.is_none());
    }

    #[test]
    fn firing_evidence_aggregates_sessions_and_range() {
        let f = assemble(&manifest(), &events(), "t");
        let row = &f[0];
        assert_eq!(row.evidence.total, 3);
        assert_eq!(row.evidence.sessions, vec!["s1", "s2"]);
        assert_eq!(row.evidence.first_seen.as_deref(), Some("2026-05-30T10:00:00Z"));
        assert_eq!(row.evidence.last_seen.as_deref(), Some("2026-06-02T10:00:00Z"));
    }

    #[test]
    fn ways_without_claims_produce_no_rows() {
        let empty = json!({"ways": {"meta/todos": {"provenance": null}}});
        assert!(assemble(&empty, &[], "t").is_empty());
    }
}
