//! Ill-formed input paired with a hand-written expected output, one per rule
//! clause, asserted byte for byte.

use mdformat::{Format, check, format};

fn opts() -> mdstruct::Options {
    mdstruct::Options::default()
}

fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("fixtures are UTF-8")
}

fn formatted(source: &str) -> Format {
    format(source, &opts()).expect("spans convert")
}

struct Fixture {
    name: &'static str,
    clause: &'static str,
    input: &'static [u8],
    expected: &'static [u8],
}

impl Fixture {
    fn discriminating(&self) -> bool {
        self.input != self.expected
    }
}

/// The fixtures, in rule order: gaps, then endings, then tables, then markers,
/// then all at once, then the constructs a rule declines.
const FIXTURES: &[Fixture] = &[
    // ---------------------------------------------------------------- gaps --
    Fixture {
        name: "gaps: a run of blank lines collapses to one",
        clause: "between any other two top-level blocks -> exactly one blank line",
        input: b"# Title\n\n\n\nOne paragraph.\n\n\n## Section\n\ntail\n",
        expected: b"# Title\n\nOne paragraph.\n\n## Section\n\ntail\n",
    },
    Fixture {
        // The same expectation reached from the other side: the normal form is
        // a form, not a direction of travel.
        name: "gaps: a missing blank line is inserted",
        clause: "between any other two top-level blocks -> exactly one blank line",
        input: b"# Title\nOne paragraph.\n## Section\ntail\n",
        expected: b"# Title\n\nOne paragraph.\n\n## Section\n\ntail\n",
    },
    Fixture {
        name: "gaps: leading blank lines are deleted",
        clause: "before the first block -> \"\"",
        input: b"\n\n\n# Title\n\nbody\n",
        expected: b"# Title\n\nbody\n",
    },
    Fixture {
        name: "gaps: the file ends with exactly one newline",
        clause: "after the last block -> \"\\n\"",
        input: b"# Title\n\nbody\n\n\n",
        expected: b"# Title\n\nbody\n",
    },
    Fixture {
        name: "gaps: a missing final newline is added",
        clause: "after the last block -> \"\\n\"",
        input: b"# Title\n\nbody",
        expected: b"# Title\n\nbody\n",
    },
    Fixture {
        // Rule 4 (no trailing whitespace on a blank line) needs no clause of
        // its own because a gap is regenerated rather than edited. This is
        // what that claim looks like as bytes: three differently-padded blank
        // lines, one blank line out.
        name: "gaps: whitespace-only lines in a gap are regenerated, not edited",
        clause: "a gap is regenerated, so whatever its blank lines carried is gone",
        input: b"# Title\n   \n\t\n \t \nbody\n",
        expected: b"# Title\n\nbody\n",
    },
    Fixture {
        // The counterpart, and the boundary of the clause above: trailing
        // whitespace on a *content* line is span interior (content_span step 3
        // extends the span back over it), so the rule is silent about it. An
        // indented code block's literal is `"code   \n"`, and this is the
        // property that keeps it intact.
        name: "gaps: trailing whitespace on a content line is out of scope and survives",
        clause: "step 3 extends the span right to the last line's content end",
        input: b"# Title\n\n\nbody   \n",
        expected: b"# Title\n\nbody   \n",
    },
    Fixture {
        // Key whitespace, case 1: two spaces at end of line are a
        // hard line break, and they sit inside the paragraph's span.
        name: "gaps: a hard line break inside a paragraph survives",
        clause: "span interiors are unreachable; only gap bytes are rewritten",
        input: b"first line  \nsecond line\n\n\ntail\n",
        expected: b"first line  \nsecond line\n\ntail\n",
    },
    Fixture {
        // Key whitespace, case 2: the blank line between the items is
        // what makes the list loose, and it is span interior. Deleting it
        // would change the rendered HTML.
        name: "gaps: a loose list keeps the interior blank line that makes it loose",
        clause: "top-level gaps only; no recursion into containers",
        input: b"# H\n\n\n- alpha\n\n- beta\n\n\ntail\n",
        expected: b"# H\n\n- alpha\n\n- beta\n\ntail\n",
    },
    Fixture {
        // The other half of the same pair: a tight list must not gain the
        // blank lines the top-level rule would emit between blocks.
        name: "gaps: a tight list gains no blank lines between its items",
        clause: "top-level gaps only; no recursion into containers",
        input: b"# H\n\n\n- a\n- b\n- c\n\n\ntail\n",
        expected: b"# H\n\n- a\n- b\n- c\n\ntail\n",
    },
    Fixture {
        // Key whitespace, case 3: the newline between the text and
        // the underline is span interior. Emitting a blank line there would
        // turn one heading into a paragraph and a thematic break.
        name: "gaps: a setext underline stays attached to its heading",
        clause: "span interiors are unreachable; only gap bytes are rewritten",
        input: b"Title\n=====\n\n\nbody\n",
        expected: b"Title\n=====\n\nbody\n",
    },
    Fixture {
        // A blank line inside a block quote is `>`, not empty — the rule's
        // output alphabet is container-dependent, which is one of the two
        // measured reasons it does not recurse. The `>` line must survive.
        name: "gaps: a blank quote line inside a block quote is interior",
        clause: "top-level gaps only; no recursion into containers",
        input: b"# H\n\n\n> one\n>\n> two\n\n\ntail\n",
        expected: b"# H\n\n> one\n>\n> two\n\ntail\n",
    },
    Fixture {
        name: "gaps: blank lines inside a fenced code block are interior",
        clause: "span interiors are unreachable; only gap bytes are rewritten",
        input: b"# H\n\n\n```\n\ncode\n\n```\n\n\ntail\n",
        expected: b"# H\n\n```\n\ncode\n\n```\n\ntail\n",
    },
    Fixture {
        // The content_span step-2 case: comrak reports a top-level indented
        // code block starting at column 5, so its four-space indent is outside
        // the raw span and would fall in the gap. Extending the span left to
        // the line start is what keeps the indent — and with it the block's
        // identity as code.
        name: "gaps: an indented code block keeps its indent and its interior blank line",
        clause: "step 2 extends the span left to the line start",
        input: b"# H\n\n\n    code\n\n    more\n\n\ntail\n",
        expected: b"# H\n\n    code\n\n    more\n\ntail\n",
    },
    Fixture {
        name: "gaps: exactly one blank line follows front matter",
        clause: "after frontmatter -> \"\\n\\n\" (exactly one blank line)",
        input: b"---\nk: v\n---\n# H\n\nbody\n",
        expected: b"---\nk: v\n---\n\n# H\n\nbody\n",
    },
    // ------------------------------------------------------------ endings --
    Fixture {
        // Two rules agree on this one — the gap rule states its
        // separators as LF literals, and the endings rule would rewrite them
        // anyway — which is why it is not the discriminating case.
        name: "endings: a CRLF document comes out LF throughout",
        clause: "\"\\r\\n\" -> \"\\n\"",
        input: b"# Title\r\n\r\n\r\nbody\r\n",
        expected: b"# Title\n\nbody\n",
    },
    Fixture {
        // The discriminating case. A CRLF between two lines of one paragraph
        // is span interior, so no gap rule reaches it, and without this rule
        // the output would hold both endings.
        name: "endings: a CRLF inside a paragraph is span interior and is rewritten anyway",
        clause: "\"\\r\\n\" -> \"\\n\", every line ending, span interior included",
        input: b"first\r\nsecond\r\n\r\n\r\ntail\r\n",
        expected: b"first\nsecond\n\ntail\n",
    },
    Fixture {
        // A lone `\r` is a CommonMark line ending too, so these three lines
        // are a paragraph, a heading and a paragraph, and the gap rule puts one
        // blank line between each pair.
        name: "endings: a lone CR is a line ending and becomes LF",
        clause: "a lone \"\\r\" -> \"\\n\"",
        input: b"a\r## H\rbody\r",
        expected: b"a\n\n## H\n\nbody\n",
    },
    Fixture {
        // The key case of span interior: the bytes between the fences are a
        // code block's literal, which the structure oracle refuses to trim
        // because they are content. The endings rule rewrites them regardless,
        // which is why that oracle does not gate this rule.
        name: "endings: a CRLF inside a fenced code block becomes LF",
        clause: "\"\\r\\n\" -> \"\\n\", every line ending, code-block literals included",
        input: b"# H\r\n\r\n```\r\ncode\r\n```\r\n",
        expected: b"# H\n\n```\ncode\n```\n",
    },
    Fixture {
        // All three endings in one document, which is the acceptance condition
        // stated as bytes: whatever a file mixes, the output holds one ending.
        // `lf\ncrlf` is one paragraph and `cr\nend` is another, so the blank
        // line between them is the only gap.
        name: "endings: a document mixing all three endings comes out with one",
        clause: "\"\\r\\n\" -> \"\\n\", a lone \"\\r\" -> \"\\n\", \"\\n\" -> \"\\n\"",
        input: b"lf\ncrlf\r\n\r\ncr\rend\n",
        expected: b"lf\ncrlf\n\ncr\nend\n",
    },
    // -------------------------------------------------------------- tables --
    Fixture {
        // Width 1 in both columns, floored to 3; the trailing unaligned column
        // takes the separator space and the closing pipe and nothing else,
        // while its delimiter cell runs the width of the header above it —
        // here 1, so the floor of 3 decides it.
        name: "tables: every column is padded to its width, floored at 3",
        clause: "a column's width is its widest cell, floored at 3",
        input: b"| a | b |\n| --- | --- |\n| 1 | 2 |\n",
        expected: b"| a   | b |\n| --- | --- |\n| 1   | 2 |\n",
    },
    Fixture {
        // The direction a do-nothing formatter cannot fake and a
        // padding-only one cannot either: cells and the delimiter run must
        // *shrink* to the column width — and in the exempt trailing column,
        // to the width of `value`, the header the dashes sit under.
        name: "tables: over-wide cells and delimiter runs shrink back",
        clause: "each cell is \"|\" + \" \" + content + fill + \" \"; fill is exact",
        input: b"|   key   |   value   |\n| ------- | --------- |\n| a       | longer    |\n",
        expected: b"| key | value |\n| --- | ----- |\n| a   | longer |\n",
    },
    Fixture {
        // All three alignments at once. Column 3 is right-aligned, so the
        // trailing-column exemption does not apply to it: an alignment means
        // nothing without the fill that realizes it. The centre column's odd
        // fill goes right (`left = fill / 2`).
        name: "tables: alignment places the fill and the delimiter colons",
        clause: "fill right for none/left, left for right, split for centre",
        input: b"| l | c | r |\n| :-- | :-: | --: |\n| a | bb | ccc |\n",
        expected: b"| l   |  c  |   r |\n| :-- | :-: | --: |\n| a   | bb  | ccc |\n",
    },
    Fixture {
        // `\|` is measured over the
        // source bytes, escapes intact, so it counts 2 and the column is 4
        // wide. Measuring the rendered text would give 3.
        name: "tables: an escaped pipe counts two columns toward the width",
        clause: "a cell's width is measured over its source bytes, escapes intact",
        input: b"| a | b |\n| --- | --- |\n| x\\|y | z |\n",
        expected: b"| a    | b |\n| ---- | --- |\n| x\\|y | z |\n",
    },
    Fixture {
        // The one place the three candidate measures disagree. Two U+1F389
        // (PARTY POPPER) occupy 4 terminal columns and 2 characters and 8
        // bytes; the column must come out 4 wide. `\xF0\x9F\x8E\x89` is one
        // U+1F389, written as bytes so the specimen survives any editor.
        name: "tables: an emoji cell is measured in terminal columns",
        clause: "the measure is unicode-width display width, not bytes or chars",
        input: b"| f | note |\n| --- | --- |\n| \xF0\x9F\x8E\x89\xF0\x9F\x8E\x89 | done |\n",
        expected:
            b"| f    | note |\n| ---- | ---- |\n| \xF0\x9F\x8E\x89\xF0\x9F\x8E\x89 | done |\n",
    },
    Fixture {
        // A byte order mark occupies three bytes of line 1 and no other line,
        // but comrak anchors every row's cells at the table's line-1 opening
        // offset, so under a mark the body rows come out three bytes right.
        // The expectation is the mark-free fixture above with the mark
        // restored: a table does not depend on what precedes it on line 1.
        name: "tables: a table opening on a byte order mark's line pads like any other",
        clause: "a column's width is its widest cell, floored at 3",
        input: b"\xEF\xBB\xBF| a | b |\n| --- | --- |\n| 1 | 2 |\n",
        expected: b"\xEF\xBB\xBF| a   | b |\n| --- | --- |\n| 1   | 2 |\n",
    },
    // ------------------------------------------------------------ markers --
    Fixture {
        // These fixtures are the clause's only exercise: a bullet that is
        // already `-` gives the rewrite nothing to do.
        name: "markers: a star bullet becomes a dash",
        clause: "a bullet list item is introduced by `-`",
        input: b"* alpha\n* beta\n",
        expected: b"- alpha\n- beta\n",
    },
    Fixture {
        name: "markers: a plus bullet becomes a dash",
        clause: "a bullet list item is introduced by `-`",
        input: b"+ alpha\n+ beta\n",
        expected: b"- alpha\n- beta\n",
    },
    Fixture {
        name: "markers: a paren ordered delimiter becomes a period",
        clause: "an ordered list item's number is followed by `.`",
        input: b"1) one\n2) two\n",
        expected: b"1. one\n2. two\n",
    },
    Fixture {
        // The scope boundary, as bytes. Renumbering `3.`/`7.` to `1.`/`2.` is
        // a different rewrite with a different argument behind it, and the
        // hand-written expectation is what stops this rule drifting into it.
        name: "markers: the ordinals are not renumbered",
        clause: "scope is the marker character only; the ordinals are untouched",
        input: b"3) three\n7) seven\n",
        expected: b"3. three\n7. seven\n",
    },
    Fixture {
        // The other scope boundary: every edit is one ASCII byte for another,
        // so a nested item's two-space indent and the content column it aligns
        // to cannot move. Enforcing that alignment is a decision nobody has
        // made; this fixture pins that the rule does not make it by accident.
        name: "markers: a nested list keeps its indentation",
        clause: "scope is the marker character only; indentation is untouched",
        input: b"* outer\n  * inner\n* tail\n",
        expected: b"- outer\n  - inner\n- tail\n",
    },
    Fixture {
        name: "markers: a task item's checkbox survives its bullet changing",
        clause: "a bullet list item is introduced by `-`",
        input: b"* [ ] todo\n* [x] done\n",
        expected: b"- [ ] todo\n- [x] done\n",
    },
    // -------------------------------------------------------- every rule ---
    Fixture {
        // One invocation, gaps and tables, on a document ill-formed under each.
        // Column 2 holds "22" (width 2), floored to 3, which is why its
        // delimiter is three dashes and not two.
        name: "both: a gap collapse and a table padding in one pass",
        clause: "format applies every rule in RULES, gaps then tables",
        input: b"# H\n\n\n| a | b |\n| --- | --- |\n| 1 | 22 |\n\n\npara\n",
        expected: b"# H\n\n| a   | b |\n| --- | --- |\n| 1   | 22 |\n\npara\n",
    },
    Fixture {
        // All four rules in one pass, on a document ill-formed under each.
        name: "all: line endings, a gap collapse, a table padding and a bullet in one pass",
        clause: "format applies every rule in RULES, endings then gaps then tables then markers",
        input:
            b"# H\r\n\r\n\r\n| a | b |\r\n| --- | --- |\r\n| 1 | 22 |\r\n\r\n\r\n+ one\r\n+ two\r\n",
        expected: b"# H\n\n| a   | b |\n| --- | --- |\n| 1   | 22 |\n\n- one\n- two\n",
    },
    // ------------------------------------------------------- declinations --
    Fixture {
        // A ragged row makes `pad` decline the table — comrak
        // does not model raggedness, so padding it would either delete the
        // long row's overflow or materialize the short row's missing cell.
        // The document is therefore *normal* while holding an unpadded table.
        name: "declined: a ragged table is left verbatim",
        clause: "a table with a ragged row is skipped, and its whole table with it",
        input: b"| a | b | c |\n| --- | --- | --- |\n| 1 | 2 |\n",
        expected: b"| a | b | c |\n| --- | --- | --- |\n| 1 | 2 |\n",
    },
    Fixture {
        // The causal control for the fixture above, and the reason it is
        // stored as a fixture rather than an aside: the same table with one
        // cell added to the short row. The single differing factor is that
        // row's cell count, and the padding fires.
        name: "declined (control): the same table made rectangular is padded",
        clause: "a column's width is its widest cell, floored at 3",
        input: b"| a | b | c |\n| --- | --- | --- |\n| 1 | 2 | 3 |\n",
        expected: b"| a   | b   | c |\n| --- | --- | --- |\n| 1   | 2   | 3 |\n",
    },
    Fixture {
        // The gap rule's structure guard, on the shape that motivated it:
        // deleting the leading blank lines promotes the `---` into front
        // matter, which is a different parse, so the rewrite is refused and
        // the rule yields its input. A refused document is normal — the
        // declination and the exemption are the same fact.
        name: "declined: a rewrite that would promote `---` into front matter",
        clause: "the rewrite is refused when it changes the parse",
        input: b"\n\n---\nk: v\n---\n",
        expected: b"\n\n---\nk: v\n---\n",
    },
    Fixture {
        // An unterminated fence at EOF absorbs the blank lines after it into
        // its literal, so the trailing-newline clause would delete code-block
        // content. The block skeleton is one codeBlock either way, which is why
        // the oracle compares rich and HTML signatures rather than kinds.
        name: "declined: an unterminated fence whose literal holds the trailing blank lines",
        clause: "the rewrite is refused when it changes the parse",
        input: b"```\ncode\n\n\n",
        expected: b"```\ncode\n\n\n",
    },
    Fixture {
        // The causal control for the fixture above: close the fence and the
        // same trailing blank lines collapse to one newline. The single
        // differing factor is the closing fence — with it, the blank lines are
        // gap bytes rather than code-block content.
        name: "declined (control): closing the fence makes the same trailing lines a gap",
        clause: "after the last block -> \"\\n\"",
        input: b"```\ncode\n```\n\n\n",
        expected: b"```\ncode\n```\n",
    },
    Fixture {
        // The marker rule's per-construct declination. In CommonMark a change
        // of bullet character starts a new list, so these are **two** lists,
        // and unifying both markers would splice them into one. The rule
        // leaves both verbatim, and the document is therefore normal while
        // holding a `*` and a `+`.
        name: "declined: two adjacent lists with different bullets are left verbatim",
        clause: "a mixed adjacent pair is declined, because unifying it merges the two lists",
        input: b"* alpha\n\n+ beta\n",
        expected: b"* alpha\n\n+ beta\n",
    },
    Fixture {
        // The causal control for the fixture above. The single differing
        // factor is the second marker: with both `*`, this is one loose list
        // rather than two, there is nothing to merge, and both bullets are
        // unified. The blank line between the items is span interior and
        // survives, which is what keeps the list loose.
        name: "declined (control): the same two items under one bullet are one list and are unified",
        clause: "a bullet list item is introduced by `-`",
        input: b"* alpha\n\n* beta\n",
        expected: b"- alpha\n\n- beta\n",
    },
    Fixture {
        // The same declination on the ordered side: `1.` and `1)` are two
        // lists for the same reason.
        name: "declined: two adjacent ordered lists with different delimiters are left verbatim",
        clause: "a mixed adjacent pair is declined, because unifying it merges the two lists",
        input: b"1. one\n\n1) two\n",
        expected: b"1. one\n\n1) two\n",
    },
    Fixture {
        // The second causal control: a bullet list beside an ordered one
        // cannot merge whatever their markers become, so neither is declined
        // and both are unified. The single differing factor against the two
        // fixtures above is the list kind.
        name: "declined (control): a bullet list beside an ordered list cannot merge",
        clause: "a bullet list item is introduced by `-`; an ordered one's number by `.`",
        input: b"* alpha\n\n1) one\n",
        expected: b"- alpha\n\n1. one\n",
    },
    Fixture {
        // Why the adjacency check is not the whole guard. `+ + +` is three
        // nested one-item bullet lists, none beside a sibling, whose unified
        // form `- - -` is a thematic break. Only the re-parse oracle's `kinds`
        // comparison sees that.
        name: "declined: a bullet change that would turn nested lists into a thematic break",
        clause: "the rewrite is refused when it changes the parse",
        input: b"+ + +\n",
        expected: b"+ + +\n",
    },
];

