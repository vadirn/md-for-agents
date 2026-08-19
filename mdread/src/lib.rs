//! A progressive-unfolding structured Markdown reader.
//!
//! Renders a file's heading tree folded to one line per section, or unfolds one
//! addressed section. Structure comes from [`mdstruct`]; the fold thresholds,
//! token estimate and address grammar are this crate's own.
//!
//! Reading and printing are separate concerns here. [`read_file`] and
//! [`read_content`] resolve an address to a [`Reading`] and return it;
//! [`render::print`] is the only thing that turns one into terminal output. A
//! caller that wants the data takes the value and never goes through stdout,
//! and the fold threshold arrives as a parameter, so the defaults belong to
//! whichever binary sets them.

mod facet;
mod frontmatter;
mod model;
mod reading;
pub mod render;
mod resolve;
mod shadow;
mod slug;
mod unfold;
mod wikilink;

use std::path::Path;

use anyhow::Result;

// Re-exported so a caller naming the output format does not also depend on the
// crate it comes from.
pub use cli::TextJson;
pub use facet::{HeadingRule, LinkRule};
pub use reading::{
    Frontmatter, FrontmatterField, FrontmatterValue, Link, Links, Overview, Reading, TextNode,
    TreeNode, Unfold, UnfoldChild,
};

use model::{Document, Node, node_tokens, parse_document_with, range_lines, range_slice};
use resolve::resolve_address;
use shadow::{FmAddress, Reserved, reserved_reading};
use unfold::{own_prose, unfold_child, unfold_content_string};

/// The Markdown flavour a caller reads in: the two places where a defensible
/// reading of the same bytes differs. [`Default`] is plain CommonMark; a caller
/// with a stricter corpus overrides both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Dialect {
    pub headings: HeadingRule,
    pub links: LinkRule,
}

/// Read `file`: a folded overview when `address` is `None`, or the smart-unfolded
/// addressed section otherwise.
pub fn read_file(
    file: &Path,
    address: Option<&str>,
    depth: Option<usize>,
    full: bool,
    threshold: usize,
    dialect: Dialect,
) -> Result<Reading> {
    let content = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", file.display(), e))?;
    read_content(
        &file.display().to_string(),
        &content,
        address,
        depth,
        full,
        threshold,
        dialect,
    )
}

/// Read already-loaded `content`, labelled `display_path` in the result. Lets a
/// caller read from stdin or an in-memory buffer.
pub fn read_content(
    display_path: &str,
    content: &str,
    address: Option<&str>,
    depth: Option<usize>,
    full: bool,
    threshold: usize,
    dialect: Dialect,
) -> Result<Reading> {
    let doc = parse_document_with(content, dialect.headings);

    let Some(addr) = address else {
        return Ok(Reading::Overview(read_overview(
            display_path,
            content,
            &doc,
            dialect.links,
        )));
    };

    // Reserved addresses are matched before the heading tree, since they name
    // parts of the file the tree cannot reach. A heading that slugs to one of
    // them stays reachable by its numeric address, and the collision is
    // announced rather than resolved.
    //
    // The announcement rides on the value only when the reading succeeds: a
    // failed reading names the shadow in its own error instead.
    match reserved_reading(addr) {
        Some(Reserved::Fm(which)) => read_frontmatter(display_path, content, &doc, addr, which),
        Some(Reserved::Links) => {
            let mut links = read_links(display_path, content, addr, dialect.links);
            links.note = shadow::phrase(&doc, addr);
            Ok(Reading::Links(links))
        }
        Some(Reserved::Text) => {
            let mut section = read_section(display_path, &doc, addr, depth, full, threshold)?;
            section.note = shadow::phrase(&doc, addr);
            Ok(Reading::Unfold(section))
        }
        None => Ok(Reading::Unfold(read_section(
            display_path,
            &doc,
            addr,
            depth,
            full,
            threshold,
        )?)),
    }
}

