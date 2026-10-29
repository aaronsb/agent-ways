//! The theme tab's Claude Code section, read for the screens: the
//! ways-theme mod in each enabled target, its render settings and the word
//! lists the settings set. A change goes through `set_one`, as
//! `ways settings set` writes it, to the user config.

use agent_settings::load::{resolve, Layer};
use agent_settings::schema::{DefaultValue, Kind};
use agent_settings::LayerScope;
use agent_tui::{ClaudeCode, Render};

use super::super::themes::{ModSettings, RENDER};
use super::build::tilde;
use super::Ways;
use crate::cmd::reconcile::{theme_claimed, theme_wanted};

/// The mod's state in the target at `dir`, and whether it is installed and
/// active there: wanted, and listed in Claude Code's `enabledPlugins`.
fn state(dir: &std::path::Path) -> (bool, &'static str) {
    if theme_claimed(dir) {
        (true, "installed and active")
    } else if theme_wanted(dir) {
        (false, "wanted, not installed")
    } else {
        (false, "not installed")
    }
}

/// What the section shows, as the files hold it now.
pub fn section(ways: &Ways) -> ClaudeCode {
    let config = crate::config::Config::load(&crate::util::project_dir());
    let targets = config
        .targets()
        .into_iter()
        .filter(|t| t.enabled)
        .map(|t| {
            let dir = t.dir();
            let (on, word) = state(&dir);
            (tilde(&dir, &ways.ctx.home), on, word.to_string())
        })
        .collect();
    let file = tilde(&ways_core::paths::user_config(), &ways.ctx.home);
    section_in(ways, &ways.layers(), targets, file)
}

/// The section over `layers`, with the targets and the user config's path
/// as shown. A render setting a project file sets names that file.
fn section_in(ways: &Ways, layers: &[Layer], targets: Vec<(String, bool, String)>, file: String) -> ClaudeCode {
    let s = ModSettings::read(ways, layers);
    let render = RENDER
        .iter()
        .zip(&s.render)
        .map(|(k, (_, v))| {
            let key = format!("mods.ways-theme.{k}");
            let bound = ways.reg.lookup(&key);
            let spec = bound.as_ref().map(|b| b.spec);
            let options = match spec.map(|s| s.kind) {
                Some(Kind::Bool) => vec!["true".to_string(), "false".to_string()],
                Some(Kind::Choice(o)) => o.iter().map(|o| o.to_string()).collect(),
                _ => Vec::new(),
            };
            let default = match spec.map(|s| &s.default) {
                Some(DefaultValue::Yaml(y)) => y.to_string(),
                _ => String::new(),
            };
            let project = bound
                .and_then(|b| resolve(b.spec, &b.bound, layers).layer)
                .and_then(|i| layers.get(i))
                .filter(|l| l.scope == LayerScope::Project)
                .map(|l| l.path.as_deref().map_or_else(|| l.name.clone(), |p| tilde(p, &ways.ctx.home)));
            let value = v.as_str().map_or_else(|| v.to_string(), str::to_string);
            let doc = spec.map(|s| s.doc.to_string()).unwrap_or_default();
            Render { key, label: k.replace('_', " "), value, default, options, doc, project }
        })
        .collect();
    ClaudeCode { targets, render, spinner: s.spinner, done: s.done, file }
}

/// Whether `key` is one the section changes.
pub fn owns(key: &str) -> bool {
    key.strip_prefix("mods.ways-theme.").is_some_and(|k| RENDER.contains(&k))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use agent_settings::load::Layer;
    use agent_settings::LayerScope;

    use super::super::{Ctx, Ways};

    #[test]
    fn the_section_changes_the_render_keys_alone() {
        assert!(super::owns("mods.ways-theme.prompt_box"));
        assert!(!super::owns("mods.ways-theme.install"), "installing goes through `ways manage mod`");
        assert!(!super::owns("theme.active"));
    }

    fn ways(home: &Path) -> Ways {
        Ways::new(Ctx {
            project: home.join("proj"),
            for_project: true,
            in_project: true,
            home: home.to_path_buf(),
            corpus: home.join("corpus"),
            user_ways: home.join(".config/agent-ways/ways"),
            themes: None,
            xdg_config: home.join(".config"),
            claude_config_dir: None,
            claude: home.join(".claude"),
            judge: || None,
            mods: Vec::new,
            mods_background: false,
        })
    }

    fn layer(name: &str, scope: LayerScope, path: &Path, text: &str) -> Layer {
        Layer::from_text(&ways_core::settings::SCHEMA, name, ways_core::settings::FILE, scope, Some(path), text)
    }

    #[test]
    fn a_render_setting_a_project_file_sets_names_that_file() {
        let home = Path::new("/h");
        let user = layer("user", LayerScope::User, &home.join(".config/agent-ways/config.yaml"), "mods:\n  ways-theme:\n    rail: false\n    copy: false\n");
        let project = layer("project", LayerScope::Project, &home.join("proj/.claude/ways.yaml"), "mods:\n  ways-theme:\n    copy: true\n    tables: text\n    paths: truncate\n");
        let w = ways(home);
        let cc = super::section_in(&w, &[user, project], vec![], "~/.config/agent-ways/config.yaml".into());
        let row = |k: &str| cc.render.iter().find(|r| r.key == format!("mods.ways-theme.{k}")).unwrap();
        assert_eq!((row("rail").value.as_str(), row("rail").project.as_deref()), ("false", None), "the user's own value names no project");
        assert_eq!((row("copy").value.as_str(), row("copy").project.as_deref()), ("true", Some("~/proj/.claude/ways.yaml")));
        assert_eq!((row("tables").value.as_str(), row("tables").project.as_deref()), ("text", Some("~/proj/.claude/ways.yaml")));
        assert_eq!((row("frames").value.as_str(), row("frames").project.as_deref()), ("true", None), "a default names nothing");
        let paths = row("paths");
        assert_eq!((paths.value.as_str(), paths.options.as_slice(), paths.default.as_str()), ("truncate", ["wrap".to_string(), "truncate".to_string()].as_slice(), "wrap"));
        assert_eq!(cc.render.len(), 10);
    }
}
