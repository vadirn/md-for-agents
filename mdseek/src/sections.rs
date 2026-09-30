//! Turn a folder into sections: the units the search ranks.
//!
//! A section is one outline node. Code files outline through the `outline`
//! crate, and Markdown files through mdread's heading tree. Each section keeps
//! the range a reader cites, which includes its children, and its own text,
//! which excludes them. Only the own text is indexed, so a class never
//! outranks the method that answers.

use std::path::Path;

use anyhow::{Result, bail};
use ignore::WalkBuilder;
use ignore::types::TypesBuilder;
use outline::Lang;

/// Files over this size are generated or vendored far more often than written.
const MAX_FILE_BYTES: u64 = 1_000_000;

/// A file whose average line runs longer than this is minified.
const MINIFIED_LINE: usize = 300;

/// Comment lines above a definition that still describe it.
const MAX_LEADING_COMMENT: usize = 40;

/// The ripgrep file types the walk admits, per decision MDAGENTS-23.
/// JavaScript has its own type, apart from `ts`, though the TypeScript
/// outliner reads it. A `.vue` file the `js` type admits has no outliner, so
/// the walk skips it.
const TYPES: &[&str] = &["markdown", "ts", "js", "py", "rust", "ruby"];

/// What a section's file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Markdown,
    Code(Lang),
}

/// One ranked unit.
#[derive(Debug, Clone)]
pub struct Section {
    /// Path relative to the searched folder.
    pub path: String,
    pub kind: Kind,
    /// The range to cite: the whole definition or section, children included.
    pub start: usize,
    pub end: usize,
    /// The signature line, or the heading.
    pub label: String,
    /// The labels of the enclosing nodes, outermost first.
    pub context: Vec<String>,
    /// Frontmatter `description:` for Markdown, empty for code.
    pub description: String,
    /// What the index reads: the leading comment and the node's own lines.
    pub text: String,
    /// The section is a test, or lies in a test file or test module.
    pub test: bool,
}

/// One admitted file, read whole.
pub struct File {
    pub path: String,
    pub kind: Kind,
    pub content: String,
}

/// Walk `root` and read every file the admitted types include.
pub fn walk(root: &Path) -> Result<Vec<File>> {
    if !root.is_dir() {
        bail!("not a folder: {}", root.display());
    }
    let mut types = TypesBuilder::new();
    types.add_defaults();
    for name in TYPES {
        types.select(name);
    }
    let mut builder = WalkBuilder::new(root);
    builder
        .types(types.build()?)
        .require_git(false)
        .max_filesize(Some(MAX_FILE_BYTES))
        .sort_by_file_path(|a, b| a.cmp(b));

    let mut files = Vec::new();
    for entry in builder.build() {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        let Some(kind) = kind_of(&relative) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        // Prose runs a paragraph to a line, so only code can read as minified.
        if matches!(kind, Kind::Code(_)) && minified(&content) {
            continue;
        }
        files.push(File {
            path: relative,
            kind,
            content,
        });
    }
    Ok(files)
}

pub fn kind_of(path: &str) -> Option<Kind> {
    let lower = path.to_lowercase();
    if [
        ".md",
        ".markdown",
        ".mdx",
        ".mdown",
        ".mkd",
        ".mkdn",
        ".mdwn",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
    {
        return Some(Kind::Markdown);
    }
    Lang::of(path).map(Kind::Code)
}

fn minified(content: &str) -> bool {
    let lines = content.lines().count().max(1);
    content.len() / lines > MINIFIED_LINE
}

/// Every section of every file.
pub fn sections(files: &[File]) -> Vec<Section> {
    let mut out = Vec::new();
    for file in files {
        match file.kind {
            Kind::Markdown => markdown_sections(file, &mut out),
            Kind::Code(lang) => code_sections(file, lang, &mut out),
        }
    }
    out
}

fn code_sections(file: &File, lang: Lang, out: &mut Vec<Section>) {
    let lines: Vec<&str> = file.content.lines().collect();
    let nodes = outline::outline(lang, &file.content);
    let header = file_header(&lines, lang, &nodes);
    let mut walk = CodeWalk {
        file,
        lang,
        lines: &lines,
        header: &header,
        test_file: is_test_path(&file.path),
        context: Vec::new(),
        out,
    };
    for node in &nodes {
        walk.push(node, false);
    }
}

struct CodeWalk<'a> {
    file: &'a File,
    lang: Lang,
    lines: &'a [&'a str],
    header: &'a str,
    test_file: bool,
    context: Vec<String>,
    out: &'a mut Vec<Section>,
}

