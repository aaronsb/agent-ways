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

use async_channel::Receiver;
use iocraft::prelude::*;

use crate::signal::{write_broadcast, Signal};

#[derive(Default, Props)]
pub struct AppProps {
    pub receiver: Option<Receiver<Signal>>,
}

const CHIP_WIDTH: u32 = 20;

/// Upper bound on the in-memory message buffer. At typical chat rates
/// this is unreachable; the cap only matters for runaway conditions
/// (overnight runs, a misbehaving peer, a loop). When we hit it, drop
/// from the head so the newest history stays visible and the clone-
/// per-append stays O(cap).
const MAX_SIGNALS: usize = 5000;

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

    // Drain the watcher into state.
    if let Some(rx) = props.receiver.clone() {
        hooks.use_future(async move {
            while let Ok(sig) = rx.recv().await {
                let mut v = signals.read().clone();
                v.push(sig);
                if v.len() > MAX_SIGNALS {
                    let drop_n = v.len() - MAX_SIGNALS;
                    v.drain(0..drop_n);
                }
                signals.set(v);
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
                    let (before, after) = split_at_char(&v, cursor.get());
                    let next = format!("{}\n{}", before, after);
                    input.set(next);
                    cursor.set(cursor.get() + 1);
                }
                KeyCode::Enter => {
                    let msg = input.read().trim_end().to_string();
                    if !msg.is_empty() {
                        match write_broadcast(&msg) {
                            Ok(name) => {
                                status.set(format!("sent: {}", name));
                                input.set(String::new());
                                cursor.set(0);
                            }
                            Err(e) => status.set(format!("send failed: {}", e)),
                        }
                    }
                }
                KeyCode::Backspace => {
                    let v = input.read().clone();
                    let pos = cursor.get();
                    if pos == 0 {
                        return;
                    }
                    let (before, after) = split_at_char(&v, pos);
                    // Drop one char off the end of `before`.
                    let trimmed: String = before
                        .chars()
                        .take(pos.saturating_sub(1))
                        .collect();
                    input.set(format!("{}{}", trimmed, after));
                    cursor.set(pos - 1);
                }
                KeyCode::Delete => {
                    let v = input.read().clone();
                    let pos = cursor.get();
                    let total = v.chars().count();
                    if pos >= total {
                        return;
                    }
                    let (before, after) = split_at_char(&v, pos);
                    // Drop the first char of `after`.
                    let trimmed: String = after.chars().skip(1).collect();
                    input.set(format!("{}{}", before, trimmed));
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
                    // Jump to the start of the current logical line.
                    let v = input.read().clone();
                    let p = cursor.get();
                    let (before, _) = split_at_char(&v, p);
                    let line_start = before.rfind('\n').map(|i| {
                        // chars-before count = char count of
                        // `before[..=i]`.
                        before[..=i].chars().count()
                    });
                    cursor.set(line_start.unwrap_or(0));
                }
                KeyCode::End => {
                    // Jump to the end of the current logical line.
                    let v = input.read().clone();
                    let p = cursor.get();
                    let (_, after) = split_at_char(&v, p);
                    let line_end_in_after = after.find('\n');
                    let add = match line_end_in_after {
                        Some(byte) => after[..byte].chars().count(),
                        None => after.chars().count(),
                    };
                    cursor.set(p + add);
                }
                KeyCode::Char(c)
                    if !modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let v = input.read().clone();
                    let pos = cursor.get();
                    let (before, after) = split_at_char(&v, pos);
                    input.set(format!("{}{}{}", before, c, after));
                    cursor.set(pos + 1);
                }
                _ => {}
            },
            _ => {}
        }
    });

    if should_exit.get() {
        system.exit();
    }

    let rows: Vec<_> = signals
        .read()
        .iter()
        .map(|s| {
            let (who, scope) = prettify(&s.from, &s.project, &s.cwd);
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
                        Text(color: Color::Cyan, content: who, wrap: TextWrap::NoWrap)
                        Text(color: Color::DarkGrey, content: scope, wrap: TextWrap::NoWrap)
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
    let input_height = (visual.clamp(1, 10) as u32) + 2;

    element! {
        View(flex_direction: FlexDirection::Column, width, height) {
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
            ) {
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
            View(height: 1, padding_left: 1) {
                Text(color: Color::DarkGrey, content: status.to_string(), wrap: TextWrap::NoWrap)
            }
        }
    }
}

/// Condense the wire `from`/`project`/`cwd` triple into two short
/// labels for the sender chip. Goals: never exceed the chip's interior
/// width, stay readable, give the human just enough to tell who sent
/// what.
fn prettify(from: &str, project: &str, cwd: &str) -> (String, String) {
    // Interior = chip width - 2 border columns - 2 padding columns.
    let interior = (CHIP_WIDTH as usize).saturating_sub(4);

    let who = if let Some(rest) = from.strip_prefix("claude:") {
        // UUIDs are 36 chars — "c:" + first 7 of the id keeps us
        // unique enough for a handful of concurrent sessions without
        // overflowing the chip.
        let tag = rest.get(0..7).unwrap_or(rest);
        format!("c:{}", tag)
    } else if let Some(rest) = from.strip_prefix("external:") {
        // Drop the terminal suffix ("aaron@kitty" → "aaron") so humans
        // read as themselves, not as their terminal emulator.
        rest.split('@').next().unwrap_or(rest).to_string()
    } else {
        from.to_string()
    };

    // Scope: prefer the last path segment of the cwd over the raw
    // project name, since cwd is always a full path while project is
    // whatever the sender chose to pass. Fall back to project if cwd
    // is empty.
    let scope_src = if cwd.is_empty() { project } else { cwd };
    let segment = scope_src.rsplit('/').next().unwrap_or(scope_src);
    let scope = if segment.is_empty() {
        "broadcast".to_string()
    } else {
        segment.to_string()
    };

    (truncate(&who, interior), truncate(&scope, interior))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 1 {
        s.chars().take(max).collect()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

/// Count how many *rendered* rows `text` occupies inside a box of
/// `interior` columns. Approximate: counts chars, ignores grapheme
/// width — good enough for sizing a chat compose box where the exact
/// last-column edge doesn't matter.
fn visual_line_count(text: &str, interior: usize) -> usize {
    if interior == 0 {
        return 1;
    }
    let mut rows = 0usize;
    for line in text.split('\n') {
        let len = line.chars().count();
        rows += if len == 0 { 1 } else { len.div_ceil(interior) };
    }
    rows.max(1)
}

/// Split a string at a *char* offset (not a byte offset). If the
/// offset runs past the end of the string, the tail is empty.
fn split_at_char(s: &str, n: usize) -> (&str, &str) {
    match s.char_indices().nth(n) {
        Some((byte, _)) => s.split_at(byte),
        None => (s, ""),
    }
}

/// Render the compose buffer with a block cursor at `pos`. The cursor
/// sits *on* the char at `pos` (that char is replaced visually by the
/// block) or past the end if `pos == len`. Matches how most terminal
/// editors render a block cursor.
fn render_cursor(text: &str, pos: usize) -> String {
    let total = text.chars().count();
    if pos >= total {
        return format!("{}\u{2588}", text);
    }
    let (before, rest) = split_at_char(text, pos);
    // Skip the char under the cursor so the block doesn't overlap it.
    let after: String = rest.chars().skip(1).collect();
    format!("{}\u{2588}{}", before, after)
}

#[cfg(test)]
mod layout_tests {
    use super::visual_line_count;

    #[test]
    fn short_line_is_one_row() {
        assert_eq!(visual_line_count("hi", 20), 1);
    }

    #[test]
    fn wrap_on_width() {
        // 21 chars in a 10-wide box → 3 rows.
        assert_eq!(visual_line_count("abcdefghijklmnopqrstu", 10), 3);
    }

    #[test]
    fn explicit_newlines_count() {
        assert_eq!(visual_line_count("a\nb\nc", 20), 3);
    }

    #[test]
    fn combined_wrap_and_newline() {
        // "abcdefghij" (10 chars) wraps once in 5-wide → 2 rows
        // followed by "x" on its own line → 1 row = 3 total.
        assert_eq!(visual_line_count("abcdefghij\nx", 5), 3);
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::{render_cursor, split_at_char};

    #[test]
    fn split_within_ascii() {
        assert_eq!(split_at_char("hello", 2), ("he", "llo"));
    }

    #[test]
    fn split_past_end() {
        assert_eq!(split_at_char("hi", 10), ("hi", ""));
    }

    #[test]
    fn split_respects_char_boundaries() {
        // "héllo" — 'é' is 2 bytes. Splitting at char 2 should land
        // between 'é' and 'l', not mid-byte.
        let (before, after) = split_at_char("héllo", 2);
        assert_eq!(before, "hé");
        assert_eq!(after, "llo");
    }

    #[test]
    fn cursor_at_end_appends_block() {
        assert_eq!(render_cursor("hi", 2), "hi\u{2588}");
    }

    #[test]
    fn cursor_mid_text_replaces_char_visually() {
        // Cursor at position 1 over "abc" → 'a' + block + 'c'.
        assert_eq!(render_cursor("abc", 1), "a\u{2588}c");
    }

    #[test]
    fn cursor_on_empty_buffer() {
        assert_eq!(render_cursor("", 0), "\u{2588}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettify_claude_sender() {
        let (who, scope) = prettify("claude:e74a4a4b-7e3b-49bc-8404-216162e54ba8", "claude", "/home/aaron/.claude");
        assert_eq!(who, "c:e74a4a4");
        assert_eq!(scope, ".claude");
    }

    #[test]
    fn prettify_scope_prefers_cwd_basename() {
        let (_, scope) = prettify("external:aaron", "ignored", "/home/aaron/temp");
        assert_eq!(scope, "temp");
    }

    #[test]
    fn prettify_external_strips_terminal() {
        let (who, _) = prettify("external:aaron@kitty", "proj", "/home/aaron");
        assert_eq!(who, "aaron");
    }

    #[test]
    fn prettify_scope_truncates_long_segment() {
        // Interior is 16 chars for CHIP_WIDTH=20 (2 borders + 2 padding).
        let (_, scope) = prettify("external:aaron", "x", "/tmp/some-very-long-directory-name");
        assert!(scope.chars().count() <= 16);
        assert!(scope.ends_with('…'));
    }
}
