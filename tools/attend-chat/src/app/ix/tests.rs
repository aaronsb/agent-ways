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

/// `≡ #deploy + #open ⋯all` on the bar, `#deploy` shown; two agents and
/// three channels in the column, drawn; an empty draft; three messages.
fn view() -> View {
    View {
        world: Some(Laid {
            bar: vec![Slot::Common, Slot::Tab(chan("deploy")), Slot::New, Slot::Tab(chan("open")), Slot::Tab(Tab::Merged)],
            rows: vec![agent("a1"), agent("a2"), row("open"), row("deploy"), row("infra")],
            shown: chan("deploy"),
        }),
        side_drawn: true,
        draft_empty: true,
        completable: false,
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

/// On the bar's slot `slot`, remembered for the Esc cycle.
fn tab(slot: Slot) -> Ix {
    Ix { focus: Focus::Tab(slot.clone()), last_nav: Some((Focus::Tab(slot), None)), ..Ix::default() }
}

/// On the column's row `i`, remembered for the Esc cycle.
fn on_row(i: usize) -> Ix {
    let hit = view().world.unwrap().rows.get(i).cloned();
    Ix { focus: Focus::Row(i), row_hit: hit.clone(), last_nav: Some((Focus::Row(i), hit)), ..Ix::default() }
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

/// What a cell of the table asserts of the state: where the focus is, the
/// row under it and the overlays. The Esc cycle's memory has cells of its
/// own ([`check_all`]).
fn seen(ix: &Ix) -> (Focus, Option<SideHit>, Vec<Overlay>) {
    (ix.focus.clone(), ix.row_hit.clone(), ix.overlays.clone())
}

fn check(cells: Vec<Cell>) {
    for c in cells {
        let got = step(&c.ix, c.ev.clone(), &c.view).map(|(ix, fx)| (seen(&ix), fx));
        assert_eq!(got, Ok((seen(&c.want), c.fx.clone())), "{}", c.name);
    }
}

/// As [`check`], the whole state compared, the Esc cycle's memory with it.
fn check_all(cells: Vec<Cell>) {
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
        cell("Tab on a word to complete completes", compose.clone(), Ev::Key(key(KeyCode::Tab)), View { completable: true, ..drafted() }, compose.clone(), vec![Complete]),
        cell("Tab with nothing to complete leaves forward, round to ≡", compose.clone(), Ev::Key(key(KeyCode::Tab)), view(), tab(Slot::Common), vec![Redraw]),
        cell("Tab after plain text leaves forward", compose.clone(), Ev::Key(key(KeyCode::Tab)), drafted(), tab(Slot::Common), vec![Redraw]),
        cell("Shift+Tab leaves backward", compose.clone(), Ev::Key(key(KeyCode::BackTab)), view(), feed.clone(), vec![Redraw]),
        cell("Shift+Tab leaves backward over a word to complete", compose.clone(), Ev::Key(key(KeyCode::BackTab)), View { completable: true, ..drafted() }, feed.clone(), vec![Redraw]),
        cell("Esc on an empty box clears and goes to the feed", compose.clone(), Ev::Key(key(KeyCode::Esc)), view(), feed.clone(), vec![Clear, Redraw]),
        cell("Esc over a draft clears and goes to the feed, the draft kept", compose.clone(), Ev::Key(key(KeyCode::Esc)), drafted(), feed.clone(), vec![Clear, Redraw]),
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
            tab(Slot::Common),
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
            "← from the feed past a hidden column is the last tab, shown nothing",
            at(Focus::Feed),
            Ev::Key(key(KeyCode::Left)),
            View { side_drawn: false, ..view() },
            tab(Slot::Tab(Tab::Merged)),
            vec![Redraw],
        ),
        cell("→ along the bar points at the tab and shows nothing", tab(Slot::Common), Ev::Key(key(KeyCode::Right)), view(), tab(deploy.clone()), vec![Redraw]),
        cell("→ onto #open shows nothing either", tab(Slot::New), Ev::Key(key(KeyCode::Right)), view(), tab(Slot::Tab(chan("open"))), vec![Redraw]),
        cell("→ onto + only points at it", tab(deploy.clone()), Ev::Key(key(KeyCode::Right)), view(), tab(Slot::New), vec![Redraw]),
        cell("a character on the bar is typed in the box", tab(deploy.clone()), Ev::Key(ch('x')), view(), compose.clone(), vec![Redraw, Type(ch('x'))]),
        cell("a digit on the bar is typed in the box", tab(deploy.clone()), Ev::Key(ch('3')), view(), compose.clone(), vec![Redraw, Type(ch('3'))]),
        cell("q in the feed is typed in the box", at(Focus::Feed), Ev::Key(ch('q')), view(), compose.clone(), vec![Redraw, Type(ch('q'))]),
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
        cell("Backspace goes back to the box, the draft untouched", tab(deploy.clone()), Ev::Key(key(KeyCode::Backspace)), view(), compose.clone(), vec![Redraw]),
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
        cell("↓ from a channel's tab is its row", tab(Slot::Tab(chan("deploy"))), Ev::Key(key(KeyCode::Down)), view(), on_row(3), vec![Follow, Redraw]),
        cell("↓ from ≡ is the first row", tab(Slot::Common), Ev::Key(key(KeyCode::Down)), view(), on_row(0), vec![Follow, Redraw]),
        cell("↓ from the bar past a hidden column is the feed", tab(Slot::Common), Ev::Key(key(KeyCode::Down)), View { side_drawn: false, ..view() }, at(Focus::Feed), vec![Redraw]),
        cell("↑ on the bar does nothing", tab(Slot::Common), Ev::Key(key(KeyCode::Up)), view(), tab(Slot::Common), vec![]),
        cell("↑ from the first row is the tab shown", on_row(0), Ev::Key(key(KeyCode::Up)), view(), tab(Slot::Tab(chan("deploy"))), vec![Redraw]),
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
        cell("Home on the bar does nothing", tab(Slot::New), Ev::Key(key(KeyCode::Home)), view(), tab(Slot::New), vec![]),
    ]);
}