impl CodeWalk<'_> {
    fn push(&mut self, node: &outline::Node, in_test: bool) {
        let lines = self.lines;
        let first = lines.get(node.start - 1).map_or("", |l| l.trim());
        let test = in_test
            || self.test_file
            || first.starts_with("#[test]")
            || first.starts_with("#[cfg(test)]")
            || node.label.starts_with("mod tests");
        // A member that defines nothing, such as a docstring or a field,
        // stays in its parent's text instead of ranking on its own.
        let kept: Vec<&outline::Node> = node
            .children
            .iter()
            .filter(|c| is_definition(self.lang, c))
            .collect();
        if !is_import(&node.label) && !is_docstring(&node.label) {
            let mut text = leading_comment(lines, node.start, self.lang);
            // A kept child's leading comment is the child's, not the parent's.
            let owned: Vec<(usize, usize)> = kept
                .iter()
                .map(|c| (comment_start(lines, c.start, self.lang), c.end))
                .collect();
            for no in node.start..=node.end.min(lines.len()) {
                if owned.iter().any(|&(first, last)| first <= no && no <= last) {
                    continue;
                }
                text.push_str(lines[no - 1]);
                text.push('\n');
            }
            self.out.push(Section {
                path: self.file.path.clone(),
                kind: Kind::Code(self.lang),
                start: node.start,
                end: node.end,
                label: node.label.clone(),
                context: self.context.clone(),
                description: self.header.to_string(),
                text,
                test,
            });
        }
        self.context.push(node.label.clone());
        for child in kept {
            self.push(child, test);
        }
        self.context.pop();
    }
}

/// Whether a member defines something a question could be about. Ruby's
/// outliner reports only definitions; the others also report fields,
/// docstrings and statements.
fn is_definition(lang: Lang, node: &outline::Node) -> bool {
    let label = strip_modifiers(&node.label);
    match lang {
        Lang::Ruby | Lang::Rust => !is_import(label),
        Lang::Python => {
            label.starts_with("def ")
                || label.starts_with("async def ")
                || label.starts_with("class ")
        }
        Lang::TypeScript => {
            node.end > node.start
                || label.contains("=>")
                || label.contains("function")
                || label.starts_with("constructor")
                || label.starts_with("get ")
                || label.starts_with("set ")
                || label
                    .find('(')
                    .is_some_and(|k| !label[..k].contains([':', '=']))
        }
    }
}

fn strip_modifiers(label: &str) -> &str {
    let mut label = label.trim();
    loop {
        let before = label;
        for m in [
            "pub(crate) ",
            "pub(super) ",
            "pub ",
            "export ",
            "default ",
            "public ",
            "private ",
            "protected ",
            "static ",
            "readonly ",
            "abstract ",
            "override ",
            "declare ",
            "async ",
        ] {
            label = label.strip_prefix(m).unwrap_or(label);
        }
        if label == before {
            return label;
        }
    }
}

fn is_docstring(label: &str) -> bool {
    let label = label.trim_start_matches(['r', 'b', 'u', 'f', 'R', 'B', 'U', 'F']);
    label.starts_with("\"\"\"") || label.starts_with("'''")
}