#[test]
fn every_fixture_formats_to_its_hand_written_expectation() {
    let mut wrong = Vec::new();
    for f in FIXTURES {
        let got = formatted(utf8(f.input)).output;
        let want = utf8(f.expected);
        if got != want {
            wrong.push(format!(
                "\n{}\n  clause:   {}\n  input:    {:?}\n  expected: {:?}\n  got:      {:?}",
                f.name,
                f.clause,
                utf8(f.input),
                want,
                got
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} fixtures departed from their hand-written expectation:{}",
        wrong.len(),
        FIXTURES.len(),
        wrong.join("")
    );
}

#[test]
fn every_expectation_is_a_fixpoint() {
    for f in FIXTURES {
        let want = utf8(f.expected);
        let again = formatted(want);
        assert_eq!(
            again.output, want,
            "{}: the hand-written normal form is not a fixpoint",
            f.name
        );
        assert!(
            !again.changed,
            "{}: `changed` disagrees with the bytes",
            f.name
        );
        let c = check(want, &opts()).expect("spans convert");
        assert!(
            c.is_normal(),
            "{}: `check` calls the hand-written normal form abnormal: {:?}",
            f.name,
            c.departures().collect::<Vec<_>>()
        );
    }
}

#[test]
fn formatting_twice_changes_nothing() {
    for f in FIXTURES {
        let once = formatted(utf8(f.input)).output;
        let twice = formatted(&once);
        assert_eq!(
            twice.output, once,
            "{}: the second pass changed the first pass's output",
            f.name
        );
        assert!(
            !twice.changed,
            "{}: `changed` set on the second pass",
            f.name
        );
    }
}

#[test]
fn the_declining_fixtures_actually_decline() {
    let ragged = utf8(b"| a | b | c |\n| --- | --- | --- |\n| 1 | 2 |\n");
    let promoted = utf8(b"\n\n---\nk: v\n---\n");
    let fence = utf8(b"```\ncode\n\n\n");
    let mixed_bullets = utf8(b"* alpha\n\n+ beta\n");
    let mixed_delimiters = utf8(b"1. one\n\n1) two\n");
    let nested_break = utf8(b"+ + +\n");

    // A per-table declination: the table rule runs, and exempts one construct.
    let c = check(ragged, &opts()).expect("spans convert");
    assert!(c.is_normal(), "{:?}", c.departures().collect::<Vec<_>>());
    assert_eq!(c.exempt().count(), 1, "the ragged table must be exempt");
    assert_eq!(
        c.declined().count(),
        0,
        "no rule declines the whole document"
    );

    // Per-list declinations: **both** members of a mixed adjacent pair are
    // exempt, because unifying either one alone still leaves the document
    // short of the normal form and unifying both merges the two lists.
    for src in [mixed_bullets, mixed_delimiters] {
        let c = check(src, &opts()).expect("spans convert");
        assert!(c.is_normal(), "{:?}", c.departures().collect::<Vec<_>>());
        let exempt: Vec<_> = c.exempt().map(|(rule, _)| rule).collect();
        assert_eq!(exempt, vec!["markers", "markers"], "on {src:?}");
        assert_eq!(
            c.declined().count(),
            0,
            "no rule declines the whole of {src:?}"
        );
    }

    // Whole-document declinations: the gap rule refuses its own rewrite, on
    // two different shapes and for the same stated reason.
    for src in [promoted, fence] {
        let c = check(src, &opts()).expect("spans convert");
        assert!(c.is_normal(), "{:?}", c.departures().collect::<Vec<_>>());
        let declined: Vec<_> = c.declined().map(|(rule, _)| rule).collect();
        assert_eq!(declined, vec!["gaps"], "the gaps rule must refuse {src:?}");
    }

    // And the marker rule's own whole-document declination, which no adjacency
    // check could have reached: `- - -` is a thematic break.
    let c = check(nested_break, &opts()).expect("spans convert");
    assert!(c.is_normal(), "{:?}", c.departures().collect::<Vec<_>>());
    let declined: Vec<_> = c.declined().map(|(rule, _)| rule).collect();
    assert_eq!(declined, vec!["markers"]);
}

// --------------------------------------------------- proving the suite reds --
// Every test below states a formatter that would fail, or a normal form the
// real one must not produce.

fn identity(source: &str) -> String {
    source.to_string()
}

#[test]
fn the_identity_formatter_fails_this_suite() {
    let lost: Vec<&str> = FIXTURES
        .iter()
        .filter(|f| identity(utf8(f.input)) != utf8(f.expected))
        .map(|f| f.name)
        .collect();
    assert_eq!(
        lost.len(),
        FIXTURES.iter().filter(|f| f.discriminating()).count(),
        "`discriminating` must mean exactly `identity` fails it"
    );
    assert!(
        lost.len() >= 20,
        "only {} fixtures can tell `format` from `identity`: {lost:#?}",
        lost.len()
    );
    // And the converse, so the count above cannot be inflated by a fixture
    // that merely differs: every discriminating fixture is one the real
    // formatter gets right, which is what makes `identity`'s failure on it a
    // defect rather than a disagreement.
    for f in FIXTURES.iter().filter(|f| f.discriminating()) {
        assert_eq!(
            formatted(utf8(f.input)).output,
            utf8(f.expected),
            "{}",
            f.name
        );
    }
}

#[test]
fn only_the_declining_fixtures_leave_their_input_alone() {
    let passive: Vec<&str> = FIXTURES
        .iter()
        .filter(|f| !f.discriminating())
        .map(|f| f.name)
        .collect();
    assert_eq!(
        passive,
        vec![
            "declined: a ragged table is left verbatim",
            "declined: a rewrite that would promote `---` into front matter",
            "declined: an unterminated fence whose literal holds the trailing blank lines",
            "declined: two adjacent lists with different bullets are left verbatim",
            "declined: two adjacent ordered lists with different delimiters are left verbatim",
            "declined: a bullet change that would turn nested lists into a thematic break",
        ],
        "a fixture that leaves its input alone must say why in its name"
    );
}

#[test]
fn padding_the_trailing_column_is_not_the_normal_form() {
    let src = utf8(b"| a | b |\n| --- | --- |\n| 1 | 2 |\n");
    let uncapped = utf8(b"| a   | b   |\n| --- | --- |\n| 1   | 2   |\n");
    let got = formatted(src).output;
    assert_ne!(
        got, uncapped,
        "the trailing unaligned column must lose its fill"
    );
    assert_eq!(got, utf8(b"| a   | b |\n| --- | --- |\n| 1   | 2 |\n"));
}

#[test]
fn a_right_aligned_trailing_column_keeps_its_fill() {
    let src = utf8(b"| a | b |\n| --- | ---: |\n| 1 | 2 |\n");
    assert_eq!(
        formatted(src).output,
        utf8(b"| a   |   b |\n| --- | --: |\n| 1   |   2 |\n")
    );
}

#[test]
fn a_column_narrower_than_three_is_not_the_normal_form() {
    let src = utf8(b"| a | b |\n| - | - |\n| 1 | 2 |\n");
    let unfloored = utf8(b"| a | b |\n| - | - |\n| 1 | 2 |\n");
    let got = formatted(src).output;
    assert_ne!(got, unfloored, "the floor of 3 must widen this table");
    assert_eq!(got, utf8(b"| a   | b |\n| --- | --- |\n| 1   | 2 |\n"));
}

#[test]
fn character_count_is_not_the_width_measure() {
    let src = utf8(b"| f | note |\n| --- | --- |\n| \xF0\x9F\x8E\x89\xF0\x9F\x8E\x89 | done |\n");
    let by_chars =
        utf8(b"| f   | note |\n| --- | ---- |\n| \xF0\x9F\x8E\x89\xF0\x9F\x8E\x89 | done |\n");
    let got = formatted(src).output;
    assert_ne!(
        got, by_chars,
        "the width must be terminal columns, not chars"
    );
    assert_eq!(
        got,
        utf8(b"| f    | note |\n| ---- | ---- |\n| \xF0\x9F\x8E\x89\xF0\x9F\x8E\x89 | done |\n")
    );
}

#[test]
fn an_ascii_cell_of_the_same_length_cannot_discriminate_the_measures() {
    let src = utf8(b"| f | note |\n| --- | --- |\n| xy | done |\n");
    assert_eq!(
        formatted(src).output,
        utf8(b"| f   | note |\n| --- | ---- |\n| xy  | done |\n")
    );
}

#[test]
fn more_than_one_blank_line_between_blocks_is_not_the_normal_form() {
    let src = utf8(b"# H\n\n\n\npara\n");
    assert_ne!(formatted(src).output, utf8(b"# H\n\n\npara\n"));
    assert_eq!(formatted(src).output, utf8(b"# H\n\npara\n"));
}

#[test]
fn preserving_a_span_interior_crlf_is_not_the_normal_form() {
    let src = utf8(b"first\r\nsecond\r\n\r\n\r\ntail\r\n");
    let mixed = utf8(b"first\r\nsecond\n\ntail\n");
    let got = formatted(src).output;
    assert_ne!(got, mixed, "the output must not mix two line endings");
    assert_eq!(got, utf8(b"first\nsecond\n\ntail\n"));
    assert!(
        !got.contains('\r'),
        "no formatted output may hold a carriage return"
    );
}

#[test]
fn the_same_document_with_lf_endings_reaches_the_same_normal_form() {
    let src = utf8(b"first\nsecond\n\n\ntail\n");
    assert_eq!(formatted(src).output, utf8(b"first\nsecond\n\ntail\n"));
}

#[test]
fn check_reports_a_crlf_file_as_departing_from_normal_form() {
    let src = utf8(b"# Title\r\n\r\nfirst\r\nsecond\r\n");
    let c = check(src, &opts()).expect("spans convert");
    assert!(!c.is_normal(), "a CRLF file is not in normal form");
    // One departure per ending, in the source's own coordinates: L1:8 is the
    // `\r` after `# Title`, L2:1 the blank line's, L3:6 after `first`, L4:7
    // after `second`.
    assert_eq!(
        c.departures()
            .filter(|(rule, _)| *rule == "endings")
            .map(|(_, d)| (d.line, d.column))
            .collect::<Vec<_>>(),
        vec![(1, 8), (2, 1), (3, 6), (4, 7)]
    );
    assert_eq!(formatted(src).output, utf8(b"# Title\n\nfirst\nsecond\n"));
}

#[test]
fn check_faults_a_file_whose_only_crlf_no_other_rule_can_reach() {
    let src = utf8(b"first\r\nsecond\n");
    let c = check(src, &opts()).expect("spans convert");
    assert!(!c.is_normal());
    let faulting: Vec<&str> = c
        .rules
        .iter()
        .filter(|r| !r.is_normal())
        .map(|r| r.rule)
        .collect();
    assert_eq!(faulting, vec!["endings"]);
    assert_eq!(
        c.departures()
            .map(|(_, d)| (d.line, d.column))
            .collect::<Vec<_>>(),
        vec![(1, 6)]
    );
    assert_eq!(formatted(src).output, utf8(b"first\nsecond\n"));
}

#[test]
fn a_star_is_not_the_normal_form_bullet() {
    let src = utf8(b"+ alpha\n+ beta\n");
    let starred = utf8(b"* alpha\n* beta\n");
    let got = formatted(src).output;
    assert_ne!(got, starred, "the normal form bullet is `-`, not `*`");
    assert_eq!(got, utf8(b"- alpha\n- beta\n"));
}

#[test]
fn renumbering_an_ordered_list_is_not_the_normal_form() {
    let src = utf8(b"1) a\n1) b\n");
    let renumbered = utf8(b"1. a\n2. b\n");
    let got = formatted(src).output;
    assert_ne!(got, renumbered, "the ordinals must be left alone");
    assert_eq!(got, utf8(b"1. a\n1. b\n"));
    // And from the other side: a list that does not start at 1 keeps its start.
    assert_eq!(
        formatted(utf8(b"3) three\n7) seven\n")).output,
        utf8(b"3. three\n7. seven\n")
    );
}

#[test]
fn merging_two_adjacent_lists_is_not_the_normal_form() {
    let src = utf8(b"* alpha\n\n+ beta\n");
    let merged = utf8(b"- alpha\n\n- beta\n");
    let got = formatted(src).output;
    assert_ne!(got, merged, "unifying the pair would splice two lists");
    assert_eq!(got, src);
}

#[test]
fn two_mixed_lists_that_are_not_neighbours_are_both_unified() {
    let src = utf8(b"* alpha\n\npara\n\n+ beta\n");
    assert_eq!(formatted(src).output, utf8(b"- alpha\n\npara\n\n- beta\n"));
}

#[test]
fn check_reports_a_star_bullet_as_departing_from_normal_form() {
    let src = utf8(b"* outer\n  * inner\n* tail\n");
    let c = check(src, &opts()).expect("spans convert");
    assert!(!c.is_normal(), "a `*` bullet is not in normal form");
    assert_eq!(
        c.departures()
            .filter(|(rule, _)| *rule == "markers")
            .map(|(_, d)| (d.line, d.column))
            .collect::<Vec<_>>(),
        vec![(1, 1), (2, 3), (3, 1)]
    );
    assert_eq!(formatted(src).output, utf8(b"- outer\n  - inner\n- tail\n"));
}

#[test]
fn a_document_already_using_the_normal_form_markers_is_untouched() {
    let src = utf8(b"- a\n- b\n\n1. one\n2. two\n");
    let c = check(src, &opts()).expect("spans convert");
    assert!(c.is_normal(), "{:?}", c.departures().collect::<Vec<_>>());
    assert_eq!(formatted(src).output, src);
}

#[test]
fn no_blank_line_after_front_matter_is_not_the_normal_form() {
    let src = utf8(b"---\nk: v\n---\n\n\n# H\n\nbody\n");
    assert_ne!(
        formatted(src).output,
        utf8(b"---\nk: v\n---\n# H\n\nbody\n")
    );
    assert_eq!(
        formatted(src).output,
        utf8(b"---\nk: v\n---\n\n# H\n\nbody\n")
    );
}
