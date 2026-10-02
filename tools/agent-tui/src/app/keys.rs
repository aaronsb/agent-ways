//! Key and mouse handling, mode by mode. A click reaches the same paths a
//! key does, so the confirm and secret rules hold for both.

use super::*;

impl App {
    /// Handle one key. False ends the session.
    pub fn key(&mut self, k: KeyEvent) -> bool {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            // A running apply finishes its write steps, which are quick; a
            // command in flight takes a second ^C to stop. Anything pending
            // asks first.
            if let Mode::Review { run: Some(run), .. } = &self.mode {
                if run.waiting() {
                    if self.msg.starts_with("a command is running") {
                        self.stop_run("stopped by ^C");
                    } else {
                        self.msg = "a command is running: ^C again stops it".into();
                    }
                }
                return true;
            }
            if matches!(self.mode, Mode::Guard { .. }) {
                return true;
            }
            if matches!(self.mode, Mode::Flow(_)) {
                return self.flow_event(FlowEvent::Cancel);
            }
            return self.quit();
        }
        match std::mem::replace(&mut self.mode, Mode::Browse) {
            Mode::Flow(mut flow) => {
                let ev = flow.key(k);
                if ev == FlowEvent::Stay {
                    self.mode = Mode::Flow(flow);
                } else {
                    return self.flow_event(ev);
                }
            }
            Mode::Help { scroll } => match k.code {
                KeyCode::Up | KeyCode::Char('k') => self.mode = Mode::Help { scroll: scroll.saturating_sub(1) },
                KeyCode::Down | KeyCode::Char('j') => self.mode = Mode::Help { scroll: scroll.saturating_add(1) },
                KeyCode::PageUp => self.mode = Mode::Help { scroll: scroll.saturating_sub(10) },
                KeyCode::PageDown => self.mode = Mode::Help { scroll: scroll.saturating_add(10) },
                _ => {}
            },
            Mode::ThemeMenu { sel } => {
                let acts = self.theme_acts();
                match k.code {
                    KeyCode::Enter => self.theme_act(acts[sel.min(acts.len() - 1)]),
                    KeyCode::Up | KeyCode::Char('k') => self.mode = Mode::ThemeMenu { sel: sel.saturating_sub(1) },
                    KeyCode::Down | KeyCode::Char('j') => self.mode = Mode::ThemeMenu { sel: (sel + 1).min(acts.len() - 1) },
                    KeyCode::Esc | KeyCode::Char('q' | 'a') => {}
                    _ => self.mode = Mode::ThemeMenu { sel },
                }
            }
            Mode::ThemeName { op, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled".into(),
                KeyCode::Enter => self.theme_named(op, buf),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::ThemeName { op, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::ThemeName { op, buf };
                }
                _ => self.mode = Mode::ThemeName { op, buf },
            },
            Mode::ThemeDelete { name } => match k.code {
                KeyCode::Char('y' | 'Y') => self.theme_delete(&name),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => self.msg = "kept".into(),
                _ => self.mode = Mode::ThemeDelete { name },
            },
            Mode::ThemeUnsaved => self.unsaved_key(k),
            Mode::Filter => match k.code {
                KeyCode::Esc => self.clear_filter(),
                KeyCode::Enter => {}
                KeyCode::Backspace => {
                    self.filter.pop();
                    if self.filter.is_empty() {
                        self.cursor = self.saved[self.tab];
                    }
                    self.mode = Mode::Filter;
                }
                KeyCode::Char(c) => {
                    if self.filter.is_empty() {
                        self.saved[self.tab] = self.cursor;
                    }
                    self.filter.push(c);
                    self.cursor = 0;
                    self.mode = Mode::Filter;
                }
                _ => self.mode = Mode::Filter,
            },
            Mode::Edit(mut buf) => match k.code {
                KeyCode::Esc => self.msg = "edit cancelled".into(),
                KeyCode::Enter => self.commit(&buf),
                KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => self.mode = Mode::Edit(String::new()),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Edit(buf);
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Edit(buf);
                }
                _ => self.mode = Mode::Edit(buf),
            },
            Mode::Menu { path, sel } => {
                let len = tree::get(&self.roots, &path).actions.len();
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('a') => {}
                    KeyCode::Enter => self.pick(path, sel),
                    KeyCode::Up | KeyCode::Char('k') => self.mode = Mode::Menu { path, sel: sel.saturating_sub(1) },
                    KeyCode::Down | KeyCode::Char('j') => self.mode = Mode::Menu { path, sel: (sel + 1).min(len - 1) },
                    KeyCode::Char(c) if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => match self.action_for_key(&path, c) {
                        Some(i) => self.pick(path, i),
                        None => self.mode = Mode::Menu { path, sel },
                    },
                    _ => self.mode = Mode::Menu { path, sel },
                }
            }
            Mode::Arg { path, action, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled".into(),
                KeyCode::Enter if buf.trim().is_empty() => self.mode = Mode::Arg { path, action, buf },
                KeyCode::Enter => self.stage(&path, action, buf.trim(), None),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Arg { path, action, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Arg { path, action, buf };
                }
                _ => self.mode = Mode::Arg { path, action, buf },
            },
            Mode::Secret { path, action, mut buf } => match k.code {
                KeyCode::Esc => self.msg = "cancelled; nothing entered was kept".into(),
                KeyCode::Enter if buf.is_empty() => self.mode = Mode::Secret { path, action, buf },
                // The queued command shows `<stdin>`; the typed text rides
                // with it, redacted, to the command's stdin.
                KeyCode::Enter => self.stage(&path, action, "", Some(buf)),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Secret { path, action, buf };
                }
                KeyCode::Char(c) => {
                    if !buf.push(c) {
                        self.msg = format!(
                            "rejected: the entry holds at most {} bytes; what was typed past that is not kept. Esc and enter a shorter key",
                            SecretBuf::CAP
                        );
                    }
                    self.mode = Mode::Secret { path, action, buf };
                }
                _ => self.mode = Mode::Secret { path, action, buf },
            },
            Mode::Confirm { queued } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.enqueue(queued),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.msg = "not queued".into(),
                _ => self.mode = Mode::Confirm { queued },
            },
            Mode::Review { tab, run, discard } => self.review_key(k, tab, run, discard),
            Mode::DiscardTab { tab } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.discard_tab(tab),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {}
                _ => self.mode = Mode::DiscardTab { tab },
            },
            Mode::Guard { confirm: false } => match k.code {
                KeyCode::Char('r') => {
                    self.clear_filter();
                    match (0..self.roots.len()).find(|t| self.pending_in(*t) > 0) {
                        Some(first) => {
                            self.switch_tab(first);
                            self.open_review(first);
                        }
                        // Only theme edits are unsaved: their review is the editor.
                        None => self.switch_tab(self.theme_tab()),
                    }
                }
                KeyCode::Char('D') => self.mode = Mode::Guard { confirm: true },
                KeyCode::Esc | KeyCode::Char('b') | KeyCode::Enter => {}
                _ => self.mode = Mode::Guard { confirm: false },
            },
            Mode::Guard { confirm: true } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.discard_all();
                    return false;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.mode = Mode::Guard { confirm: false },
                _ => self.mode = Mode::Guard { confirm: true },
            },
            Mode::Browse if self.on_theme_tab() => return self.theme_key(k),
            Mode::Browse => return self.browse(k),
        }
        true
    }

    /// Review's keys. Moving and opening groups work; nothing edits or queues.
    /// A run in flight takes no keys.
    pub(super) fn review_key(&mut self, k: KeyEvent, tab: usize, run: Option<Run>, mut discard: bool) {
        self.mode = Mode::Review { tab, run, discard };
        if matches!(self.mode, Mode::Review { run: Some(_), .. }) {
            return;
        }
        if discard {
            match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.discard_tab(tab);
                    return self.advance(tab);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => discard = false,
                _ => {}
            }
            self.mode = Mode::Review { tab, run: None, discard };
            return;
        }
        let rows = self.review_view(tab);
        let last = rows.len().saturating_sub(1);
        let cur = self.rcursor[tab].min(last);
        let open = |app: &mut App, i: usize, on: bool| {
            if let Some(r) = rows.get(i).filter(|r| r.toggles()) {
                if on { app.closed.remove(&r.path) } else { app.closed.insert(r.path.clone()) };
            }
        };
        match k.code {
            KeyCode::Esc => {
                self.failure = None;
                self.msg.clear();
                self.mode = Mode::Browse;
            }
            KeyCode::Char('a') => self.begin_apply(tab),
            KeyCode::Char('X') => {
                if self.pending_in(tab) == 0 {
                    self.msg = format!("nothing pending in {}", self.roots[tab].name);
                } else {
                    self.mode = Mode::Review { tab, run: None, discard: true };
                }
            }
            KeyCode::Char('q') => {
                self.mode = Mode::Browse;
                self.quit();
            }
            KeyCode::Tab => self.review_to(self.next_pending(tab)),
            KeyCode::BackTab => {
                let n = self.roots.len();
                self.review_to((1..=n).map(|d| (tab + n - d) % n).find(|t| self.pending_in(*t) > 0));
            }
            KeyCode::Char(c @ '1'..='9') if (c as usize - '1' as usize) < self.roots.len() => {
                let to = c as usize - '1' as usize;
                if self.pending_in(to) > 0 {
                    self.review_to(Some(to));
                } else {
                    self.msg = format!("nothing pending in {}", self.roots[to].name);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.rcursor[tab] = cur.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.rcursor[tab] = (cur + 1).min(last),
            KeyCode::PageUp => self.rcursor[tab] = cur.saturating_sub(10),
            KeyCode::PageDown => self.rcursor[tab] = (cur + 10).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.rcursor[tab] = 0,
            KeyCode::End | KeyCode::Char('G') => self.rcursor[tab] = last,
            KeyCode::Right | KeyCode::Char('l') => match rows.get(cur) {
                Some(r) if r.toggles() && self.closed.contains(&r.path) => open(self, cur, true),
                Some(r) if r.toggles() => self.rcursor[tab] = (cur + 1).min(last),
                _ => {}
            },
            KeyCode::Left | KeyCode::Char('h') => match rows.get(cur) {
                Some(r) if r.toggles() && !self.closed.contains(&r.path) => open(self, cur, false),
                Some(r) => {
                    if let Some(parent) = rows[..cur].iter().rposition(|p| p.depth < r.depth) {
                        self.rcursor[tab] = parent;
                    }
                }
                None => {}
            },
            KeyCode::Enter | KeyCode::Char(' ') => match rows.get(cur) {
                Some(r) if r.toggles() => open(self, cur, self.closed.contains(&r.path)),
                _ => self.msg = READ_ONLY.into(),
            },
            KeyCode::Char('e' | 'd' | 'u' | 'x' | 'w' | 'c') => self.msg = READ_ONLY.into(),
            KeyCode::Char('m') => self.toggle_mouse(),
            _ => {}
        }
    }

    /// Show another tab's review; none when there is nowhere to go.
    pub(super) fn review_to(&mut self, to: Option<usize>) {
        if let (Some(to), Mode::Review { tab, .. }) = (to, &mut self.mode) {
            *tab = to;
            self.failure = None;
        }
    }

    pub(super) fn browse(&mut self, k: KeyEvent) -> bool {
        let rows = self.rows();
        if rows.is_empty() {
            match k.code {
                KeyCode::Char('q') => return self.quit(),
                KeyCode::Esc => self.clear_filter(),
                KeyCode::Char('w') => self.open_review(self.tab),
                KeyCode::Char('/') => self.mode = Mode::Filter,
                _ => {}
            }
            return true;
        }
        self.cursor = self.cursor.min(rows.len() - 1);
        let path = rows[self.cursor].path.clone();
        let last = rows.len() - 1;
        match k.code {
            KeyCode::Char('q') => return self.quit(),
            KeyCode::Esc if !self.filter.is_empty() => self.clear_filter(),
            KeyCode::Esc => return self.quit(),
            KeyCode::Char('w') => self.open_review(self.tab),
            KeyCode::Char('s') if k.modifiers.contains(KeyModifiers::CONTROL) => self.open_review(self.tab),
            KeyCode::Char('X') => self.ask_discard(self.tab),
            KeyCode::Tab => self.switch_tab((self.tab + 1) % self.tabs()),
            KeyCode::BackTab => self.switch_tab((self.tab + self.tabs() - 1) % self.tabs()),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < self.tabs() {
                    self.switch_tab(i);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(last),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(10),
            KeyCode::PageDown => self.cursor = (self.cursor + 10).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => self.cursor = last,
            KeyCode::Right | KeyCode::Char('l') => {
                let n = tree::get_mut(&mut self.roots, &path);
                if !n.children.is_empty() {
                    if n.open {
                        self.cursor = (self.cursor + 1).min(last);
                    } else {
                        n.open = true;
                    }
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                let n = tree::get_mut(&mut self.roots, &path);
                if n.open && !n.children.is_empty() && self.filter.is_empty() {
                    n.open = false;
                } else if path.len() > 1 {
                    let parent = &path[..path.len() - 1];
                    if let Some(i) = rows.iter().position(|r| r.path == parent) {
                        self.cursor = i;
                    }
                }
            }
            KeyCode::Enter if !self.filter.is_empty() => self.jump(&path),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(&path),
            KeyCode::Char('e') => self.begin_edit(&path),
            KeyCode::Char('d') => self.reset(&path, true),
            KeyCode::Char('u') => self.reset(&path, false),
            KeyCode::Char('a') => self.open_menu(&path),
            KeyCode::Char('x') => match self.queue.undo_last() {
                Some(q) => self.msg = format!("unqueued: {}", q.command),
                None => self.msg = "no queued action".into(),
            },
            KeyCode::Char('c') => self.show_changes = !self.show_changes,
            KeyCode::Char('m') => self.toggle_mouse(),
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('?') => self.mode = Mode::Help { scroll: 0 },
            KeyCode::Char(c) if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                let target = self.actions_at(&path);
                if let Some(i) = self.action_for_key(&target, c) {
                    self.pick(target, i);
                }
            }
            _ => {}
        }
        true
    }

    /// Handle one mouse event against the last frame's hits. A click reaches
    /// the same paths a key does, so the confirm and secret rules hold.
    /// During text, secret or filter entry and a confirm, only the confirm's
    /// answers respond.
    pub fn mouse(&mut self, m: MouseEvent) {
        let at = Position::new(m.column, m.row);
        let wheel = match m.kind {
            MouseEventKind::ScrollUp => Some(KeyCode::Up),
            MouseEventKind::ScrollDown => Some(KeyCode::Down),
            _ => None,
        };
        let click = m.kind == MouseEventKind::Down(MouseButton::Left);
        let press = |c| KeyEvent::new(c, KeyModifiers::NONE);
        if let Mode::Flow(flow) = &mut self.mode {
            let ev = match (wheel, click) {
                (Some(k), _) => flow.key(press(k)),
                (None, true) => flow.click(at),
                _ => FlowEvent::Stay,
            };
            self.flow_event(ev);
            return;
        }
        match &mut self.mode {
            Mode::Browse if self.tab == self.roots.len() => self.theme_mouse(m),
            Mode::Browse => {
                if let Some(k) = wheel {
                    self.browse(press(k));
                } else if click {
                    self.click_browse(at);
                }
            }
            Mode::Menu { sel, .. } | Mode::ThemeMenu { sel } => {
                if let Some(k) = wheel {
                    self.key(press(k));
                } else if click {
                    let Some((menu, items)) = &self.hits.menu else { return };
                    if let Some(i) = items.iter().position(|r| r.contains(at)) {
                        *sel = i;
                        self.key(press(KeyCode::Enter));
                    } else if !menu.contains(at) {
                        self.key(press(KeyCode::Esc));
                    }
                }
            }
            Mode::Review { run: Some(_), .. } => {}
            Mode::Confirm { .. } | Mode::DiscardTab { .. } | Mode::ThemeDelete { .. } | Mode::Guard { confirm: true } | Mode::Review { discard: true, .. } if click => {
                if let Some(&(_, yes)) = self.hits.answers.iter().find(|(r, _)| r.contains(at)) {
                    self.key(press(KeyCode::Char(if yes { 'y' } else { 'n' })));
                }
            }
            Mode::Review { discard: false, .. } => {
                if let Some(k) = wheel {
                    self.key(press(k));
                } else if click {
                    self.click_review(at);
                }
            }
            Mode::Guard { confirm: false } | Mode::ThemeUnsaved if click => self.click_button(at),
            Mode::Help { .. } if click => self.mode = Mode::Browse,
            _ => {}
        }
    }

    /// A click on a button is the key it stands for.
    pub(super) fn ask_discard(&mut self, tab: usize) {
        if self.pending_in(tab) == 0 {
            self.msg = format!("nothing pending in {}", self.roots[tab].name);
        } else {
            self.mode = Mode::DiscardTab { tab };
        }
    }

    pub(super) fn click_button(&mut self, at: Position) {
        if let Some(&(_, b)) = self.hits.buttons.iter().find(|(r, _)| r.contains(at)) {
            self.key(KeyEvent::new(b.key(), KeyModifiers::NONE));
        }
    }

    /// A click in review: a bar target, a tab, or a row, which takes the
    /// cursor; a click on a group's marker, or on the selected group, opens or
    /// closes it. Nothing a click does edits or queues.
    pub(super) fn click_review(&mut self, at: Position) {
        let Mode::Review { tab, .. } = self.mode else { return };
        if let Some(&(_, t)) = self.hits.discard_tabs.iter().find(|(r, _)| r.contains(at)) {
            self.review_to(Some(t));
            self.key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
            return;
        }
        if let Some(&(_, t)) = self.hits.tabs.iter().find(|(r, _)| r.contains(at)) {
            if t == self.theme_tab() {
                self.msg = "a theme saves on its own; nothing of it is reviewed".into();
                return;
            }
            return self.review_to(Some(t));
        }
        if self.hits.buttons.iter().any(|(r, _)| r.contains(at)) {
            return self.click_button(at);
        }
        let list = self.hits.list;
        if !list.contains(at) {
            return;
        }
        let rows = self.review_view(tab);
        let i = self.list.offset() + (at.y - list.y) as usize;
        let Some(row) = rows.get(i) else { return };
        let marker = list.x + 1 + review::GUTTER + 2 * row.depth as u16;
        if row.toggles() && (marker..marker + 2).contains(&at.x) {
            if !self.closed.remove(&row.path) {
                self.closed.insert(row.path.clone());
            }
            self.rcursor[tab] = i;
        } else if i == self.rcursor[tab] {
            self.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        } else {
            self.rcursor[tab] = i;
        }
    }

    /// A click while browsing: the call to action opens the review; a tab
    /// switches to it; a row selects it, and a second click on the selected
    /// row acts as Enter; a group's marker opens or closes it.
    pub(super) fn click_browse(&mut self, at: Position) {
        if self.hits.cta.contains(at) {
            return self.open_review(self.tab);
        }
        if let Some(&(_, tab)) = self.hits.discard_tabs.iter().find(|(r, _)| r.contains(at)) {
            return self.ask_discard(tab);
        }
        if let Some(&(_, tab)) = self.hits.tabs.iter().find(|(r, _)| r.contains(at)) {
            return self.switch_tab(tab);
        }
        let list = self.hits.list;
        if !list.contains(at) {
            return;
        }
        let i = self.list.offset() + (at.y - list.y) as usize;
        let rows = self.rows();
        let Some(row) = rows.get(i) else { return };
        // The selection mark takes one column, then two per level of depth.
        let marker = list.x + 1 + 2 * row.depth as u16;
        let n = tree::get_mut(&mut self.roots, &row.path);
        if self.filter.is_empty() && !n.children.is_empty() && (marker..marker + 2).contains(&at.x) {
            n.open = !n.open;
            self.cursor = i;
        } else if i == self.cursor {
            self.browse(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        } else {
            self.cursor = i;
        }
    }

    /// Enter: toggle a bool, cycle a choice, edit text and numbers, enter a
    /// secret masked, open the action menu of a node that only has actions, or
    /// open and close a group.
    pub(super) fn activate(&mut self, path: &[usize]) {
        let n = tree::get_mut(&mut self.roots, path);
        if !n.actions.is_empty() && n.setting.as_ref().is_some_and(|s| matches!(s.kind, Kind::ReadOnly)) {
            return self.open_menu(path);
        }
        if let Some(why) = n.setting.as_ref().and_then(|s| s.locked.clone()) {
            self.msg = format!("locked: {why}");
            return;
        }
        match n.setting.as_mut() {
            Some(s) => match &s.kind {
                Kind::Bool => {
                    s.value = if s.value == "true" { "false" } else { "true" }.into();
                    self.msg = format!("{} = {}", n.name, s.value);
                }
                Kind::Choice(opts) => {
                    let i = opts.iter().position(|o| *o == s.value).map_or(0, |i| (i + 1) % opts.len());
                    s.value = opts[i].clone();
                    self.msg = format!("{} = {}", n.name, s.value);
                }
                Kind::ReadOnly => self.msg = "read-only here; the detail pane names the command".into(),
                Kind::Secret => match n.actions.iter().position(|a| matches!(a.arg, Arg::Secret)) {
                    Some(action) => self.mode = Mode::Secret { path: path.to_vec(), action, buf: SecretBuf::default() },
                    None => self.msg = "no way to set this here".into(),
                },
                _ => self.begin_edit(path),
            },
            None => n.open = !n.open,
        }
    }

    pub(super) fn begin_edit(&mut self, path: &[usize]) {
        let n = tree::get(&self.roots, path);
        match &n.setting {
            Some(s) if s.editable() => self.mode = Mode::Edit(s.value.clone()),
            Some(Setting { locked: Some(why), .. }) => self.msg = format!("locked: {why}"),
            _ => self.msg = "nothing to edit".into(),
        }
    }

    /// The row's actions, or the tab's when the row has none.
    /// The node whose actions apply at `path`: its own, else its tab's.
    pub(super) fn actions_at(&self, path: &[usize]) -> Vec<usize> {
        let own = !tree::get(&self.roots, path).actions.is_empty();
        if own { path } else { &path[..1] }.to_vec()
    }

    /// The action of the node at `path` that `c` runs, by [`tree::action_keys`].
    pub(super) fn action_for_key(&self, path: &[usize], c: char) -> Option<usize> {
        tree::action_keys(&tree::get(&self.roots, path).actions).iter().position(|k| *k == Some(c))
    }

    pub(super) fn open_menu(&mut self, path: &[usize]) {
        let path = self.actions_at(path);
        if tree::get(&self.roots, &path).actions.is_empty() {
            self.msg = "no actions here".into();
        } else {
            self.mode = Mode::Menu { path, sel: 0 };
        }
    }

    /// An action was chosen: ask for its argument, or stage it at once.
    pub(super) fn pick(&mut self, path: Vec<usize>, action: usize) {
        match tree::get(&self.roots, &path).actions[action].arg.clone() {
            Arg::None => self.stage(&path, action, "", None),
            Arg::Text(_) => self.mode = Mode::Arg { path, action, buf: String::new() },
            Arg::Secret => self.mode = Mode::Secret { path, action, buf: SecretBuf::default() },
            Arg::Flow(name) => self.start_flow(&path, &name),
        }
    }

    /// Build the queued command; ask first when the action needs confirming.
    pub(super) fn stage(&mut self, path: &[usize], action: usize, text: &str, stdin: Option<SecretBuf>) {
        let a = &tree::get(&self.roots, path).actions[action];
        let mut queued = Queued::new(tree::key(&self.roots, path), a.label.clone(), a.render(text), a.confirm);
        queued.stdin = stdin;
        if a.confirm {
            self.mode = Mode::Confirm { queued };
        } else {
            self.enqueue(queued);
        }
    }

    pub(super) fn enqueue(&mut self, q: Queued) {
        self.msg = format!("queued: {}", q.command);
        self.queue.push(q);
    }

    pub(super) fn commit(&mut self, buf: &str) {
        let rows = self.rows();
        let path = rows[self.cursor.min(rows.len() - 1)].path.clone();
        let checked = {
            let s = tree::get(&self.roots, &path).setting.as_ref().expect("edit mode only on settings");
            match s.store.as_ref().and_then(|st| self.adapter.validate(st, buf)) {
                Some(r) => r,
                None => s.validate(buf),
            }
        };
        let n = tree::get_mut(&mut self.roots, &path);
        let s = n.setting.as_mut().expect("edit mode only on settings");
        match checked {
            Ok(v) => {
                s.value = v;
                self.msg = format!("{} = {}", n.name, s.value);
            }
            Err(e) => {
                self.msg = format!("rejected: {e}");
                self.mode = Mode::Edit(buf.to_string());
            }
        }
    }

    pub(super) fn reset(&mut self, path: &[usize], to_default: bool) {
        let n = tree::get_mut(&mut self.roots, path);
        if let Some(why) = n.setting.as_ref().and_then(|s| s.locked.clone()) {
            self.msg = format!("locked: {why}");
            return;
        }
        let Some(s) = n.setting.as_mut().filter(|s| s.kind.editable()) else {
            self.msg = "nothing to reset".into();
            return;
        };
        if to_default {
            match &s.default {
                Some(d) => {
                    s.value = d.clone();
                    self.msg = format!("{} = {} (default)", n.name, d);
                }
                None => self.msg = "no default".into(),
            }
        } else {
            s.value = s.loaded.clone();
            self.msg = format!("{} reverted", n.name);
        }
    }
}
