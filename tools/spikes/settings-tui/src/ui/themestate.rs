//! The theme tab's state: the themes on offer and the active choice, with
//! the file operations that write them, and the slot editor. Theme files and
//! the active choice are written at once, into one directory chosen at
//! startup; they never go through the settings review.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::theme::Palette;
use crate::themes::{to_toml, Background, ColorDepth, Kind, Rgb, Slots, Source, Theme, ThemeSet};

/// The theme New starts from, and the fallback when the active one goes.
pub const DEFAULT: &str = "agent-ways";

/// The editor's rows: the ten slots, then kind and background.
pub const ROWS: usize = Slots::NAMES.len() + 2;

/// Each slider's name, top, coarse step and unit.
pub const CHANNELS: [(&str, f64, f64, &str); 3] = [("H", 360.0, 15.0, "°"), ("S", 100.0, 10.0, "%"), ("L", 100.0, 10.0, "%")];

pub struct Themes {
    /// Where theme files and `active` are written; none means nothing saves.
    pub dir: Option<PathBuf>,
    pub set: ThemeSet,
    pub active: String,
    pub depth: ColorDepth,
    /// The list cursor; the theme under it is previewed while the tab shows.
    pub cursor: usize,
    pub editor: Option<Editor>,
    /// Derived palettes, most recent first: deriving settles the status
    /// roles by search, too slow to repeat every frame.
    cache: RefCell<Vec<(Theme, Palette)>>,
}

impl Themes {
    /// The bundled themes and those in `dir`, with the active choice read
    /// from `dir/active` (agent-ways when absent or unknown).
    pub fn new(dir: Option<PathBuf>, depth: ColorDepth) -> Themes {
        let set = ThemeSet::load(dir.as_deref());
        let active = dir
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("active")).ok())
            .map(|s| s.trim().to_string())
            .filter(|n| set.get(n).is_some())
            .unwrap_or_else(|| DEFAULT.into());
        let mut t = Themes { dir, set, active, depth, cursor: 0, editor: None, cache: RefCell::default() };
        t.cursor = t.index_of(&t.active.clone()).unwrap_or(0);
        t
    }

    pub fn list(&self) -> Vec<(&Theme, Source)> {
        self.set.list().collect()
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.set.list().position(|(t, _)| t.name == name)
    }

    pub fn under_cursor(&self) -> (&Theme, Source) {
        self.set.list().nth(self.cursor).unwrap_or_else(|| self.set.list().next().expect("bundled themes"))
    }

    pub fn active_theme(&self) -> &Theme {
        self.set.get(&self.active).or_else(|| self.set.get(DEFAULT)).expect("agent-ways is bundled")
    }

    pub fn path_of(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{name}.toml")))
    }

    pub fn palette(&self, t: &Theme) -> Palette {
        let mut cache = self.cache.borrow_mut();
        if let Some(i) = cache.iter().position(|(c, p)| c == t && p.depth == self.depth) {
            let hit = cache.remove(i);
            let p = hit.1.clone();
            cache.insert(0, hit);
            return p;
        }
        let p = Palette::new(t, self.depth);
        cache.insert(0, (t.clone(), p.clone()));
        cache.truncate(16);
        p
    }

    /// `[a-z0-9-]+` and not already a theme's name.
    pub fn check_name(&self, name: &str) -> Result<(), String> {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err(format!("`{name}`: a name is lowercase letters, digits and -"));
        }
        if self.set.get(name).is_some() {
            return Err(format!("`{name}` is taken"));
        }
        Ok(())
    }

    fn dir(&self) -> Result<&Path, String> {
        self.dir.as_deref().ok_or_else(|| "no themes directory; nothing is saved".to_string())
    }

    fn reload(&mut self) {
        self.set = ThemeSet::load(self.dir.as_deref());
        if self.set.get(&self.active).is_none() {
            self.active = DEFAULT.into();
        }
        self.cursor = self.cursor.min(self.set.list().count() - 1);
        self.cache.borrow_mut().clear();
    }

    pub fn focus(&mut self, name: &str) {
        if let Some(i) = self.index_of(name) {
            self.cursor = i;
        }
    }

    fn write_active(&self, name: &str) -> Result<(), String> {
        let path = self.dir()?.join("active");
        std::fs::write(&path, format!("{name}\n")).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Make `name` the active theme and write the choice.
    pub fn choose(&mut self, name: &str) -> Result<String, String> {
        self.write_active(name)?;
        self.active = name.into();
        Ok(format!("{name} is the active theme"))
    }

    /// Write `t` to its file and reload.
    pub fn save(&mut self, t: &Theme) -> Result<PathBuf, String> {
        let path = self.dir()?.join(format!("{}.toml", t.name));
        std::fs::create_dir_all(self.dir()?).map_err(|e| e.to_string())?;
        std::fs::write(&path, to_toml(t)).map_err(|e| format!("{}: {e}", path.display()))?;
        self.reload();
        Ok(path)
    }

    /// A new user theme: `from` under `name`, written at once.
    pub fn create(&mut self, from: &Theme, name: &str) -> Result<String, String> {
        self.check_name(name)?;
        let t = Theme { name: name.into(), label: name.into(), ..from.clone() };
        let path = self.save(&t)?;
        self.focus(name);
        Ok(format!("wrote {}", path.display()))
    }

    /// A user theme's file under a new name. The active choice follows it.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<String, String> {
        self.check_name(to)?;
        let old = self.set.get(from).ok_or("no such theme")?.clone();
        let label = if old.label == old.name { to.to_string() } else { old.label.clone() };
        let path = self.save(&Theme { name: to.into(), label, ..old })?;
        let gone = self.dir()?.join(format!("{from}.toml"));
        std::fs::remove_file(&gone).map_err(|e| format!("{}: {e}", gone.display()))?;
        if self.active == from {
            self.write_active(to)?;
            self.active = to.into();
        }
        self.reload();
        self.focus(to);
        Ok(format!("renamed {from} to {to}: {}", path.display()))
    }

    /// Remove a user theme's file. When it was active, agent-ways takes
    /// over, unless it overrode a bundled theme, which stays active.
    pub fn delete(&mut self, name: &str) -> Result<String, String> {
        let path = self.dir()?.join(format!("{name}.toml"));
        let was_active = self.active == name;
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.reload();
        let mut msg = format!("deleted {}", path.display());
        if was_active && self.set.get(name).is_none() {
            self.write_active(DEFAULT)?;
            msg += "; agent-ways is active";
        }
        self.focus(&self.active.clone());
        Ok(msg)
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
    /// Whether `<name>.toml` exists for this theme.
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

/// What a typed name is for.
#[derive(Debug, Clone, PartialEq)]
pub enum NameOp {
    New,
    Copy(String),
    Rename(String),
    /// Editing a bundled theme: the copy's name.
    EditCopy(String),
}

impl NameOp {
    pub fn prompt(&self) -> String {
        match self {
            NameOp::New => "new theme name".into(),
            NameOp::Copy(f) => format!("copy {f} as"),
            NameOp::Rename(f) => format!("rename {f} to"),
            NameOp::EditCopy(f) => format!("{f} is bundled; edit a copy named"),
        }
    }
}

/// The theme menu's entries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeAct {
    New,
    Copy,
    Rename,
    Delete,
    Edit,
}

impl ThemeAct {
    pub fn label(self) -> &'static str {
        match self {
            ThemeAct::New => "new",
            ThemeAct::Copy => "copy",
            ThemeAct::Rename => "rename",
            ThemeAct::Delete => "delete",
            ThemeAct::Edit => "edit",
        }
    }
}

