//! The chat's interaction state as one value, and the one function that
//! moves it (ADR-407 §Interaction state).
//!
//! [`Ix`] is where the keyboard is ([`Focus`]), the column row the focus is
//! on, the last tab or row the focus was on, and the stack of overlays the
//! shell holds open over the pane. Nothing else
//! writes it: every input that can move it reaches [`step`] as an [`Ev`],
//! with a [`View`] of what the screen lays out, and gets back the next `Ix`
//! and the [`Effect`]s to carry out, in order. `step` is pure, so each cell
//! of the interaction table is a unit test with no terminal.
//!
//! The chat holds its `Ix` in a [`Machine`], whose one writing method runs
//! `step`; the fields of `Ix` are private to this module. So the single
//! writer is the compiler's to keep: outside this module an `Ix` can be
//! read, and cloned, but never put back ([`ChatPane::dispatch`] is the only
//! caller of [`Machine::step`], and carries the effects out). The shell
//! asks how to route a key ([`Ix::ctx`]) and reads nothing else of it.
//!
//! The rules are those of [`super::focus`]: in the compose box every key
//! edits but for the ways out; outside it the keys walk the ring, a
//! character goes back to the box and is typed there, and Esc moves along
//! the Esc cycle: the box, the feed, the `≡` menu, the last tab or column
//! row the focus was on, the `≡` menu again; Tab out of the box onto `≡`
//! is not remembered. Walking the tab bar shows
//! nothing: Enter opens the focused tab's menu, and the column's numbers
//! show a channel.
//!
//! The world (the channels, the agents) is read lazily, as the chat always
//! has: a `View` carries it only when it is already read. A step that needs
//! it and finds it absent answers [`NeedWorld`]; the dispatcher reads the
//! world, which refocuses ([`Ev::WorldChanged`]), and steps again.

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::{Ctx, Overlay};

use super::activate::{How, Target};
use super::focus::Focus;
use super::sidebar::SideHit;
use crate::tabs::{Slot, Tab};

/// The chat's interaction state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Ix {
    /// Where the keyboard is.
    focus: Focus,
    /// The column row the focus is on, while it is on one: a refresh that
    /// moves the row keeps the focus on it.
    row_hit: Option<SideHit>,
    /// The last tab or column row the focus was on, and the row's hit: where
    /// the Esc cycle comes back to when the `≡` menu Esc opened closes.
    last_nav: Option<(Focus, Option<SideHit>)>,
    /// What the shell holds open over the pane, bottom first, as it
    /// reported each opening and closing: the pane's menus by their ids,
    /// its entry dialog and its questions, the key help, the exit guard.
    /// The one record of what is open.
    overlays: Vec<Overlay>,
    /// Ctrl+X was the box's last key: the first half of Ctrl+X Ctrl+S,
    /// send now. The operator's next input, whatever it is, ends it.
    chord: bool,
}

impl Ix {
    pub(super) fn focus(&self) -> &Focus {
        &self.focus
    }

    /// The column row under the focus, by what it stands for.
    #[cfg(test)]
    pub(super) fn row_hit(&self) -> Option<&SideHit> {
        self.row_hit.as_ref()
    }

    /// The compose box types every plain key while it has the focus.
    pub(super) fn owns_text(&self) -> bool {
        self.focus == Focus::Compose
    }

    /// The chat is the compose box and the feed: there every plain key is
    /// text, and the feed hands it to the box.
    pub(super) fn in_chat(&self) -> bool {
        matches!(self.focus, Focus::Compose | Focus::Feed)
    }

    /// An overlay of the shell's holds the keyboard over the pane.
    pub(super) fn overlaid(&self) -> bool {
        !self.overlays.is_empty()
    }

    pub(super) fn on_bar(&self) -> bool {
        matches!(self.focus, Focus::Tab(_))
    }

    /// The column's row under the focus, if it is there.
    pub(super) fn side_focus(&self) -> Option<usize> {
        match self.focus {
            Focus::Row(i) => Some(i),
            _ => None,
        }
    }

    /// How the shell routes a key beside the chat: the compose box and the
    /// feed take every plain key as text, so `q`, `m`, `?` and the digits
    /// are the shell's only on the tab bar and the column; a plain digit is
    /// the column's number only while the focus is on a channel's row, since
    /// agents carry no number; the chat owns the bar's focus, and Esc never
    /// quits (ADR-409).
    pub(super) fn ctx(&self) -> Ctx {
        let digits = matches!(self.focus, Focus::Row(_)) && matches!(self.row_hit, Some(SideHit::Channel(_)));
        Ctx { text: self.in_chat(), digits, owns_bar: true, esc_quits: false }
    }
}

