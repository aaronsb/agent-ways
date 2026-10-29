//! Modules and their cards: built-in and script sensors (attend present
//! and absent, a user's and a project's), a mod in each state, and a
//! bundled, a user and an overriding theme.

use std::path::{Path, PathBuf};

use agent_settings::load::Layer;
use agent_settings::LayerScope;
use agent_tui::tree::{Card, Node, Origin, StateTone};

use super::mods;
use super::{Ctx, Ways};
use crate::cmd::attend_mod::{Degraded, State, ATTEND, JUDGE, THEME};
use crate::cmd::modules::mods::{of_mod, Unread};
use crate::cmd::modules::sensors::{self, Described};
use crate::cmd::modules::themes::of_theme;
use crate::cmd::modules::{self, Health, Kind, Row};
use crate::cmd::mods::{Attention, Overview};

fn home(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ways-card-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("proj")).unwrap();
    d
}

fn ctx(home: &Path, sensors: fn() -> Option<Vec<Described>>) -> Ctx {
    Ctx {
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
        sensors,
    }
}

/// attend's user file at `home`, holding `text`.
fn attend_layer(home: &Path, text: &str) -> Layer {
    let path = home.join(".config/attend/config.yaml");
    Layer::from_text(&attend_config::schema::SCHEMA, "user", attend_config::schema::FILE, LayerScope::User, Some(&path), text)
}

/// The project's attend file under `home`, holding `text`.
fn project_layer(home: &Path, text: &str) -> Layer {
    let path = home.join("proj/.claude/attend.yaml");
    Layer::from_text(&attend_config::schema::SCHEMA, "project", attend_config::schema::FILE, LayerScope::Project, Some(&path), text)
}

fn described() -> Option<Vec<Described>> {
    let d = |name: &str, state: &str, description: &str, source: &str| Described {
        name: name.into(),
        state: state.into(),
        description: description.into(),
        source: source.into(),
    };
    Some(vec![
        d("git", "active", "tracks dirty files, branch changes, and upstream divergence", "attend@0.33.0"),
        d("peers", "active", "discovers Claude Code sessions and reads peer signals", "sensor-peers@0.6.0"),
        d("processes", "not compiled", "", ""),
    ])
}

fn sensors_tab(home: &Path, attend: fn() -> Option<Vec<Described>>, layers: &[Layer]) -> Node {
    Ways::new(ctx(home, attend)).build(layers).into_iter().find(|t| t.name == "sensors").expect("the sensors tab")
}

fn card<'a>(tab: &'a Node, name: &str) -> &'a Card {
    tab.children.iter().find(|n| n.name == name).and_then(|n| n.card.as_ref()).unwrap_or_else(|| panic!("no card on {name}"))
}

