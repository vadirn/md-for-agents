//! TypeScript and JavaScript: items split at bracket depth 0.
//!
//! A small lexer skips comments, strings, templates and regexes, so every
//! `{ ( [` it counts is code. It records, per line, the bracket depth at the
//! line's first code token, that token, and the last one. A line at depth 0
//! starts an item when it opens with a declaration word, or when the line
//! before it ended a statement. Members of a class, interface, enum,
//! namespace or module split the same way at depth 1, inside the container's
//! longest depth-0 brace pair.

use crate::Span;
use crate::pychar::{eq, find, is_alnum, is_alpha, is_word, newlines, starts};

/// A code token: one character, or a word by its range in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    Char(char),
    Word(usize, usize),
}

/// The last significant code token, which decides whether `/` opens a regex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prev {
    Nothing,
    Tok(Tok),
    Regex,
}

#[derive(Debug, Clone, Copy)]
struct LineInfo {
    line: usize,
    first: Tok,
    depth: usize,
    last: Tok,
    last_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Bracket(char),
    /// A template's `${`, which counts toward no depth.
    Interp,
}

struct Lexed {
    /// One entry per line that carries code, in line order.
    lines: Vec<LineInfo>,
    /// Every `{...}` pair as (open line, close line, depth of the open), in close order.
    braces: Vec<(usize, usize, usize)>,
}

const DECL: [&str; 25] = [
    "export",
    "import",
    "const",
    "let",
    "var",
    "function",
    "class",
    "interface",
    "type",
    "enum",
    "declare",
    "namespace",
    "module",
    "abstract",
    "async",
    "default",
    "public",
    "private",
    "protected",
    "readonly",
    "static",
    "get",
    "set",
    "constructor",
    "override",
];
const CONTAINER: [&str; 5] = ["class", "interface", "enum", "namespace", "module"];
const REGEX_AFTER_WORD: [&str; 14] = [
    "return",
    "typeof",
    "case",
    "do",
    "else",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "instanceof",
    "yield",
    "await",
];

pub(crate) fn outline(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let lexed = lex(&chars);
    let mut top = items(&chars, &lexed.lines, 0, 1, usize::MAX, false);
    let src: Vec<&str> = text.split('\n').collect();
    // Depth-0 brace pairs by open line; the index keeps close order for ties.
    let mut bodies: Vec<(usize, usize, usize)> = lexed
        .braces
        .iter()
        .enumerate()
        .filter(|(_, b)| b.2 == 0)
        .map(|(k, b)| (b.0, b.1, k))
        .collect();
    bodies.sort_by_key(|b| b.0);
    for it in &mut top {
        // The declaration line, past any decorators, names the container.
        let head: Vec<char> = crate::label(&src, it.start, it.end).chars().collect();
        if !CONTAINER.iter().any(|w| has_word(&head, w)) {
            continue;
        }
        // The longest brace pair opened within the item; the first closed wins a tie.
        let from = bodies.partition_point(|b| b.0 < it.start);
        let mut best: Option<(usize, usize, usize)> = None;
        for &b in bodies[from..].iter().take_while(|b| b.0 <= it.end) {
            let longer = match best {
                None => true,
                Some(o) => b.1 - b.0 > o.1 - o.0 || (b.1 - b.0 == o.1 - o.0 && b.2 < o.2),
            };
            if longer {
                best = Some(b);
            }
        }
        let Some((open, close, _)) = best else {
            continue;
        };
        let is_enum = has_word(&head, "enum");
        it.children = items(&chars, &lexed.lines, 1, open + 1, close - 1, is_enum);
    }
    top
}

/// `\bword\b` occurs in `line`.
fn has_word(line: &[char], word: &str) -> bool {
    let len = word.chars().count();
    (0..line.len()).any(|p| {
        (p == 0 || !is_word(line[p - 1]))
            && starts(line, p, word)
            && (p + len == line.len() || !is_word(line[p + len]))
    })
}

