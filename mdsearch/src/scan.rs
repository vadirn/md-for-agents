//! The file walk: which Markdown files reach the index.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Result, bail};
use ignore::WalkBuilder;

use crate::corpus::Doc;
use crate::frontmatter;

/// Extensions the walk reads. A Markdown file under any other name stays unread.
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown"];

/// Which files the walk yields.
#[derive(Debug, Clone)]
pub struct Walk {
    /// Obey exclusion files. Each folder's `.gitignore` and `.ignore` govern it
    /// and everything under it, in a plain folder as much as in a git repository.
    pub ignore_files: bool,
    /// Yield dot-files and dot-folders, which the walk skips by default.
    pub hidden: bool,
    /// One more exclusion filename, read alongside `.gitignore` and `.ignore`,
    /// for rules belonging to the caller rather than to the repository. The file
    /// takes gitignore syntax, whatever it is named.
    pub custom_ignore: Option<String>,
}

impl Default for Walk {
    fn default() -> Self {
        Walk {
            ignore_files: true,
            hidden: false,
            custom_ignore: None,
        }
    }
}

/// One Markdown file, read whole.
#[derive(Debug)]
pub struct MdFile {
    /// Path joined to the root that reached it, as the caller gave that root,
    /// less a leading `./`: the identity every result reports. It opens from
    /// the folder the caller ran in.
    pub path: String,
    /// File name without its extension.
    pub name: String,
    pub content: String,
}

impl MdFile {
    /// The document this file indexes as: its name titles it, its frontmatter
    /// `description:` describes it, and the prose after that block is its body.
    pub fn to_doc(&self) -> Doc {
        Doc {
            id: self.path.clone(),
            title: self.name.clone(),
            description: frontmatter::description(&self.content),
            body: frontmatter::body(&self.content).to_string(),
        }
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .is_some_and(|e| MARKDOWN_EXTENSIONS.contains(&e.as_str()))
}

/// Walk every root and read each Markdown file the options admit, in path order.
///
/// The roots feed one list, so a caller indexes them as one corpus. Each root
/// obeys its own exclusion files. A file under two roots counts once, under the
/// first root that reaches it: a copy would skew every term's rarity.
///
/// A file that fails to read is skipped with a warning on stderr, so one
/// unreadable or non-UTF-8 file never fails the search. A missing root is an
/// error instead, because an empty result would read as "no matches".
pub fn scan<P: AsRef<Path>>(roots: &[P], walk: Walk) -> Result<Vec<MdFile>> {
    let Some((first, rest)) = roots.split_first() else {
        bail!("no folder given");
    };
    for root in roots {
        let root = root.as_ref();
        if !root.is_dir() {
            bail!("not a folder: {}", root.display());
        }
    }

    let mut builder = WalkBuilder::new(first);
    for root in rest {
        builder.add(root);
    }
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
    if let (true, Some(name)) = (walk.ignore_files, walk.custom_ignore.as_deref()) {
        builder.add_custom_ignore_filename(name);
    }

    let mut seen = HashSet::new();
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
        // Overlapping roots reach one file twice, perhaps spelled two ways, as
        // in `docs/a.md` and `/home/me/docs/a.md`. One canonical path names both.
        // One root reaches each file once, so it skips the check.
        if roots.len() > 1 {
            let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            if !seen.insert(canonical) {
                continue;
            }
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: skipping {} ({})", path.display(), e);
                continue;
            }
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        files.push(MdFile {
            path: path
                .strip_prefix(".")
                .unwrap_or(path)
                .to_string_lossy()
                .to_string(),
            name,
            content,
        });
    }

    // Index in a fixed order, so equal scores rank the same way on every run.
    files.sort_by(|a, b| a.path.cmp(&b.path));
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

    /// Each file's path under `root`. The walk reports the root too, so the
    /// expected names below stay short.
    fn names(root: &Path, files: &[MdFile]) -> Vec<String> {
        files
            .iter()
            .map(|f| {
                let path = Path::new(&f.path);
                let under = path.strip_prefix(root).unwrap_or_else(|_| {
                    panic!("{} is not under {}", path.display(), root.display())
                });
                under.to_string_lossy().to_string()
            })
            .collect()
    }

