/** The arguments `mdread -` takes after the file. An omitted option takes the CLI's default. */
export interface ReadOptions {
  /** A dotted-numeric path (`2.1`), a heading slug, `0` or `text`, `fm` or `fm.<path>`, or `links`. Omitted, the whole document folds to its heading tree. */
  address?: string;
  /** Max levels to expand under the addressed node, as `--depth`. */
  depth?: number;
  /** Expand everything, ignoring threshold and depth, as `--full`. */
  full?: boolean;
  /** Inline cutoff in estimated tokens, as `--threshold`. Defaults to 2000. */
  threshold?: number;
}

export interface Mdread {
  /** The text `mdread - [address]` prints for this content. Throws where the CLI exits non-zero, with the message it prints, and throws the engine's error on a document nested deeper than its stack allows. */
  read(content: string, options?: ReadOptions): string;
}

/** Instantiate mdread.wasm once; the returned function reads in-process, and replaces the instance after a trap. */
export function load(source: BufferSource | WebAssembly.Module): Promise<Mdread>;
