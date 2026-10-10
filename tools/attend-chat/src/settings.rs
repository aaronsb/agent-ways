//! The chat's settings: the `attend.chat.*` keys of attend's schema
//! (ADR-503), in attend's user file, the one `ways settings` edits. The
//! chat reads them through `attend-config` at start and after `/config`
//! changes one, and writes them only through its writer.

use std::path::Path;

use agent_settings::load::{resolve, Layer};
use agent_settings::schema::show;
use serde_yaml::Value;

/// A value as the status line shows it: a string bare, anything else as YAML.
fn plain(v: &Value) -> String {
    v.as_str().map_or_else(|| show(v), str::to_string)
}
use agent_tui::{Jump, TabKeys};
use attend_config::{ChatConfig, Config, SCHEMA};

/// The chat's keys, by their short names under `attend.chat.`.
pub const KEYS: &[&str] = &["tabs.jump", "tabs.menu_on_repeat", "tabs.focus_key", "mouse"];

const PREFIX: &str = "attend.chat.";

/// The layers the chat's keys resolve through: attend's, for `dir`.
fn layers(dir: &Path) -> Vec<Layer> {
    attend_config::layers(dir)
}

/// The chat's settings as the layers for `dir` resolve them.
pub fn load(dir: &Path) -> ChatConfig {
    Config::from_layers(&layers(dir)).chat
}

/// The tab keys `c` asks for.
pub fn tab_keys(c: &ChatConfig) -> TabKeys {
    let (f2, ctrl_t) = match c.focus_key.as_str() {
        "f2" => (true, false),
        "ctrl-t" => (false, true),
        "none" => (false, false),
        _ => (true, true),
    };
    TabKeys { jump: Jump::parse(&c.jump).unwrap_or(Jump::Auto), menu_on_repeat: c.menu_on_repeat, f2, ctrl_t }
}

/// One line on every chat key: its value and the layer it comes from.
pub fn listing(dir: &Path) -> String {
    let layers = layers(dir);
    let mut out = Vec::new();
    for k in KEYS {
        let name = format!("{PREFIX}{k}");
        let Some(spec) = SCHEMA.keys.iter().find(|s| s.name == name) else { continue };
        let r = resolve(spec, &[], &layers);
        let from = r.layer.map_or("default", |i| layers[i].name.as_str());
        let value = r.value.as_ref().map(plain).unwrap_or_default();
        out.push(format!("{k} = {value} ({from})"));
    }
    format!("{} · /config <key> <value> sets one", out.join(" · "))
}

/// Set chat key `key` (its short name) to `raw` in the user file at
/// `user`: one key, as [`set_many`] sets several.
pub fn set(key: &str, raw: &str, user: &Path, dir: &Path) -> Result<String, String> {
    set_many(&[(key, raw)], user, dir)
}

/// Set chat keys, each a short name and a raw value, in the user file at
/// `user`, in one locked write: all or nothing. Says what changed.
///
/// The chat's keys are user-scoped, so no project layer can set them and
/// what is written is what the chat reads; nothing can override it today.
pub fn set_many(pairs: &[(&str, &str)], user: &Path, dir: &Path) -> Result<String, String> {
    let before = layers(dir);
    let mut values = Vec::new();
    for (key, raw) in pairs {
        let short = key.trim_start_matches(PREFIX);
        if !KEYS.contains(&short) {
            return Err(format!("/config: {key} is not a chat key ({})", KEYS.join(", ")));
        }
        let name = format!("{PREFIX}{short}");
        let spec = SCHEMA.keys.iter().find(|s| s.name == name).ok_or_else(|| format!("/config: {key} is not in attend's schema"))?;
        let value = spec.parse_cli(raw, &before, &[]).map_err(|e| format!("/config {short}: {e}"))?;
        values.push((name, short.to_string(), value));
    }
    let writes: Vec<(&str, Value)> = values.iter().map(|(n, _, v)| (n.as_str(), v.clone())).collect();
    let changed = attend_config::write(user, &writes).map_err(|e| format!("/config: {e}"))?;
    let shown: Vec<String> = values.iter().map(|(_, s, v)| format!("{s} = {}", plain(v))).collect();
    let when = if values.iter().any(|(_, s, _)| s == "mouse") { "; mouse takes effect on restart" } else { "" };
    Ok(if changed { format!("{}{when}", shown.join(", ")) } else { format!("already {}", shown.join(", ")) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_keys_follow_the_settings() {
        let mut c = Config::default().chat;
        assert_eq!(tab_keys(&c), TabKeys::default(), "the defaults are the shell's");
        c.jump = "alt".into();
        c.focus_key = "ctrl-t".into();
        c.menu_on_repeat = false;
        assert_eq!(tab_keys(&c), TabKeys { jump: Jump::Alt, menu_on_repeat: false, f2: false, ctrl_t: true });
    }
}
