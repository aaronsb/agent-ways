//! The questions on the bottom bar, each a [`Dialog`] placed on the bar
//! behind a [`Modal`]: the confirm before an action is queued, the discard
//! of a tab's items from the tree and from review, the delete of a named
//! item, and a pane's question (ADR-408), the shell's own "quit?" among
//! them. Each has two answers, `y` and `n` in either case; Esc and Enter
//! give the one that keeps things as they are.

use super::dialog::{Answer, Button, Dialog, DialogBuilder};
use super::modals::Modal;
use super::pane::Overlay;
use super::theme::Ground;
use super::*;

/// The confirm before an action is queued ([`Mode::Confirm`]).
pub(super) struct Confirm;
/// The discard of a tab's items, asked from the tree ([`Mode::DiscardTab`]).
pub(super) struct DiscardTab;
/// The discard of a tab's items, asked from its review ([`Mode::Review`]).
pub(super) struct ReviewDiscard;
/// The delete of one of a tab's named items ([`Mode::ItemDelete`]).
pub(super) struct ItemDelete;
/// A pane's question, or the shell's "quit?" ([`Mode::Ask`]).
pub(super) struct Ask;

/// `question` on the bar, answered `yes` on `y` or `n` on its own ground;
/// `no` is the cancel and the default.
fn question(question: String, yes: (String, Ground), no: (String, Ground)) -> DialogBuilder<'static> {
    Dialog::bar(question)
        .button(Button::new(Btn::Yes, yes.0, yes.1).key('y').key('Y'))
        .button(Button::new(Btn::No, no.0, no.1).key('n').key('N').cancel().default())
}

/// Yes or no, for an answer that is one of the two buttons.
fn yes(a: Answer) -> Option<bool> {
    match a {
        Answer::Button(Btn::Yes) => Some(true),
        Answer::Button(Btn::No) => Some(false),
        _ => None,
    }
}

/// The discard of `tab`'s items: drop on the error ground, keep beside it.
fn discard(app: &App, tab: usize) -> Dialog<'static> {
    let q = format!("discard {} pending in {}?", app.pending_in(tab), app.roots[tab].name);
    question(q, ("y discard".into(), Ground::Err), ("n keep".into(), Ground::Keep)).build().expect("n is the cancel and the default")
}

impl Modal for Confirm {
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let Mode::Confirm { queued } = &app.mode else { unreachable!("a confirm is open") };
        question(queued.command.clone(), ("y queue".into(), Ground::Ok), ("n cancel".into(), Ground::Err)).build().expect("n is the cancel and the default")
    }

    fn answer(&self, app: &mut App, a: Answer) -> bool {
        let Some(yes) = yes(a) else { return true };
        let Mode::Confirm { queued } = std::mem::replace(&mut app.mode, Mode::Browse) else { return true };
        if yes {
            app.enqueue(queued);
        } else {
            app.msg = "not queued".into();
        }
        true
    }
}

impl Modal for DiscardTab {
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let Mode::DiscardTab { tab } = app.mode else { unreachable!("a discard is open") };
        discard(app, tab)
    }

    fn answer(&self, app: &mut App, a: Answer) -> bool {
        let (Some(yes), Mode::DiscardTab { tab }) = (yes(a), &app.mode) else { return true };
        let tab = *tab;
        app.mode = Mode::Browse;
        if yes {
            app.discard_tab(tab);
        }
        true
    }
}

impl Modal for ReviewDiscard {
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let Mode::Review { tab, .. } = app.mode else { unreachable!("review is open") };
        discard(app, tab)
    }

    /// Yes drops the tab's items and goes on to the next tab with any; no
    /// goes back to the review.
    fn answer(&self, app: &mut App, a: Answer) -> bool {
        let (Some(yes), Mode::Review { tab, .. }) = (yes(a), &app.mode) else { return true };
        let tab = *tab;
        if yes {
            app.discard_tab(tab);
            app.advance(tab);
        } else {
            app.mode = Mode::Review { tab, run: None, discard: false };
        }
        true
    }
}

impl Modal for ItemDelete {
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let Mode::ItemDelete { name, noun } = &app.mode else { unreachable!("a delete is open") };
        question(format!("delete {noun} {name}?"), ("y delete".into(), Ground::Err), ("n keep".into(), Ground::Keep)).build().expect("n is the cancel and the default")
    }

    fn answer(&self, app: &mut App, a: Answer) -> bool {
        let Some(yes) = yes(a) else { return true };
        let Mode::ItemDelete { name, .. } = std::mem::replace(&mut app.mode, Mode::Browse) else { return true };
        if yes {
            app.item_delete(&name);
        } else {
            app.msg = "kept".into();
        }
        true
    }
}

impl Modal for Ask {
    /// Every key that is no answer comes back to answer no, and a press of
    /// any button off the answers, anywhere, answers no too.
    fn dialog(&self, app: &App, _: bool) -> Dialog<'static> {
        let Mode::Ask(a) = &app.mode else { unreachable!("a question is open") };
        question(a.question.clone(), (format!("y {}", a.yes), Ground::Err), (format!("n {}", a.no), Ground::Keep))
            .forward(|_| true)
            .any_press()
            .build()
            .expect("n is the cancel and the default")
    }

    /// The answer goes to the pane ([`App::answer`]); a key that is no
    /// answer answers no and may then go on ([`App::answer_then`]).
    fn answer(&self, app: &mut App, a: Answer) -> bool {
        let Mode::Ask(asked) = &app.mode else { return true };
        let asked = asked.clone();
        match a {
            Answer::Stay => {}
            Answer::Forward(k) => return app.answer_then(asked, k),
            a => app.answer(asked, a == Answer::Button(Btn::Yes)),
        }
        true
    }

    fn overlay(&self, app: &App) -> Overlay {
        let Mode::Ask(a) = &app.mode else { unreachable!("a question is open") };
        Overlay::Ask { id: a.id.clone() }
    }
}
