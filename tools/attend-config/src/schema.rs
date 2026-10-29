//! attend's sections and keys (ADR-503 §1, §13). The user file
//! (`~/.config/attend/config.yaml`) and a project's `.claude/attend.yaml`
//! take the same keys; the project's are layered on top.
//!
//! Sensors are files of their own (ADR-403), loaded by [`crate::sensors`];
//! the `sensors:` section these files once held is retired, and lint names
//! the file and field each of its keys moves to.
//!
//! Every name starts with `attend.`, so the keys sit apart from ways' in the
//! one registry `ways settings` composes. attend keeps no theme key: the one
//! user-level theme is ways' `theme.active`, which attend's tabs in the
//! settings screens read through that registry like every other tab.

use agent_settings::schema::{DefaultValue, FileSpec, Kind, KeySpec, LayerScope, Schema, Scope, SectionSpec};
use serde_yaml::Value;

/// The file kind: the user and project files both take these keys.
pub const FILE: &str = "attend";

/// The sensor files' kind (ADR-403 §5). Each sensor file is a layer of its
/// own, read by [`crate::sensors::layers`] as `sensors: {<name>: <file>}`,
/// so a key's path names the sensor and the field it sets.
pub const SENSOR_FILE: &str = "attend-sensor";

const SECTIONS: &[SectionSpec] = &[
    SectionSpec {
        name: "attend.governor",
        file: FILE,
        top: &["governor"],
        per_entry: false,
        entry: None,
        repair: None,
        columns: None,
        doc: "Rate limits on disclosures across every sensor.",
    },
    SectionSpec {
        name: "attend.engagement",
        file: FILE,
        top: &["engagement"],
        per_entry: false,
        entry: None,
        repair: None,
        columns: None,
        doc: "The action potential model: how a sensor quiets after a burst (ADR-123). `attend config tune --apply` writes it.",
    },
    SectionSpec {
        name: "attend.cleanup",
        file: FILE,
        top: &["cleanup"],
        per_entry: false,
        entry: None,
        repair: None,
        columns: None,
        doc: "The background sweep of signals whose project is gone (ADR-136).",
    },
    SectionSpec {
        name: "attend.chat",
        file: FILE,
        top: &["chat"],
        per_entry: false,
        entry: None,
        repair: None,
        columns: None,
        doc: "attend-chat, the operator's screen: how its tabs are reached and whether it starts with the mouse.",
    },
    SectionSpec {
        name: "attend.project_sensors",
        file: FILE,
        top: &["project_sensors"],
        per_entry: false,
        entry: None,
        repair: None,
        columns: None,
        doc: "The projects whose own command and mcp sensor files attend runs (ADR-403 §2).",
    },
    SectionSpec {
        name: "attend.sensors",
        file: SENSOR_FILE,
        top: &["sensors"],
        per_entry: true,
        entry: None,
        repair: None,
        columns: None,
        doc: "Each sensor's file: shipped, the user's, and a project's, layered by name (ADR-403 §2).",
    },
];

const RETIRED: &[(&str, &str)] = &[
    ("signals", "`signals:` was retired with liveness reaping (#141) and is ignored; delete it"),
    ("sensors", "`sensors:` was retired by ADR-403 and is ignored; each sensor is a file under attend/sensors/"),
];

/// Where the sensor file for `name` goes, for a file at `scope`.
fn sensor_file(name: &str, scope: LayerScope) -> String {
    match scope {
        LayerScope::Project => format!(".claude/attend/sensors/{name}.yaml"),
        _ => format!("$XDG_CONFIG_HOME/attend/sensors/{name}.yaml"),
    }
}

