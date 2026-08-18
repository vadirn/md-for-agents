//! CLI coverage for `format --write`, the crate's only file-writing path.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mdformat"))
        .arg("format")
        .args(args)
        .output()
        .expect("spawn mdformat");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).expect("utf8 stdout"),
        String::from_utf8(out.stderr).expect("utf8 stderr"),
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdformat-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, bytes).expect("write fixture");
    p
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A document that (a) changes — its table is unpadded — and (b) holds two
/// constructs the marker rule declines, because unifying the two adjacent
/// bullets would merge them into one list.
const DECLINING: &[u8] = b"# Title\n\n- alpha\n\n* beta\n\n| x | yy |\n| - | - |\n| 1 | 2 |\n";

/// A UTF-8 byte order mark followed immediately by a multi-row table, unpadded.
const MARKED: &[u8] = b"\xef\xbb\xbf| a | b |\n| --- | --- |\n| 1 | 2 |\n";

/// A three-space-indented table whose last row is a lazy continuation carrying
/// no indent.
const LAZY: &[u8] = b"   |a|b|\n   |-|-|\n   |1|2|\npara\n";

/// A document `format` returns `Err` on. Three conditions hold at once, and
/// removing any one of them makes it succeed:
///
/// 1. a row supplies **fewer cells than the header**, so comrak autocompletes
///    the missing one;
/// 2. that row **does not end in a pipe**, so the autocompleted cell is placed
///    on the delimiter that would have followed the row's last cell rather than
///    on a delimiter that is there;
/// 3. that row is the file's **last line and the file has no line ending**, so
///    the byte the cell was placed on does not exist.
const ERRS: &[u8] = b"| a | b |\n| --- | --- |\n| 1 | 2 |\npara";

#[test]
fn one_named_file_is_rewritten_in_place() {
    let dir = scratch("happy");
    let p = file(&dir, "note.md", DECLINING);

    let (code, stdout, stderr) = run(&["--write", &s(&p)]);

    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "", "--write puts bytes in the file, not on stdout");
    let after = fs::read(&p).expect("read back");
    assert_ne!(after, DECLINING, "the file must have changed");
    assert!(
        String::from_utf8(after.clone())
            .expect("utf8")
            .contains("| x   | yy |"),
        "the table must be padded: {}",
        String::from_utf8_lossy(&after)
    );
    assert!(stderr.contains("rewritten in place"), "{stderr}");
    assert!(stderr.contains("1/1 files rewritten"), "{stderr}");
}

#[test]
fn what_is_written_is_in_normal_form() {
    let dir = scratch("normal");
    let p = file(&dir, "note.md", DECLINING);

    assert_eq!(run(&["--write", &s(&p)]).0, 0);
    let (code, _, stderr) = run(&["--check", &s(&p)]);
    assert_eq!(code, 0, "the rewritten file must be normal: {stderr}");
}

#[test]
fn every_declination_is_reported_without_being_asked() {
    let dir = scratch("report");
    let p = file(&dir, "note.md", DECLINING);

    let (code, _, stderr) = run(&["--write", &s(&p)]);

    assert_eq!(code, 0, "{stderr}");
    let exempt: Vec<&str> = stderr.lines().filter(|l| l.contains("EXEMPT")).collect();
    assert_eq!(
        exempt.len(),
        2,
        "both declined lists must be named: {stderr}"
    );
    assert!(exempt.iter().all(|l| l.contains("markers")), "{stderr}");
    assert!(exempt.iter().any(|l| l.contains("L3")), "{stderr}");
    assert!(exempt.iter().any(|l| l.contains("L5")), "{stderr}");
    assert!(
        exempt.iter().all(|l| l.contains("would merge them")),
        "each exemption must carry its reason: {stderr}"
    );
    assert!(stderr.contains("2 exempt constructs"), "{stderr}");
}

#[test]
fn an_exemption_names_a_line_in_the_file_as_written() {
    let dir = scratch("coords");
    let p = file(&dir, "note.md", b"# Title\n\n\n\n- alpha\n\n* beta\n");

    let (code, _, stderr) = run(&["--write", &s(&p)]);

    assert_eq!(code, 0, "{stderr}");
    let after = String::from_utf8(fs::read(&p).expect("read back")).expect("utf8");
    let lines: Vec<&str> = after.lines().collect();
    assert_eq!(lines[2], "- alpha", "{after:?}");
    assert_eq!(lines[4], "* beta", "{after:?}");
    assert!(stderr.contains("EXEMPT: L3: markers"), "{stderr}");
    assert!(stderr.contains("EXEMPT: L5: markers"), "{stderr}");
}

#[test]
fn verbose_is_gone_from_the_surface() {
    let dir = scratch("verbose");
    let p = file(&dir, "note.md", DECLINING);

    let (code, stdout, stderr) = run(&["--write", "--verbose", &s(&p)]);

    assert_eq!(code, 2, "{stderr}");
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("unexpected argument '--verbose'"),
        "{stderr}"
    );
    assert_eq!(
        fs::read(&p).expect("read back"),
        DECLINING,
        "a refused invocation writes nothing"
    );
}

