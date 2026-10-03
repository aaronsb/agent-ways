//! `ways-agent`: the ways agent's command line. `ways agent …` runs it.
//!
//! It runs the agent (`serve`, started on demand by hooks) and carries its
//! controls: the API key lifecycle, the model list and the agent's status
//! (ADR-196 §5, ADR-502 §6-7). The engine, model and mode are settings, set
//! with `ways settings set gate.…` (ADR-507).

use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use ways_agent::keys;
use ways_agent::net;
use ways_agent::profile::{self, Provider, UserLayer};
use ways_agent::report;

#[derive(Parser)]
#[command(
    name = "ways-agent",
    version,
    about = "The ways agent: relevance judging and key custody for agent-ways",
    after_help = "The engine is the setting gate.engine: `ways settings set gate.engine <profile>` (`ways settings help gate.engine` lists the profiles). Unset, the gate takes the first shipped profile whose provider has a key, anthropic before openrouter, so adding a key does not switch it; `ways agent status` says which applies."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add, check, rotate or remove a provider API key.
    ///
    /// A key does not choose the engine: that is the setting gate.engine,
    /// `ways settings set gate.engine <profile>`. Unset, the gate takes the
    /// first shipped profile whose provider has a key, anthropic first.
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    /// List a provider's models, with the recommended one marked.
    Models {
        /// anthropic or openrouter. Default: the configured engine's provider.
        #[arg(long)]
        provider: Option<String>,
        /// OpenRouter: list every model, not only Anthropic's.
        #[arg(long)]
        all: bool,
    },
    /// Run the agent in the foreground. Hooks start it on demand.
    #[command(hide = true)]
    Serve {
        /// Exit after this many minutes without a request.
        #[arg(long, default_value_t = 30)]
        idle_minutes: u64,
    },
    /// Report the running agent: engine, requests, fallbacks, latency.
    Status,
    /// Start the agent if it is not running, then report it.
    Load,
    /// Stop the running agent.
    Unload,
    /// Report what the judge has cost, from the events log.
    Cost {
        /// Only calls on or after this UTC date (YYYY-MM-DD).
        #[arg(long)]
        since: Option<String>,
        /// Only this session id.
        #[arg(long)]
        session: Option<String>,
        /// Only calls in this project path or under it.
        #[arg(long)]
        project: Option<String>,
        /// Group rows by session, project, day or month.
        #[arg(long, value_enum, default_value = "day")]
        by: ways_agent_core::spend::By,
        /// Print JSON with every grouping.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum KeyAction {
    /// Store a key. Reads --from-file, else stdin when piped, else a prompt that shows a dot per character.
    Add {
        #[arg(long)]
        provider: String,
        #[arg(long)]
        from_file: Option<PathBuf>,
        /// Store without calling the provider to check the key. The gate
        /// stays off until a check passes.
        #[arg(long)]
        no_check: bool,
        /// Replace a stored key even when the provider could not be reached
        /// to confirm the new one.
        #[arg(long)]
        force: bool,
    },
    /// Check stored keys with a call that costs nothing.
    Check {
        #[arg(long)]
        provider: Option<String>,
    },
    /// Replace an existing key, checking the new one first.
    Rotate {
        #[arg(long)]
        provider: String,
        #[arg(long)]
        from_file: Option<PathBuf>,
        /// Replace even when the provider could not be reached to confirm it.
        #[arg(long)]
        force: bool,
    },
    /// Delete a stored key file.
    Remove {
        #[arg(long)]
        provider: String,
    },
    /// Where each provider's key comes from, and its last four characters.
    Status,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ways agent: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Key { action } => match action {
            KeyAction::Add { provider, from_file, no_check, force } => {
                key_add(Provider::parse(&provider)?, from_file, !no_check, false, force)
            }
            KeyAction::Rotate { provider, from_file, force } => key_add(Provider::parse(&provider)?, from_file, true, true, force),
            KeyAction::Check { provider } => key_check(provider.as_deref()),
            KeyAction::Remove { provider } => key_remove(Provider::parse(&provider)?),
            KeyAction::Status => key_status(),
        },
        Command::Models { provider, all } => models(provider.as_deref(), all),
        Command::Serve { idle_minutes } => serve(idle_minutes),
        Command::Status => agent_status(false),
        Command::Load => {
            ways_agent::client::clear_start_backoff();
            agent_status(true)
        }
        Command::Cost { since, session, project, by, json } => {
            report::run(since.as_deref(), session.as_deref(), project.as_deref(), by, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Unload => {
            let stopped = ways_agent::server::stop()?;
            println!("{}", if stopped { "agent stopped" } else { "no agent running" });
            Ok(ExitCode::SUCCESS)
        }
    }
}

// ---------------------------------------------------------------- the agent

fn serve(idle_minutes: u64) -> Result<ExitCode> {
    let options = ways_agent::server::Options { idle: std::time::Duration::from_secs(idle_minutes.max(1) * 60) };
    if let Err(e) = ways_agent::server::serve(options) {
        // Started by a hook with no terminal: leave the reason where status can point.
        let log = ways_core::paths::state_root().join("ways-agent.log");
        let _ = std::fs::create_dir_all(log.parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(&log, format!("ways-agent serve failed: {e:#}\n"));
        return Err(e);
    }
    Ok(ExitCode::SUCCESS)
}

fn agent_status(start: bool) -> Result<ExitCode> {
    use ways_agent::protocol::{Reply, Request};
    let timeout = std::time::Duration::from_secs(5);
    let status = match ways_agent::client::call(Request::Status, timeout, start) {
        Ok(Reply::Status(s)) => s,
        Ok(other) => bail!("unexpected reply: {other:?}"),
        Err(reason) if reason == "agent_absent" => {
            println!("no agent running; hooks start one on the next gated prompt (or run `ways agent load`)");
            return Ok(ExitCode::SUCCESS);
        }
        Err(reason) => bail!("agent unreachable: {reason}"),
    };
    println!("ways-agent {} (pid {}), up {}s, socket {}", status.version, status.pid, status.uptime_s, ways_agent::protocol::socket_path().display());
    match (&status.engine, &status.model, status.mode) {
        (Some(e), Some(m), Some(mode)) => println!("engine {e} ({}; {m}), mode {}", engine_origin(e), mode.as_str()),
        _ => println!("gate off: no engine named and no key found"),
    }
    let pct = |v: Option<u64>| v.map(|ms| format!("{ms} ms")).unwrap_or_else(|| "—".into());
    let cap = if status.concurrency == 0 { "—".to_string() } else { status.concurrency.to_string() };
    println!(
        "requests {}, judged {}, in flight {}/{cap}, latency p50 {} p95 {}",
        status.requests, status.judged, status.in_flight, pct(status.latency_p50_ms), pct(status.latency_p95_ms)
    );
    if !status.fallbacks.is_empty() {
        let list: Vec<String> = status.fallbacks.iter().map(|(k, v)| format!("{k} {v}")).collect();
        println!("fallbacks: {}", list.join(", "));
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- keys

/// Whether a key may be written, given what its check found. A key the
/// provider rejected is never stored. A stored key is replaced only by one the
/// provider accepted, unless the operator forces it; a first key may be stored
/// unconfirmed; the agent checks it on first use and gates only once it passes.
fn may_store(result: Option<&net::Check>, replacing: bool, force: bool) -> Result<(), String> {
    match result {
        Some(net::Check::Invalid(message)) => Err(format!("the provider rejected the key ({message}); nothing was stored")),
        Some(r) if replacing && !force && !r.key_authenticated() => {
            Err(format!("could not confirm the new key ({r}); the stored key was kept. Retry, or pass --force"))
        }
        None if replacing && !force => Err("--no-check cannot replace a stored key without --force".to_string()),
        _ => Ok(()),
    }
}

fn key_add(provider: Provider, from_file: Option<PathBuf>, check: bool, rotate: bool, force: bool) -> Result<ExitCode> {
    let replacing = keys::key_path(provider).is_file();
    if rotate && !replacing {
        bail!("no stored {provider} key to rotate; use `ways agent key add --provider {provider}`");
    }
    let key = keys::validate(&read_secret(provider, from_file)?)?;
    let model = engine_model(provider)?;
    // Check before writing, so a bad key cannot displace a working one.
    let result = check.then(|| net::check(provider, &key, &model));
    may_store(result.as_ref(), replacing, force).map_err(|e| anyhow::anyhow!("{provider}: {e}"))?;
    let path = keys::store(provider, &key)?;
    println!("{provider} key {} stored at {} (mode 0600)", keys::tail(&key), path.display());
    let source = keys::Source::File(path);
    match &result {
        Some(r) => keys::record_check(provider, &keys::CheckRecord::now(r.record_word(), &model, &source)),
        None => keys::clear_check(provider),
    }
    if keys::env_set(provider) {
        println!("note: ${} is set and overrides this file for `ways agent` commands; hooks use the file", provider.key_env());
    }
    let Some(result) = result else {
        println!("not checked: the agent checks it on first use, and gates only once it passes");
        engine_note(provider);
        return Ok(ExitCode::SUCCESS);
    };
    println!("check: {result}");
    engine_note(provider);
    Ok(if result.is_valid() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// After a key is stored: the engine in effect, and when its provider is
/// not `provider`, the setting that switches to it. Adding a key never
/// switches the engine by itself.
fn engine_note(provider: Provider) {
    let (user, settings) = match current_settings() {
        Ok(s) => s,
        Err(e) => return println!("engine: {e:#}; `ways settings help gate.engine` lists the profiles"),
    };
    let Some(s) = settings else { return };
    let set = user.engine.is_some();
    println!("engine in effect: {} ({})", s.engine, origin(set));
    if s.profile.provider != provider {
        println!("to judge with {provider}: ways settings set gate.engine {provider}");
    }
}

/// Whether the engine `engine` was set in agent.yaml or picked by key order.
fn engine_origin(engine: &str) -> &'static str {
    let set = UserLayer::load_with_findings(&profile::user_layer_path()).is_ok_and(|(u, _)| u.engine.as_deref() == Some(engine));
    origin(set)
}

fn origin(set: bool) -> &'static str {
    if set {
        "set"
    } else {
        "picked: first key"
    }
}

fn key_check(only: Option<&str>) -> Result<ExitCode> {
    let providers = match only {
        Some(p) => vec![Provider::parse(p)?],
        None => Provider::ALL.to_vec(),
    };
    let mut checked = 0;
    let mut all_valid = true;
    for provider in providers {
        let Some((key, source)) = keys::read(provider)? else {
            if only.is_some() {
                bail!("no {provider} key; run `ways agent key add --provider {provider}`");
            }
            continue;
        };
        checked += 1;
        let model = engine_model(provider)?;
        let result = net::check(provider, &key, &model);
        keys::record_check(provider, &keys::CheckRecord::now(result.record_word(), &model, &source));
        all_valid &= result.is_valid();
        println!("{provider}: {} from {source}: {result}", keys::tail(&key));
        warn_exposure(&source);
    }
    if checked == 0 {
        println!("no keys; the gate is off. Run `ways agent key add --provider anthropic`.");
    }
    Ok(if all_valid { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn key_remove(provider: Provider) -> Result<ExitCode> {
    keys::clear_check(provider);
    if keys::remove(provider)? {
        println!("removed {}", keys::key_path(provider).display());
    } else {
        println!("no stored {provider} key");
    }
    if keys::env_set(provider) {
        println!("note: ${} is still set and supplies a key to `ways agent` commands; hooks use the file", provider.key_env());
    }
    Ok(ExitCode::SUCCESS)
}

fn key_status() -> Result<ExitCode> {
    for provider in Provider::ALL {
        match keys::read(provider) {
            Ok(Some((key, source))) => {
                let model = engine_model(provider)?;
                println!("{provider}: {} from {source}, {}", keys::tail(&key), check_note(provider, &source, &model));
                warn_exposure(&source);
            }
            Ok(None) => println!("{provider}: no key"),
            Err(e) => println!("{provider}: {e:#}"),
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// What the last check found, if it still describes this key and model.
fn check_note(provider: Provider, source: &keys::Source, model: &str) -> String {
    match keys::last_check(provider) {
        Some(r) if r.describes(source, model) => format!("checked {} ({} min ago)", r.result, r.age_s() / 60),
        _ => "not checked since it was stored; run `ways agent key check`".to_string(),
    }
}

fn warn_exposure(source: &keys::Source) {
    if let keys::Source::File(path) = source {
        for problem in keys::exposure(path) {
            println!("  warning: {problem}");
        }
    }
}

/// The key, from a file, from piped stdin, or from a prompt that shows a dot
/// for each character typed.
fn read_secret(provider: Provider, from_file: Option<PathBuf>) -> Result<String> {
    if let Some(path) = from_file {
        return std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()));
    }
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        let mut s = String::new();
        stdin.lock().read_to_string(&mut s).context("reading the key from stdin")?;
        return Ok(s);
    }
    eprint!("Paste your {provider} API key (shown as dots): ");
    read_hidden_line()
}

/// The terminal settings to put back if a signal ends the prompt.
#[cfg(unix)]
static mut SAVED_TERMIOS: Option<libc::termios> = None;

#[cfg(unix)]
extern "C" fn restore_and_exit(signal: libc::c_int) {
    // SAFETY: tcsetattr and _exit are async-signal-safe; SAVED_TERMIOS is
    // written once before this handler is installed and only read here.
    unsafe {
        if let Some(t) = *std::ptr::addr_of!(SAVED_TERMIOS) {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &t);
        }
        libc::_exit(128 + signal);
    }
}

/// What one byte typed at the key prompt does to the key read so far.
#[derive(Debug, PartialEq)]
enum Typed {
    /// The key is complete.
    Done,
    /// A character was added: draw one dot.
    Dot,
    /// Nothing to draw: a byte inside a character or an escape sequence,
    /// or a control byte.
    Continue,
    /// This many dots to erase.
    Erase(usize),
}

/// Where the editor is in an escape sequence: an arrow key, a function key,
/// or the markers a terminal puts around a bracketed paste. None of it is
/// part of the key.
#[derive(Debug, PartialEq)]
enum Escape {
    None,
    /// ESC seen.
    Start,
    /// ESC [ or ESC O seen: bytes until a final one in 0x40..=0x7E.
    Sequence,
}

/// The key as typed at the prompt. Erased bytes are zeroed before they are
/// dropped, so no copy of a removed character stays in the buffer.
struct KeyEditor {
    key: Vec<u8>,
    escape: Escape,
}

impl KeyEditor {
    fn new() -> KeyEditor {
        KeyEditor { key: Vec::with_capacity(512), escape: Escape::None }
    }

    /// Zero the key from `at` on and drop it.
    fn cut(&mut self, at: usize) {
        self.key[at..].iter_mut().for_each(|b| *b = 0);
        self.key.truncate(at);
    }

    /// Apply one byte typed at the prompt.
    fn feed(&mut self, b: u8) -> Typed {
        // The start of a UTF-8 character: not a continuation byte.
        let starts = |b: u8| b & 0xC0 != 0x80;
        // A control byte is never part of a sequence: Enter, Backspace or
        // Ctrl-U after a lone ESC, or inside a broken sequence, still acts.
        let control = b < 0x20 || b == 0x7f;
        match self.escape {
            Escape::Start if b == b'[' || b == b'O' => {
                self.escape = Escape::Sequence;
                return Typed::Continue;
            }
            Escape::Start | Escape::Sequence if control => self.escape = Escape::None,
            // A printable byte after a lone ESC is an Alt chord: dropped.
            Escape::Start => {
                self.escape = Escape::None;
                return Typed::Continue;
            }
            Escape::Sequence => {
                if (0x40..=0x7E).contains(&b) {
                    self.escape = Escape::None;
                }
                return Typed::Continue;
            }
            Escape::None => {}
        }
        match b {
            b'\r' | b'\n' => Typed::Done,
            // Ctrl-D on an empty line ends it as Enter would; otherwise ignored.
            0x04 if self.key.is_empty() => Typed::Done,
            0x1b => {
                self.escape = Escape::Start;
                Typed::Continue
            }
            0x7f | 0x08 => match self.key.iter().rposition(|&b| starts(b)) {
                Some(at) => {
                    self.cut(at);
                    Typed::Erase(1)
                }
                None => Typed::Erase(0),
            },
            // Ctrl-U clears the line.
            0x15 => {
                let n = self.key.iter().filter(|&&b| starts(b)).count();
                self.cut(0);
                Typed::Erase(n)
            }
            b if b < 0x20 => Typed::Continue,
            b => {
                self.key.push(b);
                if starts(b) { Typed::Dot } else { Typed::Continue }
            }
        }
    }
}

/// The value that turns off a terminal control character.
#[cfg(target_os = "linux")]
const CC_DISABLED: libc::cc_t = 0;
#[cfg(all(unix, not(target_os = "linux")))]
const CC_DISABLED: libc::cc_t = 0xff;

#[cfg(unix)]
fn read_hidden_line() -> Result<String> {
    use std::io::Write;
    const SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];
    // SAFETY: termios calls on stdin with a struct tcgetattr fills; the
    // original settings are restored on every path, including a signal
    // through the handler above. Echo is never left on: a failed switch reads
    // nothing.
    unsafe {
        let mut original: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut original) != 0 {
            bail!("cannot read terminal settings; pipe the key on stdin or use --from-file");
        }
        *std::ptr::addr_of_mut!(SAVED_TERMIOS) = Some(original);
        let previous: Vec<libc::sighandler_t> =
            SIGNALS.iter().map(|&sig| libc::signal(sig, restore_and_exit as *const () as libc::sighandler_t)).collect();
        let put_back = |previous: &[libc::sighandler_t]| {
            for (&sig, &handler) in SIGNALS.iter().zip(previous) {
                libc::signal(sig, handler);
            }
        };
        // Byte at a time, no echo: each character draws its own dot. Ctrl-\
        // and Ctrl-Z are off, so neither can stop or kill the prompt with the
        // terminal raw; Ctrl-C still ends it, through the handler.
        let mut raw = original;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        raw.c_cc[libc::VQUIT] = CC_DISABLED;
        raw.c_cc[libc::VSUSP] = CC_DISABLED;
        #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
        {
            raw.c_cc[libc::VDSUSP] = CC_DISABLED;
        }
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
            put_back(&previous);
            bail!("cannot turn off terminal echo; pipe the key on stdin or use --from-file");
        }
        let mut editor = KeyEditor::new();
        let mut err = std::io::stderr();
        let result = loop {
            let mut b = 0u8;
            match libc::read(libc::STDIN_FILENO, (&mut b as *mut u8).cast(), 1) {
                1 => {}
                0 => break Err(std::io::Error::other("the terminal closed before Enter")),
                _ => break Err(std::io::Error::last_os_error()),
            }
            match editor.feed(b) {
                Typed::Done => break Ok(()),
                Typed::Dot => drop(write!(err, "•")),
                Typed::Erase(n) => drop(write!(err, "{}", "\x08 \x08".repeat(n))),
                Typed::Continue => {}
            }
            let _ = err.flush();
        };
        // TCSAFLUSH drops what was typed past Enter: the rest of a multi-line
        // paste never reaches the shell.
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &original);
        put_back(&previous);
        eprintln!();
        result.context("reading the key")?;
        String::from_utf8(std::mem::take(&mut editor.key)).context("the key is not UTF-8 text")
    }
}

#[cfg(not(unix))]
fn read_hidden_line() -> Result<String> {
    bail!("hidden input needs a Unix terminal; pipe the key on stdin or use --from-file")
}

// ---------------------------------------------------------------- engine

/// The model to check a key against: the configured engine's model when it
/// uses this provider, else the provider's recommended model.
fn engine_model(provider: Provider) -> Result<String> {
    let user = UserLayer::load(&profile::user_layer_path())?;
    let settings = profile::resolve(&user, |p| p == provider)?;
    Ok(match settings {
        Some(s) if s.profile.provider == provider => s.profile.model,
        _ => provider.recommended_model().to_string(),
    })
}

fn current_settings() -> Result<(UserLayer, Option<profile::Settings>)> {
    let user = UserLayer::load(&profile::user_layer_path())?;
    let settings = profile::resolve(&user, |p| keys::locate(p).is_some())?;
    Ok((user, settings))
}

fn models(provider: Option<&str>, all: bool) -> Result<ExitCode> {
    let provider = match provider {
        Some(p) => Provider::parse(p)?,
        None => current_settings()?.1.map(|s| s.profile.provider).unwrap_or(Provider::Anthropic),
    };
    let key = keys::read(provider)?.map(|(k, _)| k);
    let mut list = net::models(provider, key.as_deref())?;
    if provider == Provider::Openrouter && !all {
        list.retain(|m| m.id.starts_with("anthropic/"));
    }
    let recommended = provider.recommended_model();
    let base = list.iter().find(|m| provider.is_recommended(&m.id)).cloned();
    list.sort_by_key(|m| (!provider.is_recommended(&m.id), m.id.clone()));
    println!("{provider} models (the gate is tuned for {recommended}):");
    for m in &list {
        let price = match (m.input_per_mtok, m.output_per_mtok) {
            (Some(i), Some(o)) => format!("  ${i:.2}/${o:.2} per Mtok"),
            _ => String::new(),
        };
        let note = if provider.is_recommended(&m.id) {
            "  ★ recommended".to_string()
        } else {
            cost_note(m, base.as_ref())
        };
        println!("  {:<44} {}{price}{note}", m.id, m.name);
    }
    println!("choose one with `ways settings set gate.profiles.<profile>.model <id>`");
    Ok(ExitCode::SUCCESS)
}

fn cost_note(m: &net::ModelInfo, base: Option<&net::ModelInfo>) -> String {
    let ratio = match (m.input_per_mtok, base.and_then(|b| b.input_per_mtok)) {
        (Some(p), Some(b)) if b > 0.0 => p / b,
        _ => return "  (untuned)".to_string(),
    };
    if ratio > 1.05 {
        format!("  ({ratio:.1}× Haiku's price, untuned)")
    } else {
        "  (untuned)".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `s` with each `^` as the ESC byte, so no raw escape literal sits in
    /// the source (the agent-theme SGR lint).
    fn esc(s: &str) -> Vec<u8> {
        s.bytes().map(|b| if b == b'^' { 0x1b } else { b }).collect()
    }

    fn feed(e: &mut KeyEditor, bytes: &[u8]) -> Vec<Typed> {
        bytes.iter().map(|&b| e.feed(b)).collect()
    }

    /// Each character typed draws one dot, a multi-byte one included;
    /// Backspace erases a whole character, Ctrl-U the line, Enter ends it.
    #[test]
    fn the_key_prompt_draws_a_dot_per_character() {
        let mut e = KeyEditor::new();
        assert_eq!(feed(&mut e, "sk-é".as_bytes()), [Typed::Dot, Typed::Dot, Typed::Dot, Typed::Dot, Typed::Continue]);
        assert_eq!(e.feed(0x7f), Typed::Erase(1));
        assert_eq!(e.key, b"sk-");
        assert_eq!(e.feed(0x15), Typed::Erase(3));
        assert!(e.key.is_empty());
        assert_eq!(e.feed(0x7f), Typed::Erase(0), "nothing left to erase");
        assert_eq!(e.feed(b'k'), Typed::Dot);
        assert_eq!(e.feed(b'\r'), Typed::Done);
        assert_eq!(e.key, b"k");
    }

    /// An arrow key and the markers around a bracketed paste add nothing to
    /// the key and draw no dot.
    #[test]
    fn escape_sequences_stay_out_of_the_key() {
        let mut e = KeyEditor::new();
        feed(&mut e, &esc("ab^[Dc^OD"));
        assert_eq!(e.key, b"abc", "arrow keys, normal and application mode");
        let mut p = KeyEditor::new();
        let typed = feed(&mut p, &esc("^[200~sk-abc^[201~"));
        assert_eq!(p.key, b"sk-abc");
        assert_eq!(typed.iter().filter(|t| **t == Typed::Dot).count(), 6, "a dot per key character only");
    }

    /// An erased character is zeroed in the buffer, not just dropped.
    #[test]
    fn erased_key_bytes_are_zeroed() {
        let mut e = KeyEditor::new();
        feed(&mut e, b"secret");
        e.feed(0x15);
        // SAFETY: test-only; these six bytes were written, then zeroed before
        // the truncate left them in the spare capacity.
        let left: Vec<u8> = e.key.spare_capacity_mut()[..6].iter().map(|b| unsafe { b.assume_init_read() }).collect();
        assert_eq!(left, [0u8; 6]);
    }

    /// A control byte after a lone ESC, or inside a sequence, is not eaten:
    /// Enter still ends the line and Backspace still erases.
    #[test]
    fn a_control_byte_ends_an_escape_and_acts() {
        let mut e = KeyEditor::new();
        assert_eq!(feed(&mut e, &esc("^\r")), [Typed::Continue, Typed::Done]);
        let mut e = KeyEditor::new();
        assert_eq!(feed(&mut e, &esc("^[\r")), [Typed::Continue, Typed::Continue, Typed::Done]);
        let mut e = KeyEditor::new();
        feed(&mut e, b"ab");
        assert_eq!(feed(&mut e, &esc("^\x7f")), [Typed::Continue, Typed::Erase(1)]);
        assert_eq!(e.key, b"a");
        assert_eq!(feed(&mut e, &esc("^x")), [Typed::Continue, Typed::Continue], "Alt-x is dropped");
        assert_eq!(e.key, b"a");
    }

    #[test]
    fn cost_note_marks_pricier_models() {
        let haiku = net::ModelInfo { id: "h".into(), name: "h".into(), input_per_mtok: Some(1.0), output_per_mtok: Some(5.0) };
        let opus = net::ModelInfo { input_per_mtok: Some(5.0), ..haiku.clone() };
        assert!(cost_note(&opus, Some(&haiku)).contains("5.0×"));
        assert_eq!(cost_note(&haiku, Some(&haiku)), "  (untuned)");
        assert_eq!(cost_note(&opus, None), "  (untuned)");
    }

    #[test]
    fn may_store_never_lets_an_unconfirmed_key_displace_a_stored_one() {
        use net::Check::*;
        let invalid = Invalid("bad".into());
        let unreachable = Unreachable("down".into());
        // A rejected key is never stored, forced or not.
        assert!(may_store(Some(&invalid), false, true).is_err());
        // Replacing needs an authenticated key, or --force.
        for ok in [Valid, NoCredit, ModelUnavailable("m".into())] {
            assert!(may_store(Some(&ok), true, false).is_ok());
        }
        for unconfirmed in [unreachable.clone(), RateLimited, Failed(500, "x".into())] {
            assert!(may_store(Some(&unconfirmed), true, false).is_err());
            assert!(may_store(Some(&unconfirmed), true, true).is_ok());
            assert!(may_store(Some(&unconfirmed), false, false).is_ok());
        }
        assert!(may_store(None, true, false).is_err());
        assert!(may_store(None, false, false).is_ok());
    }

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
