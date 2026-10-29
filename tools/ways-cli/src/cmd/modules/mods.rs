//! A mod as a [`Module`]: shipped with agent-ways, read from its marketplace
//! folder, its state in the words the bottom bar's notice uses when it is
//! critical and does not run.

use super::{Health, Kind, Module, Origin, Row, State};
use crate::cmd::attend_mod::{self, Mod, JUDGE};
use crate::cmd::mods::Overview;

/// Why a mod's state is not to hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// The first read has not landed.
    Reading,
    /// The read failed with nothing read before it.
    Failed,
    /// Read, and the mod was not in it.
    Missing,
}

/// `m` as its overview `o` reads it, or as far as an unread state tells.
pub fn of_mod(m: &Mod, o: Result<&Overview, Unread>) -> Module {
    let critical = match (m.critical, *m == JUDGE, o.is_ok_and(|o| o.claude_code)) {
        (false, _, _) => "no",
        (true, false, _) => "yes",
        (true, true, true) => "yes: gate.engine is claude-code",
        (true, true, false) => "no: only while gate.engine is claude-code",
    };
    let rows = vec![Row::new("critical", critical), Row::new("needs", format!("Claude Code {} or later", m.min_claude_code))];
    let base = |source: String, version, state| Module {
        name: m.name.into(),
        summary: "Claude Code mod".into(),
        kind: Some(Kind::Plugin),
        origin: Origin::Builtin,
        path: None,
        source,
        version,
        state,
        rows: rows.clone(),
    };
    let o = match o {
        Ok(o) => o,
        Err(why) => {
            let state = match why {
                Unread::Reading => State::new("reading…", Health::Idle),
                Unread::Failed => State::new("state read failed", Health::Broken),
                Unread::Missing => State::new("state not read", Health::Attention),
            };
            return base(m.id.into(), None, state);
        }
    };
    let version = match (o.shipped.as_deref(), o.installed.as_slice()) {
        (None, []) => None,
        (Some(s), []) => Some(format!("shipped {s}")),
        (None, i) => Some(format!("installed {}", i.join(", "))),
        (Some(s), i) => Some(format!("shipped {s} · installed {}", i.join(", "))),
    };
    use attend_mod::State as S;
    let state = match (o.attention.first(), o.word) {
        (Some(a), _) => State::new(format!("{} · critical", a.why.word()), Health::Broken),
        (None, S::Converged) => State::new("installed · enabled", Health::Running),
        (None, S::Drift) if o.older => State::new("update · an installed copy is older", Health::Attention),
        (None, S::Drift) => State::new("drift · repair converges it", Health::Attention),
        (None, S::Pending) => State::new("pending", Health::Attention),
        (None, S::Disabled) => State::new("disabled", Health::Attention),
        (None, S::Removed) => State::new("removed", Health::Idle),
        (None, S::Off) => State::new("off · not wanted", Health::Idle),
    };
    base(format!("{} · {}", m.id, o.folder.display()), version, state)
}
