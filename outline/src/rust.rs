//! Rust: items split at brace depth 0.
//!
//! An item is the run of depth-0 tokens up to a `;`, or up to the `}` that
//! closes a depth-0 `{` when no `;` follows it, so `use a::{b};` stays one
//! item. Members of impl, trait and inline mod blocks split the same way one
//! level down.

use crate::Span;
use crate::pychar::{eq, find, is_alnum, is_alpha, newlines, starts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Open,
    Close,
    Semi,
    Word,
    /// Punctuation, or a literal (a string, char or lifetime) when `ch` is `None`.
    Other,
}

#[derive(Debug, Clone, Copy)]
struct Tok {
    line: usize,
    kind: Kind,
    /// The bracket or punctuation character.
    ch: Option<char>,
    /// A word's range in the text.
    start: usize,
    end: usize,
}

impl Tok {
    fn punct(line: usize, kind: Kind, c: char) -> Tok {
        Tok {
            line,
            kind,
            ch: Some(c),
            start: 0,
            end: 0,
        }
    }

    fn literal(line: usize) -> Tok {
        Tok {
            line,
            kind: Kind::Other,
            ch: None,
            start: 0,
            end: 0,
        }
    }
}

pub(crate) fn outline(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let toks = lex(&chars);
    let mut top = Vec::new();
    for (start, end, a, b) in split(&toks, 0, toks.len()) {
        let mut children = Vec::new();
        // The first depth-0 `{` after an impl, trait or mod keyword opens the member list.
        let (mut container, mut plain) = (false, false);
        let mut depth: i64 = 0;
        for (k, tok) in toks.iter().enumerate().take(b + 1).skip(a) {
            match tok.kind {
                Kind::Open if tok.ch == Some('{') && depth == 0 => {
                    if container && !plain {
                        children = split(&toks, k + 1, b)
                            .into_iter()
                            .map(|(s, e, _, _)| Span {
                                start: s,
                                end: e,
                                children: Vec::new(),
                            })
                            .collect();
                    }
                    break;
                }
                Kind::Open => depth += 1,
                Kind::Close => depth -= 1,
                Kind::Word if depth == 0 => {
                    let word = &chars[tok.start..tok.end];
                    container |= ["impl", "trait", "mod"].iter().any(|w| eq(word, w));
                    plain |= ["fn", "struct", "enum", "union"]
                        .iter()
                        .any(|w| eq(word, w));
                }
                _ => {}
            }
        }
        top.push(Span {
            start,
            end,
            children,
        });
    }
    top
}

/// Items among `toks[lo..hi]`, all at relative depth 0, as
/// `(start line, end line, first token, last token)`.
fn split(toks: &[Tok], lo: usize, hi: usize) -> Vec<(usize, usize, usize, usize)> {
    let mut items = Vec::new();
    let mut depth: i64 = 0;
    let mut start: Option<usize> = None;
    let mut k = lo;
    while k < hi {
        let tok = toks[k];
        // Inner attributes `#![...]` and a stray `;` belong to no item.
        if depth == 0
            && start.is_none()
            && tok.ch == Some('#')
            && k + 1 < hi
            && toks[k + 1].ch == Some('!')
        {
            k += 2;
            let mut d: i64 = 0;
            while k < hi {
                match toks[k].kind {
                    Kind::Open => d += 1,
                    Kind::Close => d -= 1,
                    _ => {}
                }
                k += 1;
                if d == 0 {
                    break;
                }
            }
            continue;
        }
        if depth == 0 && start.is_none() {
            if tok.kind == Kind::Semi {
                k += 1;
                continue;
            }
            start = Some(k);
        }
        match tok.kind {
            Kind::Open => depth += 1,
            Kind::Close => {
                depth -= 1;
                // A brace ends the item unless the item goes on: `use a::{b};`, `= S {..};`.
                let goes_on = k + 1 < hi && toks[k + 1].kind == Kind::Semi;
                if depth == 0
                    && tok.ch == Some('}')
                    && !goes_on
                    && let Some(s) = start.take()
                {
                    items.push((toks[s].line, tok.line, s, k));
                }
            }
            Kind::Semi if depth == 0 => {
                if let Some(s) = start.take() {
                    items.push((toks[s].line, tok.line, s, k));
                }
            }
            _ => {}
        }
        k += 1;
    }
    items
}

