//! The judge engine's one writer (ADR-512 item 4). `gate.engine` lives in the
//! agent file and the ways-judge mod's wanted state, `mods.ways-judge.install`,
//! in the user's ways config, so the two are written in an order that never
//! leaves `claude-code` chosen without the mod wanted: the mod's setting
//! first when switching to `claude-code`, the engine first when leaving it.
//! `ways judge-setup`, the settings gate screen, `ways settings set` and
//! `unset` of `gate.engine`, and `ways settings apply` all write through
//! [`write_engine`], and a direct write of the mod's setting in the user
//! config is refused ([`refuse`]).

use super::*;
use std::time::Duration;

/// The mod whose wanted state follows the engine.
const JUDGE: &str = "ways-judge";

/// The key that state is.
pub const JUDGE_INSTALL: &str = "mods.ways-judge.install";

/// Whether the profile `engine` names runs on the keyless `claude-code`
/// provider, the one the ways-judge mod hosts (ADR-708 §1).
pub fn is_keyless(engine: &str) -> bool {
    use ways_agent_core::profile::{profiles, user_layer_path, UserLayer};
    let user = UserLayer::load(&user_layer_path()).unwrap_or_default();
    profiles(&user).get(engine).is_some_and(|p| p.provider.keyless())
}

/// Whether `gate.engine` names a keyless profile now.
pub fn claude_code_chosen() -> bool {
    use ways_agent_core::profile::{user_layer_path, UserLayer};
    UserLayer::load(&user_layer_path()).ok().and_then(|u| u.engine).is_some_and(|e| is_keyless(&e))
}

/// Write `mods.<name>.install` in one ways config file (the user's or a
/// target's) through the one writer, or remove it with `None`. Returns
/// whether the file changed; a missing file that has nothing to remove is
/// not created.
pub fn write_mod_install(path: &Path, name: &str, value: Option<bool>) -> Result<bool, String> {
    write_mod_install_within(path, name, value, None)
}

/// [`write_mod_install`], waiting at most `wait` for the file's lock.
fn write_mod_install_within(path: &Path, name: &str, value: Option<bool>, wait: Option<Duration>) -> Result<bool, String> {
    let key: Vec<String> = vec!["mods".into(), name.into(), "install".into()];
    match value {
        Some(v) => write_file_checked(path, &[(key, Value::Bool(v))], wait, |_| Ok(())).map_err(|f| f.message)?,
        None if !path.is_file() => Ok(false),
        None => agent_settings::writer::edit_file_within(path, header_for(path), wait, |d| d.unset(&key))
            .map(|(_, changed)| changed)
            .map_err(|e| write_failed(e).message),
    }
}

/// Write the judge engine: `Some(profile)` sets `gate.engine`, `None` clears
/// it. For a keyless profile `mods.ways-judge.install: true` goes to the user
/// config first; for any other engine, or none, the engine goes first and
/// then a `true` in the user config becomes `false`. Returns each file
/// written with its layer, in order. Run the judge mod's repair after it.
pub fn write_engine(engine: Option<&str>) -> Result<Vec<(LayerScope, PathBuf)>, Failure> {
    write_engine_within(engine, None)
}

/// [`write_engine`], waiting at most `wait` for each lock it takes: the
/// settings screens write on their own thread, which must not hang behind
/// another writer. A lock not taken in time is a write failure that names
/// it, with nothing after it written.
pub fn write_engine_within(engine: Option<&str>, wait: Option<Duration>) -> Result<Vec<(LayerScope, PathBuf)>, Failure> {
    let reg = registry();
    let b = lookup(&reg, "gate.engine")?;
    let agent = ways_agent_core::profile::user_layer_path();
    let user = ways_core::paths::user_config();
    let install = |v: bool| write_mod_install_within(&user, JUDGE, Some(v), wait).map_err(|m| fail(exit::WRITE_FAILED, m));
    // The engine is checked before anything is written.
    let value = engine
        .map(|e| b.spec.parse_cli(e, &live_layers(None), &b.bound))
        .transpose()
        .map_err(|m| fail(exit::REJECTED, format!("gate.engine: {m}; `ways settings help gate.engine`")))?;
    let mut written = Vec::new();
    let keyless = engine.is_some_and(is_keyless);
    // Already chosen: the same keyless engine with the mod already wanted
    // is no new choice, so a removal by hand that no repair has seen yet
    // stays respected.
    let chosen = claude_code_chosen() && ways_core::settings::mod_install_in_user(JUDGE) == Some(true);
    if keyless {
        install(true)?;
        written.push((LayerScope::User, user.clone()));
        // Choosing the engine is an explicit want: a removal by hand that no
        // repair has seen yet is not written back as `install: false`.
        if !chosen {
            for t in crate::config::Config::load(&crate::util::project_dir()).targets().into_iter().filter(|t| t.enabled) {
                crate::cmd::attend_mod::forget_within(&t.dir(), &crate::cmd::attend_mod::JUDGE, wait)
                    .map_err(|e| fail(exit::WRITE_FAILED, format!("{e}; gate.engine is unchanged")))?;
            }
        }
    }
    match value {
        Some(v) => {
            write_file_checked(&agent, &[(b.path(), v)], wait, |_| Ok(()))?.map_err(|e| fail(exit::WRITE_FAILED, e))?;
        }
        None => {
            let kp = b.path();
            agent_settings::writer::edit_file_within(&agent, None, wait, |d| d.unset(&kp)).map_err(write_failed)?;
        }
    }
    written.push((LayerScope::User, agent));
    if !keyless && ways_core::settings::mod_install_in_user(JUDGE) == Some(true) {
        install(false)?;
        written.push((LayerScope::User, user));
    }
    Ok(written)
}

/// What `set` and `unset` print after an engine write: each file written,
/// and where the mod's wanted state changed, the repair that converges it.
/// `set` calls no external CLI (ADR-512, alternatives).
pub fn report(written: &[(LayerScope, PathBuf)]) {
    for (scope, path) in written {
        touched(*scope, path, false);
    }
    if written.iter().any(|(_, p)| *p == ways_core::paths::user_config()) {
        eprintln!("{REPAIR_HINT}");
    }
}

/// The line a write of the judge mod's wanted state ends with.
pub const REPAIR_HINT: &str =
    "the ways-judge mod follows gate.engine at the next `ways manage mod repair ways-judge --commit` or `ways manage update --commit`";

/// A write of `key` that only the engine choice may make: the user config's
/// `mods.ways-judge.install`. A project write never reaches the user file,
/// and a target's config takes `false` through `ways manage mod uninstall
/// ways-judge --target <dir>`.
pub fn refuse(key: &str) -> Out {
    if key != JUDGE_INSTALL {
        return Ok(());
    }
    Err(fail(
        exit::REJECTED,
        format!(
            "{JUDGE_INSTALL} follows gate.engine in the user config; `ways settings set gate.engine claude-code` (or `ways manage mod install ways-judge`) wants the mod, and `ways judge-setup` picks another engine"
        ),
    ))
}
