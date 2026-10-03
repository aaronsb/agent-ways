//! Schema types (ADR-503 §1). A component declares its settings as a static
//! [`Schema`]: the sections of each file kind it owns and every key in them.
//! Parsing, validation, lint, emit and help all read these declarations.

use crate::load::Layer;
use serde_yaml::Value;

/// Where a key may be set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The user's file only.
    User,
    /// A project's file only.
    Project,
    /// Either; `set` writes the user file unless a project is named.
    Both,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project => "project",
            Scope::Both => "user, project",
        }
    }
}

/// The scope a loaded layer stands at. A target layer takes the same keys as
/// the user layer except those only the user file may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerScope {
    User,
    Target,
    Project,
}

impl LayerScope {
    /// Whether a key of `scope` may be read from a layer at this scope.
    pub fn admits(self, scope: Scope) -> bool {
        matches!(
            (self, scope),
            (_, Scope::Both) | (LayerScope::User, Scope::User) | (LayerScope::Project, Scope::Project)
        )
    }
}

/// The type of a key's value.
///
/// Callers compare a kind only with a variant that holds no function, such
/// as `Kind::Secret`; two computed choices compare by function address.
#[allow(unpredictable_function_pointer_comparisons)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Bool,
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64 },
    /// One of a fixed list.
    Choice(&'static [&'static str]),
    /// One of a list computed when the settings load (with `multi`, a list
    /// of them). The owning crate supplies the list from the layers it is
    /// given, as a [`DefaultValue::Fn`] reads its base, and from the machine
    /// where the list lives there (installed themes, the language registry).
    /// A source that cannot answer leaves the key accepting text, and its
    /// help says why.
    ChoiceOf { options: Options, multi: bool },
    Text,
    Path,
    /// A sequence of strings.
    List,
    /// `true`/`false`, or a mapping whose `enabled` is a bool (ADR-131 long form).
    Toggle,
    /// Shown, never set by `set`; changed by an action command.
    ReadOnly,
    /// Present or absent; the value never passes through the schema (ADR-503 §12).
    Secret,
}

/// The list of a computed choice, read from the layers given. `Err` says
/// why the source cannot answer.
pub type Options = fn(&[Layer]) -> Result<Vec<String>, String>;

/// The choices a key offers.
#[derive(Debug, Clone, PartialEq)]
pub enum Choices {
    /// Not a choice, or a computed one whose list was not read.
    None,
    /// One of `items`, or with `multi` a list of them.
    Of { items: Vec<String>, multi: bool },
    /// A computed choice whose source could not answer, and why. The key
    /// accepts text.
    Unavailable(String),
}

impl Kind {
    /// What the type is, with the choices in effect over `layers`.
    pub fn describe(&self, layers: &[Layer]) -> String {
        match self {
            Kind::Bool => "bool".into(),
            Kind::Int { min, max } if *max == i64::MAX => format!("int, at least {min}"),
            Kind::Int { min, max } => format!("int, {min}..{max}"),
            Kind::Float { min, max } => format!("float, {min}..{max}"),
            Kind::Choice(_) | Kind::ChoiceOf { .. } => match self.choices(Some(layers)) {
                Choices::Of { items, multi: false } => format!("one of {}", items.join(", ")),
                Choices::Of { items, multi: true } => format!("a list, each one of {}", items.join(", ")),
                Choices::Unavailable(why) => format!("{} (the choices could not be listed: {why})", self.shape()),
                Choices::None => self.shape().into(),
            },
            Kind::Text => "text".into(),
            Kind::Path => "path".into(),
            Kind::List => "list of text".into(),
            Kind::Toggle => "bool (on or off)".into(),
            Kind::ReadOnly => "read-only".into(),
            Kind::Secret => "secret (present or absent)".into(),
        }
    }

    /// The choices over `layers`. With no layers, as when a load checks one
    /// file on its own, a computed list is not read: its key is checked as
    /// text there, and `set`, `apply` and `lint` check it against the list.
    pub fn choices(&self, layers: Option<&[Layer]>) -> Choices {
        match (self, layers) {
            (Kind::Choice(c), _) => Choices::Of { items: c.iter().map(|s| s.to_string()).collect(), multi: false },
            (Kind::ChoiceOf { options, multi }, Some(l)) => match options(l) {
                // An item no one could type or see whole is no choice.
                Ok(items) => Choices::Of { items: items.into_iter().filter(|i| !i.chars().any(char::is_control)).collect(), multi: *multi },
                Err(why) => Choices::Unavailable(one_line(&why)),
            },
            _ => Choices::None,
        }
    }

