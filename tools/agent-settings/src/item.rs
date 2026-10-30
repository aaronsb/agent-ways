//! The item descriptor (ADR-516 §1): one shape for a theme, a mod, a sensor,
//! a way and a judge profile, which the store, the settings screens and the
//! CLI describe the same way. Each family's state words and their tones are
//! its §2 list.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// One item. Serialized, every field is present, `null` where it does not
/// apply.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub family: Family,
    /// Unique within the family; a way's is its id.
    pub name: String,
    /// What implements it; `None` where the family has one implementation.
    pub kind: Option<ItemKind>,
    /// The lowest layer that defines an item of this name.
    pub origin: Origin,
    /// The origin layer's definition; `None` when it exists only inside a
    /// binary.
    pub path: Option<PathBuf>,
    /// Every other layer that changes it, in layer order.
    pub overrides: Vec<Override>,
    /// What runs or supplies it, when that is something other than its file.
    pub source: Option<String>,
    /// The source's version, as the source reports it.
    pub version: Option<String>,
    /// A word of the family's list: `unknown` until it is read.
    pub state: State,
    /// One line that qualifies the state.
    pub detail: Option<String>,
}

impl Item {
    /// An item of `family` named `name`, defined by `origin`, with nothing
    /// else known: its state is `unknown`.
    pub fn new(family: Family, name: impl Into<String>, origin: Origin) -> Item {
        Item { family, name: name.into(), kind: None, origin, path: None, overrides: Vec::new(), source: None, version: None, state: State::unknown(family), detail: None }
    }

    /// This item in `state`, which must be a word of its family's list.
    pub fn with_state(self, state: impl Into<State>) -> Item {
        let state = state.into();
        debug_assert_eq!(state.family(), self.family, "state {} is not a {:?} state", state.word(), self.family);
        Item { state, ..self }
    }

    /// This item changed by `overrides`, listed in layer order whatever
    /// order they come in; two of one layer keep theirs.
    pub fn with_overrides(self, mut overrides: Vec<Override>) -> Item {
        overrides.sort_by_key(|o| o.origin);
        Item { overrides, ..self }
    }

    /// The origin and each layer that changes it, as `builtin · user extends`.
    pub fn origin_text(&self) -> String {
        std::iter::once(self.origin.word().to_string()).chain(self.overrides.iter().map(|o| format!(" · {} {}", o.origin.word(), o.how.word()))).collect()
    }

    /// The highest file that changes the item, else the file that defines it.
    pub fn file(&self) -> Option<&Path> {
        self.overrides.last().map(|o| o.path.as_path()).or(self.path.as_deref())
    }
}

/// An item and the fields of its family, which serialize after the
/// descriptor's (ADR-516 §2, §5).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry<F> {
    #[serde(flatten)]
    pub item: Item,
    #[serde(flatten)]
    pub fields: F,
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
    /// The file that makes it.
    pub path: PathBuf,
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
pub enum ItemKind {
    Plugin,
    Crate,
    Command,
    Mcp,
    Ways,
    Anthropic,
    Openrouter,
    ClaudeCode,
}