/// One finding for each key under the retired `sensors:`, naming the
/// sensor file to write and the field to set in it (ADR-403 §7).
fn explain_sensors(top: &str, v: &Value, scope: LayerScope) -> Vec<(Vec<String>, String)> {
    if top != "sensors" {
        return Vec::new();
    }
    let Some(entries) = v.as_mapping() else { return Vec::new() };
    let mut out = Vec::new();
    let show = |v: &Value| serde_yaml::to_string(v).unwrap_or_default().trim().replace('\n', " ");
    for (k, entry) in entries {
        let Some(raw) = k.as_str() else { continue };
        let path = |field: Option<&str>| {
            let mut p = vec![top.to_string(), raw.to_string()];
            p.extend(field.map(str::to_string));
            p
        };
        // The old `-name` and `+name` forms.
        let name = raw.trim_start_matches(['+', '-']);
        let file = sensor_file(name, scope);
        if raw.starts_with('-') {
            out.push((path(None), format!("retired by ADR-403; write `enabled: false` in {file}")));
            continue;
        }
        let Some(fields) = entry.as_mapping().filter(|m| !m.is_empty()) else {
            out.push((path(None), format!("retired by ADR-403; the sensor's file is {file}")));
            continue;
        };
        let script = fields.get("script").is_some();
        for (f, fv) in fields {
            let Some(f) = f.as_str() else { continue };
            let message = match f {
                "script" => format!(
                    "the inline script form was retired by ADR-403; write {file} with `kind: command`, `argv: [bash, {}]` and `output: lines`",
                    show(fv)
                ),
                f if script => format!("retired by ADR-403; write `{f}: {}` in {file}, the command sensor that replaces the script", show(fv)),
                f => format!("retired by ADR-403; write `{f}: {}` in {file}", show(fv)),
            };
            out.push((path(Some(f)), message));
        }
    }
    out
}

const BASE: KeySpec = KeySpec {
    name: "",
    section: "attend.governor",
    file: FILE,
    path: &[],
    kind: SECONDS,
    default: DefaultValue::None,
    instances: &[],
    scope: Scope::Both,
    doc: "",
    long: "",
    check: None,
    computed: None,
    fail_closed: None,
};

/// A whole number of seconds, at least one.
const SECONDS: Kind = Kind::Int { min: 1, max: 7 * 86_400 };
/// A count, at least one.
const COUNT: Kind = Kind::Int { min: 1, max: u32::MAX as i64 };

