//! Deliberately wrong padders the structural oracle must reject.

use mdformat::{Structure, pad, structure_of};

fn opts() -> mdstruct::Options {
    mdstruct::Options::default()
}

fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("fixtures are UTF-8")
}

fn structure(source: &str) -> Structure {
    structure_of(source, &opts())
}

fn correct(source: &str) -> String {
    pad(source, &opts())
        .expect("spans convert")
        .accepted()
        .expect("the real padder must clear its own guards")
        .to_string()
}

fn rejected_by_the_table_signature(source: &str, wrong: &str) -> mdformat::StructureDiff {
    assert_eq!(
        structure(source).diff(&structure(&correct(source))),
        None,
        "the control must pass, or this test is not isolating the defect"
    );
    let diff = structure(source)
        .diff(&structure(wrong))
        .expect("the oracle was expected to reject this padder");
    assert!(
        !diff.tables_same,
        "the table signature must be what rejects it, got {diff}"
    );
    diff
}

#[test]
fn a_padder_that_rebuilds_a_cell_from_its_words_is_rejected() {
    let src = utf8(b"| key    name | value |\n| --- | --- |\n| a | b |\n");
    let wrong = utf8(b"| key name | value |\n| -------- | ----- |\n| a        | b     |\n");

    let diff = rejected_by_the_table_signature(src, wrong);
    assert!(
        !diff.html_same,
        "a cell's rendered text changed, so HTML must object too"
    );
}

#[test]
fn a_padder_that_synthesizes_a_cell_on_a_short_row_is_rejected() {
    let src = utf8(b"| a | b | c |\n| --- | --- | --- |\n| 1 | 2 |\n");
    let wrong = utf8(b"| a   | b   | c   |\n| --- | --- | --- |\n| 1   | 2   |     |\n");

    let diff = rejected_by_the_table_signature(src, wrong);
    assert!(
        diff.kinds_same && diff.rich_same && diff.html_same,
        "the tree signatures were expected to be fooled, got {diff}"
    );
}

#[test]
fn the_tree_signatures_are_jointly_blind_to_a_synthesized_cell() {
    let short = utf8(b"| a | b | c |\n| --- | --- | --- |\n| 1 | 2 |\n");
    let filled = utf8(b"| a | b | c |\n| --- | --- | --- |\n| 1 | 2 |  |\n");
    let (s, f) = (structure(short), structure(filled));
    assert_eq!(s.kinds, f.kinds);
    assert_eq!(s.rich, f.rich);
    assert_eq!(s.html, f.html);
    assert_ne!(
        s.tables, f.tables,
        "the source-derived signature must be the one that separates them"
    );
}

#[test]
fn a_padder_that_drops_a_long_rows_overflow_is_rejected() {
    let src = utf8(b"| a | b |\n| --- | --- |\n| 1 | 2 | 3 |\n");
    let wrong = utf8(b"| a   | b   |\n| --- | --- |\n| 1   | 2   |\n");

    let diff = rejected_by_the_table_signature(src, wrong);
    assert!(
        diff.kinds_same && diff.rich_same && diff.html_same,
        "content was deleted and the tree signatures were expected to be fooled, got {diff}"
    );
}

#[test]
fn a_padder_that_forgets_the_alignment_markers_is_rejected() {
    let src = utf8(b"| a | b | c |\n| :-- | --: | :-: |\n| xxxx | yyyy | zzzz |\n");
    let wrong = utf8(b"| a    | b    | c    |\n| ---- | ---- | ---- |\n| xxxx | yyyy | zzzz |\n");

    let diff = rejected_by_the_table_signature(src, wrong);
    assert!(
        !diff.rich_same && !diff.html_same,
        "dropping an alignment changes the parse and the render, got {diff}"
    );
}

#[test]
fn widening_a_delimiter_rows_dashes_is_not_a_structural_difference() {
    let narrow = utf8(b"| a | b |\n| :-- | --: |\n| longer | x |\n");
    let wide = utf8(b"| a | b |\n| :--------- | ------: |\n| longer | x |\n");
    assert_eq!(
        structure(narrow).diff(&structure(wide)),
        None,
        "the dash count is content this rewrite is defined to change"
    );
}

#[test]
fn losing_a_delimiter_cell_is_still_a_structural_difference() {
    let src = utf8(b"| a | b |\n| --- | --- |\n| 1 | 2 |\n");
    let wrong = utf8(b"| a | b |\n| ------- |\n| 1 | 2 |\n");
    let diff = structure(src)
        .diff(&structure(wrong))
        .expect("the oracle was expected to reject this");
    assert!(!diff.tables_same, "got {diff}");
}