    /// Whether a value of this kind is a list of choices.
    pub fn is_multi(&self) -> bool {
        self.multi()
    }

    fn multi(&self) -> bool {
        matches!(self, Kind::ChoiceOf { multi: true, .. })
    }

    /// The stored shape of a choice: text, or a list of text.
    fn shape(&self) -> &'static str {
        if self.multi() {
            "list of text"
        } else {
            "text"
        }
    }

    /// Check a stored value against the type. The message names what is
    /// wrong. A computed choice is checked as text here; [`Kind::check_in`]
    /// checks it against its list.
    pub fn check(&self, v: &Value) -> Result<(), String> {
        self.check_in(v, None)
    }

    /// Check a stored value against the type, and a choice against the
    /// choices over `layers`.
    pub fn check_in(&self, v: &Value, layers: Option<&[Layer]>) -> Result<(), String> {
        self.check_in_keeping(v, layers, &[])
    }

    /// [`Kind::check_in`], where the items of a multi choice in `keep`, such
    /// as those already stored, pass though no longer in the list.
    fn check_in_keeping(&self, v: &Value, layers: Option<&[Layer]>, keep: &[String]) -> Result<(), String> {
        match self {
            Kind::Bool => v.as_bool().map(|_| ()).ok_or_else(|| format!("expected a bool, found {}", show(v))),
            Kind::Int { min, max } => {
                let n = v.as_i64().ok_or_else(|| format!("expected an integer, found {}", show(v)))?;
                if n < *min || n > *max {
                    return Err(format!("{n} is outside {}", Kind::Int { min: *min, max: *max }.describe(&[])));
                }
                Ok(())
            }
            Kind::Float { min, max } => {
                let n = match v {
                    Value::Number(n) => n.as_f64(),
                    _ => None,
                }
                .ok_or_else(|| format!("expected a number, found {}", show(v)))?;
                if !(n >= *min && n <= *max) {
                    return Err(format!("{n} is outside {min}..{max}"));
                }
                Ok(())
            }
            Kind::Choice(_) | Kind::ChoiceOf { .. } => check_choice(v, &self.choices(layers), self.multi(), keep),
            Kind::Text | Kind::Path => v.as_str().map(|_| ()).ok_or_else(|| format!("expected text, found {}", show(v))),
            Kind::List => match v {
                Value::Sequence(s) if s.iter().all(|i| i.is_string()) => Ok(()),
                _ => Err(format!("expected a list of text, found {}", show(v))),
            },
            Kind::Toggle => match v {
                Value::Bool(_) => Ok(()),
                Value::Mapping(m) => match m.get("enabled") {
                    None | Some(Value::Bool(_)) => Ok(()),
                    Some(o) => Err(format!("enabled: expected a bool, found {}", show(o))),
                },
                _ => Err(format!("expected a bool, found {}", show(v))),
            },
            Kind::ReadOnly => Ok(()),
            Kind::Secret => Err("a secret is never stored in a settings file".into()),
        }
    }

    /// Parse a command-line value into a typed value. Errors name the type.
    /// A choice is checked against the choices over `layers`.
    pub fn parse_cli(&self, s: &str, layers: Option<&[Layer]>) -> Result<Value, String> {
        self.parse_cli_keeping(s, layers, &[])
    }

    /// [`Kind::parse_cli`], keeping the items of a multi choice in `keep`.
    pub fn parse_cli_keeping(&self, s: &str, layers: Option<&[Layer]>, keep: &[String]) -> Result<Value, String> {
        let v = match self {
            Kind::Bool | Kind::Toggle => match s {
                "true" | "on" | "yes" => Value::Bool(true),
                "false" | "off" | "no" => Value::Bool(false),
                _ => return Err(format!("expected true or false, found '{s}'")),
            },
            Kind::Int { .. } => {
                Value::Number(s.trim().parse::<i64>().map_err(|_| format!("expected an integer, found '{s}'"))?.into())
            }
            Kind::Float { .. } => {
                let f = s.trim().parse::<f64>().map_err(|_| format!("expected a number, found '{s}'"))?;
                Value::Number(f.into())
            }
            Kind::Choice(_) | Kind::ChoiceOf { multi: false, .. } | Kind::Text | Kind::Path => Value::String(s.to_string()),
            Kind::List | Kind::ChoiceOf { multi: true, .. } => {
                let t = s.trim();
                if t.starts_with('[') {
                    serde_yaml::from_str(t).map_err(|e| format!("expected a list such as [a, b]: {e}"))?
                } else if t.is_empty() {
                    Value::Sequence(Vec::new())
                } else {
                    Value::Sequence(t.split(',').map(|p| Value::String(p.trim().to_string())).collect())
                }
            }
            Kind::ReadOnly => return Err("read-only; it is changed by its action command".into()),
            Kind::Secret => return Err("a secret is entered on stdin to its own command, never as an argument".into()),
        };
        self.check_in_keeping(&v, layers, keep)?;
        Ok(v)
    }
}

