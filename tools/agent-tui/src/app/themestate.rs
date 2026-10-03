//! The theme tab's state: the themes on offer and the active choice, with
//! the file operations that write theme files, and the slot editor (ADR-504
//! §7). Theme files are written at once into the user theme directory and
//! never go through the settings review; the active choice is kept by the
//! adapter, which for ways is a settings key.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::theme::Palette;
use crate::named::{self, Done, ItemAct, NamedItems};
use agent_theme::{to_text, Background, ColorDepth, Kind, Rgb, Roles, Slots, Source, Theme, ThemeSet, ANSI16_RGB};

/// The default theme: the terminal's own 16 colours, chosen when nothing
/// else is and used whole on a terminal of fewer than 256 colours.
pub const DEFAULT: &str = agent_theme::TERMINAL;

/// The theme New starts from: the agent-ways identity palette.
pub const NEW_FROM: &str = "agent-ways";

/// The editor's rows: the ten slots, then kind and background.
pub const ROWS: usize = Slots::NAMES.len() + 2;

/// Each slider's name, top, coarse step and unit.
pub const CHANNELS: [(&str, f64, f64, &str); 3] = [("H", 360.0, 15.0, "°"), ("S", 100.0, 10.0, "%"), ("L", 100.0, 10.0, "%")];

/// The terminal palette as a theme, so it lists, previews and copies like
/// one: its slots are the xterm defaults of the ANSI colours it stands for.
pub fn terminal_theme() -> Theme {
    let a = |n: usize| ANSI16_RGB[n];
    Theme {
        name: DEFAULT.into(),
        label: "Terminal (the terminal's own 16 colours)".into(),
        kind: Kind::Dark,
        background: Background::Terminal,
        slots: Slots { bg: a(0), fg: a(7), dim: a(8), subtle: a(0), accent: a(6), info: a(4), ok: a(2), warn: a(3), err: a(1), alt: a(5) },
        overrides: Default::default(),
    }
}

pub struct Themes {
    /// Where theme files are written; none means nothing saves.
    pub dir: Option<PathBuf>,
    /// The home directory, shown as `~` in the paths the tab names.
    pub home: Option<PathBuf>,
    pub set: ThemeSet,
    terminal: Theme,
    pub active: String,
    pub depth: ColorDepth,
    /// The list cursor; the theme under it is previewed while the tab shows.
    pub cursor: usize,
    pub editor: Option<Editor>,
    /// Derived palettes, most recent first: deriving settles the status
    /// roles by search, too slow to repeat every frame.
    cache: RefCell<Vec<(Theme, Palette)>>,
    /// Derived roles by theme, for the checks, whatever the depth.
    roles: RefCell<Vec<(Theme, Roles)>>,
}

impl Themes {
    /// The bundled themes and those in `dir`, with `active` the choice the
    /// adapter keeps (the default when absent or unknown).
    pub fn new(dir: Option<PathBuf>, depth: ColorDepth, active: Option<String>) -> Themes {
        let set = ThemeSet::load(dir.as_deref());
        let mut t = Themes { dir, home: None, set, terminal: terminal_theme(), active: DEFAULT.into(), depth, cursor: 0, editor: None, cache: RefCell::default(), roles: RefCell::default() };
        if let Some(a) = active.filter(|a| t.get(a).is_some()) {
            t.active = a;
        }
        t.cursor = t.index_of(&t.active.clone()).unwrap_or(0);
        t
    }

    /// Show paths under `home` with `~`.
    pub fn home(mut self, home: impl Into<PathBuf>) -> Themes {
        self.home = Some(home.into());
        self
    }

    /// A path as the tab shows it.
    pub fn show(&self, p: &Path) -> String {
        let s = match self.home.as_deref().map(|h| (h, p.strip_prefix(h))) {
            Some((h, Ok(r))) if !h.as_os_str().is_empty() => format!("~/{}", r.display()),
            _ => p.display().to_string(),
        };
        s.replace('\\', "/")
    }

