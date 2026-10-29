//! A module: a mod, a sensor or a theme, described the same way whatever
//! shows it. One function per kind builds a [`Module`] from what that kind
//! reads ([`mods`], [`sensors`], [`themes`]); the settings screens draw it
//! as the detail pane's card. Paths are kept whole; a reader shows them as
//! it likes.

pub mod mods;
pub mod sensors;
pub mod themes;

use std::path::PathBuf;

use serde::Serialize;

/// One module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Module {
    pub name: String,
    /// A line on what it is.
    pub summary: String,
    /// `None` for a sensor whose file names no kind it has.
    pub kind: Option<Kind>,
    pub origin: Origin,
    /// Where the module was added: the file that names it. `None` for a
    /// builtin.
    pub path: Option<PathBuf>,
    /// What it is read from: a file, a crate, a plugin and its folder.
    pub source: String,
    pub version: Option<String>,
    pub state: State,
    /// What its kind adds, in order.
    pub rows: Vec<Row>,
}

/// What implements a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A Claude Code plugin of the agent-ways marketplace.
    Plugin,
    /// A sensor compiled into attend.
    Crate,
    /// A sensor that runs an argv.
    Command,
    /// A sensor that calls a tool on an MCP server.
    Mcp,
    /// A sensor of ways' own, which attend does not host (ADR-199).
    Ways,
    Theme,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Plugin => "plugin",
            Kind::Crate => "crate",
            Kind::Command => "command",
            Kind::Mcp => "mcp",
            Kind::Ways => "ways",
            Kind::Theme => "theme",
        }
    }
}

impl From<sensor_spec::Kind> for Kind {
    fn from(k: sensor_spec::Kind) -> Kind {
        match k {
            sensor_spec::Kind::Crate => Kind::Crate,
            sensor_spec::Kind::Command => Kind::Command,
            sensor_spec::Kind::Mcp => Kind::Mcp,
            sensor_spec::Kind::Ways => Kind::Ways,
        }
    }
}

/// Who added a module: agent-ways, the user, or a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Builtin,
    User,
    Project,
}

/// A module's state in words, and how it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct State {
    pub word: String,
    pub health: Health,
}

impl State {
    pub fn new(word: impl Into<String>, health: Health) -> State {
        State { word: word.into(), health }
    }
}

/// How a module stands: running, idle by choice, wanting a look, or not
/// running when it should.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    Running,
    Idle,
    Attention,
    Broken,
}

/// One row of a module's kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    pub label: String,
    pub value: String,
}

impl Row {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Row {
        Row { label: label.into(), value: value.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_serializes_with_plain_field_names() {
        let m = Module {
            name: "downloads".into(),
            summary: "a sensor of your own".into(),
            kind: Some(Kind::Command),
            origin: Origin::Project,
            path: Some("/p/.claude/attend/sensors/downloads.yaml".into()),
            source: "/p/dl.sh".into(),
            version: None,
            state: State::new("active", Health::Running),
            rows: vec![Row::new("threshold", "2")],
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "name": "downloads", "summary": "a sensor of your own", "kind": "command", "origin": "project",
                "path": "/p/.claude/attend/sensors/downloads.yaml", "source": "/p/dl.sh", "version": null,
                "state": {"word": "active", "health": "running"}, "rows": [{"label": "threshold", "value": "2"}]
            })
        );
    }
}
