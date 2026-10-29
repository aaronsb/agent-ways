//! The sensors tab's level above the settings: a node per sensor, each
//! carrying its card. A built-in sensor is a crate of attend's, described
//! by attend itself (`attend sensor list --json`, read once per screen);
//! a sensor of the user's is one whose `script` a settings layer sets, read
//! from those layers ([`user_sensors`]), and its rows are the generic
//! sensor keys, written like a built-in's.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent_settings::load::{resolve, Layer};
use agent_tui::tree::{Card, Node, Origin, StateTone};
use attend_config::schema::BUILTINS;

use super::build::tilde;

/// One sensor as `attend sensor list --json` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described {
    pub name: String,
    /// `builtin` or `script`.
    pub kind: String,
    /// `active`, `off`, `not compiled` or `missing`.
    pub state: String,
    pub description: String,
    /// A built-in's `crate@version`, a script sensor's path.
    pub source: String,
}

/// How long the screens wait on attend's answer.
const ATTEND_WAIT: Duration = Duration::from_secs(2);

/// attend's account of its sensors, from the `attend` on PATH; `None` when
/// there is none, or it does not answer within [`ATTEND_WAIT`] or in JSON.
pub fn described() -> Option<Vec<Described>> {
    let mut child = std::process::Command::new("attend")
        .args(["sensor", "list", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < ATTEND_WAIT => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    parse(&out)
}

/// The JSON `attend sensor list --json` prints, as [`Described`] rows.
pub fn parse(json: &str) -> Option<Vec<Described>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let text = |e: &serde_json::Value, k: &str| e.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    Some(
        v.as_array()?
            .iter()
            .map(|e| Described { name: text(e, "name"), kind: text(e, "kind"), state: text(e, "state"), description: text(e, "description"), source: text(e, "source") })
            .collect(),
    )
}

/// A sensor the user added: one whose `script` a layer sets.
#[derive(Debug, Clone, PartialEq)]
pub struct UserSensor {
    pub name: String,
    /// The script, with `~`, `$HOME` and `$XDG_*` expanded and a relative
    /// path taken under the project, as attend runs it.
    pub script: PathBuf,
    /// The file that sets the script.
    pub file: Option<PathBuf>,
}

/// The sensors the user added, by name, as `layers` set them.
pub fn user_sensors(layers: &[Layer], project: &Path) -> Vec<UserSensor> {
    let cfg = attend_config::Config::from_layers(layers);
    let spec = attend_config::schema::SCHEMA.keys.iter().find(|k| k.name == "attend.sensors.*.script");
    cfg.sensors
        .iter()
        .filter(|(name, _)| !BUILTINS.contains(&name.as_str()))
        .filter_map(|(name, sc)| {
            let script = Path::new(sc.script.as_deref()?);
            let file = spec.and_then(|s| resolve(s, std::slice::from_ref(name), layers).layer.and_then(|i| layers.get(i)?.path.clone()));
            Some(UserSensor { name: name.clone(), script: if script.is_absolute() { script.to_path_buf() } else { project.join(script) }, file })
        })
        .collect()
}

/// The sensors tab's keys: `keys` with every generic sensor key bound for
/// each sensor the user added, so its node holds the rows a built-in's
/// does, in the tab's order: the built-ins, then the user's, each sensor's
/// keys in the schema's order.
pub fn with_user_keys(reg: &agent_settings::Registry, mut keys: Vec<agent_settings::Bound>, layers: &[Layer], project: &Path) -> Vec<agent_settings::Bound> {
    let schema = &attend_config::schema::SCHEMA;
    let users = user_sensors(layers, project);
    for u in &users {
        for spec in schema.keys.iter().filter(|k| k.section == "attend.sensors" && k.instances == BUILTINS) {
            let name = spec.name.replacen('*', &u.name, 1);
            if !keys.iter().any(|b| b.name() == name) {
                keys.extend(reg.lookup(&name));
            }
        }
    }
    let group = |b: &agent_settings::Bound| {
        let n = b.bound.first().map(String::as_str).unwrap_or_default();
        BUILTINS.iter().position(|x| *x == n).unwrap_or(BUILTINS.len() + users.iter().position(|u| u.name == n).unwrap_or(users.len()))
    };
    let field = |b: &agent_settings::Bound| schema.keys.iter().position(|k| k.name == b.spec.name).unwrap_or(usize::MAX);
    keys.sort_by_key(|b| (group(b), field(b)));
    keys
}

/// Put a card on each sensor's node under `root`.
pub fn cards(root: &mut Node, layers: &[Layer], project: &Path, home: &Path, attend: Option<&[Described]>) {
    let cfg = attend_config::Config::from_layers(layers);
    let users = user_sensors(layers, project);
    for g in root.children.iter_mut().filter(|g| !g.section) {
        let Some(sc) = cfg.sensors.get(&g.name) else { continue };
        let timing = vec![
            ("interval".to_string(), format!("{}s quiet, {}s busy", sc.interval.as_secs(), sc.min_interval.as_secs())),
            ("threshold".to_string(), sc.threshold.to_string()),
        ];
        let enabled = if sc.enabled { ("enabled".to_string(), StateTone::Ok) } else { ("disabled".to_string(), StateTone::Plain) };
        if let Some(u) = users.iter().find(|u| u.name == g.name) {
            let state = if u.script.is_file() {
                enabled
            } else {
                (format!("{} · script not found", enabled.0), if sc.enabled { StateTone::Err } else { StateTone::Warn })
            };
            g.card = Some(Card {
                title: g.name.clone(),
                summary: "a sensor of your own".into(),
                origin: Origin::User,
                at: u.file.as_deref().map(|f| tilde(f, home)),
                kind: Some("script".into()),
                source: tilde(&u.script, home),
                version: None,
                state,
                rows: timing,
            });
        } else if BUILTINS.contains(&g.name.as_str()) {
            g.card = Some(builtin_card(&g.name, enabled, timing, attend));
        }
    }
}

/// A built-in sensor's card: its crate and version as attend reports them.
fn builtin_card(name: &str, enabled: (String, StateTone), rows: Vec<(String, String)>, attend: Option<&[Described]>) -> Card {
    let d = attend.and_then(|a| a.iter().find(|d| d.name == name));
    let (summary, source, version, state) = match (attend, d) {
        (None, _) => ("built-in sensor".to_string(), "unknown: attend absent".to_string(), None, enabled),
        (Some(_), None) => ("built-in sensor".to_string(), "unknown: attend does not list it".to_string(), None, enabled),
        (Some(_), Some(d)) if d.state == "not compiled" => (d.description.clone(), "not compiled into this attend".to_string(), None, ("not compiled".to_string(), StateTone::Warn)),
        (Some(_), Some(d)) => {
            let (krate, version) = match d.source.split_once('@') {
                Some((c, v)) => (c.to_string(), Some(v.to_string())),
                None => (d.source.clone(), None),
            };
            (d.description.clone(), krate, version, enabled)
        }
    };
    Card { title: name.to_string(), summary, origin: Origin::Builtin, at: None, kind: Some("crate".into()), source, version, state, rows }
}
