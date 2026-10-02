//! Detached tmux sessions: launch, send keys, capture, stop, list.
//!
//! tmux owns the PTY, the input, and a clean screen buffer. Each session has
//! a state directory holding an `env` file of its geometry, font and command.
//!
//! Every session runs on a private tmux server (socket [`TMUX_SOCKET`])
//! started without the user's tmux config, with its options set explicitly,
//! so a personal `~/.tmux.conf` never changes what a test sees. All roots
//! share that server and the `tui-<name>` namespace, so each session is
//! tagged with the root that launched it ([`ROOT_OPTION`]) and only that
//! root kills it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::render::{Renderer, DEFAULT_FONT, DEFAULT_SIZE, MAX_CELLS};
use crate::sgr;

pub const DEFAULT_COLS: u32 = 200;
pub const DEFAULT_ROWS: u32 = 50;

/// The socket name (`tmux -L`) of the harness's private tmux server.
pub const TMUX_SOCKET: &str = "agent-ways-tui";

/// The tmux user option holding the state root that launched a session.
pub const ROOT_OPTION: &str = "@tui_harness_root";

/// The prefix of every harness session's tmux name.
const PREFIX: &str = "tui-";

/// Server-wide options, applied in the same command list that creates a
/// session, so they hold before the first pane exists (`history-limit` only
/// affects panes created after it is set).
const SERVER_OPTIONS: [[&str; 2]; 4] = [
    ["status", "off"],
    ["pane-border-status", "off"],
    ["history-limit", "50000"],
    ["default-terminal", "tmux-256color"],
];

/// Variables never passed to a launched command: they describe the caller's
/// own terminal or shell, and tmux sets the pane's own.
const ENV_SKIP: [&str; 12] = [
    "TMUX",
    "TMUX_PANE",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "COLUMNS",
    "LINES",
    "SHLVL",
    "_",
    "PWD",
    "OLDPWD",
];

/// Variables kept in the environment of the tmux client that may start the
/// server, which becomes the server's global environment. Everything else
/// a command sees comes from its own launch (`-e`), so whoever started the
/// server leaves nothing behind in the next caller's sessions.
fn server_env_keeps(name: &str) -> bool {
    matches!(name, "PATH" | "HOME" | "TMUX_TMPDIR" | "LANG" | "LANGUAGE") || name.starts_with("LC_")
}

/// A `tmux` command aimed at the private server.
fn tmux_command() -> Command {
    let mut c = Command::new("tmux");
    c.args(["-L", TMUX_SOCKET]);
    c
}

/// Escape an argument so tmux reads it literally. tmux's argv parser ends a
/// command at any argument ending in `;`, and strips one backslash from a
/// trailing `\;`. So a trailing `;` gets one backslash before it, and tmux
/// hands the original back. No other character in an argument is special
/// to it: `{`, `}`, `#{...}`, `%`, `~` and `$` pass through `send-keys -l`,
/// `new-session` commands and `-e` unchanged (checked against tmux 3.7).
/// Options that tmux expands as formats (`-c`) also need
/// [`format_literal`].
pub fn tmux_literal(arg: &str) -> String {
    match arg.strip_suffix(';') {
        Some(head) => format!("{head}\\;"),
        None => arg.to_string(),
    }
}

/// Escape `#` for an option tmux expands as a format, such as
/// `new-session -c`, so `#{...}` in a path stays literal.
fn format_literal(s: &str) -> String {
    s.replace('#', "##")
}

/// The caller's environment, less the variables in [`ENV_SKIP`] and any that
/// are not UTF-8: what a launched command gets by default.
pub fn inherited_env() -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .filter(|(k, _)| !k.is_empty() && !k.contains('=') && !ENV_SKIP.contains(&k.as_str()))
        .collect();
    vars.sort();
    vars
}

