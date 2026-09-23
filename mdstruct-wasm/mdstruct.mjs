// Loads mdstruct.wasm and parses Markdown in-process. Runs in Bun, Node, and
// browsers: it needs only WebAssembly, TextEncoder, and TextDecoder.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/**
 * Instantiate the module once, then call `parseJson` or `parse` per document.
 *
 * @param {BufferSource | WebAssembly.Module} source the bytes of mdstruct.wasm, or a compiled module
 */
export async function load(source) {
  const instance =
    source instanceof WebAssembly.Module
      ? await WebAssembly.instantiate(source, {})
      : (await WebAssembly.instantiate(source, {})).instance;
  const { memory, alloc, dealloc, parse_json } = instance.exports;

  /**
   * The JSON line `mdstruct -` prints for this input, without its newline.
   * A string is encoded as UTF-8; bytes pass through as they are.
   */
  function parseJson(input) {
    const bytes = typeof input === "string" ? encoder.encode(input) : input;
    // Pointers come back as signed i32; `>>> 0` reads them as unsigned.
    const ptr = alloc(bytes.length) >>> 0;
    new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
    const frame = parse_json(ptr, bytes.length) >>> 0;
    dealloc(ptr, bytes.length);
    // Parsing can grow memory, which detaches old views, so take fresh ones.
    const len = new DataView(memory.buffer).getUint32(frame, true);
    const json = decoder.decode(new Uint8Array(memory.buffer, frame + 4, len));
    dealloc(frame, len + 4);
    if (json.startsWith('{"error":')) throw new Error(`mdstruct: ${JSON.parse(json).error}`);
    return json;
  }

  return {
    parseJson,
    /** The parsed document, as `JSON.parse` returns it. */
    parse: (input) => JSON.parse(parseJson(input)),
  };
}