    /// The default first, then the bundled and user themes.
    pub fn list(&self) -> Vec<(&Theme, Source)> {
        std::iter::once((&self.terminal, Source::Bundled)).chain(self.set.list()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&Theme> {
        if name == DEFAULT {
            return Some(&self.terminal);
        }
        self.set.get(name)
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.list().iter().position(|(t, _)| t.name == name)
    }

    pub fn under_cursor(&self) -> (&Theme, Source) {
        let list = self.list();
        list.get(self.cursor).copied().unwrap_or(list[0])
    }

    pub fn active_theme(&self) -> &Theme {
        self.get(&self.active).unwrap_or(&self.terminal)
    }

    /// The file a user theme is in, or would be written to.
    /// A loaded user theme's file is the one it was read from, whatever its
    /// stem; any other name's is the file with its stem.
    pub fn path_of(&self, name: &str) -> Option<PathBuf> {
        if let Some(f) = self.set.file(name) {
            return Some(f.to_path_buf());
        }
        let dir = self.dir.as_ref()?;
        Some(self.stem_file(name).unwrap_or_else(|| dir.join(format!("{name}.theme"))))
    }

    /// The file in the themes directory with stem `name`, if there is one.
    fn stem_file(&self, name: &str) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        agent_theme::EXTENSIONS.iter().map(|x| dir.join(format!("{name}.{x}"))).find(|p| p.is_file())
    }

    pub fn palette(&self, t: &Theme) -> Palette {
        if t.name == DEFAULT && *t == self.terminal {
            return Palette::terminal(self.depth);
        }
        let mut cache = self.cache.borrow_mut();
        if let Some(i) = cache.iter().position(|(c, p)| c == t && p.depth() == self.depth) {
            let hit = cache.remove(i);
            let p = hit.1;
            cache.insert(0, hit);
            return p;
        }
        let p = Palette::new(Some(t), self.depth);
        cache.insert(0, (t.clone(), p));
        cache.truncate(16);
        p
    }

    /// The roles `t` derives, as a truecolor terminal would draw them: what
    /// the contrast and distinctness checks judge.
    pub fn roles(&self, t: &Theme) -> Roles {
        let mut cache = self.roles.borrow_mut();
        if let Some((_, r)) = cache.iter().find(|(c, _)| c == t) {
            return *r;
        }
        let r = Roles::derive(t);
        cache.insert(0, (t.clone(), r));
        cache.truncate(16);
        r
    }

    /// A name a new theme may take: the shared rules of
    /// [`named::check_name`], `[a-z0-9-]+`, and not already a theme's name.
    pub fn check_name(&self, name: &str) -> Result<(), String> {
        named::check_name(self, name).map_err(|r| r.message)
    }

    fn dir(&self) -> Result<&Path, String> {
        self.dir.as_deref().ok_or_else(|| "no themes directory; nothing is saved".to_string())
    }

    fn reload(&mut self) {
        self.set = ThemeSet::load(self.dir.as_deref());
        self.cursor = self.cursor.min(self.list().len() - 1);
        self.cache.borrow_mut().clear();
        self.roles.borrow_mut().clear();
    }

    pub fn focus(&mut self, name: &str) {
        if let Some(i) = self.index_of(name) {
            self.cursor = i;
        }
    }

    /// Write `t` to its file, through a temporary file renamed into place,
    /// and reload.
    pub fn save(&mut self, t: &Theme) -> Result<PathBuf, String> {
        let dir = self.dir()?.to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = self.path_of(&t.name).expect("a directory is set");
        let tmp = dir.join(format!(".{}.{}.tmp", t.name, std::process::id()));
        std::fs::write(&tmp, to_text(t)).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })?;
        self.reload();
        Ok(path)
    }

    /// A new user theme: `from` under `name`, written at once.
    pub fn create(&mut self, from: &Theme, name: &str) -> Result<String, String> {
        self.check_name(name)?;
        let path = self.write_as(from, name)?;
        Ok(format!("wrote {}", self.show(&path)))
    }

    /// `from` written as user theme `name`, labelled with its name, and focused.
    fn write_as(&mut self, from: &Theme, name: &str) -> Result<PathBuf, String> {
        let path = self.save(&Theme { name: name.into(), label: name.into(), ..from.clone() })?;
        self.focus(name);
        Ok(path)
    }

    /// Where the active choice moves after `done`: to the new name when the
    /// active theme was renamed, to the default when it was deleted, unless
    /// a bundled theme of its name is left in its place.
    pub fn follow(&self, done: &Done) -> Option<String> {
        match done {
            Done::Renamed { from, to, .. } if self.active == *from => Some(to.clone()),
            Done::Deleted { name, .. } if self.active == *name && self.get(name).is_none() => Some(DEFAULT.into()),
            _ => None,
        }
    }

    /// After a copy, rename or delete: move the active choice where it
    /// follows, kept first by `keep` (the adapter's write of the setting),
    /// focus the theme the operation left, and say what happened. The
    /// screens and `ways settings theme` both end here.
    pub fn settle(&mut self, done: &Done, keep: impl FnOnce(&str) -> Result<(), String>) -> String {
        let mut msg = match done {
            Done::Copied { from, to, .. } => format!("copied {from} to {to}"),
            Done::Renamed { from, to, .. } => format!("renamed {from} to {to}"),
            Done::Deleted { name, .. } => format!("deleted {name}"),
        };
        if let Some(next) = self.follow(done) {
            match keep(&next) {
                Ok(()) => {
                    msg += &match done {
                        Done::Renamed { .. } => "; the active choice follows".to_string(),
                        _ => format!("; it was the active theme, so {next} is active now"),
                    };
                    self.active = next;
                }
                Err(e) => msg += &format!("; the active choice was not kept: {e}"),
            }
        }
        let at = match done {
            Done::Copied { to, .. } | Done::Renamed { to, .. } => to.clone(),
            Done::Deleted { .. } => self.active.clone(),
        };
        self.focus(&at);
        msg
    }
}

