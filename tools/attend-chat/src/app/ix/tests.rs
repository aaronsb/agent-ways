//! The interaction table, one row per (state, event) cell the chat defines,
//! and properties over random sequences. No terminal: [`step`] is pure.

use super::*;

const N: KeyModifiers = KeyModifiers::NONE;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, N)
}

fn ch(c: char) -> KeyEvent {
    key(KeyCode::Char(c))
}

fn chan(name: &str) -> Tab {
    Tab::Channel(name.into())
}

fn agent(id: &str) -> SideHit {
    SideHit::Agent(id.into())
}

fn row(name: &str) -> SideHit {
    SideHit::Channel(name.into())
}

/// `≡ #deploy + #open ⋯all` on the bar; two agents and three channels in
/// the column, drawn; `⋯all` shown; an empty draft; three messages.
fn view() -> View {
    View {
        world: Some(Laid {
            bar: vec![Slot::Common, Slot::Tab(chan("deploy")), Slot::New, Slot::Tab(chan("open")), Slot::Tab(Tab::Merged)],
            rows: vec![agent("a1"), agent("a2"), row("open"), row("deploy"), row("infra")],
            shown: Tab::Merged,
        }),
        side_drawn: true,
        draft_empty: true,
        caret_start: true,
        caret_end: true,
        feed_len: 3,
        feed_selected: None,
        menu_on_repeat: false,
    }
}

/// A draft with the caret in its middle.
fn drafted() -> View {
    View { draft_empty: false, caret_start: false, caret_end: false, ..view() }
}

fn at(focus: Focus) -> Ix {
    Ix { focus, ..Ix::default() }
}

/// On the bar's slot `slot`, walked there from `⋯all`.
fn walked(slot: Slot) -> Ix {
    Ix { focus: Focus::Tab(slot), preview_from: Some(Tab::Merged), ..Ix::default() }
}

fn on_row(i: usize) -> Ix {
    Ix { focus: Focus::Row(i), row_hit: view().world.unwrap().rows.get(i).cloned(), ..Ix::default() }
}

struct Cell {
    name: &'static str,
    ix: Ix,
    ev: Ev,
    view: View,
    want: Ix,
    fx: Vec<Effect>,
}

fn cell(name: &'static str, ix: Ix, ev: Ev, view: View, want: Ix, fx: Vec<Effect>) -> Cell {
    Cell { name, ix, ev, view, want, fx }
}

fn act(target: Target, how: How, under_tab: Option<usize>) -> Effect {
    Effect::Activate { target, how, under_tab }
}

fn check(cells: Vec<Cell>) {
    for c in cells {
        let got = step(&c.ix, c.ev.clone(), &c.view);
        assert_eq!(got, Ok((c.want.clone(), c.fx.clone())), "{}", c.name);
    }
}

