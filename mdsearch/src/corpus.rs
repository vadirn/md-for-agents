//! The search core: an in-RAM BM25 index over documents a caller supplies.
//!
//! The core knows nothing about files. A caller hands it [`Doc`] values, tunes
//! the field weights through [`Scoring`], and reads back [`Hit`] values carrying
//! its own identifiers.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use anyhow::{Result, bail};

use crate::analysis::{Analyzer, bilingual_analyzer};

/// BM25 term saturation: how fast repeats of a term stop adding score.
const K1: f32 = 1.2;
/// BM25 length normalization: how much a long field is discounted.
const B: f32 = 0.75;
/// Longest snippet fragment, in bytes of the body.
const SNIPPET_BYTES: usize = 150;

/// One document to index.
///
/// The three text fields score separately, so a caller decides what counts as a
/// title and what counts as a curated precis. Leave `description` empty when the
/// caller has no such field.
#[derive(Debug, Clone, Default)]
pub struct Doc {
    /// Identity carried back on every hit: a path, a URL, a database key.
    pub id: String,
    pub title: String,
    pub description: String,
    pub body: String,
}

/// Field weights BM25 applies. The body scores at 1.0 and anchors the other two.
#[derive(Debug, Clone, Copy)]
pub struct Scoring {
    /// Weight of the title. A title states a subject without arguing it, so the
    /// default earns it no premium.
    pub title: f32,
    /// Weight of the description. A writer curates it, so the default outranks an
    /// incidental mention in the body.
    pub description: f32,
}

impl Default for Scoring {
    fn default() -> Self {
        Scoring {
            title: 1.0,
            description: 1.5,
        }
    }
}

/// A matching window of a document's body, with the spans that matched.
///
/// Highlighting is the caller's to render, so the spans arrive as byte ranges
/// into `text` rather than as markup.
#[derive(Debug, Clone, Default)]
pub struct Snippet {
    pub text: String,
    pub highlights: Vec<Range<usize>>,
}

/// One ranked document.
#[derive(Debug, Clone)]
pub struct Hit {
    /// The `id` of the [`Doc`] that matched.
    pub id: String,
    pub title: String,
    pub score: f32,
    pub snippet: Snippet,
}

/// One field's inverted index.
#[derive(Default)]
struct Postings {
    /// Each term's documents in ascending order, with the term's count in each.
    terms: HashMap<String, Vec<(u32, u32)>>,
    /// Each document's length in terms.
    lengths: Vec<u32>,
    /// All documents' lengths summed.
    total: u64,
}

impl Postings {
    fn add(&mut self, doc: u32, terms: Vec<String>) {
        self.lengths.push(terms.len() as u32);
        self.total += terms.len() as u64;
        let mut counts: HashMap<String, u32> = HashMap::new();
        for term in terms {
            *counts.entry(term).or_default() += 1;
        }
        for (term, count) in counts {
            self.terms.entry(term).or_default().push((doc, count));
        }
    }

    fn doc_freq(&self, term: &str) -> usize {
        self.terms.get(term).map_or(0, Vec::len)
    }
}

/// An in-RAM BM25 index over a set of documents.
///
/// The index lives as long as the `Corpus` and never touches disk, so no stale
/// index can outlive the documents it was built from.
pub struct Corpus {
    ids: Vec<String>,
    titles: Vec<String>,
    bodies: Vec<String>,
    title: Postings,
    description: Postings,
    body: Postings,
    analyzer: Analyzer,
}

impl Corpus {
    /// Index `docs`.
    ///
    /// Every text field is analyzed by the shared stemming chain. Only
    /// `description` goes unstored, because scoring reads it and no hit returns it.
    pub fn build(docs: Vec<Doc>) -> Result<Corpus> {
        let analyzer = bilingual_analyzer();
        let mut corpus = Corpus {
            ids: Vec::with_capacity(docs.len()),
            titles: Vec::with_capacity(docs.len()),
            bodies: Vec::with_capacity(docs.len()),
            title: Postings::default(),
            description: Postings::default(),
            body: Postings::default(),
            analyzer,
        };
        for (k, doc) in docs.into_iter().enumerate() {
            let k = u32::try_from(k)?;
            corpus.title.add(k, corpus.analyzer.terms(&doc.title));
            corpus
                .description
                .add(k, corpus.analyzer.terms(&doc.description));
            corpus.body.add(k, corpus.analyzer.terms(&doc.body));
            corpus.ids.push(doc.id);
            corpus.titles.push(doc.title);
            corpus.bodies.push(doc.body);
        }
        Ok(corpus)
    }