fn read_frontmatter(
    display_path: &str,
    content: &str,
    doc: &Document,
    address: &str,
    which: FmAddress<'_>,
) -> Result<Reading> {
    let Some(text) = frontmatter::block_text(content) else {
        // The address resolved to nothing, and a heading may be the thing the
        // caller meant. Name it and its numeric address rather than letting the
        // reserved name look like the file's last word.
        let mut msg = format!("No frontmatter block in this file (address '{}')", address);
        if let Some(p) = shadow::phrase(doc, address) {
            msg.push_str(&format!("; {}", p));
        }
        return Err(anyhow::anyhow!(msg));
    };

    match which {
        // A value is navigated over the parsed YAML rather than the line scan, so
        // `references[0].target` reaches inside a nested list the same way a bare
        // `type` reaches a top-level key.
        FmAddress::Path(path) => {
            let root = frontmatter::parsed(content)
                .expect("block_text present implies a complete block")
                .map_err(|e| anyhow::anyhow!(e))?;
            let value = frontmatter::value_at(&root, path).map_err(|e| {
                anyhow::anyhow!(
                    "{}; top-level fields: {}",
                    e,
                    frontmatter::field_order(content).join(", ")
                )
            })?;
            Ok(Reading::FrontmatterValue(FrontmatterValue {
                path: display_path.to_string(),
                address: address.to_string(),
                value: value.clone(),
            }))
        }
        FmAddress::Block => {
            let (start, end) = frontmatter::block_line_range(content).unwrap_or((1, 0));
            let fields = frontmatter::fields_with_values(content)
                .into_iter()
                .map(|f| FrontmatterField {
                    key: f.key,
                    value: f.value,
                    line: f.line,
                })
                .collect();
            Ok(Reading::Frontmatter(Frontmatter {
                path: display_path.to_string(),
                address: address.to_string(),
                line: start,
                lines: range_lines(start, end),
                fields,
                text,
                note: shadow::phrase(doc, address),
            }))
        }
    }
}

/// `links` address: the outgoing links the overview only counted.
fn read_links(display_path: &str, content: &str, address: &str, rule: LinkRule) -> Links {
    Links {
        path: display_path.to_string(),
        address: address.to_string(),
        links: facet::links(content, rule)
            .into_iter()
            .map(|l| Link {
                kind: l.kind,
                target: l.target,
                alias: l.alias,
                line: l.line,
            })
            .collect(),
        note: None,
    }
}

fn read_overview(
    display_path: &str,
    content: &str,
    doc: &Document,
    link_rule: LinkRule,
) -> Overview {
    let text = doc.text.as_ref().map(|t| TextNode {
        address: "0".to_string(),
        label: "(text)".to_string(),
        line: t.line,
        lines: range_lines(t.start, t.end),
        tokens: cli::estimate_tokens(&range_slice(&doc.lines, t.start, t.end).unwrap_or_default()),
    });
    Overview {
        path: display_path.to_string(),
        // Scan the raw frontmatter block for top-level keys in source order, so
        // the listing reflects the file rather than an alphabetization.
        fields: frontmatter::field_order(content),
        links: facet::link_count(content, link_rule),
        text,
        tree: doc.tree.iter().map(|n| tree_node(n, &doc.lines)).collect(),
        notes: shadow::overview_notes(doc),
    }
}

fn tree_node(n: &Node, lines: &[&str]) -> TreeNode {
    TreeNode {
        address: n.address.clone(),
        heading: n.heading.clone(),
        level: n.level,
        line: n.line,
        lines: range_lines(n.start, n.end),
        tokens: cli::estimate_tokens(&range_slice(lines, n.start, n.end).unwrap_or_default()),
        slug: n.slug.clone(),
        children: n.children.iter().map(|c| tree_node(c, lines)).collect(),
    }
}

