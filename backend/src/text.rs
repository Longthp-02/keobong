//! Text rules shared by all features for user-visible names.

/// Control characters break storage and logs; bidi overrides can disguise
/// text; zero-width and other invisible format characters can make a name
/// look blank or impersonate another.
pub fn is_disallowed_in_names(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
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
    fn invisible_characters_are_removed() {
        assert_eq!(clean_name("\u{200B}\u{FEFF}\u{2060}\u{200E}", 80), None);
        assert_eq!(clean_name("A\u{200B}n", 80).as_deref(), Some("An"));
    }

    #[test]
    fn clean_name_counts_characters_not_bytes() {
        assert_eq!(clean_name("Phạm Trịnh", 4).as_deref(), Some("Phạm"));
    }
}
