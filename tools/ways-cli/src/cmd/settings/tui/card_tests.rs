//! Modules and their cards: sensors of each kind and layer (attend present,
//! absent and unlisted), a mod in each state, and a bundled, a user and an
//! overriding theme.

use std::path::{Path, PathBuf};

use agent_settings::load::Layer;
use agent_settings::LayerScope;
use agent_tui::tree::{Card, Node, Origin, StateTone};
use attend_config::sensors::{file_layer, SHIPPED};
use sensor_spec::Origin as From;

use super::mods;
use super::{Ctx, Ways};
use crate::cmd::attend_mod::{Degraded, State, ATTEND, JUDGE, THEME};
use crate::cmd::modules::mods::{of_mod, Unread};
use crate::cmd::modules::sensors::{self, Attend, Described};
use crate::cmd::modules::themes::of_theme;
use crate::cmd::modules::{self, Health, Kind, Row};
use crate::cmd::mods::{Attention, Overview};

fn home(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ways-card-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("proj")).unwrap();
    d
}

fn ctx(home: &Path, sensors: fn() -> Attend) -> Ctx {
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

/// The shipped sensor files, as the settings layers hold them.
fn shipped() -> Vec<Layer> {
    SHIPPED.iter().map(|(n, t)| file_layer(n, From::Builtin, None, t)).collect()
}

/// The shipped files with a user's file for `name` under `home`.
fn with_user(home: &Path, name: &str, text: &str) -> Vec<Layer> {
    let mut l = shipped();
    l.push(file_layer(name, From::User, Some(&home.join(format!(".config/attend/sensors/{name}.yaml"))), text));
    l
}

/// A project's sensor file for `name` under `home`.
fn project_file(home: &Path, name: &str, text: &str) -> Layer {
    file_layer(name, From::Project, Some(&home.join(format!("proj/.claude/attend/sensors/{name}.yaml"))), text)
}

/// attend's user file at `home`, holding `text`.
fn attend_layer(home: &Path, text: &str) -> Layer {
    let path = home.join(".config/attend/config.yaml");
    Layer::from_text(&attend_config::schema::SCHEMA, "user", attend_config::schema::FILE, LayerScope::User, Some(&path), text)
}

fn described() -> Attend {
    let d = |name: &str, kind: &str, state: &str, source: &str, version: Option<&str>| Described {
        name: name.into(),
        kind: kind.into(),
        state: state.into(),
        source: source.into(),
        version: version.map(str::to_string),
        ..Described::default()
    };
    Attend::Listed(vec![
        d("git", "crate", "active", "attend", Some("0.33.0")),
        d("peers", "crate", "active", "sensor-peers", Some("0.6.0")),
        d("processes", "crate", "not compiled", "", None),
        Described { detail: Some("kind command not yet hosted".into()), ..d("weather", "command", "not hosted", "curl", None) },
        Described { detail: Some("poll timed out after 10 s".into()), ..d("context", "crate", "failed", "attend", Some("0.33.0")) },
    ])
}

fn sensors_tab(home: &Path, attend: fn() -> Attend, layers: &[Layer]) -> Node {
    Ways::new(ctx(home, attend)).build(layers).into_iter().find(|t| t.name == "sensors").expect("the sensors tab")
}

fn card<'a>(tab: &'a Node, name: &str) -> &'a Card {
    tab.children.iter().find(|n| n.name == name).and_then(|n| n.card.as_ref()).unwrap_or_else(|| panic!("no card on {name}"))
}

fn row<'a>(c: &'a Card, label: &str) -> Option<&'a str> {
    c.rows.iter().find(|(l, _)| l == label).map(|(_, v)| v.as_str())
}

