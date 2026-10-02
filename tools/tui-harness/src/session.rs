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
//! root drives or kills it.
//!
//! A launched command starts under the harness binary's hidden
//! `__exec-env` subcommand ([`exec_env`]), which reads its environment and
//! working directory from a file, deletes the file, and execs the command.

use std::ffi::{OsStr, OsString};
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

/// The hidden subcommand of the `tui-harness` binary that starts a
/// launched command (see [`exec_env`]).
pub const EXEC_ENV: &str = "__exec-env";

/// The file in a session's state directory holding the command's
/// environment until the command starts.
const ENVIRON: &str = "environ";

/// Where [`exec_env`] reports a command it could not start.
const EXEC_ERR: &str = "exec.err";

/// How old an `environ` file or a metadata-less state directory must be
/// before `prune` treats it as left by an interrupted launch rather than a
/// launch in progress. A launch gives up after 5 s.
const STALE_AFTER: Duration = Duration::from_secs(10);

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
    // `-f /dev/null` matters only if this call starts the server, and then
    // it keeps the user's config (and plugins such as session restorers)
    // off the private server.
    c.args(["-L", TMUX_SOCKET, "-f", "/dev/null"]);
    c
}

/// Escape an argument so tmux reads it literally. tmux's argv parser ends a
/// command at any argument ending in `;`, and strips one backslash from a
/// trailing `\;`. So a trailing `;` gets one backslash before it, and tmux
/// hands the original back. No other character in an argument is special
/// to it: `{`, `}`, `#{...}`, `%`, `~` and `$` pass through `send-keys -l`
/// and `new-session` commands unchanged (checked against tmux 3.7). The
/// working directory never goes through tmux, whose `-c` expands formats.
pub fn tmux_literal(arg: &str) -> String {
    match arg.strip_suffix(';') {
        Some(head) => format!("{head}\\;"),
        None => arg.to_string(),
    }
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
/// Write the file [`exec_env`] reads: NUL-separated records, the first the
/// working directory (empty for none), then one `KEY=VALUE` per variable.
/// Created with `O_EXCL` and mode 0600, so it is private to the user and
/// never follows a planted link.
fn write_exec_file(path: &Path, cwd: Option<&Path>, env: &[(String, String)]) -> Result<()> {
    let mut body: Vec<u8> = Vec::new();
    if let Some(cwd) = cwd {
        body.extend_from_slice(&os_bytes(cwd.as_os_str()));
    }
    body.push(0);
    for (k, v) in env {
        body.extend_from_slice(k.as_bytes());
        body.push(b'=');
        body.extend_from_slice(v.as_bytes());
        body.push(0);
    }
    write_private(path, &body)
}

/// Create `path` with `O_EXCL` and mode 0600 and write `body`.
fn write_private(path: &Path, body: &[u8]) -> Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    std::io::Write::write_all(&mut f, body).with_context(|| format!("writing {}", path.display()))
}