/// A source's reason as one line of plain text: control characters,
/// newlines among them, become spaces, and runs of spaces one. It lands in
/// help, the screens and a YAML comment in `emit`, where a newline would
/// end the comment and turn the rest into settings.
pub fn one_line(s: &str) -> String {
    s.split(|c: char| c.is_control() || c.is_whitespace()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The one check of a choice, fixed or computed: the shape, then each item
/// against the list. A list that was not read, or could not be, checks the
/// shape alone.
fn check_choice(v: &Value, choices: &Choices, multi: bool, keep: &[String]) -> Result<(), String> {
    let listed = |items: &[String]| items.join(", ");
    let picked: Vec<&str> = match (v, multi) {
        (Value::String(s), false) => vec![s.as_str()],
        (Value::Sequence(seq), true) if seq.iter().all(Value::is_string) => seq.iter().filter_map(Value::as_str).collect(),
        (_, true) => return Err(format!("expected a list of text, found {}", show(v))),
        (_, false) => {
            return Err(match choices {
                Choices::Of { items, .. } => format!("expected one of {}, found {}", listed(items), show(v)),
                _ => format!("expected text, found {}", show(v)),
            })
        }
    };
    let Choices::Of { items, .. } = choices else { return Ok(()) };
    match picked.iter().find(|p| !items.iter().any(|i| i == *p) && !(multi && keep.iter().any(|k| k == *p))) {
        None => Ok(()),
        Some(_) if !multi => Err(format!("expected one of {}, found {}", listed(items), show(v))),
        Some(bad) => Err(format!("'{bad}' is not one of {}", listed(items))),
    }
}

/// A value as a short string for messages.
pub fn show(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{s}'"),
        Value::Sequence(_) => "a list".into(),
        Value::Mapping(_) => "a mapping".into(),
        Value::Tagged(_) => "a tagged value".into(),
    }
}

/// A check a key runs beyond its type, returning the message on failure.
pub type Check = fn(&Value) -> Result<(), String>;

/// A check of an entry's name in a per-entry section.
pub type EntryCheck = fn(&str) -> Result<(), String>;

/// A key's default.
#[derive(Clone, Copy)]
pub enum DefaultValue {
    /// No default: the key is absent unless a file sets it.
    None,
    /// A YAML literal.
    Yaml(&'static str),
    /// Computed from the wildcard segments the key was bound with.
    Fn(fn(&[String]) -> Option<Value>),
}

impl std::fmt::Debug for DefaultValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DefaultValue::None => f.write_str("None"),
            DefaultValue::Yaml(s) => write!(f, "Yaml({s})"),
            DefaultValue::Fn(_) => f.write_str("Fn"),
        }
    }
}

/// One key. `name` and `path` may hold `*` segments, bound in pairs: the
/// n-th `*` of the name is the n-th `*` of the path.
#[derive(Debug, Clone, Copy)]
pub struct KeySpec {
    /// Dotted name: `matching.semantic_fire_probability`, `gate.profiles.*.threshold`.
    pub name: &'static str,
    /// The section that owns the key; its fallback unit.
    pub section: &'static str,
    /// The file kind the key lives in.
    pub file: &'static str,
    /// Key path inside the file.
    pub path: &'static [&'static str],
    pub kind: Kind,
    pub default: DefaultValue,
    /// Wildcard bindings that exist with no file setting them, for list and emit.
    pub instances: &'static [&'static str],
    pub scope: Scope,
    /// One line.
    pub doc: &'static str,
    /// The long text `help` prints and the TUI's detail pane shows.
    pub long: &'static str,
    /// A check beyond the type, such as an id grammar.
    pub check: Option<Check>,
    /// A value computed outside the files, such as whether a key file exists.
    pub computed: Option<fn(&[String]) -> Value>,
    /// The closed reading of a value, for a key that switches something off
    /// (ADR-503 addendum): what the key takes when its unit fails the schema,
    /// given the bad value, and when its file does not parse, given no value
    /// (`Value::Null`). `None` from the function means no opinion: the key
    /// falls through to the layers beneath. Keys without one fall through.
    pub fail_closed: Option<FailClosed>,
}

