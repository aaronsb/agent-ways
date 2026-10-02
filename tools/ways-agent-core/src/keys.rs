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

/// Provider keys are long; anything shorter is a mis-paste.
pub const MIN_KEY_CHARS: usize = 20;

/// The last four characters, the most of a key anything may show. A value too
/// short to be a key shows nothing, since four characters would be most of it.
pub fn tail(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < MIN_KEY_CHARS {
        return "…(too short to be a key)".to_string();
    }
    format!("…{}", chars[chars.len() - 4..].iter().collect::<String>())
}

/// Checks a key's shape before it is stored: one line, no spaces, long enough
/// to be a provider key.
pub fn validate(key: &str) -> Result<String> {
    let key = key.trim();
    if key.is_empty() {
        bail!("no key given");
    }
    if key.contains(char::is_whitespace) {
        bail!("the key contains whitespace; paste only the key");
    }
    if key.chars().count() < MIN_KEY_CHARS {
        bail!("that is {} characters; provider keys are longer. Paste the whole key", key.chars().count());
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
    // A symlinked keys directory would put the key wherever the link points.
    if std::fs::symlink_metadata(dir)?.file_type().is_symlink() {
        bail!("{} is a symbolic link; the key was not written", dir.display());
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("setting {} to mode 0700", dir.display()))?;
    let dir_mode = mode(dir)?;
    if dir_mode != 0o700 {
        bail!("{} is mode {dir_mode:o}, not 0700; the key was not written", dir.display());
    }

    let path = dir.join(provider.as_str());
    // A name of its own per write: two concurrent writes never share a file.
    let tmp = dir.join(format!(".{}.{}.{}.tmp", provider.as_str(), std::process::id(), unique()));
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
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("replacing {}", path.display()));
    }
    // Persist the rename itself, so a crash cannot bring the old key back.
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(path)
}

/// A process-local sequence number, so temp files written by one process's
/// threads never share a name. Off Unix only tests call it: agent.yaml is
/// written by the settings writer, and the key store is Unix-only.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
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

/// What the last zero-cost check of a provider's key found, kept so the agent
/// gates only on a key that worked (ADR-196 §6). The record names the key it
/// checked by a stamp (a file's modification time and length, or a variable's
/// length and hash) and the model it checked against: a different key or a
/// different model makes it stale.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CheckRecord {
    /// `valid`, `invalid`, `no_credit`, `rate_limited`, `model_unavailable`,
    /// `unreachable` or `failed`.
    pub result: String,
    /// The model checked against.
    pub model: String,
    /// Seconds since the Unix epoch.
    pub at: u64,
    /// Where the key came from, as `Source` displays it.
    pub source: String,
    /// A key file's (mtime seconds, length), or a variable's (length, hash).
    pub stamp: Option<(u64, u64)>,
}

impl CheckRecord {
    /// A record taken now for `source`.
    pub fn now(result: &str, model: &str, source: &Source) -> CheckRecord {
        CheckRecord {
            result: result.to_string(),
            model: model.to_string(),
            at: now_s(),
            source: source.to_string(),
            stamp: stamp(source),
        }
    }

    /// True when the record still describes the key `source` points at,
    /// checked against `model`.
    pub fn describes(&self, source: &Source, model: &str) -> bool {
        self.source == source.to_string() && self.stamp == stamp(source) && self.model == model
    }

    /// Seconds since the check.
    pub fn age_s(&self) -> u64 {
        now_s().saturating_sub(self.at)
    }
}

fn stamp(source: &Source) -> Option<(u64, u64)> {
    let path = match source {
        Source::File(path) => path,
        Source::Env(var) => {
            // A hash, not the value: enough to notice the variable changed.
            use std::hash::{Hash, Hasher};
            let value = std::env::var(var).ok()?;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            value.trim().hash(&mut h);
            return Some((value.trim().len() as u64, h.finish()));
        }
    };
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some((mtime, meta.len()))
}

fn now_s() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn check_path(provider: Provider) -> PathBuf {
    ways_core::paths::state_root().join("agent").join(format!("key-check-{}.json", provider.as_str()))
}

