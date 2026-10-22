//! The relevance judge's setup (ADR-196, ADR-502, ADR-708 §9): the warning
//! the top-level help (bare `ways`, `--help`, `help`) ends with when the judge
//! cannot gate, and the check and offer that `ways manage update` and the
//! installer run. Both read [`readiness`], which is `keys::judge_ready` with
//! the ways-judge mod's install state, so they never disagree.

use std::io::{BufRead, IsTerminal, Write};
use std::process::{Command, Stdio};

use anyhow::{anyhow, Context, Result};
use ways_agent_core::keys::{self, JudgeMod, Readiness};
use ways_agent_core::profile::{Mode, Provider};

use super::attend_mod::{self, Want, JUDGE};

/// The judge's readiness now: `keys::judge_ready`, told where the ways-judge
/// mod stands in the enabled targets: enabled in one, else installed and
/// disabled in one, else absent. Reads files only; Claude Code is not called.
pub fn readiness() -> Readiness {
    keys::judge_ready(|| {
        let targets = enabled_targets();
        if targets.iter().any(|dir| attend_mod::enabled_in(&JUDGE, dir)) {
            JudgeMod::Enabled
        } else if targets.iter().any(|dir| attend_mod::listed_in(&JUDGE, dir)) {
            JudgeMod::Disabled
        } else {
            JudgeMod::Absent
        }
    })
}

/// The directories of the enabled targets.
fn enabled_targets() -> Vec<std::path::PathBuf> {
    crate::config::global().targets().into_iter().filter(|t| t.enabled).map(|t| t.dir()).collect()
}

/// Why the judge cannot gate, and the fix. None when it gates or the operator
/// turned it off.
pub(crate) fn reason(state: &Readiness) -> Option<(String, String)> {
    Some(match state {
        Readiness::Ready(..) | Readiness::Fallback(..) | Readiness::Off => return None,
        // A keyless engine takes no key: its remedies are the mod or a key
        // engine to stand in (ADR-708 §7). `judge_ready` says NoMod for it;
        // the key states are matched too, so no line ever offers a keyless
        // provider a key command.
        Readiness::NoMod => no_mod(),
        Readiness::NoKey(Some(p)) | Readiness::Unverified(p, _) if p.keyless() => no_mod(),
        Readiness::ModDisabled => (
            "Claude Code internal's ways-judge mod is disabled, and no key stands in".to_string(),
            format!("claude plugin enable {}, or ways judge-setup for a key", JUDGE.id),
        ),
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

/// Why `claude-code` cannot gate with neither the mod nor a key, and both
/// remedies: setup installs the mod or adds a key.
fn no_mod() -> (String, String) {
    (
        "Claude Code internal has no ways-judge mod and no key to stand in".to_string(),
        "ways judge-setup (installs the mod, or adds an Anthropic or OpenRouter key)".to_string(),
    )
}

/// The width every line of the warning keeps within.
const COLUMNS: usize = 80;

/// Lines within 80 columns when the judge cannot gate: what that costs, then
/// why and the fix, on one line where they fit and on two where they do not.
pub fn warning(state: &Readiness) -> Option<String> {
    let (why, fix) = reason(state)?;
    let joined = format!("{why}. Fix: {fix}");
    let detail = if joined.chars().count() <= COLUMNS { joined } else { format!("{why}.\nFix: {fix}") };
    Some(format!("Ways is degraded: the relevance judge is off, so every matched way is injected.\n{detail}"))
}

/// The judge's state after setup, wrapped within 80 columns.
fn outcome(state: &Readiness) -> Option<String> {
    outcome_text(state).map(|text| wrap(&text, COLUMNS))
}

/// The judge's state in one unwrapped line, as [`outcome`] gives it and the
/// settings gate screen shows it, wrapped to its pane.
pub(crate) fn outcome_text(state: &Readiness) -> Option<String> {
    let shadow = |m: &Mode| if *m == Mode::Shadow { ", shadow: logs verdicts, blocks nothing" } else { "" };
    let text = match state {
        Readiness::Ready(p, m) => format!("Relevance judge on ({}{}).", name(*p), shadow(m)),
        Readiness::Fallback(p, m) => format!(
            "Relevance judge on ({} key{}): Claude Code internal is chosen, but the ways-judge mod is not enabled, so the {} key engine judges instead.",
            name(*p),
            shadow(m),
            name(*p)
        ),
        Readiness::Off => {
            "Relevance judge off by choice (gate.mode: off): every matched way is injected. `ways settings set gate.mode enforce` turns it on."
                .to_string()
        }
        other => format!("Relevance judge still off: {}.", reason(other)?.0),
    };
    Some(text)
}

/// `text` broken at spaces into lines of at most `width` characters; a word
/// longer than `width` keeps a line of its own.
fn wrap(text: &str, width: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines.join("\n")
}

/// The judge's state in a word, for the settings gate screen's value column.
pub(crate) fn state_word(state: &Readiness) -> &'static str {
    match state {
        Readiness::Ready(..) => "ready",
        Readiness::Fallback(..) => "key stands in",
        Readiness::Off => "off by choice",
        Readiness::Config(_) => "config error",
        Readiness::NoKey(_) | Readiness::NoMod | Readiness::ModDisabled | Readiness::Unverified(..) => "degraded",
    }
}

/// The provider's name as the operator knows it.
fn name(p: Provider) -> &'static str {
    match p {
        Provider::Anthropic => "Anthropic",
        Provider::Openrouter => "OpenRouter",
        Provider::ClaudeCode => "Claude Code internal",
    }
}

/// The help footer: [`warning`] for the judge's state now. Reads files only.
pub fn help_footer() -> Option<String> {
    warning(&readiness())
}

/// One engine setup offers (ADR-708 §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    /// Install the ways-judge mod and set `gate.engine: claude-code`.
    ClaudeCode,
    /// Add this provider's key with `ways-agent key add`.
    Key(Provider),
    /// Record `gate.mode: off`.
    Off,
}

