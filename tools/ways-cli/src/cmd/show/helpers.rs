//! Content rendering utilities — pure functions for file processing.

use std::path::Path;
use std::process::Command;

fn body_lines(content: &str) -> Vec<&str> {
    ways_core::frontmatter::split(content).map_or_else(Vec::new, |(_, body)| body.lines().collect())
}

/// Return check file sections (anchor and/or check).
pub(crate) fn check_sections_text(content: &str, include_anchor: bool) -> String {
    let mut section = String::new();
    let mut lines = Vec::new();

    for line in body_lines(content) {
        if line.starts_with("## anchor") {
            section = "anchor".to_string();
            continue;
        }
        if line.starts_with("## check") {
            section = "check".to_string();
            continue;
        }
        if line.starts_with("## ") {
            section = "other".to_string();
            continue;
        }

        if section == "check" || (section == "anchor" && include_anchor) {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// Who a macro runs for. The hook payload never reaches a macro (no stdin, no
/// arguments), so everything it may need about its session is exported.
pub(crate) struct MacroRun<'a> {
    pub session_id: &'a str,
    pub project_dir: &'a str,
    /// `agent`, `teammate` or `subagent`: the scope the way is rendered for.
    /// A subagent's hooks report the parent's session id, so a macro that
    /// consumes session state checks this before taking the parent's.
    pub scope: &'a str,
}

/// Facts a macro may read that cost a transcript read, a settings read or a
/// stat to find. Computed only for a macro whose source names the variable.
pub(crate) trait MacroFacts {
    /// Tokens used, tokens remaining and percent of the window remaining.
    fn context(&self, run: &MacroRun) -> Option<(u64, u64, u64)>;
    /// Enabled plugin ids (`name@marketplace`), sorted.
    fn enabled_plugins(&self, run: &MacroRun) -> Vec<String>;
    /// The project-relative path of an executable project tool (`adr`, `doc`).
    fn project_tool(&self, run: &MacroRun, name: &str) -> Option<String>;
}

/// The environment a macro runs with. Always: `CLAUDE_SESSION_ID`,
/// `CLAUDE_PROJECT_DIR` (resolved, never empty), `WAYS_SESSIONS_ROOT` and
/// `WAYS_SCOPE`. On demand, when the macro's source names them:
/// `WAYS_CONTEXT_USED`, `WAYS_CONTEXT_REMAINING`, `WAYS_CONTEXT_PCT_REMAINING`,
/// `WAYS_ENABLED_PLUGINS` (one id per line), `WAYS_ADR_TOOL` and
/// `WAYS_DOC_TOOL`. A name with no value is removed from the environment
/// rather than inherited.
pub(crate) fn macro_env(source: &str, run: &MacroRun, facts: &dyn MacroFacts) -> Vec<(&'static str, Option<String>)> {
    let mut env: Vec<(&'static str, Option<String>)> = vec![
        ("CLAUDE_SESSION_ID", Some(run.session_id.to_string())),
        ("CLAUDE_PROJECT_DIR", Some(run.project_dir.to_string())),
        ("WAYS_SESSIONS_ROOT", Some(crate::session::sessions_root())),
        ("WAYS_SCOPE", Some(run.scope.to_string())),
    ];
    if source.contains("WAYS_CONTEXT_") {
        let ctx = facts.context(run);
        env.push(("WAYS_CONTEXT_USED", ctx.map(|c| c.0.to_string())));
        env.push(("WAYS_CONTEXT_REMAINING", ctx.map(|c| c.1.to_string())));
        env.push(("WAYS_CONTEXT_PCT_REMAINING", ctx.map(|c| c.2.to_string())));
    }
    if source.contains("WAYS_ENABLED_PLUGINS") {
        let plugins = facts.enabled_plugins(run);
        env.push(("WAYS_ENABLED_PLUGINS", (!plugins.is_empty()).then(|| plugins.join("\n"))));
    }
    for (var, tool) in [("WAYS_ADR_TOOL", "adr"), ("WAYS_DOC_TOOL", "doc")] {
        if source.contains(var) {
            env.push((var, facts.project_tool(run, tool)));
        }
    }
    env
}

/// [`MacroFacts`] read from the session, the settings files and the project.
pub(crate) struct LiveFacts;

impl MacroFacts for LiveFacts {
    fn context(&self, run: &MacroRun) -> Option<(u64, u64, u64)> {
        use crate::cmd::context as cx;
        let ctx = super::firing_transcript()
            .and_then(|t| cx::get_context_for_transcript(t).ok())
            .or_else(|| (!run.session_id.is_empty()).then(|| cx::get_context_for_session(run.session_id).ok()).flatten())
            .or_else(|| cx::get_context(Some(run.project_dir)).ok())?;
        Some((ctx.tokens_used, ctx.tokens_remaining, ctx.pct_remaining))
    }

    fn enabled_plugins(&self, run: &MacroRun) -> Vec<String> {
        let project = Path::new(run.project_dir).join(".claude");
        enabled_plugins(&[
            crate::paths::current_config_dir().join("settings.json"),
            project.join("settings.json"),
            project.join("settings.local.json"),
        ])
    }

    fn project_tool(&self, run: &MacroRun, name: &str) -> Option<String> {
        project_tool(Path::new(run.project_dir), name)
    }
}

/// Plugins enabled across Claude Code's settings layers, later layers
/// overriding earlier ones per plugin: the `enabledPlugins` map, whose `true`
/// entries are what `claude plugin list` reports as enabled. Read from the
/// files, so no macro spawns the Node CLI to ask.
pub(crate) fn enabled_plugins(layers: &[std::path::PathBuf]) -> Vec<String> {
    let mut state = std::collections::BTreeMap::new();
    for layer in layers {
        let Ok(text) = std::fs::read_to_string(layer) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        if let Some(map) = v.get("enabledPlugins").and_then(|m| m.as_object()) {
            for (id, on) in map {
                state.insert(id.clone(), on.as_bool() == Some(true));
            }
        }
    }
    state.into_iter().filter(|(_, on)| *on).map(|(id, _)| id).collect()
}

/// The first executable `docs/scripts/<name>`, `scripts/<name>` or
/// `tools/<name>` under `project`, as the project-relative path.
pub(crate) fn project_tool(project: &Path, name: &str) -> Option<String> {
    ["docs/scripts", "scripts", "tools"]
        .iter()
        .map(|dir| format!("{dir}/{name}"))
        .find(|rel| is_executable(&project.join(rel)))
}

pub(crate) fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// Execute a macro shell script with [`macro_env`] and return its stdout.
pub(crate) fn run_macro(macro_file: &Path, run: &MacroRun) -> Option<String> {
    let source = std::fs::read_to_string(macro_file).unwrap_or_default();
    let mut cmd = Command::new("bash");
    cmd.arg(macro_file);
    for (name, value) in macro_env(&source, run, &LiveFacts) {
        match value {
            Some(v) => cmd.env(name, v),
            None => cmd.env_remove(name),
        };
    }
    let output = cmd.stderr(std::process::Stdio::null()).output().ok()?;

    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    } else {
        None
    }
}

/// Check whether a project directory is in the trusted-project-macros list.
pub(crate) fn is_project_trusted(project_dir: &str) -> bool {
    let trust_file = crate::paths::trusted_project_macros();
    if let Ok(content) = std::fs::read_to_string(&trust_file) {
        content.lines().any(|line| line.trim() == project_dir)
    } else {
        false
    }
}

/// Extract attend signal types from frontmatter.
/// Looks for `type: attend` and collects `signals:` list items.
pub(crate) fn extract_attend_signals(content: &str) -> Vec<String> {
    let Some((fm, _)) = ways_core::frontmatter::split(content) else {
        return Vec::new();
    };
    let mut has_attend_type = false;
    let mut in_signals = false;
    let mut signals = Vec::new();

    for line in fm.lines() {
        let trimmed = line.trim();

        if trimmed == "type: attend" {
            has_attend_type = true;
        }

        if trimmed == "signals:" {
            in_signals = true;
            continue;
        }

        if in_signals {
            if let Some(signal) = trimmed.strip_prefix("- ") {
                signals.push(signal.trim().to_string());
            } else {
                in_signals = false;
            }
        }
    }

    if has_attend_type { signals } else { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts what a macro asked for, so a test sees which facts were read.
    #[derive(Default)]
    struct Fake {
        reads: std::cell::RefCell<Vec<&'static str>>,
    }

    impl MacroFacts for Fake {
        fn context(&self, _: &MacroRun) -> Option<(u64, u64, u64)> {
            self.reads.borrow_mut().push("context");
            Some((40_000, 160_000, 80))
        }
        fn enabled_plugins(&self, _: &MacroRun) -> Vec<String> {
            self.reads.borrow_mut().push("plugins");
            vec!["a@m".into(), "b@m".into()]
        }
        fn project_tool(&self, _: &MacroRun, name: &str) -> Option<String> {
            self.reads.borrow_mut().push("tool");
            (name == "adr").then(|| "docs/scripts/adr".to_string())
        }
    }

    fn value<'a>(env: &'a [(&str, Option<String>)], name: &str) -> Option<&'a str> {
        env.iter().find(|(n, _)| *n == name).and_then(|(_, v)| v.as_deref())
    }

    #[test]
    fn a_subagent_macro_gets_the_session_its_scope_and_the_root() {
        let run = MacroRun { session_id: "parent-sess", project_dir: "/srv/p", scope: "subagent" };
        let fake = Fake::default();
        let env = macro_env("echo hi", &run, &fake);
        assert_eq!(value(&env, "CLAUDE_SESSION_ID"), Some("parent-sess"));
        assert_eq!(value(&env, "CLAUDE_PROJECT_DIR"), Some("/srv/p"));
        assert_eq!(value(&env, "WAYS_SCOPE"), Some("subagent"));
        assert_eq!(value(&env, "WAYS_SESSIONS_ROOT").map(str::to_string), Some(crate::session::sessions_root()));
        assert!(fake.reads.borrow().is_empty(), "nothing named, nothing read");
    }

    #[test]
    fn facts_are_read_only_for_a_macro_that_names_them() {
        let run = MacroRun { session_id: "s", project_dir: "/srv/p", scope: "agent" };
        let fake = Fake::default();
        let env = macro_env("echo $WAYS_CONTEXT_REMAINING $WAYS_ENABLED_PLUGINS $WAYS_ADR_TOOL $WAYS_DOC_TOOL", &run, &fake);
        assert_eq!(value(&env, "WAYS_CONTEXT_USED"), Some("40000"));
        assert_eq!(value(&env, "WAYS_CONTEXT_REMAINING"), Some("160000"));
        assert_eq!(value(&env, "WAYS_CONTEXT_PCT_REMAINING"), Some("80"));
        assert_eq!(value(&env, "WAYS_ENABLED_PLUGINS"), Some("a@m\nb@m"));
        assert_eq!(value(&env, "WAYS_ADR_TOOL"), Some("docs/scripts/adr"));
        // Named but absent: removed, never inherited from the hook's environment.
        assert!(env.iter().any(|(n, v)| *n == "WAYS_DOC_TOOL" && v.is_none()));
        assert_eq!(*fake.reads.borrow(), vec!["context", "plugins", "tool", "tool"]);
    }

    #[test]
    fn enabled_plugins_layer_later_files_over_earlier() {
        let dir = std::env::temp_dir().join(format!("ways-plugins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let user = dir.join("user.json");
        let project = dir.join("project.json");
        std::fs::write(&user, r#"{"enabledPlugins":{"a@m":true,"b@m":true,"c@m":false}}"#).unwrap();
        std::fs::write(&project, r#"{"enabledPlugins":{"b@m":false,"c@m":true}}"#).unwrap();
        let got = enabled_plugins(&[user, dir.join("missing.json"), project]);
        assert_eq!(got, vec!["a@m".to_string(), "c@m".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn project_tool_takes_the_first_executable_location() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ways-tool-{}", std::process::id()));
        for rel in ["scripts/adr", "tools/adr", "docs/scripts/doc"] {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
        }
        std::fs::set_permissions(dir.join("tools/adr"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(project_tool(&dir, "adr").as_deref(), Some("tools/adr"));
        assert_eq!(project_tool(&dir, "doc"), None, "not executable");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extract_attend_signals_basic() {
        let content = "---\ntrigger:\n  type: attend\n  signals:\n    - context-pressure\n    - reflection-overdue\n---\nBody text.";
        let signals = extract_attend_signals(content);
        assert_eq!(signals, vec!["context-pressure", "reflection-overdue"]);
    }

    #[test]
    fn extract_attend_signals_not_attend() {
        let content = "---\ndescription: normal way\nvocabulary: test\n---\nBody.";
        let signals = extract_attend_signals(content);
        assert!(signals.is_empty());
    }

    #[test]
    fn extract_attend_signals_no_frontmatter() {
        let content = "Just a plain file.";
        let signals = extract_attend_signals(content);
        assert!(signals.is_empty());
    }

    #[test]
    fn extract_attend_signals_type_without_signals() {
        let content = "---\ntrigger:\n  type: attend\n---\nBody.";
        let signals = extract_attend_signals(content);
        assert!(signals.is_empty());
    }

    #[test]
    fn check_sections_keep_horizontal_rules() {
        let content = "---\ndescription: d\n---\n## anchor\nA\n## check\nfirst\n---\nsecond\n";
        assert_eq!(check_sections_text(content, false), "first\n---\nsecond");
        assert_eq!(check_sections_text(content, true), "A\nfirst\n---\nsecond");
    }
}
