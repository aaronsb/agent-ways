//! The sensor spec (ADR-403): every sensor is one YAML file. The file
//! carries the shared core (identity, `enabled`, `where` conditions), the
//! attend block for the kinds attend hosts (cadence, emission, permissions,
//! triggers, state), and its kind's own block.
//!
//! This crate parses, merges and checks files and runs nothing. Each host
//! reads its own directories, layers the files of one name with [`merge`],
//! and hands the result to [`parse`]. A [`Problem`] of severity
//! [`Severity::Invalid`] keeps the sensor from running; the others are lint
//! findings on a sensor that loads.

pub mod path;
pub mod template;

mod merge;
mod parse;
#[cfg(test)]
mod tests;

pub use merge::merge;
pub use parse::{parse, Parsed};

use std::collections::BTreeMap;

use path::JsonPath;
use serde_yaml::{Mapping, Value};
use template::Template;

/// The spec version this crate reads. A file names it in `spec`; a file
/// with no `spec` is read as this version.
pub const SPEC_VERSION: u64 = 1;

/// The bounds of §8.
pub mod bounds {
    /// At most this many declared state fields.
    pub const STATE_FIELDS: usize = 16;
    /// A state field's name, `[a-z][a-z0-9_]*`, is at most this long.
    pub const STATE_NAME: usize = 32;
    /// A string field's `max` when none is given.
    pub const STRING_MAX_DEFAULT: u64 = 256;
    /// The largest `max` a string field may take.
    pub const STRING_MAX: u64 = 1024;
    /// The serialized state is at most this many bytes.
    pub const STATE_BYTES: u64 = 4096;
    /// A command sensor's timeout when none is given, in seconds.
    pub const COMMAND_TIMEOUT: u64 = 10;
    /// The longest timeout a command sensor may set, in seconds.
    pub const COMMAND_TIMEOUT_MAX: u64 = 600;
    /// A sensor's store is removed this many days after its last write.
    pub const RETENTION_DAYS: u64 = 30;
}

/// The cadence and threshold a file that sets none takes.
pub const DEFAULTS: Cadence = Cadence { interval: 60, min_interval: 15, threshold: 2.0, decay_threshold: 4 };

/// A sensor's kind, and the host that runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Compiled poll code in attend.
    Crate,
    /// An argv attend runs directly.
    Command,
    /// One read-only tool call on a stdio MCP server, run by attend.
    Mcp,
    /// The session's hook stream, run by ways (ADR-199).
    Ways,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Crate => "crate",
            Kind::Command => "command",
            Kind::Mcp => "mcp",
            Kind::Ways => "ways",
        }
    }

    /// The kind a file's `kind` names.
    pub fn parse(s: &str) -> Option<Kind> {
        [Kind::Crate, Kind::Command, Kind::Mcp, Kind::Ways].into_iter().find(|k| k.as_str() == s)
    }

    /// Whether the file runs code its author chose: a project file of this
    /// kind needs the project allow-listed (§2).
    pub fn runs_chosen_code(self) -> bool {
        matches!(self, Kind::Command | Kind::Mcp)
    }
}

/// Where a sensor's notifications go (§3a, ADR-136).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// What the environment generates: governed, refractory.
    Event,
    /// Words a person or agent composed.
    Message,
}

impl Lane {
    pub fn as_str(self) -> &'static str {
        match self {
            Lane::Event => "event",
            Lane::Message => "message",
        }
    }
}

/// The layer a file came from, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// Shipped with the host.
    Builtin,
    User,
    Project,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Builtin => "builtin",
            Origin::User => "user",
            Origin::Project => "project",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cadence {
    /// Seconds between polls while quiet.
    pub interval: u64,
    /// Seconds between polls while things change.
    pub min_interval: u64,
    /// The accumulated magnitude that discloses.
    pub threshold: f64,
    /// Quiet polls before the interval decays back.
    pub decay_threshold: u32,
}

/// How bad a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Lint reports it; the sensor runs.
    Warning,
    /// Lint reports it as an error; the sensor runs with the correction the
    /// message names.
    Error,
    /// The sensor does not run.
    Invalid,
}