/// The fail-closed reading of a raw value.
pub type FailClosed = fn(&Value) -> Option<Value>;

impl KeySpec {
    /// Whether the name holds a `*`.
    pub fn is_pattern(&self) -> bool {
        self.name.contains('*')
    }

    /// Check a stored value as a load does, one file on its own: the type,
    /// then the key's own check. A computed choice is checked as text.
    pub fn check_value(&self, v: &Value) -> Result<(), String> {
        self.check_value_in(v, None)
    }

    /// Check a value about to be written, or reported by `lint`: as
    /// [`KeySpec::check_value`], and a choice against the choices over
    /// `layers`.
    pub fn check_value_in(&self, v: &Value, layers: Option<&[Layer]>) -> Result<(), String> {
        self.kind.check_in(v, layers)?;
        if let Some(c) = self.check {
            c(v)?;
        }
        Ok(())
    }

    /// The strings each layer of this key's file stores under its path.
    fn stored_items(&self, layers: &[Layer]) -> Vec<String> {
        let mut out = Vec::new();
        for l in layers.iter().filter(|l| l.file == self.file) {
            let mut at = l.accepted.get(self.path.first().copied().unwrap_or(""));
            for k in self.path.iter().skip(1) {
                at = at.and_then(|v| v.get(*k));
            }
            if let Some(Value::Sequence(seq)) = at {
                out.extend(seq.iter().filter_map(Value::as_str).map(str::to_string));
            }
        }
        out
    }

    /// Parse a command-line value: the type, a choice against the choices
    /// over `layers`, then the key's own check.
    pub fn parse_cli(&self, s: &str, layers: &[Layer]) -> Result<Value, String> {
        // A multi choice keeps what a layer already stores, so one entry
        // that has left the list never blocks adding another; lint still
        // reports it.
        let keep = if self.kind.is_multi() { self.stored_items(layers) } else { Vec::new() };
        let v = self.kind.parse_cli_keeping(s, Some(layers), &keep)?;
        if let Some(c) = self.check {
            c(&v)?;
        }
        Ok(v)
    }

    pub fn default_for(&self, bound: &[String]) -> Option<Value> {
        match self.default {
            DefaultValue::None => None,
            DefaultValue::Yaml(s) => serde_yaml::from_str(s).ok(),
            DefaultValue::Fn(f) => f(bound),
        }
    }

    /// Bind the wildcards to produce the concrete dotted name and file path.
    pub fn bind(&self, bound: &[String]) -> (String, Vec<String>) {
        let mut it = bound.iter();
        let name: Vec<String> = self
            .name
            .split('.')
            .map(|s| if s == "*" { it.next().cloned().unwrap_or_default() } else { s.to_string() })
            .collect();
        let mut it = bound.iter();
        let path = self
            .path
            .iter()
            .map(|s| if *s == "*" { it.next().cloned().unwrap_or_default() } else { s.to_string() })
            .collect();
        (name.join("."), path)
    }

    /// Match a concrete dotted name, returning the wildcard bindings. A
    /// binding takes as many dot-separated parts as it needs for the rest
    /// of the pattern to match, so way ids such as `a/b` and model names
    /// with dots bind whole.
    pub fn match_name(&self, name: &str) -> Option<Vec<String>> {
        let pat: Vec<&str> = self.name.split('.').collect();
        let parts: Vec<&str> = name.split('.').collect();
        let mut out = Vec::new();
        if match_parts(&pat, &parts, &mut out) {
            Some(out)
        } else {
            None
        }
    }

    /// Match a concrete file path, returning the wildcard bindings.
    pub fn match_path(&self, path: &[String]) -> Option<Vec<String>> {
        if path.len() != self.path.len() {
            return None;
        }
        let mut out = Vec::new();
        for (p, s) in self.path.iter().zip(path) {
            if *p == "*" {
                out.push(s.clone());
            } else if p != s {
                return None;
            }
        }
        Some(out)
    }
}

