//! Keyboard handlers extracted from the iocraft component closure.
//!
//! Each handler is a free function that takes the relevant string +
//! cursor + cycle inputs and returns the next state. The component
//! closure does the `State::set` calls; pulling the logic out keeps
//! the closure short and lets the handlers be unit-tested without
//! standing up an iocraft `App` instance.

use agent_identity::TermCaps;

use crate::chip::{known_identities, resolve_nickname};
use crate::groups::{channels, live_peer_count, resolve_group_dir};
use crate::legend::{
    all_completions, all_group_completions, apply_completion, find_trailing_mention,
    parse_addressed, Addressed, Sigil,
};
use crate::sessions::discover as discover_sessions;
use crate::signal::{cwd_dir, write_broadcast, write_signal, Signal};
use crate::slash;
use crate::text_layout::split_at_char;

/// Tab-completion cycle state.
///
/// First Tab on `@T<Tab>` inserts the first prefix-matching candidate
/// and stores the candidate list + sigil here. Each subsequent Tab
/// (with the buffer still ending in the previously-inserted token)
/// advances the index, wrapping at the end. Any other keystroke
/// implicitly resets the cycle: the Tab handler checks that the
/// buffer's tail still matches `candidates[index]`, and if it
/// doesn't, treats the press as a fresh completion start.
///
/// Same model applies to `#group<Tab>` cycling — the sigil
/// discriminates which pool to draw candidates from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabCycle {
    pub sigil: Sigil,
    pub candidates: Vec<String>,
    pub index: usize,
}

/// What the closure should do after `handle_enter` runs.
pub enum EnterAction {
    /// Empty input — no state change.
    None,
    /// Success path: set status, clear input + cursor.
    ClearWithStatus(String),
    /// Failure path: set status, leave input + cursor intact so the
    /// user can edit and retry.
    StatusOnly(String),
}

/// Plain-Enter dispatch. Slash command first; otherwise parse the
/// `@`/`#` address sigil and write to the right inbox. Returns an
/// `EnterAction` describing how the closure should update state.
pub fn handle_enter(input_value: &str, signals: &[Signal]) -> EnterAction {
    let msg = input_value.trim_end().to_string();
    if msg.is_empty() {
        return EnterAction::None;
    }
    // Slash-command interceptor — stops `/help`, `/whois`, etc.
    // from leaking onto the signal bus as plain messages. Runs
    // before the `@`/`#`/broadcast dispatch because slash syntax
    // is start-of-input only and unambiguous.
    if let Some((cmd, args)) = slash::parse(&msg) {
        return match slash::dispatch(cmd, args) {
            slash::SlashOutcome::Ok(s) => EnterAction::ClearWithStatus(s),
            slash::SlashOutcome::Err(s) => EnterAction::StatusOnly(s),
        };
    }
    let caps = TermCaps::detect();
    let seeds = discover_sessions();
    // Local instance cache for this Enter-handler invocation.
    // Distinct from the per-render cache because key handlers are
    // not on the render path — they fire on demand and resolve
    // against the current registry state.
    let instance_cache = attend_instances::SnapshotCache::new();
    let agents = known_identities(signals, &seeds, caps, &instance_cache);
    let result = match parse_addressed(&msg) {
        Some(Addressed::Agent(name)) => match resolve_nickname(name, &agents) {
            Some(target_cwd) => write_signal(&cwd_dir(&target_cwd), &msg)
                .map(|n| format!("sent → @{name}: {n}")),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("@{name}: unknown nickname"),
            )),
        },
        // Empty-group rejection (ADR-129 follow-up). Mirrors
        // `attend send --focus` discipline: a `#groupname` send
        // to a group with zero live members would land in
        // `@<name>/` and sit unread, with the chat reporting
        // "sent" so the user assumes delivery. The base channel
        // (`#open`) bypasses the check — it rides `_broadcast/`,
        // where every attend scans regardless of group membership.
        Some(Addressed::Group(name)) => match resolve_group_dir(name) {
            Some(dir) if live_peer_count(name) > 0 => {
                write_signal(&dir, &msg).map(|n| format!("sent → #{name}: {n}"))
            }
            Some(_) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "#{name}: no live peers — message would not be read. \
                     Try #open to broadcast."
                ),
            )),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("#{name}: unknown group"),
            )),
        },
        None => write_broadcast(&msg).map(|n| format!("sent: {n}")),
    };
    match result {
        Ok(s) => EnterAction::ClearWithStatus(s),
        Err(e) => EnterAction::StatusOnly(format!("send failed: {}", e)),
    }
}

