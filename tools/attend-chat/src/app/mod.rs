//! Minimum-bidirectional chat component.
//!
//! Stream of signals on top, input box on bottom. Sender + scope live
//! in a small bordered "chip" to the left of each message; the message
//! body wraps naturally. Input is a minimal custom buffer: Enter sends,
//! Shift-Enter or Alt-Enter inserts a newline. We don't use iocraft's
//! `TextInput` because its multi-line handler consumes plain Enter and
//! fires its `on_change` after our own handler clears the buffer,
//! which races with the send path.
//!
//! No sidebar, no focus filter, no threading render — those land in
//! follow-up PRs on ADR-120.
//!
//! The keyboard handler bodies live in [`keys`] so each branch is a
//! free function that can be unit-tested without standing up an
//! iocraft `App`. The closure here is a thin dispatcher: read state,
//! call the handler, write state back.

mod keys;

use agent_identity::TermCaps;
use async_channel::Receiver;
use iocraft::prelude::*;

use crate::chip::{chip_for, color_for, known_identities, CHIP_WIDTH};
use crate::groups::channels;
use crate::helper::{self, HelperMode};
use crate::legend::{find_trailing_mention, group_legend_row, legend_row, Sigil};
use crate::sessions::discover as discover_sessions;
use crate::signal::Signal;
use crate::slash;
use crate::tabs::{self, Tab};
use crate::text_layout::{render_cursor, visual_line_count};

use keys::{
    destination_label, handle_backspace, handle_char_insert, handle_delete, handle_end,
    handle_enter, handle_home, handle_newline_insert, handle_tab, EnterAction, TabCycle,
};

#[derive(Default, Props)]
pub struct AppProps {
    pub receiver: Option<Receiver<Signal>>,
}

/// Upper bound on the in-memory message buffer. At typical chat rates
/// this is unreachable; the cap only matters for runaway conditions
/// (overnight runs, a misbehaving peer, a loop). When we hit it, drop
/// from the head so the newest history stays visible and the clone-
/// per-append stays O(cap).
const MAX_SIGNALS: usize = 5000;

/// Append `sig` to the message buffer, dropping from the head if it would
/// exceed [`MAX_SIGNALS`]. Both writers into `signals` — the async
/// watcher drain and the directed-send echo — go through here so the
/// cap/overflow policy has a single home and the two paths cannot desync.
fn push_capped(signals: &mut State<Vec<Signal>>, sig: Signal) {
    let mut v = signals.read().clone();
    v.push(sig);
    if v.len() > MAX_SIGNALS {
        let drop_n = v.len() - MAX_SIGNALS;
        v.drain(0..drop_n);
    }
    signals.set(v);
}

