//! Rewrites every line ending to LF.
//!
//! Reads no parse: CommonMark makes every `\r` a line ending, in a code block
//! and front matter as much as anywhere else.

use crate::span::LineIndex;

/// One line ending the rewrite would change, in the source's own coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndingChange {
    /// 1-based line this ending terminates.
    pub line: usize,
    /// 1-based column of the `\r`.
    pub column: usize,
    /// Byte offset of the `\r`.
    pub start: usize,
    /// What the source holds there: `"\r\n"` or `"\r"`.
    pub old: &'static str,
}

/// The LF rewrite of one document: the bytes, and every ending that changed.
///
/// Unlike [`crate::Normalization`] and [`crate::Padding`] this has no
/// `accepted`: the rewrite's effect is fixed by its own statement rather than
/// by the document, so there is nothing for a guard to read.
#[derive(Debug, Clone)]
pub struct LineEndings {
    /// The rewritten bytes. Holds no `\r`.
    pub output: String,
    /// Every ending that was not already LF.
    pub changes: Vec<EndingChange>,
}

impl LineEndings {
    /// Whether the rewrite differs from its input.
    pub fn changed(&self) -> bool {
        !self.changes.is_empty()
    }
}

/// Rewrite every line ending in `source` to LF.
///
/// Total and context-free: it reads no parse, takes no options, and cannot
/// fail. Every `\r` is a CommonMark line ending, so `\r\n` and a lone `\r`
/// each become one `\n` and every other byte is copied.
pub fn to_lf(source: &str) -> LineEndings {
    let bytes = source.as_bytes();
    let idx = LineIndex::new(source);
    let mut output = String::with_capacity(source.len());
    let mut changes = Vec::new();
    let mut cursor = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'\r' {
            i += 1;
            continue;
        }
        let crlf = bytes.get(i + 1) == Some(&b'\n');
        let (line, column) = idx.position_of(i);
        changes.push(EndingChange {
            line,
            column,
            start: i,
            old: if crlf { "\r\n" } else { "\r" },
        });
        output.push_str(&source[cursor..i]);
        output.push('\n');
        i += if crlf { 2 } else { 1 };
        cursor = i;
    }
    output.push_str(&source[cursor..]);
    LineEndings { output, changes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_becomes_lf() {
        let e = to_lf("# H\r\n\r\nbody\r\n");
        assert_eq!(e.output, "# H\n\nbody\n");
        assert_eq!(e.changes.len(), 3);
        assert!(e.changed());
    }

    #[test]
    fn a_lone_cr_becomes_lf_too() {
        // A lone `\r` is a CommonMark line ending like any other, so leaving it
        // would let the output mix two endings just as surely as CRLF would.
        let e = to_lf("a\r## H\rbody\r");
        assert_eq!(e.output, "a\n## H\nbody\n");
        assert_eq!(
            e.changes.iter().map(|c| c.old).collect::<Vec<_>>(),
            ["\r"; 3]
        );
    }

    #[test]
    fn an_lf_only_document_is_untouched() {
        for src in ["", "# H\n\nbody\n", "no trailing newline", "\n\n   \n"] {
            let e = to_lf(src);
            assert_eq!(e.output, src);
            assert!(!e.changed());
        }
    }

    #[test]
    fn a_mixed_document_is_reported_ending_by_ending() {
        // The shape the rule exists for. Coordinates address the source: the
        // `\r\n` ends line 2 at column 5, the lone `\r` ends line 3 at column 3.
        let e = to_lf("lf\ncrlf\r\ncr\rend\n");
        assert_eq!(e.output, "lf\ncrlf\ncr\nend\n");
        assert_eq!(
            e.changes
                .iter()
                .map(|c| (c.line, c.column, c.old))
                .collect::<Vec<_>>(),
            vec![(2, 5, "\r\n"), (3, 3, "\r")]
        );
    }

    #[test]
    fn the_output_holds_no_carriage_return_and_the_rewrite_is_a_fixpoint() {
        for src in ["a\r\nb\r", "\r", "\r\n", "a\r\r\nb", "x\n\ry\r\n"] {
            let once = to_lf(src).output;
            assert!(!once.contains('\r'), "{src:?} left a CR in {once:?}");
            assert_eq!(to_lf(&once).output, once, "{src:?} is not a fixpoint");
        }
    }
}
