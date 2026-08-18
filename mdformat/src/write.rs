//! The crate's one file-writing path, and the gate around it.
//!
//! Writes through a temporary file and renames, so a failed write cannot leave
//! a truncated file behind.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The sentence every refusal ends with. Named once so a refusal cannot be
/// worded into sounding like a transient error the caller should retry around.
pub const GATE: &str = "this rewrites one file a person chose; rewriting a batch, \
                        a directory, or a file nobody looked at is a separate tier \
                        this binary does not implement";

/// Why [`target`] refused an invocation.
///
/// Every variant is a refusal of the *invocation*, decided before a byte is
/// read — not a failure of the formatting, which happens later and reports
/// itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Some number of paths other than one. Zero and two are refused for the
    /// same reason: neither is a file a person pointed at.
    NotOne(usize),
    /// `-`, or an empty path. Stdin has no file to rewrite, and inventing one
    /// would be this program choosing the target.
    Stdin,
    /// The path exists but is not a regular file — a directory most often,
    /// which is the shape a batch would arrive in.
    NotAFile(PathBuf),
    /// The path is a symbolic link. A rename over it would replace the link
    /// itself with a regular file, which is a change to the tree and not to the
    /// document; following it instead would rewrite a file whose name the
    /// caller did not type.
    Symlink(PathBuf),
    /// The path could not be stat'd — it does not exist, or is unreadable.
    Unreadable(PathBuf, String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NotOne(n) => {
                write!(f, "--write takes exactly one file path and got {n}; {GATE}")
            }
            Refusal::Stdin => write!(
                f,
                "--write rewrites a named file, so it takes no stdin input; {GATE}"
            ),
            Refusal::NotAFile(p) => write!(
                f,
                "--write takes one regular file and {} is not one; {GATE}",
                p.display()
            ),
            Refusal::Symlink(p) => write!(
                f,
                "--write takes one regular file and {} is a symbolic link, which \
                 replacing it atomically would destroy; {GATE}",
                p.display()
            ),
            Refusal::Unreadable(p, e) => {
                write!(f, "--write cannot read {}: {e}; {GATE}", p.display())
            }
        }
    }
}

/// The one file this invocation may rewrite, or why there is none.
///
/// Takes the caller's paths **as given** — before any "no paths means stdin"
/// defaulting — because that defaulting is exactly what would turn a bare
/// `--write` into a rewrite of a file nobody named.
pub fn target(paths: &[String]) -> Result<PathBuf, Refusal> {
    let [only] = paths else {
        return Err(Refusal::NotOne(paths.len()));
    };
    if only == "-" || only.is_empty() {
        return Err(Refusal::Stdin);
    }
    let path = PathBuf::from(only);
    // `symlink_metadata`, not `metadata`: the question is what this name is,
    // not what it points at.
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) => return Err(Refusal::Unreadable(path, e.to_string())),
    };
    if meta.file_type().is_symlink() {
        return Err(Refusal::Symlink(path));
    }
    if !meta.is_file() {
        return Err(Refusal::NotAFile(path));
    }
    Ok(path)
}

/// Replace `path`'s contents with `contents`, atomically, keeping its
/// permission bits.
///
/// The target is never opened for writing: the bytes go to a sibling temp file
/// which is then renamed over it. An interrupted run therefore leaves either the
/// old document intact or the new one complete, and the temp file it may leave
/// behind is a hidden dotfile beside the target rather than a half-written
/// version of it.
///
/// `path` must already exist — the permission bits come from it, and a
/// formatter has no file to rewrite otherwise.
pub fn replace(path: &Path, contents: &str) -> io::Result<()> {
    let perms = fs::metadata(path)?.permissions();
    let tmp = temp_path(path);
    let outcome = fill(&tmp, contents).and_then(|()| {
        // Ownership cannot be carried without privileges and is left to the
        // filesystem's default; the mode is what a `chmod` on this file
        // actually recorded, so it is what must survive.
        fs::set_permissions(&tmp, perms)?;
        fs::rename(&tmp, path)
    });
    if outcome.is_err() {
        // The target is still untouched at this point, so the only thing to
        // undo is the temp file. Its removal is best-effort: a stale temp file
        // is inert, and reporting its removal error would hide the real one.
        let _ = fs::remove_file(&tmp);
        return outcome;
    }
    // Best-effort: fsync on the directory is what makes the rename itself
    // durable across a power loss. It is not what makes it atomic, and it fails
    // on filesystems that refuse a directory handle, so a failure here is not a
    // failed write.
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}

