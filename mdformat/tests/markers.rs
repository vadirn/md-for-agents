//! The list-marker rule's guards, held open against deliberately wrong
//! unifiers.

use mdformat::{
    ListSkipReason, MarkerViolation, RuleRun, Structure, check, marker_violation, structure_of,
    unify,
};

fn opts() -> mdstruct::Options {
    mdstruct::Options::default()
}

fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("fixtures are UTF-8")
}

fn structure(source: &str) -> Structure {
    structure_of(source, &opts())
}

fn markers_row(source: &str) -> RuleRun {
    let c = check(source, &opts()).expect("spans convert");
    c.rules
        .iter()
        .find(|r| r.rule == "markers")
        .expect("the marker rule is in RULES")
        .clone()
}

fn correct(source: &str) -> String {
    unify(source, &opts())
        .expect("spans convert")
        .accepted()
        .expect("the real unifier must clear its own guards")
        .to_string()
}

fn assert_the_pair_is_reported_exempt(source: &str) {
    let u = unify(source, &opts()).expect("spans convert");
    assert!(
        u.structure.is_none() && u.violation.is_none(),
        "{source:?}: a whole-document guard fired, so the per-construct \
         declination is not what left this document alone: {:?} {:?}",
        u.structure.as_ref().map(|d| d.to_string()),
        u.violation.as_ref().map(|v| v.to_string()),
    );
    let pair: Vec<(usize, char, char, usize)> = u
        .skipped
        .iter()
        .map(|s| match s.reason {
            ListSkipReason::MixedAdjacent {
                neighbour,
                here,
                there,
            } => (s.line, here, there, neighbour),
            ref other => panic!(
                "{source:?}: the list at line {} was declined for the wrong reason: {other}",
                s.line
            ),
        })
        .collect();
    assert_eq!(
        pair.len(),
        2,
        "{source:?}: both members of the pair must be reported, got {pair:?}"
    );
    let (a, b) = (pair[0], pair[1]);
    assert_eq!(
        (a.3, b.3),
        (b.0, a.0),
        "{source:?}: the two exemptions must name each other's line, got {pair:?}"
    );
    assert_eq!(
        (a.1, a.2),
        (b.2, b.1),
        "{source:?}: each exemption must read the pair's markers from its own \
         side, got {pair:?}"
    );
    assert_eq!(
        u.accepted(),
        Some(source),
        "the real rule must leave {source:?} verbatim"
    );

    let r = markers_row(source);
    assert!(
        r.is_normal(),
        "{source:?}: a document whose only fault is exempt is in normal form"
    );
    assert_eq!(
        r.departures(),
        &[],
        "{source:?}: a construct the rule declined produces no departure"
    );
    assert!(
        r.declined.is_none(),
        "{source:?}: the rule declined the whole document: {:?}",
        r.declined
    );
    assert_eq!(
        r.exempt.len(),
        2,
        "{source:?}: both members must reach the report, got {:?}",
        r.exempt
    );
    for e in &r.exempt {
        assert!(
            e.why.contains("merge them into one list"),
            "{source:?}: the exemption must state why: {e:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Guard 1: re-parse structural equivalence, less the marker signature
// ---------------------------------------------------------------------------

#[test]
fn the_structure_oracle_rejects_the_merge_the_declination_prevents() {
    // (input, what a unifier without the declination would emit)
    let merges: &[(&[u8], &[u8])] = &[
        // Two top-level bullet lists, tight.
        (b"- a\n+ b\n", b"- a\n- b\n"),
        // The same pair loose, which is the shape the fixture suite carries.
        (b"* alpha\n\n+ beta\n", b"- alpha\n\n- beta\n"),
        // Ordered, where the delimiter rather than the bullet decides it.
        (b"1. one\n\n1) two\n", b"1. one\n\n1. two\n"),
        // Two sublists of one item: a check that looked only at top-level
        // blocks would miss this entirely.
        (b"- a\n  - b\n  * c\n", b"- a\n  - b\n  - c\n"),
        // And inside a block quote.
        (b"> - q\n> + r\n", b"> - q\n> - r\n"),
    ];
    for (input, merged) in merges {
        let (input, merged) = (utf8(input), utf8(merged));
        let diff = structure(input)
            .diff_ignoring_markers(&structure(merged))
            .unwrap_or_else(|| panic!("the oracle passed a merge of {input:?} into {merged:?}"));
        assert!(
            !diff.kinds_same,
            "the merge of {input:?} must show as a kinds difference, got {diff}"
        );
        // The causal control: the same input left alone is accepted, so the
        // rejection above is about the merge, not the specimen. Read from the
        // report, since a declined document is byte-indistinguishable from one
        // the rule had nothing to do in.
        assert_the_pair_is_reported_exempt(input);
    }
}

#[test]
fn the_same_oracle_accepts_a_marker_change_that_merges_nothing() {
    for src in [
        &b"* a\n* b\n"[..],
        &b"1) a\n2) b\n"[..],
        &b"* outer\n  * inner\n* tail\n"[..],
        &b"* [ ] todo\n* [x] done\n"[..],
        &b"> * quoted\n"[..],
    ] {
        let src = utf8(src);
        let out = correct(src);
        assert_ne!(out, src, "{src:?} must actually be rewritten");
        assert_eq!(
            structure(src).diff_ignoring_markers(&structure(&out)),
            None,
            "the oracle refused a rewrite that merges nothing: {src:?}"
        );
    }
}

#[test]
fn the_unexempt_oracle_still_sees_the_marker_change() {
    let src = utf8(b"* a\n* b\n");
    let out = correct(src);
    let diff = structure(src)
        .diff(&structure(&out))
        .expect("the full oracle must see a marker change");
    assert!(!diff.markers_same, "{diff}");
    assert!(diff.kinds_same && diff.rich_same && diff.html_same && diff.tables_same);
}

#[test]
fn a_unifier_that_rewrites_a_star_in_running_text_is_rejected() {
    let src = utf8(b"* item\n\npara with *emphasis* in it\n");
    let naive = utf8(b"- item\n\npara with -emphasis- in it\n");
    assert_eq!(
        marker_violation(src, naive),
        None,
        "the substitution oracle cannot see this one, which is why guard 1 exists"
    );
    let diff = structure(src)
        .diff_ignoring_markers(&structure(naive))
        .expect("the oracle was expected to reject this unifier");
    assert!(!diff.html_same, "{diff}");
    // The causal control: the real rule changes the bullet and leaves the
    // emphasis alone.
    assert_eq!(
        correct(src),
        utf8(b"- item\n\npara with *emphasis* in it\n")
    );
}

#[test]
fn a_unifier_that_reaches_into_a_code_block_is_rejected() {
    let src = utf8(b"* item\n\n```\n* not a list\n```\n");
    let naive = utf8(b"- item\n\n```\n- not a list\n```\n");
    assert_eq!(marker_violation(src, naive), None);
    let diff = structure(src)
        .diff_ignoring_markers(&structure(naive))
        .expect("the oracle was expected to reject this unifier");
    assert!(!diff.rich_same, "{diff}");
    assert_eq!(correct(src), utf8(b"- item\n\n```\n* not a list\n```\n"));
}

// ---------------------------------------------------------------------------
// Guard 2: the substitution oracle
// ---------------------------------------------------------------------------

#[test]
fn only_the_substitution_oracle_can_see_which_marker_was_chosen() {
    // `-` is the normal form, so nothing may leave it.
    let src = utf8(b"- a\n- b\n");
    let starred = utf8(b"* a\n* b\n");
    assert_eq!(
        structure(src).diff_ignoring_markers(&structure(starred)),
        None,
        "guard 1 is exempt from exactly this, which is why guard 2 exists"
    );
    assert_eq!(
        marker_violation(src, starred),
        Some(MarkerViolation::Substitution {
            line: 1,
            column: 1,
            before: '-',
            after: '*',
        })
    );

    // And the same for a unifier that moved between the two wrong bullets.
    let plus = utf8(b"+ a\n");
    let star = utf8(b"* a\n");
    assert_eq!(
        structure(plus).diff_ignoring_markers(&structure(star)),
        None
    );
    assert!(matches!(
        marker_violation(plus, star),
        Some(MarkerViolation::Substitution {
            before: '+',
            after: '*',
            ..
        })
    ));
    assert_eq!(correct(plus), utf8(b"- a\n"));
}

#[test]
fn a_unifier_that_also_renumbers_is_rejected_by_both_guards() {
    let src = utf8(b"1) a\n1) b\n");
    let renumbering = utf8(b"1. a\n2. b\n");
    let diff = structure(src)
        .diff_ignoring_markers(&structure(renumbering))
        .expect("guard 1 must reject a renumbering");
    assert!(!diff.rich_same, "{diff}");
    assert_eq!(
        marker_violation(src, renumbering),
        Some(MarkerViolation::Substitution {
            line: 2,
            column: 1,
            before: '1',
            after: '2',
        })
    );
    assert_eq!(correct(src), utf8(b"1. a\n1. b\n"));
}

#[test]
fn a_unifier_that_also_reindents_is_rejected_by_both_guards() {
    let src = utf8(b"* outer\n  * inner\n");
    let reindented = utf8(b"- outer\n    - inner\n");
    let diff = structure(src)
        .diff_ignoring_markers(&structure(reindented))
        .expect("guard 1 must reject a reindent");
    assert!(!diff.rich_same, "{diff}");
    assert_eq!(
        marker_violation(src, reindented),
        Some(MarkerViolation::Length {
            before: 18,
            after: 20
        })
    );
    assert_eq!(correct(src), utf8(b"- outer\n  - inner\n"));
}

// ---------------------------------------------------------------------------
// The rule itself
// ---------------------------------------------------------------------------

#[test]
fn unification_is_idempotent() {
    for src in [
        &b"* a\n+ b\n"[..],
        &b"* outer\n  + inner\n\n1) one\n"[..],
        &b"* alpha\n\n+ beta\n"[..],
        &b"+ + +\n"[..],
        &b"- a\n- b\n"[..],
        &b""[..],
    ] {
        let src = utf8(src);
        let once = unify(src, &opts()).expect("spans convert");
        let first = once.accepted().unwrap_or(src).to_string();
        let twice = unify(&first, &opts()).expect("spans convert");
        assert_eq!(
            twice.accepted().unwrap_or(&first),
            first,
            "the second pass changed the first's output for {src:?}"
        );
    }
}

#[test]
fn an_already_normal_document_is_reported_normal_by_the_rule() {
    for src in [
        &b"- a\n- b\n"[..],
        &b"1. one\n2. two\n"[..],
        &b"- outer\n  - inner\n  - sibling\n"[..],
        &b"- [x] done\n- [ ] todo\n"[..],
        &b"> - q\n> - r\n"[..],
        &b"- bullet\n\n1. ordered\n"[..],
        &b"- a\n\n1. one\n   - nested\n"[..],
    ] {
        let src = utf8(src);
        let u = unify(src, &opts()).expect("spans convert");
        assert!(
            !u.changed(),
            "{src:?}: the rule claims a marker to change: {:?}",
            u.changes
        );
        assert_eq!(u.changes, vec![], "{src:?}");
        assert_eq!(
            u.skipped,
            vec![],
            "{src:?}: an already-normal list must be found normal, not declined"
        );
        assert_eq!(u.accepted(), Some(src), "{src:?}");

        let r = markers_row(src);
        assert!(r.is_normal(), "{src:?}: the rule calls it abnormal");
        assert_eq!(
            r.departures(),
            &[],
            "{src:?}: a departure was reported where the markers are already \
             the normal form's"
        );
        assert!(r.declined.is_none(), "{src:?}: {:?}", r.declined);
        assert_eq!(
            r.exempt,
            vec![],
            "{src:?}: nothing here is declined, so nothing here is exempt"
        );
        assert_eq!(r.yielded(), src, "{src:?}");
        assert_eq!(r.accepted(), Some(src), "{src:?}");
    }
}

#[test]
fn a_declined_pair_leaves_the_rest_of_the_document_formattable() {
    let src = utf8(b"* alone\n\npara\n\n* mixed\n\n+ pair\n");
    let u = unify(src, &opts()).expect("spans convert");
    assert_eq!(u.skipped.len(), 2, "both members of the pair are exempt");
    assert_eq!(
        u.accepted(),
        Some(utf8(b"- alone\n\npara\n\n* mixed\n\n+ pair\n"))
    );
}

#[test]
fn a_realistic_document_is_unified_in_one_pass() {
    let src = utf8(
        b"# Title\n\n\
          Intro with *emphasis*.\n\n\
          + first\n\
          + second\n\
          \x20 * nested\n\
          + [x] done\n\n\
          1) step one\n\
          2) step two\n",
    );
    assert_eq!(
        correct(src),
        utf8(
            b"# Title\n\n\
              Intro with *emphasis*.\n\n\
              - first\n\
              - second\n\
              \x20 - nested\n\
              - [x] done\n\n\
              1. step one\n\
              2. step two\n",
        )
    );
}
