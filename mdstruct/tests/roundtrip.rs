//! The freeze gate over the mandatory fixtures, plus golden interior-span
//! assertions that tiling and inline fidelity cannot see.

use mdstruct::{Node, Options, parse, verify_spans};

fn doc(src: &str) -> mdstruct::Document {
    let d = parse(src, &Options::default());
    verify_spans(&d, src).expect("freeze gate must pass on fixture");
    d
}

fn slice(src: &str, span: mdstruct::Span) -> &str {
    &src[span.start..span.end]
}

#[test]
fn example_interior_spans() {
    let src = "---\ntitle: Example\n---\n# Guide\n\nText [link](https://x.io) and [[Note#Sec|alias]].\n\n## Setup\n\n```rust\nlet x = 1;\n```\n";
    let d = doc(src);

    // Frontmatter: present, body starts after the block.
    let fm = d.frontmatter().expect("frontmatter present");
    assert_eq!(fm.format.as_deref(), Some("yaml"));
    assert_eq!(fm.body_start_line, 4);

    // H1 → H2 nested; textSpan excludes the `#` markers.
    assert_eq!(d.headings.len(), 1);
    let h1 = &d.headings[0];
    assert_eq!(h1.level, 1);
    assert_eq!(slice(src, h1.text_span), "Guide");
    assert_eq!(h1.children.len(), 1);
    let h2 = &h1.children[0];
    assert_eq!(slice(src, h2.text_span), "Setup");
    // Section extends to EOF for both. The source is 12 lines (the trailing
    // newline adds no phantom line), so both sectionEndLine land on 12.
    assert_eq!(h1.section_span.end, src.len());
    assert_eq!(h2.section_span.end, src.len());
    assert_eq!(h1.section_end_line, 12);
    assert_eq!(h2.section_end_line, 12);

    // codeBlock: bodySpan is the RAW body (no fence, no trailing newline);
    // infoSpan is the info string.
    let cb = d
        .nodes
        .iter()
        .find(|n| matches!(n, Node::CodeBlock { .. }))
        .unwrap();
    if let Node::CodeBlock {
        info_span,
        body_span,
        info,
        ..
    } = cb
    {
        assert_eq!(info, "rust");
        assert_eq!(slice(src, info_span.unwrap()), "rust");
        assert_eq!(slice(src, *body_span), "let x = 1;");
    }

    // Inlines: link + wikilink decomposed.
    let wl = d
        .inlines
        .iter()
        .find(|i| matches!(i, mdstruct::Inline::Wikilink { .. }))
        .unwrap();
    if let mdstruct::Inline::Wikilink {
        target,
        page,
        heading,
        block,
        embed,
        alias,
        alias_span,
        ..
    } = wl
    {
        assert_eq!(target, "Note#Sec");
        assert_eq!(page, "Note");
        assert_eq!(heading.as_deref(), Some("Sec"));
        assert_eq!(*block, None);
        assert!(!*embed);
        assert_eq!(alias.as_deref(), Some("alias"));
        assert_eq!(slice(src, alias_span.unwrap()), "alias");
    }
}

#[test]
fn table_cell_wikilink_and_embed() {
    let src =
        "| Ref | Note |\n| --- | --- |\n| [[Alpha]] | ![[Beta]] |\n| [[Gamma\\|display]] | x |\n";
    let d = doc(src);
    let wl = |target: &str| {
        d.inlines.iter().find_map(|i| match i {
            mdstruct::Inline::Wikilink {
                target: t,
                alias,
                embed,
                ..
            } if t == target => Some((alias.clone(), *embed)),
            _ => None,
        })
    };
    // Plain cell wikilink: target from decoded url, no pipe → alias None.
    assert_eq!(wl("Alpha"), Some((None, false)));
    // Cell embed: emitted with embed:true, byte-exact span.
    assert_eq!(wl("Beta"), Some((None, true)));
    // Escaped-pipe cell wikilink: alias recovered from the decoded display.
    assert_eq!(wl("Gamma"), Some((Some("display".to_string()), false)));
}

