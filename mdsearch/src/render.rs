//! The command-line half: walk a folder, search it, and print the hits.
//!
//! Everything here is presentation over the core — highlight markers, a token
//! estimate, and the JSON envelope. A library caller that wants none of it uses
//! [`crate::Corpus`] directly.

use std::collections::HashMap;
use std::io::{self, Write};
use std::ops::Range;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::corpus::{Corpus, Hit, Scoring};
use crate::format::TextJson;
use crate::scan::{self, Walk};
use crate::tokens::estimate_tokens;

/// One result as the CLI reports it.
#[derive(Debug, Serialize)]
pub struct SearchResult {
    /// Path relative to the searched folder.
    pub path: String,
    /// File name without its extension.
    pub title: String,
    pub score: f32,
    /// The window of body text the query matched.
    pub snippet: String,
    /// Estimated tokens in the body, so a caller can price the read.
    pub tokens: usize,
}

/// The JSON envelope one run prints.
#[derive(Debug, Serialize)]
pub struct SearchOutput {
    pub query: String,
    pub count: usize,
    pub results: Vec<SearchResult>,
}

/// Wrap every matched span of `text` in `*`, so a terminal reader sees what the
/// query hit.
///
/// Two query terms can match one overlapping span, so the spans merge first;
/// marking each on its own would nest the markers inside each other.
fn mark(text: &str, highlights: &[Range<usize>]) -> String {
    let mut spans: Vec<Range<usize>> = highlights.to_vec();
    spans.sort_by_key(|s| s.start);

    let mut merged: Vec<Range<usize>> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }

    let mut out = String::with_capacity(text.len() + merged.len() * 2);
    let mut cursor = 0;
    for span in merged {
        out.push_str(&text[cursor..span.start]);
        out.push('*');
        out.push_str(&text[span.clone()]);
        out.push('*');
        cursor = span.end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Run `render` against a locked stdout, treating a closed pipe as a clean stop.
///
/// `println!` panics once a downstream reader exits, which `mdsearch … | head`
/// does by design.
fn with_stdout<F>(render: F) -> Result<()>
where
    F: FnOnce(&mut io::StdoutLock) -> io::Result<()>,
{
    let mut out = io::stdout().lock();
    match render(&mut out) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Walk `root`, index what the walk admits, and rank `query` against it.
///
/// Returns the hits and the token estimate of each hit's body, which the core
/// does not carry because a token is a presentation unit.
fn ranked(
    query: &str,
    root: &Path,
    limit: usize,
    walk: Walk,
    scoring: Scoring,
) -> Result<(Vec<Hit>, HashMap<String, usize>)> {
    let files = scan::scan(root, walk)?;
    let docs: Vec<_> = files.iter().map(|f| f.to_doc()).collect();
    let tokens = docs
        .iter()
        .map(|d| (d.id.clone(), estimate_tokens(&d.body)))
        .collect();
    let hits = Corpus::build(&docs)?.search(query, limit, scoring)?;
    Ok((hits, tokens))
}

/// Search the Markdown under `root` and return the results the CLI prints.
pub fn search(query: &str, root: &Path, limit: usize, walk: Walk) -> Result<Vec<SearchResult>> {
    let (hits, tokens) = ranked(query, root, limit, walk, Scoring::default())?;
    Ok(hits
        .into_iter()
        .map(|hit| SearchResult {
            title: hit.title,
            score: hit.score,
            snippet: hit.snippet.text,
            tokens: tokens.get(&hit.id).copied().unwrap_or(0),
            path: hit.id,
        })
        .collect())
}

/// Search the Markdown under `root` and print the results in `format`.
///
/// JSON always prints an envelope, empty results included. Text prints nothing
/// when nothing matched, the way a line-matching search does.
pub fn run(query: &str, root: &Path, limit: usize, format: TextJson, walk: Walk) -> Result<()> {
    if format == TextJson::Json {
        let results = search(query, root, limit, walk)?;
        let output = SearchOutput {
            query: query.to_string(),
            count: results.len(),
            results,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let (hits, tokens) = ranked(query, root, limit, walk, Scoring::default())?;
    with_stdout(|out| {
        for hit in &hits {
            let count = tokens.get(&hit.id).copied().unwrap_or(0);
            writeln!(out, "[{:.2}] {} ({} tokens)", hit.score, hit.id, count)?;
            for line in mark(&hit.snippet.text, &hit.snippet.highlights).lines() {
                writeln!(out, "  {}", line)?;
            }
            writeln!(out)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_wraps_each_matched_span() {
        let one = 6..10;
        assert_eq!(
            mark("alpha beta gamma", std::slice::from_ref(&one)),
            "alpha *beta* gamma"
        );
        assert_eq!(mark("alpha beta", &[0..5, 6..10]), "*alpha* *beta*");
    }

    #[test]
    fn mark_leaves_an_unmatched_fragment_alone() {
        assert_eq!(mark("alpha beta", &[]), "alpha beta");
    }

    #[test]
    fn mark_merges_overlapping_spans_into_one_pair() {
        // Two terms matching one span would otherwise nest as "*al*pha**".
        assert_eq!(mark("alpha beta", &[0..2, 0..5]), "*alpha* beta");
        assert_eq!(mark("alpha beta", &[0..3, 2..5]), "*alpha* beta");
    }

    #[test]
    fn mark_keeps_markup_characters_as_written() {
        // The fragment is raw text: quotes and ampersands stay themselves.
        let fragment = r#"a "quoted" & <angled> word"#;
        assert_eq!(mark(fragment, &[]), fragment);
    }
}
