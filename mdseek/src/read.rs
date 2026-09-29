//! Outline-first reads: a file folded to one line per definition or section,
//! or one range unfolded.
//!
//! Markdown goes through mdread whole, addresses and all. Code prints its
//! outline tree, and a range prints those lines numbered.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::sections::{Kind, kind_of};

pub fn run(out: &mut impl Write, file: &Path, address: Option<&str>) -> Result<()> {
    let name = file.to_string_lossy();
    let Some(kind) = kind_of(&name) else {
        bail!("no outliner for {}", name);
    };
    if kind == Kind::Markdown && !address.is_some_and(is_range) {
        let reading = mdread::read_file(
            file,
            address,
            None,
            false,
            mdread::DEFAULT_THRESHOLD,
            mdread::Dialect::default(),
        )?;
        mdread::render::write_text(out, &reading)?;
        return Ok(());
    }
    let content = std::fs::read_to_string(file).with_context(|| format!("cannot read {}", name))?;
    match address {
        Some(range) => lines(out, &content, range),
        None => {
            let Kind::Code(lang) = kind else {
                unreachable!("Markdown returned above")
            };
            writeln!(out, "{} · {} lines", name, content.lines().count())?;
            for node in outline::outline(lang, &content) {
                tree(out, &node, 1)?;
            }
            writeln!(out, "next: seek read {} <first>-<last>", name)?;
            Ok(())
        }
    }
}

fn is_range(address: &str) -> bool {
    let (a, b) = address.split_once('-').unwrap_or((address, address));
    a.parse::<usize>().is_ok() && b.parse::<usize>().is_ok()
}

fn tree(out: &mut impl Write, node: &outline::Node, depth: usize) -> Result<()> {
    writeln!(
        out,
        "{}{}-{}  {}",
        "  ".repeat(depth),
        node.start,
        node.end,
        node.label
    )?;
    for child in &node.children {
        tree(out, child, depth + 1)?;
    }
    Ok(())
}

fn lines(out: &mut impl Write, content: &str, range: &str) -> Result<()> {
    let (a, b) = range.split_once('-').unwrap_or((range, range));
    let (Ok(first), Ok(last)) = (a.parse::<usize>(), b.parse::<usize>()) else {
        bail!("a code range is <first>-<last>, got {:?}", range);
    };
    if first == 0 || last < first {
        bail!("empty range {:?}", range);
    }
    let width = last.to_string().len();
    for (no, line) in content
        .lines()
        .enumerate()
        .skip(first - 1)
        .take(last + 1 - first)
    {
        writeln!(out, "{:>width$}  {}", no + 1, line)?;
    }
    Ok(())
}
