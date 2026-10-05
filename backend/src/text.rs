//! Text rules shared by all features for user-visible names.

/// Control characters break storage and logs; bidi overrides can disguise
/// text; zero-width and blank-looking characters can make a name look empty or
/// impersonate another. Zero-width (non-)joiners U+200C/U+200D are kept: emoji
/// sequences and some scripts need them.
pub fn is_disallowed_in_names(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{034F}'
                | '\u{115F}'
                | '\u{1160}'
                | '\u{200B}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{2800}'
                | '\u{3164}'
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
    fn joiners_used_by_emoji_and_scripts_are_kept() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(clean_name(family, 80).as_deref(), Some(family));
        assert_eq!(clean_name("a\u{200C}b", 80).as_deref(), Some("a\u{200C}b"));
    }

    #[test]
    fn blank_looking_characters_are_removed() {
        for blank in ["\u{3164}", "\u{115F}\u{1160}", "\u{2800}", "\u{034F}"] {
            assert_eq!(clean_name(blank, 80), None, "{blank:?}");
        }
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
