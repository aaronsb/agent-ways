//! The sensors tab's level above the settings: a node per sensor its files
//! declare (ADR-403), each carrying its card ([`modules::sensors`]) and the
//! keys every sensor has, written to its own file.

use std::path::Path;

use agent_settings::load::Layer;
use agent_settings::{LayerScope, Registry};
use agent_tui::tree::Node;
use attend_config::schema::SENSOR_FILE;

use crate::cmd::modules::sensors::{self as modules, Attend};

/// The sensors tab's keys: each sensor's, in name order, every one the
/// sensor has whether or not a file sets it.
pub fn keys(reg: &Registry, layers: &[Layer]) -> Vec<agent_settings::Bound> {
    crate::cmd::settings::sensor_keys(reg, layers)
}

/// Whether the key `key` of the sensor `name` is written to the project's
/// sensor file: every key of a sensor a project's file defines (sets its
/// `kind`), but its `enabled`, which is each operator's own. An override of
/// a shipped or user sensor stays in the user's file.
pub fn writes_project(layers: &[Layer], name: &str, key: &str) -> bool {
    let at = ["sensors".to_string(), name.to_string(), "kind".to_string()];
    key != "attend.sensors.*.enabled"
        && layers.iter().rev().filter(|l| l.file == SENSOR_FILE).find(|l| l.get(&at).is_some()).is_some_and(|l| l.scope == LayerScope::Project)
}

/// The user's `attend.project_sensors`, as `layers` hold it.
fn trusted(layers: &[Layer]) -> Vec<String> {
    let user: Vec<Layer> = layers.iter().filter(|l| l.file == attend_config::FILE && l.scope == LayerScope::User).cloned().collect();
    attend_config::Config::from_layers(&user).project_sensors
}

/// The sensors the files `layers` hold declare, layered for `project`
/// under the user's allow-list.
pub fn loaded(layers: &[Layer], project: &Path) -> Vec<attend_config::sensors::Loaded> {
    attend_config::sensors::from_layers(layers, project, &trusted(layers))
}

/// Put a card on each sensor's node under `root`, from the sensor files
/// `layers` hold, layered for `project`.
pub fn cards(root: &mut Node, layers: &[Layer], project: &Path, home: &Path, attend: &Attend) {
    let loaded = loaded(layers, project);
    for g in root.children.iter_mut().filter(|g| !g.section) {
        let Some(l) = loaded.iter().find(|l| l.name == g.name) else { continue };
        g.card = Some(super::card::shown(super::card::to_card(modules::item(l, attend)), home));
    }
}

/// The directories of the user's and the project's sensor files, for the
/// change watch: a file added there reloads the screens.
pub fn dirs(project: Option<&Path>) -> Vec<std::path::PathBuf> {
    let mut out = vec![attend_config::sensors::user_dir()];
    out.extend(project.map(attend_config::sensors::project_dir));
    out
}
