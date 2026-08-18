//! Byte order mark handling.
//!
//! A UTF-8 BOM occupies three bytes on line 1 and no other line, so column
//! arithmetic has to skip it on line 1 alone.

/// A byte order mark, UTF-8 encoded: the three bytes `EF BB BF`.
pub const BOM: &str = "\u{feff}";
