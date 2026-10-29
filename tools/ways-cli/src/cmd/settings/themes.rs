//! `ways settings theme list|copy|rename|delete` (#851): the theme tab's
//! file actions on the command line (ADR-503: the screens and the CLI are
//! two ways in). Both go through `agent_tui::named` over the same
//! `Themes`, and keep the active choice through the same adapter call, so
//! they refuse and report alike.
//!
//! `ways settings theme export` prints a theme for readers outside
//! agent-ways (ADR-514): the resolved roles, or a target program's own
//! theme mapped from them in agent-theme.

use agent_settings::exit;
use agent_theme::{Background, ColorDepth, Kind, Rgb, Roles, Theme, TERMINAL};
use agent_tui::named::{self, Done, Refusal, Refused};
use agent_tui::{Adapter, Themes};
use serde_json::json;

use super::tui::{self, Ctx, Ways};
use super::{fail, Failure, Out};

/// The operation `ways settings theme` was asked for.
pub enum Op<'a> {
    Copy { from: &'a str, to: &'a str },
    Rename { from: &'a str, to: &'a str },
    Delete { name: &'a str },
}

fn load() -> (Ways, Themes) {
    let ways = Ways::new(Ctx::from_env(None));
    let layers = ways.layers();
    // The depth only shapes drawing, which nothing here does.
    let themes = tui::themes(&ways, &layers, ColorDepth::TrueColor);
    (ways, themes)
}

/// The themes on offer, then each user file that does not load: name,
/// origin with any override, state and why; with `--json`, each theme's
/// item (ADR-516 §5) and its label.
pub fn list(as_json: bool) -> Out {
    let (_, themes) = load();
    let entries = crate::cmd::modules::themes::all(&themes);
    if as_json {
        println!("{}", serde_json::to_string_pretty(&entries).unwrap_or_default());
        return Ok(());
    }
    let w = entries.iter().map(|e| e.item.name.len()).max().unwrap_or(0);
    for e in &entries {
        let i = &e.item;
        let overrides = i.overrides.iter().map(|o| format!(" · {} {}", o.origin.word(), o.how.word()));
        let origin: String = std::iter::once(i.origin.word().to_string()).chain(overrides).collect();
        let detail = i.detail.as_deref().map(|d| format!("  {d}")).unwrap_or_default();
        println!("{:<w$}  {origin}  {}{detail}", i.name, i.state.word());
    }
    Ok(())
}

fn refused(r: Refused) -> Failure {
    match r.why {
        Refusal::Missing => fail(exit::USAGE, format!("{}; `ways settings theme list` names the themes", r.message)),
        Refusal::Bundled | Refusal::Name | Refusal::Taken => fail(exit::REJECTED, r.message),
        Refusal::Write => fail(exit::WRITE_FAILED, r.message),
    }
}

/// Copy, rename or delete a theme file. The active choice follows a rename
/// of the active theme and falls back to the default when it is deleted;
/// the first line printed says so, the second names the file written or
/// removed.
pub fn run(op: Op, as_json: bool) -> Out {
    let (mut ways, mut themes) = load();
    let done = match op {
        Op::Copy { from, to } => named::copy(&mut themes, from, to),
        Op::Rename { from, to } => named::rename(&mut themes, from, to),
        Op::Delete { name } => named::delete(&mut themes, name),
    }
    .map_err(refused)?;
    let moves = themes.follow(&done);
    let msg = themes.settle(&done, |n| ways.choose_theme(n));
    let kept = moves.as_ref().is_none_or(|n| *n == themes.active);
    if as_json {
        let mut out = match &done {
            Done::Copied { from, to, .. } => json!({ "action": "copy", "from": from, "to": to }),
            Done::Renamed { from, to, .. } => json!({ "action": "rename", "from": from, "to": to }),
            Done::Deleted { name, .. } => json!({ "action": "delete", "name": name }),
        };
        out["file"] = json!(done.file());
        out["active"] = json!(themes.active);
        out["active_moved"] = json!(moves.is_some() && kept);
        out["message"] = json!(msg);
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else {
        println!("{msg}\n{}", themes.show(done.file()));
    }
    if !kept {
        return Err(fail(exit::WRITE_FAILED, "theme.active was not moved; `ways settings set theme.active <name>` sets it"));
    }
    Ok(())
}

/// What `ways settings theme export` prints.
pub enum Form {
    /// The roles from `Roles::derive`, floors applied.
    Roles,
    /// A Claude Code custom theme file.
    ClaudeCode,
}

fn hex_map<'a>(pairs: impl IntoIterator<Item = (&'a str, Rgb)>) -> serde_json::Map<String, serde_json::Value> {
    pairs.into_iter().map(|(k, c)| (k.to_string(), json!(c.hex()))).collect()
}

