//! What stands in for the oracle the endings rule cannot have.

use mdformat::{RuleRun, Structure, check, structure_of, to_lf};

fn opts() -> mdstruct::Options {
    mdstruct::Options::default()
}

fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("specimens are UTF-8")
}

/// Documents whose parse the rewrite must not disturb, one per block shape the
/// crate has a story about.
const SPECIMENS: &[(&str, &[u8])] = &[
    ("blocks", b"# H\r\n\r\npara one\r\n\r\n- a\r\n- b\r\n"),
    ("front-matter", b"---\r\ntitle: x\r\n---\r\n\r\n# H\r\n"),
    ("lone-cr", b"a\r## H\rbody\r"),
    ("fenced-code", b"```\r\ncode   \r\n\r\nmore\r\n```\r\n"),
    ("indented-code", b"    indented\r\n    code\r\n"),
    (
        "html-block",
        b"<div>\r\n  <p>x</p>\r\n</div>\r\n\r\nafter\r\n",
    ),
    ("table", b"| a | b |\r\n| --- | --- |\r\n| 1 | 2 |\r\n"),
    (
        "ragged-table",
        b"| a | b | c |\r\n| --- | --- | --- |\r\n| 1 | 2 |\r\n",
    ),
    ("block-quote", b"> quoted\r\n> lines\r\n\r\nafter\r\n"),
    ("hard-break", b"first  \r\nsecond\r\n"),
    ("setext", b"Title\r\n=====\r\n\r\nbody\r\n"),
    ("code-span", b"para with `co\r\nde` span\r\n"),
    (
        "link-reference-definition",
        b"[label]: https://example.com\r\n\r\nSee [label].\r\n",
    ),
    ("task-list", b"- [x] done\r\n- [ ] todo\r\n"),
    ("backslash-break", b"text\\\r\nbreak\r\n"),
    ("mixed", b"lf\ncrlf\r\ncr\rend\n"),
    ("bom", b"\xEF\xBB\xBF# H\r\n\r\nbody\r\n"),
];

/// Documents that already hold the normal form's one line ending, one per block
/// shape [`SPECIMENS`] covers. Every specimen there is built around a `\r`, so
/// without these the fixpoint clause has nothing to run on.
const LF_CLEAN: &[(&str, &[u8])] = &[
    ("blocks", b"# H\n\npara one\n\n- a\n- b\n"),
    ("front-matter", b"---\ntitle: x\n---\n\n# H\n"),
    ("fenced-code", b"```\ncode\n\nmore\n```\n"),
    ("indented-code", b"    indented\n    code\n"),
    ("html-block", b"<div>\n  <p>x</p>\n</div>\n\nafter\n"),
    ("table", b"| a   | b   |\n| --- | --- |\n| 1   | 2   |\n"),
    ("block-quote", b"> quoted\n> lines\n\nafter\n"),
    ("hard-break", b"first  \nsecond\n"),
    ("setext", b"Title\n=====\n\nbody\n"),
    ("code-span", b"para with `co\nde` span\n"),
    ("task-list", b"- [x] done\n- [ ] todo\n"),
    ("empty", b""),
    ("no-final-newline", b"no trailing newline"),
];

fn signatures(source: &str) -> Structure {
    structure_of(source, &opts())
}

fn endings_row(source: &str) -> RuleRun {
    let c = check(source, &opts()).expect("spans convert");
    c.rules
        .iter()
        .find(|r| r.rule == "endings")
        .expect("the endings rule is in RULES")
        .clone()
}

fn assert_reported_normal(name: &str, source: &str) {
    let r = endings_row(source);
    assert!(
        r.is_normal(),
        "{name}: the rule calls an LF-clean document abnormal"
    );
    assert_eq!(
        r.departures(),
        &[],
        "{name}: a departure was reported where there is no `\\r`"
    );
    assert!(r.declined.is_none(), "{name}: this rule declines nothing");
    assert!(r.exempt.is_empty(), "{name}: this rule exempts nothing");
    assert_eq!(
        r.yielded(),
        source,
        "{name}: the rule did not pass it through"
    );
    assert_eq!(
        r.accepted(),
        Some(source),
        "{name}: an unchanged document is still an accepted one"
    );
}

