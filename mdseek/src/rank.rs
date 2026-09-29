//! Rank sections against a question, then decide whether the best answers it.
//!
//! Ranking is mdsearch's BM25 core over one document per section. The verdict
//! is consult's relative gate: some top-3 section must hold enough of the
//! question's terms, and the top score must stand clear of the median. A
//! `no-answer` carries terms the caller may add, drawn from the best sections
//! by pseudo-relevance feedback.

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::Result;
use mdsearch::{Corpus, Doc, Scoring, analysis};
use tantivy::Term;

use crate::sections::{Kind, Section, split_identifiers};

/// Hits the gate compares and the output may list.
pub const POOL: usize = 10;

/// The share of the question's terms some top-3 section must hold. Consult's
/// deliberate-mode default.
const COVERAGE: f32 = 0.45;

/// How far the top score must stand above the median. Consult's default.
const ELBOW: f32 = 1.5;

/// Suggested terms a `no-answer` offers.
const SUGGESTIONS: usize = 8;

/// Words that carry a question's grammar, not its subject. Coverage counts
/// only the rest, because a definition never says "which" or "where".
const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "do", "doe", "does", "for", "from",
    "get", "has", "have", "how", "i", "in", "is", "it", "its", "me", "my", "of", "on", "or", "s",
    "that", "the", "their", "them", "then", "there", "thi", "this", "to", "wa", "was", "what",
    "when", "where", "which", "who", "whi", "why", "will", "with", "you", "your",
];

/// A test section's score is scaled by this. Tests stay findable, but a
/// definition that matches as well outranks them.
const TEST_WEIGHT: f32 = 0.5;

/// Words a question is phrased with rather than about. They leave the query
/// before BM25 sees it, because code uses several of them as keywords
/// (`into`, `for`, `in`, `is`, `self`) and would match them everywhere.
const QUESTION_WORDS: &[&str] = &[
    "a",
    "about",
    "all",
    "an",
    "and",
    "any",
    "are",
    "as",
    "at",
    "be",
    "been",
    "being",
    "but",
    "by",
    "can",
    "class",
    "classes",
    "code",
    "could",
    "definition",
    "defined",
    "did",
    "do",
    "does",
    "doing",
    "done",
    "each",
    "file",
    "files",
    "find",
    "for",
    "from",
    "function",
    "functions",
    "had",
    "has",
    "have",
    "having",
    "he",
    "her",
    "here",
    "his",
    "how",
    "i",
    "if",
    "in",
    "into",
    "is",
    "it",
    "its",
    "may",
    "me",
    "method",
    "methods",
    "might",
    "must",
    "my",
    "no",
    "not",
    "note",
    "notes",
    "of",
    "on",
    "onto",
    "or",
    "our",
    "part",
    "section",
    "sections",
    "she",
    "should",
    "so",
    "some",
    "such",
    "than",
    "that",
    "the",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "this",
    "those",
    "to",
    "us",
    "was",
    "we",
    "were",
    "what",
    "when",
    "where",
    "which",
    "who",
    "whom",
    "whose",
    "why",
    "will",
    "with",
    "would",
    "you",
    "your",
];

/// The question without its [`QUESTION_WORDS`].
fn subject_words(question: &str) -> String {
    question
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty() && !QUESTION_WORDS.contains(&w.to_lowercase().as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Field weights. A section's label and path title it, and a note's
/// frontmatter description counts for less than the section's own words,
/// because it describes the whole note, not this section.
const SCORING: Scoring = Scoring {
    title: 1.0,
    description: 0.5,
};

/// Options an evaluation turns off, one at a time.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub split_identifiers: bool,
    pub descriptions: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            split_identifiers: true,
            descriptions: true,
        }
    }
}

pub struct Index {
    corpus: Corpus,
    sections: Vec<Section>,
    tokens: Vec<HashSet<String>>,
}

#[derive(Debug, Clone)]
pub struct Ranked {
    pub section: usize,
    pub score: f32,
}

#[derive(Debug)]
pub struct Outcome {
    pub answered: bool,
    pub reason: Option<&'static str>,
    pub hits: Vec<Ranked>,
    pub coverage: f32,
    pub elbow: Option<f32>,
    pub suggestions: Vec<String>,
}