/// The roles of `t` as JSON: every colour role, a segment pair as `.fg`
/// and `.bg`, and the gradient when the theme has one.
fn roles_json(t: &Theme) -> serde_json::Value {
    let r = Roles::derive(t);
    let mut roles = hex_map([
        ("bg", r.bg),
        ("body", r.body),
        ("accent", r.accent),
        ("accent_dim", r.accent_dim),
        ("info", r.info),
        ("ok", r.ok),
        ("warn", r.warn),
        ("err", r.err),
        ("alt", r.alt),
        ("hot", r.hot),
        ("muted", r.muted),
        ("ink", r.ink),
        ("text", r.text),
        ("faded_ink", r.faded_ink),
        ("faded_text", r.faded_text),
        ("rule", r.rule),
        ("track", r.track),
        ("track_past", r.track_past),
        ("selection_bg", r.selection_bg),
        ("dark_seg", r.dark_seg),
    ]);
    for (n, p) in [("badge", r.badge), ("mode_browse", r.mode_browse), ("mode_edit", r.mode_edit), ("mode_review", r.mode_review), ("mode_apply", r.mode_apply)] {
        roles.insert(format!("{n}.fg"), json!(p.fg.hex()));
        roles.insert(format!("{n}.bg"), json!(p.bg.hex()));
    }
    let gradient = r.gradient.map(|g| json!({ "axis": g.axis.name(), "stops": g.stops().iter().map(|c| c.hex()).collect::<Vec<_>>() }));
    json!({
        "name": t.name,
        "label": t.label,
        "kind": if t.kind == Kind::Dark { "dark" } else { "light" },
        "background": if t.background == Background::Fill { "fill" } else { "terminal" },
        "roles": roles,
        "gradient": gradient,
    })
}