    /// How many documents the index holds.
    pub fn num_docs(&self) -> usize {
        self.ids.len()
    }

    /// Rank `query` over title, description, and body, returning at most `limit`
    /// hits in descending score, each with the body window that matched best.
    ///
    /// `query` is free text, never a query language: it is analyzed into terms and
    /// looked up, so no character in it is reserved. A query holding no terms is an
    /// error, since an empty query would report "no matches" for what is really a
    /// malformed request.
    pub fn search(&self, query: &str, limit: usize, scoring: Scoring) -> Result<Vec<Hit>> {
        let terms = self.analyzer.terms(query);
        let weights = self.snippet_weights(&terms);
        Ok(self
            .top(&terms, query, limit, scoring)?
            .into_iter()
            .map(|(k, score)| Hit {
                id: self.ids[k].clone(),
                title: self.titles[k].clone(),
                score,
                snippet: self.snippet(&self.bodies[k], &weights),
            })
            .collect())
    }

    /// The best `limit` documents for `terms` with their scores, best first.
    fn top(
        &self,
        terms: &[String],
        query: &str,
        limit: usize,
        scoring: Scoring,
    ) -> Result<Vec<(usize, f32)>> {
        if terms.is_empty() {
            bail!("query has no searchable terms: {:?}", query);
        }
        let docs = self.num_docs();
        if limit == 0 || docs == 0 {
            return Ok(Vec::new());
        }
        let mut scores = vec![0.0f32; docs];
        let mut matched = vec![false; docs];
        for (postings, weight) in [
            (&self.title, scoring.title),
            (&self.description, scoring.description),
            (&self.body, 1.0),
        ] {
            self.score_field(postings, weight, terms, &mut scores, &mut matched);
        }
        let mut ranked: Vec<usize> = (0..docs).filter(|&k| matched[k]).collect();
        // Equal scores keep index order, so the ranking is deterministic.
        let order = |a: &usize, b: &usize| scores[*b].total_cmp(&scores[*a]).then(a.cmp(b));
        // Only the best `limit` need sorting, so the rest are split off unsorted.
        if ranked.len() > limit {
            ranked.select_nth_unstable_by(limit, order);
            ranked.truncate(limit);
        }
        ranked.sort_by(order);
        Ok(ranked.into_iter().map(|k| (k, scores[k])).collect())
    }

    /// Add each term's BM25 score in one field, scaled by the field's weight.
    ///
    /// Every term counts on its own, repeats included, and a document matching
    /// two terms outscores one matching a single term. That is what a bare list
    /// of words means to a reader: every word contributes, none is required.
    /// `importer's` contributes `importer` and `s` separately, and a document
    /// holding either scores. Adjacency is a phrase search, and this tool does not
    /// offer one.
    ///
    /// A zero-weight field still matches, so a document found only there ranks
    /// with a score of zero rather than vanishing.
    fn score_field(
        &self,
        postings: &Postings,
        weight: f32,
        terms: &[String],
        scores: &mut [f32],
        matched: &mut [bool],
    ) {
        let docs = self.num_docs() as f32;
        let average = postings.total as f32 / docs;
        for term in terms {
            let Some(list) = postings.terms.get(term) else {
                continue;
            };
            let df = list.len() as f32;
            let idf = (1.0 + (docs - df + 0.5) / (df + 0.5)).ln();
            let term_weight = idf * (1.0 + K1) * weight;
            for &(doc, count) in list {
                let length = postings.lengths[doc as usize] as f32;
                let norm = K1 * (1.0 - B + B * length / average);
                let count = count as f32;
                scores[doc as usize] += term_weight * count / (count + norm);
                matched[doc as usize] = true;
            }
        }
    }

