//! attend's sections and keys (ADR-503 §1, §13). The user file
//! (`~/.config/attend/config.yaml`) and a project's `.claude/attend.yaml`
//! take the same keys; the project's are layered on top.
//!
//! Every name starts with `attend.`, so the keys sit apart from ways' in the
//! one registry `ways settings` composes. attend keeps no theme key: the one
//! user-level theme is ways' `theme.active`, which attend's tabs in the
//! settings screens read through that registry like every other tab.

use agent_settings::schema::{DefaultValue, FileSpec, Kind, KeySpec, Schema, Scope, SectionSpec};
use serde_yaml::Value;

/// The file kind: the user and project files both take these keys.
pub const FILE: &str = "attend";

/// The built-in sensors. Each has its keys listed with no file setting them.
pub const BUILTINS: &[&str] = &["context", "git", "peers", "processes", "disclosure", "keepwarm"];

/// A sensor's defaults.
#[derive(Debug)]
pub struct SensorDefaults {
    pub name: &'static str,
    /// Seconds between polls while quiet.
    pub interval: u64,
    /// Seconds between polls while things change.
    pub min_interval: u64,
    pub threshold: f64,
    /// Quiet polls before the interval decays back.
    pub decay_threshold: u64,
    /// The permissions it needs (ADR-116).
    pub requires: &'static [&'static str],
}

const fn row(name: &'static str, interval: u64, min_interval: u64, threshold: f64, decay_threshold: u64, requires: &'static [&'static str]) -> SensorDefaults {
    SensorDefaults { name, interval, min_interval, threshold, decay_threshold, requires }
}

/// Each built-in's defaults. A sensor of your own, named with a `script`,
/// takes the last row.
const SENSOR_DEFAULTS: &[SensorDefaults] = &[
    row("context", 60, 20, 1.5, 3, &["Read"]),
    row("keepwarm", 60, 60, 3.0, 3, &["Bash(ways:*)"]),
    row("git", 30, 10, 2.0, 4, &["Bash(git:*)"]),
    row("peers", 30, 10, 2.0, 5, &["Read"]),
    row("processes", 30, 5, 2.0, 5, &["Bash(ps:*)"]),
    row("disclosure", 60, 20, 5.0, 3, &["Bash(ways:*)"]),
    row("", 60, 15, 2.0, 4, &[]),
];

/// The defaults of the sensor `name`: its built-in row, or a script sensor's.
pub fn sensor_defaults(name: &str) -> &'static SensorDefaults {
    SENSOR_DEFAULTS.iter().find(|r| r.name == name).unwrap_or(&SENSOR_DEFAULTS[SENSOR_DEFAULTS.len() - 1])
}

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
        doc: "The action potential model: how a sensor quiets after a burst (ADR-123). `attend tune --apply` writes it.",
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
        name: "attend.sensors",
        file: FILE,
        top: &["sensors"],
        per_entry: true,
        entry: Some(check_sensor_name),
        repair: None,
        columns: None,
        doc: "Each sensor's polling, threshold and permissions; a sensor of your own names a script. Each sensor falls back alone.",
    },
];

const RETIRED: &[(&str, &str)] = &[(
    "signals",
    "`signals:` was retired with liveness reaping (#141) and is ignored; delete it",
)];

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

