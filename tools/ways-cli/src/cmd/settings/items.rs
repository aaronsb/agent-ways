//! `ways settings items [<family>]` (ADR-516 §5): every theme, mod, sensor,
//! way and judge profile as the settings screens' cards describe them, from
//! the same producers ([`crate::cmd::modules`]).

use std::path::Path;

use agent_settings::exit;
use agent_settings::item::{Entry, Item};
use agent_theme::ColorDepth;
use serde::Serialize;
use serde_json::Value;

use super::tui::{self, Ctx, Ways};
use super::{fail, Failure, Out};

/// The families, in the order they list.
pub const FAMILIES: [&str; 5] = ["theme", "mod", "sensor", "way", "profile"];

/// An entry's descriptor, with the entry serialized.
fn value<F: Serialize>(e: Entry<F>) -> Result<(Item, Value), Failure> {
    let v = serde_json::to_value(&e).map_err(|err| fail(exit::WRITE_FAILED, format!("serializing {:?} {}: {err}", e.item.family, e.item.name)))?;
    Ok((e.item, v))
}

fn all<F: Serialize>(entries: impl IntoIterator<Item = Entry<F>>) -> Result<Vec<(Item, Value)>, Failure> {
    entries.into_iter().map(value).collect()
}

/// Every item of `family`, each with its entry serialized, for the project
/// `ways` was opened for.
fn of(family: &str, ways: &Ways) -> Result<Vec<(Item, Value)>, Failure> {
    use crate::cmd::modules;
    let ctx = &ways.ctx;
    match family {
        "theme" => all(modules::themes::all(&tui::themes(ways, &ways.layers(), ColorDepth::TrueColor))),
        "mod" => crate::cmd::mods::overview().iter().map(|o| value(modules::mods::item(&o.m, Ok(o)))).collect(),
        "sensor" => {
            let attend = (ctx.sensors)();
            tui::sensors::loaded(&ways.layers(), &ctx.project).iter().map(|l| value(modules::sensors::item(l, &attend))).collect()
        }
        "way" => all(modules::ways::all(&ctx.corpus, &ctx.user_ways, &ctx.project)),
        "profile" => {
            let has_key = |p| ways_agent_core::keys::locate_file(p).is_some();
            all(modules::profiles::all(&ways_agent_core::profile::user_layer_path(), has_key))
        }
        _ => Ok(Vec::new()),
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
    let mut items = Vec::new();
    for f in FAMILIES.iter().filter(|f| family.is_none_or(|w| w == **f)) {
        items.extend(of(f, &ways)?);
    }
    if as_json {
        let values: Vec<&Value> = items.iter().map(|(_, v)| v).collect();
        let text = serde_json::to_string_pretty(&values).map_err(|e| fail(exit::WRITE_FAILED, format!("serializing the items: {e}")))?;
        println!("{text}");
        return Ok(());
    }
    let word = |s: Option<&str>| s.map_or("-".to_string(), str::to_string);
    let rows: Vec<[String; 6]> = items
        .iter()
        .map(|(i, _)| {
            let family = format!("{:?}", i.family).to_lowercase();
            let file = i.file().map(|p| tilde(&p.display().to_string(), &ways.ctx.home));
            [family, i.name.clone(), i.origin_text(), word(i.kind.map(|k| k.word())), i.state.word().to_string(), word(file.as_deref())]
        })
        .collect();
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