/// Each inline as `(kind, raw slice)`, in document order.
fn inline_slices<'s>(d: &mdstruct::Document, src: &'s str) -> Vec<(&'static str, &'s str)> {
    d.inlines
        .iter()
        .map(|i| (i.kind(), slice(src, i.span())))
        .collect()
}

#[test]
fn table_cell_span_counts_the_escaped_pipes_before_it() {
    // comrak drops the `\` of each `\|` in a cell before it parses inlines, so
    // every one before an inline would pull that inline's span one byte short.
    for cell in [
        "[[X]]",
        "a \\| [[X]]",
        "a \\| b \\| [[X]]",
        "a \\| b \\| c \\| [[X]]",
        "[[X]] \\| a",
        // Only an odd backslash run escapes the pipe, so only it loses a byte.
        "a \\\\| [[X]]",
        "a \\\\\\| [[X]]",
    ] {
        let src = format!("| h |\n| --- |\n| {cell} |\n");
        let d = doc(&src);
        assert_eq!(
            inline_slices(&d, &src),
            [("wikilink", "[[X]]")],
            "cell {cell:?}"
        );
    }
}

#[test]
fn table_cell_escaped_pipe_wikilink_slices_exactly() {
    let src = "| h |\n| --- |\n| [[Page\\|alias]] |\n";
    let d = doc(src);
    let [
        mdstruct::Inline::Wikilink {
            target,
            alias,
            alias_span,
            span,
            ..
        },
    ] = d.inlines.as_slice()
    else {
        panic!("expected one wikilink, got {:?}", d.inlines);
    };
    assert_eq!(target, "Page");
    assert_eq!(alias.as_deref(), Some("alias"));
    assert_eq!(slice(src, *span), "[[Page\\|alias]]");
    assert_eq!(slice(src, alias_span.unwrap()), "alias");
}

#[test]
fn table_cell_bare_pipe_splits_the_link() {
    // GFM splits the row at a bare `|`, as Obsidian does, so neither form is a
    // link: one cell ends in `[[Page` or `![[img.png`, and nothing closes it.
    for cell in ["[[Page|alias]]", "![[img.png|100]]"] {
        let src = format!("| a | b |\n| --- | --- |\n| {cell} | c |\n");
        let d = doc(&src);
        assert!(d.inlines.is_empty(), "cell {cell:?}: {:?}", d.inlines);
    }
}

#[test]
fn table_cell_embed_reads_its_escaped_pipe_as_the_separator() {
    let src = "| a | b |\n| --- | --- |\n| ![[img.png\\|100]] | c |\n";
    let d = doc(src);
    let [
        mdstruct::Inline::Wikilink {
            target,
            page,
            alias,
            embed: true,
            span,
            ..
        },
    ] = d.inlines.as_slice()
    else {
        panic!("expected one embed, got {:?}", d.inlines);
    };
    assert_eq!(target, "img.png");
    assert_eq!(page, "img.png");
    assert_eq!(alias.as_deref(), Some("100"));
    assert_eq!(slice(src, *span), "![[img.png\\|100]]");
}

#[test]
fn paragraph_lines_above_a_table_keep_raw_spans() {
    // comrak also drops the `\` of each `\|` in the paragraph lines it splits
    // off above a table's header row. Each line shifts by its own escapes only.
    let src = "one \\| [[A]]\ntwo \\| b \\| [[B]]\n| h |\n| - |\n| c |\n";
    let d = doc(src);
    assert_eq!(
        inline_slices(&d, src),
        [("wikilink", "[[A]]"), ("wikilink", "[[B]]")]
    );
}

