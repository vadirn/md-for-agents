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

## mdread in WebAssembly

`mdread-wasm` compiles the reader to a WebAssembly module with no imports. A JavaScript host loads it once and reads in-process. For the same content and options, the text is byte-identical to what `mdread - [address]` prints.

```js
import { load } from "./mdread.mjs";

const mdread = await load(await Bun.file("mdread.wasm").arrayBuffer());
const overview = mdread.read(page);
const section = mdread.read(page, { address: "2.1", depth: 1 });
```

The loader, `mdread-wasm/mdread.mjs`, runs in Bun, Node, and browsers. `read` takes the content as a string and these options. Each option the caller omits takes the CLI's default.

| Option      | CLI equivalent       |
| ----------- | -------------------- |
| `address`   | the address argument |
| `depth`     | `--depth`            |
| `full`      | `--full`             |
| `threshold` | `--threshold`        |

`read` returns the text and throws where the CLI exits non-zero, with the message the CLI prints. The overview names the content `-`, as the CLI names stdin. A note the CLI prints on stderr beside a successful reading is not returned. The module reads with the CLI's default dialect, so `--strict-headings` and `--wikilinks-only` have no option.

Build the module:

```bash
cargo build --profile wasm --target wasm32-unknown-unknown -p mdread-wasm
```

It lands at `target/wasm32-unknown-unknown/wasm/mdread_wasm.wasm`. A host without the loader calls three exports:

| Export                         | Contract |
| ------------------------------ | -------- |
| `alloc(len) -> ptr`            | Reserves `len` bytes for the request. |
| `read_json(ptr, len) -> frame` | Reads the JSON request `{"content": ..., "address"?: ..., "depth"?: ..., "full"?: ..., "threshold"?: ...}`. The frame is a little-endian `u32` length, then that many bytes of `{"text": ...}` JSON. |
| `dealloc(ptr, len)`            | Frees the request with its `len`, or a frame with 4 plus its length. |

A request the CLI would exit non-zero on frames `{"error": ...}` instead of text. So does a request that is not JSON of that shape, or that names an unknown option.

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

## mdstruct in WebAssembly

`mdstruct-wasm` compiles the parser to a WebAssembly module with no imports. A JavaScript host loads it once and parses in-process. For the same input, the JSON is byte-identical to the line `mdstruct -` prints, without the newline.

```js
import { load } from "./mdstruct.mjs";

const mdstruct = await load(await Bun.file("mdstruct.wasm").arrayBuffer());
const doc = mdstruct.parse("# Title\n\nSee [[Page]].\n");
```

The loader, `mdstruct-wasm/mdstruct.mjs`, runs in Bun, Node, and browsers. `parse` returns the document, and `parseJson` returns its JSON text. Both take a string or UTF-8 bytes, and both throw on bytes that are not UTF-8.

An input nested deeper than the engine's stack allows traps the module. The loader throws the engine's error and replaces the instance, so the next call is unaffected. Because wasm memory never shrinks, the loader also replaces an instance that one large document left holding more than 64 MiB. The module reserves the CLI's 8 MiB stack, so Bun reads nesting about as deep as the CLI does; Node's own stack stops sooner.

Build the module:

```bash
cargo build --profile wasm --target wasm32-unknown-unknown -p mdstruct-wasm
```

It lands at `target/wasm32-unknown-unknown/wasm/mdstruct_wasm.wasm`. A host without the loader calls three exports:

| Export                          | Contract |
| ------------------------------- | -------- |
| `alloc(len) -> ptr`             | Reserves `len` bytes for the input. |
| `parse_json(ptr, len) -> frame` | Parses the input as `mdstruct -` does. The frame is a little-endian `u32` length, then that many bytes of JSON. |
| `dealloc(ptr, len)`             | Frees the input with its `len`, or a frame with 4 plus its length. |

Bytes that are not UTF-8 frame `{"error":"..."}` instead of a document. A trap leaves the instance unusable, so a host that keeps one instance across calls replaces it after one.

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
cli/            command-line concerns the tools share: format flag, token estimate, stdout guard
mdstruct/       the parsing core; mdread, mdformat, and mdstruct-wasm depend on it
mdstruct-wasm/  mdstruct as a WebAssembly module, with its JavaScript loader
mdread/         progressive-unfolding reader
mdread-wasm/    mdread as a WebAssembly module, with its JavaScript loader
mdformat/       block-level passthrough printer
mdsearch/       BM25 search over a folder
```

`cli` is a library with no binary, and `mdstruct-wasm` and `mdread-wasm` each ship a WebAssembly module instead of one. Every other crate ships a binary. The `mdstruct` and `mdread` libraries build without clap when their `cli` feature is off, which is how their dependents take them.

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

`shell.nix` also provides `lld`, which Nix's `rustc` needs to link the WebAssembly build.

## Releases

The `musl tools` workflow checks every main-branch push, pull request, and manual run. It builds `mdstruct` and `mdread` with Rust 1.91.1 on a native arm64 Linux runner, tests the musl build, and runs both tools inside plain Alpine 3.22.6. The release gate compares fixture output byte-for-byte with the macOS build.

The same workflow builds `mdstruct.wasm` and `mdread.wasm` with Rust 1.91.1. It runs the fixtures through both modules in Node and compares that output byte-for-byte with the macOS build too. `mdread.wasm` reads each fixture with every case in `scripts/mdread-cases.txt`, and a failing case compares its error message.

After those checks pass, pushing a `v*` tag publishes the same tested files:

- the musl binaries, and `md-tools-aarch64-unknown-linux-musl.tar.gz`, which holds `usr/local/bin/mdstruct` and `usr/local/bin/mdread` for rootfs assembly
- `mdstruct.wasm`, with its loader `mdstruct.mjs` and types `mdstruct.d.mts`
- `mdread.wasm`, with its loader `mdread.mjs` and types `mdread.d.mts`
- `SHA256SUMS` and `SOURCE_REVISION`

Pin both the release URL and the SHA-256 of each asset you consume. Branch and manual runs upload CI artifacts without creating a release.

## License

MIT. See [LICENSE.md](LICENSE.md).
