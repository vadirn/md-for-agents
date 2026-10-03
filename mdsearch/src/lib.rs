//! BM25 search over Markdown, in two parts.
//!
//! [`Corpus`] is the core: an in-RAM BM25 index over [`Doc`] values a caller
//! supplies, with the field weights in [`Scoring`] and the stemming chain in
//! [`analysis`]. It knows nothing about files, so a caller with its own corpus
//! or its own exclusion rules reuses it whole.
//!
//! [`scan`] and [`run`] are the Markdown half the `mdsearch` binary is built
//! from: walk one or more folders as one corpus, turn each file into a `Doc` by
//! its name, its frontmatter `description:`, and the prose after that block,
//! then print the hits. The binary holds the defaults; nothing here presumes
//! them.

pub mod analysis;
mod corpus;
pub mod frontmatter;
mod render;
mod scan;

// Re-exported so a caller reads a `SearchResult` without also depending on the
// crate these come from.
pub use cli::{TextJson, estimate_tokens};
pub use corpus::{Corpus, Doc, Hit, Scoring, Snippet};
pub use render::{SearchOutput, SearchResult, run, search};
pub use scan::{MARKDOWN_EXTENSIONS, MdFile, NotAFolder, Walk, scan};
