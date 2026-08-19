//! What one read resolved to.
//!
//! [`Reading`] is what the library returns and what the renderer consumes, so a
//! caller receives the same data the CLI prints. Field order here is the JSON
//! output's field order, which a golden test pins.

use serde::{Serialize, Serializer, ser::SerializeStruct};

/// The five things an address resolves to.
#[derive(Debug)]
pub enum Reading {
    Overview(Overview),
    Frontmatter(Frontmatter),
    FrontmatterValue(FrontmatterValue),
    Links(Links),
    Unfold(Unfold),
}

impl Reading {
    /// A heading shadows the reserved address just served, so the caller would
    /// otherwise never learn the heading exists. Belongs on stderr: the payload
    /// on stdout stays byte-identical in both formats, and a note cannot corrupt
    /// it.
    ///
    /// An overview carries its shadows in [`Overview::notes`] instead, since it
    /// reports on the tree it just printed rather than on one served address.
    pub fn note(&self) -> Option<&str> {
        match self {
            Reading::Frontmatter(f) => f.note.as_deref(),
            Reading::Links(l) => l.note.as_deref(),
            Reading::Unfold(u) => u.note.as_deref(),
            // A value address names a path inside the block, which no heading
            // can spell, so it has no shadow to report.
            Reading::Overview(_) | Reading::FrontmatterValue(_) => None,
        }
    }
}

/// The folded whole-file view: every section as one line, with its size.
#[derive(Debug, Serialize)]
pub struct Overview {
    pub path: String,
    pub fields: Vec<String>,
    pub links: usize,
    pub text: Option<TextNode>,
    pub tree: Vec<TreeNode>,
    /// Headings that also answer to a reserved address.
    #[serde(skip)]
    pub notes: Vec<String>,
}

/// The lede: the prose before the first heading, addressed `0`.
#[derive(Debug, Serialize)]
pub struct TextNode {
    pub address: String,
    pub label: String,
    pub line: usize,
    pub lines: usize,
    pub tokens: usize,
}

/// One section in the folded tree, with its descendants nested under it.
#[derive(Debug, Serialize)]
pub struct TreeNode {
    pub address: String,
    pub heading: String,
    pub level: usize,
    pub line: usize,
    pub lines: usize,
    pub tokens: usize,
    pub slug: String,
    pub children: Vec<TreeNode>,
}

/// The frontmatter block, listed field by field.
#[derive(Debug, Serialize)]
pub struct Frontmatter {
    pub path: String,
    pub address: String,
    pub line: usize,
    pub lines: usize,
    pub fields: Vec<FrontmatterField>,
    /// The block's inner YAML, which text output prints whole. JSON serves the
    /// same block through `fields` instead.
    #[serde(skip)]
    pub text: String,
    #[serde(skip)]
    pub note: Option<String>,
}

/// One top-level frontmatter key, in source order.
#[derive(Debug, Serialize)]
pub struct FrontmatterField {
    pub key: String,
    pub value: String,
    pub line: usize,
}

/// One value addressed inside the frontmatter, as `fm.<path>`.
#[derive(Debug)]
pub struct FrontmatterValue {
    pub path: String,
    pub address: String,
    /// The value with its YAML type intact, so a list stays a list and a number
    /// stays a number. Text output renders it as YAML; JSON converts it.
    pub value: serde_yaml::Value,
}

impl Serialize for FrontmatterValue {
    /// Hand-written because the YAML value is the data, and JSON is one
    /// rendering of it. A value JSON cannot represent — a mapping with
    /// non-string keys — serializes as null rather than failing the read.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("FrontmatterValue", 3)?;
        out.serialize_field("path", &self.path)?;
        out.serialize_field("address", &self.address)?;
        let value = serde_json::to_value(&self.value).unwrap_or(serde_json::Value::Null);
        out.serialize_field("value", &value)?;
        out.end()
    }
}

/// The outgoing links the overview only counted.
#[derive(Debug, Serialize)]
pub struct Links {
    pub path: String,
    pub address: String,
    pub links: Vec<Link>,
    #[serde(skip)]
    pub note: Option<String>,
}

/// One outgoing link.
#[derive(Debug, Serialize)]
pub struct Link {
    pub kind: &'static str,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub line: usize,
}

/// One addressed section, smart-unfolded: its own prose, plus each child either
/// inlined or folded to a placeholder.
#[derive(Debug, Serialize)]
pub struct Unfold {
    pub path: String,
    pub address: String,
    pub heading: String,
    pub slug: String,
    pub level: usize,
    pub line: usize,
    pub lines: usize,
    pub tokens: usize,
    /// The node's own prose, stopping at its first child.
    pub content: String,
    pub children: Vec<UnfoldChild>,
    /// The whole unfolded section as one block: own prose, then each child
    /// inlined or folded. Text output prints this. It comes from the same walker
    /// that fills `children`, so the two cannot disagree.
    #[serde(skip)]
    pub text: String,
    #[serde(skip)]
    pub note: Option<String>,
}

/// One child of an unfolded section. `content` is present when the child was
/// inlined, absent when it was folded.
#[derive(Debug, Serialize)]
pub struct UnfoldChild {
    pub address: String,
    pub heading: String,
    pub level: usize,
    pub line: usize,
    pub lines: usize,
    pub tokens: usize,
    pub folded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}
