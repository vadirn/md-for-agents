//! Python: items read from indentation.
//!
//! Indentation delimits a block only on a logical line's first physical line.
//! Inside brackets, after a backslash, and inside a triple-quoted string it
//! means nothing, so a small lexer still tracks brackets and strings; it reads
//! structure from columns instead of from `{}`. A class's members split the
//! same way at the column of its first indented line.

use crate::Span;
use crate::pychar::{eq, is_space, is_word};

/// A logical line's first physical line.
#[derive(Debug, Clone, Copy)]
struct Start {
    line: usize,
    /// The column of its first code character, in characters.
    col: usize,
    /// Its first word is `elif`, `else`, `except` or `finally`.
    continues: bool,
    /// It starts with `@`.
    decorator: bool,
}

pub(crate) fn outline(text: &str) -> Vec<Span> {
    let src: Vec<&str> = text.split('\n').collect();
    let (starts, code) = logical_lines(&src);
    let mut top = items(&starts, &code, 0, 1, src.len());
    for it in &mut top {
        let head = src[it.start - 1];
        if !(head.starts_with('@') || starts_word(head, "class")) {
            continue;
        }
        // The first indented logical line inside the item opens the class body.
        let from = starts.partition_point(|s| s.line <= it.start);
        let Some(body) = starts[from..]
            .iter()
            .take_while(|s| s.line <= it.end)
            .find(|s| s.col > 0)
        else {
            continue;
        };
        let is_class = src[it.start - 1..body.line - 1]
            .iter()
            .any(|l| starts_word(l.trim_start_matches(is_space), "class"));
        if is_class {
            it.children = items(&starts, &code, body.col, body.line, it.end);
        }
    }
    top
}

/// `line` starts with `word` followed by a word boundary.
fn starts_word(line: &str, word: &str) -> bool {
    line.strip_prefix(word)
        .is_some_and(|rest| !rest.chars().next().is_some_and(is_word))
}

/// Each logical line's first physical line, and every line that carries code
/// (which the item ends come from), both in line order.
fn logical_lines(src: &[&str]) -> (Vec<Start>, Vec<usize>) {
    let mut depth: usize = 0;
    let mut in_string: Option<char> = None; // the quote of an open triple-quoted string
    let mut continued = false;
    let mut starts = Vec::new();
    let mut code = Vec::new();
    let mut raw: Vec<char> = Vec::new();
    for (idx, line) in src.iter().enumerate() {
        let no = idx + 1;
        raw.clear();
        raw.extend(line.chars());
        let n = raw.len();
        let mut i = 0;
        let begins_logical = depth == 0 && in_string.is_none() && !continued;
        continued = false;
        let mut has_code = in_string.is_some();
        let mut first: Option<Start> = None;
        while i < n {
            let c = raw[i];
            if let Some(q) = in_string {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if raw[i..].starts_with(&[q, q, q]) {
                    i += 3;
                    in_string = None;
                    continue;
                }
                i += 1;
                continue;
            }
            if c == ' ' || c == '\t' {
                i += 1;
                continue;
            }
            if c == '#' {
                break;
            }
            has_code = true;
            if first.is_none() {
                // `[A-Za-z_]\w*`, else the one character.
                let mut j = i + 1;
                if c.is_ascii_alphabetic() || c == '_' {
                    while j < n && is_word(raw[j]) {
                        j += 1;
                    }
                }
                let word = &raw[i..j];
                first = Some(Start {
                    line: no,
                    col: i,
                    continues: ["elif", "else", "except", "finally"]
                        .iter()
                        .any(|w| eq(word, w)),
                    decorator: c == '@',
                });
            }
            if c == '\\' && i == n - 1 {
                continued = true;
                break;
            }
            if c == '\'' || c == '"' {
                if raw[i..].starts_with(&[c, c, c]) {
                    in_string = Some(c);
                    i += 3;
                    continue;
                }
                let mut j = i + 1;
                while j < n && raw[j] != c {
                    j += if raw[j] == '\\' { 2 } else { 1 };
                }
                i = j + 1;
                continue;
            }
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
            i += 1;
        }
        if has_code {
            code.push(no);
        }
        if begins_logical && let Some(start) = first {
            starts.push(start);
        }
    }
    (starts, code)
}

/// Items among the logical lines `lo..=hi` at column `indent`. A decorator
/// joins the item it decorates; each item ends at its last code line before
/// the next item.
fn items(starts: &[Start], code: &[usize], indent: usize, lo: usize, hi: usize) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut decorator = false;
    let from = starts.partition_point(|s| s.line < lo);
    for s in starts[from..].iter().take_while(|s| s.line <= hi) {
        if s.col != indent || s.continues {
            continue;
        }
        if !decorator {
            out.push(Span {
                start: s.line,
                end: s.line,
                children: Vec::new(),
            });
        }
        decorator = s.decorator;
    }
    let nexts: Vec<usize> = out
        .iter()
        .skip(1)
        .map(|it| it.start)
        .chain([hi + 1])
        .collect();
    for (it, next) in out.iter_mut().zip(nexts) {
        // The start line carries code, so the last code line before `next` is at or after it.
        let k = code.partition_point(|&l| l < next);
        it.end = if k > 0 {
            code[k - 1].max(it.start)
        } else {
            it.start
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> crate::Shape {
        crate::shape(outline(text))
    }

    #[test]
    fn decorated_method_joins_its_decorator() {
        let text = "import os\n\n\n@dataclass\nclass A(\n    Base,\n):\n    \"\"\"Doc.\n\ndef not_a_def(): pass\n\"\"\"\n\n    x = 1\n\n    @property\n    def y(self):\n        return [\n1]\n\n\ndef f():\n    pass\n# trailing comment\n";
        assert_eq!(
            spans(text),
            vec![
                (1, 1, vec![]),
                (4, 18, vec![(8, 11), (13, 13), (15, 18)]),
                (21, 22, vec![]),
            ]
        );
    }

    #[test]
    fn else_and_except_continue_the_item() {
        let text = "if x:\n    a()\nelse:\n    b()\ntry:\n    c()\nexcept E:\n    d()\n";
        assert_eq!(spans(text), vec![(1, 4, vec![]), (5, 8, vec![])]);
    }

    #[test]
    fn backslash_continues_the_logical_line() {
        let text = "x = 1 + \\\n2\ny = 3\n";
        assert_eq!(spans(text), vec![(1, 2, vec![]), (3, 3, vec![])]);
    }
}
