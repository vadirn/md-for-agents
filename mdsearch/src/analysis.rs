//! Text analysis shared by every index built on this core: the stemming chain
//! and the query tokenizer.
//!
//! [`crate::Corpus`] analyzes both its documents and every query through this
//! one chain. Analyzing a query differently from the corpus silently skews
//! relevance.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Mutex, PoisonError};

use rust_stemmers::{Algorithm, Stemmer};

/// A token of this many bytes or more is dropped before it is indexed. Such a
/// run is a hash, a base64 blob or a minified line, never a word anyone types.
const MAX_TOKEN_BYTES: usize = 40;

/// The most words the stem cache remembers. A word past it is stemmed afresh,
/// so a long-lived corpus answering many queries stops growing its cache.
const MAX_STEMS: usize = 1 << 16;

/// One term the chain emits, with the byte range of the word it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub span: Range<usize>,
}

/// The analysis chain: split on every character that is not alphanumeric, drop
/// tokens of [`MAX_TOKEN_BYTES`] or more, lowercase, then stem English and
/// Russian in turn.
///
/// English stemming mutates Latin suffixes and passes Cyrillic through unchanged;
/// Russian stemming does the reverse. Chaining them stems both languages without
/// corrupting either.
///
/// Stemming dominates indexing, and a corpus repeats a small vocabulary many
/// times over, so each lowercased word is stemmed once and remembered.
pub struct Analyzer {
    english: Stemmer,
    russian: Stemmer,
    stems: Mutex<HashMap<String, String>>,
}

impl Analyzer {
    /// Every token of `text` in order, each with its source span.
    pub fn tokens(&self, text: &str) -> Vec<Token> {
        words(text)
            .filter(|span| span.len() < MAX_TOKEN_BYTES)
            .map(|span| Token {
                text: self.normalize(&text[span.clone()]),
                span,
            })
            .collect()
    }

    /// Every term of `text` in order, repeats kept.
    ///
    /// A query reaches the index through this and nothing else. No character is
    /// reserved, because nothing parses the text: it is tokenized and looked up. A
    /// quotation mark, an apostrophe, a colon or a bracket separates words and
    /// carries no meaning.
    pub fn terms(&self, text: &str) -> Vec<String> {
        self.tokens(text)
            .into_iter()
            .map(|token| token.text)
            .collect()
    }

    fn normalize(&self, word: &str) -> String {
        let lower = if word.is_ascii() {
            word.to_ascii_lowercase()
        } else {
            // Char by char, so Σ lowercases to σ wherever it stands in a word.
            word.chars().flat_map(char::to_lowercase).collect()
        };
        let mut stems = self.stems.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(stem) = stems.get(&lower) {
            return stem.clone();
        }
        let stem = self.russian.stem(&self.english.stem(&lower)).into_owned();
        if stems.len() < MAX_STEMS {
            stems.insert(lower, stem.clone());
        }
        stem
    }
}

/// The byte spans of the maximal alphanumeric runs in `text`.
fn words(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        let (start, _) = chars.find(|(_, c)| c.is_alphanumeric())?;
        let mut end = text.len();
        while let Some(&(at, c)) = chars.peek() {
            if !c.is_alphanumeric() {
                end = at;
                break;
            }
            chars.next();
        }
        Some(start..end)
    })
}

/// Build the analysis chain every field of a [`crate::Corpus`] is indexed with.
pub fn bilingual_analyzer() -> Analyzer {
    Analyzer {
        english: Stemmer::create(Algorithm::English),
        russian: Stemmer::create(Algorithm::Russian),
        stems: Mutex::new(HashMap::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(text: &str) -> Vec<String> {
        bilingual_analyzer().terms(text)
    }

    #[test]
    fn punctuation_separates_terms_instead_of_carrying_meaning() {
        // Each of these is a query-language metacharacter elsewhere, and none is reserved here.
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

    #[test]
    fn a_token_of_forty_bytes_or_more_is_dropped() {
        let long = "a".repeat(40);
        let kept = "b".repeat(39);
        assert_eq!(terms(&format!("{long} {kept}")), [kept]);
    }

    #[test]
    fn a_token_keeps_the_span_of_its_source_word() {
        let text = "Ранние Tokens, then more";
        let tokens = bilingual_analyzer().tokens(text);
        let spans: Vec<&str> = tokens.iter().map(|t| &text[t.span.clone()]).collect();
        assert_eq!(spans, ["Ранние", "Tokens", "then", "more"]);
        assert_eq!(tokens[1].text, "token");
    }
}
