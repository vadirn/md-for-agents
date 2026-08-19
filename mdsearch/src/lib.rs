//! BM25 search over Markdown, in two parts.
//!
//! [`Corpus`] is the core: an in-RAM Tantivy index over [`Doc`] values a caller
//! supplies, with the field weights in [`Scoring`] and the stemming chain in
//! [`analysis`]. It knows nothing about files, so a caller with its own corpus,
//! its own exclusion rules, or its own ranking reuses it whole — taking
//! [`Corpus::index`] to query Tantivy directly when [`Corpus::search`] is not the
//! retrieval it wants.
//!
//! [`scan`] and [`run`] are the Markdown half the `mdsearch` binary is built
//! from: walk a folder, turn each file into a `Doc` by its name, its frontmatter
//! `description:`, and the prose after that block, then print the hits. The
//! binary holds the defaults; nothing here presumes them.

pub mod analysis;
mod corpus;
mod format;
pub mod frontmatter;
mod render;
mod scan;
mod tokens;

pub use corpus::{Corpus, Doc, Fields, Hit, Scoring, Snippet};
pub use format::TextJson;
pub use render::{SearchOutput, SearchResult, run, search};
pub use scan::{MARKDOWN_EXTENSIONS, MdFile, Walk, scan};
pub use tokens::estimate_tokens;
