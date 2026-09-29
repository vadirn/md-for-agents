//! Outline source code without a parser.
//!
//! Each outliner reads a file's definitions and their line ranges from what
//! delimits blocks in that language. Braces delimit Rust and TypeScript,
//! indentation delimits Python, and keyword pairs delimit Ruby. A small lexer
//! skips comments, strings and regexes first, so every delimiter it counts is code.
//!
//! The result is a tree of [`Node`] values, one per top-level item, with the
//! members of a container (an impl, trait, mod, class or module) as children.
//! Nothing here depends on Markdown, so a heading tree fits the same shape.

use std::borrow::Cow;

mod pychar;
mod python;
mod ruby;
mod rust;
mod typescript;

/// One outlined definition: a function, method, class, impl block or similar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The definition's first code line, trimmed: its signature, or a heading.
    pub label: String,
    /// Inclusive 1-based line range. It starts at the first line of the item,
    /// which may be a decorator or an attribute, and ends at its last code line.
    pub start: usize,
    pub end: usize,
    pub children: Vec<Node>,
}

/// A language an outliner exists for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    TypeScript,
    Python,
    Ruby,
}

impl Lang {
    /// The language a file name implies, by its extension or its whole name.
    pub fn of(file_name: &str) -> Option<Lang> {
        let name = file_name.rsplit('/').next().unwrap_or(file_name);
        if matches!(name, "Gemfile" | "Rakefile" | "config.ru" | "Guardfile") {
            return Some(Lang::Ruby);
        }
        let ext = name.rsplit_once('.')?.1;
        match ext {
            "rs" => Some(Lang::Rust),
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => Some(Lang::TypeScript),
            "py" | "pyi" => Some(Lang::Python),
            "rb" | "rake" | "gemspec" | "ru" => Some(Lang::Ruby),
            _ => None,
        }
    }
}

/// Outline `text` as `lang`.
///
/// A `\r\n` counts as one line break, as a `\n` does, so a CRLF file outlines
/// as its LF twin. A lone `\r` breaks no line.
pub fn outline(lang: Lang, text: &str) -> Vec<Node> {
    let text: Cow<str> = if text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(text)
    };
    let text = text.as_ref();
    // Split once: every node's label reads its own lines.
    let lines: Vec<&str> = text.lines().collect();
    let spans = match lang {
        Lang::Rust => rust::outline(text),
        Lang::TypeScript => typescript::outline(text),
        Lang::Python => python::outline(text),
        Lang::Ruby => return nest(ruby::outline(text), &lines),
    };
    spans
        .into_iter()
        .map(|span| to_node(span, &lines))
        .collect()
}

/// A span an outliner produced, before it gets a label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) children: Vec<Span>,
}

fn to_node(span: Span, lines: &[&str]) -> Node {
    Node {
        label: label(lines, span.start, span.end),
        start: span.start,
        end: span.end,
        children: span
            .children
            .into_iter()
            .map(|child| to_node(child, lines))
            .collect(),
    }
}

/// Nest flat spans by containment, for an outliner that reports every block
/// at once. Spans arrive in any order and never partly overlap.
fn nest(mut spans: Vec<(usize, usize)>, lines: &[&str]) -> Vec<Node> {
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    spans.dedup();
    fn build(spans: &[(usize, usize)], k: &mut usize, end: usize, lines: &[&str]) -> Vec<Node> {
        let mut out = Vec::new();
        while *k < spans.len() && spans[*k].0 <= end {
            let (s, e) = spans[*k];
            *k += 1;
            let children = build(spans, k, e, lines);
            out.push(Node {
                label: label(lines, s, e),
                start: s,
                end: e,
                children,
            });
        }
        out
    }
    let mut k = 0;
    build(&spans, &mut k, usize::MAX, lines)
}

/// The first line of `start..=end` that is neither a decorator nor an
/// attribute, trimmed. Falls back to the first line.
fn label(lines: &[&str], start: usize, end: usize) -> String {
    let from = start.saturating_sub(1).min(lines.len());
    let to = (from + (end + 1).saturating_sub(start)).min(lines.len());
    let lines = &lines[from..to];
    let pick = lines
        .iter()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('@') && !l.starts_with("#["))
        .or_else(|| lines.first().map(|l| l.trim()))
        .unwrap_or("");
    let mut label: String = pick.chars().take(120).collect();
    if pick.chars().count() > 120 {
        label.push('…');
    }
    label
}

/// Spans as `(start, end, [(child start, child end)])`, for the outliners' tests.
#[cfg(test)]
pub(crate) type Shape = Vec<(usize, usize, Vec<(usize, usize)>)>;

#[cfg(test)]
pub(crate) fn shape(spans: Vec<Span>) -> Shape {
    spans
        .into_iter()
        .map(|s| {
            let kids = s.children.iter().map(|c| (c.start, c.end)).collect();
            (s.start, s.end, kids)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(nodes: &[Node]) -> Vec<(usize, usize, String)> {
        nodes
            .iter()
            .map(|n| (n.start, n.end, n.label.clone()))
            .collect()
    }

    #[test]
    fn crlf_outlines_as_lf() {
        let lf = "class A:\n\n    def f(self):\n        pass\n\n\ndef g():\n    pass\n";
        let crlf = lf.replace('\n', "\r\n");
        for lang in [Lang::Python, Lang::Ruby, Lang::Rust, Lang::TypeScript] {
            assert_eq!(outline(lang, &crlf), outline(lang, lf), "{lang:?}");
        }
        let nodes = outline(Lang::Python, &crlf);
        assert_eq!(
            lines(&nodes),
            vec![(1, 4, "class A:".into()), (7, 8, "def g():".into())]
        );
        assert_eq!(
            lines(&nodes[0].children),
            vec![(3, 4, "def f(self):".into())]
        );
    }

    #[test]
    fn ruby_spans_nest_by_containment() {
        let text = "module M\n  class C\n    def f\n    end\n  end\nend\n";
        let nodes = outline(Lang::Ruby, text);
        assert_eq!(lines(&nodes), vec![(1, 6, "module M".into())]);
        assert_eq!(lines(&nodes[0].children), vec![(2, 5, "class C".into())]);
        assert_eq!(
            lines(&nodes[0].children[0].children),
            vec![(3, 4, "def f".into())]
        );
    }
}