#[test]
fn enter_opens_the_focused_elements_menu() {
    use Effect::*;
    let deploy = Slot::Tab(chan("deploy"));
    check(vec![
        cell("Enter on a tab docks its menu under it", tab(deploy.clone()), Ev::Key(key(KeyCode::Enter)), view(), tab(deploy.clone()), vec![act(Target::Channel("deploy".into()), How::Menu, Some(1))]),
        cell("Enter on #open opens its menu, shows nothing", tab(Slot::Tab(chan("open"))), Ev::Key(key(KeyCode::Enter)), view(), tab(Slot::Tab(chan("open"))), vec![act(Target::Channel("open".into()), How::Menu, Some(3))]),
        cell("Enter on ≡", tab(Slot::Common), Ev::Key(key(KeyCode::Enter)), view(), tab(Slot::Common), vec![act(Target::Common, How::Menu, Some(0))]),
        cell("Enter on +", tab(Slot::New), Ev::Key(key(KeyCode::Enter)), view(), tab(Slot::New), vec![act(Target::NewChannel, How::Menu, Some(2))]),
        cell("Enter on an agent's row", on_row(1), Ev::Key(key(KeyCode::Enter)), view(), on_row(1), vec![act(Target::Agent("a2".into()), How::Menu, None)]),
        cell("Enter on a channel's row", on_row(4), Ev::Key(key(KeyCode::Enter)), view(), on_row(4), vec![act(Target::Channel("infra".into()), How::Menu, None)]),
        cell("Enter on the feed", at(Focus::Feed), Ev::Key(key(KeyCode::Enter)), view(), at(Focus::Feed), vec![FeedMenu]),
        cell("Enter on a slot no longer on the bar does nothing", tab(Slot::Tab(chan("gone"))), Ev::Key(key(KeyCode::Enter)), view(), tab(Slot::Tab(chan("gone"))), vec![]),
    ]);
}

