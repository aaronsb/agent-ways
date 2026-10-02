//! The directory name Claude Code gives a project under `~/.claude/projects`.

/// The longest slug Claude Code writes before it truncates and appends a hash.
pub const MAX_SLUG_LEN: usize = 200;

/// The directory name Claude Code gives the project at `path`.
///
/// Every character outside ASCII `[A-Za-z0-9]` becomes `-`, so `/a/_b.c` is
/// `-a--b-c`. Claude Code applies `replace(/[^a-zA-Z0-9]/g, "-")` to the
/// path's UTF-16 code units, so a character outside the BMP yields two dashes.
///
/// A slug longer than [`MAX_SLUG_LEN`] is cut to that length and followed by
/// `-` and a hash of the original path: the absolute value of a 32-bit
/// `h = h * 31 + unit` over the path's UTF-16 code units, written in base 36.
/// This is the `sanitizePath` rule in Claude Code 2.1.287. Claude Code itself
/// also finds a long project's directory by its 200-character prefix, because
/// the hash has differed between its runtimes; [`crate::find_project_dir_in`]
/// does the same.
pub fn project_slug(path: &str) -> String {
    let mut slug = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else {
            slug.extend(std::iter::repeat_n('-', c.len_utf16()));
        }
    }
    if slug.len() <= MAX_SLUG_LEN {
        return slug;
    }
    // Every char in `slug` is ASCII, so the byte cut is a char cut.
    slug.truncate(MAX_SLUG_LEN);
    slug.push('-');
    slug.push_str(&base36(path_hash(path).unsigned_abs()));
    slug
}

/// Claude Code's path hash: `h = (h << 5) - h + unit`, wrapping at 32 bits,
/// over UTF-16 code units.
pub(crate) fn path_hash(path: &str) -> i32 {
    path.encode_utf16().fold(0i32, |h, unit| {
        h.wrapping_shl(5).wrapping_sub(h).wrapping_add(i32::from(unit))
    })
}

/// Lowercase base 36, as JavaScript's `Number.prototype.toString(36)`.
pub(crate) fn base36(mut n: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("base-36 digits are ASCII")
}

/// True when `name` is the directory Claude Code would give `path`: the exact
/// slug, or, for a slug over [`MAX_SLUG_LEN`], its prefix followed by `-` and
/// any hash.
pub fn slug_matches(name: &str, path: &str) -> bool {
    let slug = project_slug(path);
    if name == slug {
        return true;
    }
    slug.len() > MAX_SLUG_LEN && name.starts_with(&slug[..=MAX_SLUG_LEN])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_non_alphanumeric_to_a_dash() {
        // Observed in ~/.claude/projects: `_prod` is stored as `-prod`.
        assert_eq!(project_slug("/home/a/Projects/ai/mcp/_prod"), "-home-a-Projects-ai-mcp--prod");
        assert_eq!(project_slug("/home/a/.dotfiles"), "-home-a--dotfiles");
        assert_eq!(project_slug("/x/llama.cpp-kv"), "-x-llama-cpp-kv");
        assert_eq!(project_slug("/x/SWA - Delivery"), "-x-SWA---Delivery");
        assert_eq!(project_slug(""), "");
    }

    #[test]
    fn non_ascii_counts_utf16_units() {
        assert_eq!(project_slug("/x/é"), "-x--");
        assert_eq!(project_slug("/x/🦀"), "-x---");
        assert_eq!(project_slug("/x/日本"), "-x---");
    }

    // The expected values below were produced by running Claude Code's own
    // `sanitizePath` and hash functions, copied from the 2.1.287 binary, under
    // node: `n.length<=200 ? n : n.slice(0,200)+"-"+Math.abs(qZ(e)).toString(36)`.

    #[test]
    fn slug_of_199_and_200_chars_is_not_truncated() {
        let p199 = format!("/{}", "a".repeat(198));
        let p200 = format!("/{}", "a".repeat(199));
        assert_eq!(project_slug(&p199), format!("-{}", "a".repeat(198)));
        assert_eq!(project_slug(&p200).len(), 200);
        assert_eq!(project_slug(&p200), format!("-{}", "a".repeat(199)));
    }

    #[test]
    fn slug_of_201_chars_is_cut_and_hashed() {
        // Hash -676821585: a negative hash takes its absolute value.
        let p201 = format!("/{}", "a".repeat(200));
        let slug = project_slug(&p201);
        assert_eq!(slug, format!("-{}-b6ymvl", "a".repeat(199)));
        assert_eq!(slug.len(), 207);
    }

    #[test]
    fn long_slug_hashes_match_claude_code() {
        let deep = format!("/home/aaron/Projects/{}repo", "deep/".repeat(40));
        assert!(project_slug(&deep).ends_with("-76t208"));
        let underscores = format!("/srv/{}", "b_".repeat(100));
        assert!(project_slug(&underscores).ends_with("-3ecfff"));
    }

    #[test]
    fn long_non_ascii_slug_hashes_utf16_units() {
        // 120 crabs are 240 UTF-16 units: the cut lands inside the dashes, and
        // the hash runs over surrogate pairs, not scalar values.
        let crabs = format!("/x/{}", "🦀".repeat(120));
        let slug = project_slug(&crabs);
        assert_eq!(slug, format!("-x{}-cxbkkq", "-".repeat(198)));
    }

    #[test]
    fn slug_matches_a_long_name_by_prefix() {
        let p = format!("/{}", "a".repeat(250));
        let exact = project_slug(&p);
        assert!(slug_matches(&exact, &p));
        // A directory another runtime named with a different hash still matches.
        let other = format!("{}-zzz", &exact[..MAX_SLUG_LEN]);
        assert!(slug_matches(&other, &p));
        assert!(!slug_matches("-b", &p));
        assert!(!slug_matches(&format!("-{}", "a".repeat(199)), &p));
    }

    #[test]
    fn base36_matches_javascript() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        // Math.abs(-2147483648).toString(36)
        assert_eq!(base36(i32::MIN.unsigned_abs()), "zik0zk");
    }
}