/// With-address path: smart-unfold the addressed node.
///
/// `content` is the node's own prose and `children` carries each child inlined
/// or folded; `text` is the whole section as one block. All three come from the
/// same walker, so they cannot disagree.
fn read_section(
    display_path: &str,
    doc: &Document,
    address: &str,
    depth: Option<usize>,
    full: bool,
    threshold: usize,
) -> Result<Unfold> {
    let n = resolve_address(doc, address)?;
    Ok(Unfold {
        path: display_path.to_string(),
        address: n.address.clone(),
        heading: n.heading.clone(),
        slug: n.slug.clone(),
        level: n.level,
        line: n.line,
        lines: range_lines(n.start, n.end),
        tokens: node_tokens(n, &doc.lines),
        content: own_prose(n, &doc.lines),
        children: n
            .children
            .iter()
            .map(|c| unfold_child(c, &doc.lines, 1, depth, threshold, full))
            .collect(),
        text: unfold_content_string(n, &doc.lines, 0, depth, threshold, full),
        note: None,
    })
}

#[cfg(test)]
mod tests {
    use crate::model::*;
    use crate::render::tree_line_string;
    use crate::resolve::{ResolveError, resolve, resolve_address};
    use crate::unfold::*;

    const SAMPLE: &str = "---\ntype: note\nslug: x\n---\n\nLede prose before any heading.\nSecond line of lede.\n\n# Direction\n\nDir body.\n\n## Sub one\n\nsub one body\n\n## Sub two\n\nsub two body\n\n# Glossary\n\ngloss body\n\n# Log & Notes\n\nfirst.\n\n# Log Notes\n\nsecond.\n";

    #[test]
    fn tree_shape_and_addresses() {
        let doc = parse_document(SAMPLE);
        // Top-level: Direction(1), Glossary(2), Log & Notes(3), Log Notes(4).
        assert_eq!(doc.tree.len(), 4);
        assert_eq!(doc.tree[0].address, "1");
        assert_eq!(doc.tree[0].heading, "Direction");
        assert_eq!(doc.tree[0].children.len(), 2);
        assert_eq!(doc.tree[0].children[0].address, "1.1");
        assert_eq!(doc.tree[0].children[0].heading, "Sub one");
        assert_eq!(doc.tree[0].children[1].address, "1.2");
        assert_eq!(doc.tree[1].address, "2");
        assert_eq!(doc.tree[1].heading, "Glossary");
        assert!(doc.tree[1].children.is_empty());
    }

    #[test]
    fn content_range_includes_descendants() {
        let doc = parse_document(SAMPLE);
        let dir = &doc.tree[0];
        // Direction starts at its heading line and ends just before Glossary.
        let glossary_line = doc.tree[1].line;
        assert_eq!(dir.end, glossary_line - 1);
        // The Direction range therefore spans its two subsections.
        assert!(dir.end > dir.children[1].line);
    }

    #[test]
    fn text_region_detected_and_trimmed() {
        let doc = parse_document(SAMPLE);
        let t = doc.text.as_ref().expect("text region present");
        // Lede starts at the first non-blank body line (line 6), ends before `# Direction`.
        assert_eq!(t.line, 6);
        let first_heading = doc.tree[0].line;
        assert_eq!(t.end, first_heading - 1);
    }

    #[test]
    fn headingless_whole_body_is_text() {
        let content = "---\ntype: note\n---\n\nJust body.\nNo headings here.\n";
        let doc = parse_document(content);
        assert!(doc.tree.is_empty());
        let t = doc.text.as_ref().expect("text region present");
        assert_eq!(t.line, 5);
    }

    #[test]
    fn no_text_region_when_heading_first() {
        let content = "# Only Heading\n\nbody\n";
        let doc = parse_document(content);
        assert!(doc.text.is_none());
        assert_eq!(doc.tree.len(), 1);
    }

