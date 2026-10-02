//! Keeping the tree in step with the files: the watch, and the reload that
//! takes the adapter's fresh tree while keeping what is pending, open and
//! under the cursor. Everything carried across is found again by its
//! dotted key, never by its position, since a fresh tree may order or
//! number its rows differently.

use std::collections::HashSet;

use super::*;

/// What a reload did to the pending edits.
#[derive(Debug, Default, PartialEq)]
pub struct Reloaded {
    /// Edits kept whose file changed under them: the value they replace is
    /// now the one on disk.
    pub moved: Vec<String>,
    /// Edits that could not be kept, with why.
    pub dropped: Vec<(String, String)>,
}

impl Reloaded {
    /// One line for the bottom bar, the dropped edits first; empty when
    /// every edit came through as it was.
    pub fn message(&self) -> String {
        let mut parts = Vec::new();
        if !self.dropped.is_empty() {
            let n = self.dropped.len();
            let each: Vec<String> = self.dropped.iter().map(|(k, why)| format!("{k}: {why}")).collect();
            parts.push(format!("{n} pending edit{} dropped: {}", if n == 1 { "" } else { "s" }, each.join("; ")));
        }
        if !self.moved.is_empty() {
            parts.push(format!(
                "{} changed on disk under a pending edit; the edit is kept, review shows the new value it would replace",
                self.moved.join(", ")
            ));
        }
        parts.join(" · ")
    }

    pub fn is_clean(&self) -> bool {
        self.moved.is_empty() && self.dropped.is_empty()
    }
}

/// The dotted key of each row of `rows`, by index.
fn keys_of(roots: &[Node], rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| tree::key(roots, &r.path)).collect()
}

impl App {
    /// Reload the tree when a file it was read from changed on disk.
    pub fn watch(&mut self) {
        let now = self.adapter.stamp();
        if now.is_some() && now != self.stamp && matches!(self.mode, Mode::Browse | Mode::Review { run: None, .. }) {
            let r = self.reload();
            self.msg = if r.is_clean() { "reloaded: a settings file changed on disk".into() } else { r.message() };
        }
    }

    /// Take the adapter's fresh tree. Pending edits are kept where their
    /// setting can still take them, and reported where it cannot. What is
    /// open, closed, under each cursor and marked failed is found again by
    /// key.
    pub fn reload(&mut self) -> Reloaded {
        self.stamp = self.adapter.stamp();
        let Some(mut fresh) = self.adapter.reload() else { return Reloaded::default() };

        // Where things are now, by key.
        let tab_keys = |app: &App, t: usize| keys_of(&app.roots, &tree::tab_rows(&app.roots, t));
        let cursor_key = keys_of(&self.roots, &self.rows()).get(self.cursor).cloned();
        let saved_keys: Vec<Option<String>> =
            (0..self.roots.len()).map(|t| tab_keys(self, t).get(self.saved[t]).cloned()).collect();
        let review_keys: Vec<Option<String>> = (0..self.roots.len())
            .map(|t| self.review_view(t).get(self.rcursor[t]).map(|r| tree::key(&self.roots, &r.path)))
            .collect();
        let closed: Vec<String> = self.closed.iter().map(|p| tree::key(&self.roots, p)).collect();
        let failed: Option<Vec<String>> = self.failure.as_ref().map(|f| f.paths.iter().map(|p| tree::key(&self.roots, p)).collect());

        // Open state and pending edits, by key.
        let mut kept: Kept = HashMap::new();
        for (path, key) in tree::keyed(&self.roots) {
            let n = tree::get(&self.roots, &path);
            let pending = n.setting.as_ref().filter(|s| s.changed()).map(|s| (s.loaded.clone(), s.value.clone()));
            kept.insert(key, (n.open, pending));
        }
        let mut report = Reloaded::default();
        let mut seen = HashSet::new();
        for (path, key) in tree::keyed(&fresh) {
            let Some((open, pending)) = kept.get(&key) else { continue };
            seen.insert(key.clone());
            let n = tree::get_mut(&mut fresh, &path);
            n.open = *open;
            if let Some((loaded, value)) = pending {
                match n.setting.as_mut() {
                    Some(s) if s.editable() => {
                        if s.loaded != *loaded {
                            report.moved.push(key.clone());
                        }
                        s.value = value.clone();
                    }
                    Some(s) => {
                        let why = s.locked.clone().unwrap_or_else(|| "it can no longer be changed here".into());
                        report.dropped.push((key.clone(), why));
                    }
                    None => report.dropped.push((key.clone(), "it is no longer a setting".into())),
                }
            }
        }
        let mut gone: Vec<_> = kept.iter().filter(|(k, (_, p))| p.is_some() && !seen.contains(*k)).map(|(k, _)| k.clone()).collect();
        gone.sort();
        report.dropped.extend(gone.into_iter().map(|k| (k, "it is gone from the settings".to_string())));
        self.roots = fresh;

        // Put everything back where its key now is.
        let n = self.roots.len();
        self.saved.resize(n + 1, 0);
        self.rcursor.resize(n, 0);
        self.tab = self.tab.min(n);
        for (t, key) in saved_keys.iter().enumerate().take(n) {
            if let Some(i) = key.as_ref().and_then(|k| tab_keys(self, t).iter().position(|x| x == k)) {
                self.saved[t] = i;
            }
        }
        if let Some(i) = cursor_key.and_then(|k| keys_of(&self.roots, &self.rows()).iter().position(|x| *x == k)) {
            self.cursor = i;
        }
        self.closed = closed.iter().filter_map(|k| tree::path_of(&self.roots, k)).collect();
        if let (Some(f), Some(keys)) = (self.failure.as_mut(), failed) {
            f.paths = keys.iter().filter_map(|k| tree::path_of(&self.roots, k)).collect();
        }
        for (t, key) in review_keys.iter().enumerate().take(n) {
            if let Some(i) = key.as_ref().and_then(|k| self.review_view(t).iter().position(|r| tree::key(&self.roots, &r.path) == *k)) {
                self.rcursor[t] = i;
            }
        }
        report
    }
}
