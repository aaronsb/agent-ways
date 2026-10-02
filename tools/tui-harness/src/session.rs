//! Detached tmux sessions: launch, send keys, capture, stop, list.
//!
//! tmux owns the PTY, the input, and a clean screen buffer. Each session has
//! a state directory holding an `env` file of its geometry, font and command.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::render::{Renderer, DEFAULT_FONT, DEFAULT_SIZE};
use crate::sgr;

pub const DEFAULT_COLS: u32 = 200;
pub const DEFAULT_ROWS: u32 = 50;

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

/// Geometry and font for a new session.
#[derive(Clone, Debug)]
pub struct LaunchOptions {
    pub cols: u32,
    pub rows: u32,
    pub font: String,
    pub size: u32,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        LaunchOptions {
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            font: DEFAULT_FONT.to_string(),
            size: DEFAULT_SIZE,
        }
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

    /// Start `cmd` in a detached tmux session of the requested geometry.
    pub fn launch(&self, name: &str, opts: &LaunchOptions, cmd: &[String]) -> Result<Session> {
        validate_name(name)?;
        if cmd.is_empty() {
            bail!("missing command");
        }
        let dir = self.sessions_dir().join(name);
        if dir.exists() {
            bail!("session '{name}' already exists (down it first)");
        }
        let tmux_name = format!("tui-{name}");
        if has_session(&tmux_name) {
            bail!("tmux session '{tmux_name}' already exists");
        }
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

        let mut tmux_args: Vec<String> = vec![
            "new-session".into(),
            "-d".into(),
            "-s".into(),
            tmux_name.clone(),
            "-x".into(),
            opts.cols.to_string(),
            "-y".into(),
            opts.rows.to_string(),
        ];
        tmux_args.extend(cmd.iter().cloned());

        // setsid -f so the tmux server survives the caller reaping our
        // descendants: a tmux server first started from a harness shell is
        // otherwise killed when that shell returns. Where setsid is missing
        // (macOS), tmux runs directly; it daemonises its server anyway.
        // tmux's stderr goes to a log file rather than a pipe: a pipe would
        // be held open by the forked tmux, and setsid's own exit status says
        // nothing about tmux's.
        let log_path = dir.join("launch.log");
        let spawn = |program: &str, pre: &[&str]| -> std::io::Result<std::process::ExitStatus> {
            let log = std::fs::File::create(&log_path)?;
            Command::new(program)
                .args(pre)
                .args(&tmux_args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log)
                .status()
        };
        let fail = |msg: String| -> anyhow::Error {
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
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

        // setsid -f returns before tmux has made the session; wait for it.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !has_session(&tmux_name) {
            if Instant::now() > deadline {
                return Err(fail(format!(
                    "tmux session '{tmux_name}' did not appear (did the command exit at once?)"
                )));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        // A pane border status line (set in some tmux configs) takes a row
        // from the pane. Turn it off for this window so the pane is the
        // geometry that was asked for.
        let _ = Command::new("tmux")
            .args([
                "set-option",
                "-w",
                "-t",
                &format!("={tmux_name}:"),
                "pane-border-status",
                "off",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

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

    /// Whether the tmux session is still running.
    pub fn alive(&self) -> bool {
        has_session(&self.tmux_name)
    }

    /// Pass keys to `tmux send-keys`, same vocabulary: `"j"`, `"Enter"`,
    /// `"C-c"`, or `"-l", "literal text"`.
    pub fn send<S: AsRef<str>>(&self, keys: &[S]) -> Result<()> {
        let mut args = vec!["send-keys".to_string(), "-t".into(), self.pane()];
        args.extend(keys.iter().map(|k| k.as_ref().to_string()));
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
        let path = match out {
            Some(p) => p.to_path_buf(),
            None => self
                .shots_dir
                .join(format!("{}-{}.png", self.name, timestamp())),
        };
        let renderer = Renderer::new(&self.font, self.size);
        let grid = sgr::parse(&self.text(true)?);
        renderer.render_to(&grid, Some(self.cols), Some(self.rows), &path)?;
        Ok(path)
    }

    /// Kill the tmux session and remove the state directory.
    pub fn down(self) -> Result<()> {
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &format!("={}", self.tmux_name)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::fs::remove_dir_all(&self.dir)
            .with_context(|| format!("removing {}", self.dir.display()))
    }
}

fn tmux(args: &[String]) -> Result<String> {
    let out = Command::new("tmux")
        .args(args)
        .output()
        .context("running tmux")?;
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
    Command::new("tmux")
        .args(["has-session", "-t", &format!("={tmux_name}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
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

/// `YYYYmmdd-HHMMSS` in UTC.
fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
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
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
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
    fn timestamp_shape() {
        let t = timestamp();
        assert_eq!(t.len(), 15);
        assert_eq!(&t[8..9], "-");
    }
}
