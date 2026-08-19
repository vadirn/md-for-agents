/// Rough token estimate: one token per four characters.
///
/// Presentation policy, deliberately kept out of the `mdstruct` structural core.
/// A reader's fold thresholds are tuned against it, and a search result reports
/// it so a caller can price the read before opening the file.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_zero_tokens() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn four_chars_per_token_floors() {
        // Integer division floors, so sub-token remainders are dropped.
        assert_eq!(estimate_tokens("abc"), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefg"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn counts_unicode_scalars_not_bytes() {
        // "тест" is 4 chars / 8 bytes; the estimate uses chars.
        assert_eq!(estimate_tokens("тест"), 1);
    }
}