#[test]
fn the_compose_box_types_every_key_but_its_ways_out() {
    use Effect::*;
    let compose = at(Focus::Compose);
    let feed = at(Focus::Feed);
    check(vec![
        cell("a character is typed", compose.clone(), Ev::Key(ch('a')), view(), compose.clone(), vec![Type(ch('a'))]),
        cell("q is typed, not asked", compose.clone(), Ev::Key(ch('q')), view(), compose.clone(), vec![Type(ch('q'))]),
        cell("a digit is typed", compose.clone(), Ev::Key(ch('1')), view(), compose.clone(), vec![Type(ch('1'))]),
        cell("Enter sends", compose.clone(), Ev::Key(key(KeyCode::Enter)), view(), compose.clone(), vec![Send]),
        cell(
            "Shift+Enter is a new line",
            compose.clone(),
            Ev::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
            view(),
            compose.clone(),
            vec![Type(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT))],
        ),
        cell("Tab completes", compose.clone(), Ev::Key(key(KeyCode::Tab)), view(), compose.clone(), vec![Complete]),
        cell("Shift+Tab leaves backward", compose.clone(), Ev::Key(key(KeyCode::BackTab)), view(), feed.clone(), vec![Redraw]),
        cell("Esc on an empty box clears and leaves", compose.clone(), Ev::Key(key(KeyCode::Esc)), view(), feed.clone(), vec![Clear, Redraw]),
        cell("Esc over a draft clears and stays", compose.clone(), Ev::Key(key(KeyCode::Esc)), drafted(), compose.clone(), vec![Clear]),
        cell("↑ in an empty box does nothing", compose.clone(), Ev::Key(key(KeyCode::Up)), view(), compose.clone(), vec![]),
        cell("↓ in an empty box does nothing", compose.clone(), Ev::Key(key(KeyCode::Down)), view(), compose.clone(), vec![]),
        cell("↑ in a draft moves the caret", compose.clone(), Ev::Key(key(KeyCode::Up)), drafted(), compose.clone(), vec![Caret { up: true }]),
        cell("↓ in a draft moves the caret", compose.clone(), Ev::Key(key(KeyCode::Down)), drafted(), compose.clone(), vec![Caret { up: false }]),
        cell(
            "↑ at the start of a draft leaves backward",
            compose.clone(),
            Ev::Key(key(KeyCode::Up)),
            View { caret_start: true, ..drafted() },
            feed.clone(),
            vec![Redraw],
        ),
        cell(
            "↓ at the end of a draft leaves forward, round to ≡",
            compose.clone(),
            Ev::Key(key(KeyCode::Down)),
            View { caret_end: true, ..drafted() },
            Ix { focus: Focus::Tab(Slot::Common), preview_from: Some(Tab::Merged), ..Ix::default() },
            vec![Redraw],
        ),
        cell("PgUp pages", compose.clone(), Ev::Key(key(KeyCode::PageUp)), view(), compose.clone(), vec![Page { up: true }]),
        cell("PgDn pages", compose.clone(), Ev::Key(key(KeyCode::PageDown)), view(), compose.clone(), vec![Page { up: false }]),
        cell("Backspace edits", compose.clone(), Ev::Key(key(KeyCode::Backspace)), drafted(), compose.clone(), vec![Type(key(KeyCode::Backspace))]),
    ]);
}

#[test]
fn outside_the_box_the_keys_walk_and_a_character_goes_back_to_type() {
    use Effect::*;
    let compose = at(Focus::Compose);
    let deploy = Slot::Tab(chan("deploy"));
    check(vec![
        cell("Tab from the feed is the box", at(Focus::Feed), Ev::Key(key(KeyCode::Tab)), view(), compose.clone(), vec![Redraw]),
        cell("→ from the feed is the box", at(Focus::Feed), Ev::Key(key(KeyCode::Right)), view(), compose.clone(), vec![Redraw]),
        cell("Shift+Tab from the feed is the last row", at(Focus::Feed), Ev::Key(key(KeyCode::BackTab)), view(), on_row(4), vec![Follow, Redraw]),
        cell(
            "← from the feed past a hidden column is the last tab, shown",
            at(Focus::Feed),
            Ev::Key(key(KeyCode::Left)),
            View { side_drawn: false, ..view() },
            Ix { focus: Focus::Tab(Slot::Tab(Tab::Merged)), preview_from: Some(Tab::Merged), ..Ix::default() },
            vec![ShowTab(Tab::Merged), Redraw],
        ),
        cell("→ along the bar shows the tab", walked(Slot::Common), Ev::Key(key(KeyCode::Right)), view(), walked(deploy.clone()), vec![ShowTab(chan("deploy")), Redraw]),
        cell("→ onto + only points at it", walked(deploy.clone()), Ev::Key(key(KeyCode::Right)), view(), walked(Slot::New), vec![Redraw]),
        cell("Esc on the bar puts the tab back", walked(deploy.clone()), Ev::Key(key(KeyCode::Esc)), view(), compose.clone(), vec![RestoreTab(Tab::Merged), Redraw]),
        cell(
            "a character on the bar puts the tab back, then is typed",
            walked(deploy.clone()),
            Ev::Key(ch('x')),
            view(),
            compose.clone(),
            vec![RestoreTab(Tab::Merged), Redraw, Type(ch('x'))],
        ),
        cell("a character in the feed is typed in the box", at(Focus::Feed), Ev::Key(ch('x')), view(), compose.clone(), vec![Redraw, Type(ch('x'))]),
        cell(
            "a shifted character is a character",
            on_row(1),
            Ev::Key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT)),
            view(),
            compose.clone(),
            vec![Redraw, Type(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT))],
        ),
        cell("Ctrl+x outside the box does nothing", at(Focus::Feed), Ev::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)), view(), at(Focus::Feed), vec![]),
        cell("Backspace goes back to the box, the draft untouched", walked(deploy.clone()), Ev::Key(key(KeyCode::Backspace)), view(), compose.clone(), vec![RestoreTab(Tab::Merged), Redraw]),
        cell("Delete goes back to the box", at(Focus::Feed), Ev::Key(key(KeyCode::Delete)), view(), compose.clone(), vec![Redraw]),
        cell("PgDn pages from the column", on_row(1), Ev::Key(key(KeyCode::PageDown)), view(), on_row(1), vec![Page { up: false }]),
        cell("F5 does nothing", on_row(1), Ev::Key(key(KeyCode::F(5))), view(), on_row(1), vec![]),
    ]);
}

