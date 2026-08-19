//! The rendering layer: turn a [`Reading`] into what a terminal shows.
//!
//! All of this crate's output is written here, through [`cli::with_stdout`], so
//! a reader that exits early stops the run rather than panicking it. The library
//! builds values; this module is the only place that decides how they look,
//! which is what lets a caller take the same data and render it its own way.

use std::io::{self, Write};

use anyhow::Result;
use cli::{TextJson, with_stdout};

use crate::frontmatter;
use crate::model::{Node, range_lines, range_slice};
use crate::reading::{Frontmatter, FrontmatterValue, Links, Overview, Reading, TreeNode, Unfold};

/// Print one reading in the requested format.
pub fn print(reading: &Reading, format: TextJson) -> Result<()> {
    // Serialized before the writer opens, so a serde failure stays separate from
    // what the pipe did.
    if format == TextJson::Json {
        let json = match reading {
            Reading::Overview(o) => serde_json::to_string_pretty(o)?,
            Reading::Frontmatter(f) => serde_json::to_string_pretty(f)?,
            Reading::FrontmatterValue(v) => serde_json::to_string_pretty(v)?,
            Reading::Links(l) => serde_json::to_string_pretty(l)?,
            Reading::Unfold(u) => serde_json::to_string_pretty(u)?,
        };
        with_stdout(|out| writeln!(out, "{}", json))?;
        return Ok(());
    }

    with_stdout(|out| match reading {
        Reading::Overview(o) => write_overview(out, o),
        Reading::Frontmatter(f) => write_frontmatter(out, f),
        Reading::FrontmatterValue(v) => write_frontmatter_value(out, v),
        Reading::Links(l) => write_links(out, l),
        Reading::Unfold(u) => write_unfold(out, u),
    })?;
    Ok(())
}

fn write_overview(out: &mut io::StdoutLock<'_>, o: &Overview) -> io::Result<()> {
    writeln!(out, "{}", o.path)?;
    if !o.fields.is_empty() {
        writeln!(out, "fields: {}", o.fields.join(", "))?;
    }
    writeln!(out, "links: {}", o.links)?;
    writeln!(out)?;

    if let Some(t) = &o.text {
        // Two leading spaces to align under the `+`/space marker column.
        writeln!(
            out,
            "  [0]  (text)        L{}   {} lines · ~{} tok",
            t.line, t.lines, t.tokens
        )?;
    }

    for n in &o.tree {
        write_tree(out, n)?;
    }

    writeln!(out)?;
    // Tool-agnostic: names addresses, not a command, so the `mdread` CLI and any
    // wrapper around it print something true of themselves.
    writeln!(
        out,
        "next: <addr> a section · fm frontmatter (fm.<path> one value) · links outgoing links"
    )?;
    // Only when the document actually collides, so the common overview is
    // unchanged. The line is a report about the tree above it, which is why it
    // may join stdout where the unfold notes may not.
    for line in &o.notes {
        writeln!(out, "{}", line)?;
    }
    Ok(())
}

fn write_tree(out: &mut io::StdoutLock<'_>, n: &TreeNode) -> io::Result<()> {
    writeln!(
        out,
        "{}",
        tree_line(
            &n.address,
            &n.heading,
            n.line,
            n.lines,
            n.tokens,
            !n.children.is_empty()
        )
    )?;
    for c in &n.children {
        write_tree(out, c)?;
    }
    Ok(())
}

fn write_frontmatter(out: &mut io::StdoutLock<'_>, f: &Frontmatter) -> io::Result<()> {
    writeln!(
        out,
        "{}  (frontmatter)   L{}   {} lines",
        f.address, f.line, f.lines
    )?;
    writeln!(out)?;
    writeln!(out, "{}", f.text)
}

fn write_frontmatter_value(out: &mut io::StdoutLock<'_>, v: &FrontmatterValue) -> io::Result<()> {
    writeln!(out, "{}", frontmatter::value_to_text(&v.value))
}

fn write_links(out: &mut io::StdoutLock<'_>, l: &Links) -> io::Result<()> {
    writeln!(out, "{}  (outgoing)   {} links", l.address, l.links.len())?;
    writeln!(out)?;
    for link in &l.links {
        let display = match &link.alias {
            Some(alias) => format!("{} -> {}", link.target, alias),
            None => link.target.clone(),
        };
        writeln!(out, "  L{:<5} {:<9}  {}", link.line, link.kind, display)?;
    }
    Ok(())
}

fn write_unfold(out: &mut io::StdoutLock<'_>, u: &Unfold) -> io::Result<()> {
    writeln!(
        out,
        "{}  {}   L{}   {} lines · ~{} tok",
        u.address, u.heading, u.line, u.lines, u.tokens
    )?;
    writeln!(out)?;
    write!(out, "{}", u.text)
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
        cli::estimate_tokens(&range_slice(lines, n.start, n.end).unwrap_or_default()),
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