#[cfg(unix)]
fn os_bytes(s: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn os_bytes(s: &OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn bytes_os(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(b).to_os_string()
}

#[cfg(not(unix))]
fn bytes_os(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

/// Whether a one-word command is a plain program name or path, with no
/// whitespace and nothing a shell would interpret. Such a word is executed
/// directly, so a missing program fails with `ENOENT` rather than as a
/// shell's exit 127; anything else runs through `$SHELL -c`.
fn is_plain_word(word: &OsStr) -> bool {
    let w = word.to_string_lossy();
    !w.is_empty()
        && !w
            .chars()
            .any(|c| c.is_whitespace() || "|&;<>()$`\\\"'*?[]#~=%{}!".contains(c))
}

/// Start a launched command: the body of `tui-harness __exec-env FILE --
/// CMD...`, which tmux runs as the pane's process.
///
/// It reads `file` (see [`write_exec_file`]) and deletes it before anything
/// else, then builds the command's environment from nothing: the pane's
/// `TERM`, `TMUX` and `TMUX_PANE`, `COLORTERM=truecolor`, then every pair in
/// the file, which wins. Pairs the OS would refuse (an empty name) are
/// skipped. It changes to the recorded directory and execs the command,
/// searching the new `PATH`. A one-word command that is a plain program name
/// or path is executed directly; any other one-word command is a shell
/// string and runs through `$SHELL -c` (else `/bin/sh -c`), as tmux runs
/// one. No shell parses the file, so any name or value, readonly in bash or
/// not, arrives intact.
///
/// It returns only on failure. The error is also appended to `exec.err`
/// beside the file, where `launch` looks for it.
pub fn exec_env(file: &Path, cmd: &[OsString]) -> std::io::Error {
    let err = exec_env_inner(file, cmd);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file.with_file_name(EXEC_ERR))
        .and_then(|mut f| {
            std::io::Write::write_all(
                &mut f,
                format!("tui-harness: cannot start the command: {err}\n").as_bytes(),
            )
        });
    err
}

fn exec_env_inner(file: &Path, cmd: &[OsString]) -> std::io::Error {
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => return e,
    };
    if let Err(e) = std::fs::remove_file(file) {
        return e;
    }
    let mut records = bytes.split(|b| *b == 0);
    let cwd = records.next().unwrap_or_default();
    let pairs: Vec<(OsString, OsString)> = records
        .filter_map(|r| {
            let eq = r.iter().position(|b| *b == b'=')?;
            let (k, v) = (&r[..eq], &r[eq + 1..]);
            (!k.is_empty()).then(|| (bytes_os(k), bytes_os(v)))
        })
        .collect();
    let Some((program, args)) = cmd.split_first() else {
        return std::io::Error::new(std::io::ErrorKind::InvalidInput, "no command");
    };
    let mut command = if args.is_empty() && !is_plain_word(program) {
        let shell = pairs
            .iter()
            .rev()
            .find(|(k, _)| k == "SHELL")
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| OsString::from("/bin/sh"));
        let mut c = Command::new(shell);
        c.arg("-c").arg(program);
        c
    } else {
        let mut c = Command::new(program);
        c.args(args);
        c
    };
    command.env_clear();
    for k in ["TERM", "TMUX", "TMUX_PANE"] {
        if let Some(v) = std::env::var_os(k) {
            command.env(k, v);
        }
    }
    command.env("COLORTERM", "truecolor");
    command.envs(pairs);
    if !cwd.is_empty() {
        command.current_dir(bytes_os(cwd));
    }
    exec(command)
}

#[cfg(unix)]
fn exec(mut command: Command) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    command.exec()
}

#[cfg(not(unix))]
fn exec(mut command: Command) -> std::io::Error {
    match command.status() {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => e,
    }
}

