//! The generic half: a tree of typed settings and the edits made to it.
//!
//! Nothing here knows about ways. This module and `ui.rs` are the candidate
//! for a shared crate; `ways.rs` is the adapter a command supplies.

use std::path::PathBuf;

/// What a value may be, and so how the editor changes it.
#[derive(Debug, Clone)]
pub enum Kind {
    Bool,
    Float { min: f64, max: f64 },
    Int { min: i64, max: i64 },
    Choice(Vec<String>),
    Text,
    /// Shown, never edited here; `doc` says which command changes it.
    ReadOnly,
}

/// Where an edit would land: a file and the key inside it.
#[derive(Debug, Clone)]
pub struct Store {
    pub file: PathBuf,
    pub key: String,
}

#[derive(Debug, Clone)]
pub struct Setting {
    pub kind: Kind,
    pub value: String,
    /// The value as loaded, to diff against.
    pub loaded: String,
    pub default: Option<String>,
    /// The layer the loaded value came from: default, user, project, shipped.
    pub source: String,
    pub store: Option<Store>,
}

impl Setting {
    pub fn new(kind: Kind, value: impl Into<String>, source: impl Into<String>) -> Self {
        let value = value.into();
        Setting { kind, loaded: value.clone(), value, default: None, source: source.into(), store: None }
    }
    pub fn default(mut self, d: impl Into<String>) -> Self {
        self.default = Some(d.into());
        self
    }
    pub fn store(mut self, file: PathBuf, key: impl Into<String>) -> Self {
        self.store = Some(Store { file, key: key.into() });
        self
    }
    pub fn changed(&self) -> bool {
        self.value != self.loaded
    }

    /// Parse and range-check text typed for this setting.
    pub fn validate(&self, input: &str) -> Result<String, String> {
        let s = input.trim();
        match &self.kind {
            Kind::Float { min, max } => {
                let v: f64 = s.parse().map_err(|_| format!("not a number: {s}"))?;
                if v < *min || v > *max {
                    return Err(format!("out of range {min}..={max}"));
                }
                Ok(s.to_string())
            }
            Kind::Int { min, max } => {
                let v: i64 = s.parse().map_err(|_| format!("not an integer: {s}"))?;
                if v < *min || v > *max {
                    return Err(format!("out of range {min}..={max}"));
                }
                Ok(v.to_string())
            }
            Kind::Bool => match s {
                "true" | "false" => Ok(s.to_string()),
                _ => Err("true or false".into()),
            },
            Kind::Choice(opts) => {
                if opts.iter().any(|o| o == s) {
                    Ok(s.to_string())
                } else {
                    Err(format!("one of: {}", opts.join(", ")))
                }
            }
            Kind::Text => Ok(s.to_string()),
            Kind::ReadOnly => Err("read-only here".into()),
        }
    }
}

/// A node is a group, a setting, or both: a way is a setting (enabled) that
/// also has child ways.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub doc: String,
    pub setting: Option<Setting>,
    pub children: Vec<Node>,
    pub open: bool,
}

impl Node {
    pub fn group(name: impl Into<String>, doc: impl Into<String>, children: Vec<Node>) -> Self {
        Node { name: name.into(), doc: doc.into(), setting: None, children, open: false }
    }
    pub fn leaf(name: impl Into<String>, doc: impl Into<String>, setting: Setting) -> Self {
        Node { name: name.into(), doc: doc.into(), setting: Some(setting), children: vec![], open: false }
    }
    pub fn opened(mut self) -> Self {
        self.open = true;
        self
    }
    pub fn changes(&self) -> usize {
        self.setting.as_ref().map_or(0, |s| s.changed() as usize)
            + self.children.iter().map(Node::changes).sum::<usize>()
    }
}

/// A visible row: its depth and the index path from the roots.
#[derive(Debug, Clone)]
pub struct Row {
    pub depth: usize,
    pub path: Vec<usize>,
}

pub fn get<'a>(roots: &'a [Node], path: &[usize]) -> &'a Node {
    let mut n = &roots[path[0]];
    for &i in &path[1..] {
        n = &n.children[i];
    }
    n
}

pub fn get_mut<'a>(roots: &'a mut [Node], path: &[usize]) -> &'a mut Node {
    let mut n = &mut roots[path[0]];
    for &i in &path[1..] {
        n = &mut n.children[i];
    }
    n
}

/// The dotted key of the node at `path`.
pub fn key(roots: &[Node], path: &[usize]) -> String {
    let mut parts = Vec::new();
    let mut n = &roots[path[0]];
    parts.push(n.name.clone());
    for &i in &path[1..] {
        n = &n.children[i];
        parts.push(n.name.clone());
    }
    parts.join(".")
}

/// Rows to draw. With a filter, every node whose key contains it is shown
/// with its ancestors, whatever is open.
pub fn rows(roots: &[Node], filter: &str) -> Vec<Row> {
    let mut out = Vec::new();
    let f = filter.to_lowercase();
    for (i, n) in roots.iter().enumerate() {
        walk(n, vec![i], &n.name.to_lowercase(), &f, &mut out);
    }
    out
}

fn walk(n: &Node, path: Vec<usize>, key: &str, f: &str, out: &mut Vec<Row>) -> bool {
    let depth = path.len() - 1;
    if f.is_empty() {
        out.push(Row { depth, path: path.clone() });
        if n.open {
            for (i, c) in n.children.iter().enumerate() {
                let mut p = path.clone();
                p.push(i);
                walk(c, p, "", f, out);
            }
        }
        return true;
    }
    let at = out.len();
    let mut any = false;
    for (i, c) in n.children.iter().enumerate() {
        let mut p = path.clone();
        p.push(i);
        let ck = format!("{key}.{}", c.name.to_lowercase());
        any |= walk(c, p, &ck, f, out);
    }
    if any || key.contains(f) {
        out.insert(at, Row { depth, path });
        true
    } else {
        false
    }
}

/// Every changed setting: (dotted key, store, from, to).
pub fn changes(roots: &[Node]) -> Vec<(String, Option<Store>, String, String)> {
    let mut out = Vec::new();
    fn go(n: &Node, prefix: &str, out: &mut Vec<(String, Option<Store>, String, String)>) {
        let k = if prefix.is_empty() { n.name.clone() } else { format!("{prefix}.{}", n.name) };
        if let Some(s) = &n.setting {
            if s.changed() {
                out.push((k.clone(), s.store.clone(), s.loaded.clone(), s.value.clone()));
            }
        }
        for c in &n.children {
            go(c, &k, out);
        }
    }
    for n in roots {
        go(n, "", &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Node> {
        vec![Node::group(
            "matching",
            "",
            vec![
                Node::leaf("tau_s", "", Setting::new(Kind::Float { min: 0.0, max: 1.0 }, "0.5", "default")),
                Node::leaf("scope", "", Setting::new(Kind::Choice(vec!["agent".into()]), "agent", "user")),
            ],
        )]
    }

    #[test]
    fn closed_groups_hide_children_and_filters_reveal_them() {
        let t = sample();
        assert_eq!(rows(&t, "").len(), 1);
        let r = rows(&t, "tau");
        assert_eq!(r.len(), 2);
        assert_eq!(key(&t, &r[1].path), "matching.tau_s");
    }

    #[test]
    fn validation_ranges_and_changes() {
        let mut t = sample();
        let s = get_mut(&mut t, &[0, 0]).setting.as_mut().unwrap();
        assert!(s.validate("1.5").is_err());
        s.value = s.validate("0.4").unwrap();
        assert_eq!(changes(&t).len(), 1);
        assert_eq!(t[0].changes(), 1);
    }
}