#[test]
fn up_and_down_move_along_the_column_and_the_feed() {
    use Effect::*;
    let compose = at(Focus::Compose);
    check(vec![
        cell(
            "↓ from a channel's tab is its row; the preview is put back",
            walked(Slot::Tab(chan("deploy"))),
            Ev::Key(key(KeyCode::Down)),
            view(),
            on_row(3),
            vec![RestoreTab(Tab::Merged), Follow, Redraw],
        ),
        cell("↓ from ≡ is the first row", walked(Slot::Common), Ev::Key(key(KeyCode::Down)), view(), on_row(0), vec![RestoreTab(Tab::Merged), Follow, Redraw]),
        cell(
            "↓ from the bar past a hidden column is the feed",
            walked(Slot::Common),
            Ev::Key(key(KeyCode::Down)),
            View { side_drawn: false, ..view() },
            at(Focus::Feed),
            vec![RestoreTab(Tab::Merged), Redraw],
        ),
        cell("↑ on the bar does nothing", walked(Slot::Common), Ev::Key(key(KeyCode::Up)), view(), walked(Slot::Common), vec![]),
        cell(
            "↑ from the first row is the tab shown",
            on_row(0),
            Ev::Key(key(KeyCode::Up)),
            view(),
            Ix { focus: Focus::Tab(Slot::Tab(Tab::Merged)), preview_from: Some(Tab::Merged), ..Ix::default() },
            vec![ShowTab(Tab::Merged), Redraw],
        ),
        cell("↑ in the column", on_row(2), Ev::Key(key(KeyCode::Up)), view(), on_row(1), vec![Follow, Redraw]),
        cell("↓ in the column", on_row(2), Ev::Key(key(KeyCode::Down)), view(), on_row(3), vec![Follow, Redraw]),
        cell("↓ from the last row is the feed", on_row(4), Ev::Key(key(KeyCode::Down)), view(), at(Focus::Feed), vec![Redraw]),
        cell("↓ from the last agent is the first channel, as drawn", on_row(1), Ev::Key(key(KeyCode::Down)), view(), on_row(2), vec![Follow, Redraw]),
        cell("↑ from the first channel is the last agent", on_row(2), Ev::Key(key(KeyCode::Up)), view(), on_row(1), vec![Follow, Redraw]),
        cell("Home in the column", on_row(3), Ev::Key(key(KeyCode::Home)), view(), on_row(0), vec![Follow, Redraw]),
        cell("End in the column", on_row(1), Ev::Key(key(KeyCode::End)), view(), on_row(4), vec![Follow, Redraw]),
        cell("↑ into the feed selects the newest", at(Focus::Feed), Ev::Key(key(KeyCode::Up)), view(), at(Focus::Feed), vec![Reveal(2)]),
        cell("↑ in the feed selects the older", at(Focus::Feed), Ev::Key(key(KeyCode::Up)), View { feed_selected: Some(1), ..view() }, at(Focus::Feed), vec![Reveal(0)]),
        cell("↑ at the oldest stays", at(Focus::Feed), Ev::Key(key(KeyCode::Up)), View { feed_selected: Some(0), ..view() }, at(Focus::Feed), vec![Reveal(0)]),
        cell("↓ in the feed selects the newer", at(Focus::Feed), Ev::Key(key(KeyCode::Down)), View { feed_selected: Some(0), ..view() }, at(Focus::Feed), vec![Reveal(1)]),
        cell("↓ past the newest is the box", at(Focus::Feed), Ev::Key(key(KeyCode::Down)), View { feed_selected: Some(2), ..view() }, compose.clone(), vec![Deselect, Redraw]),
        cell("↓ with none selected is the box", at(Focus::Feed), Ev::Key(key(KeyCode::Down)), view(), compose.clone(), vec![Deselect, Redraw]),
        cell("↓ in an empty feed is the box", at(Focus::Feed), Ev::Key(key(KeyCode::Down)), View { feed_len: 0, ..view() }, compose.clone(), vec![Redraw]),
        cell("↑ in an empty feed does nothing", at(Focus::Feed), Ev::Key(key(KeyCode::Up)), View { feed_len: 0, ..view() }, at(Focus::Feed), vec![]),
        cell("Home in the feed is the oldest", at(Focus::Feed), Ev::Key(key(KeyCode::Home)), view(), at(Focus::Feed), vec![Reveal(0)]),
        cell("End in the feed is the newest", at(Focus::Feed), Ev::Key(key(KeyCode::End)), view(), at(Focus::Feed), vec![Reveal(2)]),
        cell("End in an empty feed does nothing", at(Focus::Feed), Ev::Key(key(KeyCode::End)), View { feed_len: 0, ..view() }, at(Focus::Feed), vec![]),
        cell("Home on the bar does nothing", walked(Slot::New), Ev::Key(key(KeyCode::Home)), view(), walked(Slot::New), vec![]),
    ]);
}

