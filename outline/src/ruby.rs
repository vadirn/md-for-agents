//! Ruby: blocks read by pairing each opening keyword with its `end`.
//!
//! The lexer skips what can hide a keyword: comments, `=begin`/`=end`,
//! strings with `#{}` interpolation, heredoc bodies, percent literals,
//! regexes, symbols, and labels such as `end:`. A keyword after `.` or `::`
//! is a method call, and `if`, `unless`, `while` and `until` open a block only
//! where a statement starts, so a modifier `x if y` opens nothing.

use crate::pychar::{eq, is_alnum, is_alpha, is_digit, is_upper, is_word, newlines, starts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Const,
    Method,
    Number,
    Str,
    Symbol,
    Ivar,
    Open,
    Close,
    Op,
    /// A line break or a `;`.
    Nl,
}

/// The keywords the outliner and the lexer's value test look for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kw {
    None,
    Def,
    Class,
    Module,
    If,
    Unless,
    While,
    Until,
    Case,
    Begin,
    For,
    Do,
    End,
    Then,
    Else,
    Elsif,
    And,
    Or,
    Not,
    Ensure,
    In,
    When,
}

impl Kw {
    fn of(word: &[char]) -> Kw {
        const ALL: [(&str, Kw); 21] = [
            ("def", Kw::Def),
            ("class", Kw::Class),
            ("module", Kw::Module),
            ("if", Kw::If),
            ("unless", Kw::Unless),
            ("while", Kw::While),
            ("until", Kw::Until),
            ("case", Kw::Case),
            ("begin", Kw::Begin),
            ("for", Kw::For),
            ("do", Kw::Do),
            ("end", Kw::End),
            ("then", Kw::Then),
            ("else", Kw::Else),
            ("elsif", Kw::Elsif),
            ("and", Kw::And),
            ("or", Kw::Or),
            ("not", Kw::Not),
            ("ensure", Kw::Ensure),
            ("in", Kw::In),
            ("when", Kw::When),
        ];
        ALL.iter()
            .find(|(s, _)| eq(word, s))
            .map_or(Kw::None, |&(_, kw)| kw)
    }

    /// A keyword that opens a block, as `end` expects.
    fn opens(self) -> bool {
        use Kw::*;
        matches!(
            self,
            Def | Class | Module | If | Unless | While | Until | Case | Begin | For | Do
        )
    }

    /// After this word a statement starts.
    fn starts_statement(self) -> bool {
        use Kw::*;
        self.opens()
            || matches!(
                self,
                Then | Else | Elsif | Do | And | Or | Not | Begin | Ensure | In | When
            )
    }
}

#[derive(Debug, Clone, Copy)]
struct Tok {
    line: usize,
    kind: Kind,
    /// The token's text is `t[start..end]`; a heredoc's opener has none.
    start: usize,
    end: usize,
    /// Whitespace comes before it.
    spaced: bool,
    /// The keyword a word spells, for words and method names.
    kw: Kw,
}

impl Tok {
    fn text<'a>(&self, t: &'a [char]) -> &'a [char] {
        &t[self.start..self.end]
    }

    fn is(&self, t: &[char], s: &str) -> bool {
        eq(self.text(t), s)
    }
}

/// The token ends a value, so an operator after it is binary.
fn is_value(tok: Option<&Tok>) -> bool {
    let Some(tok) = tok else {
        return false;
    };
    use Kind::*;
    matches!(
        tok.kind,
        Word | Const | Number | Str | Close | Symbol | Ivar | Method
    ) && !(tok.kind == Word && tok.kw.starts_statement())
}

