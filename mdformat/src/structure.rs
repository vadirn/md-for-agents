//! Re-parse structural equivalence: the oracle gating every rewrite.
//!
//! Compares the parse of a rewrite's input against the parse of its output on
//! node kinds, node attributes, rendered HTML and table source shape.

use comrak::nodes::{AstNode, ListDelimType, NodeList, NodeValue};

use crate::print::block_kind;
use crate::span::LineIndex;
use crate::table::{line_content_range, split_unescaped_pipes};

/// Five renderings of one parse: block kinds with nesting, the same walk with
/// every node attribute, the rendered HTML, each table's source shape, and
/// every list's marker. Compare with [`Structure::diff`], or with
/// [`Structure::diff_ignoring_markers`] from the one rule that rewrites a
/// marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    /// Pre-order walk of block nodes: `"  "*depth + kind`.
    pub kinds: Vec<String>,
    /// The same walk, with each node's full `NodeValue` debug rendering —
    /// **less** the two marker fields, which live in `markers`.
    pub rich: Vec<String>,
    /// `comrak::format_html` output.
    pub html: String,
    /// Per table, the source shape of its rows and delimiter — the one
    /// signature the tree cannot supply.
    pub tables: Vec<String>,
    /// Per list and list item, in the same pre-order walk: the bullet
    /// character and the ordered-list delimiter. Held apart so
    /// [`Structure::diff_ignoring_markers`] can name the exemption.
    pub markers: Vec<String>,
}

/// How two [`Structure`]s differ, with the first differing entry in context so
/// a failure names the construct rather than the offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureDiff {
    pub kinds_same: bool,
    pub rich_same: bool,
    pub html_same: bool,
    pub tables_same: bool,
    /// Whether every list's bullet character and ordered delimiter survived.
    /// Reported honestly even by [`Structure::diff_ignoring_markers`], which
    /// only declines to *gate* on it.
    pub markers_same: bool,
    /// Index of the first differing `rich` entry, when there is one.
    pub at: Option<usize>,
    /// The first differing entry before the rewrite, in context.
    pub before: String,
    /// The same window after the rewrite.
    pub after: String,
}

impl StructureDiff {
    /// Whether any signature other than `markers` differs — the condition
    /// [`Structure::diff_ignoring_markers`] gates on.
    fn beyond_markers(&self) -> bool {
        !self.kinds_same || !self.rich_same || !self.html_same || !self.tables_same
    }

    /// Whether any of the five differ.
    fn any(&self) -> bool {
        self.beyond_markers() || !self.markers_same
    }
}

impl std::fmt::Display for StructureDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "kinds_same={} rich_same={} html_same={} tables_same={} markers_same={}",
            self.kinds_same, self.rich_same, self.html_same, self.tables_same, self.markers_same
        )?;
        if let Some(at) = self.at {
            write!(
                f,
                "; first difference at block node {at}: before {:?}, after {:?}",
                self.before, self.after
            )?;
        } else if !self.tables_same || !self.markers_same {
            write!(
                f,
                "; first source-read difference: before {:?}, after {:?}",
                self.before, self.after
            )?;
        }
        Ok(())
    }
}

impl Structure {
    /// `None` when the two parses are structurally equivalent on all five
    /// signatures; otherwise which comparison failed, and where. This is the
    /// oracle for every rewrite that is **not** entitled to change a list
    /// marker, which is all of them but one.
    pub fn diff(&self, other: &Structure) -> Option<StructureDiff> {
        let d = self.compare(other);
        d.any().then_some(d)
    }

    /// The same comparison with `markers` reported but not gated on — the
    /// oracle for [`crate::markers`], the one rewrite whose whole content is
    /// to change a bullet character or an ordered delimiter.
    ///
    /// It is not a weaker oracle for the merge hazard that rule carries:
    /// splicing two lists into one changes `kinds`, which this still compares.
    pub fn diff_ignoring_markers(&self, other: &Structure) -> Option<StructureDiff> {
        let d = self.compare(other);
        d.beyond_markers().then_some(d)
    }

