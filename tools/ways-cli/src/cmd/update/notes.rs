//! Release notes for a what-if: every published release of a component
//! between the installed version (exclusive) and the target (inclusive),
//! read from GitHub Releases of `aaronsb/agent-ways`.
//!
//! The releases API is read with `curl`, the tool the installer one-liner
//! already needs, so the report works without `gh`. `GH_TOKEN` or
//! `GITHUB_TOKEN`, when set, is sent through curl's config on stdin, never
//! on the command line. Offline, the report says the notes could not be
//! fetched and goes on.

use super::version_older;
use std::io::Write as _;
use std::process::{Command, Stdio};

/// The repository the suite releases from.
pub const REPO: &str = "aaronsb/agent-ways";

/// The pages of 100 releases read at most.
const MAX_PAGES: usize = 6;

/// One published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub body: String,
}

/// Every published release (drafts and prereleases left out), newest first,
/// or why they could not be read.
///
/// `WAYS_RELEASES_JSON` names a file holding one page of the API's answer,
/// read in its place: a mirror for an offline machine, and the fixture the
/// tests use.
pub fn fetch() -> Result<Vec<Release>, String> {
    if let Some(file) = std::env::var_os("WAYS_RELEASES_JSON") {
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", std::path::Path::new(&file).display()))?;
        let list: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", std::path::Path::new(&file).display()))?;
        return Ok(parse(list.as_array().map(Vec::as_slice).unwrap_or(&[])));
    }
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=100&page={page}");
        let text = curl(&url)?;
        let list: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("GitHub answered with something other than JSON: {e}"))?;
        let Some(items) = list.as_array() else {
            let msg = list.get("message").and_then(|m| m.as_str()).unwrap_or("an unexpected answer");
            return Err(format!("GitHub: {msg}"));
        };
        out.extend(parse(items));
        if items.len() < 100 {
            break;
        }
    }
    Ok(out)
}

fn parse(items: &[serde_json::Value]) -> Vec<Release> {
    items
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false) && !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| Some(Release { tag: r["tag_name"].as_str()?.to_string(), body: r["body"].as_str().unwrap_or("").to_string() }))
        .collect()
}

/// GET `url` with curl, with a token from the environment when one is set.
fn curl(url: &str) -> Result<String, String> {
    let token = ["GH_TOKEN", "GITHUB_TOKEN"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
    let mut child = Command::new("curl")
        .args(["-sS", "-f", "-L", "--max-time", "15", "-H", "Accept: application/vnd.github+json", "--config", "-", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl could not run ({e})"))?;
    if let Some(mut stdin) = child.stdin.take() {
        if let Some(t) = token {
            let _ = writeln!(stdin, "header = \"Authorization: Bearer {t}\"");
        }
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "GitHub Releases could not be reached".to_string() } else { err });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The version a tag names for `component`: `attend-v0.28.1` → `0.28.1`.
/// `attend-chat-v0.21.0` is not an `attend` tag.
pub fn tag_version<'a>(tag: &'a str, component: &str) -> Option<&'a str> {
    tag.strip_prefix(component)?.strip_prefix("-v")
}

/// The releases of `component` after `installed` up to and including
/// `target`, oldest first. An unparseable installed version takes only the
/// target's own release.
pub fn between<'a>(releases: &'a [Release], component: &str, installed: &str, target: &str) -> Vec<&'a Release> {
    let mut hits: Vec<(&str, &Release)> = releases
        .iter()
        .filter_map(|r| Some((tag_version(&r.tag, component)?, r)))
        .filter(|(v, _)| version_older(installed, v) && !version_older(target, v))
        .collect();
    hits.sort_by(|(a, _), (b, _)| if version_older(a, b) { std::cmp::Ordering::Less } else if version_older(b, a) { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal });
    hits.into_iter().map(|(_, r)| r).collect()
}