/// The arguments after `tmux` that create a detached session on the private
/// server and tag it: no user config, the options, `new-session`, then
/// `set-option` with the root.
/// The shell program every launched command runs under. tmux starts it
/// with the pane's own `TERM`, `TMUX` and `TMUX_PANE`; it clears every other
/// variable, sources the environment file (`$1`), deletes it, and execs the
/// command. A one-word command runs through `$SHELL -c`, as tmux runs one.
/// The file, not `new-session -e`, carries the environment: a full
/// environment overflows tmux's message size ("command too long").
const ENV_WRAPPER: &str = r#"f=$1; shift
exec env -i TERM="$TERM" TMUX="$TMUX" TMUX_PANE="$TMUX_PANE" COLORTERM=truecolor /bin/sh -c '. "$0" && rm -f -- "$0"
if [ $# -eq 1 ]; then exec "${SHELL:-/bin/sh}" -c "$1"; fi
exec "$@"' "$f" "$@""#;

/// Write `env` as a file `/bin/sh` can source: one `export` per variable,
/// values single-quoted. Names that are not shell identifiers are dropped,
/// since no shell could export them. The file is private to the user.
fn write_env_file(path: &Path, env: &[(String, String)]) -> Result<()> {
    let mut body = String::new();
    for (k, v) in env {
        let ident = k
            .chars()
            .next()
            .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
            && k.chars().all(|c| c == '_' || c.is_ascii_alphanumeric());
        if ident {
            body.push_str(&format!("export {k}='{}'\n", v.replace('\'', r"'\''")));
        }
    }
    let mut file = std::fs::OpenOptions::new();
    file.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        file.mode(0o600);
    }
    let mut f = file
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    std::io::Write::write_all(&mut f, body.as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}

fn new_session_args(
    tmux_name: &str,
    opts: &LaunchOptions,
    cmd: &[String],
    env_file: &Path,
    root_tag: &str,
) -> Vec<String> {
    let mut args: Vec<String> = ["-L", TMUX_SOCKET, "-f", "/dev/null"]
        .map(String::from)
        .to_vec();
    for [opt, value] in SERVER_OPTIONS {
        args.extend(["set-option", "-g", opt, value, ";"].map(String::from));
    }
    // Colour depth for apps whose caller sets no COLORTERM.
    args.extend(["set-environment", "-g", "COLORTERM", "truecolor", ";"].map(String::from));
    args.extend(["new-session", "-d", "-s", tmux_name].map(String::from));
    args.extend([
        "-x".into(),
        opts.cols.to_string(),
        "-y".into(),
        opts.rows.to_string(),
    ]);
    if let Some(cwd) = &opts.cwd {
        args.push("-c".into());
        args.push(tmux_literal(&format_literal(&cwd.display().to_string())));
    }
    args.extend(["/bin/sh", "-c", ENV_WRAPPER, "tui-harness"].map(tmux_literal));
    args.push(tmux_literal(&env_file.display().to_string()));
    args.extend(cmd.iter().map(|a| tmux_literal(a)));
    args.push(";".into());
    args.extend(["set-option", "-t", &format!("={tmux_name}:"), ROOT_OPTION].map(String::from));
    args.push(tmux_literal(root_tag));
    args
}

/// Whether a `tmux` binary can be run.
pub fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The default state root: `$XDG_STATE_HOME/agent-ways/tui-harness`, with
/// `XDG_STATE_HOME` defaulting to `~/.local/state`.
pub fn default_state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| PathBuf::from(h).join(".local").join("state"))
        })
        .unwrap_or_else(|| PathBuf::from(".local/state"));
    base.join("agent-ways").join("tui-harness")
}