/// Write `contents` to a temp file that must not already exist, and flush it to
/// the device before anyone renames it.
fn fill(tmp: &Path, contents: &str) -> io::Result<()> {
    // `create_new`: a colliding name is an error rather than a silent
    // overwrite, so this can never clobber a file it did not create.
    let mut f = OpenOptions::new().write(true).create_new(true).open(tmp)?;
    f.write_all(contents.as_bytes())?;
    // Before the rename, not after: a rename that publishes unflushed bytes is
    // atomic in the directory and empty in the file.
    f.sync_all()
}

/// A sibling of `target`, in the same directory, whose name no other run can
/// pick: hidden (so an indexer ignores it), tagged with the process id,
/// and counted within the process.
fn temp_path(target: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut name = OsString::from(".");
    match target.file_name() {
        Some(f) => name.push(f),
        None => name.push("mdformat"),
    }
    name.push(format!(".mdformat-{}-{n}.tmp", std::process::id()));
    match target.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(dir) => dir.join(name),
        None => PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;

    /// A fresh empty directory under the environment's temp dir, named after
    /// the calling test. Never `/tmp` directly: `temp_dir` reads `TMPDIR`.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mdformat-write-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, bytes).expect("write fixture");
        p
    }

    #[test]
    fn one_existing_file_is_the_only_accepted_invocation() {
        let dir = scratch("target-ok");
        let p = file(&dir, "note.md", b"# T\n");
        let got = target(&[p.to_string_lossy().into_owned()]).expect("one file is admitted");
        assert_eq!(got, p);
    }

    #[test]
    fn two_paths_are_refused_and_the_message_names_the_gate() {
        let dir = scratch("target-two");
        let a = file(&dir, "a.md", b"# A\n");
        let b = file(&dir, "b.md", b"# B\n");
        let err = target(&[
            a.to_string_lossy().into_owned(),
            b.to_string_lossy().into_owned(),
        ])
        .expect_err("two paths are refused");
        assert_eq!(err, Refusal::NotOne(2));
        let said = err.to_string();
        assert!(said.contains("exactly one file path"), "{said}");
        assert!(said.contains(GATE), "{said}");
    }

    #[test]
    fn no_path_is_refused_rather_than_defaulted_to_stdin() {
        let err = target(&[]).expect_err("no path is refused");
        assert_eq!(err, Refusal::NotOne(0));
    }

    #[test]
    fn stdin_is_refused_because_it_names_no_file() {
        let err = target(&["-".to_string()]).expect_err("stdin is refused");
        assert_eq!(err, Refusal::Stdin);
        assert!(err.to_string().contains(GATE));
    }

    #[test]
    fn a_directory_is_refused_and_never_walked() {
        let dir = scratch("target-dir");
        file(&dir, "inside.md", b"# I\n");
        let err =
            target(&[dir.to_string_lossy().into_owned()]).expect_err("a directory is refused");
        assert_eq!(err, Refusal::NotAFile(dir.clone()));
        let said = err.to_string();
        assert!(said.contains("one regular file"), "{said}");
        assert!(said.contains(GATE), "{said}");
    }

    #[test]
    fn a_symlink_is_refused_because_replacing_it_would_destroy_the_link() {
        let dir = scratch("target-link");
        let real = file(&dir, "real.md", b"# R\n");
        let link = dir.join("link.md");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let err = target(&[link.to_string_lossy().into_owned()]).expect_err("a symlink is refused");
        assert_eq!(err, Refusal::Symlink(link));
    }

    #[test]
    fn a_missing_path_is_refused_before_anything_is_read() {
        let dir = scratch("target-missing");
        let p = dir.join("absent.md");
        let err =
            target(&[p.to_string_lossy().into_owned()]).expect_err("a missing path is refused");
        assert!(matches!(err, Refusal::Unreadable(ref q, _) if *q == p));
    }

    #[test]
    fn replace_puts_the_new_bytes_in_the_file() {
        let dir = scratch("replace-ok");
        let p = file(&dir, "note.md", b"old\n");
        replace(&p, "new\n").expect("replace");
        assert_eq!(fs::read(&p).expect("read back"), b"new\n");
    }

    #[test]
    fn the_original_file_object_is_never_written_to() {
        use std::os::unix::fs::MetadataExt;
        let dir = scratch("replace-atomic");
        let p = file(&dir, "note.md", b"old bytes\n");
        let before = fs::metadata(&p).expect("stat before").ino();
        let mut held = File::open(&p).expect("hold the old file open");

        replace(&p, "new bytes\n").expect("replace");

        let mut still = String::new();
        held.read_to_string(&mut still).expect("read the held file");
        assert_eq!(
            still, "old bytes\n",
            "the old inode must still hold the old document"
        );
        assert_eq!(fs::read(&p).expect("read back"), b"new bytes\n");
        let after = fs::metadata(&p).expect("stat after").ino();
        assert_ne!(before, after, "the name must point at a new inode");
    }

    #[test]
    fn a_successful_replacement_leaves_no_temp_file_behind() {
        let dir = scratch("replace-clean");
        let p = file(&dir, "note.md", b"old\n");
        replace(&p, "new\n").expect("replace");
        let entries: Vec<_> = fs::read_dir(&dir)
            .expect("read dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(entries, vec![OsString::from("note.md")], "{entries:?}");
    }

    #[test]
    fn the_temp_file_is_a_hidden_sibling_of_the_target() {
        let dir = scratch("replace-sibling");
        let p = dir.join("note.md");
        let tmp = temp_path(&p);
        assert_eq!(tmp.parent(), Some(dir.as_path()));
        let name = tmp
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        assert!(name.starts_with(".note.md.mdformat-"), "{name}");
        assert!(name.ends_with(".tmp"), "{name}");
        assert_ne!(tmp, temp_path(&p), "two temp names must not collide");
    }

    #[test]
    fn the_permission_bits_survive_the_replacement() {
        let dir = scratch("replace-perms");
        for mode in [0o600u32, 0o640, 0o755, 0o444] {
            let p = file(&dir, &format!("m{mode:o}.md"), b"old\n");
            fs::set_permissions(&p, fs::Permissions::from_mode(mode)).expect("chmod");
            replace(&p, "new\n").expect("replace");
            assert_eq!(fs::read(&p).expect("read back"), b"new\n");
            let got = fs::metadata(&p).expect("stat").permissions().mode() & 0o777;
            assert_eq!(got, mode, "mode {mode:o} must survive, got {got:o}");
        }
    }

    #[test]
    fn a_stale_temp_file_blocks_nothing() {
        let dir = scratch("replace-stale");
        let p = file(&dir, "note.md", b"old\n");
        let stale = dir.join(".note.md.mdformat-0-0.tmp");
        fs::write(&stale, b"half written").expect("stale temp");
        replace(&p, "new\n").expect("replace");
        assert_eq!(fs::read(&p).expect("read back"), b"new\n");
        assert_eq!(fs::read(&stale).expect("stale survives"), b"half written");
    }

    #[test]
    fn replacing_a_missing_file_is_an_error_and_creates_nothing() {
        let dir = scratch("replace-missing");
        let p = dir.join("absent.md");
        replace(&p, "new\n").expect_err("no file to take permissions from");
        assert!(!p.exists(), "nothing is created");
    }
}
