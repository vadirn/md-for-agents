//! Known-item evaluation for mdseek's ranking and verdict.
//!
//! Usage:
//!   knownitem vault <root>                 description queries: sections vs whole files, and feedback terms
//!   knownitem gate <root> <seed>           held-out description queries: one JSON record per query
//!   knownitem code <root> <queries.jsonl>  mined commit queries: split identifiers on and off
//!
//! Descriptions never enter the index here, so a description used as a query
//! cannot match itself. A file's rank is the rank of its best section.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use mdseek::rank::{Index, Options, Ranked};
use mdseek::sections::{self, File, Kind};
use serde_json::{Value, json};

const DEPTH: usize = 300;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("vault") => vault(Path::new(&args[2])),
        Some("gate") => gate(Path::new(&args[2]), args[3].parse()?),
        Some("code") => code(Path::new(&args[2]), Path::new(&args[3])),
        _ => bail!("usage: knownitem vault|gate|code ..."),
    }
}

fn blank() -> Options {
    Options {
        descriptions: false,
        ..Options::default()
    }
}

fn described(files: &[File]) -> Vec<(String, String)> {
    files
        .iter()
        .filter(|f| f.kind == Kind::Markdown)
        .filter_map(|f| {
            let d = mdsearch::frontmatter::description(&f.content);
            (!d.trim().is_empty()).then(|| (d, f.path.clone()))
        })
        .collect()
}

/// 1-based rank of `path` among distinct files, or 0 when absent.
fn file_rank(index: &Index, hits: &[Ranked], path: &str) -> usize {
    let mut seen = HashSet::new();
    for h in hits {
        let p = index.sections()[h.section].path.as_str();
        if seen.insert(p) && p == path {
            return seen.len();
        }
    }
    0
}

struct Tally {
    n: usize,
    rr: f64,
    hit1: usize,
    hit3: usize,
}

impl Tally {
    fn new() -> Tally {
        Tally {
            n: 0,
            rr: 0.0,
            hit1: 0,
            hit3: 0,
        }
    }
    fn add(&mut self, rank: usize) {
        self.n += 1;
        if rank > 0 {
            self.rr += 1.0 / rank as f64;
        }
        self.hit1 += (rank == 1) as usize;
        self.hit3 += (1..=3).contains(&rank) as usize;
    }
    fn line(&self, name: &str) -> String {
        let n = self.n.max(1) as f64;
        format!(
            "{name:28} n={} mrr={:.4} hit1={:.3} hit3={:.3}",
            self.n,
            self.rr / n,
            self.hit1 as f64 / n,
            self.hit3 as f64 / n
        )
    }
}

fn vault(root: &Path) -> Result<()> {
    let files = sections::walk(root)?;
    let queries = described(&files);
    let by_section = Index::build(sections::sections(&files), blank())?;
    let by_file = Index::build(sections::whole_files(&files), blank())?;
    let (mut s, mut f) = (Tally::new(), Tally::new());
    // Feedback: for a query whose target is not first, add the top 3 suggested terms.
    let (mut missed, mut improved, mut worsened, mut now_first) = (0, 0, 0, 0);
    for (query, path) in &queries {
        let hits = by_section.search(query, DEPTH)?.hits;
        let rank = file_rank(&by_section, &hits, path);
        s.add(rank);
        f.add(file_rank(
            &by_file,
            &by_file.search(query, DEPTH)?.hits,
            path,
        ));
        if rank != 1 {
            missed += 1;
            let terms = by_section.suggestions(query, &hits);
            let expanded = format!(
                "{} {}",
                query,
                terms.iter().take(3).cloned().collect::<Vec<_>>().join(" ")
            );
            let again = file_rank(
                &by_section,
                &by_section.search(&expanded, DEPTH)?.hits,
                path,
            );
            let better = |a: usize, b: usize| a > 0 && (b == 0 || a < b);
            improved += better(again, rank) as usize;
            worsened += better(rank, again) as usize;
            now_first += (again == 1) as usize;
        }
    }
    println!(
        "files={} sections={} queries={}",
        files.len(),
        by_section.sections().len(),
        queries.len()
    );
    println!("{}", s.line("sections, best per file"));
    println!("{}", f.line("whole files"));
    println!(
        "feedback on {missed} non-first queries: improved {improved}, worsened {worsened}, now first {now_first}"
    );
    Ok(())
}

/// Hold out every `seed`-th described note, so its query has no answer, and
/// record the verdict for held-out and present queries alike.
fn gate(root: &Path, seed: usize) -> Result<()> {
    let files = sections::walk(root)?;
    let queries = described(&files);
    let held: HashSet<String> = queries
        .iter()
        .enumerate()
        .filter(|(k, _)| k % seed == 0)
        .map(|(_, (_, p))| p.clone())
        .collect();
    let kept: Vec<File> = files
        .into_iter()
        .filter(|f| !held.contains(&f.path))
        .collect();
    let index = Index::build(sections::sections(&kept), blank())?;
    for (k, (query, path)) in queries.iter().enumerate() {
        let present = !held.contains(path);
        // As many present as held-out queries: the ones just after each held-out one.
        if present && k % seed != 1 {
            continue;
        }
        let outcome = index.search(query, 6)?;
        let top: Vec<Value> = outcome
            .hits
            .iter()
            .take(3)
            .map(|h| {
                let s = &index.sections()[h.section];
                json!({"path": s.path, "start": s.start, "end": s.end, "score": h.score})
            })
            .collect();
        println!(
            "{}",
            json!({"query": query, "target": path, "present": present, "answered": outcome.answered,
                   "reason": outcome.reason, "coverage": outcome.coverage, "elbow": outcome.elbow, "top": top})
        );
    }
    Ok(())
}

fn code(root: &Path, queries: &Path) -> Result<()> {
    let files = sections::walk(root)?;
    let text =
        std::fs::read_to_string(queries).with_context(|| format!("{}", queries.display()))?;
    let rows: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let title: f32 = std::env::var("TITLE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let scoring = mdsearch::Scoring {
        title,
        description: 0.5,
    };
    for (name, options) in [
        (
            "split identifiers",
            Options {
                scoring,
                ..Options::default()
            },
        ),
        (
            "unsplit",
            Options {
                split_identifiers: false,
                scoring,
                ..Options::default()
            },
        ),
    ] {
        let index = Index::build(sections::sections(&files), options)?;
        let (mut t, mut file_t) = (Tally::new(), Tally::new());
        for row in &rows {
            let (query, path) = (
                row["query"].as_str().unwrap_or(""),
                row["path"].as_str().unwrap_or(""),
            );
            let (start, end) = (
                row["start"].as_u64().unwrap_or(0) as usize,
                row["end"].as_u64().unwrap_or(0) as usize,
            );
            let hits = index.search(query, DEPTH)?.hits;
            let rank = hits
                .iter()
                .position(|h| {
                    let s = &index.sections()[h.section];
                    s.path == path
                        && s.start <= end
                        && start <= s.end
                        && s.end - s.start <= 2 * (end - start) + 20
                })
                .map_or(0, |p| p + 1);
            t.add(rank);
            file_t.add(file_rank(&index, &hits, path));
        }
        println!("{}", t.line(&format!("{name}, section")));
        println!("{}", file_t.line(&format!("{name}, file")));
    }
    Ok(())
}