#[test]
fn a_click_takes_the_keyboard_to_the_box_but_where_it_keeps_it() {
    use Effect::*;
    let compose = at(Focus::Compose);
    let pane = |column: bool, feed: bool, row: Option<SideHit>| Ev::Click(Click::Pane { column, feed, row });
    let select = |t: Target| act(t, How::Select { repeat: true }, None);
    check(vec![
        cell("a click off the bar is the box", tab(Slot::Tab(chan("deploy"))), pane(false, false, None), view(), compose.clone(), vec![]),
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
            "a click on a row from the bar puts the keyboard on it",
            tab(Slot::Tab(chan("deploy"))),
            pane(true, false, Some(row("deploy"))),
            view(),
            on_row(3),
            vec![Follow, select(Target::Channel("deploy".into()))],
        ),
        cell("a click on a row the world no longer lists selects it from the box", compose.clone(), pane(true, false, Some(row("gone"))), view(), compose.clone(), vec![select(Target::Channel("gone".into()))]),
        cell("a click in the column off its rows keeps the column's keyboard", on_row(2), pane(true, false, None), view(), on_row(2), vec![]),
        cell("a click on a message keeps the feed's keyboard", at(Focus::Feed), pane(false, true, None), view(), at(Focus::Feed), vec![]),
        cell("a click on the feed from the column is the box", on_row(2), pane(false, true, None), view(), compose.clone(), vec![]),
        cell("a click the shell takes is the box", tab(Slot::Tab(chan("deploy"))), Ev::Click(Click::Shell), view(), compose.clone(), vec![Redraw]),
        cell("a click the shell takes from the box changes nothing", compose.clone(), Ev::Click(Click::Shell), view(), compose.clone(), vec![]),
    ]);
}

#[test]
fn the_shells_tab_inputs_choose_a_tab() {
    use Effect::*;
    let compose = at(Focus::Compose);
    check(vec![
        cell("the shell showing a tab from the box shows it", compose.clone(), Ev::Jump(3), view(), compose.clone(), vec![ShowTab(chan("open"))]),
        cell("the shell showing a tab on the bar moves the focus to it", tab(Slot::Common), Ev::Jump(3), view(), tab(Slot::Tab(chan("open"))), vec![Redraw, ShowTab(chan("open"))]),
        cell("the shell showing a slot does nothing", tab(Slot::Common), Ev::Jump(0), view(), tab(Slot::Common), vec![]),
        cell("the shell showing past the bar does nothing", compose.clone(), Ev::Jump(9), view(), compose.clone(), vec![]),
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
            tab(Slot::Tab(chan("deploy"))),
            Ev::TabActivated { i: 0, menu: true },
            view(),
            tab(Slot::Common),
            vec![Redraw, act(Target::Common, How::Menu, None)],
        ),
        cell("a paste on the bar is the box", tab(Slot::Tab(chan("deploy"))), Ev::Paste, view(), compose.clone(), vec![Redraw]),
        cell("a paste in the box stays", compose.clone(), Ev::Paste, view(), compose.clone(), vec![Redraw]),
        cell("a menu item that writes in the box takes the focus there", at(Focus::Feed), Ev::Compose, view(), compose.clone(), vec![Redraw]),
        cell("F2 from the box is the tab shown", compose.clone(), Ev::ToggleBar, view(), tab(Slot::Tab(chan("deploy"))), vec![Redraw]),
        cell("F2 on the bar is the box", tab(Slot::Tab(chan("deploy"))), Ev::ToggleBar, view(), compose.clone(), vec![Redraw]),
    ]);
}

