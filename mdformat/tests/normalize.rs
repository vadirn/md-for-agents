//! Blank-line normalization: the normal form, and every hazard that decided
//! its shape.

use comrak::Arena;
use comrak::nodes::NodeValue;
use mdformat::{Normalization, block_spans, check_partition, normalize, structure_of};

fn opts() -> mdstruct::Options {
    mdstruct::Options::default()
}

fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("specimen is UTF-8")
}

fn norm(source: &str) -> Normalization {
    normalize(source, &opts()).unwrap_or_else(|e| panic!("sourcepos errors: {e:?}"))
}

fn accept(source: &str) -> String {
    let n = norm(source);
    match n.accepted() {
        Some(out) => out.to_string(),
        None => panic!(
            "expected an accepted normalization of {:?}, got {}",
            source.escape_debug(),
            n.structure
                .map(|d| d.to_string())
                .unwrap_or_else(|| "a failing input partition".into())
        ),
    }
}

fn refuse(source: &str) -> (String, mdformat::StructureDiff) {
    let n = norm(source);
    assert!(
        n.accepted().is_none(),
        "expected the guard to refuse {:?}",
        source.escape_debug()
    );
    let diff = n.structure.clone().expect("a structural difference");
    (n.output.clone(), diff)
}

// ---------------------------------------------------------------------------
// 1. The normal form
// ---------------------------------------------------------------------------

/// `(name, input, output)`. Names appear in assertion messages.
const NORMAL_FORM: &[(&str, &[u8], &[u8])] = &[
    ("already-normal", b"# H\n\npara\n", b"# H\n\npara\n"),
    ("no-blank-line", b"# H\npara\n", b"# H\n\npara\n"),
    ("two-blank-lines", b"# H\n\n\npara\n", b"# H\n\npara\n"),
    ("five-blank-lines", b"a\n\n\n\n\n\nb\n", b"a\n\nb\n"),
    // Rule 4: no trailing whitespace on an otherwise-blank line. It needs no
    // clause of its own — a gap is regenerated, not edited.
    ("blank-line-with-spaces", b"a\n   \nb\n", b"a\n\nb\n"),
    ("blank-line-with-tabs", b"a\n\t\t\nb\n", b"a\n\nb\n"),
    // Rule 3: exactly one trailing newline.
    ("no-trailing-newline", b"a", b"a\n"),
    ("three-trailing-newlines", b"a\n\n\n", b"a\n"),
    ("trailing-blank-with-spaces", b"a\n  \n", b"a\n"),
    // The head of the file is not "between two blocks" and the four rules say
    // nothing about it. This deletes it — see the front-matter promotion hazard
    // below for what that costs.
    ("leading-blank-lines", b"\n\na\n", b"a\n"),
    // An empty file has no block to hang a trailing newline on, so it stays
    // empty rather than becoming one byte.
    ("empty", b"", b""),
    ("whitespace-only", b"\n \n\t\n", b""),
    // Front matter takes one blank line like any other block.
    (
        "frontmatter-normal",
        b"---\nk: v\n---\n\nbody\n",
        b"---\nk: v\n---\n\nbody\n",
    ),
    (
        "frontmatter-tight",
        b"---\nk: v\n---\nbody\n",
        b"---\nk: v\n---\n\nbody\n",
    ),
    (
        "frontmatter-loose",
        b"---\nk: v\n---\n\n\n\nbody\n",
        b"---\nk: v\n---\n\nbody\n",
    ),
    // A BOM is a block with no node behind it, and nothing may be inserted
    // between it and the text it prefixes.
    (
        "bom",
        "\u{feff}# H\n".as_bytes(),
        "\u{feff}# H\n".as_bytes(),
    ),
    // Blank lines *inside* a top-level span are not gaps and are not governed.
    (
        "blank-lines-inside-a-fence",
        b"```\na\n\n\nb\n```\n\n\ntail\n",
        b"```\na\n\n\nb\n```\n\ntail\n",
    ),
    (
        "blank-line-inside-a-list",
        b"- a\n\n- b\n\n\ntail\n",
        b"- a\n\n- b\n\ntail\n",
    ),
];

#[test]
fn the_normal_form_is_what_it_says() {
    for (name, input, want) in NORMAL_FORM {
        let (input, want) = (utf8(input), utf8(want));
        assert_eq!(
            accept(input),
            want,
            "{name}: normalizing {:?}",
            input.escape_debug()
        );
    }
}

#[test]
fn normalization_is_idempotent() {
    for (name, input, _) in NORMAL_FORM {
        let once = accept(utf8(input));
        assert_eq!(accept(&once), once, "{name}: second pass changed the bytes");
    }
}

