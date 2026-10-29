//! `ways settings items [<family>]` (ADR-516 §5): every theme, mod, sensor,
//! way and judge profile as the settings screens' cards describe them, from
//! the same producers ([`crate::cmd::modules`]).

use std::path::Path;

use agent_settings::exit;
use agent_theme::ColorDepth;
use serde::Serialize;
use serde_json::Value;

use super::tui::{self, Ctx, Ways};
use super::{fail, Out};

/// The families, in the order they list.
pub const FAMILIES: [&str; 5] = ["theme", "mod", "sensor", "way", "profile"];

fn value(e: impl Serialize) -> Value {
    serde_json::to_value(e).unwrap_or_default()
}

/// Every item of `family`, each serialized as its entry, for the project
/// `ways` was opened for.
fn of(family: &str, ways: &Ways) -> Vec<Value> {
    use crate::cmd::modules;
    let ctx = &ways.ctx;
    match family {
        "theme" => modules::themes::all(&tui::themes(ways, &ways.layers(), ColorDepth::TrueColor)).into_iter().map(value).collect(),
        "mod" => crate::cmd::mods::overview().iter().map(|o| value(modules::mods::item(&o.m, Ok(o)))).collect(),
        "sensor" => {
            let attend = (ctx.sensors)();
            tui::sensors::loaded(&ways.layers(), &ctx.project).iter().map(|l| value(modules::sensors::item(l, &attend))).collect()
        }
        "way" => modules::ways::all(&ctx.corpus, &ctx.user_ways, &ctx.project).into_iter().map(value).collect(),
        "profile" => {
            let has_key = |p| ways_agent_core::keys::locate_file(p).is_some();
            modules::profiles::all(&ways_agent_core::profile::user_layer_path(), has_key).into_iter().map(value).collect()
        }
        _ => Vec::new(),
    }
}

/// `~` for the home directory.
fn tilde(p: &str, home: &Path) -> String {
    let h = home.display().to_string();
    match p.strip_prefix(&h) {
        Some(rest) if !h.is_empty() => format!("~{rest}"),
        _ => p.to_string(),
    }
}

/// List the items of `family`, or of every family: one line each, or with
/// `--json` one object each.
pub fn list(family: Option<&str>, as_json: bool, project: Option<&Path>) -> Out {
    if let Some(f) = family.filter(|f| !FAMILIES.contains(f)) {
        return Err(fail(exit::USAGE, format!("no family `{f}`; the families are {}", FAMILIES.join(", "))));
    }
    let ways = Ways::new(Ctx::from_env(project));
    let items: Vec<Value> = FAMILIES.iter().filter(|f| family.is_none_or(|w| w == **f)).flat_map(|f| of(f, &ways)).collect();
    if as_json {
        println!("{}", serde_json::to_string_pretty(&items).unwrap_or_default());
        return Ok(());
    }
    let text = |v: &Value, k: &str| v[k].as_str().map_or("-".to_string(), |s| tilde(s, &ways.ctx.home));
    let rows: Vec<[String; 6]> = items.iter().map(|v| ["family", "name", "origin", "kind", "state", "path"].map(|k| text(v, k))).collect();
    let mut w = [0; 5];
    for r in &rows {
        for (i, c) in r.iter().take(5).enumerate() {
            w[i] = w[i].max(c.chars().count());
        }
    }
    for r in &rows {
        println!("{:<a$}  {:<b$}  {:<c$}  {:<d$}  {:<e$}  {}", r[0], r[1], r[2], r[3], r[4], r[5], a = w[0], b = w[1], c = w[2], d = w[3], e = w[4]);
    }
    Ok(())
}