fn match_parts(pat: &[&str], parts: &[&str], out: &mut Vec<String>) -> bool {
    match (pat.first(), parts.first()) {
        (None, None) => true,
        (None, _) | (_, None) => false,
        (Some(&"*"), _) => {
            // Shortest binding that lets the rest match; the rest of a
            // pattern holds fixed segments, so this is unambiguous.
            let rest = pat.len() - 1;
            if parts.len() < rest + 1 {
                return false;
            }
            for take in 1..=parts.len() - rest {
                let mut trial = out.clone();
                trial.push(parts[..take].join("."));
                if match_parts(&pat[1..], &parts[take..], &mut trial) {
                    *out = trial;
                    return true;
                }
            }
            false
        }
        (Some(p), Some(q)) => p == q && match_parts(&pat[1..], &parts[1..], out),
    }
}

/// A section: the fallback unit. It owns some top-level keys of one file kind.
#[derive(Debug, Clone, Copy)]
pub struct SectionSpec {
    pub name: &'static str,
    pub file: &'static str,
    /// Top-level keys of the file this section owns.
    pub top: &'static [&'static str],
    /// The fallback unit is each entry of the section's mapping, not the
    /// section: a bad entry is dropped and its siblings load. For a
    /// collection of switches, where losing all of them would turn back on
    /// everything the operator turned off.
    pub per_entry: bool,
    /// The grammar of an entry's name in a per-entry section: a name it
    /// refuses makes that entry fall back, as a bad value in it would.
    pub entry: Option<EntryCheck>,
    /// The command that repairs this section, when `ways settings fix` cannot
    /// (an action command owns it). The diagnostic names it.
    pub repair: Option<&'static str>,
    /// What the screens' header row calls the section's two columns, name
    /// and value, where `setting` and `value` would mislead.
    pub columns: Option<(&'static str, &'static str)>,
    pub doc: &'static str,
}

/// A file kind a schema owns sections of.
#[derive(Debug, Clone, Copy)]
pub struct FileSpec {
    pub id: &'static str,
    /// Retired top-level keys and the message that names their replacement.
    /// They are lint findings and never make a section fall back.
    pub retired: &'static [(&'static str, &'static str)],
}

/// One component's schema.
#[derive(Debug, Clone, Copy)]
pub struct Schema {
    pub component: &'static str,
    pub files: &'static [FileSpec],
    pub sections: &'static [SectionSpec],
    pub keys: &'static [KeySpec],
}

impl Schema {
    pub fn section(&self, name: &str) -> Option<&SectionSpec> {
        self.sections.iter().find(|s| s.name == name)
    }

    pub fn file(&self, id: &str) -> Option<&FileSpec> {
        self.files.iter().find(|f| f.id == id)
    }

    /// The section owning a top-level key of a file kind.
    pub fn section_of_top(&self, file: &str, top: &str) -> Option<&SectionSpec> {
        self.sections.iter().find(|s| s.file == file && s.top.contains(&top))
    }

    pub fn keys_of_section<'a>(&'a self, section: &'a str) -> impl Iterator<Item = &'a KeySpec> + 'a {
        self.keys.iter().filter(move |k| k.section == section)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: KeySpec = KeySpec {
        name: "ways.project.*",
        section: "ways.project",
        file: "config",
        path: &["ways", "*"],
        kind: Kind::Toggle,
        default: DefaultValue::Yaml("true"),
        instances: &[],
        scope: Scope::Project,
        doc: "",
        long: "",
        check: None,
        computed: None,
        fail_closed: None,
    };

    #[test]
    fn wildcards_bind_whole_ids() {
        assert_eq!(K.match_name("ways.project.itops/incident"), Some(vec!["itops/incident".into()]));
        assert_eq!(K.match_name("ways.project"), None);
        let p = KeySpec { name: "gate.profiles.*.model", path: &["profiles", "*", "model"], ..K };
        assert_eq!(p.match_name("gate.profiles.my.fast.model"), Some(vec!["my.fast".into()]));
        assert_eq!(p.match_name("gate.profiles.a.threshold"), None);
        assert_eq!(p.bind(&["a".into()]), ("gate.profiles.a.model".into(), vec!["profiles".into(), "a".into(), "model".into()]));
    }

    #[test]
    fn kinds_check_and_parse() {
        let f = Kind::Float { min: 0.0, max: 1.0 };
        assert!(f.parse_cli("0.4", None).is_ok());
        assert!(f.parse_cli("1.5", None).is_err());
        assert!(f.check(&Value::Number(1.into())).is_ok());
        assert!(Kind::Toggle.check(&serde_yaml::from_str("{enabled: false, later: 1}").unwrap()).is_ok());
        assert!(Kind::Toggle.check(&serde_yaml::from_str("{enabled: 3}").unwrap()).is_err());
        assert_eq!(Kind::List.parse_cli("a, b", None).unwrap(), serde_yaml::from_str::<Value>("[a, b]").unwrap());
        assert!(Kind::Choice(&["x"]).parse_cli("y", None).is_err());
        assert!(Kind::Secret.parse_cli("sk-123", None).is_err());
    }