/// The column numbers its channels from 1 (agents carry none), and 0 is the
/// operator view: Ctrl or Alt with a digit from anywhere, a plain digit on a
/// column row. A number picks as a click on the row picks.
#[test]
fn the_columns_numbers_pick_a_channel() {
    use Effect::*;
    let compose = at(Focus::Compose);
    let pick = |name: &str, repeat: bool| act(Target::Channel(name.into()), How::Select { repeat }, None);
    let repeat = View { menu_on_repeat: true, ..view() };
    check(vec![
        cell("1 is the first channel, #open", compose.clone(), Ev::Numbered { n: 1, menu: true }, view(), compose.clone(), vec![pick("open", false), Redraw]),
        cell("3 is the third channel, past the agents", compose.clone(), Ev::Numbered { n: 3, menu: false }, view(), compose.clone(), vec![pick("infra", false), Redraw]),
        cell("Ctrl+digit asks for the menu on a repeat where menu_on_repeat allows", compose.clone(), Ev::Numbered { n: 2, menu: true }, repeat.clone(), compose.clone(), vec![pick("deploy", true), Redraw]),
        cell("a plain digit never asks for the menu", on_row(0), Ev::Numbered { n: 2, menu: false }, repeat.clone(), on_row(3), vec![Follow, pick("deploy", false), Redraw]),
        cell("0 is the operator view", compose.clone(), Ev::Numbered { n: 0, menu: false }, view(), compose.clone(), vec![act(Target::Merged, How::Select { repeat: false }, None), Redraw]),
        cell("a number past the channels does nothing", compose.clone(), Ev::Numbered { n: 4, menu: true }, view(), compose.clone(), vec![]),
        cell("on a column row the focus moves to the row picked", on_row(1), Ev::Numbered { n: 3, menu: false }, view(), on_row(4), vec![Follow, pick("infra", false), Redraw]),
        cell("on the bar the focus stays", tab(Slot::New), Ev::Numbered { n: 1, menu: false }, view(), tab(Slot::New), vec![pick("open", false), Redraw]),
        cell("in the feed the focus stays", at(Focus::Feed), Ev::Numbered { n: 1, menu: false }, view(), at(Focus::Feed), vec![pick("open", false), Redraw]),
    ]);
}

fn esc_menu(by_esc: bool) -> Overlay {
    Overlay::Menu { id: "common".into(), by_esc }
}