impl Choice {
    fn label(self) -> &'static str {
        match self {
            Choice::ClaudeCode => "Claude Code internal: the ways-judge mod, on this account; no key",
            Choice::Key(Provider::Anthropic) => "An Anthropic API key",
            Choice::Key(_) => "An OpenRouter API key",
            Choice::Off => "No judge: gate off; every matched way is injected, quality drops",
        }
    }
}

/// The choices in the order ADR-708 §9 sets: Claude Code internal where the
/// host can load mods, the key engines where `ways-agent` is installed to add
/// a key, and no judge.
fn choices(mods: bool, keys: bool) -> Vec<Choice> {
    let mut out = Vec::new();
    if mods {
        out.push(Choice::ClaudeCode);
    }
    if keys {
        out.extend([Choice::Key(Provider::Anthropic), Choice::Key(Provider::Openrouter)]);
    }
    out.push(Choice::Off);
    out
}

/// The menu, one numbered line per choice, and the prompt.
fn menu(choices: &[Choice]) -> String {
    let mut text = String::from("Choose the relevance judge's engine:\n");
    for (i, c) in choices.iter().enumerate() {
        text.push_str(&format!("  [{}] {}\n", i + 1, c.label()));
    }
    text.push_str(&format!("Choice [1-{}, s to skip]: ", choices.len()));
    text
}

/// What one answer to the menu means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Pick(Choice),
    Skip,
    Again,
}

fn answer(choices: &[Choice], text: &str) -> Answer {
    match text.trim() {
        "s" | "S" => Answer::Skip,
        n => match n.parse::<usize>() {
            Ok(i) if (1..=choices.len()).contains(&i) => Answer::Pick(choices[i - 1]),
            _ => Answer::Again,
        },
    }
}