#[test]
fn two_paths_are_refused_and_neither_file_is_touched() {
    let dir = scratch("two");
    let a = file(&dir, "a.md", DECLINING);
    let b = file(&dir, "b.md", DECLINING);

    let (code, stdout, stderr) = run(&["--write", &s(&a), &s(&b)]);

    assert_eq!(code, 2, "{stderr}");
    assert_eq!(stdout, "");
    assert!(stderr.contains("exactly one file path"), "{stderr}");
    assert!(
        stderr.contains("separate tier this binary does not implement"),
        "the refusal must name the gate: {stderr}"
    );
    assert_eq!(fs::read(&a).expect("read a"), DECLINING);
    assert_eq!(fs::read(&b).expect("read b"), DECLINING);
}

#[test]
fn a_directory_is_refused_and_its_contents_are_not_walked() {
    let dir = scratch("dir");
    let inside = file(&dir, "inside.md", DECLINING);

    let (code, stdout, stderr) = run(&["--write", &s(&dir)]);

    assert_eq!(code, 2, "{stderr}");
    assert_eq!(stdout, "");
    assert!(stderr.contains("one regular file"), "{stderr}");
    assert!(
        stderr.contains("separate tier this binary does not implement"),
        "{stderr}"
    );
    assert_eq!(fs::read(&inside).expect("read inside"), DECLINING);
}

#[test]
fn a_bare_write_is_refused_rather_than_reading_stdin() {
    let (code, stdout, stderr) = run(&["--write"]);
    assert_eq!(code, 2, "{stderr}");
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("exactly one file path and got 0"),
        "{stderr}"
    );
}

#[test]
fn stdin_is_refused_because_it_names_no_file() {
    let (code, _, stderr) = run(&["--write", "-"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("no stdin input"), "{stderr}");
}

#[test]
fn write_and_check_together_are_refused() {
    let dir = scratch("both");
    let p = file(&dir, "note.md", DECLINING);

    let (code, _, stderr) = run(&["--write", "--check", &s(&p)]);

    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("give one or the other"), "{stderr}");
    assert_eq!(fs::read(&p).expect("read back"), DECLINING);
}

#[test]
fn a_missing_file_is_refused_before_anything_is_read() {
    let dir = scratch("missing");
    let (code, _, stderr) = run(&["--write", &s(&dir.join("absent.md"))]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("--write cannot read"), "{stderr}");
}

#[test]
fn an_erroring_document_is_left_alone() {
    let dir = scratch("err");
    let p = file(&dir, "note.md", ERRS);
    let before = fs::metadata(&p).expect("stat").modified().expect("mtime");

    let (code, stdout, stderr) = run(&["--write", &s(&p)]);

    assert_ne!(code, 0, "an erroring document must exit non-zero");
    assert_eq!(code, 5, "{stderr}");
    assert_eq!(stdout, "");
    assert!(stderr.contains("SOURCEPOS ERROR"), "{stderr}");
    assert!(stderr.contains("NOT REWRITTEN"), "{stderr}");
    assert_eq!(fs::read(&p).expect("read back"), ERRS, "bytes must survive");
    assert_eq!(
        fs::metadata(&p).expect("stat").modified().expect("mtime"),
        before,
        "an untouched file must keep its mtime"
    );
}

#[test]
fn the_erroring_specimen_needs_all_three_conditions() {
    let dir = scratch("conditions");
    // (name, document, expected exit code). Each control drops exactly one
    // condition from `ERRS` and must format.
    let cases: &[(&str, &[u8], i32)] = &[
        ("the specimen", ERRS, 5),
        // 3 dropped: the file ends in a line ending, which is a byte the
        // autocompleted cell can land on.
        (
            "with a final line ending",
            b"| a | b |\n| --- | --- |\n| 1 | 2 |\npara\n",
            0,
        ),
        // 1 dropped: a square row needs no autocompleted cell at all.
        (
            "with a square last row",
            b"| a | b |\n| --- | --- |\n| 1 | 2 |\n| p | q |",
            0,
        ),
        // 2 dropped: still one cell short, but the row's closing pipe is a byte
        // the autocompleted cell can be placed on.
        (
            "with a closing pipe on the short row",
            b"| a | b |\n| --- | --- |\n| 1 | 2 |\n| p |",
            0,
        ),
    ];
    for (name, doc, want) in cases {
        let p = file(&dir, &format!("{}.md", name.replace(' ', "-")), doc);
        let (code, _, stderr) = run(&[&s(&p)]);
        assert_eq!(code, *want, "{name}: {stderr}");
        assert_eq!(
            stderr.contains("SOURCEPOS ERROR"),
            *want == 5,
            "{name}: {stderr}"
        );
    }
}