#[test]
fn enter_opens_the_focused_elements_menu() {
    use Effect::*;
    let deploy = Slot::Tab(chan("deploy"));
    let chosen = |slot: Slot| Ix { focus: Focus::Tab(slot), ..Ix::default() };
    check(vec![
        cell(
            "Enter on a tab keeps it shown and docks its menu under it",
            walked(deploy.clone()),
            Ev::Key(key(KeyCode::Enter)),
            view(),
            chosen(deploy.clone()),
            vec![act(Target::Channel("deploy".into()), How::Menu, Some(1))],
        ),
        cell("Enter on ≡", walked(Slot::Common), Ev::Key(key(KeyCode::Enter)), view(), chosen(Slot::Common), vec![act(Target::Common, How::Menu, Some(0))]),
        cell("Enter on +", walked(Slot::New), Ev::Key(key(KeyCode::Enter)), view(), chosen(Slot::New), vec![act(Target::NewChannel, How::Menu, Some(2))]),
        cell("Enter on an agent's row", on_row(1), Ev::Key(key(KeyCode::Enter)), view(), on_row(1), vec![act(Target::Agent("a2".into()), How::Menu, None)]),
        cell("Enter on a channel's row", on_row(4), Ev::Key(key(KeyCode::Enter)), view(), on_row(4), vec![act(Target::Channel("infra".into()), How::Menu, None)]),
        cell("Enter on the feed", at(Focus::Feed), Ev::Key(key(KeyCode::Enter)), view(), at(Focus::Feed), vec![FeedMenu]),
        cell(
            "Enter on a slot no longer on the bar keeps the tab and does nothing",
            walked(Slot::Tab(chan("gone"))),
            Ev::Key(key(KeyCode::Enter)),
            view(),
            chosen(Slot::Tab(chan("gone"))),
            vec![],
        ),
    ]);
}

