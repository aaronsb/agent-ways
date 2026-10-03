//! `ways settings theme list|copy|rename|delete` (#851): the theme tab's
//! file actions on the command line (ADR-503: the screens and the CLI are
//! two ways in). Both go through `agent_tui::named` over the same
//! `Themes`, and keep the active choice through the same adapter call, so
//! they refuse and report alike.

use agent_settings::exit;
use agent_theme::{ColorDepth, Source};
use agent_tui::named::{self, Done, Refusal, Refused};
use agent_tui::{Adapter, Themes};
use serde_json::json;

use super::tui::{self, Ctx, Ways};
use super::{fail, Failure, Out};

/// The operation `ways settings theme` was asked for.
pub enum Op<'a> {
    Copy { from: &'a str, to: &'a str },
    Rename { from: &'a str, to: &'a str },
    Delete { name: &'a str },
}

fn load() -> (Ways, Themes) {
    let ways = Ways::new(Ctx::from_env(None));
    let layers = ways.layers();
    // The depth only shapes drawing, which nothing here does.
    let themes = tui::themes(&ways, &layers, ColorDepth::TrueColor);
    (ways, themes)
}

fn source(s: Source) -> &'static str {
    match s {
        Source::Bundled => "bundled",
        Source::User => "user",
        Source::Override => "override",
    }
}

/// The themes on offer: name, source and the active mark; with `--json`,
/// each with its label and file.
pub fn list(as_json: bool) -> Out {
    let (_, themes) = load();
    let rows = themes.list();
    if as_json {
        let out: Vec<_> = rows
            .iter()
            .map(|(t, s)| {
                let file = (*s != Source::Bundled).then(|| themes.path_of(&t.name)).flatten();
                json!({ "name": t.name, "label": t.label, "source": source(*s), "active": t.name == themes.active, "file": file })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return Ok(());
    }
    let w = rows.iter().map(|(t, _)| t.name.len()).max().unwrap_or(0);
    for (t, s) in &rows {
        let mark = if t.name == themes.active { " active" } else { "" };
        println!("{:<w$}  {}{mark}", t.name, source(*s));
    }
    Ok(())
}

fn refused(r: Refused) -> Failure {
    match r.why {
        Refusal::Missing => fail(exit::USAGE, format!("{}; `ways settings theme list` names the themes", r.message)),
        Refusal::Bundled | Refusal::Name | Refusal::Taken => fail(exit::REJECTED, r.message),
        Refusal::Write => fail(exit::WRITE_FAILED, r.message),
    }
}

/// Copy, rename or delete a theme file. The active choice follows a rename
/// of the active theme and falls back to the default when it is deleted;
/// the first line printed says so, the second names the file written or
/// removed.
pub fn run(op: Op, as_json: bool) -> Out {
    let (mut ways, mut themes) = load();
    let done = match op {
        Op::Copy { from, to } => named::copy(&mut themes, from, to),
        Op::Rename { from, to } => named::rename(&mut themes, from, to),
        Op::Delete { name } => named::delete(&mut themes, name),
    }
    .map_err(refused)?;
    let moves = themes.follow(&done);
    let msg = themes.settle(&done, |n| ways.choose_theme(n));
    let kept = moves.as_ref().is_none_or(|n| *n == themes.active);
    if as_json {
        let mut out = match &done {
            Done::Copied { from, to, .. } => json!({ "action": "copy", "from": from, "to": to }),
            Done::Renamed { from, to, .. } => json!({ "action": "rename", "from": from, "to": to }),
            Done::Deleted { name, .. } => json!({ "action": "delete", "name": name }),
        };
        out["file"] = json!(done.file());
        out["active"] = json!(themes.active);
        out["active_moved"] = json!(moves.is_some() && kept);
        out["message"] = json!(msg);
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else {
        println!("{msg}\n{}", themes.show(done.file()));
    }
    if !kept {
        return Err(fail(exit::WRITE_FAILED, "theme.active was not moved; `ways settings set theme.active <name>` sets it"));
    }
    Ok(())
}