    /// Every comparison, always run and always reported. The two public
    /// accessors differ only in which subset they gate on, so neither can see
    /// a signature the other cannot.
    fn compare(&self, other: &Structure) -> StructureDiff {
        let kinds_same = self.kinds == other.kinds;
        let rich_same = self.rich == other.rich;
        let html_same = self.html == other.html;
        let tables_same = self.tables == other.tables;
        let markers_same = self.markers == other.markers;
        let window = |v: &[String], i: usize| {
            let lo = i.saturating_sub(1);
            let hi = (i + 2).min(v.len());
            v.get(lo..hi).unwrap_or(&[]).join(" | ")
        };
        let at = (0..self.rich.len().max(other.rich.len()))
            .find(|&i| self.rich.get(i) != other.rich.get(i));
        // When only a source-read signature differs there is no differing block
        // node to point at, so the context window comes from that signature
        // instead — otherwise the one failure mode it exists for would report
        // no location at all.
        let first_difference = |a: &[String], b: &[String]| {
            let j = (0..a.len().max(b.len()))
                .find(|&i| a.get(i) != b.get(i))
                .unwrap_or(0);
            (window(a, j), window(b, j))
        };
        let (before, after) = match at {
            Some(i) => (window(&self.rich, i), window(&other.rich, i)),
            None if !tables_same => first_difference(&self.tables, &other.tables),
            None if !markers_same => first_difference(&self.markers, &other.markers),
            None => (String::new(), String::new()),
        };
        StructureDiff {
            kinds_same,
            rich_same,
            html_same,
            tables_same,
            markers_same,
            at,
            before,
            after,
        }
    }
}

/// Parse `source` under the shared comrak configuration and take its five
/// signatures.
pub fn structure_of(source: &str, opts: &mdstruct::Options) -> Structure {
    let arena = comrak::Arena::new();
    crate::parse_with(&arena, source, opts, |root| {
        let mut kinds = Vec::new();
        let mut rich = Vec::new();
        let mut markers = Vec::new();
        walk(root, 0, &mut kinds, &mut rich, &mut markers);
        let mut html = String::new();
        let options = crate::comrak_options(opts);
        comrak::format_html(root, &options, &mut html)
            .expect("formatting into a String cannot fail");
        let tables = table_shapes(root, source);
        Structure {
            kinds,
            rich,
            html,
            tables,
            markers,
        }
    })
}

/// The source shape of every table in `root`, in document order.
///
/// Read from `source`, not from the tree, because the shape this covers has no
/// tree representation. Rows are addressed through their own `sourcepos`, so
/// the signature survives a rewrite that moves a table without editing it.
fn table_shapes<'a>(root: &'a AstNode<'a>, source: &str) -> Vec<String> {
    let idx = LineIndex::new(source);
    let line = |l: usize| line_content_range(&idx, l).map(|(s, e)| &source[s..e]);
    let mut out = Vec::new();
    for node in root.descendants() {
        let alignments = match &node.data.borrow().value {
            NodeValue::Table(t) => t.alignments.clone(),
            _ => continue,
        };
        out.push(format!(
            "table cols={} align={alignments:?}",
            alignments.len()
        ));
        let mut header_end: Option<usize> = None;
        for row in node.children() {
            let sp = row.data.borrow().sourcepos;
            if header_end.is_none() {
                header_end = Some(sp.end.line);
            }
            let segments: Vec<&str> = match line(sp.start.line) {
                Some(text) => split_unescaped_pipes(text)
                    .into_iter()
                    .map(|s| s.trim_matches(' '))
                    .collect(),
                None => vec!["<line out of range>"],
            };
            out.push(format!("  row {segments:?}"));
        }
        let delimiter = header_end.and_then(|l| line(l + 1));
        out.push(match delimiter {
            // Only the colons and the cell count, never the dash run: the dash
            // run is exactly what table padding rewrites.
            Some(text) => format!(
                "  delim {:?}",
                text.split('|')
                    .map(|seg| {
                        let t = seg.trim();
                        (
                            t.starts_with(':'),
                            t.len() > 1 && t.ends_with(':'),
                            t.is_empty(),
                        )
                    })
                    .collect::<Vec<_>>()
            ),
            None => "  delim <missing>".to_string(),
        });
    }
    out
}

/// Pre-order walk of the block skeleton. Inline subtrees are skipped whole —
/// a block-level rewrite never reaches inside one, and the HTML signature is
/// what covers inline text.
fn walk<'a>(
    node: &'a AstNode<'a>,
    depth: usize,
    kinds: &mut Vec<String>,
    rich: &mut Vec<String>,
    markers: &mut Vec<String>,
) {
    for child in node.children() {
        {
            let data = child.data.borrow();
            // `FrontMatter` reports `block() == false` in comrak 0.53 despite
            // being a root child, so it has to be admitted by name or the whole
            // front-matter clause would be invisible to this oracle.
            if !data.value.block() && !matches!(data.value, NodeValue::FrontMatter(_)) {
                continue;
            }
            let pad = "  ".repeat(depth);
            kinds.push(format!("{pad}{}", block_kind(&data.value)));
            rich.push(format!("{pad}{}", normalized_debug(&data.value)));
            if let Some(sig) = marker_signature(&data.value) {
                markers.push(format!("{pad}{sig}"));
            }
        }
        walk(child, depth + 1, kinds, rich, markers);
    }
}

