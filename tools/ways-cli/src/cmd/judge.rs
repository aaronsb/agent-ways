//! The relevance judge's setup (ADR-196, ADR-502): the warning `ways` prints
//! with its commands when the judge cannot gate, and the check and offer that
//! `ways update` and the installer run. Both read `keys::judge_ready`, so they
//! never disagree.

use std::io::{BufRead, IsTerminal, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use ways_agent_core::keys::{self, Readiness};
use ways_agent_core::profile::Provider;

/// Two lines, each within 80 columns, when the judge cannot gate: what that
/// costs, then why and the fix. None when it gates or the operator turned it off.
pub fn warning(state: &Readiness) -> Option<String> {
    let (why, fix) = match state {
        Readiness::Ready(_) | Readiness::Off => return None,
        Readiness::NoKey(None) => {
            ("No provider key".to_string(), "ways agent key add --provider anthropic|openrouter".to_string())
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
    };
    Some(format!("Ways is degraded: the relevance judge is off, so every matched way is injected.\n{why}. Fix: {fix}"))
}

/// The provider's name as the operator knows it.
fn name(p: Provider) -> &'static str {
    match p {
        Provider::Anthropic => "Anthropic",
        Provider::Openrouter => "OpenRouter",
    }
}

/// The help footer:[`warning`] for the judge's state now. Reads files only.
pub fn help_footer() -> Option<String> {
    warning(&keys::judge_ready())
}

/// Checks each stored key at no cost, then says whether the judge gates. When
/// it does not, and stdin and stdout are terminals, offers to add a key with
/// `ways-agent key add`, which prompts for it hidden, checks it and stores it.
pub fn setup() -> Result<()> {
    let agent = super::agent::resolve();
    if let Some(bin) = &agent {
        for p in Provider::ALL.into_iter().filter(|p| keys::locate(*p).is_some()) {
            // The check records its result; judge_ready reads the record.
            let _ = Command::new(bin)
                .args(["key", "check", "--provider", p.as_str()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    let state = keys::judge_ready();
    if let Readiness::Ready(p) = state {
        println!("Relevance judge on ({p}).");
        return Ok(());
    }
    let Some(warning) = warning(&state) else { return Ok(()) };
    println!("{warning}");
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let (Some(bin), true, false) = (agent, interactive, matches!(state, Readiness::Config(_))) else {
        return Ok(());
    };
    print!("Add a key now? [1] Anthropic  [2] OpenRouter  [s] skip: ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    let provider = match answer.trim() {
        "1" => Provider::Anthropic,
        "2" => Provider::Openrouter,
        _ => return Ok(()),
    };
    Command::new(&bin)
        .args(["key", "add", "--provider", provider.as_str()])
        .status()
        .with_context(|| format!("running {}", bin.display()))?;
    Ok(())
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
        assert_eq!(warning(&Readiness::Ready(Provider::Anthropic)), None);
        assert_eq!(warning(&Readiness::Off), None);
    }
}
