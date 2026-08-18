//! Table padding: the properties the rewrite claims, and the specimens that
//! decide its open questions.

use std::collections::{BTreeMap, BTreeSet};

use comrak::Arena;
use comrak::nodes::{NodeValue, Sourcepos};
use mdformat::table::whitespace_violation;
use mdformat::{PadViolationKind, Padding, SkipReason, check, pad};

fn opts() -> mdstruct::Options {
    mdstruct::Options::default()
}

fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("fixtures are UTF-8")
}

fn padded(source: &str) -> Padding {
    pad(source, &opts()).expect("spans convert")
}

fn accept(source: &str) -> String {
    let p = padded(source);
    match p.accepted() {
        Some(out) => out.to_string(),
        None => panic!(
            "padding was refused: structure={:?} violation={:?}",
            p.structure.as_ref().map(|d| d.to_string()),
            p.violation.as_ref().map(|v| v.to_string())
        ),
    }
}

#[test]
fn a_table_cells_sourcepos_is_byte_exact_with_escapes_intact() {
    let src = utf8(b"| a \\| b | c |\n| --- | --- |\n| x\\|y | z |\n");
    let arena = Arena::new();
    let (cells, inline) = mdformat::parse_with(&arena, src, &opts(), |root| {
        let idx = mdformat::LineIndex::new(src);
        let mut cells = Vec::new();
        let mut inline = Vec::new();
        for node in root.descendants() {
            let sp = node.data.borrow().sourcepos;
            match &node.data.borrow().value {
                NodeValue::TableCell => {
                    let (s, e) = idx.byte_span("tableCell", sp).expect("converts");
                    cells.push(src[s..e].to_string());
                }
                NodeValue::Text(t) => inline.push((t.to_string(), sp.start.column)),
                _ => {}
            }
        }
        (cells, inline)
    });

    assert_eq!(
        cells,
        vec![" a \\| b ", " c ", " x\\|y ", " z "],
        "cell sourcepos must slice the source verbatim, backslash included"
    );
    // The counterpart, and the reason this transformation reads cells and never
    // inlines: the text node inside the first cell has already been unescaped,
    // and its column no longer indexes the bytes on the line.
    assert_eq!(inline[0], ("a | b".to_string(), 3));
    assert_eq!(
        &src[2..7],
        "a \\| ",
        "the text node's own span slices five bytes that are not its text"
    );
}

#[test]
fn the_corpus_alignment_specimen_is_reproduced_byte_for_byte() {
    let src = utf8(
        b"| Test                            | Time taken | RAM usage |\n\
          | :------------------------------ | ---------: | --------: |\n\
          | **automerge (v1.0.0-preview2)** |       291s |    880 MB |\n\
          | _Plain string edits in JS_      |      0.61s |    0.1 MB |\n",
    );
    let p = padded(src);
    assert_eq!(p.accepted(), Some(src));
    assert!(!p.changed(), "the specimen is already in the normal form");
    assert_eq!(p.tables_seen, 1);
    assert_eq!(p.tables_changed, 0);
}

#[test]
fn squeezing_the_alignment_specimen_and_repadding_restores_it() {
    let squeezed = utf8(
        b"| Test | Time taken | RAM usage |\n\
          | :-- | --: | --: |\n\
          | **automerge (v1.0.0-preview2)** | 291s | 880 MB |\n\
          | _Plain string edits in JS_ | 0.61s | 0.1 MB |\n",
    );
    let expected = utf8(
        b"| Test                            | Time taken | RAM usage |\n\
          | :------------------------------ | ---------: | --------: |\n\
          | **automerge (v1.0.0-preview2)** |       291s |    880 MB |\n\
          | _Plain string edits in JS_      |      0.61s |    0.1 MB |\n",
    );
    assert_eq!(accept(squeezed), expected);
}

