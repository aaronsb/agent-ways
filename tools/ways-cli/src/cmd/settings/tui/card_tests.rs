//! The module card on the sensors and mod tabs: built-in and script
//! sensors, attend present and absent, and a mod in each state.

use std::path::{Path, PathBuf};

use agent_settings::load::Layer;
use agent_settings::LayerScope;
use agent_tui::tree::{Card, Node, Origin, StateTone};

use super::mods::{self, Read};
use super::sensors::{self, Described};
use super::{Ctx, Ways};
use crate::cmd::attend_mod::{Degraded, State, ATTEND, JUDGE, THEME};
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

/// attend's file at `home`, holding `text`.
fn attend_layer(home: &Path, text: &str) -> Layer {
    let path = home.join(".config/attend/config.yaml");
    Layer::from_text(&attend_config::schema::SCHEMA, "user", attend_config::schema::FILE, LayerScope::User, Some(&path), text)
}

fn described() -> Option<Vec<Described>> {
    let d = |name: &str, state: &str, description: &str, source: &str| Described {
        name: name.into(),
        kind: "builtin".into(),
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
        folder: "~/.local/share/agent-ways/plugins".into(),
    }
}

#[test]
fn a_mod_s_card_reads_each_state_as_the_notice_does() {
    let c = mods::card(&ATTEND, Some(&overview(ATTEND, State::Converged, &["0.29.0"], None)), &Read::Done);
    assert_eq!((c.origin, c.kind.as_deref()), (Origin::Builtin, Some("plugin")));
    assert_eq!(c.source, "attend@agent-ways · ~/.local/share/agent-ways/plugins");
    assert_eq!(c.version.as_deref(), Some("shipped 0.29.0 · installed 0.29.0"));
    assert_eq!(c.state, ("installed · enabled".to_string(), StateTone::Ok));
    assert_eq!(c.rows, [("critical".to_string(), "yes".to_string()), ("needs".to_string(), "Claude Code 2.1.296 or later".to_string())]);

    for (word, why, said) in [(State::Drift, Degraded::Missing, "not installed"), (State::Disabled, Degraded::Disabled, "disabled"), (State::Pending, Degraded::Pending, "pending")] {
        let o = overview(ATTEND, word, &[], Some(why));
        let c = mods::card(&ATTEND, Some(&o), &Read::Done);
        assert_eq!(c.state, (format!("{said} · critical"), StateTone::Err));
        assert!(mods::notice(std::slice::from_ref(&o))[1].contains(said), "the card and the notice agree");
        assert_eq!(c.version.as_deref(), Some("shipped 0.29.0"));
    }
    let removed = mods::card(&THEME, Some(&overview(THEME, State::Removed, &[], None)), &Read::Done);
    assert_eq!((removed.state.0.as_str(), removed.rows[0].1.as_str()), ("removed", "no"));
    let older = Overview { older: true, ..overview(ATTEND, State::Drift, &["0.28.0"], None) };
    assert_eq!(mods::card(&ATTEND, Some(&older), &Read::Done).state, ("update · an installed copy is older".to_string(), StateTone::Warn));
    let off = mods::card(&THEME, Some(&overview(THEME, State::Off, &[], None)), &Read::Done);
    assert_eq!(off.state.0, "off · not wanted");

    // ways-judge is critical only under the claude-code engine.
    let keyed = Overview { claude_code: false, ..overview(JUDGE, State::Off, &[], None) };
    assert_eq!(mods::card(&JUDGE, Some(&keyed), &Read::Done).rows[0].1, "no: only while gate.engine is claude-code");
    assert_eq!(mods::card(&JUDGE, Some(&overview(JUDGE, State::Converged, &["0.29.0"], None)), &Read::Done).rows[0].1, "yes: gate.engine is claude-code");

    // Unread: the card says where the read stands.
    let reading = mods::card(&ATTEND, None, &Read::Reading);
    assert_eq!((reading.state.0.as_str(), reading.source.as_str(), reading.version.as_deref()), ("reading…", "attend@agent-ways", None));
    assert_eq!(mods::card(&ATTEND, None, &Read::Failed("x".into())).state, ("state read failed".to_string(), StateTone::Err));
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