    #[test]
    fn fenced_hash_is_not_a_heading() {
        let content = "# Real\n\n```\n# not a heading\n```\n\n## Sub\n";
        let doc = parse_document(content);
        assert_eq!(doc.tree.len(), 1);
        assert_eq!(doc.tree[0].address, "1");
        assert_eq!(doc.tree[0].children.len(), 1);
        assert_eq!(doc.tree[0].children[0].heading, "Sub");
    }

    #[test]
    fn range_slice_out_of_bounds_is_none() {
        // Out-of-range requests return None (an explicit guard) rather than a
        // silent empty string or a slice-index panic.
        let lines = ["a", "b", "c"];
        assert_eq!(range_slice(&lines, 0, 2), None); // start before line 1
        assert_eq!(range_slice(&lines, 4, 5), None); // start past EOF
        assert_eq!(range_slice(&lines, 2, 1), None); // inverted end < start
        assert_eq!(range_slice(&lines, 1, 2).as_deref(), Some("a\nb"));
    }

    #[test]
    fn numeric_resolution() {
        let doc = parse_document(SAMPLE);
        let n = resolve(&doc, "1.2").expect("1.2 resolves");
        assert_eq!(n.address, "1.2");
        assert_eq!(n.heading, "Sub two");
    }

    #[test]
    fn slug_resolution() {
        let doc = parse_document(SAMPLE);
        let n = resolve(&doc, "glossary").expect("glossary resolves");
        assert_eq!(n.address, "2");
    }

    #[test]
    fn text_resolution() {
        let doc = parse_document(SAMPLE);
        // `0` and `text` both resolve to the synthetic text node.
        for addr in ["0", "text"] {
            let n = resolve(&doc, addr).expect("text node resolves");
            assert_eq!(n.address, "0");
            assert_eq!(n.heading, "(text)");
            assert_eq!(n.slug, "text");
        }
    }

    #[test]
    fn numeric_overflow_is_out_of_range_not_panic() {
        // An all-digit address that overflows usize must report out-of-range.
        let doc = parse_document(SAMPLE);
        match resolve(&doc, "99999999999999999999") {
            Err(ResolveError::OutOfRange(addr)) => assert_eq!(addr, "99999999999999999999"),
            _ => panic!("expected OutOfRange, got a different result"),
        }
    }

    #[test]
    fn numeric_past_end_is_out_of_range() {
        let doc = parse_document(SAMPLE);
        assert!(matches!(
            resolve(&doc, "99"),
            Err(ResolveError::OutOfRange(_))
        ));
    }

    #[test]
    fn no_slug_match_errors() {
        let doc = parse_document(SAMPLE);
        assert!(matches!(
            resolve(&doc, "nope"),
            Err(ResolveError::NoSlugMatch(_))
        ));
    }

    #[test]
    fn ambiguous_slug_errors_with_candidates() {
        let doc = parse_document(SAMPLE);
        match resolve(&doc, "log-notes") {
            Err(ResolveError::Ambiguous(needle, candidates)) => {
                assert_eq!(needle, "log-notes");
                assert_eq!(candidates.len(), 2);
            }
            _ => panic!("expected Ambiguous"),
        }
    }

    #[test]
    fn resolve_address_errors_instead_of_exiting() {
        // resolve_address returns a Result the caller propagates with `?` (no
        // process::exit), so the error path is unit-testable.
        let doc = parse_document(SAMPLE);
        let oob = resolve_address(&doc, "99").unwrap_err();
        assert!(oob.to_string().contains("out of range"), "got: {}", oob);
        let ambig = resolve_address(&doc, "log-notes").unwrap_err();
        let msg = ambig.to_string();
        assert!(msg.contains("Ambiguous"), "got: {}", msg);
        assert!(
            msg.contains("Log & Notes") && msg.contains("Log Notes"),
            "got: {}",
            msg
        );
        // A valid address still resolves to the node.
        assert_eq!(resolve_address(&doc, "2").unwrap().heading, "Glossary");
    }

