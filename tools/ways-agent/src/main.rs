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

#[derive(Parser)]
#[command(name = "ways-agent", version, about = "The ways agent: relevance judging and key custody for agent-ways")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add, check, rotate or remove a provider API key.
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
}

#[derive(Subcommand)]
enum KeyAction {
    /// Store a key. Reads --from-file, else stdin when piped, else a hidden prompt.
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
        (Some(e), Some(m), Some(mode)) => println!("engine {e} ({m}), mode {}", mode.as_str()),
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
    if std::env::var(provider.key_env()).is_ok_and(|v| !v.trim().is_empty()) {
        println!("note: ${} is set and overrides this file", provider.key_env());
    }
    let Some(result) = result else {
        println!("not checked: the agent checks it on first use, and gates only once it passes");
        return Ok(ExitCode::SUCCESS);
    };
    println!("check: {result}");
    Ok(if result.is_valid() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
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
    if std::env::var(provider.key_env()).is_ok_and(|v| !v.trim().is_empty()) {
        println!("note: ${} is still set and still supplies a key", provider.key_env());
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

/// The key, from a file, from piped stdin, or from a prompt that does not echo.
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
    eprint!("Paste your {provider} API key (input hidden): ");
    read_hidden_line()
}

/// The terminal settings to put back if Ctrl-C arrives mid-prompt.
#[cfg(unix)]
static mut SAVED_TERMIOS: Option<libc::termios> = None;

#[cfg(unix)]
extern "C" fn restore_and_exit(_signal: libc::c_int) {
    // SAFETY: tcsetattr and _exit are async-signal-safe; SAVED_TERMIOS is
    // written once before this handler is installed and only read here.
    unsafe {
        if let Some(t) = *std::ptr::addr_of!(SAVED_TERMIOS) {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t);
        }
        libc::_exit(130);
    }
}

#[cfg(unix)]
fn read_hidden_line() -> Result<String> {
    // SAFETY: termios calls on stdin with a struct tcgetattr fills; the
    // original settings are restored on every path, including Ctrl-C through
    // the handler above. Echo is never left on: a failed switch reads nothing.
    unsafe {
        let mut original: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut original) != 0 {
            bail!("cannot read terminal settings; pipe the key on stdin or use --from-file");
        }
        *std::ptr::addr_of_mut!(SAVED_TERMIOS) = Some(original);
        let previous = libc::signal(libc::SIGINT, restore_and_exit as *const () as libc::sighandler_t);
        let mut hidden = original;
        hidden.c_lflag &= !libc::ECHO;
        hidden.c_lflag |= libc::ECHONL;
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &hidden) != 0 {
            libc::signal(libc::SIGINT, previous);
            bail!("cannot turn off terminal echo; pipe the key on stdin or use --from-file");
        }
        let mut line = String::new();
        let read = std::io::stdin().read_line(&mut line);
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
        libc::signal(libc::SIGINT, previous);
        read.context("reading the key")?;
        Ok(line)
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
