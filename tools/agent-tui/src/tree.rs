//! A tree of typed settings and the edits made to it: the content model of
//! the tree-and-detail screen. Nothing here knows what the settings are; an
//! adapter builds the tree and applies what it holds (ADR-504 §3).

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
    /// The layer the file belongs to: user or project.
    pub layer: String,
    pub file: PathBuf,
    pub key: String,
    /// The file as the screens show it, such as with `~` for the home
    /// directory. The file's own path unless the adapter says otherwise.
    pub shown: String,
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
    /// Why the value cannot be changed here, when it cannot: its file fails
    /// closed, say. The editor refuses and shows the reason.
    pub locked: Option<String>,
}

impl Setting {
    pub fn new(kind: Kind, value: impl Into<String>, source: impl Into<String>) -> Self {
        let value = value.into();
        Setting { kind, loaded: value.clone(), value, default: None, source: source.into(), store: None, locked: None }
    }
    pub fn default(mut self, d: impl Into<String>) -> Self {
        self.default = Some(d.into());
        self
    }
    pub fn store(mut self, layer: impl Into<String>, file: PathBuf, key: impl Into<String>) -> Self {
        let shown = file.display().to_string();
        self.store = Some(Store { layer: layer.into(), file, key: key.into(), shown });
        self
    }
    /// Show the store's file as `label`.
    pub fn shown_as(mut self, label: impl Into<String>) -> Self {
        if let Some(st) = self.store.as_mut() {
            st.shown = label.into();
        }
        self
    }
    pub fn lock(mut self, why: impl Into<String>) -> Self {
        self.locked = Some(why.into());
        self
    }
    pub fn changed(&self) -> bool {
        self.value != self.loaded
    }
    /// Whether the editor may change the value in place.
    pub fn editable(&self) -> bool {
        self.kind.editable() && self.locked.is_none()
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
    /// A problem the adapter found here, such as a lint finding in the file
    /// the value comes from. Shown on the row and in the detail pane.
    pub finding: Option<String>,
    pub setting: Option<Setting>,
    pub children: Vec<Node>,
    pub open: bool,
    pub actions: Vec<Action>,
    /// What the header row calls the two columns, name and value. Set on a
    /// tab's top-level groups.
    pub columns: Option<(String, String)>,
    /// A section that only gathers rows under a header: its name is no part
    /// of its children's keys ([`key`]).
    pub section: bool,
    /// What the row stands for, beyond its setting: shown below the
    /// controls in the detail pane, under `about_title`.
    pub about: String,
    pub about_title: String,
    /// What the detail pane heads the row with in place of its key: for a
    /// row that stands for no setting, whose key would read as one.
    pub heading: Option<String>,
}

impl Node {
    pub fn group(name: impl Into<String>, doc: impl Into<String>, children: Vec<Node>) -> Self {
        Node { name: name.into(), doc: doc.into(), finding: None, setting: None, children, open: false, actions: vec![], columns: None, section: false, about: String::new(), about_title: String::new(), heading: None }
    }
    pub fn leaf(name: impl Into<String>, doc: impl Into<String>, setting: Setting) -> Self {
        Node { name: name.into(), doc: doc.into(), finding: None, setting: Some(setting), children: vec![], open: false, actions: vec![], columns: None, section: false, about: String::new(), about_title: String::new(), heading: None }
    }
    /// Rows gathered under a header that names their columns, keyed as if
    /// they sat in the parent.
    pub fn section(name: impl Into<String>, doc: impl Into<String>, columns: (&str, &str), children: Vec<Node>) -> Self {
        Node { section: true, ..Node::group(name, doc, children).columns(columns) }.opened()
    }
    pub fn about(mut self, title: impl Into<String>, text: impl Into<String>) -> Self {
        self.about_title = title.into();
        self.about = text.into();
        self
    }
    pub fn headed(mut self, heading: impl Into<String>) -> Self {
        self.heading = Some(heading.into());
        self
    }
    pub fn columns(mut self, (name, value): (&str, &str)) -> Self {
        self.columns = Some((name.to_string(), value.to_string()));
        self
    }
    pub fn opened(mut self) -> Self {
        self.open = true;
        self
    }
    pub fn opened_if(mut self, open: bool) -> Self {
        self.open |= open;
        self
    }
    pub fn with_finding(mut self, finding: impl Into<String>) -> Self {
        self.finding = Some(finding.into());
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
    /// A guided flow, named for the adapter that builds it. Finishing it
    /// queues the flow's own commands, so the action's `command` is only a
    /// description for the detail pane.
    Flow(String),
    /// A change of what the tree shows, named for the adapter
    /// ([`crate::Adapter::view`]). Nothing is queued; the tree reloads.
    View(String),
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
    /// What it does, for the review's detail pane.
    pub doc: String,
    /// The files or directories it changes, when known.
    pub touches: String,
    /// The key that runs it in browse mode and in the menu. Unset: one is
    /// assigned from the label ([`action_keys`]).
    pub key: Option<char>,
}

impl Action {
    pub fn new(label: impl Into<String>, command: impl Into<String>) -> Self {
        Action { label: label.into(), command: command.into(), arg: Arg::None, confirm: false, doc: String::new(), touches: String::new(), key: None }
    }
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }
    pub fn touches(mut self, touches: impl Into<String>) -> Self {
        self.touches = touches.into();
        self
    }
    pub fn arg(mut self, arg: Arg) -> Self {
        self.arg = arg;
        self
    }
    pub fn confirm(mut self) -> Self {
        self.confirm = true;
        self
    }
    pub fn key(mut self, key: char) -> Self {
        self.key = Some(key);
        self
    }