/// The sensor keys share these.
const SENSOR: KeySpec = KeySpec { section: "attend.sensors", instances: BUILTINS, ..BASE };

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
        long: "No event fires in this window, whatever its magnitude. `attend tune` sets it to the median think time.",
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
        long: "Off, signals stay until `attend cleanup` is run by hand.",
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
        name: "attend.chat.mouse",
        section: "attend.chat",
        path: &["chat", "mouse"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("false"),
        scope: Scope::User,
        doc: "Whether attend-chat starts with the mouse on.",
        long: "Off, the terminal selects text and middle-click pastes; Alt+m turns the mouse on for clicks on tabs, messages and chips.",
        ..BASE
    },
    KeySpec {
        name: "attend.sensors.*.enabled",
        path: &["sensors", "*", "enabled"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        fail_closed: Some(closed_off),
        doc: "Whether the sensor runs.",
        long: "false in a project's .claude/attend.yaml switches a sensor off in that project. Anything but true in a sensor whose entry fails reads as off.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.interval",
        path: &["sensors", "*", "interval"],
        default: DefaultValue::Fn(|b| Some(Value::from(sensor_defaults(name(b)).interval))),
        doc: "Seconds between polls while quiet.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.min_interval",
        path: &["sensors", "*", "min_interval"],
        default: DefaultValue::Fn(|b| Some(Value::from(sensor_defaults(name(b)).min_interval))),
        doc: "Seconds between polls while things change.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.threshold",
        path: &["sensors", "*", "threshold"],
        kind: Kind::Float { min: 0.0, max: 100.0 },
        default: DefaultValue::Fn(|b| Some(Value::from(sensor_defaults(name(b)).threshold))),
        doc: "The accumulated magnitude a sensor must pass to disclose.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.decay_threshold",
        path: &["sensors", "*", "decay_threshold"],
        kind: COUNT,
        default: DefaultValue::Fn(|b| Some(Value::from(sensor_defaults(name(b)).decay_threshold))),
        doc: "Quiet polls before the interval decays back to its base.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.requires",
        path: &["sensors", "*", "requires"],
        kind: Kind::List,
        default: DefaultValue::Fn(|b| {
            Some(Value::Sequence(sensor_defaults(name(b)).requires.iter().map(|s| Value::String(s.to_string())).collect()))
        }),
        doc: "The tool permissions the sensor needs (ADR-116).",
        long: "`attend permissions audit` checks each against settings.json.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.script",
        path: &["sensors", "*", "script"],
        kind: Kind::Path,
        // A built-in runs no script, so none is listed for one.
        instances: &[],
        doc: "The script a sensor of your own runs.",
        long: "A sensor with a script is one of your own. A relative path is under the project; $HOME, ~ and $XDG_* are expanded. Scripts run as you: read one before you enable it.",
        ..SENSOR
    },
    KeySpec {
        name: "attend.sensors.*.watch",
        path: &["sensors", "*", "watch"],
        kind: Kind::List,
        instances: &["processes"],
        doc: "The processes sensor's watch list; it replaces the built-in list.",
        long: "Only watched processes get exit-code enrichment. Unset, the sensor's built-in list applies.",
        ..SENSOR
    },
];

/// One line on the sensor `name`, for the settings screens.
pub fn sensor_doc(name: &str) -> String {
    if BUILTINS.contains(&name) {
        format!("The built-in {name} sensor. `attend sensors` shows what it watches and whether it runs.")
    } else {
        format!("{name}, a sensor of your own: it runs its script. `attend sensors` shows whether the script resolves.")
    }
}

fn name(bound: &[String]) -> &str {
    bound.first().map(String::as_str).unwrap_or("")
}

fn closed_off(v: &Value) -> Option<Value> {
    (v != &Value::Bool(true)).then_some(Value::Bool(false))
}

/// A sensor's name: letters, digits, `-` and `_`, starting with a letter or
/// digit. The old `+name` and `-name` forms name nothing (ADR-506): a sensor
/// of your own is any name with a `script`, and `enabled: false` switches
/// one off.
fn check_sensor_name(n: &str) -> Result<(), String> {
    let ok = n.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        return Ok(());
    }
    match n.chars().next() {
        Some('+') => Err(format!("'{n}' is not a sensor name; a sensor of your own is `{}:` with a script", &n[1..])),
        Some('-') => Err(format!("'{n}' is not a sensor name; switch one off with `{}: {{enabled: false}}`", &n[1..])),
        _ => Err(format!("'{n}' is not a sensor name (letters, digits, - and _)")),
    }
}

pub static SCHEMA: Schema = Schema {
    component: "attend",
    files: &[FileSpec { id: FILE, retired: RETIRED }],
    sections: SECTIONS,
    keys: KEYS,
};
