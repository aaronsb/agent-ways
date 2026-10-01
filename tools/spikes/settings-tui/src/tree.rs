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
    /// Present or absent. Its material is never held; entry is masked and
    /// committing queues an action rather than changing the value.
    Secret,
}

impl Kind {
    /// Whether the value editor may change this kind in place.
    pub fn editable(&self) -> bool {
        !matches!(self, Kind::ReadOnly | Kind::Secret)
    }
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
            Kind::Secret => Err("secret: entered masked, never stored here".into()),
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
    pub actions: Vec<Action>,
}

impl Node {
    pub fn group(name: impl Into<String>, doc: impl Into<String>, children: Vec<Node>) -> Self {
        Node { name: name.into(), doc: doc.into(), setting: None, children, open: false, actions: vec![] }
    }
    pub fn leaf(name: impl Into<String>, doc: impl Into<String>, setting: Setting) -> Self {
        Node { name: name.into(), doc: doc.into(), setting: Some(setting), children: vec![], open: false, actions: vec![] }
    }
    pub fn opened(mut self) -> Self {
        self.open = true;
        self
    }
    pub fn with_actions(mut self, actions: Vec<Action>) -> Self {
        self.actions = actions;
        self
    }
    pub fn changes(&self) -> usize {
        self.setting.as_ref().map_or(0, |s| s.changed() as usize)
            + self.children.iter().map(Node::changes).sum::<usize>()
    }
}

/// What an action needs typed before it can be queued.
#[derive(Debug, Clone)]
pub enum Arg {
    None,
    /// A visible argument, such as a path; the text names the prompt.
    Text(String),
    /// Entered masked and handed to the command on stdin, never in argv.
    Secret,
}

/// Something a node can do that is not a value change: a named command line.
#[derive(Debug, Clone)]
pub struct Action {
    pub label: String,
    /// The command line; `{}` takes a `Text` argument.
    pub command: String,
    pub arg: Arg,
    /// Ask y/n before queueing: the action is destructive or reconciles.
    pub confirm: bool,
}

impl Action {
    pub fn new(label: impl Into<String>, command: impl Into<String>) -> Self {
        Action { label: label.into(), command: command.into(), arg: Arg::None, confirm: false }
    }
    pub fn arg(mut self, arg: Arg) -> Self {
        self.arg = arg;
        self
    }
    pub fn confirm(mut self) -> Self {
        self.confirm = true;
        self
    }

    /// The command line as queued. A secret shows as `<stdin>`.
    pub fn render(&self, text: &str) -> String {
        match self.arg {
            Arg::None => self.command.clone(),
            Arg::Text(_) => self.command.replace("{}", &quote(text)),
            Arg::Secret => format!("{} < <stdin>", self.command),
        }
    }
}

/// Quote one word for a shell, leaving plain paths bare.
pub fn quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || "/._-~=:@+,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// An action waiting to be run: the node it came from and the final command.
#[derive(Debug, Clone)]
pub struct Queued {
    pub key: String,
    pub label: String,
    pub command: String,
}

/// Queued actions, in the order they were chosen.
#[derive(Debug, Default)]
pub struct Queue(Vec<Queued>);

impl Queue {
    pub fn push(&mut self, q: Queued) {
        self.0.push(q);
    }
    /// Drop the most recent action.
    pub fn undo_last(&mut self) -> Option<Queued> {
        self.0.pop()
    }
    pub fn items(&self) -> &[Queued] {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Actions queued from nodes under the root named `root`.
    pub fn under(&self, root: &str) -> usize {
        let dotted = format!("{root}.");
        self.0.iter().filter(|q| q.key == root || q.key.starts_with(&dotted)).count()
    }
}

/// Typed secret text. Debug redacts, there is no Display, and the bytes are
/// overwritten when it drops. Capacity is fixed up front so growth never
/// leaves a copy behind.
pub struct SecretBuf(String);

const SECRET_CAP: usize = 512;

impl Default for SecretBuf {
    fn default() -> Self {
        SecretBuf(String::with_capacity(SECRET_CAP))
    }
}

impl SecretBuf {
    pub fn push(&mut self, c: char) {
        if self.0.len() + c.len_utf8() <= SECRET_CAP {
            self.0.push(c);
        }
    }
    pub fn pop(&mut self) {
        self.0.pop();
    }
    pub fn len(&self) -> usize {
        self.0.chars().count()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for SecretBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBuf(<redacted>)")
    }
}

impl Drop for SecretBuf {
    fn drop(&mut self) {
        let mut b = std::mem::take(&mut self.0).into_bytes();
        b.fill(0);
        std::hint::black_box(&b);
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
/// with its ancestors, whatever is open; each root heads its own matches.
pub fn rows(roots: &[Node], filter: &str) -> Vec<Row> {
    rows_in(roots, 0, roots.len(), filter)
}

/// The rows of one tab: the root at index `tab` and what is open under it.
pub fn tab_rows(roots: &[Node], tab: usize) -> Vec<Row> {
    rows_in(roots, tab, 1, "")
}

fn rows_in(roots: &[Node], skip: usize, take: usize, filter: &str) -> Vec<Row> {
    let mut out = Vec::new();
    let f = filter.to_lowercase();
    for (i, n) in roots.iter().enumerate().skip(skip).take(take) {
        walk(n, vec![i], &n.name.to_lowercase(), &f, &mut out);
    }
    out
}

/// What a tab has pending: value changes plus queued actions under its root.
pub fn pending(root: &Node, queue: &Queue) -> usize {
    root.changes() + queue.under(&root.name)
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
    fn tab_rows_cover_one_root_and_pending_counts_its_queue() {
        let mut t = sample();
        t.push(Node::group("gate", "", vec![]));
        assert_eq!(tab_rows(&t, 1).len(), 1);
        assert_eq!(rows(&t, "").len(), 2);
        let mut q = Queue::default();
        for k in ["matching.scope", "gate", "gatekeeper"] {
            q.push(Queued { key: k.into(), label: String::new(), command: String::new() });
        }
        assert_eq!((pending(&t[0], &q), pending(&t[1], &q)), (1, 1));
    }

    #[test]
    fn queue_keeps_order_and_undoes_the_last() {
        let mut q = Queue::default();
        let a = Action::new("add", "ways x add {}").arg(Arg::Text("dir".into()));
        for d in ["a", "b c"] {
            q.push(Queued { key: "k".into(), label: a.label.clone(), command: a.render(d) });
        }
        assert_eq!(q.items()[1].command, "ways x add 'b c'");
        assert_eq!(q.undo_last().unwrap().command, "ways x add 'b c'");
        assert_eq!(q.len(), 1);
        assert_eq!(q.items()[0].command, "ways x add a");
    }

    #[test]
    fn secret_buf_debug_redacts() {
        let mut b = SecretBuf::default();
        "hunter2".chars().for_each(|c| b.push(c));
        assert!(!format!("{b:?}").contains("hunter"));
        assert_eq!(b.len(), 7);
        let a = Action::new("set", "ways k add").arg(Arg::Secret);
        assert_eq!(a.render(""), "ways k add < <stdin>");
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