/// Geometry, font, environment and working directory for a new session.
#[derive(Clone, Debug)]
pub struct LaunchOptions {
    /// Columns, clamped to `1..=10000` (tmux's limit).
    pub cols: u32,
    /// Rows, clamped to `1..=10000`.
    pub rows: u32,
    pub font: String,
    pub size: u32,
    /// The command's whole environment, whoever started the tmux server.
    /// The default is the caller's ([`inherited_env`]). tmux adds `TERM`,
    /// `TMUX` and `TMUX_PANE`, and `COLORTERM` is `truecolor` unless set
    /// here. Nothing else reaches the command. It is passed through a file
    /// in the session's state directory, readable only by the user, which
    /// the command's shell deletes once it has read it.
    pub env: Vec<(String, String)>,
    /// The command's working directory. The default is the caller's.
    pub cwd: Option<PathBuf>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        LaunchOptions {
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            font: DEFAULT_FONT.to_string(),
            size: DEFAULT_SIZE,
            env: inherited_env(),
            cwd: std::env::current_dir().ok(),
        }
    }
}

/// What [`Session::down`] did with the tmux session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownOutcome {
    /// It was this root's, and it was killed.
    Killed,
    /// It was not running.
    AlreadyGone,
    /// A session of that name belongs to another root (or to none) and was
    /// left running. Only this root's state was removed.
    LeftRunning { owner: Option<String> },
}

/// A `tui-*` session on the private server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSession {
    pub tmux_name: String,
    /// The root that launched it; `None` when untagged.
    pub owner: Option<String>,
}

impl ServerSession {
    /// The harness name: the tmux name without `tui-`.
    pub fn name(&self) -> &str {
        self.tmux_name
            .strip_prefix(PREFIX)
            .unwrap_or(&self.tmux_name)
    }
}

/// A state root holding `sessions/<name>/env` and `shots/`.
#[derive(Clone, Debug)]
pub struct Harness {
    root: PathBuf,
}