/// The chat's one [`Ix`], and the one way to move it. It has no `Default`,
/// so `std::mem::take` cannot reset it outside this module; replacing the
/// whole value with [`Machine::new`] is the one write the compiler allows
/// outside `step`, and only [`ChatPane::new`] does it.
#[derive(Debug)]
pub(super) struct Machine(Ix);

impl Machine {
    /// The state a chat starts in: the keyboard in the compose box,
    /// nothing open.
    pub(super) fn new() -> Machine {
        Machine(Ix::default())
    }

    /// Step by `ev` ([`step`]) and keep the result: the effects to carry
    /// out, in order. On [`NeedWorld`] nothing moves.
    pub(super) fn step(&mut self, ev: Ev, v: &View) -> Result<Vec<Effect>, NeedWorld> {
        let (ix, fx) = step(&self.0, ev, v)?;
        self.0 = ix;
        Ok(fx)
    }
}

impl std::ops::Deref for Machine {
    type Target = Ix;

    fn deref(&self) -> &Ix {
        &self.0
    }
}

/// An input that can move the interaction state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ev {
    /// A key the shell left to the pane.
    Key(KeyEvent),
    Click(Click),
    /// A paste: the text goes into the compose box, and the focus with it.
    Paste,
    /// A menu item put text in the compose box, or pointed a reply there:
    /// the focus goes to the box.
    Compose,
    /// F2 or Ctrl+T: the focus to the tab shown, or from the bar to the box.
    ToggleBar,
    /// The bar's slot `i` is shown (`Pane::set_tab`).
    Jump(usize),
    /// The column's channel `n`, counted from 1, or with 0 the operator
    /// view: Ctrl+digit or Alt+digit (`menu`), or a plain digit on a column
    /// row (`Pane::jump`).
    Numbered { n: usize, menu: bool },
    /// The bar's slot `i` activated: Ctrl+digit, a click on it, a right click
    /// (`menu`) (`Pane::activate_tab`).
    TabActivated { i: usize, menu: bool },
    /// The world was read again: a focused row follows its row, a focused tab
    /// whose channel is gone moves to the tab shown.
    WorldChanged,
    /// The column is hidden, by its key or for want of width.
    ColumnHidden,
    /// A key of the operator's that the shell, or the chat's own column
    /// key, took before the interaction state saw it: the key help, the
    /// mouse, quitting, a new channel, the column shown or hidden.
    KeyTaken,
    /// The shell opened an overlay over the pane, on top of the rest
    /// (`Pane::overlay_opened`).
    OverlayOpened(Overlay),
    /// The shell closed the overlay on top (`Pane::overlay_closed`).
    OverlayClosed(Overlay),
}

impl Ev {
    /// Whether the operator gave it: a key (one the shell took too), a
    /// click, a paste, a menu's text into the box, the tab bar's toggle, a
    /// jump or a tab's activation. A refresh, the column hidden for want of
    /// width and an overlay's coming and going are not. Only the operator's
    /// input ends the send-now chord.
    pub(super) fn is_input(&self) -> bool {
        matches!(self, Ev::Key(_) | Ev::KeyTaken | Ev::Click(_) | Ev::Paste | Ev::Compose | Ev::ToggleBar | Ev::Jump(_) | Ev::Numbered { .. } | Ev::TabActivated { .. })
    }
}

/// A left click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Click {
    /// In the pane: whether it is in the column and in the feed, and the
    /// column row it lands on when nothing drawn over the column takes it.
    Pane { column: bool, feed: bool, row: Option<SideHit> },
    /// One the shell took: a tab, the bottom bar, a middle click.
    Shell,
}

