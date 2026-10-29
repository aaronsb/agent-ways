//! The mod tab's level above the settings (ADR-512 item 6): one node per
//! shipped mod, labelled with its shipped version and its state in a word,
//! holding a read-only row per enabled target, the mod's settings, and the
//! `ways manage mod` commands as actions, on the node and on each target
//! row. The state is read from [`crate::cmd::mods::overview`] off the
//! screen's thread ([`super::modread`]). A critical mod that does not run
//! carries a finding on its node and on the row of each target where it
//! does not run, and puts a notice on the bottom bar. Each node carries the
//! mod's card ([`modules::mods`]), whose state agrees with that notice.

use agent_tui::tree::{Action, Kind, Node, Setting};

use crate::cmd::attend_mod::{State, JUDGE, MODS};
use crate::cmd::modules::{self, mods::Unread};
use crate::cmd::mods::{Attention, Overview};

/// What the target rows' value column comes from.
const SOURCE: &str = "Claude Code's plugin lists, repair's record";

/// What the install, uninstall and repair actions change in each enabled
/// target.
const TOUCHES: &str = "each enabled target's plugin list and settings.json, and the ways config";

/// Where the read of the mods' state stands, as the tab shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// Read; a mod missing from it is not shipped.
    Done,
    /// The first read has not landed yet.
    Reading,
    /// The read failed, saying why, with nothing read before it.
    Failed(String),
    /// The last read failed, saying why; the tab shows the one before it.
    Stale(String),
}

/// Turn the tab's groups, one per mod, into the mod nodes. A mod with no
/// group gets one, so every shipped mod has its node. A mod whose state is
/// not read yet still offers `check`, which reads for itself.
pub fn nodes(root: &mut Node, states: &[Overview], read: &Read, home: &std::path::Path) {
    for m in MODS {
        if !root.children.iter().any(|g| g.name == m.name) {
            root.children.push(Node::group(m.name, "", vec![]));
        }
    }
    for g in root.children.iter_mut().filter(|g| MODS.iter().any(|m| m.name == g.name)) {
        // Not a section: the mod's name joins its rows' keys, so attend's
        // and ways-judge's `install` and target rows stay apart across a
        // reload, a pending edit and the cursor with them.
        g.section = false;
        g.open = true;
        if let Read::Stale(why) = read {
            g.finding = Some(format!("The last read of the mods' state failed: {why}. This shows the read before it."));
        }
        let found = states.iter().find(|o| o.m.name == g.name);
        if let Some(m) = MODS.iter().find(|m| m.name == g.name) {
            let unread = match read {
                Read::Reading => Unread::Reading,
                Read::Failed(_) => Unread::Failed,
                Read::Done | Read::Stale(_) => Unread::Missing,
            };
            g.card = Some(super::card::card(&modules::mods::of_mod(m, found.ok_or(unread)), home));
        }
        let Some(o) = found else {
            let label = match read {
                Read::Reading => "reading…",
                Read::Failed(_) => "state read failed",
                Read::Done | Read::Stale(_) => "state not read",
            };
            g.columns = Some((String::new(), label.into()));
            if let (Read::Failed(why), Some(m)) = (read, MODS.iter().find(|m| m.name == g.name)) {
                g.finding = Some(format!("Reading the {} state failed: {why}", m.label));
            }
            g.actions = vec![check(g.name.as_str(), &format!("the {} mod", g.name))];
            continue;
        };
        g.columns = Some((String::new(), format!("{}  {}", o.shipped.as_deref().unwrap_or("not shipped"), o.word.word())));
        let acts = actions(o);
        let rows: Vec<Node> = o
            .rows
            .iter()
            .map(|(dir, row, full)| {
                let doc = format!("{full}\n\nThe {} in {dir}, read from Claude Code's plugin lists and repair's record of what it installed there.", o.m.label);
                let mut n = Node::leaf(dir.clone(), doc, Setting::new(Kind::ReadOnly, row.clone(), SOURCE))
                    .headed(format!("{} in {dir}", o.m.name))
                    .with_actions(acts.clone());
                n.finding = o.attention.iter().find(|a| a.dir == *dir).map(|a| attention_line(o, a, "here"));
                n
            })
            .collect();
        g.children.splice(0..0, rows);
        g.actions = acts;
        if g.finding.is_none() && !o.attention.is_empty() {
            g.finding = Some(o.attention.iter().map(|a| attention_line(o, a, &format!("in {}", a.dir))).collect::<Vec<_>>().join("\n"));
        }
    }
}

