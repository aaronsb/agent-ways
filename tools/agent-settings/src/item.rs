//! The item descriptor (ADR-516 §1): one shape for a theme, a mod, a sensor,
//! a way and a judge profile, which the store, the settings screens and the
//! CLI describe the same way. Each family's state words and their tones are
//! its §2 list.

use std::path::PathBuf;

use serde::Serialize;

/// One item. Serialized, every field is present, `null` where it does not
/// apply.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub family: Family,
    /// Unique within the family; a way's is its id.
    pub name: String,
    /// What implements it; `None` where the family has one implementation.
    pub kind: Option<Kind>,
    /// The lowest layer that defines an item of this name.
    pub origin: Origin,
    /// The origin layer's definition; `None` when it exists only inside a
    /// binary.
    pub path: Option<PathBuf>,
    /// Each higher layer that changes it, lowest first.
    pub overrides: Vec<Override>,
    /// What runs or supplies it, when that is something other than its file.
    pub source: Option<String>,
    /// The source's version, as the source reports it.
    pub version: Option<String>,
    /// A word of the family's list; `None` while it is not known.
    pub state: Option<State>,
    /// One line that qualifies the state.
    pub detail: Option<String>,
}

impl Item {
    /// An item of `family` named `name`, defined by `origin`, with nothing
    /// else known.
    pub fn new(family: Family, name: impl Into<String>, origin: Origin) -> Item {
        Item { family, name: name.into(), kind: None, origin, path: None, overrides: Vec::new(), source: None, version: None, state: None, detail: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Theme,
    Mod,
    Sensor,
    Way,
    Profile,
}

/// Where an item's definition is found: shipped with agent-ways, in the
/// user's config root, or under a project's `.claude/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Builtin,
    User,
    Project,
}

impl Origin {
    pub fn word(self) -> &'static str {
        match self {
            Origin::Builtin => "builtin",
            Origin::User => "user",
            Origin::Project => "project",
        }
    }
}

/// A higher layer's change to an item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Override {
    pub origin: Origin,
    pub how: How,
    /// The file that makes it; `None` for a layer read from no file.
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum How {
    /// It sets fields over the definition beneath.
    Extends,
    /// A whole definition of the same name takes its place.
    Replaces,
}

impl How {
    pub fn word(self) -> &'static str {
        match self {
            How::Extends => "extends",
            How::Replaces => "replaces",
        }
    }
}

/// What implements an item: a mod's plugin, a sensor's kind (ADR-403), a
/// profile's provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Plugin,
    Crate,
    Command,
    Mcp,
    Ways,
    Anthropic,
    Openrouter,
    ClaudeCode,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Plugin => "plugin",
            Kind::Crate => "crate",
            Kind::Command => "command",
            Kind::Mcp => "mcp",
            Kind::Ways => "ways",
            Kind::Anthropic => "anthropic",
            Kind::Openrouter => "openrouter",
            Kind::ClaudeCode => "claude-code",
        }
    }
}

/// How a state reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Ok,
    Plain,
    Warn,
    Err,
}

/// An item's state: a word of its family's list. It serializes as the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum State {
    Theme(ThemeState),
    Mod(ModState),
    Sensor(SensorState),
    Way(WayState),
    Profile(ProfileState),
}

impl State {
    pub fn family(self) -> Family {
        match self {
            State::Theme(_) => Family::Theme,
            State::Mod(_) => Family::Mod,
            State::Sensor(_) => Family::Sensor,
            State::Way(_) => Family::Way,
            State::Profile(_) => Family::Profile,
        }
    }

    pub fn word(self) -> &'static str {
        self.entry().0
    }

    pub fn tone(self) -> Tone {
        self.entry().1
    }

    fn entry(self) -> (&'static str, Tone) {
        match self {
            State::Theme(s) => s.entry(),
            State::Mod(s) => s.entry(),
            State::Sensor(s) => s.entry(),
            State::Way(s) => s.entry(),
            State::Profile(s) => s.entry(),
        }
    }
}

/// One family's state list: each state's word and tone, in one table.
macro_rules! states {
    ($(#[$doc:meta])* $name:ident, $variant:ident { $($state:ident => $word:literal, $tone:ident;)* }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        pub enum $name {
            $(#[serde(rename = $word)] $state,)*
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$state),*];

            fn entry(self) -> (&'static str, Tone) {
                match self {
                    $($name::$state => ($word, Tone::$tone),)*
                }
            }
        }

        impl From<$name> for State {
            fn from(s: $name) -> State {
                State::$variant(s)
            }
        }
    };
}

states!(ThemeState, Theme {
    Active => "active", Ok;
    Inactive => "inactive", Plain;
    Invalid => "invalid", Err;
});

states!(
    /// ADR-512's words.
    ModState, Mod {
    Converged => "converged", Ok;
    Drift => "drift", Warn;
    Pending => "pending", Warn;
    Disabled => "disabled", Warn;
    Removed => "removed", Plain;
    Off => "off", Plain;
});

