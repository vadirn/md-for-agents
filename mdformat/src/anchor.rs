//! Corrects the cell offsets comrak reports for table rows.
//!
//! comrak fixes a table's opening offset once, when it recognises the header
//! row, then adds that offset to every later row's cells. Rows after the first
//! are skewed by the header line's own indent, which this module subtracts.

use comrak::nodes::{AstNode, NodeValue};

use crate::span::LineIndex;
use crate::table::line_content_range;

/// Byte offset within one line's content of the first byte a table row can
/// open on: the length of the leading run of `' '`, `'\t'` and `'>'`.
///
/// Reproduces comrak's `parser.first_nonspace` for a line a table row sits on.
fn row_offset(content: &str) -> usize {
    content
        .bytes()
        .take_while(|&b| matches!(b, b' ' | b'\t' | b'>'))
        .count()
}

/// The column `carried` should have had, or `None` when it cannot be one of
/// comrak's anchored columns.
///
/// Every anchored column is `anchor + offset` for an offset comrak measured
/// from the row's own content, so subtracting the anchor recovers the offset and
/// adding the line's true opening recovers the column. A column *below* the
/// anchor was not built that way and is left alone rather than wrapped into a
/// wrong one.
fn repaired(idx: &LineIndex, line: usize, anchor: usize, carried: usize) -> Option<usize> {
    let offset = carried.checked_sub(anchor)?;
    let (start, end) = line_content_range(idx, line)?;
    Some(row_offset(&idx.source()[start..end]) + 1 + offset)
}

/// Re-anchor one column in place, when it is one comrak anchored on a line
/// after `open_line`.
///
/// A column on the opening line itself is left alone: that is the anchor's own
/// line, where the offset in front of the header really does sit.
fn reanchor(idx: &LineIndex, anchor: usize, open_line: usize, line: usize, column: &mut usize) {
    if line <= open_line {
        return;
    }
    if let Some(c) = repaired(idx, line, anchor, *column) {
        *column = c;
    }
}

