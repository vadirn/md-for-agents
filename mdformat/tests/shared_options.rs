use comrak::Arena;
use comrak::nodes::NodeValue;

#[test]
fn shared_options_keep_footnote_definitions_as_paragraphs() {
    let src = "[^1]: A footnote-style bibliography entry, corpus convention.\n\nBody text.\n";
    let opts = mdstruct::Options::default();
    let comrak_opts = mdformat::comrak_options(&opts);
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, src, &comrak_opts);

    let has_footnote_def = root
        .descendants()
        .any(|n| matches!(n.data.borrow().value, NodeValue::FootnoteDefinition(_)));
    assert!(
        !has_footnote_def,
        "shared options must leave footnotes off, matching mdstruct exactly"
    );

    let paragraph_count = root
        .descendants()
        .filter(|n| matches!(n.data.borrow().value, NodeValue::Paragraph))
        .count();
    assert_eq!(
        paragraph_count, 2,
        "the footnote-style line and the body line both survive as paragraphs"
    );
}

#[test]
fn shared_options_enable_documented_extensions() {
    let src = "---\ntitle: x\n---\n\n~~strike~~ and https://example.com bare\n\n- [x] done\n";
    let opts = mdstruct::Options::default();
    let comrak_opts = mdformat::comrak_options(&opts);
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, src, &comrak_opts);

    let mut has_frontmatter = false;
    let mut has_strikethrough = false;
    let mut has_autolink = false;
    let mut has_tasklist = false;
    for n in root.descendants() {
        match n.data.borrow().value {
            NodeValue::FrontMatter(_) => has_frontmatter = true,
            NodeValue::Strikethrough => has_strikethrough = true,
            NodeValue::Link(_) => has_autolink = true,
            NodeValue::TaskItem(_) => has_tasklist = true,
            _ => {}
        }
    }
    assert!(has_frontmatter, "front_matter_delimiter must be set");
    assert!(has_strikethrough, "extension.strikethrough must be on");
    assert!(has_autolink, "extension.autolink must be on");
    assert!(has_tasklist, "extension.tasklist must be on");
}

#[test]
fn spans_partition_a_realistic_document() {
    let src = "---\ntitle: x\n---\n# Heading\n\nSome *text* with a [[Wikilink]] and a [link](https://x.io).\n\n- one\n- two\n";
    let opts = mdstruct::Options::default();
    let part = mdformat::partition(src, &opts).expect("every sourcepos converts");
    assert!(part.passed(), "{:?}", part.report.violations);
    assert_eq!(part.report.content_bytes, part.report.covered_content_bytes);
}
