//! End-to-end search over a temporary folder: ranking, exclusion, and output.

use std::fs;
use std::path::Path;

use mdsearch::{TextJson, Walk};
use tempfile::TempDir;

/// Result count these tests ask for; the binary has its own default.
const DEFAULT_LIMIT: usize = 10;

fn write(dir: &Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// A folder holding one file about retrieval and one about gardening.
fn corpus() -> TempDir {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "notes/Retrieval.md",
        "---\ntype: card\ndescription: ranking documents by term frequency\n---\n\
         BM25 scores a document against a query by term frequency.\n",
    );
    write(
        tmp.path(),
        "notes/Gardening.md",
        "Tomatoes want six hours of sun and a deep weekly watering.\n",
    );
    tmp
}

/// Each hit's path under `root`. A hit reports the root too, so the expected
/// paths below stay short.
fn paths(root: &Path, hits: &[mdsearch::SearchResult]) -> Vec<String> {
    hits.iter()
        .map(|h| {
            let path = Path::new(&h.path);
            let under = path
                .strip_prefix(root)
                .unwrap_or_else(|_| panic!("{} is not under {}", path.display(), root.display()));
            under.to_string_lossy().to_string()
        })
        .collect()
}

#[test]
fn ranks_the_matching_file_first() {
    let tmp = corpus();
    let hits = mdsearch::search(
        "term frequency",
        &[tmp.path()],
        DEFAULT_LIMIT,
        Walk::default(),
    )
    .unwrap();
    assert_eq!(paths(tmp.path(), &hits), vec!["notes/Retrieval.md"]);
    assert!(hits[0].score > 0.0);
}

