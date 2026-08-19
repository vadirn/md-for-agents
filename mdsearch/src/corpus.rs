//! The search core: an in-RAM BM25 index over documents a caller supplies.
//!
//! The core knows nothing about files. A caller hands it [`Doc`] values, tunes
//! the field weights through [`Scoring`], and reads back [`Hit`] values carrying
//! its own identifiers. Callers with their own retrieval logic take
//! [`Corpus::index`] and [`Corpus::fields`] and query Tantivy directly.

use std::ops::Range;

use anyhow::{Result, bail};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::snippet::SnippetGenerator;
use tantivy::{Index, IndexWriter, doc};

use crate::analysis::{TOKENIZER, bilingual_analyzer, sanitize_query};

/// Least heap Tantivy accepts for one index writer.
const WRITER_BUDGET: usize = 15_000_000;

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

/// Field handles for the schema, for a caller querying the index itself.
/// `Field` is `Copy`, so these pass straight into a query parser or a collector.
#[derive(Debug, Clone, Copy)]
pub struct Fields {
    pub id: Field,
    pub title: Field,
    pub description: Field,
    pub body: Field,
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

/// An in-RAM BM25 index over a set of documents.
///
/// The index lives as long as the `Corpus` and never touches disk, so no stale
/// index can outlive the documents it was built from.
pub struct Corpus {
    index: Index,
    fields: Fields,
}

impl Corpus {
    /// Index `docs`.
    ///
    /// Every text field is analyzed by the shared stemming chain. Only
    /// `description` goes unstored, because scoring reads it and no hit returns it.
    pub fn build(docs: &[Doc]) -> Result<Corpus> {
        let mut schema_builder = Schema::builder();
        let stored_text = || {
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer(TOKENIZER)
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored()
        };
        let indexed_only = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let title = schema_builder.add_text_field("title", stored_text());
        let description = schema_builder.add_text_field("description", indexed_only);
        let body = schema_builder.add_text_field("body", stored_text());
        let id = schema_builder.add_text_field("id", STRING | STORED);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        index.tokenizers().register(TOKENIZER, bilingual_analyzer());

        let total: usize = docs.iter().map(|d| d.body.len()).sum();
        let mut writer: IndexWriter = index.writer(total.max(WRITER_BUDGET))?;
        for document in docs {
            writer.add_document(doc!(
                title => document.title.as_str(),
                description => document.description.as_str(),
                body => document.body.as_str(),
                id => document.id.as_str(),
            ))?;
        }
        writer.commit()?;

        Ok(Corpus {
            index,
            fields: Fields {
                id,
                title,
                description,
                body,
            },
        })
    }

    /// The Tantivy index, for a caller running its own query, collector, or
    /// stored-field readback.
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// The field handles of [`Corpus::index`].
    pub fn fields(&self) -> Fields {
        self.fields
    }

    /// Rank `query` over title, description, and body, returning at most `limit`
    /// hits in descending score.
    ///
    /// The query is sanitized before parsing. A query left with no terms is an
    /// error, since parsing an empty query would report "no matches" for what is
    /// really a malformed request.
    pub fn search(&self, query: &str, limit: usize, scoring: Scoring) -> Result<Vec<Hit>> {
        let sanitized = sanitize_query(query);
        if sanitized.trim().is_empty() {
            bail!("query has no searchable terms: {:?}", query);
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let searcher = self.index.reader()?.searcher();
        let mut parser = QueryParser::for_index(
            &self.index,
            vec![self.fields.title, self.fields.description, self.fields.body],
        );
        parser.set_field_boost(self.fields.title, scoring.title);
        parser.set_field_boost(self.fields.description, scoring.description);
        let parsed = parser.parse_query(&sanitized)?;

        let top_docs = searcher.search(&parsed, &TopDocs::with_limit(limit).order_by_score())?;
        if top_docs.is_empty() {
            return Ok(Vec::new());
        }

        // The generator must come from the searcher and query that produced the hits.
        let generator = SnippetGenerator::create(&searcher, &parsed, self.fields.body)?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (score, address) in top_docs {
            let document: TantivyDocument = searcher.doc(address)?;
            let stored = |field| {
                document
                    .get_first(field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            let snippet = generator.snippet(&stored(self.fields.body));
            hits.push(Hit {
                id: stored(self.fields.id),
                title: stored(self.fields.title),
                score,
                snippet: Snippet {
                    text: snippet.fragment().to_string(),
                    highlights: snippet.highlighted().to_vec(),
                },
            });
        }
        Ok(hits)
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
        let corpus = Corpus::build(&docs()).unwrap();
        let hits = corpus
            .search("term frequency", 10, Scoring::default())
            .unwrap();
        assert_eq!(ids(&hits), vec!["retrieval"]);
    }

    #[test]
    fn the_description_is_scored_and_the_id_is_not() {
        let corpus = Corpus::build(&docs()).unwrap();
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
        let corpus = Corpus::build(&docs()).unwrap();
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
        let corpus = Corpus::build(&docs()).unwrap();
        let hits = corpus.search("tomatoes", 10, Scoring::default()).unwrap();
        let snippet = &hits[0].snippet;
        assert!(snippet.text.to_lowercase().contains("tomatoes"));
        assert!(!snippet.highlights.is_empty());
        assert!(!snippet.text.contains('<'), "got: {:?}", snippet.text);
    }

    #[test]
    fn limit_truncates_and_zero_returns_nothing() {
        let corpus = Corpus::build(&docs()).unwrap();
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
        let corpus = Corpus::build(&docs()).unwrap();
        let err = corpus.search("***", 10, Scoring::default()).unwrap_err();
        assert!(
            err.to_string().contains("no searchable terms"),
            "got: {}",
            err
        );
    }

    #[test]
    fn an_apostrophe_searches_the_words_around_it() {
        let corpus = Corpus::build(&[Doc {
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
    fn an_empty_corpus_answers_without_matching() {
        let corpus = Corpus::build(&[]).unwrap();
        assert!(
            corpus
                .search("alpha", 10, Scoring::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_index_and_fields_are_reachable_for_a_caller_query() {
        let corpus = Corpus::build(&docs()).unwrap();
        let searcher = corpus.index().reader().unwrap().searcher();
        assert_eq!(searcher.num_docs(), 2);
        let parser = QueryParser::for_index(corpus.index(), vec![corpus.fields().body]);
        let parsed = parser.parse_query("tomatoes").unwrap();
        let found = searcher
            .search(&parsed, &TopDocs::with_limit(10).order_by_score())
            .unwrap();
        assert_eq!(found.len(), 1);
    }
}
