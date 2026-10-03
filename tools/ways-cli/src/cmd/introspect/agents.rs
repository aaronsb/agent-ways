//! Who fired each way (#814): the agent ids the event log records, named
//! for the timeline's Agent column.
//!
//! The log records `main` for the top-level agent and the subagent's id
//! otherwise. Claude Code writes each subagent's transcript under the
//! session's directory with a `meta.json` beside it: a Task subagent at
//! `subagents/agent-<id>.meta.json`, whose `agentType` is the dispatch's
//! `subagent_type`, and a workflow member at
//! `subagents/workflows/<run>/agent-<id>.meta.json`, whose `description`
//! is its label in the run. The hook input does not say which kind an
//! agent is; the directory does. The log records an agent under
//! [`crate::session::agent_key`] of its id, so a file's id is keyed the same
//! way before it is matched.

use std::collections::HashMap;
use std::path::Path;

use agent_tui::ratatui::style::Style;
use agent_tui::theme;

use super::frames::MAIN;
use super::model::WayEvent;

/// What a subagent's `meta.json` says of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Meta {
    /// The dispatch's `subagent_type`, or `workflow-subagent`.
    pub(crate) agent_type: String,
    /// A workflow member's label in its run; empty for a Task subagent.
    pub(crate) workflow_label: Option<String>,
}

/// The agents of one session, in the order they first fired a way.
#[derive(Clone, Debug, Default)]
pub(crate) struct Agents {
    order: Vec<String>,
    meta: HashMap<String, Meta>,
    /// Each subagent's name: its base, and a short id where another agent
    /// of the session has the same base, so the two read apart without
    /// colour.
    names: HashMap<String, (String, Option<String>)>,
}

impl Agents {
    /// The agents that fired in `events`, with what `meta` knows of them.
    pub(crate) fn new(events: &[WayEvent], meta: HashMap<String, Meta>) -> Agents {
        let mut order: Vec<String> = Vec::new();
        for ev in events {
            let id = ev.agent_id.as_str();
            if !id.is_empty() && id != MAIN && !order.iter().any(|o| o == id) {
                order.push(id.to_string());
            }
        }
        let bases: Vec<String> = order.iter().map(|id| base_label(meta.get(id), id)).collect();
        let names = order
            .iter()
            .zip(&bases)
            .map(|(id, b)| {
                let shared = bases.iter().filter(|o| *o == b).count() > 1;
                (id.clone(), (b.clone(), shared.then(|| id.chars().take(5).collect())))
            })
            .collect();
        Agents { order, meta, names }
    }

    /// The widest name of the session's agents, `main` included.
    pub(crate) fn widest(&self) -> usize {
        self.order.iter().map(|id| agent_fmt::visible_len(&self.label(id))).max().unwrap_or(0).max(MAIN.len())
    }

    /// The agents of `events`, named from the subagent transcripts beside
    /// the session's `transcript`.
    pub(crate) fn read(events: &[WayEvent], transcript: Option<&Path>) -> Agents {
        let meta = transcript.map(|t| read_meta(&t.with_extension(""))).unwrap_or_default();
        Agents::new(events, meta)
    }

    /// The agent's `subagent_type`, where its transcript recorded one.
    pub(crate) fn agent_type(&self, id: &str) -> Option<&str> {
        self.meta.get(id).map(|m| m.agent_type.as_str()).filter(|t| !t.is_empty())
    }

    /// How the Agent column names it: `main`; a workflow member as `wf·`
    /// and its label; a subagent by its type (`general-purpose` as
    /// `general`); else a short id. Two agents of one base add `·` and the
    /// first five characters of their id.
    pub(crate) fn label(&self, id: &str) -> String {
        match self.parts(id) {
            (base, Some(suffix)) => format!("{base}·{suffix}"),
            (base, None) => base,
        }
    }

    /// The label's base and its disambiguating suffix, if any.
    pub(crate) fn parts(&self, id: &str) -> (String, Option<String>) {
        if id == MAIN || id.is_empty() {
            return (id.to_string(), None);
        }
        self.names.get(id).cloned().unwrap_or_else(|| (base_label(self.meta.get(id), id), None))
    }

    /// Main muted; each subagent a categorical colour from agent-identity,
    /// by the order it first fired, so the first few never share one.
    pub(crate) fn style(&self, id: &str) -> Style {
        if id == MAIN || id.is_empty() {
            return theme::muted();
        }
        let i = self
            .order
            .iter()
            .position(|o| o == id)
            .unwrap_or_else(|| agent_identity::identity::fnv1a_64(id.as_bytes()) as usize);
        match agent_identity::categorical(i).color(theme::current().depth()) {
            Some(c) => Style::new().fg(agent_theme::ratatui::color(c)),
            None => Style::new(),
        }
    }
}

/// An agent's name before any suffix.
fn base_label(meta: Option<&Meta>, id: &str) -> String {
    match meta {
        Some(Meta { workflow_label: Some(l), .. }) if !l.is_empty() => format!("wf·{l}"),
        Some(Meta { workflow_label: Some(_), .. }) => format!("wf·{}", short_id(id)),
        Some(m) if m.agent_type == "general-purpose" => "general".to_string(),
        Some(m) if !m.agent_type.is_empty() => m.agent_type.clone(),
        _ => short_id(id),
    }
}

