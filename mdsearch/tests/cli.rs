//! CLI coverage for the folder arguments: which folders a run reads, and how
//! each result names its file. These run the binary, because the default
//! folder is the working directory, which one test process shares.

mod common;

use std::path::Path;
use std::process::Command;

use common::{two_folders, write};
use tempfile::TempDir;

/// Run `mdsearch` from `cwd` and return its exit code, stdout, and stderr.
fn run(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mdsearch"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn mdsearch");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).expect("utf8 stdout"),
        String::from_utf8(out.stderr).expect("utf8 stderr"),
    )
}

/// The `path` of each result in a `--format json` run.
fn json_paths(stdout: &str) -> Vec<String> {
    let output: serde_json::Value = serde_json::from_str(stdout).expect("json stdout");
    output["results"]
        .as_array()
        .expect("results array")
        .iter()
        .map(|r| r["path"].as_str().expect("path string").to_string())
        .collect()
}

/// `docs` and `deploy` side by side, with a third folder no run names.
fn folders() -> TempDir {
    let tmp = two_folders();
    write(tmp.path(), "other/halt.md", "Halt recovery, unrelated.\n");
    tmp
}

#[test]
fn the_default_folder_reports_paths_without_a_dot_prefix() {
    let tmp = folders();
    let (code, stdout, stderr) = run(tmp.path(), &["recovery", "--format", "json"]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    let mut paths = json_paths(&stdout);
    paths.sort();
    assert_eq!(
        paths,
        vec!["deploy/README.md", "docs/sync/recovery.md", "other/halt.md"]
    );
}

#[test]
fn two_folders_rank_as_one_list() {
    let tmp = folders();
    // The form an agent reached for: a flag, the query, then two folders.
    let (code, stdout, stderr) = run(
        tmp.path(),
        &[
            "-l",
            "8",
            "halt recovery",
            "docs",
            "deploy",
            "--format",
            "json",
        ],
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
    let mut paths = json_paths(&stdout);
    paths.sort();
    assert_eq!(paths, vec!["deploy/README.md", "docs/sync/recovery.md"]);
}

#[test]
fn a_dot_slash_folder_reports_paths_without_the_prefix() {
    let tmp = folders();
    let (code, stdout, stderr) = run(tmp.path(), &["recovery", "./docs", "--format", "json"]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert_eq!(json_paths(&stdout), vec!["docs/sync/recovery.md"]);
}

#[test]
fn text_output_names_each_file_by_its_folder() {
    let tmp = folders();
    let (code, stdout, stderr) = run(tmp.path(), &["halt recovery", "docs", "deploy"]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(
        stdout.contains("] docs/sync/recovery.md ("),
        "got: {}",
        stdout
    );
    assert!(stdout.contains("] deploy/README.md ("), "got: {}", stdout);
}

#[test]
fn a_missing_second_folder_fails_naming_it() {
    let tmp = folders();
    let (code, stdout, stderr) = run(tmp.path(), &["recovery", "docs", "absent"]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty(), "got: {}", stdout);
    assert!(stderr.contains("not a folder: absent"), "got: {}", stderr);
}