/// Every def, class, module and singleton-class block as an inclusive line span.
pub(crate) fn outline(text: &str) -> Vec<(usize, usize)> {
    let t: Vec<char> = text.chars().collect();
    let toks = lex(&t);
    let n = toks.len();
    let one = |tok: &Tok, set: &str| tok.end == tok.start + 1 && set.contains(t[tok.start]);
    let mut stack: Vec<(Kw, usize)> = Vec::new();
    let mut spans = Vec::new();
    let mut loop_header = false;
    let mut k = 0;
    while k < n {
        let tok = toks[k];
        if tok.kind == Kind::Nl {
            loop_header = false;
            k += 1;
            continue;
        }
        if tok.kind != Kind::Word {
            k += 1;
            continue;
        }
        let line = tok.line;
        if tok.kw == Kw::End {
            if let Some((kw, start)) = stack.pop()
                && matches!(kw, Kw::Def | Kw::Class | Kw::Module)
            {
                spans.push((start, line));
            }
            k += 1;
            continue;
        }
        if !tok.kw.opens() {
            k += 1;
            continue;
        }
        let prev = if k > 0 { Some(&toks[k - 1]) } else { None };
        if matches!(tok.kw, Kw::If | Kw::Unless | Kw::While | Kw::Until) {
            let starts_statement = match prev {
                None => true,
                Some(p) => {
                    matches!(p.kind, Kind::Nl | Kind::Open | Kind::Op)
                        || (p.kind == Kind::Word && p.kw.starts_statement())
                }
            };
            if !starts_statement {
                k += 1;
                continue;
            }
        }
        if tok.kw == Kw::Do && loop_header {
            loop_header = false;
            k += 1;
            continue;
        }
        if tok.kw == Kw::Def {
            // Skip the name (with receiver) and parameters; an `=` next means an endless def.
            let mut j = k + 1;
            if j < n
                && matches!(
                    toks[j].kind,
                    Kind::Word | Kind::Const | Kind::Ivar | Kind::Method
                )
            {
                j += 1;
                while j + 1 < n
                    && toks[j].kind == Kind::Op
                    && (toks[j].is(&t, ".") || toks[j].is(&t, "::"))
                {
                    j += 2;
                }
            } else if j < n && toks[j].kind == Kind::Op {
                j += 1;
                while j < n
                    && matches!(toks[j].kind, Kind::Op | Kind::Close)
                    && one(&toks[j], "=~@]<>!*+-")
                {
                    if toks[j].is(&t, "=") && j + 1 < n && toks[j + 1].kind == Kind::Open {
                        break;
                    }
                    j += 1;
                }
                if j < n && toks[j].kind == Kind::Open && toks[j].is(&t, "[") {
                    j += 2;
                }
            }
            // A setter name `foo=` has no space before its `=`.
            if j < n && toks[j].kind == Kind::Op && toks[j].is(&t, "=") && !toks[j].spaced {
                j += 1;
            }
            let parens =
                j < n && toks[j].kind == Kind::Open && toks[j].is(&t, "(") && !toks[j].spaced;
            if parens {
                let mut depth: i64 = 0;
                while j < n {
                    match toks[j].kind {
                        Kind::Open => depth += 1,
                        Kind::Close => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            if j < n && toks[j].kind == Kind::Op && toks[j].is(&t, "=") {
                // Endless: the body runs to the end of the logical line.
                let mut depth: i64 = 0;
                let mut e = j + 1;
                while e < n && !(toks[e].kind == Kind::Nl && depth == 0) {
                    match toks[e].kind {
                        Kind::Open => depth += 1,
                        Kind::Close => depth -= 1,
                        _ => {}
                    }
                    e += 1;
                }
                spans.push((line, toks[e - 1].line));
                k = e;
                continue;
            }
            // Skip the name and any unparenthesized parameters, so `def for` opens nothing more.
            while !parens && j < n && toks[j].kind != Kind::Nl {
                j += 1;
            }
            stack.push((Kw::Def, line));
            k = j;
            continue;
        }
        if tok.kw == Kw::Class
            && k + 1 < n
            && toks[k + 1].kind == Kind::Op
            && toks[k + 1].is(&t, "<<")
        {
            stack.push((Kw::Class, line));
            k += 2;
            continue;
        }
        if matches!(tok.kw, Kw::While | Kw::Until | Kw::For) {
            loop_header = true;
        }
        stack.push((tok.kw, line));
        k += 1;
    }
    spans
}

/// The closing delimiter of a literal that opens with `open`.
fn closer(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        c => c,
    }
}

/// Skip a delimited literal from just after its opening delimiter. Returns
/// the index after its closing delimiter and the line breaks it spans.
fn skip_delimited(t: &[char], mut j: usize, open: char, interp: bool) -> (usize, usize) {
    let n = t.len();
    let close = closer(open);
    let (mut depth, mut lines) = (1, 0);
    while j < n {
        let ch = t[j];
        if ch == '\\' {
            j += 2;
            continue;
        }
        if ch == '\n' {
            lines += 1;
        }
        if interp && ch == '#' && j + 1 < n && t[j + 1] == '{' {
            let (next, extra) = skip_interp(t, j + 2);
            j = next;
            lines += extra;
            continue;
        }
        if open != close && ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return (j + 1, lines);
            }
        }
        j += 1;
    }
    (j, lines)
}

