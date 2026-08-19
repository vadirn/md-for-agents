//! Text analysis shared by every index built on this core: the stemming chain
//! and the query tokenizer.
//!
//! A caller running its own query against [`crate::Corpus`] uses these to match
//! how the documents were indexed. Analyzing a query differently from the corpus
//! silently skews relevance.

use tantivy::tokenizer::{
    Language, LowerCaser, RemoveLongFilter, SimpleTokenizer, Stemmer, TextAnalyzer,
};

/// Name the analysis chain registers under, and the one every field declares.
pub const TOKENIZER: &str = "default";

/// Split `text` into the terms an index built on `analyzer` actually holds.
///
/// This is how a query reaches the index without passing through Tantivy's query
/// grammar. The grammar reserves a set of metacharacters — quotation marks, an
/// apostrophe, a backtick, `+`, `-`, `:`, `^`, `~`, brackets, braces — so text a
/// person would type as an ordinary phrase can fail to parse, and guarding it
/// means mirroring a character set the grammar keeps private. Here no character
/// is reserved, because nothing parses the text: it is tokenized and looked up.
///
/// Pass the analyzer the field itself declares, from
/// [`tantivy::Index::tokenizer_for_field`]. Building a fresh chain risks drifting
/// from the one the documents were indexed with, which is the skew this module's
/// header warns about.
pub fn query_terms(analyzer: &mut TextAnalyzer, text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    analyzer
        .token_stream(text)
        .process(&mut |token| terms.push(token.text.clone()));
    terms
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

    fn terms(text: &str) -> Vec<String> {
        query_terms(&mut bilingual_analyzer(), text)
    }

    #[test]
    fn punctuation_separates_terms_instead_of_carrying_meaning() {
        // Each of these is a Tantivy query metacharacter, and none is reserved here.
        // "importer" stems to "import", the same form the indexed body holds.
        assert_eq!(terms("importer's work"), ["import", "s", "work"]);
        assert_eq!(terms("back`tick"), ["back", "tick"]);
        assert_eq!(terms("say \"hello\""), ["say", "hello"]);
        assert_eq!(terms("title:value"), ["titl", "valu"]);
        assert_eq!(terms("retry - backoff"), ["retri", "backoff"]);
        assert_eq!(
            terms("plan the workflow: first pass"),
            ["plan", "the", "workflow", "first", "pass"]
        );
    }

    #[test]
    fn a_query_of_only_punctuation_yields_no_terms() {
        assert!(terms("***").is_empty());
        assert!(terms("   ").is_empty());
    }

    #[test]
    fn a_term_is_stemmed_the_way_the_corpus_is() {
        assert_eq!(terms("running"), terms("runs"));
        assert_eq!(terms("документы"), terms("документа"));
    }
}