    /// The command line as queued. A secret shows as `<stdin>`.
    pub fn render(&self, text: &str) -> String {
        match self.arg {
            Arg::None => self.command.clone(),
            Arg::Text(_) => self.command.replace("{}", &quote(text)),
            Arg::Secret => format!("{} < <stdin>", self.command),
            Arg::Flow(_) | Arg::View(_) => self.command.clone(),
        }
    }
}

/// The keys browse mode binds itself (`app/keys.rs`), which no action takes.
pub const RESERVED_KEYS: &str = " 123456789/?GXacdeghjklmquwx";

/// The key of each action, in order: its own key, else the first of the
/// label's letters, then the label's letters in upper case, then any letter,
/// that neither a reserved key nor an earlier action holds. `None` when every
/// letter is taken.
pub fn action_keys(actions: &[Action]) -> Vec<Option<char>> {
    let mut taken: Vec<char> = RESERVED_KEYS.chars().chain(actions.iter().filter_map(|a| a.key)).collect();
    actions
        .iter()
        .map(|a| {
            // A reserved key would name a key that does something else.
            if let Some(k) = a.key {
                return (!RESERVED_KEYS.contains(k)).then_some(k);
            }
            let letters: Vec<char> = a.label.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_lowercase()).collect();
            let k = letters
                .iter()
                .copied()
                .chain(letters.iter().map(char::to_ascii_uppercase))
                .chain('a'..='z')
                .chain('A'..='Z')
                .find(|c| !taken.contains(c))?;
            taken.push(k);
            Some(k)
        })
        .collect()
}