fn crs_read_as_html_reads_them(s: &str) -> String {
    to_lf(s).output
}

#[test]
fn the_block_skeleton_and_every_table_shape_survive_identically() {
    for (name, input) in SPECIMENS {
        let src = utf8(input);
        let before = signatures(src);
        let after = signatures(&to_lf(src).output);
        assert_eq!(before.kinds, after.kinds, "{name}: block skeleton changed");
        assert_eq!(before.tables, after.tables, "{name}: table shape changed");
    }
}

#[test]
fn the_rendered_document_survives_once_the_html_is_read_as_html() {
    let changed: Vec<&str> = SPECIMENS
        .iter()
        .filter(|(_, input)| {
            let src = utf8(input);
            crs_read_as_html_reads_them(&signatures(src).html)
                != signatures(&to_lf(src).output).html
        })
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        changed,
        vec!["code-span"],
        "only the code span may render differently — see \
         `the_one_render_the_rewrite_changes_it_repairs`"
    );
}

#[test]
fn the_one_render_the_rewrite_changes_it_repairs() {
    let crlf = utf8(b"para with `co\r\nde` span\r\n");
    let lf = to_lf(crlf).output;
    assert_eq!(
        signatures(crlf).html,
        "<p>para with <code>co\r de</code> span</p>\n",
        "comrak leaves the CR in the code span's text"
    );
    assert_eq!(
        signatures(&lf).html,
        "<p>para with <code>co de</code> span</p>\n",
        "one line ending becomes one space, as CommonMark specifies"
    );
}

#[test]
fn the_structure_oracle_refuses_this_rewrite() {
    let refused: Vec<&str> = SPECIMENS
        .iter()
        .filter(|(_, input)| {
            let src = utf8(input);
            signatures(src)
                .diff(&signatures(&to_lf(src).output))
                .is_some()
        })
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        refused,
        vec![
            "front-matter",
            "fenced-code",
            "indented-code",
            "html-block",
            "code-span"
        ],
        "the set of specimens the oracle refuses has moved"
    );
}

#[test]
fn an_oracle_blind_to_line_endings_is_silent_by_construction() {
    for (name, input) in SPECIMENS {
        let src = utf8(input);
        // Blind both sides by canonicalizing before parsing. On the right this
        // is a no-op; on the left it is the rewrite itself. There is nothing
        // left for the comparison to see.
        let blinded = signatures(&to_lf(src).output);
        let after = signatures(&to_lf(&to_lf(src).output).output);
        assert_eq!(blinded.diff(&after), None, "{name}");
    }
}

#[test]
fn no_output_holds_a_carriage_return() {
    for (name, input) in SPECIMENS {
        let once = to_lf(utf8(input)).output;
        assert!(!once.contains('\r'), "{name}: a CR survived");
        assert_eq!(to_lf(&once).output, once, "{name}: not a fixpoint");
        assert_reported_normal(name, &once);
    }
}

#[test]
fn an_lf_clean_document_is_already_normal_for_this_rule() {
    for (name, input) in LF_CLEAN {
        let src = utf8(input);
        assert!(
            !src.contains('\r'),
            "{name}: an LF-clean specimen must hold no carriage return"
        );
        let e = to_lf(src);
        assert_eq!(
            e.changes,
            vec![],
            "{name}: an ending was reported changed where every ending is already LF"
        );
        assert!(!e.changed(), "{name}: the rewrite claims to have moved");
        assert_eq!(
            e.output, src,
            "{name}: the rewrite is not the identity here"
        );
        assert_reported_normal(name, src);
    }
}
