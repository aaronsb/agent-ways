//! The lifecycle convention (ADR-511): what every mutating `ways manage` verb
//! shares.
//!
//! - **What-if by default.** A verb prints what it would change and changes
//!   nothing until `--commit` is given. `--what-if` says so explicitly, and
//!   the older spellings stay accepted: `--dry-run` for what-if, `--yes` for
//!   commit. The verbs that honour `install.manage_mode` apply without the
//!   flag when it is `commit`; the flag on the command line wins.
//! - **One report shape.** A [`Report`] is a title and sections of marked
//!   items, rendered with semantic colour on a terminal and as the same text
//!   without colour in a pipe or under `NO_COLOR`.
//! - **One step scroll.** A run that applies prints a stage header per step
//!   and an ok, warn or fail mark per outcome ([`Steps`]), and keeps the raw
//!   output of the commands it runs for `--verbose` or a failure.
//! - **Dev links detected.** [`dev_links`] finds the binary links that point
//!   into a checkout other than the app (the `make dev-install` state), which
//!   an update or a repair would point back at the app.

use agent_theme::{ColorDepth, Painter, Role, Style};
use std::fmt::Write as _;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a verb applies or reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    WhatIf,
    Commit,
}

/// The apply flags every mutating verb takes.
#[derive(clap::Args, Debug, Clone, Copy, Default)]
pub struct Apply {
    /// Make the changes. Without it the command prints what it would
    /// change and changes nothing (`--yes` is accepted too)
    #[arg(long, alias = "yes", conflicts_with = "what_if")]
    pub commit: bool,
    /// Print what would change and change nothing: the default
    /// (`--dry-run` is accepted too)
    #[arg(long = "what-if", alias = "dry-run")]
    pub what_if: bool,
}

impl Apply {
    /// The mode these flags ask for. With neither flag, a verb that honours
    /// `install.manage_mode` takes it; every other verb reports.
    pub fn mode(self, honours_setting: bool) -> Mode {
        resolve(self.commit, self.what_if, honours_setting && ways_core::settings::manage_mode_commit())
    }
}

/// The mode from the flags and the setting: a flag wins, then the setting.
pub fn resolve(commit: bool, what_if: bool, setting_commits: bool) -> Mode {
    match (commit, what_if) {
        (true, _) => Mode::Commit,
        (_, true) => Mode::WhatIf,
        _ if setting_commits => Mode::Commit,
        _ => Mode::WhatIf,
    }
}

/// The line that ends every what-if report: how to apply it.
pub fn commit_hint(command: &str) -> String {
    format!("Nothing changed. Run `{command} --commit` to apply.")
}

// --- painting ---------------------------------------------------------------

/// The painter for a stream: the terminal palette where the stream is a
/// terminal (or colour is forced), plain text otherwise, and plain under
/// `NO_COLOR`, `TERM=dumb` or no `TERM`, attributes included.
pub fn painter_for(is_terminal: bool) -> Painter {
    let forced = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty() && v != "0");
    let depth = ColorDepth::detect();
    // NO_COLOR and TERM=dumb mean no escapes at all here, attributes
    // included: the report's text is the same either way.
    if depth != ColorDepth::NoColor && (is_terminal || forced("CLICOLOR_FORCE") || forced("FORCE_COLOR")) {
        Painter::terminal(depth)
    } else {
        Painter::plain()
    }
}

pub fn stdout_painter() -> Painter {
    painter_for(std::io::stdout().is_terminal())
}

pub fn stderr_painter() -> Painter {
    painter_for(std::io::stderr().is_terminal())
}

/// What an item is, which picks its mark and colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// A version or a state that moves forward.
    Up,
    /// A version that moves back.
    Down,
    /// Nothing changes.
    Same,
    /// Added: a feature, a new file, a link that will be made.
    Add,
    /// A fix.
    Fix,
    /// Removed or withdrawn.
    Remove,
    /// Something the commit run would do.
    Act,
    /// Needs the operator's attention.
    Warn,
    /// Neutral detail.
    Info,
    /// Succeeded (the step scroll).
    Ok,
    /// Failed (the step scroll).
    Fail,
}

impl Mark {
    fn glyph(self) -> &'static str {
        match self {
            Mark::Up => "↑",
            Mark::Down => "↓",
            Mark::Same => "=",
            Mark::Add => "+",
            Mark::Fix => "*",
            Mark::Remove => "-",
            Mark::Act => "→",
            Mark::Warn => "⚠",
            Mark::Info => "·",
            Mark::Ok => "✓",
            Mark::Fail => "✗",
        }
    }

    fn role(self) -> Role {
        match self {
            Mark::Up | Mark::Add | Mark::Ok => Role::Ok,
            Mark::Fix => Role::Info,
            Mark::Remove | Mark::Fail => Role::Err,
            Mark::Down => Role::Warn,
            Mark::Act => Role::Accent,
            Mark::Warn => Role::Warn,
            Mark::Same | Mark::Info => Role::Muted,
        }
    }
}

