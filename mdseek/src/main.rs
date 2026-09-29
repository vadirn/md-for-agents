//! `mdseek` — answer an agent's question about a folder of Markdown and code.
//!
//! `mdseek <folder> <question>` ranks outline sections with BM25 and prints
//! `answered` or `no-answer`, then the candidates with their line ranges.
//! `mdseek read <file> [address]` folds a file to its outline or unfolds one
//! part. `mdseek usage` prints the text an agent's prompt carries.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use mdseek::{rank, read, render, sections};

/// Candidates one search prints unless `--limit` says otherwise.
const DEFAULT_LIMIT: usize = 6;

/// What an agent's prompt says about the tool. It names the wrapper as
/// `<seek>`, because the prompt gives its path.
const USAGE: &str = "\
Start with `<seek> <folder> \"<question>\"`, passing the question as you were given it. It ranks every definition and note section in the folder by BM25.

It prints `answered` or `no-answer`, then candidates as `<path>:<first>-<last>`, the best two with their text. Each range is the whole definition or section, so cite it as printed; you need not open the file. On `no-answer`, search once more with a few suggested terms added, then fall back to rg.

`<seek> read <file> [<first>-<last>]` prints a file's outline, or those lines.";

#[derive(Parser)]
#[command(
    name = "mdseek",
    version,
    about = "Answer a question about a folder of Markdown and code",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    search: SearchArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Fold a file to its outline, or unfold one address or line range
    Read {
        file: PathBuf,
        address: Option<String>,
    },
    /// Print the usage text an agent's prompt carries
    Usage,
}

#[derive(clap::Args)]
struct SearchArgs {
    /// Folder to search
    folder: Option<PathBuf>,
    /// The question, in plain words
    question: Vec<String>,
    /// Candidates to print
    #[arg(short, long, default_value_t = DEFAULT_LIMIT)]
    limit: usize,
    /// Print JSON instead of text
    #[arg(long)]
    json: bool,
    /// Leave camelCase identifiers unsplit (for evaluation)
    #[arg(long, hide = true)]
    no_split: bool,
    /// Index no frontmatter descriptions (for evaluation)
    #[arg(long, hide = true)]
    no_descriptions: bool,
}

fn main() {
    let cli = Cli::parse();
    let result = cli::with_stdout(|out| match run(&cli, out) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("{:#}", e);
            std::process::exit(1);
        }
    });
    if let Err(e) = result {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

fn run(cli: &Cli, out: &mut impl std::io::Write) -> Result<()> {
    match &cli.command {
        Some(Command::Usage) => {
            writeln!(out, "{}", USAGE)?;
            Ok(())
        }
        Some(Command::Read { file, address }) => read::run(out, file, address.as_deref()),
        None => search(&cli.search, out),
    }
}

fn search(args: &SearchArgs, out: &mut impl std::io::Write) -> Result<()> {
    let Some(folder) = &args.folder else {
        anyhow::bail!("usage: mdseek <folder> <question>");
    };
    let question = args.question.join(" ");
    if question.trim().is_empty() {
        anyhow::bail!("usage: mdseek <folder> <question>");
    }
    let files = sections::walk(folder)?;
    let options = rank::Options {
        split_identifiers: !args.no_split,
        descriptions: !args.no_descriptions,
        ..rank::Options::default()
    };
    let index = rank::Index::build(sections::sections(&files), options)?;
    let outcome = index.search(&question, args.limit)?;
    if args.json {
        render::json(out, &index, &outcome)?;
    } else {
        render::text(out, &index, &outcome, &folder.to_string_lossy())?;
    }
    Ok(())
}