/// The Esc cycle: the box, the feed, the `≡` menu, the tab or column row
/// the focus was last on (the feed when there is none), the `≡` menu again.
/// The whole state is compared, the cycle's memory with it.
#[test]
fn esc_moves_along_one_cycle() {
    use Effect::*;
    let menu_fx = || vec![act(Target::Common, How::Menu, Some(0)), Redraw];
    // The focus on ≡ while the menu Esc opened is up, `from` remembered.
    let on_menu = |from: Option<(Focus, Option<SideHit>)>| Ix { focus: Focus::Tab(Slot::Common), last_nav: from, overlays: vec![esc_menu(true)], ..Ix::default() };
    let row3 = on_row(3).last_nav;
    let with_rows = |rows: Vec<SideHit>| View { world: Some(Laid { rows, ..view().world.unwrap() }), ..view() };
    check_all(vec![
        cell("the box: Esc is the feed", at(Focus::Compose), Ev::Key(key(KeyCode::Esc)), drafted(), at(Focus::Feed), vec![Clear, Redraw]),
        cell("the feed: Esc opens ≡, the focus on it", at(Focus::Feed), Ev::Key(key(KeyCode::Esc)), view(), at(Focus::Tab(Slot::Common)), menu_fx()),
        cell(
            "the menu Esc opened closes: back to the feed when no tab or row was focused",
            on_menu(None),
            Ev::OverlayClosed(esc_menu(true)),
            view(),
            at(Focus::Feed),
            vec![Redraw, Redraw],
        ),
        cell(
            "a row: Esc opens ≡, the row remembered",
            on_row(3),
            Ev::Key(key(KeyCode::Esc)),
            view(),
            Ix { focus: Focus::Tab(Slot::Common), last_nav: row3.clone(), ..Ix::default() },
            menu_fx(),
        ),
        cell("the menu closes: back to the row", on_menu(row3.clone()), Ev::OverlayClosed(esc_menu(true)), view(), on_row(3), vec![Redraw, Follow, Redraw]),
        cell(
            "back to the row where a refresh has moved it",
            on_menu(row3.clone()),
            Ev::OverlayClosed(esc_menu(true)),
            with_rows(vec![agent("a1"), row("deploy"), row("open")]),
            Ix { focus: Focus::Row(1), row_hit: Some(row("deploy")), last_nav: Some((Focus::Row(1), Some(row("deploy")))), ..Ix::default() },
            vec![Redraw, Follow, Redraw],
        ),
        cell(
            "back to the feed when the remembered channel has gone",
            on_menu(row3.clone()),
            Ev::OverlayClosed(esc_menu(true)),
            with_rows(vec![agent("a1"), agent("a2"), row("open"), row("infra")]),
            Ix { focus: Focus::Feed, last_nav: row3.clone(), ..Ix::default() },
            vec![Redraw, Redraw],
        ),
        cell(
            "back to the feed when the remembered agent has gone, not to the row now at its place",
            on_menu(on_row(1).last_nav),
            Ev::OverlayClosed(esc_menu(true)),
            with_rows(vec![agent("a1"), row("open"), row("deploy"), row("infra")]),
            Ix { focus: Focus::Feed, last_nav: on_row(1).last_nav, ..Ix::default() },
            vec![Redraw, Redraw],
        ),
        cell(
            "back to the feed when the column is hidden",
            on_menu(row3.clone()),
            Ev::OverlayClosed(esc_menu(true)),
            View { side_drawn: false, ..view() },
            Ix { focus: Focus::Feed, last_nav: row3.clone(), ..Ix::default() },
            vec![Redraw, Redraw],
        ),
        cell(
            "a tab: Esc opens ≡, the tab remembered",
            tab(Slot::Tab(chan("open"))),
            Ev::Key(key(KeyCode::Esc)),
            view(),
            Ix { focus: Focus::Tab(Slot::Common), last_nav: tab(Slot::Tab(chan("open"))).last_nav, ..Ix::default() },
            menu_fx(),
        ),
        cell("the menu closes: back to the tab", on_menu(tab(Slot::Tab(chan("open"))).last_nav), Ev::OverlayClosed(esc_menu(true)), view(), tab(Slot::Tab(chan("open"))), vec![Redraw, Redraw]),
        cell(
            "back to the feed when the tab is no longer on the bar",
            on_menu(tab(Slot::Tab(chan("gone"))).last_nav),
            Ev::OverlayClosed(esc_menu(true)),
            view(),
            Ix { focus: Focus::Feed, last_nav: tab(Slot::Tab(chan("gone"))).last_nav, ..Ix::default() },
            vec![Redraw, Redraw],
        ),
        cell("≡ itself: Esc opens its menu", tab(Slot::Common), Ev::Key(key(KeyCode::Esc)), view(), tab(Slot::Common), menu_fx()),
        cell(
            "a menu Enter opened closes: the focus stays on ≡",
            Ix { overlays: vec![esc_menu(false)], ..tab(Slot::Common) },
            Ev::OverlayClosed(esc_menu(false)),
            view(),
            tab(Slot::Common),
            vec![Redraw],
        ),
        cell(
            "the menu Esc opened closes after a click moved the focus: it stays where the click put it",
            Ix { overlays: vec![esc_menu(true)], ..on_row(2) },
            Ev::OverlayClosed(esc_menu(true)),
            view(),
            on_row(2),
            vec![Redraw],
        ),
    ]);
}

/// The views the mirror runs over: the usual one; the column hidden; no
/// agents; `#open` shown and the operator view shown, so the bar holds no
/// channel tab; and a single channel, `#open`.
fn mirror_views() -> Vec<(&'static str, View)> {
    let base = view();
    let laid = base.world.clone().unwrap();
    let reserved = vec![Slot::Common, Slot::New, Slot::Tab(chan("open")), Slot::Tab(Tab::Merged)];
    let with = |laid: Laid| View { world: Some(laid), ..view() };
    vec![
        ("the usual view", base.clone()),
        ("the column hidden", View { side_drawn: false, ..view() }),
        ("no agents", with(Laid { rows: vec![row("open"), row("deploy"), row("infra")], ..laid.clone() })),
        ("#open shown", with(Laid { bar: reserved.clone(), shown: chan("open"), ..laid.clone() })),
        ("the operator view shown", with(Laid { bar: reserved.clone(), shown: Tab::Merged, ..laid.clone() })),
        ("a single channel", with(Laid { bar: reserved, rows: vec![row("open")], shown: Tab::Merged })),
    ]
}

