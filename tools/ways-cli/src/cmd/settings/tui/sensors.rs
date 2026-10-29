//! The sensors tab's level above the settings: a node per sensor, each
//! carrying its card ([`modules::sensors`]). A sensor of the user's gets
//! the generic sensor keys a built-in has, written like a built-in's.

use std::path::Path;

use agent_settings::load::Layer;
use agent_settings::{Bound, Registry};
use agent_tui::tree::Node;
use attend_config::schema::BUILTINS;

use crate::cmd::modules::sensors::{self as modules, Described};

/// The sensors tab's keys: `keys` with every generic sensor key bound for
/// each sensor the user added, so its node holds the rows a built-in's
/// does, in the tab's order: the built-ins, then the user's, each sensor's
/// keys in the schema's order.
pub fn with_user_keys(reg: &Registry, mut keys: Vec<Bound>, layers: &[Layer], project: &Path) -> Vec<Bound> {
    let schema = &attend_config::schema::SCHEMA;
    let users = modules::user_sensors(layers, project);
    for u in &users {
        for spec in schema.keys.iter().filter(|k| k.section == "attend.sensors" && k.instances == BUILTINS) {
            let name = spec.name.replacen('*', &u.name, 1);
            if !keys.iter().any(|b| b.name() == name) {
                keys.extend(reg.lookup(&name));
            }
        }
    }
    let group = |b: &Bound| {
        let n = b.bound.first().map(String::as_str).unwrap_or_default();
        BUILTINS.iter().position(|x| *x == n).unwrap_or(BUILTINS.len() + users.iter().position(|u| u.name == n).unwrap_or(users.len()))
    };
    let field = |b: &Bound| schema.keys.iter().position(|k| k.name == b.spec.name).unwrap_or(usize::MAX);
    keys.sort_by_key(|b| (group(b), field(b)));
    keys
}

/// Put a card on each sensor's node under `root`.
pub fn cards(root: &mut Node, layers: &[Layer], project: &Path, home: &Path, attend: Option<&[Described]>) {
    let cfg = attend_config::Config::from_layers(layers);
    let users = modules::user_sensors(layers, project);
    for g in root.children.iter_mut().filter(|g| !g.section) {
        let Some(sc) = cfg.sensors.get(&g.name) else { continue };
        let m = match users.iter().find(|u| u.name == g.name) {
            Some(u) => modules::of_script_sensor(u, sc),
            None if BUILTINS.contains(&g.name.as_str()) => modules::of_builtin_sensor(&g.name, sc, attend),
            None => continue,
        };
        g.card = Some(super::card::card(&m, home));
    }
}