#[test]
fn no_content_byte_is_added_removed_or_reordered() {
    let content = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_whitespace() && *c != '\u{c}')
            .collect()
    };
    for (name, input, _) in NORMAL_FORM {
        let input = utf8(input);
        assert_eq!(
            content(&accept(input)),
            content(input),
            "{name}: content bytes changed"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. Hazards the guard must refuse
// ---------------------------------------------------------------------------

#[test]
fn deleting_head_whitespace_can_promote_a_thematic_break_into_front_matter() {
    let src = utf8(b"\n\n---\nk: v\n---\n");

    let before = structure_of(src, &opts());
    assert_eq!(before.kinds, vec!["thematicBreak", "heading"]);

    let (output, diff) = refuse(src);
    assert_eq!(output, "---\n\nk: v\n---\n");
    assert_eq!(
        structure_of(&output, &opts()).kinds,
        vec!["frontmatter"],
        "the candidate turns two rendered blocks into front matter"
    );
    assert!(!diff.kinds_same && !diff.rich_same && !diff.html_same);
}

#[test]
fn the_same_dashes_behind_a_line_indent_are_left_alone() {
    let src = utf8(b"  ---\nk: v\n---\n");
    let out = accept(src);
    assert!(
        out.starts_with("  ---"),
        "the indent must survive, got {:?}",
        out.escape_debug()
    );
    assert_eq!(
        structure_of(&out, &opts()).kinds,
        vec!["thematicBreak", "heading"]
    );
}

#[test]
fn inserting_a_blank_line_can_delete_a_link_reference_definition_from_the_render() {
    let src = utf8(b"seed\n\n[a]: /x\n| a | b |\n| - | - |\n| 1 | 2 |\n");

    let before = structure_of(src, &opts());
    assert_eq!(
        before.kinds,
        vec![
            "paragraph",
            "paragraph",
            "table",
            "  tableRow",
            "    tableCell",
            "    tableCell",
            "  tableRow",
            "    tableCell",
            "    tableCell"
        ]
    );
    assert!(
        before.html.contains("[a]: /x"),
        "the definition is rendered text before the rewrite"
    );

    let (output, diff) = refuse(src);
    assert_eq!(
        output,
        "seed\n\n[a]: /x\n\n| a | b |\n| - | - |\n| 1 | 2 |\n"
    );
    let after = structure_of(&output, &opts());
    assert!(
        !after.html.contains("[a]: /x"),
        "the definition is gone from the render after it"
    );
    assert!(!diff.kinds_same);
}

#[test]
fn an_unterminated_fence_loses_content_and_kinds_alone_would_not_notice() {
    let src = utf8(b"```\ncode\n\n\n");
    let (output, diff) = refuse(src);
    assert_eq!(output, "```\ncode\n");
    assert!(
        diff.kinds_same,
        "the block skeleton is unchanged; kinds-only would accept this"
    );
    assert!(!diff.rich_same, "the code literal loses two newlines");
    assert!(!diff.html_same);
}

#[test]
fn kinds_and_html_agree_where_the_rich_signature_does_not() {
    let indented = structure_of(utf8(b"  - item\n"), &opts());
    let flush = structure_of(utf8(b"- item\n"), &opts());
    assert_eq!(indented.kinds, flush.kinds);
    assert_eq!(indented.html, flush.html);
    assert_ne!(indented.rich, flush.rich);
    assert!(
        indented.rich[0].contains("marker_offset: 2") && flush.rich[0].contains("marker_offset: 0"),
        "expected marker_offset to be the differing attribute"
    );
}

#[test]
fn the_partition_oracle_accepts_every_refused_specimen() {
    let refused: &[&[u8]] = &[
        b"\n\n---\nk: v\n---\n",
        b"seed\n\n[a]: /x\n| a | b |\n| - | - |\n| 1 | 2 |\n",
        b"```\ncode\n\n\n",
    ];
    for src in refused {
        let src = utf8(src);
        let n = norm(src);
        assert!(n.accepted().is_none(), "{src:?} must be refused");
        assert!(
            n.input_partition.is_partition(),
            "{src:?}: the input partitions"
        );
        assert_eq!(
            n.output_partitions,
            Some(true),
            "{src:?}: the destroyed output partitions too — this is the point"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. The indented-code fix
// ---------------------------------------------------------------------------

#[test]
fn a_top_level_indented_code_block_keeps_its_indent() {
    let src = utf8(
        b"Installation caveats:\n\n    - There are a couple of daemons.\n\nInstall it first.\n",
    );

    let arena = Arena::new();
    let code_pos = mdformat::parse_with(&arena, src, &opts(), |root| {
        root.children()
            .find(|n| matches!(n.data.borrow().value, NodeValue::CodeBlock(_)))
            .expect("a top-level indented code block")
            .data
            .borrow()
            .sourcepos
    });
    assert_eq!(
        code_pos.start.column, 5,
        "comrak was expected to start the block AFTER its indent, got {code_pos:?}"
    );

    assert_eq!(accept(src), src, "the specimen is already in normal form");
    assert_eq!(
        structure_of(src, &opts()).kinds,
        vec!["paragraph", "codeBlock", "paragraph"]
    );
}

#[test]
fn the_same_block_without_its_indent_is_a_list_not_a_code_block() {
    let src =
        utf8(b"Installation caveats:\n\n- There are a couple of daemons.\n\nInstall it first.\n");
    assert_eq!(
        structure_of(src, &opts()).kinds,
        vec![
            "paragraph",
            "list",
            "  listItem",
            "    paragraph",
            "paragraph"
        ]
    );
}

#[test]
fn trailing_whitespace_on_a_content_line_is_not_a_gap() {
    let src = utf8(b"seed\n\n    code   \n");
    assert_eq!(accept(src), src);

    let arena = Arena::new();
    let literal = mdformat::parse_with(&arena, &accept(src), &opts(), |root| {
        root.children()
            .find_map(|n| match &n.data.borrow().value {
                NodeValue::CodeBlock(c) => Some(c.literal.clone()),
                _ => None,
            })
            .expect("a code block")
    });
    assert_eq!(
        literal, "code   \n",
        "the trailing spaces are the block's content"
    );
}

#[test]
fn a_one_to_three_space_block_indent_survives() {
    for src in [
        &b"seed\n\n  - item\n"[..],
        &b"seed\n\n  ```\ncode\n  ```\n"[..],
        &b"seed\n\n   1. item\n"[..],
    ] {
        let src = utf8(src);
        assert_eq!(accept(src), src, "{:?}", src.escape_debug());
    }
}

// ---------------------------------------------------------------------------
// 4. Refuted hazards, pinned anyway
// ---------------------------------------------------------------------------

#[test]
fn a_setext_underline_is_span_interior() {
    let src = utf8(b"Title\n---\n\nbody\n");
    assert_eq!(accept(src), src);
    assert_eq!(
        structure_of(src, &opts()).kinds,
        vec!["heading", "paragraph"]
    );

    // What the rule would do if it ever recursed into the heading's span.
    assert_eq!(
        structure_of(utf8(b"Title\n\n---\n\nbody\n"), &opts()).kinds,
        vec!["paragraph", "thematicBreak", "paragraph"]
    );
}

#[test]
fn list_tightness_is_not_reachable_from_a_top_level_gap() {
    let loose = utf8(b"- a\n\n- b\n");
    let tight = utf8(b"- a\n- b\n");
    assert_eq!(accept(loose), loose);
    assert_eq!(accept(tight), tight);
    assert!(structure_of(loose, &opts()).rich[0].contains("tight: false"));
    assert!(structure_of(tight, &opts()).rich[0].contains("tight: true"));
    assert_ne!(
        structure_of(loose, &opts()).html,
        structure_of(tight, &opts()).html
    );
}

#[test]
fn a_hard_line_break_survives_a_collapsed_gap_beside_it() {
    let src = utf8(b"line one  \nline two\n\n\nnext\n");
    let out = accept(src);
    assert_eq!(out, "line one  \nline two\n\nnext\n");
    let arena = Arena::new();
    let breaks = mdformat::parse_with(&arena, &out, &opts(), |root| {
        root.descendants()
            .filter(|n| matches!(n.data.borrow().value, NodeValue::LineBreak))
            .count()
    });
    assert_eq!(breaks, 1, "the hard break must survive");
}

#[test]
fn a_blockquotes_interior_is_untouched_while_the_gap_beside_it_collapses() {
    let src = utf8(b"> a\n>\n> b\n\n\n> c\n");
    assert_eq!(accept(src), "> a\n>\n> b\n\n> c\n");
    assert_eq!(
        structure_of(utf8(b"> a\n>\n> b\n"), &opts()).kinds,
        vec!["blockQuote", "  paragraph", "  paragraph"]
    );
    assert_eq!(
        structure_of(utf8(b"> a\n\n> b\n"), &opts()).kinds,
        vec!["blockQuote", "  paragraph", "blockQuote", "  paragraph"]
    );
}

// ---------------------------------------------------------------------------
// 5. Preconditions
// ---------------------------------------------------------------------------

#[test]
fn an_input_that_fails_the_partition_is_not_rewritten() {
    let src = utf8(
        b"1. first\n2. last item text\n\n        indented code\n        more code\n\ntail paragraph\n",
    );
    let arena = Arena::new();
    let blocks = mdformat::parse_with(&arena, src, &opts(), |root| {
        block_spans(root, src).expect("spans convert")
    });
    assert!(
        !check_partition(src, &blocks).is_partition(),
        "the specimen was expected to fail the partition"
    );

    let n = norm(src);
    assert!(n.accepted().is_none());
    assert!(!n.changed());
    assert_eq!(n.output, src, "a file it cannot account for is left alone");
}
