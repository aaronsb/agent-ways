//! An item's card as the detail pane draws it (ADR-516 §6): the entry a
//! family's producer builds ([`crate::cmd::modules`]), with the family's
//! fields as rows, and paths under home written `~` and with `/` between
//! their parts, as the screens write every path.

use std::path::{Path, PathBuf};

use agent_settings::item::{Entry, Item};
use agent_tui::tree::Card;

use super::build::{tilde, tilde_text};
use crate::cmd::modules::mods::ModFields;
use crate::cmd::modules::sensors::SensorFields;
use crate::cmd::modules::themes::ThemeFields;

/// How a family's fields head and fill its card.
pub trait Rows {
    fn title(&self, item: &Item) -> String {
        item.name.clone()
    }
    /// The dim line under the title.
    fn summary(&self, item: &Item) -> String;
    fn rows(&self) -> Vec<(String, String)>;
    /// The state rides the origin line, for a pane whose own rows leave the
    /// card little room.
    fn folds(&self) -> bool {
        false
    }
}

fn row(label: &str, value: impl Into<String>) -> (String, String) {
    (label.to_string(), value.into())
}

impl Rows for ModFields {
    fn summary(&self, _: &Item) -> String {
        "Claude Code mod".into()
    }
    fn rows(&self) -> Vec<(String, String)> {
        let installed = (!self.installed.is_empty()).then(|| row("installed", self.installed.join(", ")));
        installed.into_iter().chain([row("critical", self.critical), row("needs", self.needs.clone())]).collect()
    }
}

impl Rows for SensorFields {
    fn summary(&self, item: &Item) -> String {
        match (&self.description, item.kind) {
            (Some(d), _) => d.clone(),
            (None, Some(k)) => format!("a {} sensor", k.word()),
            (None, None) => "a sensor file".into(),
        }
    }
    fn rows(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let (Some(i), Some(m)) = (self.interval, self.min_interval) {
            out.push(row("interval", format!("{i}s quiet, {m}s busy")));
        }
        out.extend(self.threshold.map(|t| row("threshold", t.to_string())));
        out.extend(self.lane.map(|l| row("lane", l)));
        out
    }
}

impl Rows for ThemeFields {
    fn title(&self, item: &Item) -> String {
        self.label.clone().unwrap_or_else(|| item.name.clone())
    }
    fn summary(&self, item: &Item) -> String {
        format!("{} · {} · background {}", item.name, self.kind, self.ground)
    }
    fn rows(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn folds(&self) -> bool {
        true
    }
}

/// The card for `e`: its descriptor, and its family's fields as rows.
pub fn to_card<F: Rows>(e: Entry<F>) -> Card {
    let Entry { item, fields } = e;
    Card { title: fields.title(&item), summary: fields.summary(&item), fold_state: fields.folds(), rows: fields.rows(), item }
}

pub fn shown(mut c: Card, home: &Path) -> Card {
    let at = |p: &Path| PathBuf::from(tilde(p, home));
    c.item.path = c.item.path.as_deref().map(at);
    c.item.overrides.iter_mut().for_each(|o| o.path = at(&o.path));
    c.item.source = c.item.source.map(|s| tilde_text(&s, home));
    c.item.detail = c.item.detail.map(|s| tilde_text(&s, home));
    for (_, v) in &mut c.rows {
        *v = tilde_text(v, home);
    }
    c
}
