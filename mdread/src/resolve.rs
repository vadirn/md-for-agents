//! Address resolution: map a structural address (`"0"`/`text`, a dotted-numeric
//! path, or a heading slug) onto a node in the document tree.

use anyhow::Result;

use crate::model::{Document, Node, flatten, is_numeric_address};

/// Why an address failed to resolve. Carries the data each variant needs to
/// reproduce the exact message the `resolve_address` wrapper prints.
#[derive(Debug)]
pub(crate) enum ResolveError {
    NoTextRegion(String),
    /// A dotted-numeric address that runs past the sections at one level.
    OutOfRange {
        address: String,
        /// The section whose subsections the path ran past; `None` at the top level.
        parent: Option<String>,
        /// The addresses at that level, in order.
        available: Vec<String>,
    },
    NoSlugMatch(String),
    /// Holds the candidate `(address, heading)` pairs.
    Ambiguous(String, Vec<(String, String)>),
}

/// Pure address resolution: all the descent/match logic, no IO.
/// `resolve_address` wraps this to format the error; tests call it directly so
/// there is no parallel test mirror to drift.
pub(crate) fn resolve<'a>(doc: &'a Document, address: &str) -> Result<&'a Node, ResolveError> {
    // `[0]` / `text` → the synthetic text node. Reserved, like `fm` and
    // `links`, so a heading slugging to `text` is reachable only by its number.
    // The predicate is `shadow`'s, so interception and announcement cannot
    // drift apart.
    if crate::shadow::reserved_reading(address) == Some(crate::shadow::Reserved::Text) {
        return doc
            .text
            .as_ref()
            .ok_or_else(|| ResolveError::NoTextRegion(address.to_string()));
    }

    if is_numeric_address(address) {
        let mut level: &[Node] = &doc.tree;
        let mut current: Option<&Node> = None;
        for seg in address.split('.') {
            // An all-digit segment can still overflow `usize`; treat overflow as
            // out-of-range rather than panicking. Parsing as the walk descends
            // pins the miss to the level where it happens.
            let node = match seg.parse::<usize>() {
                Ok(idx) if idx >= 1 => level.get(idx - 1),
                _ => None,
            };
            let Some(node) = node else {
                return Err(ResolveError::OutOfRange {
                    address: address.to_string(),
                    // The node's own address, so `01.9` names section 1.
                    parent: current.map(|n| n.address.clone()),
                    available: level.iter().map(|n| n.address.clone()).collect(),
                });
            };
            current = Some(node);
            level = &node.children;
        }
        return Ok(current.expect("numeric address yields a node"));
    }

    let needle = crate::slug::segment(address);
    let mut all: Vec<&Node> = Vec::new();
    flatten(&doc.tree, &mut all);
    let matches: Vec<&Node> = all.into_iter().filter(|n| n.slug == needle).collect();
    match matches.len() {
        0 => Err(ResolveError::NoSlugMatch(needle)),
        1 => Ok(matches[0]),
        _ => Err(ResolveError::Ambiguous(
            needle,
            matches
                .iter()
                .map(|n| (n.address.clone(), n.heading.clone()))
                .collect(),
        )),
    }
}

/// Resolve an address against a document, returning an error that names the
/// failure rather than exiting. `main` owns the exit code, so a miss propagates
/// with `?`. Thin wrapper over the pure `resolve`.
pub(crate) fn resolve_address<'a>(doc: &'a Document, address: &str) -> Result<&'a Node> {
    match resolve(doc, address) {
        Ok(n) => Ok(n),
        Err(ResolveError::NoTextRegion(addr)) => {
            // As with `fm`: a reserved address that resolved to nothing names the
            // heading that answers to the same word, and how to reach it.
            let mut msg = format!("No text region in this file (address '{}')", addr);
            if let Some(p) = crate::shadow::phrase(doc, &addr) {
                msg.push_str(&format!("; {}", p));
            }
            Err(anyhow::anyhow!(msg))
        }
        Err(ResolveError::OutOfRange {
            address,
            parent,
            available,
        }) => {
            // Name what the level holds, so the caller corrects the address
            // without folding the file again.
            let (owner, noun) = match &parent {
                Some(p) => (format!("section {}", p), "subsection"),
                None => ("this file".to_string(), "top-level section"),
            };
            let holds = match available.as_slice() {
                [] => format!("no {}s", noun),
                [only] => format!("1 {} ({})", noun, only),
                [first, .., last] => {
                    format!("{} {}s ({}–{})", available.len(), noun, first, last)
                }
            };
            Err(anyhow::anyhow!(
                "Address '{}' out of range; {} has {}",
                address,
                owner,
                holds
            ))
        }
        Err(ResolveError::NoSlugMatch(needle)) => {
            Err(anyhow::anyhow!("No heading matches slug '{}'", needle))
        }
        Err(ResolveError::Ambiguous(needle, candidates)) => {
            let mut msg = format!("Ambiguous slug '{}'; candidates:", needle);
            for (addr, heading) in &candidates {
                msg.push_str(&format!("\n  {}  {}", addr, heading));
            }
            Err(anyhow::anyhow!(msg))
        }
    }
}
