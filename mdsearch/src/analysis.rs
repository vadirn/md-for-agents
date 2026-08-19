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
/// and match nothing. `the importer's work` would fail to parse outright, because
/// Tantivy opens a phrase on `'` and on `` ` `` just as it does on `"`.
///
/// The set mirrors `SPECIAL_CHARS` in `tantivy-query-grammar`, which is private, so
/// a Tantivy upgrade can widen the grammar without widening this list.
///
/// Each metacharacter becomes a space rather than nothing. The indexing tokenizer
/// breaks on every non-alphanumeric character, so a body storing `importer's` holds
/// the two tokens `importer` and `s`. A space reproduces that pair, and a deletion
/// would ask the index for `importers`, which it never held.
pub fn sanitize_query(query: &str) -> String {
    query
        .chars()
        .map(|c| match c {
            ':' | '+' | '-' | '(' | ')' | '^' | '~' | '"' | '\'' | '`' | '*' | '?' | '[' | ']'
            | '{' | '}' | '\\' | '!' => ' ',
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

    #[test]
    fn sanitize_query_spaces_the_phrase_quotes() {
        // Tantivy opens a phrase on all three, so an odd count fails to parse.
        assert_eq!(sanitize_query("importer's work"), "importer s work");
        assert_eq!(sanitize_query("back`tick"), "back tick");
        assert_eq!(sanitize_query("say \"hello\""), "say  hello ");
    }
}