/// A critical mod that does not run `at` a target, and its fix.
fn attention_line(o: &Overview, a: &Attention, at: &str) -> String {
    format!("{} is critical and {} {at}. Fix: {}.", o.m.name, a.why.word(), a.fix)
}

/// The bottom bar's notice of the critical mods that do not run, longest
/// form first: one mod by name and why, several by count; the longer forms
/// point at this tab, whose rows say why and how to fix it.
pub fn notice(states: &[Overview]) -> Vec<String> {
    let flagged: Vec<&Overview> = states.iter().filter(|o| !o.attention.is_empty()).collect();
    let count = format!("{} mod{} degraded", flagged.len(), if flagged.len() == 1 { "" } else { "s" });
    match flagged.as_slice() {
        [] => Vec::new(),
        [o] => {
            let named = format!("{} {}", o.m.name, o.attention[0].why.word());
            let mut forms = vec![format!("{named} · see mod tab"), named];
            if count.len() < forms[1].len() {
                forms.push(count);
            }
            forms
        }
        _ => vec![format!("{count} · see mod tab"), count],
    }
}

/// The `check` action, which reads for itself.
fn check(name: &str, label: &str) -> Action {
    Action::new("check", format!("ways manage mod check {name}"))
        .reads()
        .verifies()
        .doc(format!("Checks {label} in each enabled target and names the fix for each problem. Changes nothing."))
}

/// The mod's actions: `check`, `install` and `uninstall` where they apply,
/// and `repair`, labelled `update` when an installed copy is older than the
/// shipped one.
pub fn actions(o: &Overview) -> Vec<Action> {
    let name = o.m.name;
    let judge = o.m == JUDGE;
    let mut out = vec![check(name, &format!("the {}", o.m.label))];
    // An app that does not ship the mod has nothing to install.
    if o.shipped.is_none() {
        return out;
    }
    // ways-judge follows the engine: installing it is the claude-code choice,
    // shown only where it would change something.
    if !judge || !o.claude_code || o.word == State::Removed {
        let doc = if judge && o.claude_code {
            format!("Writes mods.ways-judge.install: true in the user config (gate.engine is already claude-code), removes an install false from each enabled target's config, then installs the {}.", o.m.label)
        } else if judge {
            format!("Sets gate.engine to claude-code and mods.ways-judge.install to true in the user config, removes an install false from each enabled target's config, then installs the {}.", o.m.label)
        } else {
            format!("Writes mods.{name}.install: true in the user config, removes an install false from each enabled target's config, then installs the {} in every enabled target.", o.m.label)
        };
        let touches = if judge { format!("{TOUCHES}, and gate.engine in agent.yaml") } else { TOUCHES.to_string() };
        out.push(Action::new("install", format!("ways manage mod install {name} --commit")).confirm().doc(doc).touches(touches));
    }
    // Removing the judge's host while it is the engine would leave the gate
    // failing open or paying for a key: `ways judge-setup` picks another.
    if !judge || !o.claude_code {
        out.push(
            Action::new("uninstall", format!("ways manage mod uninstall {name} --commit"))
                .confirm()
                
                .doc(format!("Writes mods.{name}.install: false in the user config, then uninstalls the {} from every enabled target.", o.m.label))
                .touches(TOUCHES),
        );
    }
    let (label, doc) = if o.older {
        ("update", format!("Updates the {} where an installed copy is older than the shipped {}, and converges it in every enabled target.", o.m.label, o.shipped.as_deref().unwrap_or("?")))
    } else {
        ("repair", format!("Converges the {} in every enabled target to its wanted state: installs, updates or uninstalls it.", o.m.label))
    };
    out.push(Action::new(label, format!("ways manage mod repair {name} --commit")).confirm().doc(doc).touches(TOUCHES));
    out
}