/// Skip a `#{...}` body from just after its `{`.
fn skip_interp(t: &[char], mut j: usize) -> (usize, usize) {
    let n = t.len();
    let (mut depth, mut lines) = (1, 0);
    while j < n {
        let ch = t[j];
        if ch == '\n' {
            lines += 1;
        }
        if ch == '"' || ch == '\'' {
            let (next, extra) = skip_delimited(t, j + 1, ch, ch == '"');
            j = next;
            lines += extra;
            continue;
        }
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return (j + 1, lines);
            }
        }
        j += 1;
    }
    (j, lines)
}

/// The end of the first line at or after `from` that `hit` accepts: the
/// index of its `\n`, or of the text's end. A line starts at 0 or after a `\n`.
fn find_line(t: &[char], from: usize, hit: impl Fn(&[char]) -> bool) -> Option<usize> {
    let n = t.len();
    let mut p = from;
    if p > 0 && p <= n && t[p - 1] != '\n' {
        p = (p..n).find(|&k| t[k] == '\n')? + 1;
    }
    while p <= n {
        let end = (p..n).find(|&k| t[k] == '\n').unwrap_or(n);
        if hit(&t[p..end]) {
            return Some(end);
        }
        if end >= n {
            return None;
        }
        p = end + 1;
    }
    None
}

/// A heredoc opener `<<ID`, `<<~ID`, `<<-"ID"` at `i`: whether it has a `~`
/// or `-`, whether it quotes its terminator, the terminator's range, and the
/// index after the opener.
fn heredoc(t: &[char], i: usize) -> Option<(bool, bool, usize, usize, usize)> {
    let n = t.len();
    if !starts(t, i, "<<") {
        return None;
    }
    let mut j = i + 2;
    let indented = j < n && matches!(t[j], '~' | '-');
    if indented {
        j += 1;
    }
    let quote = (j < n && matches!(t[j], '\'' | '"' | '`')).then(|| t[j]);
    if quote.is_some() {
        j += 1;
    }
    if !(j < n && (t[j].is_ascii_alphabetic() || t[j] == '_')) {
        return None;
    }
    let start = j;
    j += 1;
    while j < n && is_word(t[j]) {
        j += 1;
    }
    let end = j;
    if let Some(q) = quote {
        if j < n && t[j] == q {
            j += 1;
        } else {
            return None;
        }
    }
    Some((indented, quote.is_some(), start, end, j))
}

