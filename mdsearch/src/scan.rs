//! The file walk: which Markdown files reach the index.

use std::path::Path;

use anyhow::{Result, bail};
use ignore::WalkBuilder;

/// Exclusion file read alongside `.gitignore` and `.ignore`, for rules that
/// belong to the search rather than to the repository.
pub const IGNORE_FILE: &str = ".mdsearchignore";

/// Extensions the walk reads. A Markdown file under any other name stays unread.
const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown"];

/// Which files the walk yields.
#[derive(Debug, Clone, Copy)]
pub struct Walk {
    /// Obey exclusion files. Each folder's `.gitignore`, `.ignore` and
    /// `.mdsearchignore` govern it and everything under it, in a plain folder as
    /// much as in a git repository.
    pub ignore_files: bool,
    /// Yield dot-files and dot-folders, which the walk skips by default.
    pub hidden: bool,
}

impl Default for Walk {
    fn default() -> Self {
        Walk {
            ignore_files: true,
            hidden: false,
        }
    }
}

/// One Markdown file, read whole.
#[derive(Debug)]
pub struct MdFile {
    /// Path relative to the search root: the identity every result reports.
    pub relative: String,
    /// File name without its extension.
    pub name: String,
    pub content: String,
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .is_some_and(|e| MARKDOWN_EXTENSIONS.contains(&e.as_str()))
}

/// Walk `root` and read every Markdown file the options admit, in path order.
///
/// A file that fails to read is skipped with a warning on stderr, so one
/// unreadable or non-UTF-8 file never fails the search. A missing root is an
/// error instead, because an empty result would read as "no matches".
pub fn scan(root: &Path, walk: Walk) -> Result<Vec<MdFile>> {
    if !root.is_dir() {
        bail!("not a folder: {}", root.display());
    }

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!walk.hidden)
        .parents(walk.ignore_files)
        .ignore(walk.ignore_files)
        .git_ignore(walk.ignore_files)
        .git_global(walk.ignore_files)
        .git_exclude(walk.ignore_files)
        // Read `.gitignore` outside a repository too: the folder searched is not
        // always the folder git tracks.
        .require_git(false);
    if walk.ignore_files {
        builder.add_custom_ignore_filename(IGNORE_FILE);
    }

    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("warning: {}", e);
                continue;
            }
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if !is_markdown(path) {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: skipping {} ({})", path.display(), e);
                continue;
            }
        };
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        files.push(MdFile {
            relative,
            name,
            content,
        });
    }

    // Index in a fixed order, so equal scores rank the same way on every run.
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn names(files: &[MdFile]) -> Vec<String> {
        files.iter().map(|f| f.relative.clone()).collect()
    }

    #[test]
    fn reads_markdown_and_skips_other_extensions() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "a");
        write(tmp.path(), "long.markdown", "b");
        write(tmp.path(), "code.rs", "c");
        let files = scan(tmp.path(), Walk::default()).unwrap();
        assert_eq!(names(&files), vec!["long.markdown", "note.md"]);
    }

    #[test]
    fn descends_into_subfolders() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "deep/nested/note.md", "a");
        let files = scan(tmp.path(), Walk::default()).unwrap();
        assert_eq!(names(&files), vec!["deep/nested/note.md"]);
    }

    #[test]
    fn a_gitignore_excludes_its_subtree() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".gitignore", "vendor/\n");
        write(tmp.path(), "keep.md", "a");
        write(tmp.path(), "vendor/skip.md", "b");
        let files = scan(tmp.path(), Walk::default()).unwrap();
        assert_eq!(names(&files), vec!["keep.md"]);
    }

    #[test]
    fn the_search_ignore_file_excludes_its_own_patterns() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), IGNORE_FILE, "*.tmp.md\n");
        write(tmp.path(), "keep.md", "a");
        write(tmp.path(), "draft.tmp.md", "b");
        let files = scan(tmp.path(), Walk::default()).unwrap();
        assert_eq!(names(&files), vec!["keep.md"]);
    }

    #[test]
    fn ignore_files_off_yields_the_excluded_files() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".gitignore", "vendor/\n");
        write(tmp.path(), "keep.md", "a");
        write(tmp.path(), "vendor/skip.md", "b");
        let walk = Walk {
            ignore_files: false,
            hidden: false,
        };
        let files = scan(tmp.path(), walk).unwrap();
        assert_eq!(names(&files), vec!["keep.md", "vendor/skip.md"]);
    }

    #[test]
    fn hidden_files_stay_out_until_asked_for() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "keep.md", "a");
        write(tmp.path(), ".secret/note.md", "b");
        assert_eq!(
            names(&scan(tmp.path(), Walk::default()).unwrap()),
            vec!["keep.md"]
        );
        let walk = Walk {
            ignore_files: true,
            hidden: true,
        };
        assert_eq!(
            names(&scan(tmp.path(), walk).unwrap()),
            vec![".secret/note.md", "keep.md"]
        );
    }

    #[test]
    fn a_missing_root_is_an_error_not_an_empty_result() {
        let tmp = TempDir::new().unwrap();
        let err = scan(&tmp.path().join("absent"), Walk::default()).unwrap_err();
        assert!(err.to_string().contains("not a folder"), "got: {}", err);
    }

    #[test]
    fn name_drops_the_extension() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "Alpha note.md", "a");
        let files = scan(tmp.path(), Walk::default()).unwrap();
        assert_eq!(files[0].name, "Alpha note");
    }
}
