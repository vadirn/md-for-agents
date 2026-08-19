//! Text analysis shared by every index built on this core: the stemming chain
//! and the query sanitizer.
//!
//! A caller running its own query against [`crate::Corpus`] uses these to match
//! how the documents were indexed. Analyzing a query differently from the corpus
//! silently skews relevance.

use tantivy::tokenizer::{
    Language, LowerCaser, RemoveLongFilter, SimpleTokenizer, Stemmer, TextAnalyzer,
};

/// Name the analysis chain registers under, and the one every field declares.
pub const TOKENIZER: &str = "default";

/// Replace Tantivy query-syntax metacharacters with spaces, so a query written as
/// a phrase searches for its words.
///
/// `plan the workflow: first pass` would otherwise read `workflow` as a field name
/// and match nothing.
pub fn sanitize_query(query: &str) -> String {
    query
        .chars()
        .map(|c| match c {
            ':' | '+' | '-' | '(' | ')' | '^' | '~' | '"' | '*' | '?' | '[' | ']' | '{' | '}'
            | '\\' | '!' => ' ',
            other => other,
        })
        .collect()
}

/// Build the analysis chain: tokenize, drop tokens over 40 bytes, lowercase, then
/// stem English and Russian in turn.
///
/// English stemming mutates Latin suffixes and passes Cyrillic through unchanged;
/// Russian stemming does the reverse. Chaining them stems both languages without
/// corrupting either.
pub fn bilingual_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(40))
        .filter(LowerCaser)
        .filter(Stemmer::new(Language::English))
        .filter(Stemmer::new(Language::Russian))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_query_replaces_metacharacters() {
        assert_eq!(
            sanitize_query("structure the workflow: plan first"),
            "structure the workflow  plan first"
        );
        assert_eq!(sanitize_query("retry - backoff"), "retry   backoff");
        assert_eq!(sanitize_query("title:value"), "title value");
        assert_eq!(sanitize_query("no specials here"), "no specials here");
    }
}