/// Print theme `name`, or the active theme, in `form`. The terminal theme
/// is the terminal's own colours, which have no value to print.
pub fn export(name: Option<&str>, form: Form) -> Out {
    let (_, themes) = load();
    let name = name.unwrap_or(&themes.active);
    if name == TERMINAL {
        return Err(fail(exit::REJECTED, "terminal is the terminal's own colours; it has nothing to export"));
    }
    let Some(t) = themes.get(name) else {
        return Err(fail(exit::USAGE, format!("no theme `{name}`; `ways settings theme list` names the themes")));
    };
    let out = match form {
        Form::Roles => roles_json(t),
        Form::ClaudeCode => {
            let cc = agent_theme::claude_code(t);
            json!({ "name": cc.name, "base": cc.base, "overrides": hex_map(cc.overrides) })
        }
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    Ok(())
}

/// The ways-theme mod's render keys under `mods.ways-theme`, in the order
/// the export and the theme tab show them.
pub const RENDER: [&str; 10] = ["rail", "tables", "shimmer", "charts", "markdown", "icons", "frames", "prompt_box", "copy", "paths"];

/// The `mods.ways-theme` settings the mod reads, as resolved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModSettings {
    /// Each of [`RENDER`] with its value: a bool or a choice's word.
    pub render: Vec<(&'static str, serde_json::Value)>,
    /// `spinner_words` and `done_words`; empty leaves the theme's.
    pub spinner: Vec<String>,
    pub done: Vec<String>,
}

impl ModSettings {
    /// The settings as `layers` resolve them.
    pub fn read(ways: &Ways, layers: &[agent_settings::Layer]) -> ModSettings {
        let get = |k: &str| ways.value(&format!("mods.ways-theme.{k}"), layers);
        let words = |k: &str| -> Vec<String> {
            get(k).and_then(|v| v.as_sequence().map(|s| s.iter().filter_map(|w| w.as_str().map(str::to_string)).collect())).unwrap_or_default()
        };
        let render = RENDER
            .iter()
            .map(|k| {
                let v = match get(k) {
                    Some(serde_yaml::Value::Bool(b)) => json!(b),
                    Some(serde_yaml::Value::String(s)) => json!(s),
                    _ => serde_json::Value::Null,
                };
                (*k, v)
            })
            .collect();
        ModSettings { render, spinner: words("spinner_words"), done: words("done_words") }
    }

    /// The words in effect under `t`: each list the setting's when set,
    /// else the theme's; `None` when both come out empty.
    pub fn words(&self, t: Option<&Theme>) -> Option<agent_theme::Words> {
        let pick = |own: &[String], theme: Option<&[String]>| if own.is_empty() { theme.unwrap_or_default().to_vec() } else { own.to_vec() };
        let w = agent_theme::Words { spinner: pick(&self.spinner, t.map(|t| t.words.spinner.as_slice())), done: pick(&self.done, t.map(|t| t.words.done.as_slice())) };
        (!w.is_empty()).then_some(w)
    }

    /// The render settings as the mod takes them under `t`: `prompt_box`
    /// gradient draws as frame where the theme has no gradient.
    pub fn render_under(&self, t: Option<&Theme>) -> serde_json::Map<String, serde_json::Value> {
        let flat = t.is_none_or(|t| t.gradient.is_none());
        self.render
            .iter()
            .map(|(k, v)| {
                let v = if *k == "prompt_box" && flat && v == "gradient" { json!("frame") } else { v.clone() };
                (k.to_string(), v)
            })
            .collect()
    }
}

/// What the ways-theme mod reads at session start: theme `t` (`None` for
/// terminal) with the roles Claude Code's grounds take, its gradient, the
/// words in effect and the render settings. Under terminal the words are
/// the settings' alone.
pub fn mod_json(t: Option<&Theme>, s: &ModSettings) -> serde_json::Value {
    let render = s.render_under(t);
    let words = s.words(t).map(|w| json!({ "spinner": w.spinner, "done": w.done }));
    let Some(t) = t else {
        return json!({ "theme": TERMINAL, "kind": "dark", "roles": null, "gradient": null, "words": words, "render": render });
    };
    let r = agent_theme::claude_code_roles(t);
    let roles = hex_map([
        ("bg", r.bg),
        ("body", r.body),
        ("accent", r.accent),
        ("accent_dim", r.accent_dim),
        ("info", r.info),
        ("ok", r.ok),
        ("warn", r.warn),
        ("err", r.err),
        ("alt", r.alt),
        ("hot", r.hot),
        ("muted", r.muted),
        ("rule", r.rule),
        ("selection_bg", r.selection_bg),
        ("subtle", r.track),
    ]);
    let gradient = t.gradient.map(|g| json!({ "axis": g.axis.name(), "stops": g.stops().iter().map(|c| c.hex()).collect::<Vec<_>>() }));
    json!({
        "theme": t.name,
        "kind": if t.kind == Kind::Dark { "dark" } else { "light" },
        "roles": roles,
        "gradient": gradient,
        "words": words,
        "render": render,
    })
}

/// Print what the ways-theme mod reads for theme `name`, or the active
/// theme, on one line.
pub fn export_mod(name: Option<&str>) -> Out {
    let (ways, themes) = load();
    let name = name.unwrap_or(&themes.active);
    let t = if name == TERMINAL {
        None
    } else {
        Some(themes.get(name).ok_or_else(|| fail(exit::USAGE, format!("no theme `{name}`; `ways settings theme list` names the themes")))?)
    };
    let s = ModSettings::read(&ways, &ways.layers());
    println!("{}", mod_json(t, &s));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_theme::ThemeSet;

    fn theme(name: &str) -> Theme {
        ThemeSet::bundled().get(name).cloned().unwrap()
    }

    fn defaults() -> ModSettings {
        let render = RENDER
            .iter()
            .map(|k| {
                let v = match *k {
                    "tables" => json!("on"),
                    "icons" => json!("unicode"),
                    "prompt_box" => json!("gradient"),
                    "paths" => json!("wrap"),
                    _ => json!(true),
                };
                (*k, v)
            })
            .collect();
        ModSettings { render, ..Default::default() }
    }

    #[test]
    fn a_theme_exports_its_claude_code_roles_gradient_words_and_render() {
        let t = theme("miami-nights");
        let v = mod_json(Some(&t), &defaults());
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["theme", "kind", "roles", "gradient", "words", "render"]);
        assert_eq!((v["theme"].as_str(), v["kind"].as_str()), (Some("miami-nights"), Some("dark")));
        let roles: Vec<&str> = v["roles"].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(roles, ["bg", "body", "accent", "accent_dim", "info", "ok", "warn", "err", "alt", "hot", "muted", "rule", "selection_bg", "subtle"]);
        // The roles are the Claude Code export's: lifted onto the ground band.
        let cc = agent_theme::claude_code_roles(&t);
        assert_eq!(v["roles"]["body"], json!(cc.body.hex()));
        assert_eq!(v["roles"]["accent"], json!(cc.accent.hex()));
        assert_eq!(v["gradient"], json!({ "axis": "vertical", "stops": ["#0d0221", "#241046", "#4a0e4e"] }));
        assert_eq!(v["words"], json!({ "spinner": ["Cruising", "Gliding", "Glowing", "Pulsing", "Racing", "Drifting"], "done": ["Cruised", "Glowed", "Docked", "Landed"] }));
        assert_eq!(
            v["render"],
            json!({ "rail": true, "tables": "on", "shimmer": true, "charts": true, "markdown": true, "icons": "unicode", "frames": true, "prompt_box": "gradient", "copy": true, "paths": "wrap" })
        );
        assert!(!v.to_string().contains('\n'), "one line");
    }

    #[test]
    fn terminal_exports_render_and_the_settings_words_alone() {
        let mut s = defaults();
        s.done = vec!["Moored".into()];
        assert_eq!(mod_json(None, &s)["words"], json!({ "spinner": [], "done": ["Moored"] }));
        let v = mod_json(None, &defaults());
        assert_eq!(
            v,
            json!({ "theme": "terminal", "kind": "dark", "roles": null, "gradient": null, "words": null,
                    "render": { "rail": true, "tables": "on", "shimmer": true, "charts": true, "markdown": true, "icons": "unicode", "frames": true, "prompt_box": "frame", "copy": true, "paths": "wrap" } })
        );
    }

    #[test]
    fn the_settings_words_win_list_by_list_and_none_is_null() {
        let mut s = defaults();
        s.spinner = vec!["Sailing".into()];
        let v = mod_json(Some(&theme("tampa")), &s);
        assert_eq!(v["words"], json!({ "spinner": ["Sailing"], "done": ["Struck", "Crossed", "Landed", "Plundered"] }));
        s.done = vec!["Moored".into()];
        assert_eq!(mod_json(Some(&theme("nord")), &s)["words"], json!({ "spinner": ["Sailing"], "done": ["Moored"] }));
        assert_eq!(mod_json(Some(&theme("nord")), &defaults())["words"], serde_json::Value::Null, "a theme with no [words] and no setting");
    }

    #[test]
    fn prompt_box_gradient_draws_as_frame_without_a_gradient() {
        assert_eq!(mod_json(Some(&theme("tampa")), &defaults())["render"]["prompt_box"], json!("gradient"));
        assert_eq!(mod_json(Some(&theme("nord")), &defaults())["render"]["prompt_box"], json!("frame"));
        let mut s = defaults();
        s.render.iter_mut().find(|(k, _)| *k == "prompt_box").unwrap().1 = json!("off");
        assert_eq!(mod_json(Some(&theme("nord")), &s)["render"]["prompt_box"], json!("off"), "only gradient moves");
    }

    #[test]
    fn paths_reaches_the_mod_as_set() {
        let mut s = defaults();
        assert_eq!(mod_json(None, &s)["render"]["paths"], json!("wrap"));
        s.render.iter_mut().find(|(k, _)| *k == "paths").unwrap().1 = json!("truncate");
        assert_eq!(mod_json(Some(&theme("nord")), &s)["render"]["paths"], json!("truncate"));
    }
}