/// What a step asks the chat to do, in the order given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Effect {
    /// The key edits the compose box.
    Type(KeyEvent),
    /// The caret up or down a row of the compose box.
    Caret { up: bool },
    /// Enter in the box: send, or run the command.
    Send,
    /// Ctrl+Enter in the box, where the terminal reports it, or Ctrl+X
    /// Ctrl+S in any terminal: send now, to an agent that is quiet too, as
    /// Claude Code's `chat:sendNow` and its chord do.
    SendNow,
    /// Tab in the box on a word to complete: complete it.
    Complete,
    /// Esc in the box: the message selected and the status cleared.
    Clear,
    Page { up: bool },
    /// Show this tab.
    ShowTab(Tab),
    /// The column's list keeps its cursor in view again.
    Follow,
    /// The one activation path; a menu docks under the bar's slot when given.
    Activate { target: Target, how: How, under_tab: Option<usize> },
    /// Enter on the feed: the selected message's menu.
    FeedMenu,
    /// Select the feed's message `i` and bring it into view.
    Reveal(usize),
    /// No message selected.
    Deselect,
    /// The pane is drawn afresh on the next frame.
    Redraw,
}

/// What a step reads of the screen besides the interaction state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct View {
    /// The tab bar and the column, when the world is read.
    pub(super) world: Option<Laid>,
    /// Whether the last frame drew the column.
    pub(super) side_drawn: bool,
    pub(super) draft_empty: bool,
    /// The caret ends a word Tab completes: a command, a subcommand, an
    /// `@agent` or a `#channel`, or the completion Tab last made.
    pub(super) completable: bool,
    /// The caret at the start, or the end, of the draft: ↑ or ↓ has nowhere
    /// left to go in the text.
    pub(super) caret_start: bool,
    pub(super) caret_end: bool,
    pub(super) feed_len: usize,
    pub(super) feed_selected: Option<usize>,
    /// `attend.chat.tabs.menu_on_repeat`.
    pub(super) menu_on_repeat: bool,
    /// The view shown has no compose box: an agent's own DMs, which offer
    /// nothing to send. The ring leaves the box out, and the focus that
    /// would go to it goes to the feed.
    pub(super) read_only: bool,
}

/// The tab bar and the column as the world lays them out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Laid {
    /// The bar's slots in order ([`crate::tabs::bar`]), for the tab shown.
    pub(super) bar: Vec<Slot>,
    /// The column's rows, agents then channels, drawn or not.
    pub(super) rows: Vec<SideHit>,
    /// The tab shown, against the live strip.
    pub(super) shown: Tab,
}

/// The step needs the world, and the view had none: read it and step again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NeedWorld;

/// The interaction state after `ev`, and what to do, in order.
fn step(ix: &Ix, ev: Ev, v: &View) -> Result<(Ix, Vec<Effect>), NeedWorld> {
    let mut s = Step { ix: ix.clone(), fx: Vec::new(), v };
    s.ev(ev)?;
    Ok((s.ix, s.fx))
}

struct Step<'a> {
    ix: Ix,
    fx: Vec<Effect>,
    v: &'a View,
}

type R = Result<(), NeedWorld>;

