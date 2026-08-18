//! Opt-in rewrite of the whitespace between top-level blocks, gated by re-parse
//! structural equivalence.

use crate::print::{Block, PartitionReport, block_spans, check_partition, is_ws};
use crate::span::{LineIndex, PosError};
use crate::structure::{StructureDiff, structure_of};

/// One gap the rewrite would change, for reporting. `old` is what the source
/// holds there; `new` is the normal form's separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapChange {
    /// 1-based line the gap starts on.
    pub line: usize,
    /// Byte offset the gap starts at.
    pub start: usize,
    /// Kind of the block before the gap; `"<bof>"` at the head of the file.
    pub prev: &'static str,
    /// Kind of the block after the gap; `"<eof>"` at the tail.
    pub next: &'static str,
    pub old: String,
    pub new: &'static str,
}

/// A candidate normalization and everything needed to decide whether to take
/// it. Construct with [`normalize`]; read the bytes with
/// [`Normalization::accepted`].
#[derive(Debug, Clone)]
pub struct Normalization {
    /// The input's partition verdict. When this fails, no rewrite is attempted
    /// and `output` is the source unchanged — the gap definition is unsound
    /// without it.
    pub input_partition: PartitionReport,
    /// The candidate bytes. Present even when refused, so a caller can report
    /// *what* would have happened.
    pub output: String,
    /// Gaps examined, changed or not — head and tail included.
    pub gaps_considered: usize,
    /// Gaps the rewrite would change.
    pub gaps: Vec<GapChange>,
    /// `None` when the re-parse is structurally equivalent; otherwise why not.
    pub structure: Option<StructureDiff>,
    /// Whether the *output* still satisfies the partition oracle. Recorded, not
    /// relied on: it holds even for outputs whose parse the rewrite destroyed,
    /// which is precisely why it cannot be the guard. `None` when the output's
    /// sourcepos did not convert.
    pub output_partitions: Option<bool>,
}

impl Normalization {
    /// Whether the candidate differs from the input at all.
    pub fn changed(&self) -> bool {
        !self.gaps.is_empty()
    }

    /// The normalized bytes, or `None` when they must not be used: the input
    /// failed the partition, or the rewrite changed the parse. This is the only
    /// accessor that clears the guard, so a caller cannot take the bytes
    /// without it.
    pub fn accepted(&self) -> Option<&str> {
        (self.input_partition.is_partition() && self.structure.is_none()).then_some(&*self.output)
    }
}

/// The separator the normal form puts *before* a block, as a function of the
/// preceding block's kind (`None` at the head of the file).
fn separator(prev: Option<&str>) -> &'static str {
    match prev {
        // The head of the file, and the bytes after a BOM, which comrak counts
        // in its columns but assigns to no node.
        None | Some("bom") => "",
        // Front matter takes one blank line like any other block. The arm is
        // written out rather than folded into the fallback because this is the
        // one place someone would reinstate the withdrawn `"\n"` — no blank
        // line after front matter — and doing so rewrites nearly every file
        // that has one, for a cosmetic preference.
        Some("frontmatter") => "\n\n",
        Some(_) => "\n\n",
    }
}

/// Whitespace that can sit *within* a line: space, tab, form feed. Excludes the
/// line endings, which is what makes "extend to this line's start/end" a
/// single-line operation.
fn is_inline_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == 0x0c
}

/// A block's content span: raw span trimmed to content, then extended to its
/// first line's start and its last line's content end where those bytes are
/// blank. `floor` and `ceil` clamp the extensions to the neighbouring blocks so
/// this can never manufacture an overlap (a BOM block, whose successor starts
/// on the same line, is the case that needs it).
///
/// `None` for a span holding no content byte; its bytes fall into the
/// surrounding gap.
fn content_span(
    source: &str,
    idx: &LineIndex,
    block: &Block,
    floor: usize,
    ceil: usize,
) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let end = block.end.min(source.len());
    let start = block.start.min(end);
    let first = (start..end).find(|&i| !is_ws(bytes[i]))?;
    let last = (start..end).rev().find(|&i| !is_ws(bytes[i]))? + 1;

    let line_start = idx
        .line_start(idx.position_of(first).0)
        .filter(|&ls| ls >= floor && source[ls..first].bytes().all(is_inline_ws))
        .unwrap_or(first);

    let mut line_end = idx
        .line_range(idx.position_of(last - 1).0)
        .map(|(_, e)| e)
        .unwrap_or(last);
    while line_end > last && matches!(bytes[line_end - 1], b'\n' | b'\r') {
        line_end -= 1;
    }
    if line_end > ceil || !source[last..line_end].bytes().all(is_inline_ws) {
        line_end = last;
    }

    Some((line_start, line_end))
}

