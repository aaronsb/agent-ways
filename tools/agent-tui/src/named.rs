//! Managing named items: copy, rename, and delete unless bundled (#851).
//!
//! A kind of item that lives as named user files beside items the
//! application ships (themes, and later sensors) implements [`NamedItems`].
//! The checks every kind shares live here, once: a new name is checked
//! against the shared rule and the kind's own, and refused when it is
//! taken; a bundled item is copied but never renamed or deleted. The
//! screens and the command line both call [`copy`], [`rename`] and
//! [`delete`], so the two ways in refuse and report alike (ADR-503).
//!
//! What follows an operation, such as a setting that named the renamed
//! item, is the caller's: the [`Done`] it gets back says what happened.

use std::fmt;
use std::path::PathBuf;

/// A kind of named item the screens and the command line copy, rename and
/// delete.
pub trait NamedItems {
    /// What one item is called in messages, such as `theme`.
    fn noun(&self) -> &'static str;

    /// Whether an item of this name exists, bundled or the user's.
    fn exists(&self, name: &str) -> bool;

    /// Whether `name` is an item the application ships. Copied, never
    /// renamed or deleted.
    fn bundled(&self, name: &str) -> bool;

    /// The kind's own rule for a name, past the shared one [`check_name`]
    /// applies first. The message names the rule.
    fn name_rule(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }

    /// Write `from` as a new user item `to`; the file written.
    fn copy_item(&mut self, from: &str, to: &str) -> Result<PathBuf, String>;

    /// Move user item `from` to `to`, its file and its own name; the file
    /// written.
    fn rename_item(&mut self, from: &str, to: &str) -> Result<PathBuf, String>;

    /// Remove user item `name`; the file removed.
    fn delete_item(&mut self, name: &str) -> Result<PathBuf, String>;
}

/// The actions on one item, as a menu offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemAct {
    Copy,
    Rename,
    Delete,
}

impl ItemAct {
    pub fn label(self) -> &'static str {
        match self {
            ItemAct::Copy => "copy",
            ItemAct::Rename => "rename",
            ItemAct::Delete => "delete",
        }
    }
}

/// An action that takes a new name, with the item it starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemOp {
    Copy(String),
    Rename(String),
}

impl ItemOp {
    /// The name prompt's text.
    pub fn prompt(&self) -> String {
        match self {
            ItemOp::Copy(f) => format!("copy {f} as"),
            ItemOp::Rename(f) => format!("rename {f} to"),
        }
    }
}

/// Why an operation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No item of that name.
    Missing,
    /// A bundled item, which is never renamed or deleted.
    Bundled,
    /// A new name the rules refuse.
    Name,
    /// A new name already in use.
    Taken,
    /// The file could not be written or removed.
    Write,
}

impl Refusal {
    /// The word `--json` reports.
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Missing => "missing",
            Refusal::Bundled => "bundled",
            Refusal::Name => "name",
            Refusal::Taken => "taken",
            Refusal::Write => "write",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub why: Refusal,
    pub message: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

fn refuse(why: Refusal, message: impl Into<String>) -> Refused {
    Refused { why, message: message.into() }
}

/// What an operation did, with the file it wrote or removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Copied { from: String, to: String, file: PathBuf },
    Renamed { from: String, to: String, file: PathBuf },
    Deleted { name: String, file: PathBuf },
}

impl Done {
    pub fn file(&self) -> &PathBuf {
        match self {
            Done::Copied { file, .. } | Done::Renamed { file, .. } | Done::Deleted { file, .. } => file,
        }
    }
}

/// The actions an item offers: copy always; rename and delete only for a
/// user item.
pub fn acts(items: &dyn NamedItems, name: &str) -> Vec<ItemAct> {
    if items.bundled(name) {
        vec![ItemAct::Copy]
    } else {
        vec![ItemAct::Copy, ItemAct::Rename, ItemAct::Delete]
    }
}

/// A new name: not empty, no path separator, no leading dot, no space or
/// control character, the kind's own rule, and not the name of a bundled
/// or user item.
pub fn check_name(items: &dyn NamedItems, name: &str) -> Result<(), Refused> {
    let noun = items.noun();
    if name.is_empty() {
        return Err(refuse(Refusal::Name, format!("a {noun} name cannot be empty")));
    }
    if name.contains(['/', '\\']) {
        return Err(refuse(Refusal::Name, format!("`{name}`: a {noun} name has no path separator")));
    }
    if name.starts_with('.') {
        return Err(refuse(Refusal::Name, format!("`{name}`: a {noun} name does not start with a dot")));
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(refuse(Refusal::Name, format!("`{name}`: a {noun} name has no spaces")));
    }
    items.name_rule(name).map_err(|m| refuse(Refusal::Name, m))?;
    if items.bundled(name) {
        return Err(refuse(Refusal::Taken, format!("`{name}` is taken by a bundled {noun}")));
    }
    if items.exists(name) {
        return Err(refuse(Refusal::Taken, format!("`{name}` is taken by a user {noun}")));
    }
    Ok(())
}

fn existing(items: &dyn NamedItems, name: &str) -> Result<(), Refused> {
    if items.exists(name) {
        Ok(())
    } else {
        Err(refuse(Refusal::Missing, format!("no {} named `{name}`", items.noun())))
    }
}

fn user_owned(items: &dyn NamedItems, name: &str, verb: &str) -> Result<(), Refused> {
    existing(items, name)?;
    if items.bundled(name) {
        let noun = items.noun();
        return Err(refuse(Refusal::Bundled, format!("{name} is a bundled {noun}, which is never {verb}; copy it to change it")));
    }
    Ok(())
}