/// Code tokens: brackets, `;`, words and other punctuation. Comments vanish,
/// and each string, char or lifetime becomes one literal token.
fn lex(t: &[char]) -> Vec<Tok> {
    let n = t.len();
    let (mut i, mut line) = (0, 1);
    let mut out = Vec::new();
    while i < n {
        let mut c = t[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if matches!(c, ' ' | '\t' | '\r') {
            i += 1;
            continue;
        }
        if starts(t, i, "//") {
            while i < n && t[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if starts(t, i, "/*") {
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if starts(t, i, "/*") {
                    depth += 1;
                    i += 2;
                } else if starts(t, i, "*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    line += usize::from(t[i] == '\n');
                    i += 1;
                }
            }
            continue;
        }
        // Raw strings: r"..", r#".."#, br"..", cr"..".
        let mut j = i;
        if matches!(t[j], 'b' | 'c') && j + 1 < n && t[j + 1] == 'r' {
            j += 1;
        }
        if t[j] == 'r' && j + 1 < n && matches!(t[j + 1], '#' | '"') {
            let mut k = j + 1;
            let mut hashes = 0;
            while k < n && t[k] == '#' {
                hashes += 1;
                k += 1;
            }
            if k < n && t[k] == '"' {
                let close = format!("\"{}", "#".repeat(hashes));
                let end = find(t, &close, k + 1).map_or(n, |e| e + 1 + hashes);
                out.push(Tok::literal(line));
                line += newlines(t, i, end);
                i = end;
                continue;
            }
        }
        if matches!(c, 'b' | 'c') && i + 1 < n && t[i + 1] == '"' {
            i += 1;
            c = '"';
        }
        if c == '"' {
            out.push(Tok::literal(line));
            i += 1;
            while i < n && t[i] != '"' {
                if t[i] == '\\' {
                    i += 1;
                }
                if i < n && t[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == 'b' && i + 1 < n && t[i + 1] == '\'' {
            i += 1;
            c = '\'';
        }
        if c == '\'' {
            // A char literal ('x', '\n', '\u{..}') or a lifetime ('a, 'static).
            if i + 1 < n && t[i + 1] == '\\' {
                i = find(t, "'", i + 2).map_or(n, |e| e + 1);
                out.push(Tok::literal(line));
                continue;
            }
            if i + 2 < n && t[i + 2] == '\'' {
                i += 3;
                out.push(Tok::literal(line));
                continue;
            }
            i += 1;
            while i < n && (is_alnum(t[i]) || t[i] == '_') {
                i += 1;
            }
            out.push(Tok::literal(line));
            continue;
        }
        if is_alpha(c) || c == '_' {
            let mut j = i;
            while j < n && (is_alnum(t[j]) || t[j] == '_') {
                j += 1;
            }
            out.push(Tok {
                line,
                kind: Kind::Word,
                ch: None,
                start: i,
                end: j,
            });
            i = j;
            continue;
        }
        let kind = match c {
            '{' | '(' | '[' => Kind::Open,
            '}' | ')' | ']' => Kind::Close,
            ';' => Kind::Semi,
            _ => Kind::Other,
        };
        out.push(Tok::punct(line, kind, c));
        i += 1;
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
    fn impl_members_split_one_level_down() {
        let text = "use a::{b, c};\n\n#[derive(Debug)]\nstruct S {\n    x: u8,\n}\n\nimpl S {\n    const N: u8 = 1;\n\n    /// Doc.\n    fn f(&self) -> u8 {\n        if true { 1 } else { 2 }\n    }\n}\n";
        assert_eq!(
            spans(text),
            vec![
                (1, 1, vec![]),
                (3, 6, vec![]),
                (8, 15, vec![(9, 9), (12, 14)]),
            ]
        );
    }

    #[test]
    fn nested_mod_keeps_members_one_level() {
        let text = "mod m {\n    impl T for U {\n        fn g() {}\n    }\n    fn h() {}\n}\n";
        assert_eq!(spans(text), vec![(1, 6, vec![(2, 4), (5, 5)])]);
    }

    #[test]
    fn strings_chars_and_comments_hide_braces() {
        let text = "#![allow(x)]\nfn f() {\n    let s = r#\"}\"#; // }\n    let c = '}';\n    /* { /* } */ */\n}\nfn g<'a>(x: &'a str) {}\n";
        assert_eq!(spans(text), vec![(2, 6, vec![]), (7, 7, vec![])]);
    }
}