impl ItemKind {
    pub fn word(self) -> &'static str {
        match self {
            ItemKind::Plugin => "plugin",
            ItemKind::Crate => "crate",
            ItemKind::Command => "command",
            ItemKind::Mcp => "mcp",
            ItemKind::Ways => "ways",
            ItemKind::Anthropic => "anthropic",
            ItemKind::Openrouter => "openrouter",
            ItemKind::ClaudeCode => "claude-code",
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

    /// The family's `unknown`: a state not yet read.
    pub fn unknown(f: Family) -> State {
        match f {
            Family::Theme => ThemeState::Unknown.into(),
            Family::Mod => ModState::Unknown.into(),
            Family::Sensor => SensorState::Unknown.into(),
            Family::Way => WayState::Unknown.into(),
            Family::Profile => ProfileState::Unknown.into(),
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
    Unknown => "unknown", Plain;
});

states!(
    /// ADR-512's words, and `unknown`.
    ModState, Mod {
    Converged => "converged", Ok;
    Drift => "drift", Warn;
    Pending => "pending", Warn;
    Disabled => "disabled", Warn;
    Removed => "removed", Plain;
    Held => "held", Warn;
    Off => "off", Plain;
    Unknown => "unknown", Plain;
});

states!(SensorState, Sensor {
    Active => "active", Ok;
    Off => "off", Plain;
    NotCompiled => "not-compiled", Warn;
    Missing => "missing", Warn;
    Refused => "refused", Warn;
    Failing => "failing", Err;
    Invalid => "invalid", Err;
    Degraded => "degraded", Warn;
    Unknown => "unknown", Plain;
});

states!(WayState, Way {
    Enabled => "enabled", Ok;
    Disabled => "disabled", Plain;
    Invalid => "invalid", Err;
    Unknown => "unknown", Plain;
});

states!(ProfileState, Profile {
    Active => "active", Ok;
    Inactive => "inactive", Plain;
    Invalid => "invalid", Err;
    Unknown => "unknown", Plain;
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
            r#"{"family":"theme","name":"nord","kind":null,"origin":"builtin","path":null,"overrides":[],"source":null,"version":null,"state":"unknown","detail":null}"#
        );
    }

    #[test]
    fn a_full_item_serializes_its_overrides_under_the_descriptor_names() {
        let git = Item {
            kind: Some(ItemKind::Crate),
            path: Some("/d/agent-ways/attend/sensors/git.yaml".into()),
            overrides: vec![
                Override { origin: Origin::User, how: How::Extends, path: "/c/attend/sensors/git.yaml".into() },
                Override { origin: Origin::Project, how: How::Replaces, path: "/p/.claude/attend/sensors/git.yaml".into() },
            ],
            source: Some("attend".into()),
            version: Some("0.33.0".into()),
            state: SensorState::NotCompiled.into(),
            detail: Some("this build leaves it out".into()),
            ..Item::new(Family::Sensor, "git", Origin::Builtin)
        };
        assert_eq!(
            json(&git),
            concat!(
                r#"{"family":"sensor","name":"git","kind":"crate","origin":"builtin","path":"/d/agent-ways/attend/sensors/git.yaml","#,
                r#""overrides":[{"origin":"user","how":"extends","path":"/c/attend/sensors/git.yaml"},{"origin":"project","how":"replaces","path":"/p/.claude/attend/sensors/git.yaml"}],"#,
                r#""source":"attend","version":"0.33.0","state":"not-compiled","detail":"this build leaves it out"}"#
            )
        );
    }

    #[test]
    #[should_panic(expected = "is not a Theme state")]
    fn a_state_of_another_family_is_refused() {
        let _ = Item::new(Family::Theme, "nord", Origin::Builtin).with_state(SensorState::Active);
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
        assert_eq!(words(Family::Theme), ["active", "inactive", "invalid", "unknown"]);
        assert_eq!(words(Family::Mod), ["converged", "drift", "pending", "disabled", "removed", "held", "off", "unknown"]);
        assert_eq!(words(Family::Sensor), ["active", "off", "not-compiled", "missing", "refused", "failing", "invalid", "degraded", "unknown"]);
        assert_eq!(words(Family::Way), ["enabled", "disabled", "invalid", "unknown"]);
        assert_eq!(words(Family::Profile), ["active", "inactive", "invalid", "unknown"]);
    }

    #[test]
    fn the_tone_tables_read_running_ok_and_broken_err() {
        assert_eq!(State::from(SensorState::Active).tone(), Tone::Ok);
        assert_eq!(State::from(SensorState::Failing).tone(), Tone::Err);
        assert_eq!(State::from(ModState::Drift).tone(), Tone::Warn);
        assert_eq!(State::from(ThemeState::Inactive).tone(), Tone::Plain);
        assert_eq!(State::from(SensorState::Degraded).tone(), Tone::Warn);
    }

    #[test]
    fn every_family_starts_unknown_in_the_plain_tone() {
        for f in [Family::Theme, Family::Mod, Family::Sensor, Family::Way, Family::Profile] {
            let s = Item::new(f, "x", Origin::Builtin).state;
            assert_eq!((s.family(), s.word(), s.tone()), (f, "unknown", Tone::Plain));
        }
    }

    #[test]
    fn overrides_list_in_layer_order() {
        let o = |origin: Origin| Override { origin, how: How::Extends, path: format!("/{}", origin.word()).into() };
        let item = Item::new(Family::Sensor, "inbox", Origin::Project).with_overrides(vec![o(Origin::Project), o(Origin::User), o(Origin::Builtin)]);
        assert_eq!(item.overrides.iter().map(|o| o.origin).collect::<Vec<_>>(), [Origin::Builtin, Origin::User, Origin::Project]);
    }

    #[test]
    fn an_entry_puts_its_family_fields_after_the_descriptor() {
        #[derive(Serialize)]
        struct Label {
            label: &'static str,
        }
        let e = Entry { item: Item::new(Family::Theme, "nord", Origin::Builtin), fields: Label { label: "Nord" } };
        let v = serde_json::to_string(&e).unwrap();
        assert!(v.starts_with(r#"{"family":"theme","name":"nord""#) && v.ends_with(r#""detail":null,"label":"Nord"}"#), "{v}");
    }

    #[test]
    fn the_other_words_serialize_as_written() {
        fn v<T: Serialize>(x: &T) -> String {
            serde_json::to_value(x).unwrap().as_str().unwrap().to_string()
        }
        assert_eq!(v(&Family::Profile), "profile");
        assert_eq!(v(&How::Replaces), "replaces");
        assert_eq!(v(&Tone::Warn), "warn");
        for k in [ItemKind::Plugin, ItemKind::Crate, ItemKind::Command, ItemKind::Mcp, ItemKind::Ways, ItemKind::Anthropic, ItemKind::Openrouter, ItemKind::ClaudeCode] {
            assert_eq!(v(&k), k.word());
        }
        assert_eq!(v(&ItemKind::ClaudeCode), "claude-code");
        for o in [Origin::Builtin, Origin::User, Origin::Project] {
            assert_eq!(v(&o), o.word());
        }
    }

    #[test]
    fn the_origin_text_and_file_follow_the_overrides() {
        let o = |origin: Origin, how: How| Override { origin, how, path: format!("/{}", origin.word()).into() };
        let bare = Item { path: Some("/base".into()), ..Item::new(Family::Sensor, "git", Origin::Builtin) };
        assert_eq!((bare.origin_text().as_str(), bare.file()), ("builtin", Some(Path::new("/base"))));
        let over = bare.with_overrides(vec![o(Origin::Project, How::Replaces), o(Origin::User, How::Extends)]);
        assert_eq!(over.origin_text(), "builtin · user extends · project replaces");
        assert_eq!(over.file(), Some(Path::new("/project")));
        assert_eq!(Item::new(Family::Sensor, "x", Origin::Builtin).file(), None);
    }
}