/// Split the code lines `lo..=hi` into items at bracket `depth`.
fn items(
    t: &[char],
    lines: &[LineInfo],
    depth: usize,
    lo: usize,
    hi: usize,
    is_enum: bool,
) -> Vec<Span> {
    let mut out: Vec<(Span, bool)> = Vec::new();
    let mut prev_last: Option<Tok> = None;
    let from = lines.partition_point(|l| l.line < lo);
    for info in lines[from..].iter().take_while(|l| l.line <= hi) {
        let first = info.first;
        if info.depth == depth && !leads_continuation(t, first) && first != Tok::Char('`') {
            let ended = match prev_last {
                None => true,
                Some(p) => {
                    matches!(p, Tok::Char(';' | '}' | ')'))
                        || (is_enum && p == Tok::Char(','))
                        || (!continues(p) && !is_enum)
                }
            };
            if is_decl(t, first) || ended {
                let decorator = first == Tok::Char('@');
                match out.last_mut() {
                    Some((_, pending)) if *pending => *pending = decorator,
                    _ => out.push((
                        Span {
                            start: info.line,
                            end: info.line,
                            children: Vec::new(),
                        },
                        decorator,
                    )),
                }
            }
        }
        if let Some((last, _)) = out.last_mut() {
            last.end = info.line;
        }
        if info.last_depth >= depth {
            prev_last = Some(info.last);
        }
    }
    out.into_iter().map(|(span, _)| span).collect()
}

fn word(t: &[char], tok: Tok) -> Option<&[char]> {
    match tok {
        Tok::Word(s, e) => Some(&t[s..e]),
        Tok::Char(_) => None,
    }
}

/// A line's first token that continues the statement above it.
fn leads_continuation(t: &[char], tok: Tok) -> bool {
    match tok {
        Tok::Char(c) => "{})].,?:|&+-*/=>".contains(c),
        Tok::Word(..) => word(t, tok).is_some_and(|w| eq(w, "extends") || eq(w, "implements")),
    }
}

/// A line's last token that leaves the statement open.
fn continues(tok: Tok) -> bool {
    matches!(tok, Tok::Char(c) if "=,([{+-*/%&|^!~?:<>.".contains(c))
}

/// A line's first token is a declaration word or a decorator. A word token
/// can hold `$`, which ends a regex word, so `export$x` counts as `export`.
fn is_decl(t: &[char], tok: Tok) -> bool {
    let Some(w) = word(t, tok) else {
        return tok == Tok::Char('@');
    };
    DECL.iter().any(|kw| {
        let len = kw.len();
        starts(w, 0, kw) && (w.len() == len || w[len] == '$')
    })
}

fn regex_may_follow(t: &[char], prev: Prev) -> bool {
    match prev {
        Prev::Nothing => true,
        Prev::Regex => false,
        Prev::Tok(Tok::Char(c)) => "(,=:[!&|?{};+-*%<>~^".contains(c),
        Prev::Tok(tok @ Tok::Word(..)) => {
            word(t, tok).is_some_and(|w| REGEX_AFTER_WORD.iter().any(|k| eq(w, k)))
        }
    }
}

fn note(lines: &mut Vec<LineInfo>, line: usize, tok: Tok, depth: usize) {
    match lines.last_mut() {
        Some(info) if info.line == line => {
            info.last = tok;
            info.last_depth = depth;
        }
        _ => lines.push(LineInfo {
            line,
            first: tok,
            depth,
            last: tok,
            last_depth: depth,
        }),
    }
}