/// The width of a section rule.
const RULE: usize = 64;

/// A section header: `── Name ──────…`, the visual break between stages.
fn header(p: &Painter, name: &str) -> String {
    let fill = RULE.saturating_sub(name.chars().count() + 4).max(3);
    format!("{} {} {}", p.paint(Role::Rule, "──"), p.paint(Style::new().role(Role::Accent).bold(), name), p.paint(Role::Rule, "─".repeat(fill)))
}

#[derive(Debug, Clone)]
enum Line {
    Item { mark: Mark, indent: usize, text: String },
    /// A label in the mark's colour, then text: `feat  attend: …`.
    Tagged { mark: Mark, indent: usize, tag: String, width: usize, text: String },
    Sub(String),
    Text { indent: usize, text: String },
    Strong { indent: usize, text: String },
    Muted { indent: usize, text: String },
    Blank,
}

/// A staged report: a title, then sections of marked lines.
#[derive(Debug, Clone, Default)]
pub struct Report {
    title: String,
    subtitle: Vec<String>,
    sections: Vec<(String, Vec<Line>)>,
    footer: Vec<(Mark, String)>,
}

impl Report {
    pub fn new(title: impl Into<String>) -> Report {
        Report { title: title.into(), ..Default::default() }
    }

    /// A muted line under the title.
    pub fn subtitle(&mut self, text: impl Into<String>) -> &mut Self {
        self.subtitle.push(text.into());
        self
    }

    /// Open a section; the lines that follow belong to it.
    pub fn section(&mut self, name: impl Into<String>) -> &mut Self {
        self.sections.push((name.into(), Vec::new()));
        self
    }

    fn push(&mut self, line: Line) -> &mut Self {
        if self.sections.is_empty() {
            self.sections.push((String::new(), Vec::new()));
        }
        self.sections.last_mut().expect("a section").1.push(line);
        self
    }

    pub fn item(&mut self, mark: Mark, text: impl Into<String>) -> &mut Self {
        self.item_at(mark, 2, text)
    }

    /// An item at `indent`, as under a sub-heading.
    pub fn item_at(&mut self, mark: Mark, indent: usize, text: impl Into<String>) -> &mut Self {
        self.push(Line::Item { mark, indent, text: text.into() })
    }

    /// A tag in the mark's colour, padded to `width`, then text.
    pub fn tagged(&mut self, mark: Mark, indent: usize, tag: impl Into<String>, width: usize, text: impl Into<String>) -> &mut Self {
        self.push(Line::Tagged { mark, indent, tag: tag.into(), width, text: text.into() })
    }

    /// A bold sub-heading inside a section.
    pub fn sub(&mut self, text: impl Into<String>) -> &mut Self {
        self.push(Line::Sub(text.into()))
    }

    pub fn text(&mut self, indent: usize, text: impl Into<String>) -> &mut Self {
        self.push(Line::Text { indent, text: text.into() })
    }

    /// Bold text at `indent`: a heading inside quoted content.
    pub fn strong(&mut self, indent: usize, text: impl Into<String>) -> &mut Self {
        self.push(Line::Strong { indent, text: text.into() })
    }

    pub fn muted(&mut self, indent: usize, text: impl Into<String>) -> &mut Self {
        self.push(Line::Muted { indent, text: text.into() })
    }

    pub fn blank(&mut self) -> &mut Self {
        self.push(Line::Blank)
    }

    /// A closing line after the last section.
    pub fn footer(&mut self, mark: Mark, text: impl Into<String>) -> &mut Self {
        self.footer.push((mark, text.into()));
        self
    }

