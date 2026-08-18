//! Structural Markdown formatting: comrak's parser plus a sourcepos-driven
//! printer.
//!
//! A sibling to `mdstruct`, not built on it. `mdstruct` drops its comrak arena
//! before returning, and a printer needs the live AST, so this crate parses
//! again and keeps the arena alive for the duration of the walk.

use comrak::Arena;
use comrak::nodes::AstNode;

pub mod anchor;
pub mod bom;
pub mod endings;
pub mod format;
pub mod markers;
pub mod normalize;
pub mod print;
pub mod span;
pub mod structure;
pub mod table;
pub mod write;

pub use bom::BOM;
pub use endings::{EndingChange, LineEndings, to_lf};
pub use format::{
    Check, Departure, Exemption, Format, Rule, RuleRun, check, check_with, escape_whitespace,
    format, format_with, rule_named, rule_names,
};
pub use markers::{
    ListSkipReason, MarkerChange, MarkerViolation, SkippedList, Unification, marker_violation,
    unify,
};
pub use normalize::{GapChange, Normalization, normalize};
pub use print::{
    Block, PartitionReport, Violation, block_kind, block_spans, check_partition, reassemble,
};
pub use span::{LineIndex, PosError, PosReason};
pub use structure::{Structure, StructureDiff, structure_of};
pub use table::{
    LineChange, PadViolation, PadViolationKind, Padding, SkipReason, SkippedTable, pad,
};
pub use write::{Refusal, replace, target};

/// `mdformat`'s comrak parse configuration. Forwards to
/// [`mdstruct::comrak_options`] verbatim — `mdformat` has no comrak settings
/// of its own. Keeping this as a real (if thin) function, rather than callers
/// reaching for `mdstruct::comrak_options` directly, gives the crate one
/// named seam to audit and to hold `tests::comrak_options_agrees_with_mdstruct`
/// against.
pub fn comrak_options(opts: &mdstruct::Options) -> comrak::Options<'static> {
    mdstruct::comrak_options(opts)
}

/// Parse `source` into `arena` under the shared comrak configuration, and
/// hand the live root node to `f`. The arena has to outlive the callback (the
/// AST borrows from it), so this takes the shape of a scoped callback rather
/// than returning the node directly.
///
/// One correction runs between the parse and the callback:
/// [`anchor::repair_table_columns`] re-anchors the columns comrak carries from a
/// table's opening line onto its later rows, which land wherever the header
/// opened rather than where they are. It lives here, and not in one reader,
/// because two readers consume those columns — [`print::block_spans`] converts
/// them to byte ranges and [`table::pad`] slices the source at them — so a
/// correction in either alone would leave the other reading the wrong bytes.
/// That module states the measured boundary of what it touches.
pub fn parse_with<'a, R>(
    arena: &'a Arena<'a>,
    source: &str,
    opts: &mdstruct::Options,
    f: impl FnOnce(&'a AstNode<'a>) -> R,
) -> R {
    let options = comrak_options(opts);
    let root = comrak::parse_document(arena, source, &options);
    anchor::repair_table_columns(root, source);
    f(root)
}

/// One file's partition result: the block spans and the verdict on them.
#[derive(Debug, Clone)]
pub struct Partition {
    pub blocks: Vec<Block>,
    pub report: PartitionReport,
}

impl Partition {
    /// A file passes when its blocks partition its content bytes. That is the
    /// whole verdict: [`print::reassemble`] returns its input for any span set,
    /// corrupt ones included, so its output is no evidence.
    pub fn passed(&self) -> bool {
        self.report.is_partition()
    }
}

/// Parse `source` under the shared configuration, tile it with the byte range
/// of each top-level block, and report whether those ranges partition its
/// content bytes.
///
/// Runs no reassembly. [`reassemble`] returns its input for any span set, so
/// calling it here would allocate a second copy of every file to confirm a
/// tautology. A caller wanting the printer's bytes calls
/// [`reassemble`] itself.
///
/// `Err` carries every sourcepos that does not name a byte range in `source`;
/// unlike `mdstruct`, an out-of-range position is never clamped.
pub fn partition(source: &str, opts: &mdstruct::Options) -> Result<Partition, Vec<PosError>> {
    let arena = Arena::new();
    parse_with(&arena, source, opts, |root| {
        let blocks = block_spans(root, source)?;
        let report = check_partition(source, &blocks);
        Ok(Partition { blocks, report })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comrak_options_agrees_with_mdstruct() {
        let opts = mdstruct::Options::default();
        let ours = format!("{:?}", comrak_options(&opts));
        let theirs = format!("{:?}", mdstruct::comrak_options(&opts));
        assert_eq!(
            ours, theirs,
            "mdformat's comrak options must match mdstruct's exactly"
        );
    }

    #[test]
    fn partition_passes_and_accounts_for_every_content_byte() {
        let src = "# Heading\n\nSome *text* with a [[Wikilink]].\n";
        let opts = mdstruct::Options::default();
        let r = partition(src, &opts).expect("spans convert");
        assert!(r.passed(), "{:?}", r.report.violations);
        assert_eq!(r.report.content_bytes, r.report.covered_content_bytes);
    }
}