#[component]
pub fn App(props: &AppProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut system = hooks.use_context_mut::<SystemContext>();
    let (width, height) = hooks.use_terminal_size();
    let mut signals = hooks.use_state::<Vec<Signal>, _>(Vec::new);
    let mut input = hooks.use_state(String::new);
    // Cursor position measured in *chars*, not bytes. Clamped into
    // `[0, input.chars().count()]` on every mutation.
    let mut cursor = hooks.use_state(|| 0usize);
    let mut should_exit = hooks.use_state(|| false);
    let mut status = hooks.use_state(String::new);
    let mut tab_cycle = hooks.use_state::<Option<TabCycle>, _>(|| None);
    // Foreground tab (#393). Merged (slot zero) at launch — today's
    // full-stream view stays the opening posture; per-channel focus
    // is an explicit gesture (Tab / Alt+N).
    let mut foreground = hooks.use_state(Tab::default);

    // Time-based refresh tick (ADR-129). Bumped every 5 seconds so the
    // render path re-runs `discover_sessions` + `known_identities` on a
    // human-scale clock rather than only on signal/keypress events. A
    // peer that boots up without sending anything appears in the legend
    // within one tick; a peer whose attend stops shows up as stale once
    // its heartbeat ages past grace, also within one tick. The chat is
    // a turn-blind surface — wall-clock time is the right axis here.
    //
    // 5s is a balance: short enough that "I just launched another
    // claude" feels live, long enough that we are not re-walking
    // ~/.claude/sessions/*.json several times per second on idle.
    let mut tick = hooks.use_state(|| 0u64);
    hooks.use_future(async move {
        loop {
            // Human presence heartbeat (ADR-170): while the chat is
            // open, keep `heartbeat/<username>` fresh so this human
            // counts as a live focus-group member — to our own
            // `live_peer_count`, to attend's `cleanup_stale`, and to
            // agent-side `send --focus` validation. Touched before
            // the first sleep so presence starts at launch, not one
            // tick later. Best-effort like every heartbeat write.
            let _ = attend_heartbeat::touch(&crate::signal::human_member_id());
            smol::Timer::after(std::time::Duration::from_secs(5)).await;
            let next = tick.get().wrapping_add(1);
            tick.set(next);
        }
    });

    // Drain the watcher into state.
    if let Some(rx) = props.receiver.clone() {
        hooks.use_future(async move {
            while let Ok(sig) = rx.recv().await {
                push_capped(&mut signals, sig);
            }
        });
    }

    hooks.use_terminal_events({
        move |event| match event {
            TerminalEvent::Key(KeyEvent {
                code,
                kind,
                modifiers,
                ..
            }) if kind != KeyEventKind::Release => match code {
                KeyCode::Esc => should_exit.set(true),
                KeyCode::Enter
                    if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
                {
                    // Shift-Enter / Alt-Enter → insert newline at the
                    // cursor. Shift-Enter only comes through on
                    // terminals that speak the kitty keyboard
                    // protocol; Alt-Enter is a cross-terminal
                    // fallback.
                    let v = input.read().clone();
                    let (next, new_cursor) = handle_newline_insert(&v, cursor.get());
                    input.set(next);
                    cursor.set(new_cursor);
                }
                KeyCode::Enter => {
                    let v = input.read().clone();
                    // Scope the read guard to the handler call only: the
                    // echo arm below needs to `signals.set(...)`, which
                    // cannot run while a read guard is live. Hold the guard
                    // rather than clone for the call — Rust deref-coerces
                    // `&Ref<Vec<Signal>>` to `&[Signal]`, so the handler
                    // sees a borrowed slice without copying the (capped,
                    // but potentially 5000-entry) buffer on every keypress.
                    let fg = foreground.read().clone();
                    let action = {
                        let sigs_guard = signals.read();
                        handle_enter(&v, &sigs_guard, &fg)
                    };
                    match action {
                        EnterAction::None => {}
                        EnterAction::ClearWithStatus(s) => {
                            status.set(s);
                            input.set(String::new());
                            cursor.set(0);
                        }
                        EnterAction::ClearWithStatusAndEcho { status: s, echo } => {
                            status.set(s);
                            input.set(String::new());
                            cursor.set(0);
                            // Append the display-only echo through the same
                            // capped path the watcher drain uses, so a
                            // directed send appears in the sender's own
                            // transcript just as a broadcast would.
                            push_capped(&mut signals, echo);
                        }
                        EnterAction::StatusOnly(s) => status.set(s),
                        EnterAction::ClearWithStatusAndFocus { status: s, focus } => {
                            status.set(s);
                            input.set(String::new());
                            cursor.set(0);
                            // `/dissolve` closed the foreground tab —
                            // move focus where the handler advanced it.
                            foreground.set(focus);
                        }
                        EnterAction::ClearTranscript => {
                            // Display-only: the buffer empties but
                            // nothing on the bus is touched — signals
                            // on disk stay for other readers, and
                            // fresh traffic streams back in.
                            signals.set(Vec::new());
                            status.set("transcript cleared".into());
                            input.set(String::new());
                            cursor.set(0);
                        }
                    }
                }
                KeyCode::Tab => {
                    let buf = input.read().clone();
                    if buf.is_empty() {
                        // Empty compose line: Tab cycles the
                        // foreground tab (#393). With content it
                        // stays completion — the keybinding-collision
                        // resolution from the issue thread.
                        let names =
                            tabs::strip_names(&channels(TermCaps::detect()));
                        let cur =
                            tabs::normalize(foreground.read().clone(), &names);
                        foreground.set(tabs::cycle_next(&cur, &names));
                    } else {
                        let cur = cursor.get();
                        let cycle = tab_cycle.read().clone();
                        // Same borrow-not-clone discipline as Enter — Tab
                        // fires more often (every keystroke when cycling
                        // completions), so the per-press cost matters.
                        let sigs_guard = signals.read();
                        let res = handle_tab(&buf, cur, cycle, &sigs_guard);
                        input.set(res.new_buf);
                        cursor.set(res.new_cursor);
                        tab_cycle.set(res.new_cycle);
                    }
                }
                KeyCode::Backspace => {
                    let v = input.read().clone();
                    let (next, new_cursor) = handle_backspace(&v, cursor.get());
                    input.set(next);
                    cursor.set(new_cursor);
                }
                KeyCode::Delete => {
                    let v = input.read().clone();
                    let (next, new_cursor) = handle_delete(&v, cursor.get());
                    input.set(next);
                    cursor.set(new_cursor);
                }
                KeyCode::Left => {
                    let p = cursor.get();
                    if p > 0 {
                        cursor.set(p - 1);
                    }
                }
                KeyCode::Right => {
                    let p = cursor.get();
                    let total = input.read().chars().count();
                    if p < total {
                        cursor.set(p + 1);
                    }
                }
                KeyCode::Home => {
                    let v = input.read().clone();
                    cursor.set(handle_home(&v, cursor.get()));
                }
                KeyCode::End => {
                    let v = input.read().clone();
                    cursor.set(handle_end(&v, cursor.get()));
                }
                KeyCode::Char(c)
                    if modifiers.contains(KeyModifiers::ALT) && c.is_ascii_digit() =>
                {
                    // Alt+1..9 jumps straight to a tab (IRC prior):
                    // 1 = merged, 2 = #open, 3.. = named channels in
                    // strip order. Empty slots are no-ops.
                    if let Some(slot) = c.to_digit(10) {
                        let names =
                            tabs::strip_names(&channels(TermCaps::detect()));
                        if let Some(tab) = tabs::jump(slot, &names) {
                            foreground.set(tab);
                        }
                    }
                }
                KeyCode::Char(c)
                    if !modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let v = input.read().clone();
                    let (next, new_cursor) = handle_char_insert(&v, cursor.get(), c);
                    input.set(next);
                    cursor.set(new_cursor);
                }
                _ => {}
            },
            _ => {}
        }
    });

    if should_exit.get() {
        system.exit();
    }

    // Detect terminal capability once per render pass and reuse for
    // every chip. Cheap env reads, but doing it in the per-signal map
    // would still be pointless repetition.
    let caps = TermCaps::detect();
    // Subscribe to the wall-clock refresh tick so iocraft re-renders
    // when the timer future bumps it. The value itself is not used —
    // we only need the read to register the dependency in the
    // reactive graph. Drives time-based discovery (new peers
    // appear, stale peers disappear) without requiring a key event.
    let _refresh = tick.get();
    // Per-render instance-registry cache (ADR-129; PR #77 review).
    // Built once at the top of each render and passed down to every
    // chip / known_identities call so the registry yaml is read at
    // most once per distinct cwd per render. Deliberately scoped to
    // this function — never share across renders, since the
    // registry can change between renders (peer registers, GC).
    let instance_cache = attend_instances::SnapshotCache::new();
    // Registries and trailing-partial for the legend + completion
    // highlight. Re-derived each render:
    // - `known_identities`: dedupes in O(n) over the capped signal
    //   buffer; bounded.
    // - `scan_groups`: one `read_dir` + one YAML parse per render.
    //   The YAML is tiny (it has per-group membership, not message
    //   history), so the cost is negligible next to the render we'd
    //   do anyway. Simpler than caching with invalidation.
    let seed_sessions = discover_sessions();
    let known = known_identities(&signals.read(), &seed_sessions, caps, &instance_cache);
    // `channels` prepends the synthetic `#open` base and drops any
    // lingering literal `open` group so the legend has a single
    // commons chip (ADR-124 §1–§2). The base entry has empty
    // membership, so it's a no-op for `session_to_groups` below.
    let groups_known = channels(caps);
    // Foreground tab, normalized against the current strip: a channel
    // dissolved by a peer between renders degrades to the `#open`
    // floor instead of leaving focus on a tab that no longer renders.
    let fg_tab = tabs::normalize(
        foreground.read().clone(),
        &tabs::strip_names(&groups_known),
    );
    // Pre-index session-id → groups once per render so the chip
    // loop is O(signals) instead of O(signals · groups · members).
    // At today's scale neither form matters, but the render path is
    // hot and the reindex is trivial.
    let session_to_groups: std::collections::HashMap<&str, Vec<&crate::groups::KnownGroup>> = {
        let mut m: std::collections::HashMap<&str, Vec<&crate::groups::KnownGroup>> =
            std::collections::HashMap::new();
        for kg in &groups_known {
            for member in &kg.membership.members {
                m.entry(member.as_str()).or_default().push(kg);
            }
        }
        m
    };
    // `input.read()` returns a read guard — borrow it into a local
    // binding so `find_trailing_mention`'s `&str` doesn't outlive
    // the temporary guard.
    let input_snapshot = input.read().clone();
    // One wall-clock read per render pass shared by every cell's
    // datetime — per-cell `now()` calls could straddle a minute
    // boundary and render inconsistent times within one frame.
    let now = std::time::SystemTime::now();
    // Trailing-mention context is still consumed by the top channel
    // bar so it can underline a matching `#partial` when the user is
    // picking a group; the agent-side partial now flows through
    // `helper::derive` below and doesn't need its own binding here.
    let mention_ctx = find_trailing_mention(&input_snapshot);
    let group_partial: Option<String> = mention_ctx
        .as_ref()
        .filter(|m| m.sigil == Sigil::Group)
        .map(|m| m.partial.to_string());
    // Helper-row mode is a pure function of the input buffer. The
    // state machine lives in `helper::derive` — see that module's
    // docs + tests for the full rule table. Once a slash command
    // is past its name, the registry's `ArgKind` routes the helper
    // (`/whois ` → agents, `/join ` → channels).
    let helper_mode = helper::derive(&input_snapshot);
    let rows: Vec<_> = signals
        .read()
        .iter()
        // Per-tab transcript (#393): merged shows the full stream;
        // a channel tab filters to its own traffic (+ local Info).
        .filter(|s| tabs::visible_in(&s.channel, &fg_tab))
        .map(|s| {
            let chip = chip_for(&s.from, &s.project, &s.cwd, caps, &instance_cache);
            let weight = if chip.style.bold { Weight::Bold } else { Weight::Normal };
            let color = color_for(chip.palette, caps);
            // Group glyphs for this chip: cross-reference the
            // sender's membership key against _groups.yaml. Claudes
            // key by session UUID; humans key by their sanitized
            // username (ADR-170) — the same id `/join` writes, so a
            // joined human's chip carries the group glyph exactly
            // like a claude's.
            let membership_key: Option<String> = chip.session_id.clone().or_else(|| {
                s.from.strip_prefix("external:").map(|rest| {
                    agent_identity::sanitize_id_component(
                        rest.split('@').next().unwrap_or(rest),
                    )
                })
            });
            let group_chips: Vec<AnyElement<'static>> = membership_key
                .as_deref()
                .and_then(|key| session_to_groups.get(key))
                .map(|groups| {
                    groups
                        .iter()
                        .map(|kg| {
                            let gcolor = color_for(kg.group.palette, caps);
                            element! {
                                Text(
                                    color: gcolor,
                                    content: format!("{} ", kg.group.glyph),
                                    wrap: TextWrap::NoWrap,
                                )
                            }
                            .into_any()
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Cell datetime (#389): the identity block's third line
            // pairs the charm glyphs with a tight timestamp. One
            // shared format across every attend surface —
            // `agent_fmt::compact_time` — so the TUI, `attend inbox`,
            // and the drain cannot drift. `ts == 0` means the signal
            // file's mtime was unreadable; render nothing rather
            // than a wrong epoch date.
            let when = if s.ts == 0 {
                String::new()
            } else {
                agent_fmt::compact_time(
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(s.ts),
                    now,
                )
            };
            element! {
                View(flex_direction: FlexDirection::Row, margin_bottom: 1) {
                    View(
                        border_style: BorderStyle::Round,
                        border_color: Color::DarkGrey,
                        padding_left: 1,
                        padding_right: 1,
                        width: CHIP_WIDTH,
                        flex_direction: FlexDirection::Column,
                        flex_shrink: 0.0,
                    ) {
                        Text(
                            color,
                            weight,
                            italic: chip.style.italic,
                            content: chip.primary,
                            wrap: TextWrap::NoWrap,
                        )
                        Text(color: Color::DarkGrey, content: chip.secondary, wrap: TextWrap::NoWrap)
                        View(flex_direction: FlexDirection::Row) {
                            #(group_chips)
                            Text(color: Color::DarkGrey, content: when, wrap: TextWrap::NoWrap)
                        }
                    }
                    View(
                        flex_grow: 1.0,
                        padding_left: 1,
                        padding_right: 1,
                        padding_top: 1,
                    ) {
                        Text(content: s.message.clone(), wrap: TextWrap::Wrap)
                    }
                }
            }
        })
        .collect();

    // Render the input buffer with a block cursor sitting on the char
    // at `cursor`. `format!` allocates once per frame; cheap at chat-
    // compose scale and not worth caching into state.
    let input_value = input.to_string();
    let display_with_cursor = render_cursor(&input_value, cursor.get());
    // Interior width = total - 2 border cols - 2 padding cols - 2
    // prompt cols. We grow the box to the visual row count so a long
    // wrapped message expands it instead of spilling onto the border
    // or status row.
    let interior = (width as usize).saturating_sub(6).max(1);
    let visual = visual_line_count(&display_with_cursor, interior);
    // +2 borders, +1 for the destination-flag row pinned at the
    // bottom of the box (#392).
    let input_height = (visual.clamp(1, 10) as u32) + 3;
    // Destination flag (#392): inverse-styled label in the input
    // box's lower-right showing where Enter would send this buffer.
    // Live: re-derived from the buffer every render, through the same
    // routing parse the send path uses. Hidden while composing a
    // slash command — nothing would be sent.
    let dest_chip: Vec<AnyElement<'static>> =
        destination_label(&input_snapshot, &tabs::send_scope(&fg_tab))
        .map(|label| {
            vec![element! {
                View(background_color: Color::Blue) {
                    Text(
                        color: Color::Black,
                        content: format!(" {label} "),
                        wrap: TextWrap::NoWrap,
                    )
                }
            }
            .into_any()]
        })
        .unwrap_or_default();

    element! {
        View(flex_direction: FlexDirection::Column, width, height) {
            // Tab strip at the top (#393) — fixed one-row height:
            // `[merged] [#open] [#g] …`, merged pinned at slot zero,
            // the foreground tab inverse in its channel color. Doubles
            // as the channel legend: each chip renders `<glyph> #name`
            // in the group's hashed color, with the `#partial`
            // completion underline the old channel bar had.
            #(tabs::tab_strip_row(&groups_known, &fg_tab, group_partial.as_deref()))
            // `min_height: 0` + `overflow: Hidden` keep this pane
            // inside its flex-grown slot instead of expanding to the
            // intrinsic height of the message list — without them, a
            // long scrollback pushes the input box past the bottom of
            // the terminal.
            View(
                flex_grow: 1.0,
                min_height: 0,
                overflow: Overflow::Hidden,
                border_style: BorderStyle::Round,
                border_color: Color::DarkGrey,
                padding_left: 1,
                padding_right: 1,
            ) {
                ScrollView(auto_scroll: true) {
                    View(flex_direction: FlexDirection::Column) {
                        #(rows)
                    }
                }
            }
            View(
                border_style: BorderStyle::Round,
                border_color: Color::Blue,
                padding_left: 1,
                padding_right: 1,
                height: input_height,
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
            ) {
                View(flex_direction: FlexDirection::Row, flex_grow: 1.0) {
                    View(width: 2, flex_shrink: 0.0) {
                        Text(color: Color::Blue, content: "> ")
                    }
                    View(flex_grow: 1.0) {
                        Text(
                            content: display_with_cursor,
                            wrap: TextWrap::Wrap,
                        )
                    }
                }
                View(
                    height: 1,
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::End,
                ) {
                    #(dest_chip)
                }
            }
            #(match &helper_mode {
                HelperMode::Agents(p) => legend_row(&known, p.as_deref()),
                HelperMode::Groups(p) => group_legend_row(&groups_known, p.as_deref()),
                HelperMode::Slash(p) => slash::slash_legend_row(p.as_deref()),
            })
            View(height: 1, padding_left: 1) {
                Text(color: Color::DarkGrey, content: status.to_string(), wrap: TextWrap::NoWrap)
            }
        }
    }
}
