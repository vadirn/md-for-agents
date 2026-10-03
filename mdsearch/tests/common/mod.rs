//! Helpers the library and CLI tests share, so both assert on one corpus.

// Each test crate compiles its own copy, and one need not use every helper.
#![allow(dead_code)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

pub fn write(dir: &Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Two folders whose parent holds nothing else: `docs` and `deploy`.
pub fn two_folders() -> TempDir {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "docs/sync/recovery.md",
        "Halt the sender, then clear the halt for recovery.\n",
    );
    write(
        tmp.path(),
        "docs/sync/queue.md",
        "The queue drains on restart.\n",
    );
    write(
        tmp.path(),
        "docs/style.md",
        "Headings take sentence case.\n",
    );
    write(
        tmp.path(),
        "deploy/README.md",
        "Runbook: halt traffic, restore the snapshot, confirm recovery.\n",
    );
    write(
        tmp.path(),
        "deploy/notes.md",
        "Clear the cache after a deploy.\n",
    );
    tmp
}