#[test]
fn the_corpus_ragged_specimen_is_declined_and_left_verbatim() {
    let src = utf8(
        b"| Term | Definition |\n\
          | --- | --- |\n\
          | **Claim** | The falsifiable predicate under test. |\n\
          | Escaped-pipe shift | comrak unescaping `\\|` to `|` before inline parsing. |\n",
    );

    // Pin the parser behaviour this policy exists for, so a comrak release that
    // changes it shows up here as a failure to explain rather than as silence.
    let arena = Arena::new();
    let counts = mdformat::parse_with(&arena, src, &opts(), |root| {
        root.descendants()
            .filter(|n| matches!(n.data.borrow().value, NodeValue::TableRow(_)))
            .map(|r| r.children().count())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        counts,
        vec![2, 2, 2],
        "comrak reports two cells for the last row even though it has three"
    );

    let p = padded(src);
    assert_eq!(p.accepted(), Some(src), "the table must come back verbatim");
    assert_eq!(p.tables_seen, 1);
    assert_eq!(p.tables_changed, 0);
    assert!(matches!(
        p.skipped.as_slice(),
        [s] if matches!(s.reason, SkipReason::RaggedRow { line: 4, .. })
    ));
}

#[test]
fn a_short_row_is_declined_too_and_comrak_puts_its_phantom_cell_on_the_pipe() {
    let src = utf8(b"| a | b | c |\n| --- | --- | --- |\n| 1 | 2 |\n");
    let arena = Arena::new();
    let phantom = mdformat::parse_with(&arena, src, &opts(), |root| {
        let idx = mdformat::LineIndex::new(src);
        let row = root
            .descendants()
            .filter(|n| matches!(n.data.borrow().value, NodeValue::TableRow(_)))
            .last()
            .expect("a body row");
        let last = row.children().last().expect("a cell");
        let sp = last.data.borrow().sourcepos;
        let (s, e) = idx.byte_span("tableCell", sp).expect("converts");
        src[s..e].to_string()
    });
    assert_eq!(phantom, "|", "the third cell's bytes ARE the trailing pipe");

    let p = padded(src);
    assert_eq!(p.accepted(), Some(src));
    assert!(matches!(
        p.skipped.as_slice(),
        [s] if matches!(s.reason, SkipReason::RaggedRow { line: 3, .. })
    ));
}

#[test]
fn widening_only_the_delimiter_row_is_permitted() {
    // The trailing column's cells are exempt from fill, but its delimiter run
    // still widens — to the width of the header above it, which here is the
    // same 5 the first column's computed width gives.
    let src = utf8(b"| aaaaa | bbbbb |\n| - | - |\n| ccccc | ddddd |\n");
    assert_eq!(
        accept(src),
        "| aaaaa | bbbbb |\n| ----- | ----- |\n| ccccc | ddddd |\n"
    );
}

#[test]
fn a_trailing_unaligned_column_is_left_unpadded() {
    let src = utf8(b"| Term | Definition |\n| --- | --- |\n| a | a long definition |\n");
    // `Definition` is 10 wide and the column is 17, so the dash run is 10.
    assert_eq!(
        accept(src),
        "| Term | Definition |\n| ---- | ---------- |\n| a    | a long definition |\n"
    );
}

#[test]
fn a_trailing_left_aligned_column_is_left_unpadded_too() {
    let src = utf8(b"| Term | Definition |\n| :--- | :--- |\n| a | a long definition |\n");
    // The colon survives on the same side, and the cell it opens is the width
    // of the header above it — colon included, so nine dashes follow.
    assert_eq!(
        accept(src),
        "| Term | Definition |\n| :--- | :--------- |\n| a    | a long definition |\n"
    );
}

#[test]
fn a_trailing_right_aligned_column_keeps_its_padding() {
    let src = utf8(b"| Term | Count |\n| --- | ---: |\n| a | 1 |\n");
    assert_eq!(
        accept(src),
        "| Term | Count |\n| ---- | ----: |\n| a    |     1 |\n"
    );
}

#[test]
fn a_trailing_center_aligned_column_keeps_its_padding() {
    let src = utf8(b"| Term | Count |\n| --- | :-: |\n| a | 1 |\n");
    assert_eq!(
        accept(src),
        "| Term | Count |\n| ---- | :---: |\n| a    |   1   |\n"
    );
}

#[test]
fn a_single_column_table_is_all_trailing_column() {
    let bare = utf8(b"| Key |\n| --- |\n| a |\n");
    let p = padded(bare);
    assert_eq!(p.accepted(), Some(bare));
    assert!(!p.changed(), "there is no cell left to pad");
    assert_eq!(p.tables_seen, 1);
    assert_eq!(p.tables_changed, 0);

    let aligned = utf8(b"| Only |\n| ---: |\n| a |\n");
    assert_eq!(accept(aligned), "| Only |\n| ---: |\n|    a |\n");
}

#[test]
fn a_hand_padded_trailing_column_loses_its_padding() {
    let src = utf8(b"| key | value  |\n| --- | ------ |\n| a   | longer |\n");
    let p = padded(src);
    assert!(
        p.changed(),
        "the uncapped padder's own output is no longer a fixpoint"
    );
    assert_eq!(
        p.accepted(),
        Some("| key | value |\n| --- | ----- |\n| a   | longer |\n")
    );
    assert_eq!(p.tables_changed, 1);
}

#[test]
fn the_trailing_delimiter_follows_the_header_and_not_the_column() {
    let src = utf8(
        b"| Command | Format |\n\
          | --- | --- |\n\
          | schedule | a long specification of the schedule entry format |\n",
    );
    let out = accept(src);
    assert_eq!(
        out,
        "| Command  | Format |\n\
         | -------- | ------ |\n\
         | schedule | a long specification of the schedule entry format |\n"
    );
    let widths: Vec<usize> = out.lines().map(|l| l.chars().count()).collect();
    assert_eq!(
        widths[1], widths[0],
        "the delimiter line must be as wide as the header line, not the body"
    );
}

#[test]
fn a_trailing_header_narrower_than_three_still_gets_three_dashes() {
    let src = utf8(b"| Key | x |\n| --- | - |\n| a | a much longer cell |\n");
    assert_eq!(
        accept(src),
        "| Key | x |\n| --- | --- |\n| a   | a much longer cell |\n"
    );

    // And with a colon, where the floor is what keeps the marker renderable.
    let aligned = utf8(b"| Key | x |\n| --- | :-- |\n| a | a much longer cell |\n");
    assert_eq!(
        accept(aligned),
        "| Key | x |\n| --- | :-- |\n| a   | a much longer cell |\n"
    );
}

#[test]
fn the_exemption_leaves_the_widest_line_where_the_uncapped_form_had_it() {
    let src = utf8(b"| key | value |\n| --- | --- |\n| a | longer |\n| bb | x |\n");
    let uncapped =
        utf8(b"| key | value  |\n| --- | ------ |\n| a   | longer |\n| bb  | x      |\n");
    let widest = |s: &str| s.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    let out = accept(src);
    assert_eq!(
        widest(&out),
        widest(uncapped),
        "the exemption must not change the table's widest line"
    );
    assert!(
        out.len() < uncapped.len(),
        "but it must remove bytes from the shorter rows"
    );
}

#[test]
fn padding_is_idempotent() {
    for src in [
        utf8(b"| a | b |\n| --- | --- |\n| longer | x |\n"),
        utf8(b"| \xd0\x9a\xd0\xbb\xd1\x8e\xd1\x87 | b |\n| --- | --- |\n| x | y |\n"),
        utf8(b"| a | b | c |\n| :-- | --: | :-: |\n| xxxx | yyyy | zzzz |\n"),
        utf8(b"> | a | bb |\n> | --- | --- |\n> | ccc | d |\n"),
        utf8(b"| a\\|b | c |\n| --- | --- |\n| d | e |\n"),
        // The exemption's own shapes: a trailing column that is dropped from
        // cell padding must not be re-padded on the second pass, and its
        // delimiter run — sized to the header above it — must be a fixpoint of
        // itself.
        utf8(b"| key | value  |\n| --- | ------ |\n| a   | longer |\n"),
        utf8(b"| Term | Definition |\n| :--- | :--- |\n| a | a long definition |\n"),
        utf8(b"| Only |\n| --- |\n| a |\n"),
        utf8(b"| Only |\n| ---: |\n| a |\n"),
        utf8(b"a | bb\n--- | ---\nccc | d\n"),
    ] {
        let once = accept(src);
        let twice = accept(&once);
        assert_eq!(once, twice, "padding must be a fixpoint of itself: {src:?}");
    }
}

#[test]
fn no_non_whitespace_byte_moves_outside_a_delimiter_row() {
    for src in [
        utf8(b"| a | b |\n| --- | --- |\n| longer | x |\n"),
        utf8(b"text before\n\n| a | b |\n| --- | --- |\n| longer | x |\n\ntext after\n"),
        utf8(b"| a | b | c |\n| :-- | --: | :-: |\n| xxxx | yyyy | zzzz |\n"),
        utf8(b"> | a | bb |\n> | --- | --- |\n> | ccc | d |\n"),
    ] {
        let out = accept(src);
        let strip = |s: &str| {
            s.lines()
                .enumerate()
                // Every fixture here puts its delimiter on the second line of
                // its table; dropping *all* dash runs would weaken the check.
                .filter(|(i, l)| !(l.contains("--") && (*i == 1 || *i == 3)))
                .map(|(_, l)| l.replace([' ', '\t'], ""))
                .collect::<Vec<_>>()
        };
        assert_eq!(strip(src), strip(&out), "for {src:?}");
    }
}

#[test]
fn the_whitespace_oracle_rejects_a_changed_content_byte() {
    let before = utf8(b"| a | b |\n| --- | --- |\n| 1 | 2 |\n");
    let after = utf8(b"| a | b |\n| --- | --- |\n| 1 | 9 |\n");
    let delims = BTreeMap::from([(2usize, 0usize)]);
    let rows = BTreeSet::from([1usize, 3]);
    let v = whitespace_violation(before, after, &delims, &rows).expect("must reject");
    assert_eq!(v.kind, PadViolationKind::ContentBytes);
    assert_eq!(v.line, 3);
}

#[test]
fn the_whitespace_oracle_exempts_dashes_but_not_colons() {
    let before = utf8(b"| a | b |\n| :-- | --- |\n| 1 | 2 |\n");
    let delims = BTreeMap::from([(2usize, 0usize)]);
    let rows = BTreeSet::from([1usize, 3]);

    let widened = utf8(b"| a | b |\n| :-------- | --------- |\n| 1 | 2 |\n");
    assert_eq!(
        whitespace_violation(before, widened, &delims, &rows),
        None,
        "a longer dash run is the one change this rewrite is for"
    );

    let recoloured = utf8(b"| a | b |\n| --- | --- |\n| 1 | 2 |\n");
    let v = whitespace_violation(before, recoloured, &delims, &rows).expect("must reject");
    assert_eq!(v.kind, PadViolationKind::DelimiterMarkers);
}

#[test]
fn the_cell_oracle_catches_what_the_line_check_cannot() {
    let before = utf8(b"| a  b | c |\n| --- | --- |\n| 1 | 2 |\n");
    let after = utf8(b"| a b  | c |\n| --- | --- |\n| 1 | 2 |\n");
    let delims = BTreeMap::from([(2usize, 0usize)]);

    // Without the row registered, only the non-whitespace byte sequence is
    // compared — and it is unchanged, so nothing is reported.
    assert_eq!(
        whitespace_violation(before, after, &delims, &BTreeSet::new()),
        None,
        "the line check is provably blind to an interior space"
    );

    let rows = BTreeSet::from([1usize, 3]);
    let v = whitespace_violation(before, after, &delims, &rows).expect("must reject");
    assert_eq!(v.kind, PadViolationKind::CellContent);
    assert_eq!(v.line, 1);
}

#[test]
fn every_refusal_this_rule_makes_is_one_table_and_not_the_document() {
    // (name, source). A `\r` here is deliberate: `check` runs every rule on the
    // same input, so this rule sees the endings the pipeline would have fixed.
    let specimens: &[(&str, &[u8])] = &[
        ("crlf", b"| a | b |\r\n| - | - |\r\n| 1 | 2 |\r\n"),
        ("lone-cr", b"| a | b |\r| - | - |\r| 1 | 2 |\r"),
        ("tab-in-cell", b"| a\tx | b |\n| - | - |\n| 1 | 2 |\n"),
        ("tab-at-cell-edge", b"|\ta | b |\n| - | - |\n| 1 | 2 |\n"),
        (
            "no-break-space",
            b"| \xC2\xA0a | b |\n| - | - |\n| 1 | 2 |\n",
        ),
        ("escaped-pipe", b"| a \\| b | c |\n| - | - |\n| 1 | 2 |\n"),
        (
            "escaped-backslash",
            b"| a\\\\ | b |\n| - | - |\n| 1 | 2 |\n",
        ),
        (
            "escaped-leading-pipe",
            b"\\| a | b |\n| - | - |\n| 1 | 2 |\n",
        ),
        ("no-outer-pipes", b"a | b\n- | -\n1 | 2\n"),
        (
            "in-a-block-quote",
            b"> | a | b |\n> | - | - |\n> | 1 | 2 |\n",
        ),
        ("lazy-continuation", b"> | a | b |\n> | - | - |\nlazy\n"),
        ("in-a-list-item", b"- item\n\n  | a | b |\n  | - | - |\n"),
        ("empty-cells", b"|  |  |\n| - | - |\n|  |  |\n"),
        ("aligned", b"| a | b |\n| :-: | --: |\n| 1 | 2 |\n"),
        ("trailing-spaces", b"| a | b |  \n| - | - |\n| 1 | 2 |  \n"),
        ("backslash-at-eol", b"| a | b\\ |\n| - | - |\n| 1 | 2 |\n"),
        ("code-span-pipe", b"| `a|b` | c |\n| - | - |\n| 1 | 2 |\n"),
        ("three-space-indent", b"   | a | b |\n   | - | - |\n"),
        (
            "two-tables",
            b"| a | b |\n| - | - |\n\n| c | d |\n| - | - |\n",
        ),
        ("long-row", b"| a | b |\n| - | - |\n| 1 | 2 | 3 |\n"),
        ("short-row", b"| a | b |\n| - | - |\n| 1 |\n"),
    ];

    let (mut padded_some, mut skipped_some) = (0usize, 0usize);
    for (name, input) in specimens {
        let src = utf8(input);
        let p = padded(src);
        assert!(
            p.structure.is_none(),
            "{name}: padding changed the parse, which no input was thought to \
             do: {:?}",
            p.structure.as_ref().map(|d| d.to_string())
        );
        assert!(
            p.violation.is_none(),
            "{name}: padding moved more than whitespace, which no input was \
             thought to do: {:?}",
            p.violation.as_ref().map(|v| v.to_string())
        );
        assert_eq!(
            p.accepted(),
            Some(&*p.output),
            "{name}: the bytes must be available, since no guard refused them"
        );
        padded_some += usize::from(p.changed());
        skipped_some += usize::from(!p.skipped.is_empty());

        // And the same verdict where a caller reads it: one exemption per
        // declined table, and no declination of the document.
        let c = check(src, &opts()).expect("spans convert");
        let r = c
            .rules
            .iter()
            .find(|r| r.rule == "tables")
            .expect("the table rule is in RULES");
        assert!(
            r.declined.is_none(),
            "{name}: the rule declined the whole document: {:?}",
            r.declined
        );
        assert_eq!(
            r.exempt.len(),
            p.skipped.len(),
            "{name}: every declined table must reach the report"
        );
        assert_eq!(
            r.is_normal(),
            !p.changed(),
            "{name}: the predicate must agree with the rewrite"
        );
    }

    // The battery is not vacuous in either direction: it holds shapes the rule
    // pads and shapes it declines, so the silence above is a measurement over
    // both branches rather than over an inert list.
    assert!(
        padded_some >= 12,
        "only {padded_some} specimens were padded"
    );
    assert!(
        skipped_some >= 3,
        "only {skipped_some} specimens exercised the per-table exemption"
    );
}

fn positions(src: &str) -> Vec<(&'static str, Sourcepos)> {
    let arena = Arena::new();
    mdformat::parse_with(&arena, src, &opts(), |root| {
        root.descendants()
            .skip(1)
            .map(|n| {
                let d = n.data.borrow();
                (mdformat::block_kind(&d.value), d.sourcepos)
            })
            .collect()
    })
}

#[test]
fn a_byte_order_mark_shifts_only_line_one_columns_inside_a_table() {
    // (name, the unmarked document). Each is prefixed with the mark to make the
    // marked half, so the two halves cannot drift apart.
    let specimens: &[(&str, &[u8])] = &[
        (
            "table-on-line-1",
            b"| abc | defg |\n| --- | --- |\n| 1 | 2 |\n| 33 | 44 |\n",
        ),
        (
            "table-after-a-paragraph",
            b"intro\n\n| abc | defg |\n| --- | --- |\n| 1 | 2 |\n| 33 | 44 |\n",
        ),
    ];

    let mark = "\u{feff}".len();
    let shift = |line: usize| if line == 1 { mark } else { 0 };
    for (name, body) in specimens {
        let bare = positions(utf8(body));
        let marked = positions(&format!("\u{feff}{}", utf8(body)));
        assert_eq!(
            bare.len(),
            marked.len(),
            "{name}: the mark must not change the tree, only where its nodes sit"
        );
        assert_eq!(
            bare.iter().filter(|(k, _)| *k == "tableRow").count(),
            3,
            "{name}: the specimen must carry a header row and two body rows"
        );

        for (i, ((kind, b), (_, m))) in bare.iter().zip(&marked).enumerate() {
            let want = Sourcepos::from((
                b.start.line,
                b.start.column + shift(b.start.line),
                b.end.line,
                b.end.column + shift(b.end.line),
            ));
            assert_eq!(
                *m, want,
                "{name}: node {i} ({kind}): the mark moved a column off line 1 \
                 — unmarked {b:?}, marked {m:?}"
            );
        }
    }
}