/// A file's opening comment or module docstring. It says what the module is
/// for, so every section in the file carries it as a description.
fn file_header(lines: &[&str], lang: Lang, nodes: &[outline::Node]) -> String {
    const MAX: usize = 30;
    if let Some(first) = nodes.first().filter(|n| is_docstring(&n.label)) {
        return lines
            .iter()
            .take(first.end.min(first.start + MAX - 1))
            .skip(first.start - 1)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
    }
    // A comment directly above a first definition describes that definition.
    let stop = match nodes.first() {
        Some(n) if !is_import(&n.label) => comment_start(lines, n.start, lang) - 1,
        Some(n) => n.start - 1,
        None => lines.len(),
    }
    .min(MAX);
    lines
        .iter()
        .take(stop)
        .filter(|l| {
            let t = l.trim_start();
            match lang {
                Lang::Rust | Lang::TypeScript => {
                    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
                }
                Lang::Python | Lang::Ruby => {
                    t.starts_with('#')
                        && !t.starts_with("#!")
                        && !t.contains("frozen_string_literal")
                }
            }
        })
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tests name what they check in long, plain words, so they match plain
/// questions better than the definitions those questions are about.
pub fn is_test_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    lower.split('/').any(|c| {
        matches!(
            c,
            "test" | "tests" | "spec" | "specs" | "__tests__" | "testing" | "fixtures" | "benches"
        )
    }) || name.starts_with("test_")
        || name == "tests.rs"
        || name == "conftest.py"
        || [".test.", ".spec.", "_test.", "_spec."]
            .iter()
            .any(|m| name.contains(m))
}

/// Imports carry names but answer nothing, and they would match every query
/// that names what they import.
fn is_import(label: &str) -> bool {
    let label = label
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub ");
    [
        "use ",
        "import ",
        "from ",
        "export * ",
        "export {",
        "require ",
        "require(",
        "require_relative ",
        "extern crate ",
    ]
    .iter()
    .any(|p| label.starts_with(p))
        || (label.starts_with("mod ") && label.ends_with(';'))
}

