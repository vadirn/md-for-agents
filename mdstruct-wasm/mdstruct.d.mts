export interface Mdstruct {
  /** The JSON line `mdstruct -` prints for this input, without its newline. Throws on bytes that are not UTF-8. */
  parseJson(input: string | Uint8Array): string;
  /** The parsed document. Its shape is mdstruct's JSON contract. */
  parse(input: string | Uint8Array): unknown;
}

/** Instantiate mdstruct.wasm once; the returned functions parse in-process. */
export function load(source: BufferSource | WebAssembly.Module): Promise<Mdstruct>;