    #[test]
    fn ambiguous_slug_detected() {
        let doc = parse_document(SAMPLE);
        // "Log & Notes" and "Log Notes" both slugify to "log-notes".
        let needle = crate::slug::segment("log-notes");
        let mut all = Vec::new();
        flatten(&doc.tree, &mut all);
        let matches: Vec<&Node> = all.into_iter().filter(|n| n.slug == needle).collect();
        assert_eq!(matches.len(), 2, "expected a slug collision in the fixture");
    }

    // A parent section with one small child (below threshold) and one large
    // child (above threshold), the large child carrying a grandchild. Exercises
    // the inline-vs-fold heuristic and the depth budget.
    const UNFOLD: &str = "# Sec\n\nsec prose.\n\n## Small\n\ntiny.\n\n## Large\n\nLLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL LLLL.\n\n### Grand\n\ngrand prose.\n";

    #[test]
    fn should_inline_by_threshold() {
        let doc = parse_document(UNFOLD);
        let sec = &doc.tree[0];
        let small = &sec.children[0];
        let large = &sec.children[1];
        let small_tok = node_tokens(small, &doc.lines);
        let large_tok = node_tokens(large, &doc.lines);
        assert!(small_tok < large_tok, "fixture should split on tokens");
        // Threshold between the two: small inlines, large folds.
        let cut = (small_tok + large_tok) / 2;
        assert!(should_inline(small, &doc.lines, 1, None, cut, false));
        assert!(!should_inline(large, &doc.lines, 1, None, cut, false));
        // `--full` overrides the threshold for the large child.
        assert!(should_inline(large, &doc.lines, 1, None, cut, true));
    }

    #[test]
    fn should_inline_by_depth() {
        let doc = parse_document(UNFOLD);
        let large = &doc.tree[0].children[1];
        let grand = &large.children[0];
        let big = usize::MAX; // threshold never binds
        // depth=1 admits level_depth 0 (direct children at level_depth 1 fail)…
        assert!(!should_inline(grand, &doc.lines, 1, Some(1), big, false));
        // …depth=2 admits level_depth 1.
        assert!(should_inline(grand, &doc.lines, 1, Some(2), big, false));
        // Unlimited depth admits any level.
        assert!(should_inline(grand, &doc.lines, 9, None, big, false));
    }

    #[test]
    fn own_prose_stops_at_first_child() {
        let doc = parse_document(UNFOLD);
        let sec = &doc.tree[0];
        let prose = own_prose(sec, &doc.lines);
        assert!(prose.contains("sec prose."), "own prose: {}", prose);
        assert!(
            !prose.contains("tiny."),
            "own prose must stop before first child: {}",
            prose
        );
    }

    #[test]
    fn folded_placeholder_matches_overview_line() {
        let doc = parse_document(UNFOLD);
        let large = &doc.tree[0].children[1];
        // The folded placeholder for a child equals that child's overview tree
        // line, so a reader can drill with the same address.
        let placeholder = tree_line_string(large, &doc.lines);
        assert!(placeholder.contains("1.2"), "placeholder: {}", placeholder);
        assert!(
            placeholder.contains("Large"),
            "placeholder: {}",
            placeholder
        );
        assert!(
            placeholder.trim_start().starts_with('+'),
            "Large has a child, marker '+': {}",
            placeholder
        );
    }

    #[test]
    fn unfold_content_inlines_small_folds_large() {
        let doc = parse_document(UNFOLD);
        let sec = &doc.tree[0];
        let small_tok = node_tokens(&sec.children[0], &doc.lines);
        let large_tok = node_tokens(&sec.children[1], &doc.lines);
        let cut = (small_tok + large_tok) / 2;
        let s = unfold_content_string(sec, &doc.lines, 0, None, cut, false);
        assert!(s.contains("sec prose."), "own prose present: {}", s);
        assert!(s.contains("tiny."), "small child inlined: {}", s);
        // Large child folded: its body absent, its placeholder line present.
        assert!(
            !s.contains("LLLL LLLL"),
            "large body must be folded out: {}",
            s
        );
        assert!(s.contains("1.2"), "large child placeholder present: {}", s);
    }