#[test]
fn a_built_in_sensor_s_card_names_its_crate_and_version() {
    let h = home("builtin");
    let tab = sensors_tab(&h, described, &[]);
    let git = card(&tab, "git");
    assert_eq!((git.origin, git.kind.as_deref(), git.source.as_str(), git.version.as_deref()), (Origin::Builtin, Some("crate"), "attend", Some("0.33.0")));
    assert_eq!(git.summary, "tracks dirty files, branch changes, and upstream divergence");
    assert_eq!(git.state, ("enabled".to_string(), StateTone::Ok));
    assert_eq!(git.rows, [("interval".to_string(), "30s quiet, 10s busy".to_string()), ("threshold".to_string(), "2".to_string())]);
    let peers = card(&tab, "peers");
    assert_eq!((peers.source.as_str(), peers.version.as_deref()), ("sensor-peers", Some("0.6.0")));
    assert_eq!(card(&tab, "processes").state, ("not compiled".to_string(), StateTone::Warn));
    // One switched off in a file reads disabled.
    let off = sensors_tab(&h, described, &[attend_layer(&h, "sensors:\n  git:\n    enabled: false\n")]);
    assert_eq!(card(&off, "git").state, ("disabled".to_string(), StateTone::Plain));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn no_script_sensor_configured_leaves_the_built_ins_alone() {
    let h = home("none");
    let tab = sensors_tab(&h, described, &[]);
    let names: Vec<&str> = tab.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, attend_config::schema::BUILTINS);
    assert!(tab.children.iter().all(|n| n.card.as_ref().is_some_and(|c| c.origin == Origin::Builtin)));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn a_script_sensor_is_listed_with_its_card_and_the_generic_rows() {
    let h = home("script");
    let script = h.join("dl.sh");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    let layers = [attend_layer(&h, &format!("sensors:\n  downloads:\n    script: {}\n    interval: 120\n", script.display()))];
    let tab = sensors_tab(&h, described, &layers);
    let names: Vec<&str> = tab.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names.last(), Some(&"downloads"), "after the built-ins: {names:?}");
    let node = tab.children.iter().find(|n| n.name == "downloads").unwrap();
    let c = node.card.as_ref().unwrap();
    assert_eq!((c.origin, c.kind.as_deref(), c.version.as_deref()), (Origin::User, Some("script"), None));
    assert_eq!(c.at.as_deref(), Some("~/.config/attend/config.yaml"));
    assert_eq!(c.source, "~/dl.sh");
    assert_eq!(c.state, ("enabled".to_string(), StateTone::Ok));
    assert_eq!(c.rows[0], ("interval".to_string(), "120s quiet, 15s busy".to_string()));
    // Its rows are the keys a built-in has, plus its script, each one the
    // screens write.
    let rows: Vec<&str> = node.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(rows, ["enabled", "interval", "min_interval", "threshold", "decay_threshold", "requires", "script"]);
    let interval = node.children.iter().find(|n| n.name == "interval").and_then(|n| n.setting.as_ref()).unwrap();
    assert_eq!(interval.value, "120");
    assert!(interval.kind.editable() && interval.store.as_ref().is_some_and(|s| s.key == "attend.sensors.downloads.interval"), "{interval:?}");
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn a_script_sensor_is_listed_with_attend_absent() {
    let h = home("absent");
    let layers = [attend_layer(&h, "sensors:\n  mine:\n    script: .claude/sensors/mine.sh\n    enabled: false\n")];
    let tab = sensors_tab(&h, || None, &layers);
    let c = card(&tab, "mine");
    assert_eq!(c.origin, Origin::User);
    assert_eq!(c.source, "~/proj/.claude/sensors/mine.sh", "a relative script is under the project");
    assert_eq!(c.state, ("disabled · script not found".to_string(), StateTone::Warn));
    let git = card(&tab, "git");
    assert_eq!((git.origin, git.version.as_deref()), (Origin::Builtin, None));
    assert_eq!(git.source, "unknown: attend absent");
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn an_enabled_script_sensor_whose_script_is_gone_reads_as_broken() {
    let h = home("gone");
    let layers = [attend_layer(&h, "sensors:\n  mine:\n    script: /nonexistent/mine.sh\n")];
    assert_eq!(card(&sensors_tab(&h, described, &layers), "mine").state, ("enabled · script not found".to_string(), StateTone::Err));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn attend_s_json_reads_into_described_rows() {
    let rows = sensors::parse(r#"[{"name":"git","kind":"builtin","state":"active","description":"d","source":"attend@0.33.0","interval":30,"min_interval":10}]"#).unwrap();
    assert_eq!(rows[0].source, "attend@0.33.0");
    assert!(sensors::parse("not json").is_none() && sensors::parse("{}").is_none());
}

fn overview(m: crate::cmd::attend_mod::Mod, word: State, installed: &[&str], attention: Option<Degraded>) -> Overview {
    Overview {
        m,
        shipped: Some("0.29.0".into()),
        word,
        rows: vec![],
        older: false,
        claude_code: true,
        attention: attention.map(|why| Attention { dir: "~/.claude".into(), why, fix: "fix".into() }).into_iter().collect(),
        installed: installed.iter().map(|s| s.to_string()).collect(),
        folder: "/home/u/.local/share/agent-ways/plugins".into(),
    }
}

#[test]
fn a_mod_s_module_reads_each_state_as_the_notice_does() {
    let m = of_mod(&ATTEND, Ok(&overview(ATTEND, State::Converged, &["0.29.0"], None)));
    assert_eq!((m.kind, m.origin, m.path.as_deref()), (Kind::Plugin, modules::Origin::Builtin, None));
    assert_eq!(m.source, "attend@agent-ways · /home/u/.local/share/agent-ways/plugins");
    assert_eq!(m.version.as_deref(), Some("shipped 0.29.0 · installed 0.29.0"));
    assert_eq!(m.state, modules::State::new("installed · enabled", Health::Running));
    assert_eq!(m.rows, [Row::new("critical", "yes"), Row::new("needs", "Claude Code 2.1.296 or later")]);
    let c = super::card::card(&m, Path::new("/home/u"));
    assert_eq!((c.origin, c.kind.as_deref(), c.at.as_deref()), (Origin::Builtin, Some("plugin"), None));
    assert_eq!((c.source.as_str(), &c.state), ("attend@agent-ways · ~/.local/share/agent-ways/plugins", &("installed · enabled".to_string(), StateTone::Ok)));

    for (word, why, said) in [(State::Drift, Degraded::Missing, "not installed"), (State::Disabled, Degraded::Disabled, "disabled"), (State::Pending, Degraded::Pending, "pending")] {
        let o = overview(ATTEND, word, &[], Some(why));
        let m = of_mod(&ATTEND, Ok(&o));
        assert_eq!(m.state, modules::State::new(format!("{said} · critical"), Health::Broken));
        assert!(mods::notice(std::slice::from_ref(&o))[1].contains(said), "the card and the notice agree");
        assert_eq!(m.version.as_deref(), Some("shipped 0.29.0"));
    }
    let removed = of_mod(&THEME, Ok(&overview(THEME, State::Removed, &[], None)));
    assert_eq!((removed.state.word.as_str(), removed.rows[0].value.as_str()), ("removed", "no"));
    let older = Overview { older: true, ..overview(ATTEND, State::Drift, &["0.28.0"], None) };
    assert_eq!(of_mod(&ATTEND, Ok(&older)).state, modules::State::new("update · an installed copy is older", Health::Attention));
    assert_eq!(of_mod(&THEME, Ok(&overview(THEME, State::Off, &[], None))).state.word, "off · not wanted");

    // ways-judge is critical only under the claude-code engine.
    let keyed = Overview { claude_code: false, ..overview(JUDGE, State::Off, &[], None) };
    assert_eq!(of_mod(&JUDGE, Ok(&keyed)).rows[0].value, "no: only while gate.engine is claude-code");
    assert_eq!(of_mod(&JUDGE, Ok(&overview(JUDGE, State::Converged, &["0.29.0"], None))).rows[0].value, "yes: gate.engine is claude-code");

    // Unread: the module says where the read stands.
    let reading = of_mod(&ATTEND, Err(Unread::Reading));
    assert_eq!((reading.state.word.as_str(), reading.source.as_str(), reading.version.as_deref()), ("reading…", "attend@agent-ways", None));
    assert_eq!(of_mod(&ATTEND, Err(Unread::Failed)).state.health, Health::Broken);
    assert_eq!(of_mod(&ATTEND, Err(Unread::Missing)).state.word, "state not read");
}

#[test]
fn a_project_s_script_sensor_reads_as_the_project_s() {
    let h = home("project");
    let layers = [project_layer(&h, "sensors:\n  disk:\n    script: .claude/sensors/disk.sh\n")];
    let users = sensors::user_sensors(&layers, &h.join("proj"));
    assert_eq!(users.len(), 1);
    let sc = attend_config::Config::from_layers(&layers);
    let m = sensors::of_script_sensor(&users[0], &sc.sensors["disk"]);
    assert_eq!((m.kind, m.origin), (Kind::Script, modules::Origin::Project));
    assert_eq!(m.path, Some(h.join("proj/.claude/attend.yaml")));
    assert_eq!(m.source, h.join("proj/.claude/sensors/disk.sh").display().to_string());
    let tab = sensors_tab(&h, described, &layers);
    let c = card(&tab, "disk");
    assert_eq!((c.origin, c.at.as_deref()), (Origin::Project, Some("~/proj/.claude/attend.yaml")));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn a_theme_s_module_names_where_it_comes_from() {
    use agent_theme::Source;
    let nord = agent_theme::BUNDLED.iter().find(|(n, _)| *n == "nord").map(|(_, s)| agent_theme::parse(s).unwrap()).unwrap();
    let dir = Path::new("/home/u/.config/agent-ways/themes");
    let bundled = of_theme(&nord, Source::Bundled, None, false, Some(dir), "plain");
    assert_eq!((bundled.kind, bundled.origin, bundled.path.as_deref(), bundled.source.as_str()), (Kind::Theme, modules::Origin::Builtin, None, "bundled (read-only)"));
    assert_eq!(bundled.state, modules::State::new("inactive (Enter uses it)", Health::Idle));
    assert_eq!(bundled.rows, [Row::new("saves to", dir.display().to_string()), Row::new("shape", "plain (a: shape cycles it)")]);
    assert_eq!(bundled.summary, "nord · dark · background terminal");

    let file = dir.join("mine.theme");
    let user = of_theme(&nord, Source::User, Some(&file), true, Some(dir), "plain");
    assert_eq!((user.origin, user.path.as_deref(), user.source.as_str()), (modules::Origin::User, Some(file.as_path()), file.to_str().unwrap()));
    assert_eq!(user.state.health, Health::Running);
    let c = super::card::card(&user, Path::new("/home/u"));
    assert_eq!((c.at.as_deref(), c.source.as_str()), (None, "~/.config/agent-ways/themes/mine.theme"), "a source naming the file it was added in says it once");

    let over = of_theme(&nord, Source::Override, Some(&dir.join("nord.theme")), false, None, "plain");
    assert_eq!(over.rows[0], Row::new("overrides", "the bundled nord"));
    assert_eq!(over.rows[1], Row::new("saves to", "none: nothing saves"));
}

#[test]
fn the_mod_tab_puts_the_card_on_each_mod_node() {
    let h = home("modtab");
    let mut c = ctx(&h, || None);
    c.mods = || vec![overview(ATTEND, State::Converged, &["0.29.0"], None)];
    let tab = Ways::new(c).build(&[]).into_iter().find(|t| t.name == "mod").unwrap();
    let names: Vec<(&str, &str)> = tab.children.iter().map(|n| (n.name.as_str(), n.card.as_ref().map_or("", |c| c.state.0.as_str()))).collect();
    assert_eq!(names, [("attend", "installed · enabled"), ("ways-judge", "state not read"), ("ways-theme", "state not read")]);
    let _ = std::fs::remove_dir_all(&h);
}
