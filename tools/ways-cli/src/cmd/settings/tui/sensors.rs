//! The sensors tab's level above the settings: a node per sensor, each
//! carrying its card ([`modules::sensors`]).

use std::path::Path;

use agent_settings::load::Layer;
use agent_settings::{Bound, Registry};
use agent_tui::tree::Node;

use crate::cmd::modules::sensors::{self as modules, Attend};

/// The sensors tab's keys.
pub fn with_user_keys(_reg: &Registry, keys: Vec<Bound>, _layers: &[Layer], _project: &Path) -> Vec<Bound> {
    keys
}

/// Put a card on each sensor's node under `root`.
pub fn cards(root: &mut Node, _layers: &[Layer], _project: &Path, home: &Path, attend: &Attend) {
    for g in root.children.iter_mut().filter(|g| !g.section) {
        let m = modules::of_builtin_sensor(&g.name, attend);
        g.card = Some(super::card::card(&m, home));
    }
}
