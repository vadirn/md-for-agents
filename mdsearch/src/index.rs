//! The BM25 index: its schema, its analysis chain, and the query sanitizer.

use anyhow::Result;
use tantivy::schema::*;
use tantivy::tokenizer::{
    Language, LowerCaser, RemoveLongFilter, SimpleTokenizer, Stemmer, TextAnalyzer,
};
use tantivy::{Index, IndexWriter, doc};

use crate::frontmatter;
use crate::scan::MdFile;

/// Least heap Tantivy accepts for one index writer.
const WRITER_BUDGET: usize = 15_000_000;

/// Replace Tantivy query-syntax metacharacters with spaces, so a query written as
/// a phrase searches for its words.
///
/// `plan the workflow: first pass` would otherwise read `workflow` as a field name
/// and match nothing.
pub(crate) fn sanitize_query(query: &str) -> String {
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
fn bilingual_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(40))
        .filter(LowerCaser)
        .filter(Stemmer::new(Language::English))
        .filter(Stemmer::new(Language::Russian))
        .build()
}

/// Field handles for the schema, returned by [`build`]. `Field` is `Copy`, so a
/// caller passes these straight into the query parser and the snippet generator.
pub(crate) struct Fields {
    pub title: Field,
    pub description: Field,
    pub body: Field,
    pub path: Field,
}

/// Build the index over `files` in RAM.
///
/// Four fields carry a file: `title` from the file name, `description` from
/// frontmatter, `body` from the prose after it, and `path` for identity. Only
/// `description` goes unstored, because scoring reads it and no result prints it.
///
/// The index lives for one search and never touches disk, so a result can never
/// describe a file that has since changed.
pub(crate) fn build(files: &[MdFile]) -> Result<(Index, Fields)> {
    let mut schema_builder = Schema::builder();
    let stored_text = || {
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored()
    };
    let indexed_only = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer("default")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let title = schema_builder.add_text_field("title", stored_text());
    let description = schema_builder.add_text_field("description", indexed_only);
    let body = schema_builder.add_text_field("body", stored_text());
    let path = schema_builder.add_text_field("path", STRING | STORED);
    let schema = schema_builder.build();

    let index = Index::create_in_ram(schema);
    index.tokenizers().register("default", bilingual_analyzer());

    let total_content: usize = files.iter().map(|f| f.content.len()).sum();
    let mut writer: IndexWriter = index.writer(total_content.max(WRITER_BUDGET))?;

    for file in files {
        writer.add_document(doc!(
            title => file.name.as_str(),
            description => frontmatter::description(&file.content),
            body => frontmatter::body(&file.content),
            path => file.relative.as_str(),
        ))?;
    }
    writer.commit()?;

    Ok((
        index,
        Fields {
            title,
            description,
            body,
            path,
        },
    ))
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