/// Copy any item, bundled or the user's, to a new user item `to`.
pub fn copy(items: &mut dyn NamedItems, from: &str, to: &str) -> Result<Done, Refused> {
    existing(items, from)?;
    check_name(items, to)?;
    let file = items.copy_item(from, to).map_err(|m| refuse(Refusal::Write, m))?;
    Ok(Done::Copied { from: from.into(), to: to.into(), file })
}

/// Rename a user item. A bundled one is refused with the reason.
pub fn rename(items: &mut dyn NamedItems, from: &str, to: &str) -> Result<Done, Refused> {
    user_owned(items, from, "renamed")?;
    check_name(items, to)?;
    let file = items.rename_item(from, to).map_err(|m| refuse(Refusal::Write, m))?;
    Ok(Done::Renamed { from: from.into(), to: to.into(), file })
}

/// Delete a user item. A bundled one is refused with the reason.
pub fn delete(items: &mut dyn NamedItems, name: &str) -> Result<Done, Refused> {
    user_owned(items, name, "deleted")?;
    let file = items.delete_item(name).map_err(|m| refuse(Refusal::Write, m))?;
    Ok(Done::Deleted { name: name.into(), file })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Items in memory: `shipped` bundled, the rest the user's.
    struct Mem {
        shipped: Vec<&'static str>,
        user: Vec<String>,
    }

    impl NamedItems for Mem {
        fn noun(&self) -> &'static str {
            "widget"
        }
        fn exists(&self, n: &str) -> bool {
            self.shipped.contains(&n) || self.user.iter().any(|u| u == n)
        }
        fn bundled(&self, n: &str) -> bool {
            self.shipped.contains(&n)
        }
        fn name_rule(&self, n: &str) -> Result<(), String> {
            if n.chars().any(|c| c.is_ascii_uppercase()) {
                return Err(format!("`{n}`: lowercase only"));
            }
            Ok(())
        }
        fn copy_item(&mut self, _: &str, to: &str) -> Result<PathBuf, String> {
            self.user.push(to.into());
            Ok(PathBuf::from(format!("/u/{to}")))
        }
        fn rename_item(&mut self, from: &str, to: &str) -> Result<PathBuf, String> {
            self.user.retain(|u| u != from);
            self.user.push(to.into());
            Ok(PathBuf::from(format!("/u/{to}")))
        }
        fn delete_item(&mut self, n: &str) -> Result<PathBuf, String> {
            self.user.retain(|u| u != n);
            Ok(PathBuf::from(format!("/u/{n}")))
        }
    }

    fn mem() -> Mem {
        Mem { shipped: vec!["base"], user: vec!["mine".into()] }
    }

    #[test]
    fn a_bundled_item_offers_copy_only() {
        let m = mem();
        assert_eq!(acts(&m, "base"), vec![ItemAct::Copy]);
        assert_eq!(acts(&m, "mine"), vec![ItemAct::Copy, ItemAct::Rename, ItemAct::Delete]);
    }

    #[test]
    fn new_names_are_checked_shared_rule_then_kind_then_collision() {
        let m = mem();
        let why = |n: &str| check_name(&m, n).map_err(|r| r.why);
        assert_eq!(why(""), Err(Refusal::Name));
        assert_eq!(why("a/b"), Err(Refusal::Name));
        assert_eq!(why("a\\b"), Err(Refusal::Name));
        assert_eq!(why(".hidden"), Err(Refusal::Name));
        assert_eq!(why("two words"), Err(Refusal::Name));
        assert_eq!(why("Upper"), Err(Refusal::Name));
        assert_eq!(why("base"), Err(Refusal::Taken));
        assert_eq!(why("mine"), Err(Refusal::Taken));
        assert_eq!(why("fresh"), Ok(()));
        assert!(check_name(&m, "base").unwrap_err().message.contains("bundled widget"));
        assert!(check_name(&m, "mine").unwrap_err().message.contains("user widget"));
    }

    #[test]
    fn copy_rename_and_delete_report_what_they_did() {
        let mut m = mem();
        assert_eq!(copy(&mut m, "base", "two").unwrap(), Done::Copied { from: "base".into(), to: "two".into(), file: "/u/two".into() });
        assert_eq!(rename(&mut m, "two", "three").unwrap(), Done::Renamed { from: "two".into(), to: "three".into(), file: "/u/three".into() });
        assert_eq!(m.user, vec!["mine".to_string(), "three".to_string()]);
        assert_eq!(delete(&mut m, "three").unwrap(), Done::Deleted { name: "three".into(), file: "/u/three".into() });
        assert_eq!(m.user, vec!["mine".to_string()]);
    }

    #[test]
    fn bundled_missing_and_taken_are_refused_and_change_nothing() {
        let mut m = mem();
        let r = rename(&mut m, "base", "other").unwrap_err();
        assert_eq!(r.why, Refusal::Bundled);
        assert_eq!(r.message, "base is a bundled widget, which is never renamed; copy it to change it");
        assert_eq!(delete(&mut m, "base").unwrap_err().why, Refusal::Bundled);
        assert_eq!(delete(&mut m, "gone").unwrap_err().why, Refusal::Missing);
        assert_eq!(copy(&mut m, "gone", "x").unwrap_err().why, Refusal::Missing);
        assert_eq!(copy(&mut m, "base", "mine").unwrap_err().why, Refusal::Taken);
        assert_eq!(rename(&mut m, "mine", "base").unwrap_err().why, Refusal::Taken);
        assert_eq!((m.shipped, m.user), (vec!["base"], vec!["mine".to_string()]));
    }
}