    #[test]
    fn reads_markdown_and_skips_other_extensions() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "note.md", "a");
        write(tmp.path(), "long.markdown", "b");
        write(tmp.path(), "code.rs", "c");
        let files = scan(&[tmp.path()], Walk::default()).unwrap();
        assert_eq!(names(tmp.path(), &files), vec!["long.markdown", "note.md"]);
    }

    #[test]
    fn descends_into_subfolders() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "deep/nested/note.md", "a");
        let files = scan(&[tmp.path()], Walk::default()).unwrap();
        assert_eq!(names(tmp.path(), &files), vec!["deep/nested/note.md"]);
    }

    #[test]
    fn a_gitignore_excludes_its_subtree() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".gitignore", "vendor/\n");
        write(tmp.path(), "keep.md", "a");
        write(tmp.path(), "vendor/skip.md", "b");
        let files = scan(&[tmp.path()], Walk::default()).unwrap();
        assert_eq!(names(tmp.path(), &files), vec!["keep.md"]);
    }

    #[test]
    fn the_custom_ignore_file_excludes_its_own_patterns() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".customignore", "*.tmp.md\n");
        write(tmp.path(), "keep.md", "a");
        write(tmp.path(), "draft.tmp.md", "b");
        let walk = Walk {
            custom_ignore: Some(".customignore".into()),
            ..Walk::default()
        };
        let files = scan(&[tmp.path()], walk).unwrap();
        assert_eq!(names(tmp.path(), &files), vec!["keep.md"]);
    }

    #[test]
    fn ignore_files_off_yields_the_excluded_files() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".gitignore", "vendor/\n");
        write(tmp.path(), "keep.md", "a");
        write(tmp.path(), "vendor/skip.md", "b");
        let walk = Walk {
            ignore_files: false,
            ..Walk::default()
        };
        let files = scan(&[tmp.path()], walk).unwrap();
        assert_eq!(names(tmp.path(), &files), vec!["keep.md", "vendor/skip.md"]);
    }

    #[test]
    fn hidden_files_stay_out_until_asked_for() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "keep.md", "a");
        write(tmp.path(), ".secret/note.md", "b");
        assert_eq!(
            names(tmp.path(), &scan(&[tmp.path()], Walk::default()).unwrap()),
            vec!["keep.md"]
        );
        let walk = Walk {
            hidden: true,
            ..Walk::default()
        };
        assert_eq!(
            names(tmp.path(), &scan(&[tmp.path()], walk).unwrap()),
            vec![".secret/note.md", "keep.md"]
        );
    }

    #[test]
    fn a_missing_root_is_an_error_not_an_empty_result() {
        let tmp = TempDir::new().unwrap();
        let err = scan(&[tmp.path().join("absent")], Walk::default()).unwrap_err();
        assert!(err.to_string().contains("not a folder"), "got: {}", err);
    }

    #[test]
    fn a_path_starts_with_its_root_as_given() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "docs/sync/recovery.md", "a");
        let root = tmp.path().join("docs");
        let files = scan(&[&root], Walk::default()).unwrap();
        assert_eq!(
            files[0].path,
            root.join("sync/recovery.md").to_string_lossy()
        );
    }

    #[test]
    fn two_roots_feed_one_list_in_path_order() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "docs/b.md", "a");
        write(tmp.path(), "deploy/a.md", "b");
        write(tmp.path(), "elsewhere/c.md", "c");
        let roots = [tmp.path().join("docs"), tmp.path().join("deploy")];
        let files = scan(&roots, Walk::default()).unwrap();
        assert_eq!(names(tmp.path(), &files), vec!["deploy/a.md", "docs/b.md"]);
    }

    #[test]
    fn overlapping_roots_count_a_file_once() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "docs/top.md", "a");
        write(tmp.path(), "docs/sync/deep.md", "b");
        let roots = [tmp.path().join("docs"), tmp.path().join("docs/sync")];
        let files = scan(&roots, Walk::default()).unwrap();
        assert_eq!(
            names(tmp.path(), &files),
            vec!["docs/sync/deep.md", "docs/top.md"]
        );
    }

    #[test]
    fn a_root_spelled_two_ways_counts_its_files_once() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "docs/a.md", "a");
        let roots = [tmp.path().join("docs"), tmp.path().join("docs/../docs")];
        let files = scan(&roots, Walk::default()).unwrap();
        assert_eq!(names(tmp.path(), &files), vec!["docs/a.md"]);
    }

    #[test]
    fn two_readmes_under_different_roots_both_reach_the_list() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "docs/README.md", "a");
        write(tmp.path(), "deploy/README.md", "b");
        let roots = [tmp.path().join("docs"), tmp.path().join("deploy")];
        let files = scan(&roots, Walk::default()).unwrap();
        assert_eq!(
            names(tmp.path(), &files),
            vec!["deploy/README.md", "docs/README.md"]
        );
    }

    #[test]
    fn a_missing_second_root_is_an_error_naming_it() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "docs/a.md", "a");
        let roots = [tmp.path().join("docs"), tmp.path().join("absent")];
        let err = scan(&roots, Walk::default()).unwrap_err();
        assert!(err.to_string().contains("not a folder"), "got: {}", err);
        assert!(err.to_string().contains("absent"), "got: {}", err);
    }

    #[test]
    fn no_root_at_all_is_an_error() {
        let err = scan::<&Path>(&[], Walk::default()).unwrap_err();
        assert!(err.to_string().contains("no folder given"), "got: {}", err);
    }

    #[test]
    fn name_drops_the_extension() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "Alpha note.md", "a");
        let files = scan(&[tmp.path()], Walk::default()).unwrap();
        assert_eq!(files[0].name, "Alpha note");
    }
}