/// Result of a Tab keypress. Always returns the next buffer +
/// cursor + cycle so the closure applies the result unconditionally,
/// with no "did anything change" branching at the call site.
pub struct TabResult {
    pub new_buf: String,
    pub new_cursor: usize,
    pub new_cycle: Option<TabCycle>,
}

/// Tab-completion + cycle advance. No-op when the caret is mid-buffer
/// (avoids surprising mid-sentence edits) or when no completion
/// applies. Slash completions short-circuit before `@`/`#` parsing,
/// matching the original closure's order.
pub fn handle_tab(
    buf: &str,
    cursor: usize,
    current_cycle: Option<TabCycle>,
    signals: &[Signal],
) -> TabResult {
    let unchanged = TabResult {
        new_buf: buf.to_string(),
        new_cursor: cursor,
        new_cycle: None,
    };

    if cursor != buf.chars().count() {
        return unchanged;
    }

    // Slash completion runs first — start-of-input-only grammar, so
    // when it matches the `@`/`#` logic below never applies. Silent
    // fall-through on no match so a stale `/` with no registry hit
    // doesn't trap the user.
    if let Some(partial) = slash::find_slash_partial(buf) {
        if let Some(hit) = slash::best_slash_completion(partial.partial) {
            let (next, new_cursor) = slash::apply_slash_completion(hit.name);
            return TabResult { new_buf: next, new_cursor, new_cycle: None };
        }
        // Slash and `@`/`#` share the cycle slot; resetting prevents
        // a stale agent cycle from carrying across a slash insertion.
        return unchanged;
    }

    // Cycling: if we have a stored cycle and the buffer still ends
    // with the candidate we just inserted (sigil + name + trailing
    // space, the exact form `apply_completion` produces), advance
    // to the next candidate.
    if let Some(c) = current_cycle {
        let sigil_char = match c.sigil {
            Sigil::Agent => '@',
            Sigil::Group => '#',
        };
        let expected_tail = format!("{}{} ", sigil_char, c.candidates[c.index]);
        if buf.ends_with(&expected_tail) && !c.candidates.is_empty() {
            let next_idx = (c.index + 1) % c.candidates.len();
            let next = &c.candidates[next_idx];
            // Splice the old tail off and append the new one. We
            // work in chars rather than bytes so multi-byte names
            // stay intact.
            let total_chars = buf.chars().count();
            let tail_chars = expected_tail.chars().count();
            let head: String = buf.chars().take(total_chars - tail_chars).collect();
            let new_buf = format!("{}{}{} ", head, sigil_char, next);
            let new_cursor = new_buf.chars().count();
            return TabResult {
                new_buf,
                new_cursor,
                new_cycle: Some(TabCycle { index: next_idx, ..c }),
            };
        }
    }

    // Fresh cycle: derive candidates from the current trailing mention.
    let Some(mention) = find_trailing_mention(buf) else {
        return unchanged;
    };
    let caps = TermCaps::detect();
    let candidates: Vec<String> = match mention.sigil {
        Sigil::Agent => {
            let seeds = discover_sessions();
            let instance_cache = attend_instances::SnapshotCache::new();
            let agents = known_identities(signals, &seeds, caps, &instance_cache);
            all_completions(mention.partial, &agents)
        }
        Sigil::Group => {
            let groups = channels(caps);
            all_group_completions(mention.partial, &groups)
        }
    };
    if candidates.is_empty() {
        return unchanged;
    }
    let first = candidates[0].clone();
    let (next_buf, new_cursor) = apply_completion(buf, &mention, &first);
    TabResult {
        new_buf: next_buf,
        new_cursor,
        new_cycle: Some(TabCycle {
            sigil: mention.sigil,
            candidates,
            index: 0,
        }),
    }
}