#[test]
fn a_click_takes_the_keyboard_to_the_box_but_where_it_keeps_it() {
    use Effect::*;
    let compose = at(Focus::Compose);
    let pane = |column: bool, feed: bool, row: Option<SideHit>| Ev::Click(Click::Pane { column, feed, row });
    let select = |t: Target| act(t, How::Select { repeat: true }, None);
    check(vec![
        cell("a click off the bar keeps the walk's tab, the preview gone", walked(Slot::Tab(chan("deploy"))), pane(false, false, None), view(), compose.clone(), vec![]),
        cell(
            "a click on a row while the column has the keyboard moves its cursor there and selects it",
            on_row(1),
            pane(true, false, Some(row("deploy"))),
            view(),
            on_row(3),
            vec![Follow, select(Target::Channel("deploy".into()))],
        ),
        cell("a click on a row from the box puts the keyboard on it and selects it", compose.clone(), pane(true, false, Some(agent("a1"))), view(), on_row(0), vec![Follow, select(Target::Agent("a1".into()))]),
        cell("a click on a row from the feed puts the keyboard on it", at(Focus::Feed), pane(true, false, Some(row("infra"))), view(), on_row(4), vec![Follow, select(Target::Channel("infra".into()))]),
        cell(
            "a click on a row from the bar puts the keyboard on it, the walk's tab kept",
            walked(Slot::Tab(chan("deploy"))),
            pane(true, false, Some(row("deploy"))),
            view(),
            on_row(3),
            vec![Follow, select(Target::Channel("deploy".into()))],
        ),
        cell("a click on a row the world no longer lists selects it from the box", compose.clone(), pane(true, false, Some(row("gone"))), view(), compose.clone(), vec![select(Target::Channel("gone".into()))]),
        cell("a click in the column off its rows keeps the column's keyboard", on_row(2), pane(true, false, None), view(), on_row(2), vec![]),
        cell("a click on a message keeps the feed's keyboard", at(Focus::Feed), pane(false, true, None), view(), at(Focus::Feed), vec![]),
        cell("a click on the feed from the column is the box", on_row(2), pane(false, true, None), view(), compose.clone(), vec![]),
        cell("a click the shell takes is the box", walked(Slot::Tab(chan("deploy"))), Ev::Click(Click::Shell), view(), compose.clone(), vec![Redraw]),
        cell("a click the shell takes from the box changes nothing", compose.clone(), Ev::Click(Click::Shell), view(), compose.clone(), vec![]),
    ]);
}

#[test]
fn the_shells_tab_inputs_choose_a_tab() {
    use Effect::*;
    let compose = at(Focus::Compose);
    let chosen = |slot: Slot| Ix { focus: Focus::Tab(slot), ..Ix::default() };
    check(vec![
        cell("a digit from the box shows its tab", compose.clone(), Ev::Jump(1), view(), compose.clone(), vec![ShowTab(chan("deploy"))]),
        cell("a digit on the bar moves the focus to its tab and keeps it", walked(Slot::Common), Ev::Jump(1), view(), chosen(Slot::Tab(chan("deploy"))), vec![Redraw, ShowTab(chan("deploy"))]),
        cell("a digit on a slot does nothing but keep the tab", walked(Slot::Common), Ev::Jump(0), view(), chosen(Slot::Common), vec![]),
        cell("a digit past the bar does nothing", compose.clone(), Ev::Jump(9), view(), compose.clone(), vec![]),
        cell(
            "a tab activated from the box selects it",
            compose.clone(),
            Ev::TabActivated { i: 1, menu: false },
            view(),
            compose.clone(),
            vec![act(Target::Channel("deploy".into()), How::Select { repeat: false }, None)],
        ),
        cell(
            "menu_on_repeat carries into the select",
            compose.clone(),
            Ev::TabActivated { i: 4, menu: false },
            View { menu_on_repeat: true, ..view() },
            compose.clone(),
            vec![act(Target::Merged, How::Select { repeat: true }, None)],
        ),
        cell(
            "a right click on a slot from the bar follows it and opens its menu",
            walked(Slot::Tab(chan("deploy"))),
            Ev::TabActivated { i: 0, menu: true },
            view(),
            chosen(Slot::Common),
            vec![Redraw, act(Target::Common, How::Menu, None)],
        ),
        cell("a paste on the bar puts the tab back and is the box", walked(Slot::Tab(chan("deploy"))), Ev::Paste, view(), compose.clone(), vec![RestoreTab(Tab::Merged), Redraw]),
        cell("a paste in the box stays", compose.clone(), Ev::Paste, view(), compose.clone(), vec![Redraw]),
        cell(
            "F2 from the box is the tab shown",
            compose.clone(),
            Ev::ToggleBar,
            view(),
            Ix { focus: Focus::Tab(Slot::Tab(Tab::Merged)), preview_from: Some(Tab::Merged), ..Ix::default() },
            vec![ShowTab(Tab::Merged), Redraw],
        ),
        cell("F2 on the bar is the box, the tab put back", walked(Slot::Tab(chan("deploy"))), Ev::ToggleBar, view(), compose.clone(), vec![RestoreTab(Tab::Merged), Redraw]),
    ]);
}