    /// The report as text, painted by `p`; [`Painter::plain`] gives the same
    /// structure with no escapes.
    pub fn render(&self, p: &Painter) -> String {
        let mut out = String::new();
        if !self.title.is_empty() {
            let _ = writeln!(out, "{}", p.paint(Style::new().bold(), &self.title));
        }
        for s in &self.subtitle {
            let _ = writeln!(out, "{}", p.paint(Role::Muted, s));
        }
        for (name, lines) in &self.sections {
            out.push('\n');
            if !name.is_empty() {
                let _ = writeln!(out, "{}", header(p, name));
            }
            for line in lines {
                match line {
                    Line::Item { mark, indent, text } => {
                        let _ = writeln!(out, "{:indent$}{} {text}", "", p.paint(mark.role(), mark.glyph()));
                    }
                    Line::Tagged { mark, indent, tag, width, text } => {
                        let pad = width.saturating_sub(tag.chars().count());
                        let sep = if text.is_empty() { "" } else { " " };
                        let _ = writeln!(out, "{:indent$}{}{:pad$}{sep}{text}", "", p.paint(mark.role(), tag), "");
                    }
                    Line::Sub(text) => {
                        let _ = writeln!(out, "  {}", p.paint(Style::new().bold(), text));
                    }
                    Line::Text { text, .. } if text.is_empty() => out.push('\n'),
                    Line::Text { indent, text } => {
                        let _ = writeln!(out, "{:indent$}{text}", "");
                    }
                    Line::Strong { indent, text } => {
                        let _ = writeln!(out, "{:indent$}{}", "", p.paint(Style::new().bold(), text));
                    }
                    Line::Muted { indent, text } => {
                        let _ = writeln!(out, "{:indent$}{}", "", p.paint(Role::Muted, text));
                    }
                    Line::Blank => out.push('\n'),
                }
            }
        }
        if !self.footer.is_empty() {
            out.push('\n');
            for (mark, text) in &self.footer {
                let _ = writeln!(out, "{} {}", p.paint(mark.role(), mark.glyph()), p.paint(Style::new().bold(), text));
            }
        }
        out
    }

    /// Print to stdout, coloured when stdout is a terminal.
    pub fn print(&self) {
        print!("{}", self.render(&stdout_painter()));
    }
}

// --- the step scroll ---------------------------------------------------------

static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Show the raw output of every command a commit run starts.
pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
}

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// The step scroll of a commit run, on stderr: a header per stage and a
/// mark per outcome.
pub struct Steps;

impl Steps {
    /// A stage header, the visual break between steps.
    pub fn stage(name: &str) {
        let p = stderr_painter();
        eprintln!("\n{}", header(&p, name));
    }

    pub fn ok(text: impl AsRef<str>) {
        Self::line(Mark::Ok, text.as_ref());
    }

    pub fn warn(text: impl AsRef<str>) {
        Self::line(Mark::Warn, text.as_ref());
    }

    pub fn fail(text: impl AsRef<str>) {
        Self::line(Mark::Fail, text.as_ref());
    }

    /// What is about to happen, for a step that takes a while.
    pub fn act(text: impl AsRef<str>) {
        Self::line(Mark::Act, text.as_ref());
    }

    pub fn info(text: impl AsRef<str>) {
        let p = stderr_painter();
        eprintln!("  {}", p.paint(Role::Muted, text.as_ref()));
    }

    pub fn line(mark: Mark, text: &str) {
        let p = stderr_painter();
        eprintln!("  {} {text}", p.paint(mark.role(), mark.glyph()));
    }

    /// Captured command output, dimmed and indented: a failure's evidence.
    pub fn output(out: &Output) {
        let p = stderr_painter();
        for line in String::from_utf8_lossy(&out.stdout).lines().chain(String::from_utf8_lossy(&out.stderr).lines()) {
            eprintln!("    {}", p.paint(Role::Muted, line));
        }
    }
}

/// Run a command for a commit run. Under `--verbose` its output streams as
/// it always did; otherwise it is captured and shown only when the command
/// fails. Returns whether it succeeded, and the captured output (empty under
/// `--verbose`).
pub fn run_quiet(cmd: &mut Command) -> std::io::Result<(bool, Output)> {
    if verbose() {
        let status = cmd.status()?;
        let empty = Output { status, stdout: Vec::new(), stderr: Vec::new() };
        return Ok((status.success(), empty));
    }
    let out = cmd.stdin(std::process::Stdio::null()).output()?;
    if !out.status.success() {
        Steps::output(&out);
    }
    Ok((out.status.success(), out))
}

/// [`run_quiet`], reduced to whether the command ran and succeeded.
pub fn run_quiet_ok(cmd: &mut Command) -> bool {
    run_quiet(cmd).is_ok_and(|(ok, _)| ok)
}

// --- dev links ---------------------------------------------------------------

/// A binary link that points into a checkout other than the app: the state
/// `make dev-install` leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevLink {
    pub link: PathBuf,
    pub points_to: PathBuf,
    pub checkout: PathBuf,
}

