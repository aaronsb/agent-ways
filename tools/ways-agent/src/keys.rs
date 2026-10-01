//! Provider API keys: where they live, how they are written, and what may be
//! shown of them (ADR-502 §6).
//!
//! A key is a file `$XDG_CONFIG_HOME/agent-ways/keys/<provider>`, mode 0600,
//! inside a directory of mode 0700, written atomically. The provider's
//! environment variable overrides the file. Nothing here prints a key: callers
//! show only [`tail`], at most the last four characters.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::profile::Provider;

/// Where a key was found.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Env(&'static str),
    File(PathBuf),
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Env(var) => write!(f, "${var}"),
            Source::File(p) => write!(f, "{}", p.display()),
        }
    }
}

pub fn keys_dir() -> PathBuf {
    ways_core::paths::config_root().join("keys")
}

pub fn key_path(provider: Provider) -> PathBuf {
    keys_dir().join(provider.as_str())
}

/// Where the provider's key comes from, without reading the file.
pub fn locate(provider: Provider) -> Option<Source> {
    locate_in(provider, &keys_dir())
}

fn locate_in(provider: Provider, dir: &Path) -> Option<Source> {
    if std::env::var(provider.key_env()).is_ok_and(|v| !v.trim().is_empty()) {
        return Some(Source::Env(provider.key_env()));
    }
    let path = dir.join(provider.as_str());
    path.is_file().then_some(Source::File(path))
}

/// Reads the key. Only the agent and `key check` call this.
pub fn read(provider: Provider) -> Result<Option<(String, Source)>> {
    let Some(source) = locate(provider) else { return Ok(None) };
    let key = match &source {
        Source::Env(var) => std::env::var(var).unwrap_or_default(),
        Source::File(path) => {
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
        }
    };
    let key = key.trim().to_string();
    if key.is_empty() {
        bail!("the key at {source} is empty");
    }
    Ok(Some((key, source)))
}

/// The last four characters, the most of a key anything may show.
pub fn tail(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let start = chars.len().saturating_sub(4);
    format!("…{}", chars[start..].iter().collect::<String>())
}

/// Checks a key's shape before it is stored: one non-empty line with no spaces.
pub fn validate(key: &str) -> Result<String> {
    let key = key.trim();
    if key.is_empty() {
        bail!("no key given");
    }
    if key.contains(char::is_whitespace) {
        bail!("the key contains whitespace; paste only the key");
    }
    Ok(key.to_string())
}

/// Stores the key for `provider`, replacing any earlier one.
pub fn store(provider: Provider, key: &str) -> Result<PathBuf> {
    store_in(&keys_dir(), provider, key)
}

#[cfg(unix)]
fn store_in(dir: &Path, provider: Provider, key: &str) -> Result<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let key = validate(key)?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("setting {} to mode 0700", dir.display()))?;
    let dir_mode = mode(dir)?;
    if dir_mode != 0o700 {
        bail!("{} is mode {dir_mode:o}, not 0700; the key was not written", dir.display());
    }

    let path = dir.join(provider.as_str());
    let tmp = dir.join(format!(".{}.tmp", provider.as_str()));
    let _ = std::fs::remove_file(&tmp);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    let written = (|| -> Result<()> {
        // `mode()` above is masked by the umask; set it explicitly too.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(key.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        let tmp_mode = mode(&tmp)?;
        if tmp_mode != 0o600 {
            bail!("{} came out mode {tmp_mode:o}, not 0600", tmp.display());
        }
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.context("the key was not written"));
    }
    std::fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(path)
}

#[cfg(not(unix))]
fn store_in(_dir: &Path, _provider: Provider, _key: &str) -> Result<PathBuf> {
    bail!("key storage needs a Unix file system; set the provider's environment variable instead")
}

/// Removes the stored key file. True when there was one.
pub fn remove(provider: Provider) -> Result<bool> {
    let path = key_path(provider);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

/// Ways the key file or its directory could be read by another account
/// (ADR-604 rule 6: name the weak configuration). Empty when all is well.
#[cfg(unix)]
pub fn exposure(path: &Path) -> Vec<String> {
    use std::os::unix::fs::MetadataExt;
    let mut found = Vec::new();
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let candidates = [Some(path), path.parent()];
    for p in candidates.into_iter().flatten() {
        let Ok(meta) = std::fs::metadata(p) else { continue };
        let m = meta.mode() & 0o777;
        if m & 0o077 != 0 {
            let want = if meta.is_dir() { "0700" } else { "0600" };
            found.push(format!("{} is mode {m:o}; others can read it (want {want})", p.display()));
        }
        if meta.uid() != uid {
            found.push(format!("{} is owned by uid {}, not you ({uid})", p.display(), meta.uid()));
        }
    }
    found
}

#[cfg(not(unix))]
pub fn exposure(_path: &Path) -> Vec<String> {
    Vec::new()
}

#[cfg(unix)]
fn mode(path: &Path) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(std::fs::metadata(path)
        .with_context(|| format!("reading {}", path.display()))?
        .permissions()
        .mode()
        & 0o777)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ways-agent-keys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn store_writes_0600_in_0700_and_replaces() {
        let dir = scratch("store").join("keys");
        let path = store_in(&dir, Provider::Anthropic, "  sk-test-1234\n").unwrap();
        assert_eq!(mode(&path).unwrap(), 0o600);
        assert_eq!(mode(&dir).unwrap(), 0o700);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "sk-test-1234\n");
        store_in(&dir, Provider::Anthropic, "sk-test-5678").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "sk-test-5678\n");
        assert!(exposure(&path).is_empty());
        assert!(!dir.join(".anthropic.tmp").exists());
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn store_tightens_a_loose_directory() {
        let dir = scratch("loose").join("keys");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        store_in(&dir, Provider::Openrouter, "or-key").unwrap();
        assert_eq!(mode(&dir).unwrap(), 0o700);
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn exposure_names_a_readable_key() {
        let dir = scratch("exposed").join("keys");
        let path = store_in(&dir, Provider::Anthropic, "k").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let found = exposure(&path);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("644"));
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn validate_refuses_empty_and_spaced_keys() {
        assert!(validate("  \n").is_err());
        assert!(validate("sk a").is_err());
        assert_eq!(validate("\tsk-a\n").unwrap(), "sk-a");
    }

    #[test]
    fn tail_shows_at_most_four_characters() {
        assert_eq!(tail("sk-ant-abcdef"), "…cdef");
        assert_eq!(tail("ab"), "…ab");
    }

    #[test]
    fn locate_prefers_the_file_when_no_env_is_set() {
        let dir = scratch("locate").join("keys");
        assert_eq!(locate_in(Provider::Openrouter, &dir), None.or(locate_env(Provider::Openrouter)));
        let path = store_in(&dir, Provider::Openrouter, "k").unwrap();
        if locate_env(Provider::Openrouter).is_none() {
            assert_eq!(locate_in(Provider::Openrouter, &dir), Some(Source::File(path)));
        }
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    fn locate_env(p: Provider) -> Option<Source> {
        std::env::var(p.key_env()).ok().filter(|v| !v.trim().is_empty()).map(|_| Source::Env(p.key_env()))
    }
}
