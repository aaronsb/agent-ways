//! The gate tab's `judge` section (ADR-708 §7, §8): whether the relevance
//! judge can gate, read from `cmd::judge::readiness`, the one function setup,
//! the help footer and `ways status` read; which shipped profile an unset
//! engine resolves to; and each live session's resolved engine and why.
//! Every row is read-only: the settings above it change what it reports.

use std::collections::BTreeMap;

use agent_tui::tree::{Kind, Node, Setting};
use ways_agent_core::engine::{self, LiveSession};
use ways_agent_core::keys::Readiness;
use ways_agent_core::profile::{self, Profile};

/// What the section shows, read once per build of the tree.
#[derive(Debug, Clone)]
pub struct JudgeState {
    pub readiness: Readiness,
    /// The profile an unset `gate.engine` resolves to now; `None` with no key.
    pub unset: Option<String>,
    pub sessions: Vec<LiveSession>,
    /// The profiles after the user's patches, for their display names.
    pub profiles: BTreeMap<String, Profile>,
}

impl JudgeState {
    /// The state on this machine: the readiness, the key files, Claude
    /// Code's session records and each session's resolution file. Reads
    /// files only.
    pub fn live() -> Option<JudgeState> {
        let user = profile::UserLayer::load_with_findings(&profile::user_layer_path()).map(|(u, _)| u).unwrap_or_default();
        let profiles = profile::profiles(&user);
        let has_key = |p| ways_agent_core::keys::locate_file(p).is_some();
        Some(JudgeState {
            readiness: crate::cmd::judge::readiness(),
            unset: profile::first_with_key(&profiles, has_key).map(str::to_string),
            sessions: engine::live_sessions(),
            profiles,
        })
    }

    /// Each profile's display name, as (profile, name), for the rows'
    /// values.
    fn labels(&self) -> Vec<(String, String)> {
        self.profiles.iter().filter_map(|(n, p)| Some((n.clone(), p.display_name.clone()?))).collect()
    }
}

/// The section: the readiness, the unset engine, and the live sessions.
pub fn section(state: &JudgeState) -> Node {
    let labels = state.labels();
    let ro = |value: &str, source: &str| Setting::new(Kind::ReadOnly, value, source).labels(labels.clone());
    let s = &state.readiness;
    let mut readiness = Node::leaf(
        "readiness",
        format!(
            "{}\n\nRead as setup, the help footer and `ways status` read it: the engine named in gate.engine, its key file and last check, and for claude-code whether the ways-judge mod is installed and enabled. `ways judge-setup` offers the engines.",
            crate::cmd::judge::outcome_text(s).unwrap_or_default()
        ),
        ro(crate::cmd::judge::state_word(s), "gate.engine, key files, the ways-judge mod"),
    )
    .headed("judge readiness");
    if let Some((why, fix)) = crate::cmd::judge::reason(s) {
        readiness = readiness.with_finding(format!("{why}.\nFix: {fix}"));
    }
    let unset = Node::leaf(
        "unset engine",
        "The profile an unset gate.engine resolves to: the first shipped profile whose provider has a key file, anthropic then openrouter. claude-code takes no key, so it is chosen only by name.",
        ro(state.unset.as_deref().unwrap_or("none"), if state.unset.is_some() { "first key file" } else { "no key file" }),
    )
    .headed("unset engine");
    let mut rows: Vec<Node> = state.sessions.iter().map(|l| session_row(l, &labels)).collect();
    if rows.is_empty() {
        rows.push(Node::leaf(
            "(none live)",
            "No running Claude Code session is recorded in ~/.claude/sessions.",
            Setting::new(Kind::ReadOnly, "none", "Claude Code's session records"),
        )
        .headed("live sessions"));
    }
    let sessions = Node::group(
        "sessions",
        "Each running Claude Code session and the engine it resolved to at its first judge request, held for its life (ADR-708 §2). A session not yet resolved has made none. A change to gate.engine reaches the next session.",
        rows,
    )
    .headed("live sessions")
    .opened();
    Node::group(
        "judge",
        "Whether the relevance judge gates, and with which engine each live session judges.",
        vec![readiness, unset, sessions],
    )
    .columns(("state", "value"))
    .headed("relevance judge")
    .opened()
}