#[test]
fn a_refresh_keeps_the_focus_on_what_it_was_on() {
    use Effect::*;
    let with_rows = |rows: Vec<SideHit>| View { world: Some(Laid { rows, ..view().world.unwrap() }), ..view() };
    let gone = Slot::Tab(chan("gone"));
    check(vec![
        cell(
            "a row that moved is followed",
            on_row(1),
            Ev::WorldChanged,
            with_rows(vec![agent("new"), agent("a1"), agent("a2"), row("open")]),
            Ix { focus: Focus::Row(2), row_hit: Some(agent("a2")), ..Ix::default() },
            vec![],
        ),
        cell(
            "a row gone leaves the cursor in place, held within the list",
            on_row(4),
            Ev::WorldChanged,
            with_rows(vec![agent("a1"), row("open")]),
            Ix { focus: Focus::Row(1), row_hit: Some(row("open")), ..Ix::default() },
            vec![Follow],
        ),
        cell("an empty column is the box", on_row(1), Ev::WorldChanged, with_rows(vec![]), at(Focus::Compose), vec![]),
        cell(
            "a tab whose channel is gone moves to the tab shown, the preview kept",
            Ix { focus: Focus::Tab(gone), preview_from: Some(chan("deploy")), ..Ix::default() },
            Ev::WorldChanged,
            view(),
            Ix { focus: Focus::Tab(Slot::Tab(Tab::Merged)), preview_from: Some(chan("deploy")), ..Ix::default() },
            vec![Redraw],
        ),
        cell("a tab still there stays", walked(Slot::Common), Ev::WorldChanged, view(), walked(Slot::Common), vec![]),
        cell("the box stays", at(Focus::Compose), Ev::WorldChanged, view(), at(Focus::Compose), vec![]),
        cell("the column hidden under its row is the box", on_row(3), Ev::ColumnHidden, view(), at(Focus::Compose), vec![]),
        cell("the column hidden under the feed leaves it", at(Focus::Feed), Ev::ColumnHidden, view(), at(Focus::Feed), vec![]),
    ]);
}

#[test]
fn the_overlay_is_what_the_shell_last_reported() {
    use Effect::*;
    let up = Ix { overlay: Some(Overlay::Shell), ..at(Focus::Compose) };
    check(vec![
        cell("up", at(Focus::Compose), Ev::Overlaid(true), view(), up.clone(), vec![Redraw]),
        cell("up again changes nothing", up.clone(), Ev::Overlaid(true), view(), up.clone(), vec![]),
        cell("down", up.clone(), Ev::Overlaid(false), view(), at(Focus::Compose), vec![Redraw]),
        cell("down again changes nothing", at(Focus::Compose), Ev::Overlaid(false), view(), at(Focus::Compose), vec![]),
    ]);
    assert!(up.overlaid() && !at(Focus::Compose).overlaid());
}