/// The newest release of `component`.
pub fn latest<'a>(releases: &'a [Release], component: &str) -> Option<&'a str> {
    releases
        .iter()
        .filter_map(|r| tag_version(&r.tag, component))
        .fold(None, |best: Option<&str>, v| match best {
            Some(b) if !version_older(b, v) => Some(b),
            _ => Some(v),
        })
}

/// A release body as the report shows it: the `## component X.Y.Z` heading
/// (the report prints its own) and the CI tail after the last `---` that
/// introduces the pre-built binaries are dropped, and blank runs collapse.
pub fn trim(body: &str) -> Vec<String> {
    let body = body.replace("\r\n", "\n");
    let mut lines: Vec<&str> = body.lines().collect();
    if let Some(cut) = lines.iter().rposition(|l| l.trim() == "---") {
        let tail = lines[cut + 1..].join("\n").to_lowercase();
        if tail.contains("pre-built") || tail.contains("build from source") {
            lines.truncate(cut);
        }
    }
    if lines.first().is_some_and(|l| l.starts_with("## ")) {
        lines.remove(0);
    }
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        let l = l.trim_end();
        if l.is_empty() && out.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        out.push(l.to_string());
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str) -> Release {
        Release { tag: tag.into(), body: String::new() }
    }

    #[test]
    fn the_range_is_after_installed_up_to_target_for_that_component_only() {
        let all = [
            rel("ways-v1.41.1"),
            rel("attend-chat-v0.21.0"),
            rel("ways-v1.41.0"),
            rel("ways-audit-v1.0.3"),
            rel("ways-v1.40.2"),
            rel("ways-v1.39.0"),
            rel("ways-v1.42.0"),
        ];
        let tags = |c: &str, a: &str, b: &str| between(&all, c, a, b).iter().map(|r| r.tag.clone()).collect::<Vec<_>>();
        assert_eq!(tags("ways", "1.40.0", "1.41.1"), ["ways-v1.40.2", "ways-v1.41.0", "ways-v1.41.1"]);
        assert_eq!(tags("ways", "1.41.1", "1.41.1"), Vec::<String>::new(), "unchanged has no notes");
        assert!(tags("attend", "0.1.0", "9.0.0").is_empty(), "attend-chat tags are not attend's");
        assert_eq!(tags("attend-chat", "0.20.0", "0.21.0"), ["attend-chat-v0.21.0"]);
        assert_eq!(latest(&all, "ways"), Some("1.42.0"));
        assert_eq!(latest(&all, "ways-mcp"), None);
    }

    #[test]
    fn trim_drops_the_heading_and_the_ci_tail() {
        let body = "## ways 1.41.1\r\n\r\n### Fixed\r\n- **Entry dialogs wrap.** A fix (#1021).\r\n\r\n\r\n---\r\nPre-built ways CLI binaries for 4 platforms.\r\n\r\nInstall: `bash tools/scripts/download-prebuilt.sh ways`\r\nOr build from source: `make ways`\r\n";
        assert_eq!(trim(body), ["### Fixed", "- **Entry dialogs wrap.** A fix (#1021)."]);
        // A rule that does not introduce the CI tail stays.
        assert_eq!(trim("### Added\n- a\n---\n### Notes\n- b\n"), ["### Added", "- a", "---", "### Notes", "- b"]);
        assert!(trim("").is_empty());
    }

    #[test]
    fn parse_skips_drafts_and_prereleases() {
        let v: serde_json::Value = serde_json::from_str(
            r#"[{"tag_name":"ways-v1.0.0","body":"b","draft":false,"prerelease":false},
                {"tag_name":"ways-v1.1.0","body":"d","draft":true,"prerelease":false},
                {"tag_name":"ways-v1.2.0-rc1","body":"p","draft":false,"prerelease":true},
                {"tag_name":"attend-v0.1.0","body":null,"draft":false,"prerelease":false}]"#,
        )
        .unwrap();
        let got = parse(v.as_array().unwrap());
        assert_eq!(got, [Release { tag: "ways-v1.0.0".into(), body: "b".into() }, Release { tag: "attend-v0.1.0".into(), body: String::new() }]);
    }
}
