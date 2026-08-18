//! CLI coverage for the read-only verbs and flags.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mdformat"))
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
    let dir = std::env::temp_dir().join(format!("mdformat-surface-{}-{name}", std::process::id()));
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

/// One departure for `gaps` (a doubled blank line) and two for `tables` (the
/// delimiter row and the short body row), so a single-rule run is visible as
/// the *other* rule's departure surviving.
const BOTH: &[u8] = b"# H\n\n\n| key | value |\n| --- | --- |\n| a | longer |\n";

/// What each rule alone makes of `BOTH`, and what all four make of it.
const GAPS_ONLY: &str = "# H\n\n| key | value |\n| --- | --- |\n| a | longer |\n";
const TABLES_ONLY: &str = "# H\n\n\n| key | value |\n| --- | ----- |\n| a   | longer |\n";
const FORMATTED: &str = "# H\n\n| key | value |\n| --- | ----- |\n| a   | longer |\n";

#[test]
fn normalize_and_pad_are_no_longer_verbs() {
    for verb in ["normalize", "pad"] {
        let dir = scratch(verb);
        let p = file(&dir, "note.md", BOTH);
        let (code, stdout, stderr) = run(&[verb, &s(&p)]);
        assert_eq!(code, 2, "{verb}: {stderr}");
        assert_eq!(stdout, "", "{verb} must print no bytes");
        assert!(
            stderr.contains("unrecognized subcommand"),
            "{verb}: {stderr}"
        );
        assert_eq!(fs::read(&p).expect("read back"), BOTH);
    }
}

