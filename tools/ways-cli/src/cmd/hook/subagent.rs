//! Two-phase subagent injection (ADR-123): `ways hook task` matches the
//! delegation and writes a stash, and SubagentStart claims the oldest stash
//! and injects its ways. Subagents get the ways fresh, whatever the parent
//! session has already been shown.

use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::cmd::show;
use crate::session;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Stash {
    ways: Vec<String>,
    channels: Vec<String>,
    is_teammate: bool,
    team_name: String,
}

/// Whether a Task names an agent with its own definition (project, user or
/// plugin). Its `.md` is the agent's constitution, so ways injection would be
/// redundant, and its large delegation prompts overrun the embedder. Only a
/// plain name (`[A-Za-z0-9_-]+`, as Claude Code names agents) is looked up:
/// the value is model-written and becomes a path component.
pub fn is_defined_agent(name: &str, project_dir: &Path, claude_dir: &Path) -> bool {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return false;
    }
    let file = format!("{name}.md");
    if project_dir.join(".claude/agents").join(&file).is_file() || claude_dir.join("agents").join(&file).is_file() {
        return true;
    }
    let pattern = claude_dir.join("plugins/marketplaces/*/plugins/*/agents").join(&file);
    glob::glob(&pattern.to_string_lossy()).is_ok_and(|mut paths| paths.any(|p| p.is_ok()))
}

/// Claim the oldest stash by renaming it, so a parallel SubagentStart cannot
/// take the same one, then read and remove it.
fn claim_oldest(stash_dir: &Path) -> Option<Stash> {
    let mut stashes: Vec<PathBuf> = std::fs::read_dir(stash_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    stashes.sort();
    let oldest = stashes.into_iter().next()?;
    let claimed = oldest.with_extension("json.claimed");
    std::fs::rename(&oldest, &claimed).ok()?;
    let text = std::fs::read_to_string(&claimed).unwrap_or_default();
    let _ = std::fs::remove_file(&claimed);
    Some(serde_json::from_str(&text).unwrap_or_default())
}

/// SubagentStart: the context to inject, empty when there is none.
pub fn inject(session_id: &str, project_dir: &str) -> anyhow::Result<String> {
    let Some(stash) = claim_oldest(&session::session_dir(session_id).join("subagent-stash")) else {
        return Ok(String::new());
    };
    // A teammate's own hooks read this marker for the rest of its session.
    if stash.is_teammate {
        let dir = session::session_dir(session_id);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("teammate"), format!("{}\n", stash.team_name))?;
    }
    let scope = if stash.is_teammate { "teammate" } else { "subagent" };
    let mut context = String::new();
    for (i, way) in stash.ways.iter().enumerate().filter(|(_, w)| !w.is_empty()) {
        let out = show::subagent_way(way, session_id, scope)?;
        if out.is_empty() {
            continue;
        }
        context.push_str(&out);
        context.push_str("\n\n");
        let trigger = stash.channels.get(i).map_or("prompt", String::as_str);
        let domain = way.split('/').next().unwrap_or(way);
        let mut fields = vec![
            ("event", "way_fired"),
            ("way", way.as_str()),
            ("domain", domain),
            ("trigger", trigger),
            ("scope", scope),
            ("project", project_dir),
            ("session", session_id),
        ];
        if !stash.team_name.is_empty() {
            fields.push(("team", stash.team_name.as_str()));
        }
        session::log_event(&fields);
    }
    Ok(context.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defined_agents_are_found_in_each_place_and_odd_names_never_looked_up() {
        let base = std::env::temp_dir().join(format!("ways-agents-{}", std::process::id()));
        let (project, claude) = (base.join("project"), base.join("claude"));
        for f in [
            project.join(".claude/agents/local.md"),
            claude.join("agents/user.md"),
            claude.join("plugins/marketplaces/m/plugins/p/agents/plugged.md"),
        ] {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, "---\n---\n").unwrap();
        }
        for name in ["local", "user", "plugged"] {
            assert!(is_defined_agent(name, &project, &claude), "{name}");
        }
        for name in ["general-purpose", "*", "../agents/user", ""] {
            assert!(!is_defined_agent(name, &project, &claude), "{name}");
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn the_oldest_stash_is_claimed_once() {
        let dir = std::env::temp_dir().join(format!("ways-stash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("200.json"), r#"{"ways":["b/two"]}"#).unwrap();
        std::fs::write(dir.join("100.json"), r#"{"ways":["a/one"],"channels":["keyword"],"is_teammate":true,"team_name":"red"}"#).unwrap();
        let first = claim_oldest(&dir).unwrap();
        assert_eq!((first.ways, first.channels, first.is_teammate, first.team_name.as_str()),
            (vec!["a/one".to_string()], vec!["keyword".to_string()], true, "red"));
        assert_eq!(claim_oldest(&dir).unwrap().ways, vec!["b/two".to_string()]);
        assert!(claim_oldest(&dir).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