#[test]
fn a_step_asks_for_the_world_only_where_it_reads_it() {
    let bare = View { world: None, ..drafted() };
    let need = |ix: Ix, ev: Ev| step(&ix, ev, &bare) == Err(NeedWorld);
    // Typing, the caret, a feed move and Esc off the bar read nothing of it.
    assert!(!need(at(Focus::Compose), Ev::Key(ch('a'))));
    assert!(!need(at(Focus::Compose), Ev::Key(key(KeyCode::Up))));
    assert!(!need(at(Focus::Feed), Ev::Key(key(KeyCode::Up))));
    assert!(!need(walked(Slot::Common), Ev::Key(key(KeyCode::Esc))));
    assert!(!need(at(Focus::Feed), Ev::Click(Click::Pane { column: false, feed: false, row: None })));
    // The ring, the column's rows and the tab shown are the world's.
    assert!(need(at(Focus::Compose), Ev::Key(key(KeyCode::BackTab))));
    assert!(need(on_row(1), Ev::Key(key(KeyCode::Up))));
    assert!(need(at(Focus::Compose), Ev::Click(Click::Pane { column: true, feed: false, row: Some(row("open")) })));
    assert!(need(at(Focus::Compose), Ev::ToggleBar));
    assert!(need(at(Focus::Compose), Ev::Jump(1)));
}

/// A small, seeded generator: the properties replay exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn coin(&mut self) -> bool {
        self.next() & 1 == 0
    }
}

fn any_key(r: &mut Rng) -> KeyEvent {
    let codes = [
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::F(5),
        KeyCode::Char('a'),
        KeyCode::Char('q'),
        KeyCode::Char('1'),
        KeyCode::Char(' '),
    ];
    let mods = [KeyModifiers::NONE, KeyModifiers::NONE, KeyModifiers::NONE, KeyModifiers::SHIFT, KeyModifiers::CONTROL, KeyModifiers::ALT];
    KeyEvent::new(codes[r.below(codes.len())], mods[r.below(mods.len())])
}

fn any_view(r: &mut Rng) -> View {
    let names = ["deploy", "infra", "ops"];
    let mut bar = vec![Slot::Common];
    bar.extend(names.iter().take(r.below(4)).map(|n| Slot::Tab(chan(n))));
    bar.extend([Slot::New, Slot::Tab(chan("open")), Slot::Tab(Tab::Merged)]);
    let mut rows: Vec<SideHit> = ["a1", "a2", "a3"].iter().take(r.below(4)).map(|a| agent(a)).collect();
    rows.extend(["open", "deploy", "infra"].iter().take(r.below(4)).map(|c| row(c)));
    let shown = [Tab::Merged, chan("open"), chan("deploy")][r.below(3)].clone();
    let draft_empty = r.below(3) == 0;
    let (caret_start, caret_end) = if draft_empty { (true, true) } else { (r.coin(), r.coin()) };
    let feed_len = r.below(4);
    View {
        world: (r.below(5) != 0).then_some(Laid { bar, rows, shown }),
        side_drawn: r.below(4) != 0,
        draft_empty,
        caret_start,
        caret_end,
        feed_len,
        feed_selected: (feed_len > 0 && r.coin()).then(|| r.below(feed_len)),
        menu_on_repeat: r.coin(),
    }
}

fn any_ev(r: &mut Rng, v: &View) -> Ev {
    let rows = v.world.as_ref().map(|w| w.rows.clone()).unwrap_or_default();
    match r.below(20) {
        0..=10 => Ev::Key(any_key(r)),
        11 => Ev::Click(Click::Pane { column: r.coin(), feed: r.coin(), row: (!rows.is_empty() && r.coin()).then(|| rows[r.below(rows.len())].clone()) }),
        12 => Ev::Click(Click::Shell),
        13 => Ev::Paste,
        14 => Ev::ToggleBar,
        15 => Ev::Jump(r.below(8)),
        16 => Ev::TabActivated { i: r.below(8), menu: r.coin() },
        17 => Ev::WorldChanged,
        18 => Ev::ColumnHidden,
        _ => Ev::Overlaid(r.coin()),
    }
}