    /// Names in the layers' `names` lists, after a fixed base: the shape of
    /// a computed list, such as the shipped profiles and the user's.
    fn names(layers: &[Layer]) -> Result<Vec<String>, String> {
        let mut out = vec!["base".to_string()];
        for l in layers {
            if let Some(Value::Sequence(s)) = l.accepted.get("names") {
                out.extend(s.iter().filter_map(Value::as_str).map(str::to_string));
            }
        }
        Ok(out)
    }

    fn offline(_: &[Layer]) -> Result<Vec<String>, String> {
        Err("no network".into())
    }

    #[test]
    fn a_source_s_reason_and_items_reach_the_output_as_one_line() {
        fn noisy(_: &[Layer]) -> Result<Vec<String>, String> {
            Err("timed out\nengine: injected\r\n\tretry \u{7}later".into())
        }
        fn odd(_: &[Layer]) -> Result<Vec<String>, String> {
            Ok(vec!["a".into(), "b\nc: d".into()])
        }
        let d = Kind::ChoiceOf { options: noisy, multi: false }.describe(&[]);
        assert_eq!(d, "text (the choices could not be listed: timed out engine: injected retry later)");
        assert!(!d.chars().any(char::is_control));
        assert_eq!(Kind::ChoiceOf { options: odd, multi: false }.describe(&[]), "one of a");
    }

    fn with_names(names: &str) -> Vec<Layer> {
        let mut l = Layer::from_text(&EMPTY, "user", "cfg", LayerScope::User, None, "");
        l.accepted.insert("names".into(), serde_yaml::from_str(names).unwrap());
        vec![l]
    }

    static EMPTY: Schema = Schema { component: "t", files: &[FileSpec { id: "cfg", retired: &[] }], sections: &[], keys: &[] };

    #[test]
    fn a_computed_choice_accepts_and_refuses_against_the_layers() {
        let one = Kind::ChoiceOf { options: names, multi: false };
        let layers = with_names("[mine]");
        assert_eq!(one.parse_cli("mine", Some(&layers)).unwrap(), Value::from("mine"));
        assert_eq!(one.parse_cli("base", Some(&layers)).unwrap(), Value::from("base"));
        assert_eq!(one.parse_cli("nope", Some(&layers)).unwrap_err(), "expected one of base, mine, found 'nope'");
        assert!(one.parse_cli("mine", Some(&[])).is_err(), "a name only another layer holds");
        // A load checks one file on its own: the shape, not the list.
        assert!(one.check(&Value::from("nope")).is_ok());
        assert!(one.check(&Value::from(3)).is_err());
        assert!(one.check_in(&Value::from("nope"), Some(&layers)).is_err());
        // Several at once.
        let many = Kind::ChoiceOf { options: names, multi: true };
        assert_eq!(many.parse_cli("base, mine", Some(&layers)).unwrap(), serde_yaml::from_str::<Value>("[base, mine]").unwrap());
        assert_eq!(many.parse_cli("[base, x]", Some(&layers)).unwrap_err(), "'x' is not one of base, mine");
        assert!(many.check_in(&Value::from("base"), Some(&layers)).is_err(), "a list, not one");
        // A source that cannot answer leaves the key taking text.
        let net = Kind::ChoiceOf { options: offline, multi: false };
        assert!(net.parse_cli("anything", Some(&layers)).is_ok());
        assert_eq!(net.choices(Some(&layers)), Choices::Unavailable("no network".into()));
    }

    #[test]
    fn describe_lists_the_choices_in_effect() {
        let layers = with_names("[mine]");
        assert_eq!(Kind::Choice(&["a", "b"]).describe(&[]), "one of a, b");
        assert_eq!(Kind::ChoiceOf { options: names, multi: false }.describe(&layers), "one of base, mine");
        assert_eq!(Kind::ChoiceOf { options: names, multi: true }.describe(&[]), "a list, each one of base");
        assert_eq!(Kind::ChoiceOf { options: offline, multi: false }.describe(&[]), "text (the choices could not be listed: no network)");
        // A fixed choice says the same thing it always has.
        assert_eq!(Kind::Choice(&["x"]).check(&Value::from(1)).unwrap_err(), "expected one of x, found 1");
    }
}