/// Tab and Shift+Tab are mirrors around the one ring: from every element,
/// N steps forward then N back come home, for every N round the ring, in
/// every view.
#[test]
fn tab_and_shift_tab_are_mirrors_from_every_element() {
    for (name, v) in mirror_views() {
        let w = v.world.clone().unwrap();
        let mut starts: Vec<Ix> = w.bar.iter().cloned().map(tab).collect();
        if v.side_drawn {
            starts.extend((0..w.rows.len()).map(|i| Ix { focus: Focus::Row(i), row_hit: w.rows.get(i).cloned(), ..Ix::default() }));
        }
        starts.extend([at(Focus::Feed), at(Focus::Compose)]);
        let len = starts.len();
        for start in &starts {
            for n in 1..=len {
                let mut ix = start.clone();
                let mut path = vec![ix.focus.clone()];
                for code in std::iter::repeat_n(KeyCode::Tab, n).chain(std::iter::repeat_n(KeyCode::BackTab, n)) {
                    ix = step(&ix, Ev::Key(key(code)), &v).expect("the world is read").0;
                    path.push(ix.focus.clone());
                }
                assert_eq!(ix.focus, start.focus, "{name}: {n} × Tab, then {n} × Shift+Tab from {:?}: {path:?}", start.focus);
                assert_eq!(path[n..].iter().rev().collect::<Vec<_>>(), path[..=n].iter().collect::<Vec<_>>(), "{name}: the way back retraces the way out");
            }
        }
        if name == "the usual view" {
            assert_eq!(len, 12, "five slots, five rows, the feed and the box");
        }
    }
}

/// With a word to complete in the box, Tab there completes and the focus
/// stays: the mirror holds round the ring, not through a box whose Tab is
/// completing. Once Tab has nothing more to complete it leaves, and the
/// mirror holds from there.
#[test]
fn tab_in_the_box_completes_before_it_walks() {
    let completing = View { completable: true, ..drafted() };
    let (ix, fx) = step(&at(Focus::Compose), Ev::Key(key(KeyCode::Tab)), &completing).unwrap();
    assert_eq!((ix.focus, fx), (Focus::Compose, vec![Effect::Complete]));
    let done = drafted();
    let (ix, _) = step(&at(Focus::Compose), Ev::Key(key(KeyCode::Tab)), &done).unwrap();
    assert_eq!(ix.focus, Focus::Tab(Slot::Common), "nothing more to complete: Tab leaves");
    let (back, _) = step(&ix, Ev::Key(key(KeyCode::BackTab)), &done).unwrap();
    assert_eq!(back.focus, Focus::Compose, "and Shift+Tab comes back");
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
        cell("a tab no longer on the bar moves to the tab shown", tab(gone), Ev::WorldChanged, view(), tab(Slot::Tab(chan("deploy"))), vec![Redraw]),
        cell("a tab still there stays", tab(Slot::Common), Ev::WorldChanged, view(), tab(Slot::Common), vec![]),
        cell("the box stays", at(Focus::Compose), Ev::WorldChanged, view(), at(Focus::Compose), vec![]),
        cell("the column hidden under its row is the box", on_row(3), Ev::ColumnHidden, view(), at(Focus::Compose), vec![]),
        cell("the column hidden under the feed leaves it", at(Focus::Feed), Ev::ColumnHidden, view(), at(Focus::Feed), vec![]),
    ]);
}

fn menu(id: &str) -> Overlay {
    Overlay::Menu { id: id.into(), by_esc: false }
}