    /// Each distinct query term the body holds, weighted by its rarity: a term in
    /// fewer documents marks a window as more telling.
    fn snippet_weights(&self, terms: &[String]) -> BTreeMap<String, f32> {
        terms
            .iter()
            .filter_map(|term| {
                let df = self.body.doc_freq(term);
                (df > 0).then(|| (term.clone(), 1.0 / (1.0 + df as f32)))
            })
            .collect()
    }

    /// The window of `body` whose matched terms weigh most, earliest on a tie.
    ///
    /// Windows are cut greedily: a token that would carry a window past
    /// [`SNIPPET_BYTES`] opens the next one. A body with no matched term has no
    /// snippet.
    fn snippet(&self, body: &str, weights: &BTreeMap<String, f32>) -> Snippet {
        let mut best: Option<Window> = None;
        let mut window = Window::at(0);
        for token in self.analyzer.tokens(body) {
            if token.span.end - window.start > SNIPPET_BYTES {
                window.offer_to(&mut best);
                window = Window::at(token.span.start);
            }
            window.end = token.span.end;
            if let Some(weight) = weights.get(&token.text) {
                window.score += weight;
                window.highlights.push(token.span);
            }
        }
        window.offer_to(&mut best);
        best.map(|w| Snippet {
            text: body[w.start..w.end].to_string(),
            highlights: w
                .highlights
                .iter()
                .map(|h| h.start - w.start..h.end - w.start)
                .collect(),
        })
        .unwrap_or_default()
    }
}

/// A candidate snippet: a byte range of the body and the matches inside it.
struct Window {
    start: usize,
    end: usize,
    score: f32,
    highlights: Vec<Range<usize>>,
}

impl Window {
    fn at(start: usize) -> Window {
        Window {
            start,
            end: start,
            score: 0.0,
            highlights: Vec::new(),
        }
    }