impl Index {
    pub fn build(sections: Vec<Section>, options: Options) -> Result<Index> {
        let docs: Vec<Doc> = sections
            .iter()
            .enumerate()
            .map(|(k, s)| to_doc(k, s, options))
            .collect();
        let mut analyzer = analysis::bilingual_analyzer();
        let tokens = docs
            .iter()
            .map(|d| {
                let text = format!("{}\n{}", d.title, d.body);
                analysis::query_terms(&mut analyzer, &text)
                    .into_iter()
                    .collect()
            })
            .collect();
        let corpus = Corpus::build(&docs)?;
        Ok(Index {
            corpus,
            sections,
            tokens,
        })
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub fn search(&self, question: &str, limit: usize) -> Result<Outcome> {
        let subject = subject_words(question);
        let query = if subject.trim().is_empty() {
            question
        } else {
            subject.as_str()
        };
        let mut hits: Vec<Ranked> = self
            .corpus
            .search(query, limit.max(POOL * 3), SCORING)?
            .into_iter()
            .filter_map(|h| {
                let section: usize = h.id.parse().ok()?;
                let weight = if self.sections[section].test {
                    TEST_WEIGHT
                } else {
                    1.0
                };
                Some(Ranked {
                    section,
                    score: h.score * weight,
                })
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        let terms = self.content_terms(question);
        let coverage = hits
            .iter()
            .take(3)
            .map(|h| coverage_of(&terms, &self.tokens[h.section]))
            .fold(0.0f32, f32::max);
        let elbow = elbow_of(&self.best_per_file(&hits));
        let reason = if hits.is_empty() {
            Some("nothing matched")
        } else if coverage < COVERAGE {
            Some("low coverage")
        } else if elbow.is_some_and(|e| e < ELBOW) {
            Some("no score elbow")
        } else {
            None
        };
        let suggestions = if reason.is_some() {
            self.suggest(&terms, &hits)
        } else {
            Vec::new()
        };
        hits.truncate(limit);
        Ok(Outcome {
            answered: reason.is_none(),
            reason,
            hits,
            coverage,
            elbow,
            suggestions,
        })
    }

    /// The best hit of each file, up to [`POOL`] files. The elbow compares
    /// files, as consult's does, so sibling sections of one note never flatten it.
    fn best_per_file(&self, hits: &[Ranked]) -> Vec<Ranked> {
        let mut seen = HashSet::new();
        hits.iter()
            .filter(|h| seen.insert(self.sections[h.section].path.as_str()))
            .take(POOL)
            .cloned()
            .collect()
    }

    fn content_terms(&self, question: &str) -> BTreeSet<String> {
        let mut analyzer = analysis::bilingual_analyzer();
        analysis::query_terms(&mut analyzer, question)
            .into_iter()
            .filter(|t| !STOP.contains(&t.as_str()))
            .collect()
    }

    /// Terms the best sections share that the question lacks, rarest in the
    /// corpus first: pseudo-relevance feedback. Surface forms print, because an
    /// agent types words, not stems.
    /// The terms a `no-answer` would suggest for `question`, given its hits.
    pub fn suggestions(&self, question: &str, hits: &[Ranked]) -> Vec<String> {
        self.suggest(&self.content_terms(question), hits)
    }

    fn suggest(&self, terms: &BTreeSet<String>, hits: &[Ranked]) -> Vec<String> {
        let searcher = match self.corpus.index().reader() {
            Ok(r) => r.searcher(),
            Err(_) => return Vec::new(),
        };
        let body = self.corpus.fields().body;
        let total = searcher.num_docs().max(1) as f32;
        let mut analyzer = analysis::bilingual_analyzer();
        let mut weight: HashMap<String, f32> = HashMap::new();
        let mut surface: HashMap<String, HashMap<String, usize>> = HashMap::new();
        for hit in hits.iter().take(5) {
            let text = &self.sections[hit.section].text;
            let mut seen = HashSet::new();
            for word in text.split(|c: char| !c.is_alphanumeric()) {
                if word.chars().count() < 3 || word.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let lower = word.to_lowercase();
                let Some(stem) = analysis::query_terms(&mut analyzer, &lower)
                    .into_iter()
                    .next()
                else {
                    continue;
                };
                if terms.contains(&stem) || STOP.contains(&stem.as_str()) {
                    continue;
                }
                *surface
                    .entry(stem.clone())
                    .or_default()
                    .entry(lower)
                    .or_default() += 1;
                if seen.insert(stem.clone()) {
                    let df = searcher
                        .doc_freq(&Term::from_field_text(body, &stem))
                        .unwrap_or(0) as f32;
                    let idf = (total / (df + 1.0)).ln();
                    *weight.entry(stem).or_default() += idf.max(0.0);
                }
            }
        }
        let mut ranked: Vec<(String, f32)> = weight.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked
            .into_iter()
            .take(SUGGESTIONS)
            .filter_map(|(stem, _)| {
                let forms = surface.get(&stem)?;
                forms
                    .iter()
                    .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                    .map(|(w, _)| w.clone())
            })
            .collect()
    }
}

fn to_doc(k: usize, s: &Section, options: Options) -> Doc {
    let mut title = s.label.clone();
    for c in &s.context {
        title.push_str(" · ");
        title.push_str(c);
    }
    title.push_str(" · ");
    title.push_str(&s.path);
    let mut body = s.text.clone();
    if options.split_identifiers && matches!(s.kind, Kind::Code(_)) {
        let split = split_identifiers(&format!("{}\n{}", title, s.text));
        body.push('\n');
        body.push_str(&split);
    }
    Doc {
        id: k.to_string(),
        title,
        description: if options.descriptions {
            s.description.clone()
        } else {
            String::new()
        },
        body,
    }
}

fn coverage_of(terms: &BTreeSet<String>, tokens: &HashSet<String>) -> f32 {
    if terms.is_empty() {
        return 0.0;
    }
    terms.iter().filter(|t| tokens.contains(*t)).count() as f32 / terms.len() as f32
}

fn elbow_of(hits: &[Ranked]) -> Option<f32> {
    if hits.len() < 2 {
        return None;
    }
    let mut scores: Vec<f32> = hits.iter().map(|h| h.score).collect();
    scores.sort_by(f32::total_cmp);
    let mid = scores.len() / 2;
    let median = if scores.len().is_multiple_of(2) {
        (scores[mid - 1] + scores[mid]) / 2.0
    } else {
        scores[mid]
    };
    (median > 0.0).then(|| hits[0].score / median)
}