/// The directory `$XDG_BIN_HOME`, else `~/.local/bin`: where `make link`
/// puts the suite on PATH.
pub fn xdg_bin() -> PathBuf {
    crate::paths::xdg_dir("XDG_BIN_HOME").unwrap_or_else(|| claude_sessions::home_dir().join(".local").join("bin"))
}

/// The git checkout holding `path`: its nearest ancestor with a `.git`
/// directory or file (a worktree has a file).
pub fn checkout_of(path: &Path) -> Option<PathBuf> {
    path.ancestors().skip(1).find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

/// Whether the link at `link` points into a git checkout that is none of
/// `own`: the app the verb projects and the installed app.
pub fn dev_link(link: &Path, own: &[&Path]) -> Option<DevLink> {
    let to = std::fs::read_link(link).ok()?;
    let to = if to.is_absolute() { to } else { link.parent()?.join(to) };
    let to_c = canon(&to);
    if own.iter().any(|o| to_c.starts_with(canon(o))) {
        return None;
    }
    let checkout = checkout_of(&to_c)?;
    Some(DevLink { link: link.to_path_buf(), points_to: to, checkout })
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Every link of `bins` under `dirs` that points into another checkout.
pub fn dev_links(own: &[&Path], bins: &[String], dirs: &[PathBuf]) -> Vec<DevLink> {
    let mut out: Vec<DevLink> = Vec::new();
    for dir in dirs {
        for b in bins {
            if let Some(d) = dev_link(&dir.join(b), own).filter(|d| !out.iter().any(|o| o.link == d.link)) {
                out.push(d);
            }
        }
    }
    out
}

/// The links an update moves: the suite in `$XDG_BIN` (through `make
/// relink`, which does not link way-embed there), and the suite and
/// way-embed in each enabled target's `bin/` (through the repair).
pub fn update_dev_links(app: &Path) -> Vec<DevLink> {
    let data = crate::paths::data_root();
    let own = [app, data.as_path()];
    let mut out = dev_links(&own, &suite_bins(app), &[xdg_bin()]);
    for d in dev_links(&own, &linked_bins(app), &target_bin_dirs()) {
        if !out.iter().any(|o| o.link == d.link) {
            out.push(d);
        }
    }
    out
}

/// Each enabled target's `bin/`: the links a repair projects.
pub fn target_bin_dirs() -> Vec<PathBuf> {
    crate::config::global().targets().into_iter().filter(|t| t.enabled).map(|t| t.dir().join("bin")).collect()
}

/// The suite binaries `tools/suite-bins` in `app` lists.
pub fn suite_bins(app: &Path) -> Vec<String> {
    std::fs::read_to_string(app.join("tools/suite-bins"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_lowercase()))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect()
}

/// The suite and way-embed: every binary a target's `bin/` links.
pub fn linked_bins(app: &Path) -> Vec<String> {
    let mut bins = suite_bins(app);
    bins.push("way-embed".to_string());
    bins
}

/// `n` and the noun, singular or plural: `1 link`, `2 links`.
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `s` as one shell word: as it is when it needs no quoting, else in single
/// quotes, so a hint with a path holding a space pastes as one argument.
pub fn shell_word(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-~:@+=,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The report section for dev links: which checkout each group points
/// into, where the verb would point them, and, when that is the installed
/// app, the way back to the dev build.
pub fn dev_links_section(r: &mut Report, links: &[DevLink], what: &str, app: &Path) {
    if links.is_empty() {
        return;
    }
    r.section("Development setup");
    let mut groups: std::collections::BTreeMap<&Path, Vec<&DevLink>> = std::collections::BTreeMap::new();
    for d in links {
        groups.entry(d.checkout.as_path()).or_default().push(d);
    }
    for (checkout, group) in &groups {
        let verb = if group.len() == 1 { "points" } else { "point" };
        r.item(Mark::Warn, format!("{} {verb} into the checkout {}", count(group.len(), "binary link", "binary links"), checkout.display()));
        for d in group {
            r.muted(4, format!("{} -> {}", d.link.display(), d.points_to.display()));
        }
    }
    let them = if links.len() == 1 { "it" } else { "them" };
    let mut line = format!("{what} points {them} at {}.", app.display());
    if canon(app) == canon(&crate::paths::data_root()) {
        line.push_str(" Run `make dev-install` in the checkout afterwards to return to the dev build.");
    }
    r.text(2, line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_wins_over_the_setting() {
        assert_eq!(resolve(false, false, false), Mode::WhatIf, "the default is what-if");
        assert_eq!(resolve(false, false, true), Mode::Commit, "the setting commits");
        assert_eq!(resolve(true, false, false), Mode::Commit);
        assert_eq!(resolve(false, true, true), Mode::WhatIf, "--what-if beats the setting");
    }

    #[test]
    fn the_plain_render_keeps_the_structure_without_escapes() {
        let mut r = Report::new("ways manage update: what-if");
        r.subtitle("app /x");
        r.section("Versions").item(Mark::Up, "ways 1.0.0 → 1.1.0").item(Mark::Same, "attend 0.2.0 unchanged");
        r.section("Changes").tagged(Mark::Add, 4, "feat", 5, "ways: a thing (#1)").tagged(Mark::Fix, 4, "fix", 5, "attend: b");
        r.footer(Mark::Info, "Nothing was changed.");
        let plain = r.render(&Painter::plain());
        let rule = |n: &str| header(&Painter::plain(), n);
        assert_eq!(
            plain,
            format!(
                "ways manage update: what-if\napp /x\n\n{}\n  ↑ ways 1.0.0 → 1.1.0\n  = attend 0.2.0 unchanged\n\n\
                 {}\n    feat  ways: a thing (#1)\n    fix   attend: b\n\n· Nothing was changed.\n",
                rule("Versions"),
                rule("Changes")
            )
        );
        assert_eq!(rule("Versions").chars().count(), RULE, "a rule is RULE wide");
        let coloured = r.render(&Painter::terminal(ColorDepth::Ansi16));
        assert!(coloured.contains('\x1b'), "a terminal painter colours");
        let stripped: String = strip(&coloured);
        assert_eq!(stripped, plain, "colour adds escapes only");
    }

    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut it = s.chars();
        while let Some(c) = it.next() {
            if c == '\x1b' {
                for d in it.by_ref() {
                    if d == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn dev_links_group_once_per_checkout_whatever_their_order() {
        let link = |l: &str, c: &str| DevLink { link: PathBuf::from(l), points_to: PathBuf::from(format!("{c}/bin/x")), checkout: PathBuf::from(c) };
        let links = [link("/b/ways", "/devA"), link("/b/way-embed", "/devB"), link("/c/ways", "/devA")];
        let mut r = Report::new("");
        dev_links_section(&mut r, &links, "The update", Path::new("/not/the/app"));
        let text = r.render(&Painter::plain());
        assert_eq!(text.matches("into the checkout /devA").count(), 1, "{text}");
        assert!(text.contains("2 binary links point into the checkout /devA") && text.contains("1 binary link points into the checkout /devB"), "{text}");
        assert!(text.contains("The update points them at /not/the/app.") && !text.contains("make dev-install"), "the way back is only named for the installed app: {text}");
        let mut one = Report::new("");
        dev_links_section(&mut one, &links[1..2], "The repair", Path::new("/x"));
        assert!(one.render(&Painter::plain()).contains("The repair points it at /x."), "one link is \"it\"");
    }

    #[test]
    fn a_shell_word_quotes_only_what_needs_it() {
        assert_eq!(shell_word("~/.claude-work"), "~/.claude-work");
        assert_eq!(shell_word("/my dir/x"), "'/my dir/x'");
        assert_eq!(shell_word("it's"), "'it'\\''s'");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_into_another_checkout_is_a_dev_link_and_one_into_the_app_is_not() {
        let root = crate::test_dir::TestDir::at(std::env::temp_dir().join(format!("ways-lifecycle-{}", std::process::id())));
        let app = root.join("app");
        let dev = root.join("dev");
        let bins = root.join("bins");
        for d in [&app.join("bin"), &dev.join(".git"), &dev.join("bin"), &bins] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::create_dir_all(app.join(".git")).unwrap();
        std::fs::write(app.join("bin/ways"), "").unwrap();
        std::fs::write(dev.join("bin/attend"), "").unwrap();
        std::os::unix::fs::symlink(app.join("bin/ways"), bins.join("ways")).unwrap();
        std::os::unix::fs::symlink(dev.join("bin/attend"), bins.join("attend")).unwrap();
        std::fs::write(bins.join("plain"), "").unwrap();
        let found = dev_links(&[&app], &["ways".into(), "attend".into(), "plain".into(), "missing".into()], &[bins.to_path_buf(), bins.to_path_buf()]);
        assert_eq!(found.len(), 1, "a link seen twice is one: {found:?}");
        assert!(dev_links(&[&app, &dev], &["attend".into()], std::slice::from_ref(&bins)).is_empty(), "a link into an own app is not a dev link");
        assert_eq!(found[0].link, bins.join("attend"));
        assert_eq!(std::fs::canonicalize(&found[0].checkout).unwrap(), std::fs::canonicalize(&*dev).unwrap());
    }
}