/// The comment block directly above `start`: the words a writer chose to
/// describe the definition, which a question in plain words most often shares.
fn leading_comment(lines: &[&str], start: usize, lang: Lang) -> String {
    let mut text = String::new();
    for line in &lines[comment_start(lines, start, lang) - 1..start - 1] {
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// The first line of the comment block directly above `start`, or `start`
/// when none is there. Rust's `//!` documents the module, not the next item,
/// so it ends the block.
fn comment_start(lines: &[&str], start: usize, lang: Lang) -> usize {
    let mut first = start;
    while first > 1 && start - first < MAX_LEADING_COMMENT {
        let above = lines[first - 2].trim_start();
        let comment = match lang {
            Lang::Rust if above.starts_with("//!") || above.starts_with("/*!") => false,
            Lang::Rust | Lang::TypeScript => {
                above.starts_with("//")
                    || above.starts_with("/*")
                    || above.starts_with('*')
                    || above.ends_with("*/")
            }
            Lang::Python | Lang::Ruby => above.starts_with('#'),
        };
        if !comment {
            break;
        }
        first -= 1;
    }
    first
}

fn markdown_sections(file: &File, out: &mut Vec<Section>) {
    let Ok(mdread::Reading::Overview(overview)) = mdread::read_content(
        &file.path,
        &file.content,
        None,
        None,
        false,
        mdread::DEFAULT_THRESHOLD,
        mdread::Dialect::default(),
    ) else {
        return;
    };
    let lines: Vec<&str> = file.content.lines().collect();
    let description = mdsearch::frontmatter::description(&file.content);
    let title = file
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&file.path)
        .rsplit_once('.')
        .map_or(file.path.as_str(), |(stem, _)| stem)
        .to_string();
    let mut push =
        |start: usize, end: usize, own_end: usize, label: String, context: Vec<String>| {
            let mut text = String::new();
            for line in lines.iter().take(own_end.min(lines.len())).skip(start - 1) {
                text.push_str(line);
                text.push('\n');
            }
            if text.trim().is_empty() {
                return;
            }
            out.push(Section {
                path: file.path.clone(),
                kind: Kind::Markdown,
                start,
                end,
                label,
                context,
                description: description.clone(),
                text,
                test: false,
            });
        };
    if let Some(lede) = &overview.text {
        let end = lede.line + lede.lines.saturating_sub(1);
        push(lede.line, end, end, title.clone(), Vec::new());
    }
    fn walk_tree(
        nodes: &[mdread::TreeNode],
        context: &mut Vec<String>,
        push: &mut dyn FnMut(usize, usize, usize, String, Vec<String>),
    ) {
        for node in nodes {
            let end = node.line + node.lines.saturating_sub(1);
            let own_end = node.children.first().map_or(end, |c| c.line - 1);
            push(
                node.line,
                end,
                own_end,
                node.heading.clone(),
                context.clone(),
            );
            context.push(node.heading.clone());
            walk_tree(&node.children, context, push);
            context.pop();
        }
    }
    let mut context = vec![title];
    walk_tree(&overview.tree, &mut context, &mut push);
}

/// Split each camelCase or PascalCase identifier into its words, so a question
/// in plain words meets `selectBestEncoding` as "select best encoding".
/// snake_case needs nothing: the tokenizer already splits at `_`.
pub fn split_identifiers(text: &str) -> String {
    let mut out = String::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        let chars: Vec<char> = word.chars().collect();
        let mut parts = Vec::new();
        let mut begin = 0;
        for i in 1..chars.len() {
            let (a, b) = (chars[i - 1], chars[i]);
            let next_lower = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
            let boundary = (a.is_lowercase() && b.is_uppercase())
                || (a.is_uppercase() && b.is_uppercase() && next_lower)
                || (a.is_alphabetic() && b.is_ascii_digit())
                || (a.is_ascii_digit() && b.is_alphabetic());
            if boundary {
                parts.push(chars[begin..i].iter().collect::<String>());
                begin = i;
            }
        }
        if !parts.is_empty() {
            parts.push(chars[begin..].iter().collect());
            out.push_str(&parts.join(" "));
            out.push(' ');
        }
    }
    out
}

/// One section per file, for comparing against section-level ranking: the
/// file's name titles it and its whole text is its body.
pub fn whole_files(files: &[File]) -> Vec<Section> {
    files
        .iter()
        .map(|file| {
            let (description, text) = match file.kind {
                Kind::Markdown => (
                    mdsearch::frontmatter::description(&file.content),
                    mdsearch::frontmatter::body(&file.content).to_string(),
                ),
                Kind::Code(_) => (String::new(), file.content.clone()),
            };
            Section {
                path: file.path.clone(),
                kind: file.kind,
                start: 1,
                end: file.content.lines().count().max(1),
                label: file.path.clone(),
                context: Vec::new(),
                description,
                text,
                test: is_test_path(&file.path),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_splits_and_snake_case_is_left_to_the_tokenizer() {
        assert_eq!(
            split_identifiers("selectBestEncoding"),
            "select Best Encoding "
        );
        assert_eq!(split_identifiers("HTTPServer x"), "HTTP Server ");
        assert_eq!(split_identifiers("parse_input"), "");
        assert_eq!(split_identifiers("utf8"), "utf 8 ");
    }

    #[test]
    fn the_walk_admits_javascript_beside_typescript() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.js", "b.mjs", "c.ts", "d.vue", "e.txt"] {
            std::fs::write(dir.path().join(name), "function f() {\n}\n").unwrap();
        }
        let paths: Vec<String> = walk(dir.path())
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(paths, ["a.js", "b.mjs", "c.ts"]);
    }

    #[test]
    fn imports_are_not_sections() {
        assert!(is_import("use std::path::Path;"));
        assert!(is_import("pub use corpus::Doc;"));
        assert!(is_import("import { a } from \"b\";"));
        assert!(is_import("mod corpus;"));
        assert!(!is_import("mod tests {"));
        assert!(!is_import("fn used() {"));
    }
}