/// Tokens with kinds word, const, method, number, string, symbol, ivar, open,
/// close, op and nl.
fn lex(t: &[char]) -> Vec<Tok> {
    let n = t.len();
    let mut toks: Vec<Tok> = Vec::new();
    let (mut i, mut line) = (0, 1);
    let mut pending: Vec<(usize, usize, bool)> = Vec::new(); // (terminator range, indented)
    let mut at_line_start = true;
    let tok = |line, kind, start, end, spaced| Tok {
        line,
        kind,
        start,
        end,
        spaced,
        kw: Kw::None,
    };
    while i < n {
        let c = t[i];
        if at_line_start {
            at_line_start = false;
            if starts(t, i, "=begin") && (i + 6 == n || matches!(t[i + 6], ' ' | '\t' | '\n')) {
                let is_end = |l: &[char]| starts(l, 0, "=end") && (l.len() == 4 || !is_word(l[4]));
                let stop = find_line(t, i, is_end).unwrap_or(n);
                line += newlines(t, i, stop);
                i = stop;
                continue;
            }
            if starts(t, i, "__END__") && (i + 7 == n || t[i + 7] == '\n') {
                break;
            }
        }
        if c == '\n' {
            toks.push(tok(line, Kind::Nl, i, i + 1, false));
            line += 1;
            i += 1;
            at_line_start = true;
            for (s, e, indented) in pending.drain(..) {
                let term = &t[s..e];
                let stop = find_line(t, i, |l| {
                    if indented {
                        let l = trim_blanks(l);
                        l == term
                    } else {
                        l == term
                    }
                })
                .unwrap_or(n);
                line += newlines(t, i, stop);
                i = stop;
            }
            continue;
        }
        if matches!(c, ' ' | '\t' | '\r') {
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < n && t[i + 1] == '\n' {
            i += 2;
            line += 1;
            continue;
        }
        if c == '#' {
            while i < n && t[i] != '\n' {
                i += 1;
            }
            continue;
        }
        let spaced = i > 0 && matches!(t[i - 1], ' ' | '\t');
        let p = toks.last().copied();
        let p = p.as_ref();
        // After `def` (or `def self.`), an operator is the method's name: `def /(x)`, `def -@`.
        let after_def = p.is_some_and(|p| {
            p.kw == Kw::Def
                || (p.is(t, ".") && toks.len() > 2 && toks[toks.len() - 3].kw == Kw::Def)
        });
        if after_def && "+-*/%<=>!~^&|[`".contains(c) {
            let mut j = i;
            while j < n && "+-*/%<=>!~^&|[]@`".contains(t[j]) {
                j += 1;
            }
            toks.push(tok(line, Kind::Method, i, j, spaced));
            i = j;
            continue;
        }
        if let Some((indented, quoted, s, e, after)) = heredoc(t, i)
            && (indented || quoted || !is_value(p) || (spaced && !matches!(t[i + 2], ' ' | '=')))
            && !p.is_some_and(|p| p.kw == Kw::Class)
        {
            pending.push((s, e, indented));
            toks.push(tok(line, Kind::Str, i, i, spaced));
            i = after;
            continue;
        }
        if matches!(c, '"' | '\'' | '`') {
            let (j, extra) = skip_delimited(t, i + 1, c, c != '\'');
            toks.push(tok(line, Kind::Str, i, i + 1, spaced));
            line += extra;
            i = j;
            // A string followed by `:` is a label ("key": v).
            if i < n && t[i] == ':' && !(i + 1 < n && t[i + 1] == ':') {
                i += 1;
            }
            continue;
        }
        if c == '%' && i + 1 < n && (!is_value(p) || (spaced && !matches!(t[i + 1], ' ' | '='))) {
            let mut j = i + 1;
            let mut kind = None;
            if "qQwWiIrsx".contains(t[j])
                && j + 1 < n
                && !is_alnum(t[j + 1])
                && !matches!(t[j + 1], ' ' | '\n')
            {
                kind = Some(t[j]);
                j += 1;
            }
            if j < n && !is_alnum(t[j]) && !matches!(t[j], ' ' | '\n' | '=') {
                let interp = kind.is_none_or(|k| !"qwis".contains(k));
                let (next, extra) = skip_delimited(t, j + 1, t[j], interp);
                toks.push(tok(line, Kind::Str, i, i + 1, spaced));
                line += extra;
                i = next;
                continue;
            }
        }
        if c == '/' && (!is_value(p) || (spaced && i + 1 < n && !matches!(t[i + 1], ' ' | '='))) {
            let (mut j, extra) = skip_delimited(t, i + 1, '/', true);
            while j < n && is_alpha(t[j]) {
                j += 1;
            }
            toks.push(tok(line, Kind::Str, i, i + 1, spaced));
            line += extra;
            i = j;
            continue;
        }
        if c == '?'
            && i + 1 < n
            && !is_value(p)
            && (i + 2 >= n || !(is_alnum(t[i + 2]) || t[i + 2] == '_'))
            && !matches!(t[i + 1], ' ' | '\n')
        {
            toks.push(tok(line, Kind::Str, i, i + 1, spaced));
            i += 2 + usize::from(t[i + 1] == '\\');
            continue;
        }
        if c == ':'
            && i + 1 < n
            && t[i + 1] != ':'
            && !p.is_some_and(|p| p.is(t, ":"))
            && !(p.is_some() && is_value(p) && !spaced)
        {
            // A symbol: :foo, :"foo", :+ ...
            let mut j = i + 1;
            if matches!(t[j], '"' | '\'') {
                let (next, extra) = skip_delimited(t, j + 1, t[j], t[j] == '"');
                j = next;
                line += extra;
            } else if is_alpha(t[j]) || matches!(t[j], '_' | '@' | '$') {
                j += 1;
                while j < n && (is_alnum(t[j]) || "_?!=".contains(t[j])) {
                    if t[j] == '=' && j + 1 < n && matches!(t[j + 1], '>' | '=') {
                        break;
                    }
                    j += 1;
                }
            } else if "+-*/<=>!~[%&|^".contains(t[j]) {
                while j < n && "+-*/<=>!~[]%&|^@".contains(t[j]) {
                    j += 1;
                }
            } else {
                toks.push(tok(line, Kind::Op, i, i + 1, spaced));
                i += 1;
                continue;
            }
            toks.push(tok(line, Kind::Symbol, i, j, spaced));
            i = j;
            continue;
        }
        if c == '$' && i + 1 < n && "!@&`'+~=/\\,;.<>*$?:\"".contains(t[i + 1]) {
            // A punctuation global such as $" or $` hides no string.
            toks.push(tok(line, Kind::Ivar, i, i + 2, spaced));
            i += 2;
            continue;
        }
        if c == '@' || c == '$' {
            let mut j = i + 1;
            while j < n && (is_alnum(t[j]) || matches!(t[j], '_' | '@')) {
                j += 1;
            }
            toks.push(tok(line, Kind::Ivar, i, j, spaced));
            i = j;
            continue;
        }
        if is_digit(c) {
            let mut j = i;
            while j < n && (is_alnum(t[j]) || matches!(t[j], '_' | '.')) {
                if t[j] == '.' && !(j + 1 < n && is_digit(t[j + 1])) {
                    break;
                }
                j += 1;
            }
            toks.push(tok(line, Kind::Number, i, j, spaced));
            i = j;
            continue;
        }
        if is_alpha(c) || c == '_' {
            let mut j = i;
            while j < n && (is_alnum(t[j]) || t[j] == '_') {
                j += 1;
            }
            if j < n && matches!(t[j], '?' | '!') && !(j + 1 < n && t[j + 1] == '=') {
                j += 1;
            }
            // A label (`end: 1`, `if: x`) is a hash key, not a keyword.
            if j < n && t[j] == ':' && !(j + 1 < n && t[j + 1] == ':') {
                toks.push(tok(line, Kind::Symbol, i, j + 1, spaced));
                i = j + 1;
                continue;
            }
            let mut kind = if is_upper(c) { Kind::Const } else { Kind::Word };
            if let Some(p) = p
                && p.kind == Kind::Op
                && (p.is(t, ".") || p.is(t, "&.") || p.is(t, "::"))
                && kind != Kind::Const
            {
                kind = Kind::Method;
            }
            toks.push(Tok {
                kw: Kw::of(&t[i..j]),
                ..tok(line, kind, i, j, spaced)
            });
            i = j;
            continue;
        }
        let kind = match c {
            '(' | '[' | '{' => Some(Kind::Open),
            ')' | ']' | '}' => Some(Kind::Close),
            ';' => Some(Kind::Nl),
            _ => None,
        };
        if let Some(kind) = kind {
            toks.push(tok(line, kind, i, i + 1, spaced));
            i += 1;
            continue;
        }
        const OPS: [&str; 12] = [
            "&.", "::", "=>", "->", "==", "!=", "<=", ">=", "||", "&&", "**", "<<",
        ];
        let len = OPS
            .iter()
            .find(|op| starts(t, i, op))
            .map_or(1, |op| op.len());
        toks.push(tok(line, Kind::Op, i, i + len, spaced));
        i += len;
    }
    toks
}

/// `l` without leading and trailing spaces and tabs.
fn trim_blanks(l: &[char]) -> &[char] {
    let blank = |c: &char| matches!(c, ' ' | '\t');
    let s = l.iter().position(|c| !blank(c)).unwrap_or(l.len());
    let e = l.iter().rposition(|c| !blank(c)).map_or(s, |e| e + 1);
    &l[s..e.max(s)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(text: &str) -> Vec<(usize, usize)> {
        let mut spans = outline(text);
        spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        spans.dedup();
        spans
    }

    #[test]
    fn modifier_if_opens_no_block() {
        let text = "class A\n  def f(x)\n    return 1 if x\n    y = 2 unless x\n    while x do\n      x -= 1\n    end\n  end\nend\n";
        assert_eq!(sorted(text), vec![(1, 9), (2, 8)]);
    }

    #[test]
    fn def_named_for_opens_nothing_more() {
        let text = "module M\n  def for\n    1\n  end\n\n  def self.end = 2\n\n  class << self\n    def ==(other) = true\n  end\nend\n";
        assert_eq!(sorted(text), vec![(1, 11), (2, 4), (6, 6), (8, 10), (9, 9)]);
    }

    #[test]
    fn strings_heredocs_and_labels_hide_keywords() {
        let text = "def f\n  s = \"end #{ \"end\" }\"\n  t = <<~EOS\n    end\n  EOS\n  h = { end: 1, if: 2 }\n  %w[end def]\nend\n=begin\ndef g\n=end\n";
        assert_eq!(sorted(text), vec![(1, 8)]);
    }
}