/// Themes as named items: the bundled ones, the default among them, are
/// copied, never renamed or deleted; a user file, an override of a bundled
/// name included, is the user's.
impl NamedItems for Themes {
    fn noun(&self) -> &'static str {
        "theme"
    }

    fn exists(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    fn bundled(&self, name: &str) -> bool {
        self.list().iter().any(|(t, s)| t.name == name && *s == Source::Bundled)
    }

    fn name_rule(&self, name: &str) -> Result<(), String> {
        if !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err(format!("`{name}`: a theme name is lowercase letters, digits and -"));
        }
        Ok(())
    }

    fn copy_item(&mut self, from: &str, to: &str) -> Result<PathBuf, String> {
        let base = self.get(from).ok_or("no such theme")?.clone();
        self.write_as(&base, to)
    }

    /// A name is occupied by a theme, or by a file with its stem that holds
    /// no theme of that name: one that does not parse, or names another.
    fn occupied(&self, name: &str) -> bool {
        self.exists(name) || self.stem_file(name).is_some()
    }

    /// The theme written under its new name, then the file it was read
    /// from removed; all or nothing: that file must be there before
    /// anything is written, and the new file is taken back when it cannot
    /// be removed. A label that was the old name becomes the new one.
    fn rename_item(&mut self, from: &str, to: &str) -> Result<PathBuf, String> {
        let old = self.set.get(from).ok_or("no such theme")?.clone();
        let gone = self.set.file(from).ok_or("not a user theme")?.to_path_buf();
        if !gone.is_file() {
            return Err(format!("{} is gone; nothing was written", self.show(&gone)));
        }
        let label = if old.label == old.name { to.to_string() } else { old.label.clone() };
        let path = self.save(&Theme { name: to.into(), label, ..old })?;
        if let Err(e) = std::fs::remove_file(&gone) {
            let _ = std::fs::remove_file(&path);
            self.reload();
            return Err(format!("{}: {e}; the rename was taken back", self.show(&gone)));
        }
        self.reload();
        Ok(path)
    }

    /// The file the theme was read from, whatever its stem.
    fn delete_item(&mut self, name: &str) -> Result<PathBuf, String> {
        let path = self.set.file(name).ok_or("not a user theme")?.to_path_buf();
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", self.show(&path)))?;
        self.reload();
        Ok(path)
    }
}

/// Where the editor's keys go: the slot rows, or one slot's sliders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Rows,
    Slider(usize),
}