#[test]
fn empty_pipe_wikilink() {
    let src = "See [[Topic|]] and [[Topic]].\n";
    let d = doc(src);
    let aliases: Vec<Option<String>> = d
        .inlines
        .iter()
        .filter_map(|i| match i {
            mdstruct::Inline::Wikilink { alias, .. } => Some(alias.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(aliases, vec![Some(String::new()), None]);
}

#[test]
fn cyrillic_terminal() {
    let src = "## Заметка\n\nтекст [[Ссылка]] конец\n";
    let d = doc(src);
    let h = &d.headings[0];
    assert_eq!(slice(src, h.span), "## Заметка");
    assert_eq!(slice(src, h.text_span), "Заметка");
    let wl = &d.inlines[0];
    assert!(slice(src, wl.span()).starts_with("[[") && slice(src, wl.span()).ends_with("]]"));
}

#[test]
fn crlf() {
    let src = "# Title\r\n\r\nA paragraph with [[Note]].\r\n";
    let d = doc(src);
    assert_eq!(d.headings.len(), 1);
    assert_eq!(slice(src, d.headings[0].text_span), "Title");
    // The wikilink slices exactly despite CRLF line endings.
    let wl = &d.inlines[0];
    assert_eq!(slice(src, wl.span()), "[[Note]]");
}

#[test]
fn bom() {
    let src = "\u{feff}# Heading\n\nbody\n";
    let d = doc(src);
    assert_eq!(slice(src, d.headings[0].text_span), "Heading");
    // The BOM stays unowned-in-gap: it is never emitted as an Unknown node.
    assert!(
        !d.nodes.iter().any(|n| matches!(n, Node::Unknown { .. })),
        "BOM must not surface as an Unknown uncovered node"
    );
    // And the freeze gate (now including the no-Unknown check) still passes.
    verify_spans(&d, src).expect("freeze gate must pass with a leading BOM");
}

#[test]
fn unclosed_frontmatter() {
    let src = "---\ntitle: x\n# Not closed\n\nbody\n";
    let d = doc(src);
    assert!(d.frontmatter().is_none());
    assert_eq!(d.frontmatter.body_start_line, 1);
    assert_eq!(d.frontmatter.body_start_byte, 0);
}

#[test]
fn closing_hash() {
    let src = "## Foo ##\n\nbody\n";
    let d = doc(src);
    let h = &d.headings[0];
    assert_eq!(slice(src, h.span), "## Foo ##");
    assert_eq!(slice(src, h.text_span), "Foo");
}

#[test]
fn setext() {
    let src = "Title\n=====\n\nbody\n";
    let d = doc(src);
    let h = &d.headings[0];
    assert_eq!(h.level, 1);
    assert!(h.setext);
    assert_eq!(slice(src, h.text_span), "Title");
}

#[test]
fn nested_regions() {
    let src = "<!-- outer -->\n<!-- inner -->\ncontent\n<!-- /inner -->\n<!-- /outer -->\n";
    let opts = Options { wikilinks: true };
    let d = parse(src, &opts);
    verify_spans(&d, src).expect("region-slice check must pass");
    assert_eq!(d.regions.len(), 2);
    // Overlapping by design: inner's span sits within outer's.
    let outer = d.regions.iter().find(|r| r.label == "outer").unwrap();
    let inner = d.regions.iter().find(|r| r.label == "inner").unwrap();
    assert!(outer.span.start <= inner.span.start && inner.span.end <= outer.span.end);
    assert_eq!(slice(src, inner.body_span), "content\n");
}

#[test]
fn link_reference_definition_recovered() {
    let src = "See the note.\n\n[^1]: https://example.com/x\n";
    let d = doc(src);
    let lrd = d
        .nodes
        .iter()
        .find(|n| matches!(n, Node::LinkReferenceDefinition { .. }))
        .expect("link reference definition recovered as a node");
    assert_eq!(slice(src, lrd.span()), "[^1]: https://example.com/x");
}

#[test]
fn bom_only_file_passes_gate() {
    for src in ["\u{feff}", "\u{feff}\n\n  \n", "\u{feff}   \n"] {
        let d = parse(src, &Options::default());
        assert!(
            verify_spans(&d, src).is_ok(),
            "BOM-only src {src:?} must pass the gate"
        );
        assert!(
            !d.nodes.iter().any(|n| matches!(n, Node::Unknown { .. })),
            "BOM-only src {src:?} must not emit an unknown node"
        );
    }
}

#[test]
fn embed_escape_backslash_parity() {
    use mdstruct::Inline;
    let wikilink = |src: &str| {
        parse(src, &Options::default())
            .inlines
            .iter()
            .find_map(|i| match i {
                Inline::Wikilink { page, embed, .. } => Some((page.clone(), *embed)),
                _ => None,
            })
    };
    // one backslash: `!` escaped → plain wikilink, not an embed.
    assert_eq!(
        wikilink("x \\![[Note]] y"),
        Some(("Note".to_string(), false))
    );
    // two backslashes: literal `\` + live `!` → genuine embed.
    assert_eq!(
        wikilink("x \\\\![[Note]] y"),
        Some(("Note".to_string(), true))
    );
}

#[test]
fn emph_and_strong_star_delimiter() {
    let src = "Some *emphasis* and **strong** here.\n";
    let d = doc(src);
    let emph = d
        .inlines
        .iter()
        .find(|i| matches!(i, mdstruct::Inline::Emph { .. }))
        .expect("Emph emitted for *emphasis*");
    assert_eq!(slice(src, emph.span()), "*emphasis*");
    let strong = d
        .inlines
        .iter()
        .find(|i| matches!(i, mdstruct::Inline::Strong { .. }))
        .expect("Strong emitted for **strong**");
    assert_eq!(slice(src, strong.span()), "**strong**");
}

#[test]
fn emph_and_strong_underscore_delimiter() {
    let src = "Some _underscore_ and __underscore strong__ here.\n";
    let d = doc(src);
    let emph = d
        .inlines
        .iter()
        .find(|i| matches!(i, mdstruct::Inline::Emph { .. }))
        .expect("Emph emitted for _underscore_");
    assert_eq!(slice(src, emph.span()), "_underscore_");
    let strong = d
        .inlines
        .iter()
        .find(|i| matches!(i, mdstruct::Inline::Strong { .. }))
        .expect("Strong emitted for __underscore strong__");
    assert_eq!(slice(src, strong.span()), "__underscore strong__");
}

#[test]
fn table_cell_emphasis_is_emitted() {
    let src = "| a | b |\n| --- | --- |\n| *b* | c |\n";
    let d = doc(src);
    assert!(
        !d.inlines.is_empty(),
        "table cell with `*b*` must not yield an empty inlines[]"
    );
    assert!(
        d.inlines
            .iter()
            .any(|i| matches!(i, mdstruct::Inline::Emph { .. })),
        "table cell `*b*` must be emitted as an Emph inline"
    );
}

#[test]
fn nested_emph_strong_triple_asterisk() {
    let src = "Some ***both*** here.\n";
    let d = doc(src);
    let emph = d
        .inlines
        .iter()
        .find(|i| matches!(i, mdstruct::Inline::Emph { .. }))
        .expect("outer Emph emitted for ***both***");
    assert_eq!(slice(src, emph.span()), "***both***");
    let strong = d
        .inlines
        .iter()
        .find(|i| matches!(i, mdstruct::Inline::Strong { .. }))
        .expect("inner Strong emitted for ***both***");
    assert_eq!(slice(src, strong.span()), "**both**");
    // The inner Strong nests inside the outer Emph.
    assert!(emph.span().start <= strong.span().start && strong.span().end <= emph.span().end);
}

#[test]
fn unemitted_inline_kinds_produce_no_inlines() {
    let src = "Line one\nline two.\n\nLine three\\\nline four.\n\n~~struck~~ text.\n\nInline <span>tag</span> here.\n";
    let d = doc(src);
    assert!(
        d.inlines.is_empty(),
        "Text/SoftBreak/LineBreak/Strikethrough/HtmlInline must not appear in inlines[], got {:?}",
        d.inlines
    );
}
