//! What the `≡` menu's Settings ▸ lists and writes: the attend mod's
//! `attend_mod.listening` and `attend_mod.keepwarm`, ways keys a project
//! can set, then the chat's own `attend.chat.*` keys, which only the user
//! file holds ([`crate::settings`]). Each shows the value in effect and the
//! layer it comes from. A write goes to the file of the place chosen, the
//! user's or a project's, through the agent-settings writer, as
//! `ways settings set <key> <value> [--project <dir>]` writes it: the same
//! file, header and edit.

use std::path::{Path, PathBuf};

use agent_settings::load::{resolve, Layer};
use agent_settings::schema::{KeySpec, Kind, LayerScope, Scope};
use agent_settings::writer::edit_file;

use crate::settings::plain;

/// The attend mod's keys the menu lists, in its order.
pub const MOD_KEYS: &[&str] = &["attend_mod.listening", "attend_mod.keepwarm"];

const CHAT: &str = "attend.chat.";

/// Where a setting is written: the user's file, or a project's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    User,
    /// A project, by its root.
    Project(PathBuf),
}

impl Place {
    /// How the layer picker lists it, with what its file holds:
    /// `user: active`, `project ~/work/x: not set`.
    pub fn label(&self, held: Option<&str>) -> String {
        let held = held.unwrap_or("not set");
        match self {
            Place::User => format!("user: {held}"),
            Place::Project(root) => format!("project {}: {held}", tilde(root)),
        }
    }

    /// The place a [`Place::label`] names.
    pub fn from_label(label: &str) -> Option<Place> {
        if label.starts_with("user: ") {
            return Some(Place::User);
        }
        let (path, _) = label.strip_prefix("project ")?.rsplit_once(": ")?;
        Some(Place::Project(untilde(path)))
    }

    /// The place as a menu id carries it: `user`, or the project's root.
    pub fn id(&self) -> String {
        match self {
            Place::User => "user".into(),
            Place::Project(root) => root.display().to_string(),
        }
    }

    /// The place a [`Place::id`] names.
    pub fn from_id(id: &str) -> Place {
        if id == "user" {
            Place::User
        } else {
            Place::Project(PathBuf::from(id))
        }
    }

    /// The place in a picker's title: `user`, or `project` and the
    /// project's directory name.
    pub fn name(&self) -> String {
        match self {
            Place::User => "user".into(),
            Place::Project(root) => format!("project {}", root.file_name().map_or_else(|| tilde(root), |n| n.to_string_lossy().to_string())),
        }
    }
}

/// A settings file as the bottom bar names it: a project's overlay by its
/// directory's name (`alpha/.claude/ways.yaml`), any other with `~`.
fn short(file: &Path) -> String {
    let root = file.parent().filter(|d| d.ends_with(".claude")).and_then(Path::parent);
    match root.and_then(Path::file_name) {
        Some(dir) => format!("{}/.claude/{}", dir.to_string_lossy(), file.file_name().unwrap_or_default().to_string_lossy()),
        None => tilde(file),
    }
}

/// `path` with the home directory as `~`.
pub fn tilde(path: &Path) -> String {
    let home = claude_sessions::home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn untilde(text: &str) -> PathBuf {
    match text.strip_prefix("~/") {
        Some(rest) => claude_sessions::home_dir().join(rest),
        None if text == "~" => claude_sessions::home_dir(),
        None => PathBuf::from(text),
    }
}

/// A setting as the menu lists it: its name, the value in effect and the
/// layer that value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub name: String,
    pub value: String,
    pub from: String,
}

impl Setting {
    /// The menu's row: `name = value (layer)`.
    pub fn line(&self) -> String {
        format!("{} = {} ({})", self.name, self.value, self.from)
    }
}

/// The setting a row ([`Setting::line`]) names.
pub fn name_of(line: &str) -> &str {
    line.split(" = ").next().unwrap_or(line)
}