/// Records a check. Best effort: a failure to record only means the next use
/// checks again.
pub fn record_check(provider: Provider, record: &CheckRecord) {
    let path = check_path(provider);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(record) {
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// The last recorded check of this provider's key, if any.
pub fn last_check(provider: Provider) -> Option<CheckRecord> {
    serde_json::from_str(&std::fs::read_to_string(check_path(provider)).ok()?).ok()
}

/// Forgets the recorded check, as when the key is removed.
pub fn clear_check(provider: Provider) {
    let _ = std::fs::remove_file(check_path(provider));
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
        let path = store_in(&dir, Provider::Anthropic, "  sk-test-1234-abcdefghijkl\n").unwrap();
        assert_eq!(mode(&path).unwrap(), 0o600);
        assert_eq!(mode(&dir).unwrap(), 0o700);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "sk-test-1234-abcdefghijkl\n");
        store_in(&dir, Provider::Anthropic, "sk-test-5678-abcdefghijkl").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "sk-test-5678-abcdefghijkl\n");
        assert!(exposure(&path).is_empty());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temp file left behind");
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn store_tightens_a_loose_directory() {
        let dir = scratch("loose").join("keys");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        store_in(&dir, Provider::Openrouter, "sk-or-v1-abcdefghijklmnop").unwrap();
        assert_eq!(mode(&dir).unwrap(), 0o700);
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn exposure_names_a_readable_key() {
        let dir = scratch("exposed").join("keys");
        let path = store_in(&dir, Provider::Anthropic, "sk-ant-abcdefghijklmnop").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let found = exposure(&path);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("644"));
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn validate_refuses_empty_spaced_and_short_keys() {
        assert!(validate("  \n").is_err());
        assert!(validate("sk-ant-abcdefghij klmnop").is_err());
        assert!(validate("sk-a").is_err());
        assert_eq!(validate("\tsk-ant-abcdefghijklmnop\n").unwrap(), "sk-ant-abcdefghijklmnop");
    }

    #[test]
    fn tail_shows_at_most_four_characters_and_nothing_of_a_short_value() {
        assert_eq!(tail("sk-ant-abcdefghijklmnop"), "…mnop");
        assert_eq!(tail("ab"), "…(too short to be a key)");
    }

    #[test]
    fn store_refuses_a_symlinked_keys_directory() {
        let base = scratch("symlink");
        let real = base.join("elsewhere");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, base.join("keys")).unwrap();
        assert!(store_in(&base.join("keys"), Provider::Anthropic, "sk-ant-abcdefghijklmnop").is_err());
        assert_eq!(std::fs::read_dir(&real).unwrap().count(), 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_check_record_stops_describing_a_replaced_key_file() {
        let dir = scratch("record").join("keys");
        let path = store_in(&dir, Provider::Anthropic, "sk-ant-abcdefghijklmnop").unwrap();
        let source = Source::File(path.clone());
        let record = CheckRecord::now("valid", "m", &source);
        assert!(record.describes(&source, "m"));
        assert!(!record.describes(&source, "other-model"));
        store_in(&dir, Provider::Anthropic, "sk-ant-abcdefghijklmnopqrstu").unwrap();
        assert!(!record.describes(&source, "m"));
        assert!(!record.describes(&Source::Env("ANTHROPIC_API_KEY"), "m"));
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn locate_prefers_the_file_when_no_env_is_set() {
        let dir = scratch("locate").join("keys");
        assert_eq!(locate_in(Provider::Openrouter, &dir), None.or(locate_env(Provider::Openrouter)));
        let path = store_in(&dir, Provider::Openrouter, "sk-or-v1-abcdefghijklmnop").unwrap();
        if locate_env(Provider::Openrouter).is_none() {
            assert_eq!(locate_in(Provider::Openrouter, &dir), Some(Source::File(path)));
        }
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    fn locate_env(p: Provider) -> Option<Source> {
        std::env::var(p.key_env()).ok().filter(|v| !v.trim().is_empty()).map(|_| Source::Env(p.key_env()))
    }
}