impl Harness {
    /// A harness rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Harness {
        Harness { root: root.into() }
    }

    /// A harness rooted at [`default_state_dir`].
    pub fn from_env() -> Harness {
        Harness::new(default_state_dir())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    pub fn shots_dir(&self) -> PathBuf {
        self.root.join("shots")
    }

    /// The value sessions of this root are tagged with: the root's
    /// canonical path, so two spellings of one root agree.
    pub fn root_tag(&self) -> String {
        let abs = if self.root.is_absolute() {
            self.root.clone()
        } else {
            std::env::current_dir()
                .map(|d| d.join(&self.root))
                .unwrap_or_else(|_| self.root.clone())
        };
        abs.canonicalize().unwrap_or(abs).display().to_string()
    }

    /// Start `cmd` in a detached tmux session of the requested geometry,
    /// with `opts.env` as its environment and `opts.cwd` as its directory.
    pub fn launch(&self, name: &str, opts: &LaunchOptions, cmd: &[String]) -> Result<Session> {
        validate_name(name)?;
        if cmd.is_empty() {
            bail!("missing command");
        }
        let mut opts = opts.clone();
        opts.cols = opts.cols.clamp(1, MAX_CELLS);
        opts.rows = opts.rows.clamp(1, MAX_CELLS);
        let dir = self.sessions_dir().join(name);
        if dir.exists() {
            bail!("session '{name}' already exists (down it first)");
        }
        let tmux_name = format!("{PREFIX}{name}");
        if has_session(&tmux_name) {
            bail!(
                "tmux session '{tmux_name}' already exists (another root's, or an orphan: see ls)"
            );
        }
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let root_tag = self.root_tag();
        let env_file = dir.join("environ");
        if let Err(e) = write_env_file(&env_file, &opts.env) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        let tmux_args = new_session_args(&tmux_name, &opts, cmd, &env_file, &root_tag);

        // setsid -f so the tmux server survives the caller reaping our
        // descendants: a tmux server first started from a harness shell is
        // otherwise killed when that shell returns. Where setsid is missing
        // (macOS), tmux runs directly; it daemonises its server anyway.
        // tmux's stderr goes to a log file rather than a pipe: a pipe would
        // be held open by the forked tmux, and setsid's own exit status says
        // nothing about tmux's. The client runs with a minimal environment
        // (see `server_env_keeps`), since it may be the one that starts the
        // server.
        let log_path = dir.join("launch.log");
        let spawn = |program: &str, pre: &[&str]| -> std::io::Result<std::process::ExitStatus> {
            let log = std::fs::File::create(&log_path)?;
            Command::new(program)
                .args(pre)
                .args(&tmux_args)
                .env_clear()
                .envs(std::env::vars_os().filter(|(k, _)| k.to_str().is_some_and(server_env_keeps)))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log)
                .status()
        };
        let fail = |msg: String| -> anyhow::Error {
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            // The session may yet appear, or be up and untagged: never
            // leave it running without state.
            kill_session(&tmux_name);
            let _ = std::fs::remove_dir_all(&dir);
            match log.trim() {
                "" => anyhow::anyhow!(msg),
                l => anyhow::anyhow!("{msg}: {l}"),
            }
        };
        let status = match spawn("setsid", &["-f", "tmux"]) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => spawn("tmux", &[]),
            other => other,
        };
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => return Err(fail(format!("tmux new-session exited with {s}"))),
            Err(e) => return Err(fail(format!("running tmux new-session: {e}"))),
        }

        // setsid -f returns before tmux has made the session; wait for it
        // and for its tag, which the same command list sets. Then wait for
        // the command's shell to read and delete the environment file, so
        // the command has started with its environment when launch returns.
        let deadline = Instant::now() + Duration::from_secs(5);
        while session_owner(&tmux_name).flatten().as_deref() != Some(root_tag.as_str()) {
            if Instant::now() > deadline {
                return Err(fail(format!(
                    "tmux session '{tmux_name}' did not appear (did the command exit at once?)"
                )));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        while env_file.exists() {
            if Instant::now() > deadline {
                return Err(fail(format!(
                    "the command in '{tmux_name}' never read its environment file"
                )));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let session = Session {
            name: name.to_string(),
            tmux_name,
            cols: opts.cols,
            rows: opts.rows,
            font: opts.font.clone(),
            size: opts.size,
            cmd: cmd
                .iter()
                .map(|a| shell_quote(a))
                .collect::<Vec<_>>()
                .join(" "),
            root_tag,
            dir,
            shots_dir: self.shots_dir(),
        };
        session.write_env()?;
        Ok(session)
    }

    /// Open an existing session by name.
    pub fn session(&self, name: &str) -> Result<Session> {
        validate_name(name)?;
        let dir = self.sessions_dir().join(name);
        let env = std::fs::read_to_string(dir.join("env"))
            .map_err(|_| anyhow::anyhow!("no such session: {name}"))?;
        let get = |key: &str| {
            env.lines()
                .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
                .map(str::to_string)
                .with_context(|| format!("session '{name}' env lacks {key}"))
        };
        Ok(Session {
            name: name.to_string(),
            tmux_name: get("TMUX_NAME")?,
            cols: get("COLS")?.parse().context("COLS")?,
            rows: get("ROWS")?.parse().context("ROWS")?,
            font: get("FONT")?,
            size: get("FONT_SIZE")?.parse().context("FONT_SIZE")?,
            cmd: get("CMD").unwrap_or_default(),
            // The tag is always this root's: state found under a root
            // speaks for that root, whatever another copy of it says.
            root_tag: self.root_tag(),
            dir,
            shots_dir: self.shots_dir(),
        })
    }

    /// Every session with a state directory, sorted by name.
    pub fn list(&self) -> Result<Vec<Session>> {
        let Ok(entries) = std::fs::read_dir(self.sessions_dir()) else {
            return Ok(Vec::new());
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        Ok(names.iter().filter_map(|n| self.session(n).ok()).collect())
    }

    /// `tui-*` sessions on the private server with no state under this
    /// root that are this root's or untagged: what `prune` would kill.
    pub fn orphans(&self) -> Vec<ServerSession> {
        let tag = self.root_tag();
        server_sessions()
            .into_iter()
            .filter(|s| s.owner.as_deref().is_none_or(|o| o == tag))
            .filter(|s| !self.sessions_dir().join(s.name()).join("env").exists())
            .collect()
    }

    /// Kill this root's orphans (see [`Harness::orphans`]) and remove state
    /// whose session is gone. Returns the tmux names killed and the session
    /// names whose state was removed.
    pub fn prune(&self) -> Result<(Vec<String>, Vec<String>)> {
        let mut killed = Vec::new();
        for orphan in self.orphans() {
            kill_session(&orphan.tmux_name);
            killed.push(orphan.tmux_name);
        }
        let mut removed = Vec::new();
        for s in self.list()? {
            if !s.alive() {
                std::fs::remove_dir_all(&s.dir)
                    .with_context(|| format!("removing {}", s.dir.display()))?;
                removed.push(s.name);
            }
        }
        Ok((killed, removed))
    }

    /// Down every session of this root, then prune its orphans. Other
    /// roots' sessions are left running.
    pub fn down_all(&self) -> Result<Vec<(String, DownOutcome)>> {
        let mut out = Vec::new();
        for s in self.list()? {
            let name = s.name.clone();
            out.push((name, s.down()?));
        }
        for tmux_name in self.prune()?.0 {
            let name = tmux_name
                .strip_prefix(PREFIX)
                .unwrap_or(&tmux_name)
                .to_string();
            out.push((name, DownOutcome::Killed));
        }
        Ok(out)
    }
}

/// One launched session.
#[derive(Clone, Debug)]
pub struct Session {
    pub name: String,
    pub tmux_name: String,
    pub cols: u32,
    pub rows: u32,
    pub font: String,
    pub size: u32,
    /// The command, shell-quoted for display.
    pub cmd: String,
    root_tag: String,
    dir: PathBuf,
    shots_dir: PathBuf,
}

impl Session {
    fn write_env(&self) -> Result<()> {
        let body = format!(
            "TMUX_NAME={}\nCOLS={}\nROWS={}\nFONT={}\nFONT_SIZE={}\nCMD={}\n",
            self.tmux_name,
            self.cols,
            self.rows,
            one_line(&self.font),
            self.size,
            one_line(&self.cmd),
        );
        let path = self.dir.join("env");
        std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))
    }

    fn pane(&self) -> String {
        format!("={}:", self.tmux_name)
    }

    /// Whether a tmux session of this name is running (whoever owns it).
    pub fn alive(&self) -> bool {
        has_session(&self.tmux_name)
    }

    /// Whether the running session of this name is this root's.
    pub fn owned(&self) -> bool {
        session_owner(&self.tmux_name).flatten().as_deref() == Some(self.root_tag.as_str())
    }

    /// Pass keys to `tmux send-keys`, same vocabulary: `"j"`, `"Enter"`,
    /// `"C-c"`, or `"-l", "literal text"`. A key or text ending in `;` is
    /// sent as typed (see [`tmux_literal`]).
    pub fn send<S: AsRef<str>>(&self, keys: &[S]) -> Result<()> {
        let mut args = vec!["send-keys".to_string(), "-t".into(), self.pane()];
        args.extend(keys.iter().map(|k| tmux_literal(k.as_ref())));
        tmux(&args).map(drop)
    }

    /// The pane contents. With `ansi`, SGR escapes are kept.
    pub fn text(&self, ansi: bool) -> Result<String> {
        let mut args = vec![
            "capture-pane".to_string(),
            "-p".into(),
            "-t".into(),
            self.pane(),
        ];
        if ansi {
            args.push("-e".into());
        }
        tmux(&args)
    }

    /// Wait until the plain pane text contains `needle`, or fail after
    /// `timeout`. Returns the text that matched.
    pub fn wait_for(&self, needle: &str, timeout: Duration) -> Result<String> {
        let deadline = Instant::now() + timeout;
        loop {
            let text = self.text(false)?;
            if text.contains(needle) {
                return Ok(text);
            }
            if Instant::now() > deadline {
                bail!("timed out waiting for {needle:?}; pane was:\n{text}");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Render the pane to an image with this session's font and geometry.
    pub fn capture_image(&self) -> Result<image::RgbImage> {
        let renderer = Renderer::new(&self.font, self.size);
        self.capture_image_with(&renderer)
    }

    /// Render the pane with a caller-supplied renderer (to reuse loaded
    /// fonts across shots, or to render without fonts).
    pub fn capture_image_with(&self, renderer: &Renderer) -> Result<image::RgbImage> {
        let grid = sgr::parse(&self.text(true)?);
        Ok(renderer.render(&grid, Some(self.cols), Some(self.rows)))
    }

    /// Render the pane to a PNG at `out`, or to
    /// `<root>/shots/<name>-<UTC timestamp>.png`. Returns the path.
    pub fn shot(&self, out: Option<&Path>) -> Result<PathBuf> {
        self.shot_with(&Renderer::new(&self.font, self.size), out)
    }

    /// [`Session::shot`] with a caller-supplied renderer.
    pub fn shot_with(&self, renderer: &Renderer, out: Option<&Path>) -> Result<PathBuf> {
        let path = match out {
            Some(p) => p.to_path_buf(),
            None => unique_shot_path(&self.shots_dir, &self.name),
        };
        let grid = sgr::parse(&self.text(true)?);
        renderer.render_to(&grid, Some(self.cols), Some(self.rows), &path)?;
        Ok(path)
    }

    /// The command that attaches a terminal to this session, for a person
    /// who wants to look in: `tmux -L agent-ways-tui attach -t =tui-<name>`.
    /// `TMUX` is cleared so it also works from inside another tmux.
    pub fn attach_command(&self) -> Command {
        let mut c = tmux_command();
        c.args(["attach-session", "-t", &format!("={}", self.tmux_name)]);
        c.env_remove("TMUX");
        c
    }

    /// Kill the tmux session if it is this root's, then remove the state
    /// directory either way.
    pub fn down(self) -> Result<DownOutcome> {
        let outcome = match session_owner(&self.tmux_name) {
            None => DownOutcome::AlreadyGone,
            Some(Some(owner)) if owner == self.root_tag => {
                kill_session(&self.tmux_name);
                DownOutcome::Killed
            }
            Some(owner) => DownOutcome::LeftRunning { owner },
        };
        std::fs::remove_dir_all(&self.dir)
            .with_context(|| format!("removing {}", self.dir.display()))?;
        Ok(outcome)
    }
}

fn tmux(args: &[String]) -> Result<String> {
    let out = tmux_command().args(args).output().context("running tmux")?;
    if !out.status.success() {
        bail!(
            "tmux {} failed: {}",
            args.first().map(String::as_str).unwrap_or(""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn has_session(tmux_name: &str) -> bool {
    tmux_command()
        .args(["has-session", "-t", &format!("={tmux_name}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `None` when no such session runs; `Some(None)` when it runs untagged;
/// `Some(Some(root))` when it runs tagged with `root`.
fn session_owner(tmux_name: &str) -> Option<Option<String>> {
    let out = tmux_command()
        .args([
            "display-message",
            "-p",
            "-t",
            &format!("={tmux_name}:"),
            &format!("#{{{ROOT_OPTION}}}"),
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let tag = String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string();
    Some((!tag.is_empty()).then_some(tag))
}

/// Every `tui-*` session on the private server with its tag.
pub fn server_sessions() -> Vec<ServerSession> {
    let Ok(out) = tmux_command()
        .args([
            "list-sessions",
            "-F",
            &format!("#{{session_name}}\t#{{{ROOT_OPTION}}}"),
        ])
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (name, owner) = l.split_once('\t').unwrap_or((l, ""));
            name.starts_with(PREFIX).then(|| ServerSession {
                tmux_name: name.to_string(),
                owner: (!owner.is_empty()).then(|| owner.to_string()),
            })
        })
        .collect()
}

fn kill_session(tmux_name: &str) {
    let _ = tmux_command()
        .args(["kill-session", "-t", &format!("={tmux_name}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if !ok {
        bail!("session name must be letters, digits, '-' or '_': {name:?}");
    }
    Ok(())
}

fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

fn shell_quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// `<dir>/<name>-<timestamp>.png`, with `-2`, `-3`, ... added if that file
/// already exists.
fn unique_shot_path(dir: &Path, name: &str) -> PathBuf {
    let stamp = timestamp();
    let first = dir.join(format!("{name}-{stamp}.png"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{name}-{stamp}-{n}.png")))
        .find(|p| !p.exists())
        .expect("an unused name")
}

/// `YYYYmmdd-HHMMSS.mmm` in UTC.
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (secs, millis) = (now.as_secs(), now.subsec_millis());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}.{millis:03}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_and_names() {
        assert_eq!(shell_quote("ways"), "ways");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert!(validate_name("ok-name_1").is_ok());
        assert!(validate_name("../x").is_err());
        assert!(validate_name("").is_err());
    }

    #[test]
    fn trailing_semicolons_are_escaped_for_tmux() {
        assert_eq!(tmux_literal(";"), "\\;");
        assert_eq!(tmux_literal("a;"), "a\\;");
        assert_eq!(tmux_literal("semi;;"), "semi;\\;");
        // tmux strips one backslash from `\;`, so an argument that already
        // ends in `\;` gets another and comes back unchanged.
        assert_eq!(tmux_literal("b\\;"), "b\\\\;");
        assert_eq!(tmux_literal("x;y"), "x;y");
        assert_eq!(tmux_literal("#{session_name}"), "#{session_name}");
        assert_eq!(format_literal("/d#{x}"), "/d##{x}");
    }

    #[test]
    fn launch_args_escape_every_user_argument() {
        let opts = LaunchOptions {
            env: vec![("A".into(), "x;".into())],
            cwd: Some(PathBuf::from("/tmp/#{q};")),
            ..LaunchOptions::default()
        };
        let args = new_session_args(
            "tui-n",
            &opts,
            &["sleep".into(), "30;".into(), "new-session".into()],
            Path::new("/state/environ;"),
            "/root;",
        );
        // The only bare `;` separators are the harness's own.
        let separators = args.iter().filter(|a| a.as_str() == ";").count();
        assert_eq!(separators, SERVER_OPTIONS.len() + 2);
        assert!(args.contains(&"30\\;".to_string()));
        assert!(args.contains(&"/state/environ\\;".to_string()));
        assert!(args.contains(&"/tmp/##{q}\\;".to_string()));
        assert_eq!(args.last().unwrap(), "/root\\;");
    }

    #[test]
    fn inherited_env_skips_terminal_variables() {
        let env = inherited_env();
        assert!(env
            .iter()
            .all(|(k, _)| k != "TMUX" && k != "TERM" && k != "PWD"));
    }

    #[test]
    fn timestamp_shape() {
        let t = timestamp();
        assert_eq!(t.len(), 19);
        assert_eq!(&t[8..9], "-");
        assert_eq!(&t[15..16], ".");
    }

    #[test]
    fn shot_paths_never_collide() {
        let dir = std::env::temp_dir().join(format!("tui-harness-shots-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = unique_shot_path(&dir, "s");
        std::fs::write(&a, b"").unwrap();
        let b = unique_shot_path(&dir, "s");
        assert_ne!(a, b);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
