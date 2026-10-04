//! Text rules shared by all features for user-visible names.

/// Control characters break storage and logs; bidi overrides can disguise text.
pub fn is_disallowed_in_names(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Trims a name, drops disallowed characters and caps its length.
/// Returns `None` when nothing printable is left.
pub fn clean_name(raw: &str, max_chars: usize) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !is_disallowed_in_names(*c))
        .collect::<String>()
        .trim()
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_end()
        .to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_name_trims_strips_and_caps() {
        assert_eq!(clean_name("  Long  ", 80).as_deref(), Some("Long"));
        assert_eq!(clean_name("Lo\u{0}ng\u{202E}", 80).as_deref(), Some("Long"));
        assert_eq!(clean_name("abcdef", 3).as_deref(), Some("abc"));
        assert_eq!(clean_name(" \u{0} ", 80), None);
    }

    #[test]
    fn clean_name_counts_characters_not_bytes() {
        assert_eq!(clean_name("Phạm Trịnh", 4).as_deref(), Some("Phạm"));
    }
}
