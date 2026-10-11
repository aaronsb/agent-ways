//! The left column (ADR-405): Agents, every identity the chat has seen,
//! live ones first and the rest set back; then Channels, every channel,
//! including empty ones and ones only `_groups.yaml` still names. A click
//! on an agent puts `@Name ` in the compose box; a click on a channel
//! shows it and opens its menu. `attend.chat.sidebar_key` shows or hides
//! it, and a terminal too narrow for it beside the feed hides it.

use agent_tui::ratatui::layout::{Constraint, Layout, Rect};
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::ratatui::widgets::Paragraph;
use agent_tui::ratatui::Frame;
use agent_tui::strip::{prefix_match, target};
use agent_tui::theme;
use agent_tui::wrap::str_width;

use super::{ChatPane, Chip};
use crate::chip::{color_for, Seen};
use crate::groups::ChannelRow;
use crate::helper::{self, HelperMode};
use crate::tabs::Tab;

/// The fewest columns the feed keeps beside the column; narrower, the
/// column hides.
const MIN_FEED: u16 = 50;

/// What a click on a row of the column does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SideHit {
    Agent(String),
    Channel(String),
}

/// The column's width in a pane `total` columns wide, or `None` when the
/// pane is too narrow to hold it beside the feed.
pub(super) fn width(total: u16) -> Option<u16> {
    let w = (total / 4).clamp(24, 34);
    (total >= w + MIN_FEED).then_some(w)
}

/// Whether the helper row has anything the column does not already show:
/// slash commands, their arguments and hints do; the agent and channel
/// lists are the column's while it is shown.
pub(super) fn helper_needed(text: &str) -> bool {
    !matches!(helper::derive(text), HelperMode::Agents(_) | HelperMode::Groups(_))
}

/// The seen identities in the column's order: live ones first, each group
/// in the registry's order (newest sender first).
pub(super) fn agent_order(seen: &[Seen]) -> Vec<&Seen> {
    let mut out: Vec<&Seen> = seen.iter().filter(|s| s.live).collect();
    out.extend(seen.iter().filter(|s| !s.live));
    out
}

/// How many rows each section gets in `h` rows: each its header and rows
/// when both fit; otherwise channels up to half, agents the rest.
pub(super) fn split(h: u16, agents: usize, channels: usize) -> (u16, u16) {
    let want_a = agents as u16 + 1;
    let want_c = channels as u16 + 1;
    if want_a + want_c <= h {
        return (want_a, h - want_a);
    }
    let c = want_c.min(h / 2).max(h.saturating_sub(want_a));
    (h - c, c)
}