/// One live session: its engine, and the reason as where it came from. A
/// session left with no engine has its reason, which names the remedy, as
/// its finding, and the reason's kind as where it came from.
fn session_row(l: &LiveSession, labels: &[(String, String)]) -> Node {
    // No key with no engine named, or the gate off, is the operator's
    // setting, not a fault: only an engine that cannot run has a fix.
    let unavailable = l.engine.as_ref().filter(|e| e.resolution.is_unavailable() || e.resolution.reason.starts_with(engine::CONFIG));
    let (value, source) = match &l.engine {
        None => ("not resolved".to_string(), "no judge request yet".to_string()),
        Some(e) if unavailable.is_some() => ("none".to_string(), e.resolution.reason.split(':').next().unwrap_or_default().to_string()),
        Some(e) => (e.resolution.engine.clone().unwrap_or_else(|| "none".into()), e.resolution.reason.clone()),
    };
    let doc = "The engine this session resolved to at its first judge request, held for its life.";
    let mut row = Node::leaf(l.short_id(), doc, Setting::new(Kind::ReadOnly, value, source).labels(labels.to_vec())).headed(format!("session {}", l.session));
    if let Some(e) = unavailable {
        row = row.with_finding(e.resolution.reason.clone());
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use ways_agent_core::engine::{resolve_session, Resolution, SessionEngine, EXPLICIT};
    use ways_agent_core::profile::{shipped, Mode, Provider};

    fn live(id: &str, r: Resolution, provider: Option<Provider>) -> LiveSession {
        LiveSession { session: id.into(), engine: Some(SessionEngine { resolution: r, provider }) }
    }

    fn fixture(readiness: Readiness) -> JudgeState {
        let none = resolve_session(Some("claude-code"), &shipped(), false, |_| false);
        let fell = resolve_session(Some("claude-code"), &shipped(), false, |p| p == Provider::Anthropic);
        JudgeState {
            readiness,
            unset: None,
            sessions: vec![
                live("3f6c2a91-4b7e-4d10-9c55-2e8b7a1d0f42", Resolution { engine: Some("claude-code".into()), reason: EXPLICIT.into() }, Some(Provider::ClaudeCode)),
                live("bbbbbbbb-0000-4000-8000-000000000002", fell, Some(Provider::Anthropic)),
                live("cccccccc-0000-4000-8000-000000000003", none, None),
                LiveSession { session: "dddddddd-0000-4000-8000-000000000004".into(), engine: None },
            ],
            profiles: shipped(),
        }
    }

    fn row<'a>(n: &'a Node, name: &str) -> &'a Node {
        n.children.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no {name} under {}", n.name))
    }

    #[test]
    fn no_mod_and_no_key_reads_degraded_with_the_fix() {
        let s = section(&fixture(Readiness::NoMod));
        let r = row(&s, "readiness");
        assert_eq!(r.setting.as_ref().unwrap().value, "degraded");
        let f = r.finding.as_deref().unwrap();
        assert!(f.contains("Claude Code internal has no ways-judge mod") && f.contains("Fix: ways judge-setup (installs the mod, or adds an Anthropic or OpenRouter key)"), "{f}");
        assert!(!f.contains("--provider claude-code"), "{f}");
        let ok = section(&fixture(Readiness::Ready(Provider::ClaudeCode, Mode::Enforce)));
        assert!(row(&ok, "readiness").finding.is_none());
        assert!(row(&ok, "readiness").doc.starts_with("Relevance judge on (Claude Code internal)."), "{}", row(&ok, "readiness").doc);
    }

    #[test]
    fn a_disabled_mod_with_no_key_reads_degraded_with_the_enable_fix() {
        let s = section(&fixture(Readiness::ModDisabled));
        let r = row(&s, "readiness");
        assert_eq!(r.setting.as_ref().unwrap().value, "degraded");
        let f = r.finding.as_deref().unwrap();
        assert!(f.contains("Fix: claude plugin enable ways-judge@agent-ways, or ways judge-setup for a key"), "{f}");
    }

    #[test]
    fn each_session_shows_its_engine_with_the_display_name_and_why() {
        let s = section(&fixture(Readiness::NoMod));
        let sessions = row(&s, "sessions");
        let cc = row(sessions, "3f6c2a91").setting.as_ref().unwrap();
        assert_eq!((cc.value.as_str(), cc.source.as_str(), cc.label_of(&cc.value)), ("claude-code", "explicit", Some("Claude Code internal")));
        let fell = row(sessions, "bbbbbbbb");
        let st = fell.setting.as_ref().unwrap();
        assert_eq!((st.value.as_str(), st.source.as_str()), ("anthropic", "claude-code unavailable: no Claude Code host"));
        assert!(fell.finding.is_none(), "a key engine stands in: nothing to fix");
        let none = row(sessions, "cccccccc");
        assert_eq!(none.setting.as_ref().unwrap().source, "engine_unavailable", "the kind; the whole reason is the finding");
        assert_eq!(none.heading.as_deref(), Some("session cccccccc-0000-4000-8000-000000000003"));
        let f = none.finding.as_deref().unwrap();
        assert!(f.starts_with("engine_unavailable: claude-code unavailable: no Claude Code host"), "{f}");
        assert!(f.contains("configure an Anthropic or OpenRouter key"), "the fix line: {f}");
        assert_eq!(row(sessions, "dddddddd").setting.as_ref().unwrap().value, "not resolved");
        assert_eq!(row(&s, "unset engine").setting.as_ref().unwrap().value, "none");
    }
}