states!(SensorState, Sensor {
    Active => "active", Ok;
    Off => "off", Plain;
    NotCompiled => "not-compiled", Warn;
    Missing => "missing", Warn;
    Refused => "refused", Warn;
    Failing => "failing", Err;
    Invalid => "invalid", Err;
});

states!(WayState, Way {
    Enabled => "enabled", Ok;
    Disabled => "disabled", Plain;
    Invalid => "invalid", Err;
});

states!(ProfileState, Profile {
    Active => "active", Ok;
    Inactive => "inactive", Plain;
    Invalid => "invalid", Err;
});

#[cfg(test)]
mod tests {
    use super::*;

    fn json(i: &Item) -> String {
        serde_json::to_string(i).unwrap()
    }

    #[test]
    fn an_item_serializes_every_field_in_order_with_nulls() {
        let theme = Item::new(Family::Theme, "nord", Origin::Builtin);
        assert_eq!(
            json(&theme),
            r#"{"family":"theme","name":"nord","kind":null,"origin":"builtin","path":null,"overrides":[],"source":null,"version":null,"state":null,"detail":null}"#
        );
    }

    #[test]
    fn a_full_item_serializes_its_overrides_under_the_descriptor_names() {
        let git = Item {
            kind: Some(Kind::Crate),
            path: Some("/d/agent-ways/attend/sensors/git.yaml".into()),
            overrides: vec![
                Override { origin: Origin::User, how: How::Extends, path: Some("/c/attend/sensors/git.yaml".into()) },
                Override { origin: Origin::Project, how: How::Replaces, path: None },
            ],
            source: Some("attend".into()),
            version: Some("0.33.0".into()),
            state: Some(SensorState::NotCompiled.into()),
            detail: Some("this build leaves it out".into()),
            ..Item::new(Family::Sensor, "git", Origin::Builtin)
        };
        assert_eq!(
            json(&git),
            concat!(
                r#"{"family":"sensor","name":"git","kind":"crate","origin":"builtin","path":"/d/agent-ways/attend/sensors/git.yaml","#,
                r#""overrides":[{"origin":"user","how":"extends","path":"/c/attend/sensors/git.yaml"},{"origin":"project","how":"replaces","path":null}],"#,
                r#""source":"attend","version":"0.33.0","state":"not-compiled","detail":"this build leaves it out"}"#
            )
        );
    }

    #[test]
    fn each_state_serializes_as_its_word() {
        let all: Vec<State> = ThemeState::ALL
            .iter()
            .map(|&s| State::from(s))
            .chain(ModState::ALL.iter().map(|&s| s.into()))
            .chain(SensorState::ALL.iter().map(|&s| s.into()))
            .chain(WayState::ALL.iter().map(|&s| s.into()))
            .chain(ProfileState::ALL.iter().map(|&s| s.into()))
            .collect();
        for s in &all {
            assert_eq!(serde_json::to_value(s).unwrap(), serde_json::Value::from(s.word()), "{s:?}");
        }
        let words = |f: Family| all.iter().filter(|s| s.family() == f).map(|s| s.word()).collect::<Vec<_>>();
        assert_eq!(words(Family::Theme), ["active", "inactive", "invalid"]);
        assert_eq!(words(Family::Mod), ["converged", "drift", "pending", "disabled", "removed", "off"]);
        assert_eq!(words(Family::Sensor), ["active", "off", "not-compiled", "missing", "refused", "failing", "invalid"]);
        assert_eq!(words(Family::Way), ["enabled", "disabled", "invalid"]);
        assert_eq!(words(Family::Profile), ["active", "inactive", "invalid"]);
    }

    #[test]
    fn the_tone_tables_read_running_ok_and_broken_err() {
        assert_eq!(State::from(SensorState::Active).tone(), Tone::Ok);
        assert_eq!(State::from(SensorState::Failing).tone(), Tone::Err);
        assert_eq!(State::from(ModState::Drift).tone(), Tone::Warn);
        assert_eq!(State::from(ThemeState::Inactive).tone(), Tone::Plain);
    }

    #[test]
    fn the_other_words_serialize_as_written() {
        fn v<T: Serialize>(x: &T) -> String {
            serde_json::to_value(x).unwrap().as_str().unwrap().to_string()
        }
        assert_eq!(v(&Family::Profile), "profile");
        assert_eq!(v(&How::Replaces), "replaces");
        assert_eq!(v(&Tone::Warn), "warn");
        for k in [Kind::Plugin, Kind::Crate, Kind::Command, Kind::Mcp, Kind::Ways, Kind::Anthropic, Kind::Openrouter, Kind::ClaudeCode] {
            assert_eq!(v(&k), k.word());
        }
        assert_eq!(v(&Kind::ClaudeCode), "claude-code");
        for o in [Origin::Builtin, Origin::User, Origin::Project] {
            assert_eq!(v(&o), o.word());
        }
    }
}