/// Every sequence: step on a view without the world first, as the
/// dispatcher does, and with it when the step asks.
fn run(seed: u64, steps: usize, mut each: impl FnMut(&Ix, &Ev, &View, &Ix, &[Effect])) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut ix = Ix::default();
    for _ in 0..steps {
        let mut v = any_view(&mut r);
        let ev = any_ev(&mut r, &v);
        let (next, fx) = match step(&ix, ev.clone(), &v) {
            Ok(done) => done,
            Err(NeedWorld) => {
                v.world = any_view(&mut Rng(r.next() | 1)).world.or_else(|| view().world);
                step(&ix, ev.clone(), &v).expect("the world given")
            }
        };
        each(&ix, &ev, &v, &next, &fx);
        ix = next;
    }
}

#[test]
fn the_preview_is_never_held_off_the_bar_nor_a_row_off_the_column() {
    for seed in 1..=300 {
        run(seed, 400, |_, ev, _, ix, _| {
            assert!(ix.on_bar() || ix.preview_from.is_none(), "seed {seed}: {ev:?} left {ix:?}");
            assert!(matches!(ix.focus, Focus::Row(_)) || ix.row_hit.is_none(), "seed {seed}: {ev:?} left {ix:?}");
        });
    }
}

#[test]
fn the_overlay_mirrors_what_the_shell_reported_last() {
    for seed in 1..=300 {
        let mut shell = false;
        run(seed, 400, |_, ev, _, ix, _| {
            if let Ev::Overlaid(on) = ev {
                shell = *on;
            }
            assert_eq!(ix.overlaid(), shell, "seed {seed}: {ev:?}");
        });
    }
}

#[test]
fn a_printable_key_lands_only_in_the_compose_box() {
    for seed in 1..=300 {
        run(seed, 400, |before, ev, _, ix, fx| {
            let Ev::Key(k) = ev else { return };
            let typed: Vec<&Effect> = fx.iter().filter(|e| matches!(e, Effect::Type(_) | Effect::Send | Effect::Complete)).collect();
            let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
            match k.code {
                KeyCode::Char(_) if plain || before.owns_text() => {
                    // In the box, or back to it first: typed there, last, once.
                    assert_eq!(ix.focus, Focus::Compose, "seed {seed}: {k:?} from {before:?}");
                    assert_eq!(fx.last(), Some(&Effect::Type(*k)), "seed {seed}: {k:?} from {before:?}: {fx:?}");
                    assert_eq!(typed.len(), 1, "seed {seed}: {fx:?}");
                    let restored: Vec<&Effect> = fx.iter().filter(|e| matches!(e, Effect::RestoreTab(_))).collect();
                    match &before.preview_from {
                        Some(t) if before.on_bar() => assert_eq!(restored, [&Effect::RestoreTab(t.clone())], "seed {seed}: the walk's tab first"),
                        _ => assert!(restored.is_empty(), "seed {seed}: {fx:?}"),
                    }
                }
                // Outside the box, no other key types.
                _ if !before.owns_text() => assert!(typed.is_empty(), "seed {seed}: {k:?} from {before:?}: {fx:?}"),
                _ => {}
            }
        });
    }
}

#[test]
fn a_step_that_runs_without_the_world_decides_as_it_would_with_it() {
    for seed in 1..=300 {
        let mut r = Rng(seed | 1);
        run(seed, 200, |before, ev, v, _, _| {
            let mut bare = v.clone();
            bare.world = None;
            if let Ok(done) = step(before, ev.clone(), &bare) {
                let mut full = v.clone();
                full.world = any_view(&mut r).world.or_else(|| view().world);
                assert_eq!(step(before, ev.clone(), &full), Ok(done), "seed {seed}: {ev:?} from {before:?}");
            }
        });
    }
}