#[test]
fn the_overlays_are_what_the_shell_opened_and_has_not_closed() {
    use Effect::*;
    let with = |o: &[Overlay]| Ix { overlays: o.to_vec(), ..at(Focus::Compose) };
    let guard = [Overlay::Entry { id: "new-channel".into() }, Overlay::Guard];
    check(vec![
        cell("a menu opens", at(Focus::Compose), Ev::OverlayOpened(menu("common")), view(), with(&[menu("common")]), vec![Redraw]),
        cell("another over it", with(&[menu("common")]), Ev::OverlayOpened(menu("theme")), view(), with(&[menu("common"), menu("theme")]), vec![]),
        cell("the top closes", with(&[menu("common"), menu("theme")]), Ev::OverlayClosed(menu("theme")), view(), with(&[menu("common")]), vec![]),
        cell("the last closes", with(&[menu("common")]), Ev::OverlayClosed(menu("common")), view(), at(Focus::Compose), vec![Redraw]),
        cell("the guard over an entry", with(&guard[..1]), Ev::OverlayOpened(Overlay::Guard), view(), with(&guard), vec![]),
        cell("a close of what is not open changes nothing", at(Focus::Compose), Ev::OverlayClosed(Overlay::Help), view(), at(Focus::Compose), vec![]),
        cell("an overlay leaves the focus where it is", at(Focus::Feed), Ev::OverlayOpened(Overlay::Help), view(), Ix { overlays: vec![Overlay::Help], ..at(Focus::Feed) }, vec![Redraw]),
    ]);
    assert!(with(&[Overlay::Help]).overlaid() && !at(Focus::Compose).overlaid());
}

/// The shell's routing follows the focus: the box and the feed take every
/// plain key as text, a column row takes the digits as the column's
/// numbers, and the chat keeps the bar's focus and Esc.
#[test]
fn the_keymap_context_follows_the_focus() {
    let ctx = |f: Focus| at(f).ctx();
    let chat = Ctx { text: true, digits: false, owns_bar: true, esc_quits: false };
    assert_eq!(ctx(Focus::Compose), chat);
    assert_eq!(ctx(Focus::Feed), chat, "the feed is the chat: q, m, ? and the digits are typed");
    for slot in [Slot::Common, Slot::New, Slot::Tab(chan("deploy")), Slot::Tab(Tab::Merged)] {
        assert_eq!(ctx(Focus::Tab(slot)), Ctx { text: false, digits: false, owns_bar: true, esc_quits: false }, "a digit on the bar is typed");
    }
    assert_eq!(ctx(Focus::Row(0)), Ctx { text: false, digits: true, owns_bar: true, esc_quits: false });
}

#[test]
fn a_step_asks_for_the_world_only_where_it_reads_it() {
    let bare = View { world: None, ..drafted() };
    let need = |ix: Ix, ev: Ev| step(&ix, ev, &bare) == Err(NeedWorld);
    // Typing, the caret, a feed move and Esc in the box read nothing of it.
    assert!(!need(at(Focus::Compose), Ev::Key(ch('a'))));
    assert!(!need(at(Focus::Compose), Ev::Key(key(KeyCode::Up))));
    assert!(!need(at(Focus::Feed), Ev::Key(key(KeyCode::Up))));
    assert!(!need(at(Focus::Compose), Ev::Key(key(KeyCode::Esc))));
    assert!(!need(at(Focus::Feed), Ev::Click(Click::Pane { column: false, feed: false, row: None })));
    // The ≡ menu docks under ≡ on the bar, and the cycle's way back reads it.
    assert!(need(at(Focus::Feed), Ev::Key(key(KeyCode::Esc))));
    assert!(need(Ix { focus: Focus::Tab(Slot::Common), ..Ix::default() }, Ev::OverlayClosed(esc_menu(true))));
    assert!(need(at(Focus::Compose), Ev::Numbered { n: 1, menu: false }));
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
    let mut rows: Vec<SideHit> = ["a1", "a2", "a3"].iter().take(r.below(4)).map(|a| agent(a)).collect();
    rows.extend(["open", "deploy", "infra"].iter().take(r.below(4)).map(|c| row(c)));
    let shown = [Tab::Merged, chan("open"), chan("deploy"), chan("infra")][r.below(4)].clone();
    let mut bar = vec![Slot::Common];
    if matches!(&shown, Tab::Channel(c) if c != "open") {
        bar.push(Slot::Tab(shown.clone()));
    }
    bar.extend([Slot::New, Slot::Tab(chan("open")), Slot::Tab(Tab::Merged)]);
    let draft_empty = r.below(3) == 0;
    let (caret_start, caret_end) = if draft_empty { (true, true) } else { (r.coin(), r.coin()) };
    let feed_len = r.below(4);
    View {
        world: (r.below(5) != 0).then_some(Laid { bar, rows, shown }),
        side_drawn: r.below(4) != 0,
        draft_empty,
        completable: !draft_empty && r.coin(),
        caret_start,
        caret_end,
        feed_len,
        feed_selected: (feed_len > 0 && r.coin()).then(|| r.below(feed_len)),
        menu_on_repeat: r.coin(),
    }
}