    // --- reserved addressing ---

    #[test]
    fn frontmatter_addresses_recognized_case_insensitively() {
        use crate::{FmAddress, Reserved, reserved_reading};
        for a in ["fm", "FM", "frontmatter", "Frontmatter"] {
            assert!(
                matches!(reserved_reading(a), Some(Reserved::Fm(FmAddress::Block))),
                "{a} should name the whole block"
            );
        }
        for a in ["fm.tags", "FM.tags", "frontmatter.tags"] {
            match reserved_reading(a) {
                Some(Reserved::Fm(FmAddress::Path(p))) => assert_eq!(p, "tags"),
                _ => panic!("{a} should name a value"),
            }
        }
        // The path keeps its own dots, brackets, and original case: YAML keys are
        // case-sensitive, so only the prefix may be lowercased.
        match reserved_reading("fm.References[0].Target") {
            Some(Reserved::Fm(FmAddress::Path(p))) => assert_eq!(p, "References[0].Target"),
            _ => panic!("deep path should survive intact"),
        }
    }

    #[test]
    fn the_other_reserved_spellings_name_their_own_readings() {
        use crate::{Reserved, reserved_reading};
        for a in ["0", "text", "TEXT"] {
            assert_eq!(reserved_reading(a), Some(Reserved::Text), "{a} is the lede");
        }
        for a in ["links", "LINKS"] {
            assert_eq!(
                reserved_reading(a),
                Some(Reserved::Links),
                "{a} is the index"
            );
        }
    }

    #[test]
    fn non_reserved_addresses_fall_through_to_the_tree() {
        use crate::reserved_reading;
        // Heading addresses and a bare trailing dot are reserved by nothing, so
        // the heading-tree resolver still owns them.
        for a in ["1", "1.2", "glossary", "fm.", "format", "0.1", "textual"] {
            assert!(reserved_reading(a).is_none(), "{a} must not be reserved");
        }
    }

    #[test]
    fn frontmatter_address_does_not_shadow_a_heading_tree_lookup() {
        // `fm` is intercepted before resolution, so a document whose heading
        // slugs to `fm` still resolves every other address normally.
        let doc = parse_document(SAMPLE);
        assert_eq!(resolve_address(&doc, "2").unwrap().heading, "Glossary");
    }

    #[test]
    fn numeric_address_predicate() {
        assert!(is_numeric_address("1"));
        assert!(is_numeric_address("1.2.3"));
        assert!(!is_numeric_address("1."));
        assert!(!is_numeric_address(".1"));
        assert!(!is_numeric_address("1.a"));
        assert!(!is_numeric_address("text"));
        assert!(!is_numeric_address(""));
    }

    // --- reserved-address shadowing ---

    // A heading that slugs to `links`, in a file whose link list is non-empty:
    // the reserved reading succeeds and is still not what `## Links` holds.
    const SHADOW_LINKS: &str = "---\ntype: note\n---\n\nLede with [[Elsewhere]].\n\n# Direction\n\ndir body.\n\n## Links\n\n- [[A Note]]\n";
    // A heading that slugs to `fm`, in a file with no frontmatter block.
    const SHADOW_FM: &str = "# Direction\n\n## FM\n\nnot the frontmatter.\n";
    // A heading that slugs to `text`, in a file whose first line is a heading, so
    // there is no lede for `0`/`text` to name.
    const SHADOW_TEXT: &str = "# Direction\n\n## Text\n\nnot the lede.\n";

    fn read(content: &str, address: Option<&str>) -> anyhow::Result<crate::Reading> {
        crate::read_content(
            "x.md",
            content,
            address,
            None,
            false,
            2000,
            crate::Dialect::default(),
        )
    }