/// One problem with a sensor file, at a dotted field path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub at: String,
    pub message: String,
    pub severity: Severity,
}

impl Problem {
    pub(crate) fn invalid(at: impl Into<String>, message: impl Into<String>) -> Problem {
        Problem { at: at.into(), message: message.into(), severity: Severity::Invalid }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let level = match self.severity {
            Severity::Warning => "warning: ",
            Severity::Error => "error: ",
            Severity::Invalid => "",
        };
        match self.at.as_str() {
            "" => write!(f, "{level}{}", self.message),
            at => write!(f, "{at}: {level}{}", self.message),
        }
    }
}

/// A sensor, its layers merged.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorSpec {
    pub spec: u64,
    pub name: String,
    pub description: String,
    pub kind: Kind,
    pub enabled: bool,
    pub cadence: Cadence,
    /// The lane the file declares; [`SensorSpec::lane`] is the one it runs on.
    pub declared_lane: Lane,
    /// A crate sensor's author field: the field of its compiled items that
    /// holds who composed one.
    pub author: Option<String>,
    /// Why a shipped file rides the message lane with no author (§3a). Only
    /// a shipped layer may set it.
    pub lane_exception: Option<String>,
    /// The tool permissions the sensor needs (ADR-116).
    pub requires: Vec<String>,
    pub triggers: Vec<Trigger>,
    /// Declared state fields, by name (§3b).
    pub state: BTreeMap<String, StateField>,
    pub block: Block,
}

impl SensorSpec {
    /// The lane the sensor runs on: its declared lane, unless the message
    /// lane's author rule fails, which puts it on the event lane.
    pub fn lane(&self) -> Lane {
        match self.lane_refusal() {
            Some(_) => Lane::Event,
            None => self.declared_lane,
        }
    }

    /// Why a sensor that declares the message lane runs on the event lane.
    pub fn lane_refusal(&self) -> Option<String> {
        if self.declared_lane != Lane::Message || self.lane_exception.is_some() {
            return None;
        }
        match self.kind {
            Kind::Crate if self.author.is_none() => Some("lane: message needs an author field naming who composed each item; it runs on the event lane".into()),
            Kind::Crate => None,
            _ if self.triggers.is_empty() => Some("lane: message needs a trigger with an author field; it runs on the event lane".into()),
            _ => self
                .triggers
                .iter()
                .find(|t| t.author.is_none())
                .map(|t| format!("lane: message needs an author field on each trigger, and trigger {} has none; it runs on the event lane", t.name)),
        }
    }

    /// What the sensor reads: a crate's id, a command's program, an mcp
    /// sensor's server and tool.
    pub fn source(&self) -> String {
        match &self.block {
            Block::Crate(c) => c.id.clone(),
            Block::Command(c) => c.argv.first().cloned().unwrap_or_default(),
            Block::Mcp(m) => format!("{} {}", m.server.label(), m.tool),
            Block::Ways(_) => "ways".into(),
        }
    }
}

/// The kind's own block.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Crate(CrateBlock),
    Command(CommandBlock),
    Mcp(Box<McpBlock>),
    /// ways' rules and dispatch (ADR-199), carried as written: attend hosts
    /// no ways sensor.
    Ways(Mapping),
}

/// A compiled sensor.
#[derive(Debug, Clone, PartialEq)]
pub struct CrateBlock {
    /// The compiled sensor the file configures, as the host registers it.
    pub id: String,
    /// The compiled sensor's own fields, such as `processes`' `watch`. The
    /// host checks them against the fields the crate declares.
    pub fields: Mapping,
}

/// How a command sensor's stdout reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// One JSON document: the poll result, with an optional top-level
    /// `state` object of emitted fields.
    Json,
    /// `magnitude|description` lines.
    Lines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandBlock {
    /// The program and its arguments, run with no shell.
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    /// Seconds before the run is killed.
    pub timeout: u64,
    pub output: Output,
}