fn any_ev(r: &mut Rng, v: &View) -> Ev {
    let rows = v.world.as_ref().map(|w| w.rows.clone()).unwrap_or_default();
    match r.below(22) {
        0..=10 => Ev::Key(any_key(r)),
        20 => Ev::Numbered { n: r.below(5), menu: r.coin() },
        21 => Ev::Compose,
        11 => Ev::Click(Click::Pane { column: r.coin(), feed: r.coin(), row: (!rows.is_empty() && r.coin()).then(|| rows[r.below(rows.len())].clone()) }),
        12 => Ev::Click(Click::Shell),
        13 => Ev::Paste,
        14 => Ev::ToggleBar,
        15 => Ev::Jump(r.below(8)),
        16 => Ev::TabActivated { i: r.below(8), menu: r.coin() },
        17 => Ev::WorldChanged,
        18 => Ev::ColumnHidden,
        19 if r.coin() => Ev::OverlayOpened([menu("common"), esc_menu(true), Overlay::Help, Overlay::Ask { id: "q".into() }, Overlay::Guard][r.below(5)].clone()),
        _ => Ev::OverlayClosed([menu("common"), esc_menu(true), Overlay::Help, Overlay::Ask { id: "q".into() }, Overlay::Guard][r.below(5)].clone()),
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
fn a_row_is_never_held_off_the_column_nor_the_cycle_on_the_chat() {
    for seed in 1..=300 {
        run(seed, 400, |_, ev, _, ix, _| {
            assert!(matches!(ix.focus, Focus::Row(_)) || ix.row_hit.is_none(), "seed {seed}: {ev:?} left {ix:?}");
            assert!(matches!(ix.last_nav, None | Some((Focus::Tab(_) | Focus::Row(_), _))), "seed {seed}: the Esc cycle remembers only a tab or a row: {ix:?}");
        });
    }
}

#[test]
fn the_overlays_mirror_what_the_shell_reported() {
    for seed in 1..=300 {
        let mut shell: Vec<Overlay> = Vec::new();
        run(seed, 400, |before, ev, _, ix, fx| {
            match ev {
                Ev::OverlayOpened(o) => shell.push(o.clone()),
                Ev::OverlayClosed(o) => {
                    if let Some(i) = shell.iter().rposition(|x| x == o) {
                        shell.remove(i);
                    }
                }
                _ => {}
            }
            assert_eq!(ix.overlays, shell, "seed {seed}: {ev:?}");
            if matches!(ev, Ev::OverlayOpened(_) | Ev::OverlayClosed(_)) {
                let flipped = before.overlaid() != ix.overlaid();
                let cycle = matches!(ev, Ev::OverlayClosed(Overlay::Menu { by_esc: true, .. })) && before.focus == Focus::Tab(Slot::Common);
                if cycle {
                    assert!(!matches!(ix.focus, Focus::Tab(Slot::Common)) || ix.last_nav == Some((Focus::Tab(Slot::Common), None)), "seed {seed}: the menu Esc opened moves the cycle on: {ix:?}");
                } else {
                    assert_eq!(fx.contains(&Effect::Redraw), flipped, "seed {seed}: a redraw when, and only when, something comes or goes over the pane");
                    assert_eq!(ix.focus, before.focus, "seed {seed}: an overlay moves no focus but the Esc cycle's");
                }
            }
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
