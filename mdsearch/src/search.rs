//! Ranked search over the index: parse the query, rank the files, render the hits.

use std::io::{self, Write};
use std::ops::Range;
use std::path::Path;

use anyhow::{Result, bail};
use serde::Serialize;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::snippet::SnippetGenerator;

use crate::format::TextJson;
use crate::index::{self, sanitize_query};
use crate::scan::{self, Walk};
use crate::tokens::estimate_tokens;

/// Weight of the file name against the body, which scores at 1.0. A name states a
/// subject without arguing it, so it earns no premium.
pub const TITLE_BOOST: f32 = 1.0;

/// Weight of the frontmatter `description:`. A writer curates it, so it outranks
/// an incidental mention in the prose.
pub const DESCRIPTION_BOOST: f32 = 1.5;

/// One ranked file.
#[derive(Debug, Serialize)]
pub struct Hit {
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
    pub results: Vec<Hit>,
}

/// One hit with both snippet renderings: the plain window JSON carries, and the
/// `*term*` marked window the terminal prints.
struct Ranked {
    path: String,
    title: String,
    score: f32,
    tokens: usize,
    fragment: String,
    marked: String,
}

/// Wrap every matched span of `fragment` in `*`, so a terminal reader sees what
/// the query hit.
///
/// Two query terms can match one overlapping span, so the spans merge first;
/// marking each on its own would nest the markers inside each other.
fn mark(fragment: &str, highlighted: &[Range<usize>]) -> String {
    let mut spans: Vec<Range<usize>> = highlighted.to_vec();
    spans.sort_by_key(|s| s.start);

    let mut merged: Vec<Range<usize>> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }

    let mut out = String::with_capacity(fragment.len() + merged.len() * 2);
    let mut cursor = 0;
    for span in merged {
        out.push_str(&fragment[cursor..span.start]);
        out.push('*');
        out.push_str(&fragment[span.clone()]);
        out.push('*');
        cursor = span.end;
    }
    out.push_str(&fragment[cursor..]);
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

/// Scan, index, and rank, returning at most `limit` hits in descending score.
fn rank(query: &str, root: &Path, limit: usize, walk: Walk) -> Result<Vec<Ranked>> {
    let sanitized = sanitize_query(query);
    if sanitized.trim().is_empty() {
        bail!("query has no searchable terms: {:?}", query);
    }

    let files = scan::scan(root, walk)?;
    if files.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let (index, fields) = index::build(&files)?;
    let searcher = index.reader()?.searcher();

    let mut parser =
        QueryParser::for_index(&index, vec![fields.title, fields.description, fields.body]);
    parser.set_field_boost(fields.title, TITLE_BOOST);
    parser.set_field_boost(fields.description, DESCRIPTION_BOOST);
    let parsed = parser.parse_query(&sanitized)?;

    let top_docs = searcher.search(&parsed, &TopDocs::with_limit(limit).order_by_score())?;
    if top_docs.is_empty() {
        return Ok(Vec::new());
    }

    // The generator must come from the searcher and query that produced the hits.
    let generator = SnippetGenerator::create(&searcher, &parsed, fields.body)?;

    let mut ranked = Vec::with_capacity(top_docs.len());
    for (score, address) in top_docs {
        let doc: TantivyDocument = searcher.doc(address)?;
        let stored = |field| {
            doc.get_first(field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let body = stored(fields.body);
        let snippet = generator.snippet(&body);
        ranked.push(Ranked {
            path: stored(fields.path),
            title: stored(fields.title),
            score,
            tokens: estimate_tokens(&body),
            fragment: snippet.fragment().to_string(),
            marked: mark(snippet.fragment(), snippet.highlighted()),
        });
    }
    Ok(ranked)
}

/// Search `root` and return the ranked hits.
pub fn search(query: &str, root: &Path, limit: usize, walk: Walk) -> Result<Vec<Hit>> {
    Ok(rank(query, root, limit, walk)?
        .into_iter()
        .map(|r| Hit {
            path: r.path,
            title: r.title,
            score: r.score,
            snippet: r.fragment,
            tokens: r.tokens,
        })
        .collect())
}

/// Search `root` and print the hits in `format`.
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

    let ranked = rank(query, root, limit, walk)?;
    with_stdout(|out| {
        for hit in &ranked {
            writeln!(
                out,
                "[{:.2}] {} ({} tokens)",
                hit.score, hit.path, hit.tokens
            )?;
            for line in hit.marked.lines() {
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