const KEYS: &[KeySpec] = &[
    KeySpec {
        name: "attend.governor.base_cooldown",
        path: &["governor", "base_cooldown"],
        kind: Kind::Int { min: 0, max: 86_400 },
        default: DefaultValue::Yaml("15"),
        doc: "Seconds between any two disclosures.",
        long: "A burst of sensors ready at the same time has its disclosures spaced at least this far apart.",
        ..BASE
    },
    KeySpec {
        name: "attend.governor.max_per_window",
        path: &["governor", "max_per_window"],
        kind: Kind::Int { min: 0, max: u32::MAX as i64 },
        default: DefaultValue::Yaml("3"),
        doc: "Disclosures allowed within one rate window; 0 mutes attend.",
        long: "Sensors ready past the cap are held; their magnitudes stay in the accumulator. 0 holds every disclosure, which silences attend without stopping it.",
        ..BASE
    },
    KeySpec {
        name: "attend.governor.rate_window",
        path: &["governor", "rate_window"],
        default: DefaultValue::Yaml("120"),
        doc: "Seconds of the rolling window max_per_window counts in.",
        ..BASE
    },
    KeySpec {
        name: "attend.engagement.burst_threshold",
        section: "attend.engagement",
        path: &["engagement", "burst_threshold"],
        kind: COUNT,
        default: DefaultValue::Yaml("3"),
        doc: "Recent disclosures that put a sensor into refractory.",
        long: "Past it the sensor's threshold is raised and decays back over time (ADR-123).",
        ..BASE
    },
    KeySpec {
        name: "attend.engagement.step_multiplier",
        section: "attend.engagement",
        path: &["engagement", "step_multiplier"],
        kind: Kind::Float { min: 0.0, max: 100.0 },
        default: DefaultValue::Yaml("1.25"),
        doc: "How far a burst raises the threshold: the peak is 1 + this.",
        long: "At 1.25 the threshold just after a burst is 2.25 times the base.",
        ..BASE
    },
    KeySpec {
        name: "attend.engagement.absolute_refractory",
        section: "attend.engagement",
        path: &["engagement", "absolute_refractory"],
        kind: Kind::Int { min: 0, max: 86_400 },
        default: DefaultValue::Yaml("60"),
        doc: "Seconds of complete silence after a burst.",
        long: "No event fires in this window, whatever its magnitude. `attend config tune` sets it to the median think time.",
        ..BASE
    },
    KeySpec {
        name: "attend.engagement.decay_per_minute",
        section: "attend.engagement",
        path: &["engagement", "decay_per_minute"],
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: DefaultValue::Yaml("0.1"),
        doc: "How fast a raised threshold returns to its base, per minute.",
        long: "Converted at load to a half-life of ln(0.5)/ln(1-rate) x 60 seconds: about 395 s at 0.1.",
        ..BASE
    },
    KeySpec {
        name: "attend.engagement.peer_activity_window",
        section: "attend.engagement",
        path: &["engagement", "peer_activity_window"],
        default: DefaultValue::Yaml("900"),
        doc: "Seconds of the window the peers sensor counts each peer's activity in.",
        ..BASE
    },
    KeySpec {
        name: "attend.cleanup.enabled",
        section: "attend.cleanup",
        path: &["cleanup", "enabled"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        // A switch fails closed (ADR-503 addendum): anything but true is off,
        // so a bad value never starts a sweep that deletes files.
        fail_closed: Some(closed_off),
        doc: "Whether `attend run` sweeps away the signals of projects that are gone.",
        long: "Off, signals stay until `attend message cleanup` is run by hand.",
        ..BASE
    },
    KeySpec {
        name: "attend.cleanup.interval",
        section: "attend.cleanup",
        path: &["cleanup", "interval"],
        default: DefaultValue::Yaml("600"),
        doc: "Seconds between sweeps.",
        ..BASE
    },
    KeySpec {
        name: "attend.chat.tabs.jump",
        section: "attend.chat",
        path: &["chat", "tabs", "jump"],
        kind: Kind::Choice(&["auto", "ctrl", "alt", "both", "none"]),
        default: DefaultValue::Yaml("auto"),
        scope: Scope::User,
        doc: "Which modifier with a digit jumps to a tab: auto, ctrl, alt, both or none.",
        long: "auto is Ctrl+digit where the terminal speaks the kitty keyboard protocol and reports Ctrl+digits, else Alt+digit. Konsole and GNOME Terminal keep Alt+digits for their own tabs; F2 reaches the tabs everywhere.",
        ..BASE
    },
    KeySpec {
        name: "attend.chat.tabs.menu_on_repeat",
        section: "attend.chat",
        path: &["chat", "tabs", "menu_on_repeat"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        scope: Scope::User,
        doc: "Whether jumping to, or clicking, the tab already shown opens its menu.",
        long: "A right click on a tab, and Enter on the focused tab bar, open the menu either way.",
        ..BASE
    },
    KeySpec {
        name: "attend.chat.tabs.focus_key",
        section: "attend.chat",
        path: &["chat", "tabs", "focus_key"],
        kind: Kind::Choice(&["both", "f2", "ctrl-t", "none"]),
        default: DefaultValue::Yaml("both"),
        scope: Scope::User,
        doc: "The key that gives the tab bar the focus: both (F2 and Ctrl+T), f2, ctrl-t or none.",
        long: "On the focused tab bar Left and Right move, Enter opens the tab's menu and Esc goes back to the compose box.",
        ..BASE
    },
    KeySpec {
        name: "attend.chat.sidebar_key",
        section: "attend.chat",
        path: &["chat", "sidebar_key"],
        kind: Kind::Choice(&["f3", "ctrl-b", "none"]),
        default: DefaultValue::Yaml("f3"),
        scope: Scope::User,
        doc: "The key that shows or hides the left column of agents and channels: f3, ctrl-b or none.",
        long: "The column is shown at start, and hidden on a terminal too narrow to hold it beside the feed (ADR-405). Ctrl+B is tmux's prefix, so f3 is the default; none keeps the column shown, and the common menu still hides it.",
        ..BASE
    },
    KeySpec {
        name: "attend.chat.mouse",
        section: "attend.chat",
        path: &["chat", "mouse"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        scope: Scope::User,
        doc: "Whether attend-chat starts with the mouse on.",
        long: "On, a click reaches tabs, the left column, messages and chips, and Shift-drag selects text in most terminals. Off, the terminal selects text and middle-click pastes, and no click reaches the chat. Alt+m switches it while the chat runs.",
        ..BASE
    },
    KeySpec {
        name: "attend.project_sensors",
        section: "attend.project_sensors",
        path: &["project_sensors"],
        kind: Kind::List,
        default: DefaultValue::Yaml("[]"),
        scope: Scope::User,
        doc: "Projects whose .claude/attend/sensors/ command and mcp files run.",
        long: "Such a file runs code the repository's author chose, so it is refused until its project is listed here. Each entry is a project directory; ~, $HOME and $XDG_* are expanded. A project's file may override a crate sensor's fields without it. Only the user file holds this key.",
        ..BASE
    },
    KeySpec {
        name: "attend.sensors.*.enabled",
        path: &["sensors", "*", "enabled"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        doc: "Whether the sensor runs.",
        long: "false in a project's sensor file switches the sensor off in that project. The settings screens write a project sensor's switch to your own file, so it stays yours.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.interval",
        path: &["sensors", "*", "interval"],
        default: DefaultValue::Yaml("60"),
        doc: "Seconds between polls while quiet.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.min_interval",
        path: &["sensors", "*", "min_interval"],
        default: DefaultValue::Yaml("15"),
        doc: "Seconds between polls while things change; at most the interval.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.threshold",
        path: &["sensors", "*", "threshold"],
        kind: Kind::Float { min: 0.0, max: 100.0 },
        default: DefaultValue::Yaml("2.0"),
        doc: "The accumulated magnitude a sensor must pass to disclose.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.decay_threshold",
        path: &["sensors", "*", "decay_threshold"],
        kind: COUNT,
        default: DefaultValue::Yaml("4"),
        doc: "Quiet polls before the interval decays back to its base.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.lane",
        path: &["sensors", "*", "lane"],
        kind: Kind::Choice(&["event", "message"]),
        default: DefaultValue::Yaml("event"),
        doc: "The lane the sensor's notifications ride (ADR-403 §3a).",
        long: "message is for authored communication and needs an author field naming who composed each item; without one the sensor runs on the event lane, and lint says why.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.requires",
        path: &["sensors", "*", "requires"],
        kind: Kind::List,
        default: DefaultValue::Yaml("[]"),
        doc: "The tool permissions the sensor needs (ADR-116).",
        long: "`attend sensor permissions` checks each against settings.json.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.watch",
        path: &["sensors", "*", "watch"],
        kind: Kind::List,
        doc: "The processes sensor's watch list; it replaces the built-in list.",
        long: "Only watched processes get exit-code enrichment. Unset, the sensor's built-in list applies. Only a crate sensor of the processes crate takes it.",
        ..SENSOR
    },
];

/// The sensor keys share these.
const SENSOR: KeySpec = KeySpec { section: "attend.sensors", file: SENSOR_FILE, ..BASE };

fn closed_off(v: &Value) -> Option<Value> {
    (v != &Value::Bool(true)).then_some(Value::Bool(false))
}

pub static SCHEMA: Schema = Schema {
    component: "attend",
    files: &[FileSpec { id: FILE, retired: RETIRED, explain: Some(explain_sensors) }, FileSpec { id: SENSOR_FILE, retired: &[], explain: None }],
    sections: SECTIONS,
    keys: KEYS,
};
