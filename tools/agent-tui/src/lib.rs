//! The app shell on ratatui for agent-ways terminal applications (ADR-504
//! §3, §12).
//!
//! It owns the tab bar with pending badges, the bottom bar, the modes
//! (browse, edit, review and apply), the exit guard across tabs, mouse with
//! a capture toggle, confirm and masked entry, guided flows (pick, preview,
//! confirm), the theme tab, and the test kit: a headless render and golden
//! frames. Colour comes from `agent-theme`. It knows nothing about ways or
//! attend: an [`Adapter`] supplies the content, does the writes and runs the
//! commands, and the tree it builds is the content of the tree-and-detail
//! screen.
//!
//! Dependency direction: applications depend on this crate; it depends on
//! `agent-theme` and ratatui only. Nothing on a hook path links it into
//! anything it calls (ADR-504 §11).

pub mod adapter;
pub mod app;
pub mod testkit;
pub mod tree;

use std::io;

pub use adapter::{Adapter, Unwired, Write};
pub use app::flow;
pub use app::theme;
pub use app::term::{restore, Signals, TermGuard};
pub use app::{App, Session, Themes};
/// The ratatui this crate draws with, so an application names its types
/// (key events, buffers) without a second dependency to keep in step.
pub use ratatui;


/// Run `app` on the terminal until it quits, restoring the terminal on every
/// exit path: a return, an error, a panic and a termination signal. A
/// signal ends the session with [`Session::signal`] set; the caller exits
/// with 128 plus it.
pub fn run(app: App) -> io::Result<Session> {
    let signals = Signals::install()?;
    let mut guard = TermGuard::new();
    // ratatui::init's panic hook restores raw mode and the screen but not
    // mouse capture or the cursor; this one does it all, before a panic
    // that aborts skips the guard's drop.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        hook(info);
    }));
    app.run(&mut guard.term, &signals)
}