    /// Replace `best` with this window when it matched and outweighs it.
    fn offer_to(self, best: &mut Option<Window>) {
        if self.score > 0.0 && best.as_ref().is_none_or(|b| self.score > b.score) {
            *best = Some(self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> Vec<Doc> {
        vec![
            Doc {
                id: "retrieval".into(),
                title: "Retrieval".into(),
                description: "ranking documents by term frequency".into(),
                body: "BM25 scores a document against a query by term frequency.".into(),
            },
            Doc {
                id: "gardening".into(),
                title: "Gardening".into(),
                description: String::new(),
                body: "Tomatoes want six hours of sun and a deep weekly watering.".into(),
            },
        ]
    }

    fn ids(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.id.as_str()).collect()
    }

    #[test]
    fn ranks_the_matching_document_first() {
        let corpus = Corpus::build(docs()).unwrap();
        let hits = corpus
            .search("term frequency", 10, Scoring::default())
            .unwrap();
        assert_eq!(ids(&hits), vec!["retrieval"]);
    }

    #[test]
    fn the_description_is_scored_and_the_id_is_not() {
        let corpus = Corpus::build(docs()).unwrap();
        assert_eq!(
            ids(&corpus.search("ranking", 10, Scoring::default()).unwrap()),
            vec!["retrieval"]
        );
        // The id is stored for identity, so it never pulls a document into a match.
        assert!(
            corpus
                .search("gardening", 10, Scoring::default())
                .unwrap()
                .iter()
                .all(|h| h.title == "Gardening"),
            "the id field must not add its own match"
        );
    }

    #[test]
    fn scoring_weights_are_the_callers_to_set() {
        let corpus = Corpus::build(docs()).unwrap();
        let default = corpus.search("frequency", 10, Scoring::default()).unwrap();
        let flat = corpus
            .search(
                "frequency",
                10,
                Scoring {
                    title: 1.0,
                    description: 0.0,
                },
            )
            .unwrap();
        // Dropping the description weight drops the score of a hit that matched there.
        assert!(
            flat[0].score < default[0].score,
            "default {} vs flat {}",
            default[0].score,
            flat[0].score
        );
    }

    #[test]
    fn the_snippet_carries_spans_not_markup() {
        let corpus = Corpus::build(docs()).unwrap();
        let hits = corpus.search("tomatoes", 10, Scoring::default()).unwrap();
        let snippet = &hits[0].snippet;
        assert!(snippet.text.to_lowercase().contains("tomatoes"));
        assert!(!snippet.highlights.is_empty());
        assert!(!snippet.text.contains('<'), "got: {:?}", snippet.text);
    }

    #[test]
    fn limit_truncates_and_zero_returns_nothing() {
        let corpus = Corpus::build(docs()).unwrap();
        assert_eq!(corpus.search("a", 1, Scoring::default()).unwrap().len(), 1);
        assert!(
            corpus
                .search("a", 0, Scoring::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_query_of_only_punctuation_is_an_error() {
        let corpus = Corpus::build(docs()).unwrap();
        let err = corpus.search("***", 10, Scoring::default()).unwrap_err();
        assert!(
            err.to_string().contains("no searchable terms"),
            "got: {}",
            err
        );
    }

    #[test]
    fn an_apostrophe_searches_the_words_around_it() {
        let corpus = Corpus::build(vec![Doc {
            id: "importing".into(),
            title: "Importing".into(),
            description: String::new(),
            body: "The importer's work runs nightly.".into(),
        }])
        .unwrap();
        let hits = corpus
            .search("importer's work", 10, Scoring::default())
            .unwrap();
        assert_eq!(ids(&hits), ["importing"]);
    }

    #[test]
    fn every_query_metacharacter_is_ordinary_text() {
        let corpus = Corpus::build(docs()).unwrap();
        // Each spelling below is syntax in a Lucene-style query grammar: a phrase quote,
        // a field selector, negation, a required term, a boost, slop, a wildcard, a
        // group. Here every one of them is a word with punctuation around it.
        for query in [
            "tomatoes'",
            "tomatoes`",
            "\"tomatoes\"",
            "title:tomatoes",
            "-tomatoes",
            "+tomatoes",
            "tomatoes^2",
            "tomatoes~1",
            "tomatoes*",
            "(tomatoes)",
            "[tomatoes]",
            "{tomatoes}",
            "tomatoes!",
            "tomatoes\\",
        ] {
            let hits = corpus.search(query, 10, Scoring::default()).unwrap();
            assert_eq!(ids(&hits), ["gardening"], "query: {:?}", query);
        }
    }

    #[test]
    fn an_empty_corpus_answers_without_matching() {
        let corpus = Corpus::build(Vec::new()).unwrap();
        assert!(
            corpus
                .search("alpha", 10, Scoring::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn scores_follow_bm25() {
        // One term in one of two single-word bodies: idf = ln(1 + 1.5 / 1.5) = ln 2,
        // and a body at the average length scores idf * (k1 + 1) * 1 / (1 + k1).
        let corpus = Corpus::build(vec![
            Doc {
                id: "a".into(),
                body: "alpha".into(),
                ..Doc::default()
            },
            Doc {
                id: "b".into(),
                body: "beta".into(),
                ..Doc::default()
            },
        ])
        .unwrap();
        let hits = corpus.search("alpha", 10, Scoring::default()).unwrap();
        assert_eq!(ids(&hits), ["a"]);
        assert!(
            (hits[0].score - 2f32.ln()).abs() < 1e-6,
            "got {}",
            hits[0].score
        );
    }

    #[test]
    fn a_snippet_is_the_heaviest_window_and_the_earliest_on_a_tie() {
        let filler = "word ".repeat(40);
        let body = format!("rare first. {filler} rare second.");
        let corpus = Corpus::build(vec![Doc {
            id: "x".into(),
            body: body.clone(),
            ..Doc::default()
        }])
        .unwrap();
        let snippet = &corpus.search("rare", 10, Scoring::default()).unwrap()[0].snippet;
        assert!(
            snippet.text.starts_with("rare first"),
            "got {:?}",
            snippet.text
        );
        assert!(snippet.text.len() <= SNIPPET_BYTES);
        assert_eq!(&snippet.text[snippet.highlights[0].clone()], "rare");
    }
}
