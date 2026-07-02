//! System locale detection.
//!
//! Reads $LANG / $LC_MESSAGES / $LC_ALL to determine the system language.
//! Extracts the language code from locale strings like "ja_JP.UTF-8" → "ja".

/// Extract language from system locale environment variables.
/// Priority: LC_ALL > LC_MESSAGES > LANG
pub fn locale_language() -> Option<String> {
    for var in &["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            if let Some(lang) = parse_locale(&val) {
                if lang != "c" && lang != "posix" {
                    return Some(lang);
                }
            }
        }
    }
    None
}

/// Parse a locale string like "ja_JP.UTF-8" into a language code "ja".
/// Returns None for empty or "C"/"POSIX" locales.
fn parse_locale(locale: &str) -> Option<String> {
    let s = locale.trim();
    if s.is_empty() {
        return None;
    }
    // Strip encoding (e.g., ".UTF-8")
    let without_encoding = s.split('.').next().unwrap_or(s);
    // Take language before region (e.g., "ja" from "ja_JP")
    let lang = without_encoding.split('_').next().unwrap_or(without_encoding);
    if lang.is_empty() {
        return None;
    }
    Some(lang.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_locale() {
        assert_eq!(parse_locale("ja_JP.UTF-8"), Some("ja".to_string()));
        assert_eq!(parse_locale("en_US.UTF-8"), Some("en".to_string()));
        assert_eq!(parse_locale("de_DE"), Some("de".to_string()));
        assert_eq!(parse_locale("fr"), Some("fr".to_string()));
        assert_eq!(parse_locale("C"), Some("c".to_string()));
        assert_eq!(parse_locale(""), None);
    }
}