/// Text from a meta file, without control characters: it reaches a cell.
fn clean(s: Option<&str>) -> String {
    s.unwrap_or("").chars().filter(|c| !c.is_control()).collect()
}

/// A subagent id as a few characters: a named agent's name
/// (`a<name>-<16 hex>`), else the id's first eight.
pub(crate) fn short_id(id: &str) -> String {
    if let Some((head, tail)) = id.rsplit_once('-') {
        if tail.len() == 16 && tail.bytes().all(|b| b.is_ascii_hexdigit()) {
            if let Some(name) = head.strip_prefix('a').filter(|n| !n.is_empty()) {
                return name.to_string();
            }
        }
    }
    id.chars().take(8).collect()
}

/// The `meta.json` of each subagent under `session_dir/subagents`, by id.
fn read_meta(session_dir: &Path) -> HashMap<String, Meta> {
    let mut out = HashMap::new();
    let dir = session_dir.join("subagents");
    read_dir_meta(&dir, false, &mut out);
    if let Ok(runs) = std::fs::read_dir(dir.join("workflows")) {
        for run in runs.flatten() {
            read_dir_meta(&run.path(), true, &mut out);
        }
    }
    out
}

fn read_dir_meta(dir: &Path, workflow: bool, out: &mut HashMap<String, Meta>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_prefix("agent-").and_then(|n| n.strip_suffix(".meta.json")) else { continue };
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let agent_type = clean(v["agentType"].as_str());
        let workflow = workflow || agent_type == "workflow-subagent";
        out.insert(
            crate::session::agent_key(id),
            Meta { agent_type, workflow_label: workflow.then(|| clean(v["description"].as_str())) },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fire(agent: &str) -> WayEvent {
        WayEvent { event: "way_fired".into(), way: "d/a".into(), agent_id: agent.into(), ..Default::default() }
    }

    #[test]
    fn labels_name_main_the_type_a_workflow_member_or_a_short_id() {
        let meta = HashMap::from([
            ("a1111111111111111".to_string(), Meta { agent_type: "code-reviewer".into(), workflow_label: None }),
            ("a2222222222222222".to_string(), Meta { agent_type: "workflow-subagent".into(), workflow_label: Some("audit:judge".into()) }),
        ]);
        let a = Agents::new(&[fire("main"), fire("a1111111111111111"), fire("a2222222222222222"), fire("a3333333333333333")], meta);
        assert_eq!(a.label("main"), "main");
        assert_eq!(a.label("a1111111111111111"), "code-reviewer");
        assert_eq!(a.label("a2222222222222222"), "wf·audit:judge");
        assert_eq!(a.label("a3333333333333333"), "a3333333");
        assert_eq!(a.label("akernel-td-0a616d306c8e1ad9"), "kernel-td");
        assert_eq!(a.agent_type("a3333333333333333"), None);
    }

    #[test]
    fn meta_is_read_from_the_session_directory() {
        let tmp = std::env::temp_dir().join(format!("ways-814-agents-{}", std::process::id()));
        let session = tmp.join("s1");
        let wf = session.join("subagents/workflows/wf_x");
        std::fs::create_dir_all(&wf).unwrap();
        std::fs::write(session.join("subagents/agent-ab.meta.json"), r#"{"agentType":"Explore"}"#).unwrap();
        // Control characters in a label are dropped as it is read.
        std::fs::write(wf.join("agent-ac.meta.json"), r#"{"agentType":"workflow-subagent","description":"audit:\u0007front-door\n"}"#).unwrap();
        // An id `agent_key` hashes: one longer than 64 characters.
        let long = "a".repeat(70);
        std::fs::write(session.join(format!("subagents/agent-{long}.meta.json")), r#"{"agentType":"code-reviewer"}"#).unwrap();
        let a = Agents::read(&[], Some(&tmp.join("s1.jsonl")));
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(a.label("ab"), "Explore");
        assert_eq!(a.label("ac"), "wf·audit:front-door");
        assert_eq!(a.agent_type("ac"), Some("workflow-subagent"));
        let key = crate::session::agent_key(&long);
        assert!(key.starts_with('h') && key.len() == 17, "{key}");
        assert_eq!(a.label(&key), "code-reviewer");
    }

    /// `general-purpose` reads `general`; two agents of one name add a
    /// short id, so they read apart without colour.
    #[test]
    fn agents_sharing_a_name_add_a_short_id() {
        let gp = || Meta { agent_type: "general-purpose".into(), workflow_label: None };
        let meta = HashMap::from([
            ("a3897aaaaaaaaaaaa".to_string(), gp()),
            ("a51c0bbbbbbbbbbbb".to_string(), gp()),
            ("a0000cccccccccccc".to_string(), Meta { agent_type: "Explore".into(), workflow_label: None }),
        ]);
        let a = Agents::new(&[fire("a3897aaaaaaaaaaaa"), fire("a51c0bbbbbbbbbbbb"), fire("a0000cccccccccccc")], meta);
        assert_eq!(a.label("a3897aaaaaaaaaaaa"), "general·a3897");
        assert_eq!(a.label("a51c0bbbbbbbbbbbb"), "general·a51c0");
        assert_eq!(a.label("a0000cccccccccccc"), "Explore");
        assert_eq!(a.widest(), 13);
    }
}