/// The content spans of `blocks`, in source order, dropping any block with no
/// content byte.
fn content_spans(source: &str, blocks: &[Block]) -> Vec<(usize, usize, &'static str)> {
    let idx = LineIndex::new(source);
    let mut spans = Vec::with_capacity(blocks.len());
    let mut floor = 0usize;
    for (i, block) in blocks.iter().enumerate() {
        let ceil = blocks
            .get(i + 1)
            .map(|next| next.start)
            .unwrap_or(source.len());
        if let Some((start, end)) = content_span(source, &idx, block, floor, ceil) {
            spans.push((start, end, block.kind));
            floor = end;
        }
    }
    spans
}

/// Emit `separator + source[content span]` for each block in order. Every byte
/// inside a content span is copied verbatim; only the separators are
/// synthesized.
fn rewrite(source: &str, blocks: &[Block]) -> (String, Vec<GapChange>, usize) {
    let idx = LineIndex::new(source);
    let mut out = String::with_capacity(source.len());
    let mut gaps = Vec::new();
    let mut considered = 0usize;
    let mut cursor = 0usize;
    let mut prev: Option<&'static str> = None;

    let record =
        |gaps: &mut Vec<GapChange>, at: usize, old: &str, new: &'static str, prev, next| {
            if old != new {
                gaps.push(GapChange {
                    line: idx.position_of(at).0,
                    start: at,
                    prev,
                    next,
                    old: old.to_string(),
                    new,
                });
            }
        };

    for &(start, end, kind) in &content_spans(source, blocks) {
        let new = separator(prev);
        considered += 1;
        record(
            &mut gaps,
            cursor,
            &source[cursor..start],
            new,
            prev.unwrap_or("<bof>"),
            kind,
        );
        out.push_str(new);
        out.push_str(&source[start..end]);
        cursor = end;
        prev = Some(kind);
    }

    // The tail. A file with no block at all — empty, or whitespace only — gets
    // `""`, so a 0-byte file stays 0 bytes rather than gaining a newline.
    let new = if prev.is_some() { "\n" } else { "" };
    considered += 1;
    record(
        &mut gaps,
        cursor,
        &source[cursor..],
        new,
        prev.unwrap_or("<bof>"),
        "<eof>",
    );
    out.push_str(new);

    (out, gaps, considered)
}

/// Compute the blank-line normal form of `source` and check it.
///
/// Writes nothing and decides nothing: the result carries the candidate bytes,
/// the gaps that would change, and the guard's verdict.
/// [`Normalization::accepted`] is the only way to get bytes that cleared it.
///
/// `Err` carries every sourcepos that does not name a byte range in `source`,
/// exactly as [`crate::partition`] does.
pub fn normalize(source: &str, opts: &mdstruct::Options) -> Result<Normalization, Vec<PosError>> {
    let arena = comrak::Arena::new();
    let blocks = crate::parse_with(&arena, source, opts, |root| block_spans(root, source))?;
    let input_partition = check_partition(source, &blocks);

    // Without the partition the gap is not definable: an unclaimed content byte
    // would sit between two content spans and be deleted as if it were
    // whitespace. Refuse rather than guess.
    if !input_partition.is_partition() {
        return Ok(Normalization {
            input_partition,
            output: source.to_string(),
            gaps_considered: 0,
            gaps: Vec::new(),
            structure: None,
            output_partitions: None,
        });
    }

    let (output, gaps, gaps_considered) = rewrite(source, &blocks);
    let structure = structure_of(source, opts).diff(&structure_of(&output, opts));

    let out_arena = comrak::Arena::new();
    let output_partitions = crate::parse_with(&out_arena, &output, opts, |root| {
        block_spans(root, &output)
            .map(|b| check_partition(&output, &b).is_partition())
            .ok()
    });

    Ok(Normalization {
        input_partition,
        output,
        gaps_considered,
        gaps,
        structure,
        output_partitions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(source: &str) -> Normalization {
        normalize(source, &mdstruct::Options::default()).expect("spans convert")
    }

    #[test]
    fn one_blank_line_between_top_level_blocks() {
        let n = norm("# H\npara\n\n\n\nmore\n");
        assert_eq!(n.accepted(), Some("# H\n\npara\n\nmore\n"));
    }

    #[test]
    fn an_empty_file_stays_empty() {
        let n = norm("");
        assert_eq!(n.accepted(), Some(""));
        assert!(!n.changed());
    }

    #[test]
    fn a_whitespace_only_file_becomes_empty() {
        // No block to anchor a tail newline on, so the tail rule emits "".
        assert_eq!(norm("\n\n   \n").accepted(), Some(""));
    }

    #[test]
    fn a_bom_is_not_followed_by_a_blank_line() {
        let n = norm("\u{feff}# H\n");
        assert_eq!(n.accepted(), Some("\u{feff}# H\n"));
        assert!(!n.changed());
    }

    #[test]
    fn the_separator_before_the_first_block_is_empty() {
        assert_eq!(norm("\n\n# H\n").accepted(), Some("# H\n"));
    }
}