pub struct Editor {
    /// The theme as edited; its name is the file it saves to.
    pub theme: Theme,
    /// What the file holds, or for a copy not yet written, what it started from.
    pub saved: Theme,
    /// Whether the theme's file exists.
    pub written: bool,
    /// For a copy of a bundled theme, the theme it copies.
    pub origin: Option<String>,
    pub row: usize,
    pub focus: Focus,
    /// Hex entry in progress.
    pub hex: Option<String>,
    /// The HSL of the slot being slid, kept so steps do not drift through
    /// RGB rounding: (row, h s l).
    hsl: Option<(usize, [f64; 3])>,
}

impl Editor {
    pub fn new(theme: Theme, written: bool) -> Editor {
        Editor { saved: theme.clone(), theme, written, origin: None, row: 0, focus: Focus::Rows, hex: None, hsl: None }
    }

    pub fn dirty(&self) -> bool {
        !self.written || self.theme != self.saved
    }

    /// The slot on the current row, when it is one.
    pub fn slot(&self) -> Option<&'static str> {
        Slots::NAMES.get(self.row).copied()
    }

    pub fn color(&self) -> Option<Rgb> {
        self.slot().and_then(|s| self.theme.slots.get(s))
    }

    pub fn hsl(&self) -> [f64; 3] {
        match (self.hsl, self.color()) {
            (Some((r, v)), _) if r == self.row => v,
            (_, Some(c)) => c.to_hsl(),
            _ => [0.0; 3],
        }
    }

    pub fn set_channel(&mut self, ch: usize, v: f64) {
        let Some(slot) = self.slot() else { return };
        let mut hsl = self.hsl();
        let top = CHANNELS[ch].1;
        hsl[ch] = if ch == 0 { v.rem_euclid(top) } else { v.clamp(0.0, top) };
        self.hsl = Some((self.row, hsl));
        self.theme.slots.set(slot, Rgb::from_hsl(hsl));
    }

    pub fn step(&mut self, ch: usize, by: f64) {
        let v = self.hsl()[ch];
        self.set_channel(ch, (v + by).round());
    }

    pub fn set_hex(&mut self, text: &str) -> Result<Rgb, String> {
        let slot = self.slot().ok_or("not a colour row")?;
        let t = text.trim();
        let c = Rgb::from_hex(&if t.starts_with('#') { t.to_string() } else { format!("#{t}") }).ok_or_else(|| format!("`{t}` is not #rrggbb"))?;
        self.theme.slots.set(slot, c);
        self.hsl = None;
        Ok(c)
    }

    pub fn move_row(&mut self, to: usize) {
        self.row = to.min(ROWS - 1);
        self.hsl = None;
        if self.slot().is_none() {
            self.focus = Focus::Rows;
        }
    }

    /// Flip kind or background on their rows.
    pub fn toggle(&mut self) {
        match self.row {
            10 => self.theme.kind = if self.theme.kind == Kind::Dark { Kind::Light } else { Kind::Dark },
            11 => self.theme.background = if self.theme.background == Background::Fill { Background::Terminal } else { Background::Fill },
            _ => {}
        }
    }
}

/// What a typed name is for, past the named-item actions.
#[derive(Debug, Clone, PartialEq)]
pub enum NameOp {
    New,
    /// Editing a bundled theme: the copy's name.
    EditCopy(String),
}

impl NameOp {
    pub fn prompt(&self) -> String {
        match self {
            NameOp::New => "new theme name".into(),
            NameOp::EditCopy(f) => format!("{f} is bundled; edit a copy named"),
        }
    }
}

/// The theme menu's entries: copy, rename and delete are the named-item
/// actions; the rest are the theme tab's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeAct {
    New,
    Item(ItemAct),
    Edit,
    /// The lozenge shape, a setting of its own, cycled.
    Shape,
}

impl ThemeAct {
    pub fn label(self) -> &'static str {
        match self {
            ThemeAct::New => "new",
            ThemeAct::Item(a) => a.label(),
            ThemeAct::Edit => "edit",
            ThemeAct::Shape => "shape",
        }
    }
}
