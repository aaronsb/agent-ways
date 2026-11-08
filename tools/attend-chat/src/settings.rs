// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The chat's settings: the `attend.chat.*` keys of attend's schema
//! (ADR-503), in attend's user file, the one `ways settings` edits. The
//! chat reads them through `attend-config` at start and after `/config`
//! changes one, and writes them only through its writer.

use std::path::Path;

use agent_settings::load::{resolve, Layer};
use agent_settings::schema::show;
use serde_yaml::Value;

/// A value as the status line shows it: a string bare, a list of strings
/// joined with commas (as `ways settings set` reads one back), anything
/// else as YAML.
pub(crate) fn plain(v: &Value) -> String {
    if let Some(items) = v.as_sequence() {
        if let Some(texts) = items.iter().map(Value::as_str).collect::<Option<Vec<_>>>() {
            return texts.join(", ");
        }
    }
    v.as_str().map_or_else(|| show(v), str::to_string)
}
use agent_tui::{Jump, TabKeys};
use attend_config::{ChatConfig, Config, SCHEMA};

/// The chat's keys, by their short names under `attend.chat.`.
pub const KEYS: &[&str] = &["tabs.jump", "tabs.menu_on_repeat", "tabs.focus_key", "mouse", "sidebar_key", "send_delay"];

const PREFIX: &str = "attend.chat.";

/// How long a write from the screen waits for another writer to let go of
/// a settings file, as the `ways settings` screens wait: the chat runs on
/// the same thread as its screen, so a holder that hangs must not hang it.
pub(crate) const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// The layers the chat's keys resolve through: attend's, for `dir`.
pub(crate) fn layers(dir: &Path) -> Vec<Layer> {
    attend_config::layers(dir)
}

/// The chat's settings as the layers for `dir` resolve them.
pub fn load(dir: &Path) -> ChatConfig {
    Config::from_layers(&layers(dir)).chat
}

/// The tab keys `c` asks for, on a terminal that does (`enhanced`) or does
/// not speak the kitty keyboard protocol.
///
/// Alt+digit jumps in every terminal (`auto`, `both`, `alt`). Ctrl+digit
/// jumps only where the protocol reports it as itself (`auto`, `both`,
/// `ctrl`): without it Ctrl+3 is the Esc byte, which recalls a message
/// waiting to send, Ctrl+2 is NUL, Ctrl+4..7 are FS, GS, RS and US and
/// Ctrl+8 is DEL, so no Ctrl+digit may stand for a channel there.
pub fn tab_keys(c: &ChatConfig, enhanced: bool) -> TabKeys {
    let (f2, ctrl_t) = match c.focus_key.as_str() {
        "f2" => (true, false),
        "ctrl-t" => (false, true),
        "none" => (false, false),
        _ => (true, true),
    };
    let jump = match (Jump::parse(&c.jump).unwrap_or(Jump::Auto), enhanced) {
        (Jump::Auto | Jump::Both, true) => Jump::Both,
        (Jump::Auto | Jump::Both | Jump::Alt, _) => Jump::Alt,
        (Jump::Ctrl, true) => Jump::Ctrl,
        (Jump::Ctrl | Jump::None, _) => Jump::None,
    };
    TabKeys { jump, menu_on_repeat: c.menu_on_repeat, f2, ctrl_t }
}

/// One line on every chat key: its value and the layer it comes from.
pub fn listing(dir: &Path) -> String {
    format!("{} · /config <key> <value> sets one", listing_lines(dir).join(" · "))
}

/// Each chat key as `key = value (layer)`, a line each.
pub fn listing_lines(dir: &Path) -> Vec<String> {
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
    out
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
    let changed = attend_config::write_within(user, Some(LOCK_WAIT), &writes).map_err(|e| format!("/config: {e}"))?;
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
        assert_eq!(tab_keys(&c, true), TabKeys { jump: Jump::Both, ..TabKeys::default() }, "Alt+digit, and Ctrl+digit where it arrives");
        assert_eq!(tab_keys(&c, false), TabKeys { jump: Jump::Alt, ..TabKeys::default() }, "Alt+digit alone without the kitty protocol");
        c.jump = "alt".into();
        c.focus_key = "ctrl-t".into();
        c.menu_on_repeat = false;
        assert_eq!(tab_keys(&c, true), TabKeys { jump: Jump::Alt, menu_on_repeat: false, f2: false, ctrl_t: true });
    }

    /// No setting makes a Ctrl+digit jump where it arrives as Esc or as
    /// another control key.
    #[test]
    fn a_ctrl_digit_jumps_only_with_the_kitty_protocol() {
        let mut c = Config::default().chat;
        for jump in ["auto", "ctrl", "alt", "both", "none"] {
            c.jump = jump.into();
            assert!(!tab_keys(&c, false).ctrl(false), "{jump}");
            assert_eq!(tab_keys(&c, false).alt(false), jump != "ctrl" && jump != "none", "{jump}");
            assert_eq!(tab_keys(&c, true).ctrl(true), jump != "alt" && jump != "none", "{jump}");
        }
    }
}