/// The `tui-harness` binary that runs [`exec_env`] in the pane: `explicit`
/// if given, else `$TUI_HARNESS_BIN`, else the running program if it is
/// `tui-harness`, else a `tui-harness` beside it or one directory up (a
/// test binary in `target/<profile>/deps`), else one on `PATH`.
pub fn helper_binary(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Some(p) = std::env::var_os("TUI_HARNESS_BIN").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let exe_name = format!("tui-harness{}", std::env::consts::EXE_SUFFIX);
    if let Ok(exe) = std::env::current_exe() {
        if exe.file_name().is_some_and(|n| n == exe_name.as_str()) {
            return Ok(exe);
        }
        let near = exe
            .parent()
            .into_iter()
            .flat_map(|d| [Some(d), d.parent()])
            .flatten();
        for dir in near {
            let candidate = dir.join(&exe_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(&exe_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!(
        "the tui-harness binary was not found to start the command; build it \
         (cargo build -p tui-harness) or set TUI_HARNESS_BIN"
    )
}

fn new_session_args(
    tmux_name: &str,
    opts: &LaunchOptions,
    cmd: &[String],
    helper: &Path,
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
    args.push(tmux_literal(&helper.display().to_string()));
    args.push(EXEC_ENV.into());
    args.push(tmux_literal(&env_file.display().to_string()));
    args.push("--".into());
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
    /// [`exec_env`] deletes before it starts the command.
    pub env: Vec<(String, String)>,
    /// The command's working directory. The default is the caller's. Any
    /// path works: it never passes through tmux's format expansion, and a
    /// directory that cannot be entered fails the launch.
    pub cwd: Option<PathBuf>,
    /// The `tui-harness` binary that starts the command. `None` finds it
    /// (see [`helper_binary`]).
    pub helper: Option<PathBuf>,
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
            helper: None,
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
    /// Only state from an interrupted launch was there; it was removed.
    StaleState,
}

/// What [`Harness::prune`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// tmux names of the sessions killed.
    pub killed: Vec<String>,
    /// Names whose state directory was removed.
    pub removed: Vec<String>,
    /// `environ` files deleted from directories that stay.
    pub scrubbed: Vec<PathBuf>,
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
        validate_env(&opts.env)?;
        let mut opts = opts.clone();
        opts.cols = opts.cols.clamp(1, MAX_CELLS);
        opts.rows = opts.rows.clamp(1, MAX_CELLS);
        let dir = self.sessions_dir().join(name);
        if dir.join("env").exists() {
            bail!("session '{name}' already exists (down it first)");
        }
        if dir.exists() {
            bail!("state from an interrupted launch of '{name}' is in the way (run `down {name}` or `prune`)");
        }
        let tmux_name = format!("{PREFIX}{name}");
        if has_session(&tmux_name) {
            bail!(
                "tmux session '{tmux_name}' already exists (another root's, or an orphan: see ls)"
            );
        }
        let root_tag = self.root_tag();
        if root_tag.contains(['\n', '\t']) {
            bail!("the state root's path holds a newline or tab: {root_tag:?}");
        }
        let helper = helper_binary(opts.helper.as_deref())?;
        if let Some(cwd) = &opts.cwd {
            if !cwd.is_dir() {
                bail!("working directory {} is not a directory", cwd.display());
            }
        }
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let session = Session {
            name: name.to_string(),
            tmux_name: tmux_name.clone(),
            cols: opts.cols,
            rows: opts.rows,
            font: opts.font.clone(),
            size: opts.size,
            cmd: cmd
                .iter()
                .map(|a| shell_quote(a))
                .collect::<Vec<_>>()
                .join(" "),
            root_tag: root_tag.clone(),
            dir: dir.clone(),
            shots_dir: self.shots_dir(),
        };
        // The metadata goes first, so that whatever interrupts the launch
        // leaves state that ls, down and prune can see.
        let env_file = dir.join(ENVIRON);
        let written = session
            .write_env()
            .and_then(|_| write_exec_file(&env_file, opts.cwd.as_deref(), &opts.env));
        if let Err(e) = written {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        let tmux_args = new_session_args(&tmux_name, &opts, cmd, &helper, &env_file, &root_tag);

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

        // setsid -f returns before tmux has made the session. Wait until the
        // session is up and tagged (the same command list tags it) or has
        // already come and gone, then until the command has taken its
        // environment file, then until it has started or failed to.
        let deadline = Instant::now() + Duration::from_secs(5);
        let err_path = dir.join(EXEC_ERR);
        let helper_name = helper
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        loop {
            // State removed under the launch (an explicit `down` of this
            // name): it cannot succeed. tmux may still be about to create
            // the session, so give it a moment to appear and kill it rather
            // than leave it to come up after the launch has failed.
            if !dir.join("env").exists() {
                let settle = Instant::now() + Duration::from_secs(1);
                while Instant::now() < settle && !has_session(&tmux_name) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                return Err(fail(format!(
                    "the state of '{name}' was removed while it launched"
                )));
            }
            let tagged = session_owner(&tmux_name).flatten().as_deref() == Some(root_tag.as_str());
            let taken = !env_file.exists();
            if let Ok(msg) = std::fs::read_to_string(&err_path) {
                if !msg.trim().is_empty() {
                    return Err(fail(msg.trim().to_string()));
                }
            }
            if taken
                && (!has_session(&tmux_name)
                    || (tagged
                        && pane_command(&tmux_name).as_deref() != Some(helper_name.as_str())))
            {
                break;
            }
            if Instant::now() > deadline {
                let why = if !tagged {
                    format!("tmux session '{tmux_name}' did not appear")
                } else if !taken {
                    format!("the command in '{tmux_name}' never took its environment file")
                } else {
                    format!("the command in '{tmux_name}' did not start")
                };
                return Err(fail(why));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // The helper writes its error before it exits, so a failure that
        // ended the session is on disk by now.
        if let Ok(msg) = std::fs::read_to_string(&err_path) {
            if !msg.trim().is_empty() {
                return Err(fail(msg.trim().to_string()));
            }
        }
        // Something removed this launch's state while it ran (a `down` of
        // this name, say): the launch did not complete.
        if !dir.join("env").exists() {
            return Err(fail(format!(
                "the state of '{name}' was removed while it launched"
            )));
        }
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
        Ok(self
            .state_dirs()
            .iter()
            .filter_map(|n| self.session(n).ok())
            .collect())
    }

    /// Names of every directory under `sessions/`, sorted.
    fn state_dirs(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.sessions_dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }

    /// State directories with no `env` metadata: what an interrupted launch
    /// leaves. They may hold the caller's environment in `environ`.
    pub fn stale(&self) -> Vec<String> {
        self.state_dirs()
            .into_iter()
            .filter(|n| !self.sessions_dir().join(n).join("env").exists())
            .collect()
    }

    /// `tui-*` sessions on the private server tagged with this root but with
    /// no state under it: what `prune` kills.
    pub fn orphans(&self) -> Vec<ServerSession> {
        let tag = self.root_tag();
        server_sessions()
            .into_iter()
            .filter(|s| s.owner.as_deref() == Some(tag.as_str()))
            .filter(|s| !self.sessions_dir().join(s.name()).join("env").exists())
            .collect()
    }

    /// `tui-*` sessions on the private server that no root tagged: from a
    /// manual tmux command, an older harness, or a lost tag. No root owns
    /// them, so nothing kills them but `prune --untagged`.
    pub fn untagged(&self) -> Vec<ServerSession> {
        server_sessions()
            .into_iter()
            .filter(|s| s.owner.is_none())
            .collect()
    }

    /// Clean up after this root: kill its orphans (and, with `untagged`,
    /// every untagged `tui-*` session), remove state whose session is gone
    /// and stale state from interrupted launches, and delete any `environ`
    /// file a launch left behind. Files younger than a launch's timeout are
    /// left alone, in case that launch is still running.
    pub fn prune(&self, untagged: bool) -> Result<PruneReport> {
        let mut report = PruneReport::default();
        let mut targets = self.orphans();
        if untagged {
            targets.extend(self.untagged());
        }
        for s in targets {
            kill_session(&s.tmux_name);
            report.killed.push(s.tmux_name);
        }
        for name in self.state_dirs() {
            let dir = self.sessions_dir().join(&name);
            // A directory still holding `environ` is a launch in progress
            // until the file outlives a launch's timeout.
            if launch_in_progress(&dir) {
                continue;
            }
            let gone = match self.session(&name) {
                Ok(s) => !s.alive(),
                Err(_) => older_than(&dir, STALE_AFTER),
            };
            if gone {
                std::fs::remove_dir_all(&dir)
                    .with_context(|| format!("removing {}", dir.display()))?;
                report.removed.push(name);
                continue;
            }
            let environ = dir.join(ENVIRON);
            if environ.exists() && older_than(&environ, STALE_AFTER) {
                std::fs::remove_file(&environ)
                    .with_context(|| format!("removing {}", environ.display()))?;
                report.scrubbed.push(environ);
            }
        }
        Ok(report)
    }

    /// Stop `name`: [`Session::down`] when it has metadata, else remove the
    /// stale state an interrupted launch left.
    pub fn down(&self, name: &str) -> Result<DownOutcome> {
        validate_name(name)?;
        let dir = self.sessions_dir().join(name);
        if dir.is_dir() && !dir.join("env").exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
            return Ok(DownOutcome::StaleState);
        }
        self.session(name)?.down()
    }

    /// Down every session of this root, then prune: its orphans, stale
    /// state and stray `environ` files. Sessions of other roots and
    /// untagged ones are left running.
    pub fn down_all(&self) -> Result<Vec<(String, DownOutcome)>> {
        let mut out = Vec::new();
        for s in self.list()? {
            if launch_in_progress(&s.dir) {
                continue;
            }
            let name = s.name.clone();
            out.push((name, s.down()?));
        }
        let report = self.prune(false)?;
        for tmux_name in report.killed {
            let name = tmux_name
                .strip_prefix(PREFIX)
                .unwrap_or(&tmux_name)
                .to_string();
            out.push((name, DownOutcome::Killed));
        }
        for name in report.removed {
            out.push((name, DownOutcome::StaleState));
        }
        Ok(out)
    }
}

/// Whether the state directory `dir` belongs to a launch still running: it
/// holds an `environ` file younger than [`STALE_AFTER`].
fn launch_in_progress(dir: &Path) -> bool {
    let environ = dir.join(ENVIRON);
    environ.exists() && !older_than(&environ, STALE_AFTER)
}

/// Whether everything in `path` (a file, or a directory and its entries)
/// was last modified more than `age` ago.
fn older_than(path: &Path, age: Duration) -> bool {
    let modified = |p: &Path| p.metadata().and_then(|m| m.modified()).ok();
    // A directory's age is that of its newest entry (its own mtime moves
    // whenever an entry is added or removed); an empty one uses its own.
    let entries = std::fs::read_dir(path)
        .map(|rd| rd.flatten().filter_map(|e| modified(&e.path())).max())
        .ok()
        .flatten();
    entries
        .or_else(|| modified(path))
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|elapsed| elapsed > age)
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
        write_private(&self.dir.join("env"), body.as_bytes())
    }

    fn pane(&self) -> String {
        format!("={}:", self.tmux_name)
    }

    /// Whether a tmux session of this name is running (whoever owns it).
    pub fn alive(&self) -> bool {
        has_session(&self.tmux_name)
    }

    /// `None` when no environment file is left in the state directory;
    /// otherwise whether it is stale (older than a launch's timeout, so left
    /// by an interrupted launch) rather than a launch in progress.
    pub fn environ_left(&self) -> Option<bool> {
        let environ = self.dir.join(ENVIRON);
        environ.exists().then(|| older_than(&environ, STALE_AFTER))
    }

    /// Who owns the running session of this name: `None` when none runs,
    /// `Some(None)` when it runs untagged, else `Some(Some(root))`.
    pub fn owner(&self) -> Option<Option<String>> {
        session_owner(&self.tmux_name)
    }

    /// Whether the running session of this name is this root's.
    pub fn owned(&self) -> bool {
        session_owner(&self.tmux_name).flatten().as_deref() == Some(self.root_tag.as_str())
    }

    /// Fail unless the running session of this name is this root's.
    fn ensure_owned(&self) -> Result<()> {
        match session_owner(&self.tmux_name) {
            None => bail!("session '{}' is not running", self.name),
            Some(Some(owner)) if owner == self.root_tag => Ok(()),
            Some(owner) => bail!(
                "session '{}' belongs to {}, not this root; refusing to touch it",
                self.name,
                owner.as_deref().unwrap_or("no root")
            ),
        }
    }

    /// Pass keys to `tmux send-keys`, same vocabulary: `"j"`, `"Enter"`,
    /// `"C-c"`, or `"-l", "literal text"`. Only the leading flags `-l`,
    /// `-H` and `-N <count>` are passed as flags (an optional `--` ends
    /// them); everything after is a key, so text may begin with `-` and no
    /// argument can retarget the command. A key or text ending in `;` is
    /// sent as typed (see [`tmux_literal`]). Refuses a session of another
    /// root.
    pub fn send<S: AsRef<str>>(&self, keys: &[S]) -> Result<()> {
        self.ensure_owned()?;
        let keys: Vec<&str> = keys.iter().map(AsRef::as_ref).collect();
        let mut args = vec!["send-keys".to_string(), "-t".into(), self.pane()];
        let mut i = 0;
        while i < keys.len() {
            match keys[i] {
                "-l" | "-H" => {
                    args.push(keys[i].to_string());
                    i += 1;
                }
                "-N" if i + 1 < keys.len() => {
                    let count: u32 = keys[i + 1]
                        .parse()
                        .with_context(|| format!("-N needs a count, not {:?}", keys[i + 1]))?;
                    args.push("-N".into());
                    args.push(count.to_string());
                    i += 2;
                }
                "--" => {
                    i += 1;
                    break;
                }
                _ => break,
            }
        }
        args.push("--".into());
        args.extend(keys[i..].iter().map(|k| tmux_literal(k)));
        tmux(&args).map(drop)
    }

    /// The pane contents. With `ansi`, SGR escapes are kept. Refuses a
    /// session of another root.
    pub fn text(&self, ansi: bool) -> Result<String> {
        self.ensure_owned()?;
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
    let tag = display(tmux_name, &format!("#{{{ROOT_OPTION}}}"))?;
    Some((!tag.is_empty()).then_some(tag))
}

/// Expand `format` against the session `tmux_name`, or `None` when no such
/// session runs. `display-message` answers a missing target with an empty
/// line and exit 0 (tmux 3.7), so the session name is expanded alongside
/// and checked.
fn display(tmux_name: &str, format: &str) -> Option<String> {
    let out = tmux_command()
        .args([
            "display-message",
            "-p",
            "-t",
            &format!("={tmux_name}:"),
            &format!("#{{session_name}}\t{format}"),
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (name, value) = text.trim_end_matches('\n').split_once('\t')?;
    (name == tmux_name).then(|| value.to_string())
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
            // A line without a tab is not one of ours (launch refuses a
            // root path holding a newline or tab).
            let (name, owner) = l.split_once('\t')?;
            name.starts_with(PREFIX).then(|| ServerSession {
                tmux_name: name.to_string(),
                owner: (!owner.is_empty()).then(|| owner.to_string()),
            })
        })
        .collect()
}

/// The pane's current command name, as tmux reports it.
fn pane_command(tmux_name: &str) -> Option<String> {
    display(tmux_name, "#{pane_current_command}")
}

fn kill_session(tmux_name: &str) {
    let _ = tmux_command()
        .args(["kill-session", "-t", &format!("={tmux_name}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Refuse an environment the exec file could not carry exactly: a NUL in a
/// name or value would split its record, and an empty name or one holding
/// `=` cannot be set.
fn validate_env(env: &[(String, String)]) -> Result<()> {
    for (k, v) in env {
        if k.is_empty() || k.contains(['=', '\0']) {
            bail!("environment variable name {k:?} is empty or holds '=' or NUL");
        }
        if v.contains('\0') {
            bail!("environment variable {k} holds a NUL byte");
        }
    }
    Ok(())
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
            Path::new("/bin/tui-harness;"),
            Path::new("/state/environ;"),
            "/root;",
        );
        // The only bare `;` separators are the harness's own.
        let separators = args.iter().filter(|a| a.as_str() == ";").count();
        assert_eq!(separators, SERVER_OPTIONS.len() + 2);
        assert!(args.contains(&"30\\;".to_string()));
        assert!(args.contains(&"/bin/tui-harness\\;".to_string()));
        assert!(args.contains(&"/state/environ\\;".to_string()));
        assert_eq!(args.last().unwrap(), "/root\\;");
        // Neither the environment nor the working directory goes to tmux.
        assert!(!args
            .iter()
            .any(|a| a == "-c" || a == "-e" || a.contains("#{q}")));
        let helper = args.iter().position(|a| a == EXEC_ENV).unwrap();
        assert_eq!(args[helper + 2], "--");
        assert_eq!(args[helper + 3], "sleep");
    }

    #[test]
    fn plain_words_run_directly_and_shell_strings_through_the_shell() {
        for w in [
            "nosuchprog_xyz",
            "/no/such/path/prog",
            "ways",
            "./a.out",
            "a-b_c.d+e",
        ] {
            assert!(is_plain_word(OsStr::new(w)), "{w}");
        }
        for w in ["echo hi", "a;b", "$HOME/x", "x*", "FOO=1", "a|b", "~/x", ""] {
            assert!(!is_plain_word(OsStr::new(w)), "{w}");
        }
    }

    #[test]
    fn env_that_cannot_round_trip_is_refused() {
        let ok = vec![("A".to_string(), "x=y\n'".to_string())];
        assert!(validate_env(&ok).is_ok());
        for (k, v) in [("", "v"), ("A=B", "v"), ("A\0B", "v"), ("A", "x\0B=1")] {
            assert!(
                validate_env(&[(k.to_string(), v.to_string())]).is_err(),
                "{k:?}={v:?}"
            );
        }
    }

    #[test]
    fn exec_file_holds_cwd_then_pairs() {
        let dir = std::env::temp_dir().join(format!("tui-harness-exec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(ENVIRON);
        let env = vec![
            ("UID".to_string(), "1000".to_string()),
            ("V".to_string(), "a=b\n'$(x)'".to_string()),
        ];
        write_exec_file(&path, Some(Path::new("/d#[x]")), &env).unwrap();
        let body = std::fs::read(&path).unwrap();
        assert_eq!(body, b"/d#[x]\0UID=1000\0V=a=b\n'$(x)'\0");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
        // O_EXCL: a second write never reuses an existing file.
        assert!(write_exec_file(&path, None, &env).is_err());
        let _ = std::fs::remove_dir_all(&dir);
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