#[test]
fn the_help_lists_two_verbs() {
    let (code, stdout, stderr) = run(&["--help"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("format"), "{stdout}");
    assert!(stdout.contains("partition"), "{stdout}");
    assert!(!stdout.contains("normalize"), "{stdout}");
    assert!(
        !stdout.contains("\n  pad"),
        "no `pad` verb may be listed: {stdout}"
    );
}

#[test]
fn rule_emits_one_rules_output() {
    let dir = scratch("emit");
    let p = file(&dir, "note.md", BOTH);

    let (code, stdout, stderr) = run(&["format", "--rule", "gaps", &s(&p)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, GAPS_ONLY);

    let (code, stdout, stderr) = run(&["format", "--rule", "tables", &s(&p)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, TABLES_ONLY);

    let (code, stdout, stderr) = run(&["format", &s(&p)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, FORMATTED);

    assert_eq!(fs::read(&p).expect("read back"), BOTH, "nothing is written");
}

#[test]
fn every_departure_tag_is_a_rule_name() {
    let dir = scratch("tags");
    let p = file(
        &dir,
        "note.md",
        b"# H\r\n\r\n\r\n| a | bb |\r\n| - | - |\r\n| 1 | 2 |\r\n",
    );

    let (code, _, stderr) = run(&["format", "--check", &s(&p)]);
    assert_eq!(code, 4, "the specimen must depart: {stderr}");

    // `mdformat: <path>:L<l>:<c>: <rule>: <what>` — the tag is the field after
    // the position, and the position is what tells a departure line apart from
    // the `NOT NORMAL` and summary lines around it.
    let mut tags: Vec<String> = stderr
        .lines()
        .filter_map(|l| {
            let mut fields = l.split(": ");
            let located = fields.nth(1)?.contains(":L");
            located.then(|| fields.next()).flatten()
        })
        .map(str::to_string)
        .collect();
    tags.sort();
    tags.dedup();
    assert!(
        tags.iter().any(|t| t == "endings"),
        "the CRLF specimen must produce an `endings` tag: {stderr}"
    );
    assert!(tags.len() >= 2, "{tags:?}");

    for tag in &tags {
        let (code, _, stderr) = run(&["format", "--check", "--rule", tag, &s(&p)]);
        assert_eq!(
            code, 4,
            "--rule {tag} must be accepted and depart: {stderr}"
        );
    }

    // And the rules the specimen satisfies are selectable too, reporting no
    // departure rather than an unknown-name refusal.
    for name in ["endings", "gaps", "tables", "markers"] {
        let (code, _, stderr) = run(&["format", "--check", "--rule", name, "-"]);
        assert_eq!(code, 0, "--rule {name} on empty stdin: {stderr}");
    }
}

#[test]
fn an_unknown_rule_name_is_refused() {
    let dir = scratch("unknown");
    let p = file(&dir, "note.md", BOTH);

    let (code, stdout, stderr) = run(&["format", "--rule", "normalize", &s(&p)]);

    assert_eq!(code, 2, "{stderr}");
    assert_eq!(stdout, "", "a refused invocation prints no bytes");
    assert!(stderr.contains("no rule is named"), "{stderr}");
    assert!(
        stderr.contains("endings, gaps, tables, markers"),
        "the refusal must list the names: {stderr}"
    );
}

#[test]
fn rule_and_write_together_are_refused() {
    let dir = scratch("rule-write");
    let p = file(&dir, "note.md", BOTH);

    let (code, stdout, stderr) = run(&["format", "--write", "--rule", "gaps", &s(&p)]);

    assert_eq!(code, 2, "{stderr}");
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("--write rewrites a file to normal form"),
        "{stderr}"
    );
    assert_eq!(fs::read(&p).expect("read back"), BOTH, "nothing is written");
}

#[test]
fn check_rule_reports_one_rules_departures() {
    let dir = scratch("check");
    let p = file(&dir, "note.md", BOTH);

    let (code, _, all) = run(&["format", "--check", &s(&p)]);
    assert_eq!(code, 4, "{all}");
    assert!(all.contains("NOT NORMAL (3 departures)"), "{all}");

    let (code, stdout, gaps) = run(&["format", "--check", "--rule", "gaps", &s(&p)]);
    assert_eq!(code, 4, "{gaps}");
    assert_eq!(stdout, "", "--check prints no bytes");
    assert!(gaps.contains("NOT NORMAL (1 departures)"), "{gaps}");
    assert!(gaps.contains(": gaps: "), "{gaps}");
    assert!(!gaps.contains(": tables: "), "{gaps}");
    assert!(
        gaps.contains("mdformat format --check --rule gaps: 0/1 files are in normal form"),
        "the summary must name the restriction: {gaps}"
    );

    let (code, _, tables) = run(&["format", "--check", "--rule", "tables", &s(&p)]);
    assert_eq!(code, 4, "{tables}");
    assert!(tables.contains("NOT NORMAL (2 departures)"), "{tables}");
    assert!(!tables.contains(": gaps: "), "{tables}");

    // A rule the document satisfies calls it normal on its own, and exits 0 —
    // so `--rule` narrows the verdict as well as the report.
    let (code, _, markers) = run(&["format", "--check", "--rule", "markers", &s(&p)]);
    assert_eq!(code, 0, "{markers}");
    assert!(
        markers.contains("--rule markers: 1/1 files are in normal form"),
        "{markers}"
    );
}

#[test]
fn the_unrestricted_summary_line_is_unchanged() {
    let dir = scratch("summary");
    let p = file(&dir, "note.md", BOTH);

    let (_, _, check) = run(&["format", "--check", &s(&p)]);
    assert!(
        check.contains(
            "mdformat format --check: 0/1 files are in normal form \
             (3 departures, 0 rule declinations, 0 exempt constructs)"
        ),
        "{check}"
    );

    let (_, _, fmt) = run(&["format", &s(&p)]);
    assert!(
        fmt.contains(
            "mdformat format: 1/1 files changed (0 rule declinations, 0 exempt constructs)"
        ),
        "{fmt}"
    );
}

#[test]
fn exemptions_are_reported_without_being_asked() {
    let dir = scratch("exempt");
    let p = file(&dir, "note.md", b"# Title\n\n- alpha\n\n* beta\n");

    let (code, stdout, stderr) = run(&["format", &s(&p)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "# Title\n\n- alpha\n\n* beta\n", "nothing changes");
    assert_eq!(
        stderr.lines().filter(|l| l.contains("EXEMPT")).count(),
        2,
        "both declined lists must be named: {stderr}"
    );
    assert!(stderr.contains("EXEMPT: L3: markers"), "{stderr}");
    assert!(stderr.contains("EXEMPT: L5: markers"), "{stderr}");
    assert!(stderr.contains("2 exempt constructs"), "{stderr}");

    // Under `--check` too, where the document is *normal* — a declined
    // construct produces no departure, so the exemption is the only trace of
    // it, and it used to be the trace `--verbose` hid.
    let (code, _, stderr) = run(&["format", "--check", &s(&p)]);
    assert_eq!(code, 0, "a declined construct is not a failure: {stderr}");
    assert_eq!(
        stderr.lines().filter(|l| l.contains("EXEMPT")).count(),
        2,
        "{stderr}"
    );
    assert!(stderr.contains("2 exempt constructs"), "{stderr}");

    // And under `--rule markers`, which is the whole of what a `markers` dry
    // run would have printed.
    let (code, _, stderr) = run(&["format", "--check", "--rule", "markers", &s(&p)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stderr.lines().filter(|l| l.contains("EXEMPT")).count(),
        2,
        "{stderr}"
    );
}

#[test]
fn a_declined_document_is_reported_and_is_not_a_failure() {
    let dir = scratch("declined");
    let p = file(&dir, "note.md", b"\n\n---\nk: v\n---\n");

    let (code, stdout, stderr) = run(&["format", "--rule", "gaps", &s(&p)]);

    assert_eq!(code, 0, "a declination sets no exit code: {stderr}");
    assert_eq!(stdout, "\n\n---\nk: v\n---\n", "the input passes through");
    assert!(
        stderr.contains("the gaps rule declined this document"),
        "{stderr}"
    );
    assert!(stderr.contains("1 rule declinations"), "{stderr}");

    // `--check` agrees the document is normal, because the rule left it alone.
    let (code, _, stderr) = run(&["format", "--check", "--rule", "gaps", &s(&p)]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("1/1 files are in normal form"), "{stderr}");
    assert!(
        stderr.contains("the gaps rule declined this document"),
        "{stderr}"
    );
}

#[test]
fn rule_still_takes_one_input_when_printing_bytes() {
    let dir = scratch("two");
    let a = file(&dir, "a.md", BOTH);
    let b = file(&dir, "b.md", BOTH);

    let (code, stdout, stderr) = run(&["format", "--rule", "gaps", &s(&a), &s(&b)]);
    assert_eq!(code, 2, "{stderr}");
    assert_eq!(stdout, "");
    assert!(stderr.contains("takes exactly one input"), "{stderr}");

    // `--check` reads a list, restricted or not.
    let (code, _, stderr) = run(&["format", "--check", "--rule", "gaps", &s(&a), &s(&b)]);
    assert_eq!(code, 4, "{stderr}");
    assert!(stderr.contains("0/2 files are in normal form"), "{stderr}");
}