/// Checks the engine's key file at no cost, then says whether the judge gates.
/// When it does not, and stdin and stdout are terminals, offers the engines:
/// Claude Code internal where the host can load mods, an Anthropic or
/// OpenRouter key through `ways-agent key add` (which prompts with a dot per
/// character, checks and stores it), or no judge.
pub fn setup() -> Result<()> {
    let agent = super::agent::resolve();
    let mut state = readiness();
    if let (Readiness::Unverified(p, _), Some(bin)) = (&state, &agent) {
        println!("Checking the relevance judge's key…");
        let mut check = Command::new(bin);
        check.args(["key", "check", "--provider", p.as_str()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // Without the variable the check reads the key file, the one the agent
        // a hook starts reads, and records its result for judge_ready.
        if let Some(var) = p.key_env() {
            check.env_remove(var);
        }
        let _ = check.status();
        state = readiness();
    }
    let Some(warning) = warning(&state) else {
        if let Some(line) = outcome(&state) {
            println!("{line}");
        }
        return Ok(());
    };
    println!("{warning}");
    if let Readiness::NoKey(_) | Readiness::NoMod | Readiness::ModDisabled = state {
        for var in Provider::ALL.into_iter().filter(|p| keys::env_set(*p)).filter_map(Provider::key_env) {
            println!("${var} is set, but hooks read only the key file: add it there.");
        }
    }
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !interactive || matches!(state, Readiness::Config(_)) {
        return Ok(());
    }
    // An app that ships no ways-judge mod has nothing to offer; that is not
    // the host's limit, so it goes unsaid.
    let shipped = attend_mod::shipped_version(&JUDGE, &crate::paths::data_root()).is_some();
    let host = if shipped { mod_host() } else { Err(String::new()) };
    if let (true, Err(why)) = (shipped, &host) {
        println!("Claude Code internal is not offered: {why}.");
    }
    let offered = choices(host.is_ok(), agent.is_some());
    // "No judge" alone is no choice to put to the operator.
    if offered == [Choice::Off] {
        return Ok(());
    }
    let Some(choice) = pick(&offered)? else {
        println!("Skipped.");
        return Ok(());
    };
    match (choice, &agent) {
        (Choice::ClaudeCode, _) => install_mod()?,
        (Choice::Key(provider), Some(bin)) => add_key(bin, provider)?,
        (Choice::Key(_), None) => unreachable!("a key is offered only with ways-agent installed"),
        (Choice::Off, _) => set("gate.mode", "off")?,
    }
    if let Some(text) = outcome(&readiness()) {
        println!("{text}");
    }
    Ok(())
}

/// Adds `provider`'s key with `ways-agent key add`, which checks it before it
/// stores it, then names the provider's engine in `gate.engine` once the key
/// file passed its check: the operator chose that engine.
fn add_key(bin: &std::path::Path, provider: Provider) -> Result<()> {
    Command::new(bin)
        .args(["key", "add", "--provider", provider.as_str()])
        .status()
        .with_context(|| format!("running {}", bin.display()))?;
    if keys::key_engine_ready(provider.as_str()) {
        set("gate.engine", provider.as_str())?;
    }
    Ok(())
}

/// Whether this host can run the ways-judge mod, judged against the first
/// enabled target.
fn mod_host() -> Result<(), String> {
    let projection = crate::paths::projection_root();
    let dest = enabled_targets().into_iter().next().unwrap_or_else(|| projection.clone());
    attend_mod::host_can_load(&JUDGE, &dest, &projection)
}

/// Installs the ways-judge mod in every enabled target and records the
/// request in each target it is enabled in, so repair keeps it installed;
/// then names `claude-code` as the engine. A failed install leaves
/// `gate.engine` as it was.
fn install_mod() -> Result<()> {
    let projection = crate::paths::projection_root();
    let mut enabled = false;
    for dir in enabled_targets() {
        for line in attend_mod::converge(&JUDGE, &crate::paths::data_root(), &dir, &projection, Want::Install, false) {
            println!("{line}");
        }
        if attend_mod::enabled_in(&JUDGE, &dir) {
            enabled = true;
            attend_mod::record_request(&JUDGE, &dir).with_context(|| format!("recording the ways-judge mod's install for {}", dir.display()))?;
        }
    }
    if !enabled {
        println!("The ways-judge mod is not enabled in any target, so gate.engine is unchanged.");
        return Ok(());
    }
    set("gate.engine", "claude-code")
}

/// Writes one gate setting in the user layer, as `ways settings set` does.
fn set(key: &str, value: &str) -> Result<()> {
    super::settings::set(key, value, None).map_err(|f| anyhow!("{}", f.message))
}

/// The choice the operator picks; None on skip, end of input, or a second
/// answer that is not a choice.
fn pick(choices: &[Choice]) -> Result<Option<Choice>> {
    for _ in 0..2 {
        print!("{}", menu(choices));
        std::io::stdout().flush()?;
        let mut text = String::new();
        if std::io::stdin().lock().read_line(&mut text)? == 0 {
            println!();
            return Ok(None);
        }
        match answer(choices, &text) {
            Answer::Pick(c) => return Ok(Some(c)),
            Answer::Skip => return Ok(None),
            Answer::Again => {}
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
        let mut states = vec![Readiness::NoKey(None), Readiness::NoMod, Readiness::ModDisabled, Readiness::Config("x".into())];
        for p in Provider::ALL.into_iter().chain([Provider::ClaudeCode]) {
            states.push(Readiness::NoKey(Some(p)));
            states.push(Readiness::Unverified(p, None));
            states.extend(results.map(|r| Readiness::Unverified(p, Some(r.into()))));
        }
        for state in &states {
            let text = warning(state).unwrap();
            for line in text.lines() {
                assert!(line.chars().count() <= 80, "{state:?}: {line:?}");
            }
            assert!(!text.contains("--provider claude-code"), "{state:?}: a keyless engine takes no key: {text}");
        }
        // Every outcome line fits too, the settled states' included.
        let mut all = states.clone();
        for p in Provider::ALL.into_iter().chain([Provider::ClaudeCode]) {
            for m in [Mode::Enforce, Mode::Shadow] {
                all.extend([Readiness::Ready(p, m), Readiness::Fallback(p, m)]);
            }
        }
        all.push(Readiness::Off);
        for state in &all {
            let text = outcome(state).unwrap();
            for line in text.lines() {
                assert!(line.chars().count() <= 80, "{state:?}: {line:?}");
            }
        }
        assert_eq!(warning(&Readiness::Ready(Provider::Anthropic, Mode::Enforce)), None);
        assert_eq!(warning(&Readiness::Off), None);
        assert_eq!(outcome(&Readiness::Ready(Provider::Anthropic, Mode::Enforce)).unwrap(), "Relevance judge on (Anthropic).");
        assert!(outcome(&Readiness::Ready(Provider::Openrouter, Mode::Shadow)).unwrap().contains("shadow"));
    }

    /// An outcome as one line, for checking its words across the wrap.
    fn flat(state: &Readiness) -> String {
        outcome(state).unwrap().replace('\n', " ")
    }

    #[test]
    fn wrap_breaks_at_spaces_within_the_width() {
        assert_eq!(wrap("aaa bbb ccc", 7), "aaa bbb\nccc");
        assert_eq!(wrap("short", 80), "short");
        assert_eq!(wrap("a-very-long-word x", 5), "a-very-long-word\nx");
    }

    #[test]
    fn an_installed_but_disabled_mod_reads_as_not_enabled() {
        let text = warning(&Readiness::ModDisabled).unwrap();
        assert!(text.contains("ways-judge mod is disabled"), "{text}");
        assert!(text.contains("claude plugin enable ways-judge@agent-ways"), "{text}");
        assert!(!text.contains("has no ways-judge mod"), "{text}");
    }

    #[test]
    fn claude_code_with_the_mod_gates_and_says_so() {
        let state = Readiness::Ready(Provider::ClaudeCode, Mode::Enforce);
        assert_eq!(warning(&state), None);
        assert_eq!(outcome(&state).unwrap(), "Relevance judge on (Claude Code internal).");
    }

    #[test]
    fn claude_code_with_neither_mod_nor_key_names_both_remedies() {
        for state in [Readiness::NoMod, Readiness::NoKey(Some(Provider::ClaudeCode))] {
            let text = warning(&state).unwrap();
            let fix = text.lines().find(|l| l.contains("Fix:")).unwrap();
            assert!(fix.contains("ways judge-setup") && fix.contains("installs the mod"), "{fix}");
            assert!(fix.contains("Anthropic") && fix.contains("OpenRouter") && fix.contains("key"), "{fix}");
        }
        assert!(outcome(&Readiness::NoMod).unwrap().starts_with("Relevance judge still off"));
    }

    #[test]
    fn claude_code_without_the_mod_says_the_key_engine_runs() {
        let state = Readiness::Fallback(Provider::Anthropic, Mode::Enforce);
        assert_eq!(warning(&state), None, "a key engine stands in: not degraded");
        let line = flat(&state);
        assert!(line.starts_with("Relevance judge on (Anthropic key)"), "{line}");
        assert!(line.contains("ways-judge mod is not enabled") && line.contains("Anthropic key engine judges"), "{line}");
    }

    #[test]
    fn gate_mode_off_reads_as_off_by_choice() {
        assert_eq!(warning(&Readiness::Off), None, "nothing to fix, so the help footer is silent");
        assert!(Readiness::Off.settled());
        let line = flat(&Readiness::Off);
        assert!(line.starts_with("Relevance judge off by choice (gate.mode: off)"), "{line}");
    }

    #[test]
    fn setup_offers_four_choices_in_order_and_three_where_mods_cannot_load() {
        let all = choices(true, true);
        assert_eq!(all, vec![Choice::ClaudeCode, Choice::Key(Provider::Anthropic), Choice::Key(Provider::Openrouter), Choice::Off]);
        let no_mods = choices(false, true);
        assert_eq!(no_mods, vec![Choice::Key(Provider::Anthropic), Choice::Key(Provider::Openrouter), Choice::Off]);
        assert!(menu(&no_mods).contains("[1] An Anthropic API key"));
        assert!(!menu(&no_mods).contains("Claude Code internal"));
        assert!(menu(&no_mods).ends_with("Choice [1-3, s to skip]: "));
        // Without ways-agent no key can be added.
        assert_eq!(choices(true, false), vec![Choice::ClaudeCode, Choice::Off]);
        for line in menu(&all).lines() {
            assert!(line.chars().count() <= 80, "{line:?}");
        }
    }

    #[test]
    fn an_answer_picks_by_number_skips_or_asks_again() {
        let all = choices(true, true);
        assert_eq!(answer(&all, "1\n"), Answer::Pick(Choice::ClaudeCode));
        assert_eq!(answer(&all, " 4 "), Answer::Pick(Choice::Off));
        assert_eq!(answer(&all, "s"), Answer::Skip);
        assert_eq!(answer(&all, "5"), Answer::Again);
        assert_eq!(answer(&all, "0"), Answer::Again);
        assert_eq!(answer(&choices(false, true), "1"), Answer::Pick(Choice::Key(Provider::Anthropic)));
    }
}