/// The server an mcp sensor calls.
#[derive(Debug, Clone, PartialEq)]
pub enum Server {
    /// A server of Claude Code's MCP configuration, by name.
    Named(String),
    /// A server defined in the file.
    Inline { command: String, args: Vec<String>, env: BTreeMap<String, String> },
}

impl Server {
    pub fn label(&self) -> &str {
        match self {
            Server::Named(n) => n,
            Server::Inline { command, .. } => command,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpBlock {
    pub server: Server,
    pub tool: String,
    /// The tool's fixed arguments.
    pub args: Mapping,
    /// Run a tool that does not declare `readOnlyHint: true`.
    pub allow_unannotated: bool,
    /// What `attend sensor probe` recorded (§4).
    pub probe: Option<ProbeRecord>,
}

/// A probe's record (§4). It holds for the server, tool and args it names.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeRecord {
    pub server: String,
    pub tool: String,
    pub args: Mapping,
    /// Whether the second call returned every item of the first.
    pub calls_match: bool,
    /// Whether the operator confirmed the items were left untouched.
    pub confirmed: bool,
    /// When it was taken.
    pub at: Option<String>,
}

impl McpBlock {
    /// Whether the probe record holds for the sensor as configured: taken
    /// with its server, tool and args, with both checks passed.
    pub fn probe_holds(&self) -> bool {
        self.probe.as_ref().is_some_and(|p| p.calls_match && p.confirmed && p.server == self.server.label() && p.tool == self.tool && p.args == self.args)
    }
}

/// What a trigger compares between two polls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum On {
    Added,
    Removed,
    Changed,
    Enters,
    Leaves,
}

impl On {
    fn parse(s: &str) -> Option<On> {
        Some(match s {
            "added" => On::Added,
            "removed" => On::Removed,
            "changed" => On::Changed,
            "enters" => On::Enters,
            "leaves" => On::Leaves,
            _ => return None,
        })
    }
}

/// How a trigger's conditions combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Match {
    #[default]
    All,
    Any,
}

/// A state trigger's test on one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Changed,
    Rises,
    Falls,
}

/// A trigger (§3). It fires on a change between two polls.
#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub name: String,
    pub test: Test,
    pub match_: Match,
    /// The item field, or on a state trigger the state field, naming who
    /// composed the item.
    pub author: Option<String>,
    pub magnitude: Option<f64>,
    pub description: Option<Template>,
}

/// What a trigger looks at.
#[derive(Debug, Clone, PartialEq)]
pub enum Test {
    /// Items in the poll result.
    Items { path: JsonPath, key: Option<String>, on: On, where_: Where },
    /// Declared state fields.
    State(BTreeMap<String, Change>),
}

/// A condition object: each property, a name or a glob over names, with a
/// condition on its value.
pub type Where = BTreeMap<String, Condition>;

/// A condition on one property.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// A bare value: equal to it, or a glob when it is a string with glob
    /// characters.
    Is(Value),
    /// One or more operators, all of which hold.
    Ops(Vec<Op>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Equals(Value),
    Not(Value),
    In(Vec<Value>),
    Contains(Value),
    /// A glob.
    Like(String),
    /// A regular expression.
    Matches(String),
    Above(f64),
    Below(f64),
}

/// A declared state field's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateType {
    Number,
    String,
    Boolean,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateField {
    pub ty: StateType,
    /// A string's length bound, in characters.
    pub max: u64,
    /// The path over the poll result that fills it; none for an emitted
    /// field.
    pub from: Option<JsonPath>,
}

impl StateField {
    /// The most bytes the field can take in the serialized state: its name,
    /// quotes and separators, and its value at the worst case.
    fn max_bytes(&self, name: &str) -> u64 {
        let value = match self.ty {
            StateType::Number => 24,
            StateType::Boolean => 5,
            // Four bytes a character, plus the quotes.
            StateType::String => self.max * 4 + 2,
        };
        name.len() as u64 + 4 + value
    }
}

/// Whether `n` is a sensor or trigger name: letters, digits, `-` and `_`,
/// starting with a letter or digit.
pub fn is_name(n: &str) -> bool {
    n.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

