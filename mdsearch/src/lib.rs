//! BM25 search over the Markdown files in a folder.
//!
//! One run walks the folder, indexes what the exclusion files admit, and ranks
//! the files against the query. The index lives in RAM for that run alone, so a
//! result always describes the files as they are now, and nothing needs
//! reindexing after an edit.
//!
//! A file enters the index as four fields: its name, its frontmatter
//! `description:`, the prose after that block, and its relative path. A hit
//! reports the path, the score, a matching window, and an estimated token count.

mod format;
mod frontmatter;
mod index;
mod scan;
mod search;
mod tokens;

pub use format::TextJson;
pub use scan::{IGNORE_FILE, Walk};
pub use search::{DESCRIPTION_BOOST, Hit, SearchOutput, TITLE_BOOST, run, search};

/// Hits one run reports unless the caller asks for a different count.
pub const DEFAULT_LIMIT: usize = 10;