impl<'a> Step<'a> {
    fn world(&self) -> Result<&'a Laid, NeedWorld> {
        self.v.world.as_ref().ok_or(NeedWorld)
    }

    fn ev(&mut self, ev: Ev) -> R {
        // A view with no compose box holds no focus there, however the view
        // came to be shown under it. An overlay's coming or going moves no
        // focus; the next input does.
        let overlay = matches!(ev, Ev::OverlayOpened(_) | Ev::OverlayClosed(_));
        if self.v.read_only && self.ix.focus == Focus::Compose && !overlay {
            self.set(Focus::Feed);
            self.fx.push(Effect::Redraw);
        }
        // The chord's first key holds for the operator's next input only: a
        // refresh, the column hidden for width or an overlay's coming and
        // going leave it held.
        let chord = ev.is_input() && std::mem::take(&mut self.ix.chord);
        match ev {
            Ev::Key(k) if self.ix.owns_text() => self.compose_key(k, chord),
            Ev::Key(k) => self.nav_key(k),
            Ev::Click(Click::Pane { column, feed, row }) => self.click(column, feed, row),
            Ev::Click(Click::Shell) => {
                if self.ix.focus != Focus::Compose {
                    self.set(Focus::Compose);
                    self.fx.push(Effect::Redraw);
                }
                Ok(())
            }
            Ev::Paste | Ev::Compose => self.go(Focus::Compose),
            Ev::ToggleBar if self.ix.on_bar() => self.go(Focus::Compose),
            Ev::ToggleBar => {
                let shown = self.world()?.shown.clone();
                self.go(Focus::Tab(Slot::Tab(shown)))
            }
            Ev::Jump(i) => {
                if let Some(slot @ Slot::Tab(_)) = self.world()?.bar.get(i).cloned() {
                    self.follow_on_bar(&slot);
                    if let Slot::Tab(t) = slot {
                        self.fx.push(Effect::ShowTab(t));
                    }
                }
                Ok(())
            }
            Ev::Numbered { n, menu } => self.numbered(n, menu),
            Ev::TabActivated { i, menu } => {
                if let Some(slot) = self.world()?.bar.get(i).cloned() {
                    self.follow_on_bar(&slot);
                    let how = if menu { How::Menu } else { How::Select { repeat: self.v.menu_on_repeat } };
                    self.fx.push(Effect::Activate { target: Target::of_slot(&slot), how, under_tab: None });
                }
                Ok(())
            }
            Ev::WorldChanged => self.refocus(),
            Ev::KeyTaken => Ok(()),
            Ev::ColumnHidden => {
                if self.ix.side_focus().is_some() {
                    self.set(Focus::Compose);
                }
                Ok(())
            }
            Ev::OverlayOpened(o) => {
                if !self.ix.overlaid() {
                    self.fx.push(Effect::Redraw);
                }
                self.ix.overlays.push(o);
                Ok(())
            }
            Ev::OverlayClosed(o) => {
                if let Some(i) = self.ix.overlays.iter().rposition(|x| *x == o) {
                    self.ix.overlays.remove(i);
                    if !self.ix.overlaid() {
                        self.fx.push(Effect::Redraw);
                    }
                }
                // The `≡` menu Esc opened closed with the focus still on
                // `≡`: the cycle moves on, to where the focus was.
                if matches!(o, Overlay::Menu { by_esc: true, .. }) && self.ix.focus == Focus::Tab(Slot::Common) {
                    return self.esc_back();
                }
                Ok(())
            }
        }
    }

    /// The focus is `f`, and nothing else is done: what is no longer true
    /// of the state goes with the old focus. A tab is remembered for the
    /// Esc cycle. In a view with no compose box, the box's focus is the
    /// feed's.
    fn set(&mut self, f: Focus) {
        let f = if f == Focus::Compose && self.v.read_only { Focus::Feed } else { f };
        if !matches!(f, Focus::Row(_)) {
            self.ix.row_hit = None;
        }
        if matches!(f, Focus::Tab(_)) {
            self.ix.last_nav = Some((f.clone(), None));
        }
        self.ix.focus = f;
    }

    /// Put the focus on `f`. Walking onto a tab shows nothing: the tab bar
    /// holds the tab shown, and Enter opens a tab's menu.
    fn go(&mut self, f: Focus) -> R {
        match f {
            Focus::Row(i) => self.focus_at(i)?,
            f => self.set(f),
        }
        self.fx.push(Effect::Redraw);
        Ok(())
    }

    /// The focus on row `i`, and the list it is in where the cursor shows.
    /// The row is remembered for the Esc cycle.
    fn focus_at(&mut self, i: usize) -> R {
        let hit = self.world()?.rows.get(i).cloned();
        self.set(Focus::Row(i));
        self.ix.last_nav = Some((Focus::Row(i), hit.clone()));
        self.ix.row_hit = hit;
        self.fx.push(Effect::Follow);
        Ok(())
    }

    /// Esc outside the compose box: the `≡` menu opens, docked under `≡`,
    /// and the focus is on `≡` while it is open. The tab or row the focus
    /// was on stays remembered, so the next Esc comes back to it
    /// ([`Step::esc_back`]).
    fn esc_menu(&mut self) -> R {
        let at = self.world()?.bar.iter().position(|s| *s == Slot::Common);
        self.ix.row_hit = None;
        self.ix.focus = Focus::Tab(Slot::Common);
        self.fx.push(Effect::Activate { target: Target::Common, how: How::Menu, under_tab: at });
        self.fx.push(Effect::Redraw);
        Ok(())
    }

    /// The `≡` menu Esc opened closed: the focus goes back to the tab or
    /// column row it was last on, found by what it is, while that is still
    /// drawn; else to the feed. A row whose agent or channel has gone, or a
    /// column hidden, is no longer drawn.
    fn esc_back(&mut self) -> R {
        let w = self.world()?;
        let back = match self.ix.last_nav.clone() {
            Some((Focus::Tab(slot), _)) if w.bar.contains(&slot) => Some(Focus::Tab(slot)),
            Some((Focus::Row(i), hit)) if self.v.side_drawn => match hit {
                Some(h) => w.rows.iter().position(|r| *r == h),
                None => (i < w.rows.len()).then_some(i),
            }
            .map(Focus::Row),
            _ => None,
        };
        self.go(back.unwrap_or(Focus::Feed))
    }

    /// The column's channel `n`, or with 0 the operator view, chosen as a
    /// click on its row chooses: shown, and with `menu` (Ctrl or Alt) on the
    /// one already shown its menu, as `attend.chat.tabs.menu_on_repeat`
    /// allows. With the focus on a column row it moves to the row chosen.
    fn numbered(&mut self, n: usize, menu: bool) -> R {
        let target = match n {
            0 => Target::Merged,
            n => {
                let w = self.world()?;
                let mut channels = w.rows.iter().enumerate().filter_map(|(i, h)| match h {
                    SideHit::Channel(c) => Some((i, c.clone())),
                    SideHit::Agent(_) | SideHit::Dm(_) | SideHit::Review(_) => None,
                });
                let Some((i, name)) = channels.nth(n - 1) else { return Ok(()) };
                if self.ix.side_focus().is_some() {
                    self.focus_at(i)?;
                }
                Target::Channel(name)
            }
        };
        let how = How::Select { repeat: menu && self.v.menu_on_repeat };
        self.fx.push(Effect::Activate { target, how, under_tab: None });
        self.fx.push(Effect::Redraw);
        Ok(())
    }

    /// A slot chosen by its digit or a click while the focus is on the bar:
    /// the focus goes to that slot, so Enter acts on the tab shown.
    fn follow_on_bar(&mut self, slot: &Slot) {
        if self.ix.on_bar() {
            self.set(Focus::Tab(slot.clone()));
            self.fx.push(Effect::Redraw);
        }
    }

    /// The ring of focusable elements, in order: the tab bar's slots, the
    /// column's rows while it is drawn, the feed, the compose box where the
    /// view has one.
    fn ring(&self) -> Result<Vec<Focus>, NeedWorld> {
        let w = self.world()?;
        let mut out: Vec<Focus> = w.bar.iter().cloned().map(Focus::Tab).collect();
        if self.v.side_drawn {
            out.extend((0..w.rows.len()).map(Focus::Row));
        }
        out.push(Focus::Feed);
        if !self.v.read_only {
            out.push(Focus::Compose);
        }
        Ok(out)
    }

    /// One step along the ring, forward or back, wrapping. Leaving the
    /// compose box onto `≡` is no visit: the Esc cycle keeps the tab or row
    /// remembered before.
    fn walk(&mut self, forward: bool) -> R {
        let ring = self.ring()?;
        let n = ring.len();
        let at = ring.iter().position(|f| *f == self.ix.focus).unwrap_or(n - 1);
        let to = if forward { (at + 1) % n } else { (at + n - 1) % n };
        let kept = (self.ix.focus == Focus::Compose).then(|| self.ix.last_nav.clone());
        self.go(ring[to].clone())?;
        if let Some(last) = kept {
            self.ix.last_nav = last;
        }
        Ok(())
    }

    /// A key while the compose box has the focus: every key edits, but for
    /// the ways out. Tab completes a word that can be completed and
    /// otherwise leaves forward, as Shift+Tab leaves backward. ↑ at the very
    /// top and ↓ at the very end leave a box that holds text; in an empty
    /// box they do nothing (ADR-409). Esc goes to the feed, the draft kept:
    /// the first step of the Esc cycle. Ctrl+X Ctrl+S sends now; after
    /// Ctrl+X (`chord`) any other key drops the chord and does what it does.
    fn compose_key(&mut self, k: KeyEvent, chord: bool) -> R {
        let v = self.v;
        if k.modifiers == KeyModifiers::CONTROL {
            match k.code {
                KeyCode::Char('x') => {
                    self.ix.chord = true;
                    return Ok(());
                }
                KeyCode::Char('s') if chord => {
                    self.fx.push(Effect::SendNow);
                    return Ok(());
                }
                _ => {}
            }
        }
        match k.code {
            KeyCode::Esc => {
                self.fx.push(Effect::Clear);
                return self.go(Focus::Feed);
            }
            KeyCode::Enter if k.modifiers == KeyModifiers::CONTROL => self.fx.push(Effect::SendNow),
            KeyCode::Enter if !k.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => self.fx.push(Effect::Send),
            KeyCode::Tab if v.completable => self.fx.push(Effect::Complete),
            KeyCode::Tab => return self.walk(true),
            KeyCode::BackTab => return self.walk(false),
            KeyCode::Up if !v.caret_start => self.fx.push(Effect::Caret { up: true }),
            KeyCode::Up if !v.draft_empty => return self.walk(false),
            KeyCode::Down if !v.caret_end => self.fx.push(Effect::Caret { up: false }),
            KeyCode::Down if !v.draft_empty => return self.walk(true),
            KeyCode::Up | KeyCode::Down => {}
            KeyCode::PageUp => self.fx.push(Effect::Page { up: true }),
            KeyCode::PageDown => self.fx.push(Effect::Page { up: false }),
            _ => self.fx.push(Effect::Type(k)),
        }
        Ok(())
    }

    /// A key while the focus is outside the compose box.
    fn nav_key(&mut self, k: KeyEvent) -> R {
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match k.code {
            KeyCode::Tab | KeyCode::Right => self.walk(true),
            KeyCode::BackTab | KeyCode::Left => self.walk(false),
            KeyCode::Up => self.vertical(false),
            KeyCode::Down => self.vertical(true),
            KeyCode::Home | KeyCode::End => self.end(k.code == KeyCode::End),
            KeyCode::Enter => self.activate(),
            KeyCode::Esc => self.esc_menu(),
            KeyCode::PageUp => {
                self.fx.push(Effect::Page { up: true });
                Ok(())
            }
            KeyCode::PageDown => {
                self.fx.push(Effect::Page { up: false });
                Ok(())
            }
            // A character is typing: it goes to the compose box. Backspace
            // and Delete only go back to the box, the draft untouched. A
            // view with no box takes no text.
            KeyCode::Char(_) if plain && self.v.read_only => Ok(()),
            KeyCode::Char(_) if plain => {
                self.go(Focus::Compose)?;
                self.fx.push(Effect::Type(k));
                Ok(())
            }
            KeyCode::Backspace | KeyCode::Delete => self.go(Focus::Compose),
            _ => Ok(()),
        }
    }

    /// ↑ or ↓ outside the compose box: along the column, through the feed's
    /// messages, from the tab bar down into the column.
    fn vertical(&mut self, down: bool) -> R {
        match self.ix.focus.clone() {
            Focus::Tab(slot) if down => {
                // Into the column: the row of the tab's channel or DM view,
                // else the first row; past a hidden column, the feed.
                if !self.v.side_drawn {
                    return self.go(Focus::Feed);
                }
                let rows = &self.world()?.rows;
                if rows.is_empty() {
                    return self.go(Focus::Feed);
                }
                let row = match slot {
                    Slot::Tab(Tab::Channel(c)) => rows.iter().position(|h| *h == SideHit::Channel(c.clone())),
                    Slot::Tab(Tab::Dm(m)) => rows.iter().position(|h| *h == SideHit::Dm(m.clone())),
                    Slot::Tab(Tab::Review(m)) => rows.iter().position(|h| *h == SideHit::Review(m.clone())),
                    _ => None,
                };
                self.go(Focus::Row(row.unwrap_or(0)))
            }
            Focus::Tab(_) | Focus::Compose => Ok(()),
            Focus::Row(i) => {
                let last = self.world()?.rows.len().saturating_sub(1);
                match (down, i) {
                    (false, 0) => {
                        let shown = self.world()?.shown.clone();
                        self.go(Focus::Tab(Slot::Tab(shown)))
                    }
                    (false, i) => self.go(Focus::Row(i - 1)),
                    (true, i) if i >= last => self.go(Focus::Feed),
                    (true, i) => self.go(Focus::Row(i + 1)),
                }
            }
            Focus::Feed => self.feed_move(down),
        }
    }

    /// Home or End outside the compose box: the column's first or last row,
    /// the feed's oldest or newest message.
    fn end(&mut self, end: bool) -> R {
        match self.ix.focus {
            Focus::Row(_) => {
                let last = self.world()?.rows.len().saturating_sub(1);
                self.go(Focus::Row(if end { last } else { 0 }))
            }
            Focus::Feed => {
                let n = self.v.feed_len;
                if n > 0 {
                    self.fx.push(Effect::Reveal(if end { n - 1 } else { 0 }));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// ↑ or ↓ in the feed: the selection moves to the older or newer
    /// message, starting from the newest. Past the newest, the compose box.
    fn feed_move(&mut self, down: bool) -> R {
        let n = self.v.feed_len;
        if n == 0 {
            return if down { self.go(Focus::Compose) } else { Ok(()) };
        }
        let to = match (self.v.feed_selected, down) {
            (None, false) => Some(n - 1),
            (None, true) => None,
            (Some(i), false) => Some(i.saturating_sub(1)),
            (Some(i), true) if i + 1 < n => Some(i + 1),
            (Some(_), true) => None,
        };
        match to {
            Some(i) => {
                self.fx.push(Effect::Reveal(i));
                Ok(())
            }
            None => {
                self.fx.push(Effect::Deselect);
                self.go(Focus::Compose)
            }
        }
    }

    /// Enter outside the compose box: the focused element's menu, the one a
    /// right click on it opens, through the one activation path; on the
    /// feed, the selected message's menu. A tab's menu docks under its slot.
    fn activate(&mut self) -> R {
        match self.ix.focus.clone() {
            Focus::Tab(slot) => {
                if let Some(i) = self.world()?.bar.iter().position(|s| *s == slot) {
                    self.fx.push(Effect::Activate { target: Target::of_slot(&slot), how: How::Menu, under_tab: Some(i) });
                }
            }
            Focus::Row(i) => {
                if let Some(hit) = self.world()?.rows.get(i).cloned() {
                    self.fx.push(Effect::Activate { target: Target::of(hit), how: How::Menu, under_tab: None });
                }
            }
            Focus::Feed => self.fx.push(Effect::FeedMenu),
            Focus::Compose => {}
        }
        Ok(())
    }

    /// A left click in the pane. The keyboard goes back to the compose box,
    /// but for a click on the column
    /// while it has the focus and one on the feed while the feed has it. A
    /// click on a column row puts the keyboard on that row, from wherever it
    /// was, so ↑, ↓ and Enter go on from the row clicked, and selects it
    /// ([`ChatPane::activate`]): an agent's row opens its menu; a channel's
    /// row shows its channel, and on the channel shown opens its menu,
    /// whatever `attend.chat.tabs.menu_on_repeat` says, since that setting
    /// is about keys.
    fn click(&mut self, column: bool, feed: bool, row: Option<SideHit>) -> R {
        let keeps = match self.ix.focus {
            Focus::Row(_) => column,
            Focus::Feed => feed,
            _ => false,
        };
        if !keeps {
            self.set(Focus::Compose);
        }
        if let Some(hit) = row {
            if let Some(i) = self.world()?.rows.iter().position(|h| *h == hit) {
                self.focus_at(i)?;
            }
            self.fx.push(Effect::Activate { target: Target::of(hit), how: How::Select { repeat: true }, under_tab: None });
        }
        Ok(())
    }

    /// After the world is read again, the column's cursor follows its row to
    /// where it is now; a DM view's row gone since leaves it on its agent's
    /// row, and any other row gone leaves it at the same place, held within
    /// a list that may have shrunk. A focused tab whose channel is gone moves
    /// to the tab shown, which falls back to `#open` the same way.
    fn refocus(&mut self) -> R {
        if let (Some(i), Some(hit)) = (self.ix.side_focus(), self.ix.row_hit.clone()) {
            let rows = &self.world()?.rows;
            let owner = match &hit {
                SideHit::Dm(m) | SideHit::Review(m) => rows.iter().position(|h| *h == SideHit::Agent(m.clone())),
                _ => None,
            };
            match rows.iter().position(|h| *h == hit) {
                Some(at) => self.ix.focus = Focus::Row(at),
                None if rows.is_empty() => self.set(Focus::Compose),
                None => {
                    let at = owner.unwrap_or(i.min(rows.len() - 1));
                    self.focus_at(at)?;
                }
            }
        }
        if let Focus::Tab(slot) = &self.ix.focus {
            let w = self.world()?;
            if !w.bar.contains(slot) {
                self.ix.focus = Focus::Tab(Slot::Tab(w.shown.clone()));
                self.fx.push(Effect::Redraw);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