/// A key the menu lists, with the schema that owns it.
enum Key {
    /// A ways key: the user file, a target's, a project's `.claude/ways.yaml`.
    Ways(&'static KeySpec),
    /// One of the chat's own keys, in attend's user file alone.
    Chat(&'static KeySpec),
}

impl Key {
    fn of(name: &str) -> Option<Key> {
        if MOD_KEYS.contains(&name) {
            return ways_core::settings::SCHEMA.keys.iter().find(|k| k.name == name).map(Key::Ways);
        }
        let short = name.strip_prefix(CHAT)?;
        if !crate::settings::KEYS.contains(&short) {
            return None;
        }
        attend_config::SCHEMA.keys.iter().find(|k| k.name == name).map(Key::Chat)
    }

    fn spec(&self) -> &'static KeySpec {
        match self {
            Key::Ways(s) | Key::Chat(s) => s,
        }
    }

    fn path(&self) -> Vec<String> {
        self.spec().path.iter().map(|s| s.to_string()).collect()
    }

    /// The layers the key resolves through at `place`: a project's reads
    /// every layer up to its own file, the user's every layer but a
    /// project's.
    fn layers(&self, place: &Place) -> Vec<Layer> {
        match (self, place) {
            (Key::Ways(_), Place::Project(root)) => ways_core::settings::layers(root),
            (Key::Ways(_), Place::User) => {
                let mut out = ways_core::settings::layers(Path::new(""));
                out.retain(|l| l.scope != LayerScope::Project);
                out
            }
            (Key::Chat(_), _) => vec![attend_config::user_layer()],
        }
    }

    /// The file `place` writes the key to, and the text a new one starts
    /// with: what `ways settings set` writes, `--project` for a project.
    fn file(&self, place: &Place) -> Result<(PathBuf, Option<&'static str>), String> {
        let name = self.spec().name;
        match (self, place) {
            (Key::Ways(s), Place::Project(_)) if matches!(s.scope, Scope::User | Scope::Machine) => {
                Err(format!("{name} is set in the user file, never a project's"))
            }
            (Key::Ways(_), Place::Project(root)) => Ok((ways_core::settings::project_file(root), Some(ways_core::settings::PROJECT_HEADER))),
            (Key::Ways(_), Place::User) => Ok((ways_core::paths::user_config(), None)),
            (Key::Chat(_), Place::Project(_)) => Err(format!("{name} is set in the user file only")),
            (Key::Chat(_), Place::User) => Ok((attend_config::user_path(), Some(attend_config::HEADER))),
        }
    }

    /// The value in effect at `place`, and the layer it comes from: the
    /// layer's name, or `default`.
    fn effect(&self, place: &Place) -> (String, String) {
        let layers = self.layers(place);
        let r = resolve(self.spec(), &[], &layers);
        let from = r.layer.map_or("default".to_string(), |i| layers[i].name.clone());
        (r.value.as_ref().map(plain).unwrap_or_default(), from)
    }
}

/// The place the chat reads from at `dir`: the project enclosing it, else
/// the user's.
fn here(dir: &Path) -> Place {
    ways_core::settings::project_root(dir).map_or(Place::User, Place::Project)
}

/// Every setting the menu lists, as read from `dir`.
pub fn settings(dir: &Path) -> Vec<Setting> {
    let at = here(dir);
    let names = MOD_KEYS.iter().map(|k| k.to_string()).chain(crate::settings::KEYS.iter().map(|k| format!("{CHAT}{k}")));
    names
        .filter_map(|name| {
            let (value, from) = Key::of(&name)?.effect(&at);
            Some(Setting { name, value, from })
        })
        .collect()
}

/// Whether a project's file can hold `name`.
pub fn per_project(name: &str) -> bool {
    matches!(Key::of(name), Some(Key::Ways(s)) if matches!(s.scope, Scope::Both | Scope::Project))
}

/// The values `name` takes.
pub fn choices(name: &str) -> Vec<String> {
    match Key::of(name).map(|k| k.spec().kind) {
        Some(Kind::Bool) => vec!["true".into(), "false".into()],
        Some(Kind::Choice(c)) => c.iter().map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// The value `place`'s own file holds for `name`, if it holds one.
pub fn held(name: &str, place: &Place) -> Option<String> {
    let key = Key::of(name)?;
    let (file, _) = key.file(place).ok()?;
    let layer = match key {
        Key::Ways(_) => {
            let scope = if matches!(place, Place::User) { LayerScope::User } else { LayerScope::Project };
            Layer::read(&ways_core::settings::SCHEMA, "file", ways_core::settings::FILE, scope, &file)
        }
        Key::Chat(_) => attend_config::user_layer(),
    };
    layer.get(&key.path()).map(plain)
}

/// The value of `name` in effect at `place`: a project's reads every layer
/// up to its own file; the user's every layer but a project's.
pub fn in_effect(name: &str, place: &Place) -> String {
    Key::of(name).map(|k| k.effect(place).0).unwrap_or_default()
}

/// The projects the layer picker offers: the one enclosing `dir`, then the
/// one enclosing each of `roots` (the live agents' roots), each once.
pub fn projects(dir: &Path, roots: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let starts = std::iter::once(dir.to_path_buf()).chain(roots.iter().map(PathBuf::from));
    for root in starts.filter_map(|d| ways_core::settings::project_root(&d)) {
        if !out.contains(&root) {
            out.push(root);
        }
    }
    out
}

/// Set `name` to `raw` in `place`'s file. Says what was written, and when
/// another layer still decides the value read from `dir`, which one.
pub fn set_at(name: &str, raw: &str, place: &Place, dir: &Path) -> Result<String, String> {
    let key = Key::of(name).ok_or_else(|| format!("{name} is not a setting the chat lists"))?;
    let (file, header) = key.file(place)?;
    if let Key::Chat(_) = key {
        let short_name = &name[CHAT.len()..];
        return crate::settings::set(name, raw, &file, dir).map(|said| format!("{} · {}", said.replacen(short_name, name, 1), tilde(&file)));
    }
    let spec = key.spec();
    let v = spec.parse_cli(raw, &key.layers(place), &[]).map_err(|m| format!("{name}: {m}"))?;
    let kp = key.path();
    let (_, changed) = edit_file(&file, header, |d| d.set(&kp, &v)).map_err(|e| format!("{e}; nothing written"))?;
    let set = if changed { "" } else { "already " };
    let mut said = format!("{set}{name} = {} · {}", plain(&v), short(&file));
    // What the chat's directory reads: the place written, or for the user
    // file the project there, which may still set the key.
    let read_at = if matches!(place, Place::User) { here(dir) } else { place.clone() };
    let layers = key.layers(&read_at);
    let r = resolve(spec, &[], &layers);
    if let Some(from) = r.layer.and_then(|i| layers[i].path.clone()).filter(|p| *p != file) {
        said.push_str(&format!(" · {} still sets {}", short(&from), r.value.as_ref().map(plain).unwrap_or_default()));
    }
    Ok(said)
}

/// Remove `name` from `place`'s file. Says what is in effect after.
pub fn unset_at(name: &str, place: &Place) -> Result<String, String> {
    let key = Key::of(name).ok_or_else(|| format!("{name} is not a setting the chat lists"))?;
    let (file, _) = key.file(place)?;
    let kp = key.path();
    edit_file(&file, None, |d| d.unset(&kp)).map_err(|e| format!("{e}; nothing written"))?;
    let (value, from) = key.effect(place);
    Ok(format!("{name} unset in {}; {value} ({from}) in effect", short(&file)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A fresh home for one test, under the environment lock the caller
    /// holds: `HOME` and `XDG_CONFIG_HOME` in a temp directory, and a
    /// project in it, a git checkout at `home/work/proj`.
    fn home() -> (PathBuf, PathBuf) {
        let root = crate::test_dir::unique("settings-edit");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        let proj = root.join("home/work/proj");
        fs::create_dir_all(proj.join(".git")).unwrap();
        fs::create_dir_all(proj.join("src")).unwrap();
        (root, proj)
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn the_list_names_each_setting_with_its_value_and_the_layer_it_comes_from() {
        let _env = crate::test_dir::env_lock();
        let (root, proj) = home();
        write(&ways_core::paths::user_config(), "attend_mod:\n  keepwarm: off\n");
        write(&proj.join(".claude/ways.yaml"), "attend_mod:\n  listening: active\n");
        let lines: Vec<String> = settings(&proj.join("src")).iter().map(Setting::line).collect();
        assert_eq!(
            lines,
            [
                "attend_mod.listening = active (project)",
                "attend_mod.keepwarm = off (user)",
                "attend.chat.tabs.jump = auto (default)",
                "attend.chat.tabs.menu_on_repeat = true (default)",
                "attend.chat.tabs.focus_key = both (default)",
                "attend.chat.mouse = true (default)",
                "attend.chat.sidebar_key = f3 (default)",
            ]
        );
        assert_eq!(name_of(&lines[0]), "attend_mod.listening");
        // Outside any project no project's file is read.
        let outside = settings(&root);
        assert_eq!(outside[0].line(), "attend_mod.listening = dormant (default)");
    }

    #[test]
    fn a_write_lands_in_the_file_of_the_place_chosen() {
        let _env = crate::test_dir::env_lock();
        let (_, proj) = home();
        let here = Place::Project(proj.clone());
        let user = ways_core::paths::user_config();
        let said = set_at("attend_mod.listening", "active", &here, &proj).unwrap();
        let file = proj.join(".claude/ways.yaml");
        assert_eq!(read(&file), format!("{}attend_mod:\n  listening: active\n", ways_core::settings::PROJECT_HEADER));
        assert!(said.contains("attend_mod.listening = active") && said.contains(".claude/ways.yaml"), "{said}");
        assert!(!user.exists(), "the user file is not touched");
        assert_eq!(held("attend_mod.listening", &here).as_deref(), Some("active"));
        assert_eq!(held("attend_mod.listening", &Place::User), None);

        set_at("attend_mod.keepwarm", "off", &Place::User, &proj).unwrap();
        assert_eq!(read(&user), "attend_mod:\n  keepwarm: off\n");
        assert_eq!(in_effect("attend_mod.keepwarm", &here), "off", "the project reads the user's value");

        // The project's file still decides what is read there.
        let said = set_at("attend_mod.listening", "dormant", &Place::User, &proj).unwrap();
        assert!(said.contains("ways.yaml") && said.contains("active"), "names what still sets it: {said}");
        assert_eq!(in_effect("attend_mod.listening", &here), "active");

        let said = unset_at("attend_mod.listening", &here).unwrap();
        assert!(!read(&file).contains("listening"), "{}", read(&file));
        assert_eq!(in_effect("attend_mod.listening", &here), "dormant");
        assert!(said.contains("dormant"), "{said}");
    }

    #[test]
    fn the_chat_keys_are_the_user_files_alone_and_a_bad_value_writes_nothing() {
        let _env = crate::test_dir::env_lock();
        let (_, proj) = home();
        let said = set_at("attend.chat.mouse", "false", &Place::User, &proj).unwrap();
        assert!(said.contains("mouse = false"), "{said}");
        assert!(read(&attend_config::user_path()).contains("mouse: false"), "{}", read(&attend_config::user_path()));
        assert_eq!(in_effect("attend.chat.mouse", &Place::User), "false");

        let refused = set_at("attend.chat.mouse", "true", &Place::Project(proj.clone()), &proj).unwrap_err();
        assert!(refused.contains("user file only"), "{refused}");
        assert!(!proj.join(".claude").exists(), "nothing written in the project");

        let refused = set_at("attend_mod.listening", "loud", &Place::User, &proj).unwrap_err();
        assert!(refused.contains("loud"), "{refused}");
        assert!(!ways_core::paths::user_config().exists(), "nothing written");

        assert_eq!(choices("attend_mod.listening"), ["active", "dormant"]);
        assert_eq!(choices("attend.chat.mouse"), ["true", "false"]);
        assert!(per_project("attend_mod.keepwarm") && !per_project("attend.chat.mouse"));
    }

    #[test]
    fn a_place_reads_back_from_its_label_and_its_id() {
        let _env = crate::test_dir::env_lock();
        let (_, proj) = home();
        let here = Place::Project(proj.clone());
        assert_eq!(here.label(Some("active")), "project ~/work/proj: active");
        assert_eq!(Place::User.label(None), "user: not set");
        for p in [Place::User, here] {
            assert_eq!(Place::from_label(&p.label(Some("on"))), Some(p.clone()), "{p:?}");
            assert_eq!(Place::from_id(&p.id()), p);
        }
        assert_eq!(Place::from_label("Another project…"), None);
    }

    #[test]
    fn the_projects_offered_are_the_chats_then_each_live_agents_once() {
        let _env = crate::test_dir::env_lock();
        let (root, proj) = home();
        let other = root.join("home/work/other");
        write(&other.join(".claude/ways.yaml"), "");
        let roots = [other.join("deep").display().to_string(), proj.display().to_string(), root.join("home/loose").display().to_string()];
        fs::create_dir_all(other.join("deep")).unwrap();
        assert_eq!(projects(&proj.join("src"), &roots), [proj.clone(), other]);
    }
}
