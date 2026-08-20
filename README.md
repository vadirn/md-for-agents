# md-for-agents

Command-line tools that give an agent a structural grip on Markdown.

An agent that reads a whole Markdown file spends context on parts it does not need. These tools let it read the shape first, then unfold only what the task needs.

Every tool writes data to stdout and diagnostics to stderr, so a pipe into `jq` stays clean.

## The tools

| Tool       | What it does |
| ---------- | ------------ |
| `mdstruct` | Parses Markdown to NDJSON: one JSON document per line, one line per input. |
| `mdread`   | Folds a file to one line per section, then unfolds one section by address. |
| `mdformat` | Prints CommonMark from that same parse, scoped to whitespace and tables. |
| `mdsearch` | Ranks a folder's Markdown by BM25, over an index built in RAM for one run. |

## Quick start

The workspace is edition 2024, so it needs Rust 1.85 or newer. Clone it and build:

```bash
cargo build --release
```

The binaries land in `target/release/`. Fold this file to its shape:

```bash
./target/release/mdread README.md
```

Then unfold one section by name:

```bash
./target/release/mdread README.md quick-start
```

## mdread

`mdread` folds, then unfolds. The fold shows the heading tree, one line per section, with a line count and an estimated token count. The unfold prints the one section you name.

An address is any of these:

- a dotted-numeric path into the heading tree, such as `2.1.3`,
- a heading slug, such as `quick-start`,
- `0` or `text` for the lede before the first heading,
- `fm` for the frontmatter block, or `fm.<path>` for one value inside it,
- `links` for the outgoing links.

The reserved names win a collision. A `## Links` section is served by its numeric address instead, and the reader says so when the two collide.

```bash
mdread notes.md fm.title      # one frontmatter value
mdread notes.md 2.1 --depth 1 # one subtree, one level deep
mdread notes.md --format json # the same shape, machine-readable
```

## mdstruct

`mdstruct` parses to NDJSON on stdout. Every span in that model is a pair of byte offsets into the original input. `mdstruct` never restringifies your source, so a consumer slices its own bytes and recovers the original exactly.

```bash
mdstruct doc.md                # parse to NDJSON
mdstruct doc.md --pretty       # indented, single input only
mdstruct check doc.md          # freeze gate; exit 4 if the parse fails it
mdstruct stats doc.md          # type-coverage report
mdstruct --schema-version      # the schema contract version
```

Run `check` after changing the parser or bumping comrak. Point it at a corpus. It re-verifies that spans still tile each input byte-exactly, and that the inline grammar still holds. A failing input exits 4, and the summary goes to stderr. So it drops into CI as a gate.

## mdformat

`mdformat` rewrites layout:

- line endings
- blank-line gaps
- table padding
- list markers

It never reflows a paragraph.

```bash
mdformat format doc.md            # print the formatted result
mdformat format --check doc.md    # report what is not in normal form; exit 4
mdformat format --write doc.md    # rewrite one file in place
mdformat partition doc.md         # verify the spans partition the content
```

`--write` is the only mode that touches a file. Every other mode prints and leaves your input alone.

`partition` states the safety condition behind a block rewrite:

- Every non-whitespace byte falls in exactly one top-level span.
- No two spans overlap.
- Nothing runs past the end.

Splicing over one block's range then neither drops nor duplicates the rest of the file.

## mdsearch

`mdsearch` ranks the Markdown files in a folder against a query, best match first. Scoring is BM25 over these fields:

- the file name
- the frontmatter `description:`
- the prose after the frontmatter block

```bash
mdsearch "retry backoff" ./docs
mdsearch "importer's work" ./docs --limit 3
mdsearch "план миграции" ./docs --format json
```

Worth knowing before you use it:

- Terms are stemmed in English and Russian, so a query matches words sharing a root with it.
- Query punctuation reads as whitespace. A phrase searches for its words, and no character is query syntax.
- The walk obeys `.gitignore`, `.ignore`, and `.mdsearchignore`, in a plain folder as much as in a git repository. Pass `--no-ignore` to search anyway.

The index is built in RAM for the one run, so an edit needs no reindexing.

## Layout

```
cli/        command-line concerns the tools share: format flag, token estimate, stdout guard
mdstruct/   the parsing core; every other crate depends on it
mdread/     progressive-unfolding reader
mdformat/   block-level passthrough printer
mdsearch/   BM25 search over a folder
```

`cli` is a library with no binary. Every other crate ships one.

Shared dependencies are declared once in the root `Cargo.toml` and inherited with `.workspace = true`. So two members cannot drift onto different versions of the same crate.

## Development

`shell.nix` provides the toolchain. With direnv installed, `.envrc` loads it on `cd`:

```bash
direnv allow
```

Without direnv, enter it directly:

```bash
nix-shell
```

Then run the checks:

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets
```

Both must pass before a change lands.

## License

MIT. See [LICENSE.md](LICENSE.md).
