//! Python's `str` character classes, which the outliners were first written
//! and scored with. ASCII matches Python exactly. Beyond ASCII, Rust's Unicode
//! properties stand in for Python's general categories: they agree on letters
//! and digits, and differ only on rare marks and symbols.

/// `str.isalpha`.
pub(crate) fn is_alpha(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphabetic()
    } else {
        c.is_alphabetic()
    }
}

/// `str.isalnum`.
pub(crate) fn is_alnum(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric()
    } else {
        c.is_alphanumeric()
    }
}

/// `str.isdigit`.
pub(crate) fn is_digit(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_digit()
    } else {
        c.is_numeric()
    }
}

/// `\w` in a `str` pattern.
pub(crate) fn is_word(c: char) -> bool {
    is_alnum(c) || c == '_'
}

/// `\s` in a `str` pattern, which also counts the separators U+001C..U+001F.
pub(crate) fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.isupper` on one character.
pub(crate) fn is_upper(c: char) -> bool {
    c.is_uppercase()
}

/// `text[i:].startswith(pat)` on a char slice.
pub(crate) fn starts(text: &[char], i: usize, pat: &str) -> bool {
    (i..).zip(pat.chars()).all(|(k, p)| text.get(k) == Some(&p))
}

/// `text.count("\n", from, to)`.
pub(crate) fn newlines(text: &[char], from: usize, to: usize) -> usize {
    let to = to.min(text.len());
    if from >= to {
        return 0;
    }
    text[from..to].iter().filter(|&&c| c == '\n').count()
}

/// `text.find(pat, from)`.
pub(crate) fn find(text: &[char], pat: &str, from: usize) -> Option<usize> {
    (from..text.len()).find(|&k| starts(text, k, pat))
}

/// A char slice equals `s`.
pub(crate) fn eq(chars: &[char], s: &str) -> bool {
    chars.iter().copied().eq(s.chars())
}