fn lex(t: &[char]) -> Lexed {
    let n = t.len();
    let (mut i, mut line) = (0, 1);
    let mut stack: Vec<Frame> = Vec::new();
    let mut depth = 0; // brackets on the stack, not counting `${`
    let mut lines = Vec::new();
    let mut braces = Vec::new();
    let mut open_lines: Vec<(usize, usize)> = Vec::new();
    let mut prev = Prev::Nothing;
    let mut in_template = false;
    while i < n {
        let c = t[i];
        if in_template {
            if c == '\\' {
                // An escaped line break still ends a line.
                line += usize::from(i + 1 < n && t[i + 1] == '\n');
                i += 2;
                continue;
            }
            if c == '\n' {
                line += 1;
            } else if c == '`' {
                in_template = false;
                prev = Prev::Tok(Tok::Char('`'));
                note(&mut lines, line, Tok::Char('`'), depth);
            } else if c == '$' && i + 1 < n && t[i + 1] == '{' {
                in_template = false;
                stack.push(Frame::Interp);
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
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
            let j = find(t, "*/", i + 2).map_or(n, |j| j + 2);
            line += newlines(t, i, j);
            i = j;
            continue;
        }
        if c == '\'' || c == '"' {
            note(&mut lines, line, Tok::Char(c), depth);
            i += 1;
            while i < n && t[i] != c && t[i] != '\n' {
                if t[i] == '\\' {
                    line += usize::from(i + 1 < n && t[i + 1] == '\n');
                    i += 2;
                } else {
                    i += 1;
                }
            }
            i += 1;
            prev = Prev::Tok(Tok::Char(c));
            continue;
        }
        if c == '`' {
            note(&mut lines, line, Tok::Char(c), depth);
            in_template = true;
            i += 1;
            continue;
        }
        if c == '/' && regex_may_follow(t, prev) {
            note(&mut lines, line, Tok::Char(c), depth);
            i += 1;
            let mut in_class = false;
            while i < n && t[i] != '\n' {
                match t[i] {
                    '\\' => {
                        i += 2;
                        continue;
                    }
                    '[' => in_class = true,
                    ']' => in_class = false,
                    '/' if !in_class => break,
                    _ => {}
                }
                i += 1;
            }
            i += 1;
            while i < n && is_alpha(t[i]) {
                i += 1;
            }
            prev = Prev::Regex;
            continue;
        }
        if is_alnum(c) || c == '_' || c == '$' {
            let mut j = i;
            while j < n && (is_alnum(t[j]) || t[j] == '_' || t[j] == '$') {
                j += 1;
            }
            let tok = Tok::Word(i, j);
            note(&mut lines, line, tok, depth);
            prev = Prev::Tok(tok);
            i = j;
            continue;
        }
        if matches!(c, '{' | '(' | '[') {
            note(&mut lines, line, Tok::Char(c), depth);
            stack.push(Frame::Bracket(c));
            depth += 1;
            if c == '{' {
                open_lines.push((line, depth - 1));
            }
            prev = Prev::Tok(Tok::Char(c));
            i += 1;
            continue;
        }
        if matches!(c, '}' | ')' | ']') {
            if c == '}' && stack.last() == Some(&Frame::Interp) {
                stack.pop();
                in_template = true;
                i += 1;
                continue;
            }
            // The top frame pops whatever closes it; only a `{` pairs a brace.
            if let Some(Frame::Bracket(open)) = stack.pop() {
                depth -= 1;
                if open == '{'
                    && let Some((open_line, d)) = open_lines.pop()
                {
                    braces.push((open_line, line, d));
                }
            }
            note(&mut lines, line, Tok::Char(c), depth);
            prev = Prev::Tok(Tok::Char(c));
            i += 1;
            continue;
        }
        note(&mut lines, line, Tok::Char(c), depth);
        prev = Prev::Tok(Tok::Char(c));
        i += 1;
    }
    Lexed { lines, braces }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> crate::Shape {
        crate::shape(outline(text))
    }

    #[test]
    fn class_members_split_at_depth_one() {
        let text = "import { a } from \"a\";\n\nexport class Foo extends Bar {\n  private x = 1;\n\n  @Input()\n  get y(): number {\n    return this.x;\n  }\n\n  constructor() {\n    super();\n  }\n}\n";
        assert_eq!(
            spans(text),
            vec![(1, 1, vec![]), (3, 14, vec![(4, 4), (6, 9), (11, 13)]),]
        );
    }

    #[test]
    fn continued_statement_stays_one_item() {
        let text = "const x = a\n  + b;\nconst re = /}/g;\nconst t = `${ {a: 1}.a }\n}`;\nfunction f() {}\n";
        assert_eq!(
            spans(text),
            vec![
                (1, 2, vec![]),
                (3, 3, vec![]),
                (4, 5, vec![]),
                (6, 6, vec![])
            ]
        );
    }

    #[test]
    fn enum_members_split_at_commas() {
        let text = "enum E {\n  A = 1,\n  B,\n}\n";
        assert_eq!(spans(text), vec![(1, 4, vec![(2, 2), (3, 3)])]);
    }
}