    // The two readings a caller most wants as data. Printing nothing is the
    // point: these assert on the returned value, so the seam that lets another
    // crate call `mdread` stays open.

    #[test]
    fn the_overview_arrives_as_a_value() {
        let crate::Reading::Overview(o) = read(SAMPLE, None).unwrap() else {
            panic!("no address reads the overview")
        };
        assert_eq!(o.path, "x.md");
        assert_eq!(o.fields, ["type", "slug"]);
        assert!(o.text.is_some(), "the lede is a node of its own");
        assert_eq!(o.tree.len(), 4);
        assert_eq!(o.tree[0].address, "1");
        assert_eq!(o.tree[0].children.len(), 2);
    }

    #[test]
    fn an_unfolded_section_arrives_as_a_value() {
        let crate::Reading::Unfold(u) = read(SAMPLE, Some("1")).unwrap() else {
            panic!("a numeric address reads a section")
        };
        assert_eq!(u.address, "1");
        assert_eq!(u.heading, "Direction");
        assert_eq!(u.children.len(), 2);
        // `content` stops at the first child; `text` carries the whole section,
        // which is the difference between what JSON serves and what text prints.
        assert!(u.content.contains("Dir body."));
        assert!(!u.content.contains("sub one body"));
        assert!(u.text.contains("sub one body"));
    }

    #[test]
    fn served_links_over_a_shadow_is_announced() {
        // The reserved reading succeeds and is non-empty, so nothing errors — the
        // whole point is that the caller would otherwise never learn about 1.1.
        assert!(!crate::facet::links(SHADOW_LINKS, crate::LinkRule::All).is_empty());
        assert!(read(SHADOW_LINKS, Some("links")).is_ok());

        let doc = parse_document(SHADOW_LINKS);
        assert_eq!(
            crate::shadow::phrase(&doc, "links").as_deref(),
            Some("heading 'Links' (1.1) also answers to 'links'")
        );
        // Case of the typed address does not change the answer.
        assert!(crate::shadow::phrase(&doc, "LINKS").is_some());
    }

    #[test]
    fn overview_footer_names_the_shadowing_heading() {
        let doc = parse_document(SHADOW_LINKS);
        assert_eq!(
            crate::shadow::overview_notes(&doc),
            vec![
                "note: 'Links' (1.1) also answers to a reserved address; reach it by number"
                    .to_string()
            ]
        );
    }

    #[test]
    fn missing_frontmatter_error_names_the_shadowing_heading() {
        let err = read(SHADOW_FM, Some("fm")).unwrap_err().to_string();
        assert_eq!(
            err,
            "No frontmatter block in this file (address 'fm'); heading 'FM' (1.1) also answers to 'fm'"
        );
    }

    #[test]
    fn missing_text_region_error_names_the_shadowing_heading() {
        let err = read(SHADOW_TEXT, Some("text")).unwrap_err().to_string();
        assert_eq!(
            err,
            "No text region in this file (address 'text'); heading 'Text' (1.1) also answers to 'text'"
        );
    }

    #[test]
    fn reserved_errors_keep_their_message_without_a_shadow() {
        let plain = "# Direction\n\nbody.\n";
        assert_eq!(
            read(plain, Some("fm")).unwrap_err().to_string(),
            "No frontmatter block in this file (address 'fm')"
        );
        assert_eq!(
            read(plain, Some("text")).unwrap_err().to_string(),
            "No text region in this file (address 'text')"
        );
    }

    #[test]
    fn a_document_without_collisions_says_nothing() {
        let doc = parse_document(SAMPLE);
        assert!(crate::shadow::overview_notes(&doc).is_empty());
        for name in ["0", "text", "fm", "frontmatter", "links"] {
            assert!(
                crate::shadow::phrase(&doc, name).is_none(),
                "{name} must not report a shadow"
            );
        }
        assert!(read(SAMPLE, None).is_ok());
    }