/// What is wrong with one node's action keys: a key two actions hold, a key
/// browse mode reserves, an action left without a key. Empty when sound.
pub fn key_conflicts(actions: &[Action]) -> Vec<String> {
    let keys = action_keys(actions);
    let mut out = Vec::new();
    for (i, (a, k)) in actions.iter().zip(&keys).enumerate() {
        match (k, a.key) {
            (None, Some(own)) => out.push(format!("{}: {own} is reserved", a.label)),
            (None, None) => out.push(format!("{}: no key left", a.label)),
            (Some(k), _) if keys[..i].contains(&Some(*k)) => out.push(format!("{}: {k} is bound twice", a.label)),
            _ => {}
        }
    }
    out
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
#[derive(Debug)]
pub struct Queued {
    pub key: String,
    pub label: String,
    /// The command line as shown; a secret argument shows as `<stdin>`.
    pub command: String,
    /// The action asked first: it is destructive or reconciles.
    pub confirm: bool,
    /// Masked text typed for the command's stdin. It reaches only the
    /// command it was typed for, when the command runs.
    pub stdin: Option<SecretBuf>,
}

impl Queued {
    pub fn new(key: impl Into<String>, label: impl Into<String>, command: impl Into<String>, confirm: bool) -> Self {
        Queued { key: key.into(), label: label.into(), command: command.into(), confirm, stdin: None }
    }
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
    /// Drop the action at `i`, wherever it sits.
    pub fn remove(&mut self, i: usize) -> Option<Queued> {
        (i < self.0.len()).then(|| self.0.remove(i))
    }
    pub fn clear(&mut self) {
        self.0.clear();
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
    /// Actions queued from nodes under the node keyed `root`.
    pub fn under(&self, root: &str) -> usize {
        self.0.iter().filter(|q| is_under(&q.key, root)).count()
    }
    /// The place in the queue of the first action under `root`.
    pub fn first_under(&self, root: &str) -> Option<usize> {
        self.0.iter().position(|q| is_under(&q.key, root))
    }
    /// Unqueue every action under `root`.
    pub fn remove_under(&mut self, root: &str) {
        self.0.retain(|q| !is_under(&q.key, root));
    }
}

/// Whether the dotted `key` is `root` or sits below it.
pub fn is_under(key: &str, root: &str) -> bool {
    key == root || key.strip_prefix(root).is_some_and(|r| r.starts_with('.'))
}

/// Typed secret text. Debug redacts, there is no Display, and every byte it
/// ever held is overwritten: a Backspace zeroes the bytes it removes, and a
/// drop zeroes the whole buffer. Capacity is fixed up front so growth never
/// leaves a copy behind.
pub struct SecretBuf(Vec<u8>);

const SECRET_CAP: usize = 512;

impl Default for SecretBuf {
    fn default() -> Self {
        SecretBuf(Vec::with_capacity(SECRET_CAP))
    }
}

impl SecretBuf {
    /// Add a character; false, keeping nothing of it, when the buffer is
    /// full ([`SecretBuf::CAP`] bytes).
    pub fn push(&mut self, c: char) -> bool {
        let mut b = [0u8; 4];
        let e = c.encode_utf8(&mut b).as_bytes();
        let fits = self.0.len() + e.len() <= SECRET_CAP;
        if fits {
            self.0.extend_from_slice(e);
        }
        b.fill(0);
        std::hint::black_box(&b);
        fits
    }

    /// The most a secret holds, in bytes.
    pub const CAP: usize = SECRET_CAP;
    /// Remove the last character and zero its bytes.
    pub fn pop(&mut self) {
        let n = self.reveal().chars().next_back().map_or(0, char::len_utf8);
        let len = self.0.len() - n;
        self.0[len..].fill(0);
        std::hint::black_box(&self.0);
        self.0.truncate(len);
    }
    pub fn len(&self) -> usize {
        self.reveal().chars().count()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// The typed text, for the stdin of the command it was typed for and
    /// nothing else.
    pub fn reveal(&self) -> &str {
        // Only whole characters are ever pushed.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
    /// The `n` bytes past the end, which held removed characters.
    #[cfg(test)]
    fn residue(&self, n: usize) -> Vec<u8> {
        let spare = &self.0.spare_capacity_mut_ref()[..n];
        // SAFETY: test-only; these bytes were written before `pop` truncated.
        spare.iter().map(|b| unsafe { b.assume_init() }).collect()
    }
}

#[cfg(test)]
trait SpareRef {
    fn spare_capacity_mut_ref(&self) -> &[std::mem::MaybeUninit<u8>];
}

#[cfg(test)]
impl SpareRef for Vec<u8> {
    fn spare_capacity_mut_ref(&self) -> &[std::mem::MaybeUninit<u8>] {
        // SAFETY: test-only view of the spare capacity of a Vec<u8>.
        unsafe { std::slice::from_raw_parts(self.as_ptr().add(self.len()) as *const std::mem::MaybeUninit<u8>, self.capacity() - self.len()) }
    }
}

impl std::fmt::Debug for SecretBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBuf(<redacted>)")
    }
}

impl Drop for SecretBuf {
    fn drop(&mut self) {
        // Zero what it holds and the spare capacity Backspace left behind.
        let cap = self.0.capacity();
        self.0.resize(cap, 0);
        self.0.fill(0);
        std::hint::black_box(&self.0);
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

/// The node a dotted key names.
pub fn find<'a>(roots: &'a [Node], key: &str) -> Option<&'a Node> {
    path_of(roots, key).map(|p| get(roots, &p))
}

/// The index path of the node a dotted key names, by [`key`].
pub fn path_of(roots: &[Node], key: &str) -> Option<Vec<usize>> {
    keyed(roots).into_iter().find(|(_, k)| k == key).map(|(p, _)| p)
}

/// Every node's path and key, depth first.
pub fn keyed(roots: &[Node]) -> Vec<(Vec<usize>, String)> {
    fn go(n: &Node, path: Vec<usize>, under: &str, out: &mut Vec<(Vec<usize>, String)>) {
        out.push((path.clone(), own_key(under, n)));
        let below = child_prefix(under, n);
        for (i, c) in n.children.iter().enumerate() {
            let mut p = path.clone();
            p.push(i);
            go(c, p, &below, out);
        }
    }
    let mut out = Vec::new();
    for (i, n) in roots.iter().enumerate() {
        go(n, vec![i], "", &mut out);
    }
    out
}

/// A node's key under its parent's: the dotted name, or for a section,
/// the parent's key marked with the section's name.
fn own_key(under: &str, n: &Node) -> String {
    match (under.is_empty(), n.section) {
        (_, true) => format!("{under}#{}", n.name),
        (true, false) => n.name.clone(),
        (false, false) => format!("{under}.{}", n.name),
    }
}

/// The key a node's children are named under: a section passes its
/// parent's through.
fn child_prefix(under: &str, n: &Node) -> String {
    if n.section { under.to_string() } else { own_key(under, n) }
}

/// The dotted key of the node at `path`. A section adds nothing to the keys
/// below it.
pub fn key(roots: &[Node], path: &[usize]) -> String {
    let mut n = &roots[path[0]];
    let mut under = String::new();
    for &i in &path[1..] {
        under = child_prefix(&under, n);
        n = &n.children[i];
    }
    own_key(&under, n)
}

/// Queued actions under the node at `path`. A section holds those of the
/// rows it gathers, whose keys do not carry its name.
pub fn queued_under(roots: &[Node], path: &[usize], queue: &Queue) -> usize {
    let n = get(roots, path);
    if !n.section {
        return queue.under(&key(roots, path));
    }
    (0..n.children.len())
        .map(|i| {
            let mut p = path.to_vec();
            p.push(i);
            queued_under(roots, &p, queue)
        })
        .sum()
}

/// The node at `path` as the panes name it: its key, with a section named
/// after its parent's key.
pub fn label(roots: &[Node], path: &[usize]) -> String {
    key(roots, path).replace('#', " · ")
}

/// Rows to draw. With a filter, every node whose key contains it is shown
/// with its ancestors, whatever is open; each root heads its own matches.
pub fn rows(roots: &[Node], filter: &str) -> Vec<Row> {
    let mut out = Vec::new();
    let f = filter.to_lowercase();
    for (i, n) in roots.iter().enumerate() {
        walk(n, vec![i], &n.name.to_lowercase(), &f, &mut out);
    }
    out
}

/// The rows of one tab: the children of the root at index `tab`, and what is
/// open under them. The root is the tab itself and has no row.
pub fn tab_rows(roots: &[Node], tab: usize) -> Vec<Row> {
    let mut out = Vec::new();
    for (i, c) in roots[tab].children.iter().enumerate() {
        walk(c, vec![tab, i], "", "", &mut out);
    }
    for r in &mut out {
        r.depth -= 1;
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
        let ck = if c.section { key.to_string() } else { format!("{key}.{}", c.name.to_lowercase()) };
        any |= walk(c, p, &ck, f, out);
    }
    if any || key.contains(f) {
        out.insert(at, Row { depth, path });
        true
    } else {
        false
    }
}

/// Put every changed setting back to its loaded value.
pub fn revert_all(roots: &mut [Node]) {
    fn go(n: &mut Node) {
        if let Some(s) = n.setting.as_mut() {
            s.value = s.loaded.clone();
        }
        n.children.iter_mut().for_each(go);
    }
    roots.iter_mut().for_each(go);
}

/// Every changed setting: (dotted key, store, from, to).
pub fn changes(roots: &[Node]) -> Vec<(String, Option<Store>, String, String)> {
    let mut out = Vec::new();
    fn go(n: &Node, prefix: &str, out: &mut Vec<(String, Option<Store>, String, String)>) {
        let k = child_prefix(prefix, n);
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
    fn action_keys_come_from_the_label_and_skip_reserved_and_taken_keys() {
        let acts = |labels: &[&str]| labels.iter().map(|l| Action::new(*l, "true")).collect::<Vec<_>>();
        // "activate": a and c are reserved, t is free. "add": no lower-case
        // letter is free, so its upper case. "plan": p.
        assert_eq!(action_keys(&acts(&["activate", "add", "plan"])), [Some('t'), Some('A'), Some('p')]);
        // A later action never takes an earlier one's key, nor an explicit one.
        let mut a = acts(&["rotate", "remove", "check"]);
        a[2] = a[2].clone().key('k');
        assert_eq!(action_keys(&a)[..2], [Some('r'), Some('o')]);
        assert!(key_conflicts(&a).iter().any(|c| c.contains("reserved")), "k is a browse key");
        // Explicit keys are held first, so only two explicit keys collide.
        (a[1].key, a[2].key) = (Some('r'), Some('r'));
        assert!(key_conflicts(&a).iter().any(|c| c.contains("twice")));
        a[1].key = None;
        a[2].key = None;
        assert!(key_conflicts(&a).is_empty());
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
        t[0].open = true;
        let tab = tab_rows(&t, 0);
        assert_eq!(tab.len(), 2, "the root has no row");
        assert_eq!((tab[0].depth, key(&t, &tab[0].path)), (0, "matching.tau_s".to_string()));
        assert!(tab_rows(&t, 1).is_empty());
        assert_eq!(rows(&t, "").len(), 4);
        let mut q = Queue::default();
        for k in ["matching.scope", "gate", "gatekeeper"] {
            q.push(Queued::new(k, "", "", false));
        }
        assert_eq!((pending(&t[0], &q), pending(&t[1], &q)), (1, 1));
    }

    #[test]
    fn find_resolves_a_key_whose_names_hold_dots() {
        let t = vec![Node::group("install", "", vec![Node::group("targets", "", vec![Node::leaf("~/.claude", "", Setting::new(Kind::ReadOnly, "on", "user"))])])];
        assert_eq!(find(&t, "install.targets.~/.claude").map(|n| n.name.as_str()), Some("~/.claude"));
        assert!(find(&t, "install.nope").is_none() && find(&t, "installx").is_none());
    }

    #[test]
    fn queue_keeps_order_and_undoes_the_last() {
        let mut q = Queue::default();
        let a = Action::new("add", "ways x add {}").arg(Arg::Text("dir".into()));
        for d in ["a", "b c"] {
            q.push(Queued::new("k", a.label.clone(), a.render(d), a.confirm));
        }
        assert_eq!(q.items()[1].command, "ways x add 'b c'");
        assert_eq!(q.undo_last().unwrap().command, "ways x add 'b c'");
        assert_eq!(q.len(), 1);
        assert_eq!(q.items()[0].command, "ways x add a");
    }

    #[test]
    fn a_full_buffer_refuses_more_and_says_so() {
        let mut b = SecretBuf::default();
        for _ in 0..SecretBuf::CAP {
            assert!(b.push('k'));
        }
        assert!(!b.push('x'), "a character past the cap is refused, not kept or cut");
        assert_eq!(b.reveal().len(), SecretBuf::CAP);
    }

    #[test]
    fn backspace_zeroes_the_bytes_it_removes() {
        let mut b = SecretBuf::default();
        "ab€".chars().for_each(|c| assert!(b.push(c)));
        b.pop();
        assert_eq!(b.reveal(), "ab");
        assert_eq!(b.residue(3), [0, 0, 0], "the removed character's bytes are wiped");
        b.pop();
        assert_eq!((b.reveal(), b.residue(1)), ("a", vec![0]));
    }

    #[test]
    fn secret_buf_debug_redacts() {
        let mut b = SecretBuf::default();
        "hunter2".chars().for_each(|c| assert!(b.push(c)));
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
