//! The relevance judge's setup (ADR-196, ADR-502): the warning the top-level
//! help (bare `ways`, `--help`, `help`) ends with when the judge cannot gate,
//! and the check and offer that `ways update` and the installer run. Both read
//! `keys::judge_ready`, so they never disagree.

use std::io::{BufRead, IsTerminal, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use ways_agent_core::keys::{self, Readiness};
use ways_agent_core::profile::{Mode, Provider};

/// Why the judge cannot gate, and the fix. None when it gates or the operator
/// turned it off.
fn reason(state: &Readiness) -> Option<(String, String)> {
    Some(match state {
        Readiness::Ready(..) | Readiness::Off => return None,
        Readiness::NoKey(None) => {
            ("No provider key".to_string(), "ways agent key add --provider anthropic (or openrouter)".to_string())
        }
        Readiness::NoKey(Some(p)) => (format!("No {} key", name(*p)), format!("ways agent key add --provider {p}")),
        Readiness::Unverified(p, None) => (format!("{} key: unchecked", name(*p)), "ways agent key check".to_string()),
        Readiness::Unverified(p, Some(result)) => {
            let fix = match result.as_str() {
                "invalid" | "no_credit" => format!("ways agent key rotate --provider {p}"),
                "model_unavailable" => "ways settings gate".to_string(),
                _ => "ways agent key check".to_string(),
            };
            (format!("{} key: {result}", name(*p)), fix)
        }
        Readiness::Config(_) => ("agent.yaml does not load".to_string(), "ways settings gate".to_string()),
    })
}

/// Two lines, each within 80 columns, when the judge cannot gate: what that
/// costs, then why and the fix.
pub fn warning(state: &Readiness) -> Option<String> {
    let (why, fix) = reason(state)?;
    Some(format!("Ways is degraded: the relevance judge is off, so every matched way is injected.\n{why}. Fix: {fix}"))
}

/// One line on the judge's state after setup. None when the operator turned it off.
fn outcome(state: &Readiness) -> Option<String> {
    match state {
        Readiness::Ready(p, Mode::Shadow) => {
            Some(format!("Relevance judge on ({}, shadow: logs verdicts, blocks nothing).", name(*p)))
        }
        Readiness::Ready(p, _) => Some(format!("Relevance judge on ({}).", name(*p))),
        other => reason(other).map(|(why, _)| format!("Relevance judge still off: {why}.")),
    }
}

/// The provider's name as the operator knows it.
fn name(p: Provider) -> &'static str {
    match p {
        Provider::Anthropic => "Anthropic",
        Provider::Openrouter => "OpenRouter",
    }
}

/// The help footer: [`warning`] for the judge's state now. Reads files only.
pub fn help_footer() -> Option<String> {
    warning(&keys::judge_ready())
}

/// Checks the engine's key file at no cost, then says whether the judge gates.
/// When it does not, and stdin and stdout are terminals, offers to add a key
/// with `ways-agent key add`, which prompts for it hidden, checks and stores it.
pub fn setup() -> Result<()> {
    let agent = super::agent::resolve();
    let mut state = keys::judge_ready();
    if let (Readiness::Unverified(p, _), Some(bin)) = (&state, &agent) {
        println!("Checking the relevance judge's key…");
        // Without the variable the check reads the key file, the one the agent
        // a hook starts reads, and records its result for judge_ready.
        let _ = Command::new(bin)
            .args(["key", "check", "--provider", p.as_str()])
            .env_remove(p.key_env())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        state = keys::judge_ready();
    }
    let Some(warning) = warning(&state) else {
        if let Some(line) = outcome(&state) {
            println!("{line}");
        }
        return Ok(());
    };
    println!("{warning}");
    if let Readiness::NoKey(_) = state {
        for p in Provider::ALL.into_iter().filter(|p| std::env::var(p.key_env()).is_ok_and(|v| !v.trim().is_empty())) {
            println!("${} is set, but hooks read only the key file: add it there.", p.key_env());
        }
    }
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let (Some(bin), true, false) = (agent, interactive, matches!(state, Readiness::Config(_))) else {
        return Ok(());
    };
    let Some(provider) = pick()? else {
        println!("Skipped.");
        return Ok(());
    };
    Command::new(&bin)
        .args(["key", "add", "--provider", provider.as_str()])
        .status()
        .with_context(|| format!("running {}", bin.display()))?;
    if let Some(line) = outcome(&keys::judge_ready()) {
        println!("{line}");
    }
    Ok(())
}

/// The provider the operator picks; None on skip, end of input, or a second
/// answer that is not a choice.
fn pick() -> Result<Option<Provider>> {
    for _ in 0..2 {
        print!("Add a key now? [1] Anthropic  [2] OpenRouter  [s] skip: ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        if std::io::stdin().lock().read_line(&mut answer)? == 0 {
            println!();
            return Ok(None);
        }
        match answer.trim() {
            "1" => return Ok(Some(Provider::Anthropic)),
            "2" => return Ok(Some(Provider::Openrouter)),
            "s" | "S" => return Ok(None),
            _ => {}
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_warning_fits_80_columns_and_settled_states_say_nothing() {
        let results = ["invalid", "no_credit", "rate_limited", "model_unavailable", "unreachable", "failed"];
        let mut states = vec![Readiness::NoKey(None), Readiness::Config("x".into())];
        for p in Provider::ALL {
            states.push(Readiness::NoKey(Some(p)));
            states.push(Readiness::Unverified(p, None));
            states.extend(results.map(|r| Readiness::Unverified(p, Some(r.into()))));
        }
        for state in states {
            let text = warning(&state).unwrap();
            for line in text.lines() {
                assert!(line.chars().count() <= 80, "{state:?}: {line:?}");
            }
        }
        assert_eq!(warning(&Readiness::Ready(Provider::Anthropic, Mode::Enforce)), None);
        assert_eq!(warning(&Readiness::Off), None);
        assert_eq!(outcome(&Readiness::Off), None);
        assert_eq!(outcome(&Readiness::Ready(Provider::Anthropic, Mode::Enforce)).unwrap(), "Relevance judge on (Anthropic).");
        assert!(outcome(&Readiness::Ready(Provider::Openrouter, Mode::Shadow)).unwrap().contains("shadow"));
    }
}
