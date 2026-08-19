//! What every tool in this workspace needs at its command-line edge, and none
//! of them should own alone.
//!
//! A tool here ships in two parts: a library that returns data, and a binary
//! that prints it. That leaves the printing concerns with no home — they belong
//! to no one tool's core, and [`mdstruct`] is the structural parser and carries
//! no presentation policy. This crate is that home. It has no binary and
//! depends on nothing.

mod format;
mod stdout;
mod tokens;

pub use format::TextJson;
pub use stdout::with_stdout;
pub use tokens::estimate_tokens;