/// A node's `Debug`, less the parts a rewrite is allowed to move: the two
/// literal trims, the task item's position, and the two list marker fields,
/// which move to [`Structure::markers`] rather than being dropped.
fn normalized_debug(value: &NodeValue) -> String {
    match value {
        NodeValue::FrontMatter(literal) => format!("FrontMatter({:?})", literal.trim_end()),
        NodeValue::HtmlBlock(html) => format!("HtmlBlock({:?})", html.literal.trim_end()),
        NodeValue::TaskItem(task) => format!("TaskItem({:?})", task.symbol),
        NodeValue::List(list) => format!("List({:?})", marker_blind(*list)),
        NodeValue::Item(list) => format!("Item({:?})", marker_blind(*list)),
        other => format!("{other:?}"),
    }
}

/// One `NodeList` with its two marker fields zeroed, so that `rich` says
/// everything about a list except which character introduces it.
fn marker_blind(mut list: NodeList) -> NodeList {
    list.bullet_char = 0;
    list.delimiter = ListDelimType::Period;
    list
}

/// The marker entry for a list or list item, and `None` for every other node.
///
/// A task item carries no `NodeList` of its own — comrak replaces the `Item`
/// with a `TaskItem` — so its marker is read off the enclosing `List`, which is
/// where the bullet character is decided anyway.
fn marker_signature(value: &NodeValue) -> Option<String> {
    let (kind, list) = match value {
        NodeValue::List(list) => ("list", list),
        NodeValue::Item(list) => ("item", list),
        _ => return None,
    };
    Some(format!(
        "{kind} {:?} bullet={:?} delim={:?}",
        list.list_type, list.bullet_char as char, list.delimiter
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(source: &str) -> Structure {
        structure_of(source, &mdstruct::Options::default())
    }

    #[test]
    fn equal_documents_are_equivalent() {
        assert_eq!(s("# H\n\npara\n").diff(&s("# H\n\npara\n")), None);
    }

    #[test]
    fn front_matter_appears_in_the_signature_despite_not_reporting_as_a_block() {
        let st = s("---\nk: v\n---\n\nbody\n");
        assert_eq!(st.kinds, vec!["frontmatter", "paragraph"]);
    }

    #[test]
    fn front_matter_and_html_block_literals_are_right_trimmed() {
        // The blank line after front matter lives in the FrontMatter literal,
        // so without the trim every intended gap change would read as a tree
        // change and the oracle would refuse its own normal form.
        assert_eq!(
            s("---\nk: v\n---\n\nbody\n").diff(&s("---\nk: v\n---\nbody\n")),
            None
        );
    }

    #[test]
    fn a_code_block_literal_is_not_trimmed() {
        // The counterpart: code-block content is content, and losing trailing
        // whitespace from it must be a diff.
        let diff = s("```\ncode   \n```\n")
            .diff(&s("```\ncode\n```\n"))
            .expect("a changed code literal must diff");
        assert!(!diff.rich_same);
    }

    #[test]
    fn a_bullet_change_is_a_marker_difference_and_nothing_else() {
        // The split, stated as bytes: two one-item lists differing only in the
        // bullet character. `rich` must be silent about it — otherwise the
        // marker rule would decline every document it exists to change — and
        // `markers` must not be.
        let d = s("* a\n")
            .diff(&s("- a\n"))
            .expect("a changed bullet is a difference");
        assert!(d.kinds_same && d.rich_same && d.html_same && d.tables_same);
        assert!(!d.markers_same);
        // And the exemption is at the call site, not in the signature.
        assert_eq!(s("* a\n").diff_ignoring_markers(&s("- a\n")), None);
    }

    #[test]
    fn an_ordered_delimiter_change_is_a_marker_difference_and_nothing_else() {
        let d = s("1) a\n")
            .diff(&s("1. a\n"))
            .expect("a changed delimiter is a difference");
        assert!(d.kinds_same && d.rich_same && d.html_same && d.tables_same);
        assert!(!d.markers_same);
        assert_eq!(s("1) a\n").diff_ignoring_markers(&s("1. a\n")), None);
    }

    #[test]
    fn ignoring_markers_still_sees_two_lists_become_one() {
        // The hazard the marker rule carries, put past the loosened oracle: a
        // bullet change that *merges* two adjacent lists is a `kinds` change,
        // so the exemption does not reach it.
        let d = s("- a\n+ b\n")
            .diff_ignoring_markers(&s("- a\n- b\n"))
            .expect("a merge must survive the marker exemption");
        assert!(!d.kinds_same, "{d}");
    }

    #[test]
    fn a_task_items_symbol_position_is_not_structure() {
        // Same task list, one line further down: `symbol_sourcepos` shifts and
        // nothing structural has changed.
        assert_eq!(
            s("- [x] a\n")
                .rich
                .last()
                .map(|l| l.trim().to_string())
                .unwrap(),
            s("seed\n\n- [x] a\n")
                .rich
                .last()
                .map(|l| l.trim().to_string())
                .unwrap()
        );
    }
}