#[test]
fn a_shipped_crate_sensor_s_card_names_its_crate_version_and_cadence() {
    let h = home("builtin");
    let tab = sensors_tab(&h, described, &shipped());
    let git = card(&tab, "git");
    assert_eq!((git.origin, git.kind.as_deref(), git.source.as_str(), git.version.as_deref()), (Origin::Builtin, Some("crate"), "attend", Some("0.33.0")));
    assert_eq!(git.summary, "tracks dirty files, branch changes, and upstream divergence");
    assert_eq!(git.state, ("active".to_string(), StateTone::Ok));
    assert_eq!(git.rows, [("interval".into(), "30s quiet, 10s busy".into()), ("threshold".into(), "2".into()), ("lane".into(), "event".into())]);
    let peers = card(&tab, "peers");
    assert_eq!((peers.source.as_str(), peers.version.as_deref()), ("sensor-peers", Some("0.6.0")));
    assert_eq!(card(&tab, "processes").state, ("not compiled".to_string(), StateTone::Warn));
    assert_eq!(card(&tab, "processes").source, "not compiled into this attend");
    let failed = card(&tab, "context");
    assert_eq!((failed.state.clone(), row(failed, "why")), (("failed".to_string(), StateTone::Err), Some("poll timed out after 10 s")));
    // The examples ship off. One attend does not list reads from its file:
    // a command's program, where a crate's source is attend's to say.
    let example = card(&tab, "example-command");
    assert_eq!((example.state.clone(), example.source.as_str()), (("off".to_string(), StateTone::Plain), "sh"));
    assert_eq!(card(&tab, "keepwarm").source, "unknown: attend does not list it");
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn every_sensor_file_is_a_node_in_name_order_with_the_keys_each_sensor_has() {
    let h = home("names");
    let tab = sensors_tab(&h, || Attend::Absent, &shipped());
    let names: Vec<&str> = tab.children.iter().map(|n| n.name.as_str()).collect();
    let mut want: Vec<&str> = SHIPPED.iter().map(|(n, _)| *n).collect();
    want.sort();
    assert_eq!(names, want);
    assert!(tab.children.iter().all(|n| n.card.as_ref().is_some_and(|c| c.origin == Origin::Builtin)));
    let rows = |name: &str| tab.children.iter().find(|n| n.name == name).unwrap().children.iter().map(|n| n.name.clone()).collect::<Vec<_>>();
    assert_eq!(rows("git"), ["enabled", "interval", "min_interval", "threshold", "decay_threshold", "lane", "requires"]);
    assert_eq!(rows("processes").last().map(String::as_str), Some("watch"), "a processes crate sensor takes a watch list");
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn a_user_command_sensor_is_listed_with_its_file_and_its_rows_write_that_file() {
    let h = home("command");
    let layers = with_user(&h, "weather", "description: the wind at home\nkind: command\nargv: [curl, -s, https://example.org]\noutput: json\ninterval: 120\n");
    let tab = sensors_tab(&h, described, &layers);
    let node = tab.children.iter().find(|n| n.name == "weather").unwrap();
    let c = node.card.as_ref().unwrap();
    assert_eq!((c.origin, c.kind.as_deref(), c.version.as_deref()), (Origin::User, Some("command"), None));
    assert_eq!(c.at.as_deref(), Some("~/.config/attend/sensors/weather.yaml"));
    assert_eq!((c.summary.as_str(), c.source.as_str()), ("the wind at home", "curl"));
    assert_eq!(c.state, ("not hosted".to_string(), StateTone::Warn));
    assert_eq!(row(c, "interval"), Some("120s quiet, 15s busy"));
    assert_eq!(row(c, "why"), Some("kind command not yet hosted"));
    let interval = node.children.iter().find(|n| n.name == "interval").and_then(|n| n.setting.as_ref()).unwrap();
    assert_eq!(interval.value, "120");
    let store = interval.store.as_ref().unwrap();
    assert_eq!((store.key.as_str(), store.layer.as_str()), ("attend.sensors.weather.interval", "user"));
    assert_eq!(store.file, attend_config::sensors::user_dir().join("weather.yaml"));
    // With attend absent the file says what it can.
    let absent = sensors_tab(&h, || Attend::Absent, &layers);
    let c = card(&absent, "weather");
    assert_eq!((c.source.as_str(), c.state.clone()), ("curl", ("enabled".to_string(), StateTone::Ok)));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn a_project_mcp_sensor_is_refused_until_the_project_is_trusted() {
    let h = home("mcp");
    let mut layers = shipped();
    layers.push(project_file(&h, "inbox", "kind: mcp\nserver: slack\ntool: list_unread\n"));
    let tab = sensors_tab(&h, || Attend::Absent, &layers);
    let c = card(&tab, "inbox");
    assert_eq!((c.origin, c.kind.as_deref(), c.at.as_deref()), (Origin::Project, Some("mcp"), Some("~/proj/.claude/attend/sensors/inbox.yaml")));
    assert_eq!(c.state, ("refused".to_string(), StateTone::Warn));
    assert!(row(c, "why").is_some_and(|w| w.contains("attend.project_sensors")), "{:?}", c.rows);
    assert_eq!(c.source, "slack/list_unread");
    layers.push(attend_layer(&h, &format!("project_sensors: [{}]\n", h.join("proj").display())));
    assert_eq!(card(&sensors_tab(&h, || Attend::Absent, &layers), "inbox").state, ("enabled".to_string(), StateTone::Ok));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn an_invalid_sensor_file_shows_why_and_no_kind() {
    let h = home("invalid");
    let tab = sensors_tab(&h, || Attend::Absent, &with_user(&h, "broken", "interval: 30\n"));
    let c = card(&tab, "broken");
    assert_eq!((c.kind.as_deref(), c.state.clone()), (None, ("invalid".to_string(), StateTone::Err)));
    assert!(row(c, "why").is_some_and(|w| w.contains("kind is required")), "{:?}", c.rows);
    assert_eq!(c.summary, "a sensor file");
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn a_user_override_of_a_shipped_sensor_reads_as_the_user_s() {
    let h = home("override");
    let tab = sensors_tab(&h, || Attend::Absent, &with_user(&h, "git", "enabled: false\n"));
    let git = card(&tab, "git");
    assert_eq!((git.origin, git.at.as_deref()), (Origin::User, Some("~/.config/attend/sensors/git.yaml")));
    assert_eq!(git.state, ("off".to_string(), StateTone::Plain));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn attend_s_json_reads_into_described_rows() {
    let rows = sensors::parse(
        r#"[{"name":"git","kind":"crate","origin":"builtin","path":null,"state":"active","detail":null,"enabled":true,"lane":"event","description":"d","source":"attend","version":"0.33.0","interval":30,"min_interval":10,"threshold":2.0}]"#,
    )
    .unwrap();
    assert_eq!((rows[0].source.as_str(), rows[0].version.as_deref(), rows[0].kind.as_str()), ("attend", Some("0.33.0"), "crate"));
    assert_eq!((rows[0].interval, rows[0].min_interval, rows[0].threshold, rows[0].enabled), (Some(30), Some(10), Some(2.0), Some(true)));
    assert_eq!((rows[0].lane.as_deref(), rows[0].path.as_deref()), (Some("event"), None));
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
    assert_eq!((m.kind, m.origin, m.path.as_deref()), (Some(Kind::Plugin), modules::Origin::Builtin, None));
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
fn a_theme_s_module_names_where_it_comes_from() {
    use agent_theme::Source;
    let nord = agent_theme::BUNDLED.iter().find(|(n, _)| *n == "nord").map(|(_, s)| agent_theme::parse(s).unwrap()).unwrap();
    let dir = Path::new("/home/u/.config/agent-ways/themes");
    let bundled = of_theme(&nord, Source::Bundled, None, false, Some(dir), "plain");
    assert_eq!((bundled.kind, bundled.origin, bundled.path.as_deref(), bundled.source.as_str()), (Some(Kind::Theme), modules::Origin::Builtin, None, "bundled (read-only)"));
    assert_eq!(bundled.state, modules::State::new("inactive", Health::Idle));
    assert_eq!(bundled.rows, [Row::new("saves to", dir.display().to_string()), Row::new("shape", "plain (a: shape cycles it)")]);
    assert_eq!(bundled.summary, "nord · dark · background terminal");

    let file = dir.join("mine.theme");
    let user = of_theme(&nord, Source::User, Some(&file), true, Some(dir), "plain");
    assert_eq!((user.origin, user.path.as_deref(), user.source.as_str()), (modules::Origin::User, Some(file.as_path()), file.to_str().unwrap()));
    assert_eq!(user.state.health, Health::Running);
    let c = super::card::card(&user, Path::new("/home/u"));
    assert_eq!((c.at.as_deref(), c.source.as_str()), (None, "~/.config/agent-ways/themes/mine.theme"), "a source naming the file it was added in says it once");
    // The theme tab says it is a theme, and the state rides the origin line.
    assert_eq!((c.kind.as_deref(), c.fold_state), (None, true));

    let over = of_theme(&nord, Source::Override, Some(&dir.join("nord.theme")), false, None, "plain");
    assert_eq!(over.rows[0], Row::new("overrides", "the bundled nord"));
    assert_eq!(over.rows[1], Row::new("saves to", "none: nothing saves"));
}

#[test]
fn the_mod_tab_puts_the_card_on_each_mod_node() {
    let h = home("modtab");
    let mut c = ctx(&h, || Attend::Absent);
    c.mods = || vec![overview(ATTEND, State::Converged, &["0.29.0"], None)];
    let tab = Ways::new(c).build(&[]).into_iter().find(|t| t.name == "mod").unwrap();
    let names: Vec<(&str, &str)> = tab.children.iter().map(|n| (n.name.as_str(), n.card.as_ref().map_or("", |c| c.state.0.as_str()))).collect();
    assert_eq!(names, [("attend", "installed · enabled"), ("ways-judge", "state not read"), ("ways-theme", "state not read")]);
    let _ = std::fs::remove_dir_all(&h);
}

/// This test binary standing in for an attend that answers as `how` says:
/// `fail` exits 2, as an attend older than `--json` does; `text` exits 0
/// having printed no JSON; `slow` answers too late.
fn stand_in(how: &str) -> std::process::Command {
    let name = format!("{}::stand_in_attend", module_path!().split_once("::").map_or(module_path!(), |(_, rest)| rest));
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args([name.as_str(), "--exact", "--ignored", "--test-threads=1"]).env("WAYS_ATTEND_STAND_IN", how);
    cmd
}

#[test]
#[ignore = "the attend that described_by's tests run"]
fn stand_in_attend() {
    match std::env::var("WAYS_ATTEND_STAND_IN").as_deref() {
        Ok("fail") => std::process::exit(2),
        Ok("slow") => std::thread::sleep(std::time::Duration::from_secs(5)),
        _ => {}
    }
}

#[test]
fn an_attend_that_does_not_list_its_sensors_is_told_from_none() {
    let none = Path::new("nonexistent-dir").join("attend-for-a-test");
    assert_eq!(sensors::described_by(std::process::Command::new(none)), Attend::Absent);
    assert_eq!(sensors::described_by(stand_in("fail")), Attend::Unlisted, "a non-zero exit");
    assert_eq!(sensors::described_by(stand_in("text")), Attend::Unlisted, "output that is no JSON");

    let h = home("unlisted");
    let tab = sensors_tab(&h, || Attend::Unlisted, &shipped());
    let git = card(&tab, "git");
    assert_eq!((git.source.as_str(), git.version.as_deref()), ("unknown: attend did not list its sensors", None));
    assert_eq!(git.state, ("enabled".to_string(), StateTone::Ok), "the file's own switch");
    let absent = sensors_tab(&h, || Attend::Absent, &shipped());
    assert_eq!(card(&absent, "git").source, "unknown: attend absent");
    let _ = std::fs::remove_dir_all(&h);
}

/// A sensor a project's file defines is edited in that file, its switch
/// aside; any other sensor, in the user's.
#[test]
fn a_project_sensor_s_rows_write_to_its_file_but_its_switch() {
    let h = home("writes");
    let mut layers = with_user(&h, "mine", "kind: command\nargv: [/opt/mine]\noutput: lines\n");
    layers.push(project_file(&h, "disk", "kind: command\nargv: [df]\noutput: lines\n"));
    layers.push(project_file(&h, "git", "interval: 90\n"));
    let tab = sensors_tab(&h, described, &layers);
    let store = |sensor: &str, key: &str| {
        let node = tab.children.iter().find(|n| n.name == sensor).unwrap();
        node.children.iter().find(|n| n.name == key).and_then(|n| n.setting.as_ref()).and_then(|s| s.store.clone()).unwrap_or_else(|| panic!("{sensor}.{key} has no store"))
    };
    let disk = store("disk", "interval");
    assert_eq!((disk.layer.as_str(), disk.file.as_path()), ("project", h.join("proj/.claude/attend/sensors/disk.yaml").as_path()));
    assert_eq!(store("disk", "threshold").layer, "project");
    // Switching a project's sensor off is each operator's own.
    assert_eq!(store("disk", "enabled").layer, "user");
    assert_eq!(store("mine", "interval").layer, "user");
    // A project's override of a shipped sensor moves none of its keys into
    // the shared file.
    for key in ["enabled", "interval", "lane"] {
        assert_eq!(store("git", key).layer, "user", "git.{key}");
    }
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn an_attend_that_does_not_answer_in_time_is_unlisted_and_reaped() {
    let start = std::time::Instant::now();
    assert_eq!(sensors::described_by(stand_in("slow")), Attend::Unlisted);
    let took = start.elapsed();
    assert!(took >= std::time::Duration::from_secs(2) && took < std::time::Duration::from_secs(4), "{took:?}");
}