/// Re-anchor every table column comrak carried from its table's opening line
/// onto a line that opens somewhere else.
///
/// `root` must be the result of parsing `source`. A table whose every line opens
/// at the same offset — which is nearly all of them — comes back unchanged,
/// because the recovered anchor is then the carried one. A column on the
/// table's own opening line is left alone: that is where the carried offset
/// really does sit.
pub fn repair_table_columns<'a>(root: &'a AstNode<'a>, source: &str) {
    let idx = LineIndex::new(source);
    for table in root.descendants() {
        let (open_line, anchor) = {
            let data = table.data.borrow();
            match data.value {
                NodeValue::Table(_) => (data.sourcepos.start.line, data.sourcepos.start.column),
                _ => continue,
            }
        };
        for node in table.descendants() {
            let mut data = node.data.borrow_mut();
            // A table's and a row's own `end` come from the length of the line
            // they land on, not from the anchor, so they never carried it.
            let end_is_line_measured =
                matches!(data.value, NodeValue::Table(_) | NodeValue::TableRow(_));
            let sp = &mut data.sourcepos;
            reanchor(&idx, anchor, open_line, sp.start.line, &mut sp.start.column);
            if !end_is_line_measured {
                reanchor(&idx, anchor, open_line, sp.end.line, &mut sp.end.column);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use comrak::Arena;
    use comrak::nodes::Sourcepos;

    use super::*;
    use crate::print::block_kind;

    fn opts() -> mdstruct::Options {
        mdstruct::Options::default()
    }

    /// Every `tableCell`'s sourcepos, resolved through [`LineIndex::byte_span`]
    /// to the bytes it names. A cell whose position does not name a byte range
    /// resolves to `"<ERR>"` rather than panicking, so a specimen that errors
    /// today and a specimen that silently misattributes are compared the same
    /// way.
    fn cells(source: &str) -> Vec<String> {
        let arena = Arena::new();
        let idx = LineIndex::new(source);
        crate::parse_with(&arena, source, &opts(), |root| {
            root.descendants()
                .filter_map(|n| {
                    let d = n.data.borrow();
                    if !matches!(d.value, NodeValue::TableCell) {
                        return None;
                    }
                    Some(match idx.byte_span("tableCell", d.sourcepos) {
                        Ok((s, e)) => source[s..e].to_string(),
                        Err(_) => "<ERR>".to_string(),
                    })
                })
                .collect()
        })
    }

    /// Every node's kind and sourcepos, root excluded.
    fn positions(source: &str) -> Vec<(&'static str, Sourcepos)> {
        let arena = Arena::new();
        crate::parse_with(&arena, source, &opts(), |root| {
            root.descendants()
                .skip(1)
                .map(|n| {
                    let d = n.data.borrow();
                    (block_kind(&d.value), d.sourcepos)
                })
                .collect()
        })
    }

    #[test]
    fn row_offset_counts_the_prefix_a_row_can_open_after() {
        assert_eq!(row_offset("|a|b|"), 0);
        assert_eq!(row_offset("   |a|b|"), 3);
        assert_eq!(row_offset("> |1|2|"), 2);
        assert_eq!(row_offset(">    |1|2|"), 5);
        assert_eq!(row_offset("> > |1|"), 4);
        assert_eq!(row_offset(">\t|1|"), 2);
        // A byte order mark is not prefix: it belongs to the anchor's own line,
        // which this never runs on.
        assert_eq!(row_offset("\u{feff}|a|"), 0);
    }

    #[test]
    fn a_column_below_the_anchor_is_not_one_comrak_anchored() {
        let idx = LineIndex::new("   |a|b|\n   |-|-|\n|1|2|\n");
        // anchor 4, carried 3: no offset can produce it, so nothing is guessed.
        assert_eq!(repaired(&idx, 3, 4, 3), None);
        // anchor 4, carried 5 on a line with no indent: offset 1, column 2.
        assert_eq!(repaired(&idx, 3, 4, 5), Some(2));
        assert_eq!(repaired(&idx, 99, 4, 5), None);
    }

    #[test]
    fn every_cell_resolves_to_its_own_bytes() {
        // (name, source, the text of each cell in document order)
        let specimens: &[(&str, &str, &[&str])] = &[
            // The reproducer: a lazy row carrying no indent. Its second cell is
            // comrak's autocompleted phantom, which has no source of its own
            // and lands on the line ending — the same place it lands in the
            // unindented document below.
            (
                "lazy-row-at-eof",
                "   |a|b|\n   |-|-|\n   |1|2|\npara\n",
                &["a", "b", "1", "2", "para", "\n"],
            ),
            (
                "lazy-row-at-eof-unindented",
                "|a|b|\n|-|-|\n|1|2|\npara\n",
                &["a", "b", "1", "2", "para", "\n"],
            ),
            ("lazy-row", " |a|b|\n |-|-|\n|1|2|\n", &["a", "b", "1", "2"]),
            (
                "row-deeper-than-the-header",
                " |a|b|\n |-|-|\n   |1|2|\n",
                &["a", "b", "1", "2"],
            ),
            (
                "a-lazy-row-among-indented-ones",
                "   |a|b|\n   |-|-|\n|1|2|\n   |3|4|\n",
                &["a", "b", "1", "2", "3", "4"],
            ),
            (
                "byte-order-mark",
                "\u{feff}|a|b|\n|-|-|\n|1|2|\n",
                &["a", "b", "1", "2"],
            ),
            (
                "byte-order-mark-and-indent",
                "\u{feff}  |a|b|\n  |-|-|\n  |1|2|\n",
                &["a", "b", "1", "2"],
            ),
            (
                "block-quote-shallower-row",
                ">    |a|b|\n>    |-|-|\n> |1|2|\n",
                &["a", "b", "1", "2"],
            ),
            (
                "list-item-deeper-row",
                "- |a|b|\n  |-|-|\n     |1|2|\n",
                &["a", "b", "1", "2"],
            ),
            (
                "header-on-a-paragraphs-last-line",
                "intro\n   |a|b|\n   |-|-|\n|1|2|\n",
                &["a", "b", "1", "2"],
            ),
            // Controls: uniform opening offsets, where comrak was already right
            // and the repair must be the identity.
            ("uniform", "|a|b|\n|-|-|\n|1|2|\n", &["a", "b", "1", "2"]),
            (
                "uniform-block-quote",
                "> |a|b|\n> |-|-|\n> |1|2|\n",
                &["a", "b", "1", "2"],
            ),
        ];
        for (name, source, want) in specimens {
            assert_eq!(&cells(source), want, "{name}");
        }
    }

    #[test]
    fn the_opening_line_and_the_line_measured_ends_are_left_alone() {
        /// (name, source, every node's kind and position, root excluded).
        type Specimen<'a> = (&'a str, &'a str, &'a [(&'a str, Sourcepos)]);
        let specimens: &[Specimen] = &[
            (
                // Header opens at column 2, its body row three columns deeper.
                "row-deeper-than-the-header",
                " |a|b|\n |-|-|\n   |1|2|\n",
                &[
                    ("table", Sourcepos::from((1, 2, 3, 8))),
                    ("tableRow", Sourcepos::from((1, 2, 1, 6))),
                    ("tableCell", Sourcepos::from((1, 3, 1, 3))),
                    ("inline", Sourcepos::from((1, 3, 1, 3))),
                    ("tableCell", Sourcepos::from((1, 5, 1, 5))),
                    ("inline", Sourcepos::from((1, 5, 1, 5))),
                    ("tableRow", Sourcepos::from((3, 4, 3, 8))),
                    ("tableCell", Sourcepos::from((3, 5, 3, 5))),
                    ("inline", Sourcepos::from((3, 5, 3, 5))),
                    ("tableCell", Sourcepos::from((3, 7, 3, 7))),
                    ("inline", Sourcepos::from((3, 7, 3, 7))),
                ],
            ),
            (
                // The mark's three bytes are on line 1 and comrak counts them
                // there correctly, so line 1's columns are 3 higher than the
                // unmarked document's and every later line's are identical.
                "byte-order-mark",
                "\u{feff}|a|b|\n|-|-|\n|1|2|\n",
                &[
                    ("table", Sourcepos::from((1, 4, 3, 5))),
                    ("tableRow", Sourcepos::from((1, 4, 1, 8))),
                    ("tableCell", Sourcepos::from((1, 5, 1, 5))),
                    ("inline", Sourcepos::from((1, 5, 1, 5))),
                    ("tableCell", Sourcepos::from((1, 7, 1, 7))),
                    ("inline", Sourcepos::from((1, 7, 1, 7))),
                    ("tableRow", Sourcepos::from((3, 1, 3, 5))),
                    ("tableCell", Sourcepos::from((3, 2, 3, 2))),
                    ("inline", Sourcepos::from((3, 2, 3, 2))),
                    ("tableCell", Sourcepos::from((3, 4, 3, 4))),
                    ("inline", Sourcepos::from((3, 4, 3, 4))),
                ],
            ),
        ];
        for (name, source, want) in specimens {
            assert_eq!(&positions(source), want, "{name}");
        }
    }
}