#[test]
fn a_byte_order_marked_table_is_rewritten_in_place() {
    let dir = scratch("marked");
    let p = file(&dir, "note.md", MARKED);

    let (code, stdout, stderr) = run(&["--write", &s(&p)]);

    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");
    assert!(
        !stderr.contains("SOURCEPOS ERROR"),
        "the mark must no longer defeat the sourcepos conversion: {stderr}"
    );
    assert_eq!(
        fs::read(&p).expect("read back"),
        b"\xef\xbb\xbf| a   | b |\n| --- | --- |\n| 1   | 2 |\n",
        "the table must be padded and the mark must survive as the first bytes"
    );
    assert!(stderr.contains("1/1 files rewritten"), "{stderr}");

    // And the rewrite is a fixpoint, mark and all.
    let (code, _, stderr) = run(&["--check", &s(&p)]);
    assert_eq!(code, 0, "the rewritten file must be normal: {stderr}");
}

#[test]
fn an_indented_table_with_a_lazy_row_is_reported_rather_than_refused() {
    let dir = scratch("lazy");
    let p = file(&dir, "note.md", LAZY);

    let (code, stdout, stderr) = run(&["--write", &s(&p)]);

    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");
    assert!(
        !stderr.contains("SOURCEPOS ERROR"),
        "the omitted indent must no longer defeat the sourcepos conversion: {stderr}"
    );
    assert!(stderr.contains("EXEMPT"), "{stderr}");
    assert!(stderr.contains("tables"), "{stderr}");
    assert_eq!(
        fs::read(&p).expect("read back"),
        LAZY,
        "an exempt table leaves the document byte-identical"
    );
    assert!(stderr.contains("0/1 files rewritten"), "{stderr}");

    // The second of the three verbs the refusal used to reach. The third,
    // `partition`, is covered by the partition fixtures's `table-indented-lazy-row`
    // fixture, which this file's `run` helper cannot reach — it prepends
    // `format` to every invocation on purpose.
    let (code, _, stderr) = run(&["--check", &s(&p)]);
    assert_eq!(code, 0, "--check must agree it is normal: {stderr}");
}

#[test]
fn an_already_normal_document_is_not_rewritten() {
    let dir = scratch("noop");
    let normal = b"# Title\n\nA paragraph.\n\n- alpha\n- beta\n";
    let p = file(&dir, "note.md", normal);
    let before = fs::metadata(&p).expect("stat").modified().expect("mtime");

    let (code, stdout, stderr) = run(&["--write", &s(&p)]);

    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");
    assert!(stderr.contains("already in normal form"), "{stderr}");
    assert!(stderr.contains("0/1 files rewritten"), "{stderr}");
    assert_eq!(fs::read(&p).expect("read back"), normal);
    assert_eq!(
        fs::metadata(&p).expect("stat").modified().expect("mtime"),
        before,
        "an unwritten file must keep its mtime"
    );
}

#[test]
fn the_replacement_is_atomic_end_to_end() {
    use std::os::unix::fs::MetadataExt;
    let dir = scratch("atomic");
    let p = file(&dir, "note.md", DECLINING);
    let before = fs::metadata(&p).expect("stat").ino();

    let (code, _, stderr) = run(&["--write", &s(&p)]);

    assert_eq!(code, 0, "{stderr}");
    assert_ne!(
        fs::metadata(&p).expect("stat").ino(),
        before,
        "the rewritten name must point at a new inode"
    );
    let entries: Vec<_> = fs::read_dir(&dir)
        .expect("read dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["note.md".to_string()], "{entries:?}");
}

#[test]
fn the_permission_bits_survive_the_cli_write() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("perms");
    let p = file(&dir, "note.md", DECLINING);
    fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).expect("chmod");

    let (code, _, stderr) = run(&["--write", &s(&p)]);

    assert_eq!(code, 0, "{stderr}");
    let mode = fs::metadata(&p).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o640, "got {mode:o}");
}

#[test]
fn a_second_write_finds_nothing_to_do() {
    let dir = scratch("twice");
    let p = file(&dir, "note.md", DECLINING);

    assert_eq!(run(&["--write", &s(&p)]).0, 0);
    let once = fs::read(&p).expect("read back");

    let (code, _, stderr) = run(&["--write", &s(&p)]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("already in normal form"), "{stderr}");
    assert_eq!(fs::read(&p).expect("read back"), once);
}

#[test]
fn the_default_format_verb_still_writes_no_file() {
    let dir = scratch("stdout");
    let p = file(&dir, "note.md", DECLINING);

    let (code, stdout, stderr) = run(&[&s(&p)]);

    assert_eq!(code, 0, "{stderr}");
    assert!(!stdout.is_empty(), "the formatted bytes go to stdout");
    assert_eq!(fs::read(&p).expect("read back"), DECLINING);
}
