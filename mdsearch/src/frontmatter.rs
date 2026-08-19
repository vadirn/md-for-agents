//! The leading YAML frontmatter block: where the body starts, and the one field
//! the index scores on its own.
//!
//! Indexing asks two questions of a file — where its prose begins, and what its
//! `description:` says — so this scans delimiters instead of parsing Markdown.
//! A full parse of every file in a folder would answer neither question better.

use serde_yaml::Value;

/// The leading frontmatter block, scanned once.
///
/// A block opens when the first line, BOM stripped and trimmed, is exactly `---`.
/// It closes at the first later line whose trim is `---`. Without a closing
/// delimiter there is no block, and the whole file is body.
struct Block<'a> {
    /// BOM-stripped view of the original content. Offsets index into this.
    stripped: &'a str,
    /// Inner YAML text, present only when a closing delimiter was found.
    yaml: Option<String>,
    /// Byte offset of the first body character, or `None` without a block.
    body_offset: Option<usize>,
}

/// Index of the next `\n` at or after `from`, or `bytes.len()` if none.
fn next_newline(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// End of a line's content within `[start, nl)`, excluding a trailing `\r` so
/// CRLF and LF endings compare equal.
fn line_content_end(bytes: &[u8], start: usize, nl: usize) -> usize {
    if nl > start && bytes[nl - 1] == b'\r' {
        nl - 1
    } else {
        nl
    }
}

fn block(content: &str) -> Block<'_> {
    let stripped = content.trim_start_matches('\u{feff}');
    let bytes = stripped.as_bytes();

    let none = |stripped| Block {
        stripped,
        yaml: None,
        body_offset: None,
    };

    let first_nl = next_newline(bytes, 0);
    let first_line = &stripped[0..line_content_end(bytes, 0, first_nl)];
    if first_line.trim() != "---" {
        return none(stripped);
    }
    if first_nl == bytes.len() {
        // An opening `---` with nothing after it opens no block.
        return none(stripped);
    }

    let mut i = first_nl + 1;
    let mut inner: Vec<&str> = Vec::new();

    while i < bytes.len() {
        let line_start = i;
        let nl = next_newline(bytes, line_start);
        let line = &stripped[line_start..line_content_end(bytes, line_start, nl)];
        if line.trim() == "---" {
            // The body starts after the newline that ends the closing delimiter.
            let body_offset = (nl + 1).min(stripped.len());
            return Block {
                stripped,
                yaml: Some(inner.join("\n")),
                body_offset: Some(body_offset),
            };
        }
        inner.push(line);
        if nl == bytes.len() {
            break; // Last line, no newline, no closing delimiter.
        }
        i = nl + 1;
    }

    none(stripped)
}

/// The prose after the frontmatter block, or the whole file when it has none.
pub fn body(content: &str) -> &str {
    let b = block(content);
    match b.body_offset {
        Some(off) => &b.stripped[off..],
        None => b.stripped,
    }
}

/// The frontmatter `description:` rendered for indexing, or an empty string.
///
/// A block that is not valid YAML yields an empty string rather than an error,
/// so one malformed file never fails the search.
pub fn description(content: &str) -> String {
    let Some(yaml) = block(content).yaml else {
        return String::new();
    };
    let Ok(value) = serde_yaml::from_str::<Value>(&yaml) else {
        return String::new();
    };
    match value.get("description") {
        Some(v) => display(v),
        None => String::new(),
    }
}

/// Render a YAML value as one indexable string.
fn display(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Sequence(seq) => seq.iter().map(display).collect::<Vec<_>>().join(", "),
        Value::Mapping(m) => m
            .iter()
            .map(|(k, v)| format!("{}: {}", display(k), display(v)))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Tagged(t) => display(&t.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_starts_after_the_closing_delimiter() {
        let c = "---\ntype: note\n---\nfirst line\n";
        assert_eq!(body(c), "first line\n");
    }

    #[test]
    fn a_file_without_frontmatter_is_all_body() {
        assert_eq!(body("just prose\n"), "just prose\n");
    }

    #[test]
    fn an_unclosed_block_is_all_body() {
        // Without a closing `---` the text is prose that happens to start with one.
        let c = "---\ntype: note\nstill prose\n";
        assert_eq!(body(c), c);
    }

    #[test]
    fn a_bom_prefixed_block_still_opens() {
        let c = "\u{feff}---\ntype: note\n---\nbody\n";
        assert_eq!(body(c), "body\n");
        assert_eq!(description(c), "");
    }

    #[test]
    fn crlf_endings_close_the_block() {
        let c = "---\r\ndescription: precis\r\n---\r\nbody\r\n";
        assert_eq!(body(c), "body\r\n");
        assert_eq!(description(c), "precis");
    }

    #[test]
    fn description_reads_the_named_field_only() {
        let c = "---\ntype: card\ndescription: what it says\n---\nbody\n";
        assert_eq!(description(c), "what it says");
    }

    #[test]
    fn a_sequence_description_joins_its_items() {
        let c = "---\ndescription:\n  - one\n  - two\n---\nbody\n";
        assert_eq!(description(c), "one, two");
    }

    #[test]
    fn a_malformed_block_yields_an_empty_description() {
        let c = "---\ndescription: [unclosed\n---\nbody\n";
        assert_eq!(description(c), "");
        assert_eq!(body(c), "body\n");
    }

    #[test]
    fn an_empty_body_survives_a_block_that_ends_the_file() {
        let c = "---\ntype: note\n---";
        assert_eq!(body(c), "");
    }
}