#[test]
fn a_query_with_no_match_returns_nothing() {
    let tmp = corpus();
    let hits =
        mdsearch::search("submarine", &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap();
    assert!(hits.is_empty(), "got: {:?}", paths(tmp.path(), &hits));
}

#[test]
fn stemming_matches_an_inflected_query() {
    let tmp = corpus();
    // "watering" in the file, "watered" in the query: one stem, one match.
    let hits = mdsearch::search("watered", &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap();
    assert_eq!(paths(tmp.path(), &hits), vec!["notes/Gardening.md"]);
}

#[test]
fn query_punctuation_reads_as_whitespace() {
    let tmp = corpus();
    // Left alone, `title:` would parse as a field name and match nothing.
    let hits = mdsearch::search(
        "title: term frequency",
        &[tmp.path()],
        DEFAULT_LIMIT,
        Walk::default(),
    )
    .unwrap();
    assert_eq!(paths(tmp.path(), &hits), vec!["notes/Retrieval.md"]);
}

#[test]
fn a_query_of_only_punctuation_is_an_error() {
    let tmp = corpus();
    let err = mdsearch::search("***", &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap_err();
    assert!(
        err.to_string().contains("no searchable terms"),
        "got: {}",
        err
    );
}

#[test]
fn the_file_name_is_searchable() {
    let tmp = corpus();
    let hits =
        mdsearch::search("gardening", &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap();
    assert_eq!(paths(tmp.path(), &hits), vec!["notes/Gardening.md"]);
    assert_eq!(hits[0].title, "Gardening");
}

#[test]
fn the_frontmatter_description_is_searchable_and_the_rest_is_not() {
    let tmp = corpus();
    // `description:` is indexed.
    let described =
        mdsearch::search("ranking", &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap();
    assert_eq!(paths(tmp.path(), &described), vec!["notes/Retrieval.md"]);
    // `type: card` is not: no other frontmatter field reaches the index.
    let typed = mdsearch::search("card", &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap();
    assert!(typed.is_empty(), "got: {:?}", paths(tmp.path(), &typed));
}

#[test]
fn the_snippet_windows_the_matching_body() {
    let tmp = corpus();
    let hits = mdsearch::search("tomatoes", &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap();
    assert!(
        hits[0].snippet.to_lowercase().contains("tomatoes"),
        "got: {:?}",
        hits[0].snippet
    );
    // The window is plain text: the highlight markup belongs to the terminal.
    assert!(!hits[0].snippet.contains('<'), "got: {:?}", hits[0].snippet);
}

#[test]
fn tokens_estimate_the_body_without_the_frontmatter() {
    let tmp = TempDir::new().unwrap();
    let body = "alpha ".repeat(100);
    write(
        tmp.path(),
        "note.md",
        &format!("---\ndescription: {}\n---\n{}", "x".repeat(400), body),
    );
    let hits = mdsearch::search("alpha", &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap();
    // The body is 600 chars; counting the 400-char frontmatter would double this.
    assert_eq!(hits[0].tokens, 150);
}

#[test]
fn limit_truncates_the_ranking() {
    let tmp = TempDir::new().unwrap();
    for i in 0..5 {
        write(tmp.path(), &format!("note{}.md", i), "alpha beta gamma\n");
    }
    let hits = mdsearch::search("alpha", &[tmp.path()], 2, Walk::default()).unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn an_excluded_folder_stays_out_of_the_results() {
    let tmp = corpus();
    write(tmp.path(), ".gitignore", "vendor/\n");
    write(tmp.path(), "vendor/Copy.md", "BM25 term frequency again.\n");
    let hits = mdsearch::search(
        "term frequency",
        &[tmp.path()],
        DEFAULT_LIMIT,
        Walk::default(),
    )
    .unwrap();
    assert_eq!(paths(tmp.path(), &hits), vec!["notes/Retrieval.md"]);
}

#[test]
fn a_custom_ignore_file_excludes_what_git_keeps() {
    let tmp = corpus();
    write(tmp.path(), ".customignore", "notes/Gardening.md\n");
    let walk = Walk {
        custom_ignore: Some(".customignore".into()),
        ..Walk::default()
    };
    let hits = mdsearch::search("tomatoes", &[tmp.path()], DEFAULT_LIMIT, walk).unwrap();
    assert!(hits.is_empty(), "got: {:?}", paths(tmp.path(), &hits));
}

#[test]
fn no_ignore_searches_the_excluded_files() {
    let tmp = corpus();
    write(tmp.path(), ".gitignore", "vendor/\n");
    write(tmp.path(), "vendor/Copy.md", "BM25 term frequency again.\n");
    let walk = Walk {
        ignore_files: false,
        ..Walk::default()
    };
    let hits = mdsearch::search("term frequency", &[tmp.path()], DEFAULT_LIMIT, walk).unwrap();
    assert_eq!(hits.len(), 2, "got: {:?}", paths(tmp.path(), &hits));
}

#[test]
fn a_missing_folder_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let err = mdsearch::search(
        "alpha",
        &[tmp.path().join("absent")],
        DEFAULT_LIMIT,
        Walk::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("not a folder"), "got: {}", err);
}

/// Two folders whose parent holds nothing else: `docs` and `deploy`.
fn two_folders() -> TempDir {
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

fn scores(hits: &[mdsearch::SearchResult]) -> Vec<(String, f32)> {
    hits.iter().map(|h| (h.path.clone(), h.score)).collect()
}

#[test]
fn two_roots_score_as_their_parent_does() {
    let tmp = two_folders();
    let query = "halt sender clear recovery";
    let roots = [tmp.path().join("docs"), tmp.path().join("deploy")];
    let both = mdsearch::search(query, &roots, DEFAULT_LIMIT, Walk::default()).unwrap();
    let parent = mdsearch::search(query, &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap();
    // One file set, one index: every path and every score agrees exactly.
    assert_eq!(scores(&both), scores(&parent));
    assert_eq!(
        paths(tmp.path(), &both),
        vec![
            "docs/sync/recovery.md",
            "deploy/README.md",
            "deploy/notes.md"
        ]
    );
}

#[test]
fn a_folder_searched_alone_scores_differently() {
    let tmp = two_folders();
    let query = "halt sender clear recovery";
    let deploy = tmp.path().join("deploy");
    let readme = |hits: &[mdsearch::SearchResult]| {
        hits.iter()
            .find(|h| h.path.ends_with("README.md"))
            .unwrap()
            .score
    };
    let roots = [tmp.path().join("docs"), deploy.clone()];
    let both = mdsearch::search(query, &roots, DEFAULT_LIMIT, Walk::default()).unwrap();
    let alone = mdsearch::search(query, &[deploy], DEFAULT_LIMIT, Walk::default()).unwrap();
    // Alone, the small folder makes each query word look common, so the same
    // file scores lower. Merging two such runs would rank on unequal scales.
    assert!(
        readme(&alone) < readme(&both),
        "alone {} vs both {}",
        readme(&alone),
        readme(&both)
    );
}

#[test]
fn an_empty_folder_returns_nothing() {
    let tmp = TempDir::new().unwrap();
    let hits = mdsearch::search("alpha", &[tmp.path()], DEFAULT_LIMIT, Walk::default()).unwrap();
    assert!(hits.is_empty());
}

#[test]
fn json_and_text_runs_both_succeed() {
    let tmp = corpus();
    for format in [TextJson::Json, TextJson::Text] {
        mdsearch::run(
            "term frequency",
            &[tmp.path()],
            DEFAULT_LIMIT,
            format,
            Walk::default(),
        )
        .unwrap();
    }
}

/// What another crate reuses: its own walk, its own filter, its own weights, over
/// the same core. Nothing here goes through the CLI half.
#[test]
fn the_core_indexes_documents_a_caller_supplies() {
    use mdsearch::{Corpus, Doc, Scoring};

    let tmp = corpus();
    let files = mdsearch::scan(&[tmp.path()], Walk::default()).unwrap();

    // A caller drops what its own rules exclude, before anything is indexed.
    let docs: Vec<Doc> = files
        .iter()
        .filter(|f| f.path.contains("Retrieval"))
        .map(|f| f.to_doc())
        .collect();
    assert_eq!(docs.len(), 1);

    let corpus = Corpus::build(docs).unwrap();
    let hits = corpus
        .search(
            "term frequency",
            DEFAULT_LIMIT,
            Scoring {
                title: 2.0,
                description: 0.5,
            },
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].id,
        tmp.path().join("notes/Retrieval.md").to_string_lossy()
    );
    assert!(!hits[0].snippet.highlights.is_empty());

    // The corpus reports its size without a search.
    assert_eq!(corpus.num_docs(), 1);
}

/// Documents need not come from disk at all.
#[test]
fn the_core_needs_no_files() {
    use mdsearch::{Corpus, Doc, Scoring};

    let docs = vec![Doc {
        id: "row-42".into(),
        title: "In memory".into(),
        description: String::new(),
        body: "A document assembled in memory, never written to disk.".into(),
    }];
    let hits = Corpus::build(docs)
        .unwrap()
        .search("assembled", DEFAULT_LIMIT, Scoring::default())
        .unwrap();
    assert_eq!(hits[0].id, "row-42");
}
