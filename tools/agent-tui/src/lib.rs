//! The app shell on ratatui for agent-ways terminal applications (ADR-504
//! §3, §12).
//!
//! It owns the tab bar with pending badges, the bottom bar, the modes
//! (browse, edit, review and apply), the exit guard across tabs, mouse with
//! a capture toggle, confirm and masked entry, guided flows (pick, preview,
//! confirm), the theme tab, the copy, rename and delete of named items
//! ([`named`]), and the test kit: a headless render and golden frames. Colour comes from `agent-theme`. It knows nothing about ways or
//! attend: an [`Adapter`] supplies the content, does the writes and runs the
//! commands, and the tree it builds is the content of the tree-and-detail
//! screen.
//!
//! A screen that is not a tree, such as attend-chat, is a [`Pane`] inside
//! the same shell ([`App::with_pane`]): the shell keeps the tab bar, the
//! bottom bar and its footer, the mouse, the exit guard and the key help;
//! the pane draws between the bars with the shared parts: the text entry
//! ([`input`]), a feed of boxed entries ([`feed`]), chip rows ([`strip`])
//! and wrapping ([`wrap`]). A screen that needs none of the shell runs on
//! [`screen`] with the same terminal handling.
//!
//! Dependency direction: applications depend on this crate; it depends on
//! `agent-theme` and ratatui only. Nothing on a hook path links it into
//! anything it calls (ADR-504 §11).

pub mod adapter;
pub mod app;
pub mod feed;
pub mod hit;
pub mod input;
pub mod markdown;
pub mod named;
pub mod screen;
pub mod strip;
pub mod testkit;
pub mod timeline;
pub mod tree;
pub mod wrap;

use std::io;

use ratatui::DefaultTerminal;

pub use adapter::{Adapter, Unwired, Write};
pub use app::flow;
pub use app::theme;
pub use app::term::{clear_job_group, kill_group, kill_job_group, register_job_group, restore, Signals, TermGuard};
pub use app::pane::{binding_conflicts, Binding, Keyed, Open, Pane, PaneTab, Tone};
pub use app::{App, Jump, Session, TabKeys, Themes};
/// The ratatui this crate draws with, so an application names its types
/// (key events, buffers) without a second dependency to keep in step.
pub use ratatui;


/// Run `app` on the terminal until it quits, restoring the terminal on every
/// exit path: a return, an error, a panic and a termination signal. A
/// signal ends the session with [`Session::signal`] set; the caller exits
/// with 128 plus it.
pub fn run(app: App) -> io::Result<Session> {
    with_terminal(|term, signals| app.run(term, signals))
}

/// Take the terminal, catch the termination signals and install the panic
/// hook, then hand the terminal to `body`; restore it on every exit path.
pub(crate) fn with_terminal<T>(body: impl FnOnce(&mut DefaultTerminal, &Signals) -> io::Result<T>) -> io::Result<T> {
    let signals = Signals::install()?;
    // The hook in place before ratatui::init adds its own. Its own restores
    // less (not mouse capture or the cursor) and prints when the terminal
    // has gone, which panics inside the hook; this one replaces it, and
    // runs where a panic that aborts skips the guard's drop.
    let hook = std::panic::take_hook();
    let mut guard = TermGuard::new();
    let _ratatui_hook = std::panic::take_hook();
    std::panic::set_hook(screen::panic_hook(hook));
    body(&mut guard.term, &signals)
}
