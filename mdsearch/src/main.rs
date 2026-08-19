//! `mdsearch` CLI — rank the Markdown files in a folder against a query.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use mdsearch::{TextJson, Walk};

#[derive(Parser)]
#[command(
    name = "mdsearch",
    version,
    about = "BM25 search over the Markdown files in a folder",
    long_about = "Rank the Markdown files in a folder against a query, best match first.\n\n\
Scoring is BM25 over three fields: the file name, the frontmatter `description:`, \
and the prose after that block. Terms are stemmed in English and Russian, so a query \
matches the words it shares a root with. Query punctuation is read as whitespace, so \
a phrase searches for its words.\n\n\
The walk obeys exclusion files — `.gitignore`, `.ignore`, and `.mdsearchignore` — \
in a plain folder as much as in a git repository, and skips dot-files. The index is \
built in RAM for the one run, so there is nothing to reindex after an edit."
)]
struct Cli {
    /// Query terms
    query: String,
    /// Folder to search (default: the current directory)
    path: Option<PathBuf>,
    /// Hits to report
    #[arg(short, long, default_value_t = mdsearch::DEFAULT_LIMIT)]
    limit: usize,
    /// Output format: text (default) or json
    #[arg(long, default_value = "text")]
    format: TextJson,
    /// Search dot-files and dot-folders too
    #[arg(long)]
    hidden: bool,
    /// Search excluded files too, obeying no exclusion file
    #[arg(long = "no-ignore")]
    no_ignore: bool,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<()> {
    let root = match &cli.path {
        Some(path) => path.clone(),
        None => std::env::current_dir()?,
    };
    let walk = Walk {
        ignore_files: !cli.no_ignore,
        hidden: cli.hidden,
    };
    mdsearch::run(&cli.query, &root, cli.limit, cli.format, walk)
}
