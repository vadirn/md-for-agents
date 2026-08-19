//! The rendering layer: turn a [`Reading`] into what a terminal shows.
//!
//! Every `println!` in this crate lives here. The library builds values; this
//! module is the only place that decides how they look, which is what lets a
//! caller take the same data and render it its own way.

use anyhow::Result;

use crate::format::TextJson;
use crate::frontmatter;
use crate::model::{Node, range_lines, range_slice};
use crate::reading::{Frontmatter, FrontmatterValue, Links, Overview, Reading, TreeNode, Unfold};
use crate::tokens;

/// Print one reading in the requested format.
pub fn print(reading: &Reading, format: TextJson) -> Result<()> {
    match reading {
        Reading::Overview(o) => print_one(o, format, print_overview),
        Reading::Frontmatter(f) => print_one(f, format, print_frontmatter),
        Reading::FrontmatterValue(v) => print_one(v, format, print_frontmatter_value),
        Reading::Links(l) => print_one(l, format, print_links),
        Reading::Unfold(u) => print_one(u, format, print_unfold),
    }
}

fn print_one<T: serde::Serialize>(value: &T, format: TextJson, text: fn(&T)) -> Result<()> {
    if format == TextJson::Json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        text(value);
    }
    Ok(())
}

fn print_overview(o: &Overview) {
    println!("{}", o.path);
    if !o.fields.is_empty() {
        println!("fields: {}", o.fields.join(", "));
    }
    println!("links: {}", o.links);
    println!();

    if let Some(t) = &o.text {
        // Two leading spaces to align under the `+`/space marker column.
        println!(
            "  [0]  (text)        L{}   {} lines · ~{} tok",
            t.line, t.lines, t.tokens
        );
    }

    for n in &o.tree {
        print_tree(n);
    }

    println!();
    // Tool-agnostic: names addresses, not a command, so the `mdread` CLI and any
    // wrapper around it print something true of themselves.
    println!(
        "next: <addr> a section · fm frontmatter (fm.<path> one value) · links outgoing links"
    );
    // Only when the document actually collides, so the common overview is
    // unchanged. The line is a report about the tree above it, which is why it
    // may join stdout where the unfold notes may not.
    for line in &o.notes {
        println!("{}", line);
    }
}

fn print_tree(n: &TreeNode) {
    println!(
        "{}",
        tree_line(
            &n.address,
            &n.heading,
            n.line,
            n.lines,
            n.tokens,
            !n.children.is_empty()
        )
    );
    for c in &n.children {
        print_tree(c);
    }
}

fn print_frontmatter(f: &Frontmatter) {
    println!(
        "{}  (frontmatter)   L{}   {} lines",
        f.address, f.line, f.lines
    );
    println!();
    println!("{}", f.text);
}

fn print_frontmatter_value(v: &FrontmatterValue) {
    println!("{}", frontmatter::value_to_text(&v.value));
}

fn print_links(l: &Links) {
    println!("{}  (outgoing)   {} links", l.address, l.links.len());
    println!();
    for link in &l.links {
        let display = match &link.alias {
            Some(alias) => format!("{} -> {}", link.target, alias),
            None => link.target.clone(),
        };
        println!("  L{:<5} {:<9}  {}", link.line, link.kind, display);
    }
}

fn print_unfold(u: &Unfold) {
    println!(
        "{}  {}   L{}   {} lines · ~{} tok",
        u.address, u.heading, u.line, u.lines, u.tokens
    );
    println!();
    print!("{}", u.text);
}

/// One rule, so an overview line and a folded placeholder inside an unfold read
/// identically.
fn tree_line(
    address: &str,
    heading: &str,
    line: usize,
    lines: usize,
    tokens: usize,
    has_children: bool,
) -> String {
    let marker = if has_children { '+' } else { ' ' };
    let indent = "  ".repeat(address.matches('.').count());
    format!(
        "{} {}{:<6} {:<14} L{}   {} lines · ~{} tok",
        marker,
        indent,
        address,
        truncate_heading(heading),
        line,
        lines,
        tokens
    )
}

/// Format a single overview tree line straight from the parsed node, for the
/// unfold walker's folded placeholders. Sizes are computed here because the
/// walker holds nodes rather than the rendered tree.
pub(crate) fn tree_line_string(n: &Node, lines: &[&str]) -> String {
    tree_line(
        &n.address,
        &n.heading,
        n.line,
        range_lines(n.start, n.end),
        tokens::estimate_tokens(&range_slice(lines, n.start, n.end).unwrap_or_default()),
        !n.children.is_empty(),
    )
}

/// Trim a heading for the tree column. Long headings are cut to keep the line
/// scannable; the address remains the stable handle.
fn truncate_heading(h: &str) -> String {
    let max = 30;
    if h.chars().count() <= max {
        h.to_string()
    } else {
        let prefix: String = h.chars().take(max - 1).collect();
        format!("{}…", prefix)
    }
}