/// A string cut to `max` columns, ending in `…` when cut.
pub(super) fn fit(s: &str, max: usize) -> String {
    if str_width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for ch in s.chars() {
        if str_width(&out) + str_width(&ch.to_string()) + 1 > max {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

/// The column in `area`, with a rule on its right edge.
pub(super) fn draw(chat: &mut ChatPane, f: &mut Frame, area: Rect) {
    let text = chat.input.text().to_string();
    let (agent_partial, channel_partial) = match helper::derive(&text) {
        HelperMode::Agents(p) => (p, None),
        HelperMode::Groups(p) => (None, p),
        _ => (None, None),
    };
    let fg = chat.normal_tab();
    let world = chat.world();
    let seen: Vec<Seen> = agent_order(&world.seen).into_iter().cloned().collect();
    let rows: Vec<(ChannelRow, usize)> = world.rows.iter().cloned().zip(world.live_members.iter().copied()).collect();
    let [inner, rule] = Layout::horizontal([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    for y in rule.top()..rule.bottom() {
        f.render_widget(Paragraph::new(Span::styled("│", theme::rule())), Rect { y, height: 1, ..rule });
    }
    let (ah, ch) = split(inner.height, seen.len(), rows.len());
    let agents = Rect { height: ah, ..inner };
    let channels = Rect { y: inner.y + ah, height: ch, ..inner };
    let live = seen.iter().filter(|s| s.live).count();
    header(f, agents, &format!("Agents {live}/{}", seen.len()));
    let w = inner.width.saturating_sub(1) as usize;
    let mut hits = Vec::new();
    for (i, s) in seen.iter().enumerate().take(ah.saturating_sub(1) as usize) {
        let r = Rect { y: agents.y + 1 + i as u16, height: 1, ..agents };
        let mark = target(Style::new(), prefix_match(&s.identity.nickname, agent_partial.as_deref()));
        f.render_widget(Paragraph::new(agent_line(chat, s, w, mark)), r);
        hits.push((r, SideHit::Agent(s.identity.nickname.clone())));
    }
    header(f, channels, &format!("Channels {}", rows.len()));
    let now = chat.now();
    for (i, (row, live)) in rows.iter().enumerate().take(ch.saturating_sub(1) as usize) {
        let r = Rect { y: channels.y + 1 + i as u16, height: 1, ..channels };
        let shown = fg == Tab::Channel(row.known.group.name.clone());
        let mark = target(Style::new(), prefix_match(&row.known.group.name, channel_partial.as_deref()));
        let age = row.last.and_then(|t| now.duration_since(t).ok()).map(age_label).unwrap_or_default();
        f.render_widget(Paragraph::new(channel_line(chat, row, *live, &age, shown, w, mark)), r);
        hits.push((r, SideHit::Channel(row.known.group.name.clone())));
    }
    chat.hits.side = hits;
}

/// A time since, in its largest whole unit: `40s`, `4m`, `3h`, `2d`.
pub(super) fn age_label(d: std::time::Duration) -> String {
    let s = d.as_secs();
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

fn header(f: &mut Frame, area: Rect, label: &str) {
    if area.height == 0 {
        return;
    }
    f.render_widget(Paragraph::new(Span::styled(format!(" {label}"), theme::muted().add_modifier(Modifier::BOLD))), Rect { height: 1, ..area });
}

/// An agent's row: its name in its identity colour, its project set back,
/// and `●` live or `○` not. An inactive agent is muted whole.
fn agent_line(chat: &ChatPane, s: &Seen, w: usize, mark: Style) -> Line<'static> {
    let depth = chat.palette.depth();
    let k = &s.identity;
    let project = agent_identity::cwd_basename(&k.cwd);
    let name_w = w.saturating_sub(4).min(str_width(&k.nickname));
    let name = fit(&k.nickname, name_w);
    let rest = w.saturating_sub(str_width(&name) + 4);
    let dot = if s.live { "●" } else { "○" };
    let style = if s.live { Style::new().fg(color_for(k.palette, depth)) } else { theme::muted() };
    let mut spans = vec![Span::raw(" "), Span::styled(name, style.patch(mark))];
    if rest >= 3 {
        spans.push(Span::styled(format!(" {}", fit(&project, rest - 1)), theme::muted()));
    }
    let used: usize = spans.iter().map(|s| s.width()).sum();
    spans.push(Span::raw(" ".repeat(w.saturating_sub(used + 1))));
    spans.push(Span::styled(dot, style));
    Line::from(spans)
}

/// A channel's row: the shown channel marked, then its glyph and name,
/// its member count and the time since its last message. A channel with
/// no live member, or none on disk, is set back.
fn channel_line(chat: &ChatPane, row: &ChannelRow, live: usize, age: &str, shown: bool, w: usize, mark: Style) -> Line<'static> {
    let depth = chat.palette.depth();
    let g = &row.known.group;
    let active = row.on_disk && (row.known.is_base || live > 0);
    let style = if active { Style::new().fg(color_for(g.palette, depth)) } else { theme::muted() };
    let figures = if row.known.is_base { age.to_string() } else { format!("{}👤 {age}", row.known.membership.members.len()).trim_end().to_string() };
    let lead = if shown { Span::styled(theme::SELECTED_MARK, theme::accent()) } else { Span::raw(" ") };
    let name_w = w.saturating_sub(str_width(&figures) + 4);
    let name = fit(&format!("{} #{}", g.glyph, g.name), name_w.max(4));
    let name_style = if shown { style.add_modifier(Modifier::BOLD) } else { style };
    let used = 1 + str_width(&name);
    Line::from(vec![
        lead,
        Span::styled(name, name_style.patch(mark)),
        Span::raw(" ".repeat(w.saturating_sub(used + str_width(&figures)).max(1))),
        Span::styled(figures, theme::muted()),
    ])
}

impl ChatPane {
    /// Whether the column is drawn in a pane `total` columns wide.
    pub(super) fn side_width(&self, total: u16) -> Option<u16> {
        if self.sidebar {
            width(total)
        } else {
            None
        }
    }

    /// Whether `k` is the key `attend.chat.sidebar_key` names.
    pub(super) fn is_sidebar_key(&self, k: &agent_tui::ratatui::crossterm::event::KeyEvent) -> bool {
        use agent_tui::ratatui::crossterm::event::{KeyCode, KeyModifiers};
        match self.config.sidebar_key.as_str() {
            "ctrl-b" => k.modifiers == KeyModifiers::CONTROL && k.code == KeyCode::Char('b'),
            "f3" => k.code == KeyCode::F(3),
            _ => false,
        }
    }

    /// Show or hide the column.
    pub(super) fn toggle_sidebar(&mut self) {
        self.sidebar = !self.sidebar;
        self.invalidate();
    }

    /// A click on a row of the column.
    pub(super) fn side_click(&mut self, hit: SideHit) {
        match hit {
            SideHit::Agent(name) => self.complete(Chip::Agent(name)),
            SideHit::Channel(name) => {
                let names = self.strip_names();
                if names.contains(&name) {
                    self.set_tab(Tab::Channel(name.clone()));
                }
                self.channel_menu(Some(name));
            }
        }
    }

    /// The key that toggles the column, as the help names it.
    pub(super) fn sidebar_key_label(&self) -> Option<&'static str> {
        match self.config.sidebar_key.as_str() {
            "ctrl-b" => Some("Ctrl+B"),
            "f3" => Some("F3"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_column_hides_when_the_feed_would_get_too_narrow() {
        assert_eq!(width(80), Some(24));
        assert_eq!(width(120), Some(30));
        assert_eq!(width(200), Some(34));
        assert_eq!(width(73), None, "24 + 50 columns is the least that holds it");
        assert_eq!(width(74), Some(24));
    }

    #[test]
    fn both_sections_get_their_rows_when_they_fit_else_channels_up_to_half() {
        assert_eq!(split(20, 3, 4), (4, 16), "agents take their header and rows; channels the rest");
        assert_eq!(split(10, 12, 12), (5, 5));
        assert_eq!(split(10, 12, 2), (7, 3), "a short channel list leaves the rest to agents");
        assert_eq!(split(10, 2, 12), (3, 7), "a short agent list leaves the rest to channels");
    }

    #[test]
    fn the_helper_row_stays_for_commands_and_goes_for_lists() {
        assert!(!helper_needed(""), "the agent legend is the column's");
        assert!(!helper_needed("hi #de"), "so is the channel list");
        assert!(helper_needed("/he"), "slash commands stay in the row");
    }

    #[test]
    fn an_age_is_its_largest_whole_unit() {
        let a = |s| age_label(std::time::Duration::from_secs(s));
        assert_eq!([a(5), a(240), a(7200), a(200_000)], ["5s", "4m", "2h", "2d"]);
    }

    #[test]
    fn fit_cuts_with_an_ellipsis() {
        assert_eq!(fit("agent-ways", 20), "agent-ways");
        assert_eq!(fit("agent-ways", 6), "agent…");
    }
}