/// Insert a newline at the cursor (Shift-Enter / Alt-Enter).
pub fn handle_newline_insert(buf: &str, cursor: usize) -> (String, usize) {
    let (before, after) = split_at_char(buf, cursor);
    (format!("{}\n{}", before, after), cursor + 1)
}

/// Insert a single char at the cursor.
pub fn handle_char_insert(buf: &str, cursor: usize, c: char) -> (String, usize) {
    let (before, after) = split_at_char(buf, cursor);
    (format!("{}{}{}", before, c, after), cursor + 1)
}

/// Drop the char to the left of the cursor.
pub fn handle_backspace(buf: &str, cursor: usize) -> (String, usize) {
    if cursor == 0 {
        return (buf.to_string(), 0);
    }
    let (before, after) = split_at_char(buf, cursor);
    let trimmed: String = before.chars().take(cursor.saturating_sub(1)).collect();
    (format!("{}{}", trimmed, after), cursor - 1)
}

/// Drop the char at the cursor.
pub fn handle_delete(buf: &str, cursor: usize) -> (String, usize) {
    let total = buf.chars().count();
    if cursor >= total {
        return (buf.to_string(), cursor);
    }
    let (before, after) = split_at_char(buf, cursor);
    let trimmed: String = after.chars().skip(1).collect();
    (format!("{}{}", before, trimmed), cursor)
}

/// Jump to the start of the current logical line.
pub fn handle_home(buf: &str, cursor: usize) -> usize {
    let (before, _) = split_at_char(buf, cursor);
    before
        .rfind('\n')
        .map(|i| before[..=i].chars().count())
        .unwrap_or(0)
}

/// Jump to the end of the current logical line.
pub fn handle_end(buf: &str, cursor: usize) -> usize {
    let (_, after) = split_at_char(buf, cursor);
    let add = match after.find('\n') {
        Some(byte) => after[..byte].chars().count(),
        None => after.chars().count(),
    };
    cursor + add
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_empty_is_noop() {
        match handle_enter("   ", &[]) {
            EnterAction::None => {}
            _ => panic!("empty input should produce EnterAction::None"),
        }
    }

    #[test]
    fn backspace_at_zero_is_noop() {
        let (b, c) = handle_backspace("hello", 0);
        assert_eq!(b, "hello");
        assert_eq!(c, 0);
    }

    #[test]
    fn backspace_drops_left_char() {
        let (b, c) = handle_backspace("hello", 5);
        assert_eq!(b, "hell");
        assert_eq!(c, 4);
    }

    #[test]
    fn delete_at_end_is_noop() {
        let (b, c) = handle_delete("hi", 2);
        assert_eq!(b, "hi");
        assert_eq!(c, 2);
    }

    #[test]
    fn delete_drops_right_char() {
        let (b, c) = handle_delete("hi", 0);
        assert_eq!(b, "i");
        assert_eq!(c, 0);
    }

    #[test]
    fn char_insert_splits_at_cursor() {
        let (b, c) = handle_char_insert("abc", 1, 'X');
        assert_eq!(b, "aXbc");
        assert_eq!(c, 2);
    }

    #[test]
    fn newline_insert_splits_at_cursor() {
        let (b, c) = handle_newline_insert("abc", 1);
        assert_eq!(b, "a\nbc");
        assert_eq!(c, 2);
    }

    #[test]
    fn home_jumps_to_line_start() {
        // "ab\ncd" with cursor on `d` (chars index 4) → jump to char 3
        // (first char of second line).
        assert_eq!(handle_home("ab\ncd", 4), 3);
        // Single-line buffer → start of buffer.
        assert_eq!(handle_home("hello", 3), 0);
    }

    #[test]
    fn end_jumps_to_line_end() {
        // "ab\ncd" cursor on `c` (index 3) → end of line is index 5.
        assert_eq!(handle_end("ab\ncd", 3), 5);
        // No trailing newline → end of buffer.
        assert_eq!(handle_end("abc", 1), 3);
    }

    #[test]
    fn tab_mid_buffer_is_noop_and_resets_cycle() {
        let res = handle_tab("@foo bar", 3, None, &[]);
        assert_eq!(res.new_buf, "@foo bar");
        assert_eq!(res.new_cursor, 3);
        assert!(res.new_cycle.is_none());
    }
}