    #[test]
    fn an_alias_of_the_same_reading_is_announced_too() {
        // `fm` and `frontmatter` serve one reading, so a `## Frontmatter` section
        // is shadowed whichever spelling the caller typed. The clause names the
        // word the heading actually slugs to.
        let content =
            "---\ntype: note\n---\n\n# Direction\n\n## Frontmatter\n\nabout the fields.\n";
        let doc = parse_document(content);
        let expected = Some("heading 'Frontmatter' (1.1) also answers to 'frontmatter'");
        assert_eq!(
            crate::shadow::phrase(&doc, "frontmatter").as_deref(),
            expected
        );
        assert_eq!(crate::shadow::phrase(&doc, "fm").as_deref(), expected);
        // Same for the lede's two spellings.
        let lede = "# Direction\n\n## Text\n\nnot the lede.\n";
        let doc = parse_document(lede);
        let expected = Some("heading 'Text' (1.1) also answers to 'text'");
        assert_eq!(crate::shadow::phrase(&doc, "0").as_deref(), expected);
        assert_eq!(crate::shadow::phrase(&doc, "text").as_deref(), expected);
    }

    #[test]
    fn a_value_address_has_no_shadow() {
        // `fm.tags` is not a slug any heading can carry, so a heading slugging to
        // `fm` shadows the block address alone — announcing it under `fm.tags`
        // would name a heading that answers to nothing of the sort.
        let doc = parse_document(SHADOW_FM);
        assert!(crate::shadow::phrase(&doc, "fm").is_some());
        assert!(crate::shadow::phrase(&doc, "fm.tags").is_none());
        assert!(crate::shadow::phrase(&doc, "glossary").is_none());
    }

    #[test]
    fn several_shadows_are_all_named() {
        // Two headings slug to `links`: one clause each, joined, and one overview
        // line each.
        let content = "# One\n\n## Links\n\na\n\n# Two\n\n## Links\n\nb\n";
        let doc = parse_document(content);
        assert_eq!(
            crate::shadow::phrase(&doc, "links").as_deref(),
            Some(
                "heading 'Links' (1.1) also answers to 'links'; heading 'Links' (2.1) also answers to 'links'"
            )
        );
        assert_eq!(crate::shadow::overview_notes(&doc).len(), 2);
    }

    #[test]
    fn the_footer_groups_shadows_by_reading_not_by_document_order() {
        // The tree meets the readings in the order links, fm, text; the footer
        // reports them in `Reading`'s declaration order, so the variant order is
        // key and a reordering of the enum is a change to the output.
        let content = "# One\n\n## Links\n\na\n\n## Frontmatter\n\nb\n\n## Text\n\nc\n";
        let doc = parse_document(content);
        assert_eq!(
            crate::shadow::overview_notes(&doc),
            vec![
                "note: 'Text' (1.3) also answers to a reserved address; reach it by number",
                "note: 'Frontmatter' (1.2) also answers to a reserved address; reach it by number",
                "note: 'Links' (1.1) also answers to a reserved address; reach it by number",
            ]
        );
    }

    #[test]
    fn bom_prefixed_frontmatter_is_skipped() {
        // A BOM before the opening `---` must not shift heading line numbers.
        let body = "---\ntype: note\n---\n\nlede\n\n# Heading\n\nbody\n";
        let with_bom = format!("\u{feff}{}", body);
        let plain = parse_document(body);
        let bommed = parse_document(&with_bom);
        assert_eq!(plain.tree.len(), 1);
        assert_eq!(bommed.tree.len(), 1);
        assert_eq!(bommed.tree[0].line, plain.tree[0].line);
        assert_eq!(bommed.tree[0].heading, "Heading");
        // Field order still recovered through the BOM.
        assert_eq!(
            crate::frontmatter::field_order(&with_bom),
            vec!["type".to_string()]
        );
    }
}
