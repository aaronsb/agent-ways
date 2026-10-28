//! The theme tab's Claude Code section, read for the screens: the
//! ways-theme mod in each enabled target, its render settings and the word
//! lists the settings set. A change goes through `set_one`, as
//! `ways settings set` writes it.

use agent_settings::schema::{DefaultValue, Kind};
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
    let layers = ways.layers();
    let s = ModSettings::read(ways, &layers);
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
    let render = RENDER
        .iter()
        .zip(&s.render)
        .map(|(k, (_, v))| {
            let key = format!("mods.ways-theme.{k}");
            let spec = ways.reg.lookup(&key).map(|b| b.spec);
            let options = match spec.map(|s| s.kind) {
                Some(Kind::Bool) => vec!["true".to_string(), "false".to_string()],
                Some(Kind::Choice(o)) => o.iter().map(|o| o.to_string()).collect(),
                _ => Vec::new(),
            };
            let default = match spec.map(|s| &s.default) {
                Some(DefaultValue::Yaml(y)) => y.to_string(),
                _ => String::new(),
            };
            let value = v.as_str().map_or_else(|| v.to_string(), str::to_string);
            let doc = spec.map(|s| s.doc.to_string()).unwrap_or_default();
            Render { key, label: k.replace('_', " "), value, default, options, doc }
        })
        .collect();
    let file = tilde(&ways_core::paths::user_config(), &ways.ctx.home);
    ClaudeCode { targets, render, spinner: s.spinner, done: s.done, file }
}

/// Whether `key` is one the section changes.
pub fn owns(key: &str) -> bool {
    key.strip_prefix("mods.ways-theme.").is_some_and(|k| RENDER.contains(&k))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_section_changes_the_render_keys_alone() {
        assert!(super::owns("mods.ways-theme.prompt_box"));
        assert!(!super::owns("mods.ways-theme.install"), "installing goes through `ways manage mod`");
        assert!(!super::owns("theme.active"));
    }
}
