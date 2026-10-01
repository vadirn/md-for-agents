// Loads mdstruct.wasm and parses Markdown in-process. Runs in Bun, Node, and
// browsers: it needs only WebAssembly, TextEncoder, and TextDecoder.
//
// A host vendors this file alone beside its module, so it imports nothing.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

// Wasm memory never shrinks. An instance that one large document left holding
// more than this is replaced, so the next document does not carry its peak.
const RETAINED_MEMORY = 64 * 2 ** 20;

/**
 * Instantiate the module once, then call `parseJson` or `parse` per document.
 *
 * A call that traps, such as on input nested deeper than the engine's stack
 * allows, throws the engine's error and replaces the instance, so the next
 * call is unaffected.
 *
 * @param {BufferSource | WebAssembly.Module} source the bytes of mdstruct.wasm, or a compiled module
 */
export async function load(source) {
  const module =
    source instanceof WebAssembly.Module ? source : await WebAssembly.compile(source);
  let instance = new WebAssembly.Instance(module, {});

  /**
   * The JSON line `mdstruct -` prints for this input, without its newline.
   * A string is encoded as UTF-8; bytes pass through as they are.
   */
  function parseJson(input) {
    const bytes = typeof input === "string" ? encoder.encode(input) : input;
    const { memory, alloc, dealloc, parse_json } = instance.exports;
    let json;
    try {
      // Pointers come back as signed i32; `>>> 0` reads them as unsigned.
      const ptr = alloc(bytes.length) >>> 0;
      new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
      const frame = parse_json(ptr, bytes.length) >>> 0;
      dealloc(ptr, bytes.length);
      // Parsing can grow memory, which detaches old views, so take fresh ones.
      const len = new DataView(memory.buffer).getUint32(frame, true);
      json = decoder.decode(new Uint8Array(memory.buffer, frame + 4, len));
      dealloc(frame, len + 4);
    } catch (e) {
      // A trap leaves the stack pointer where it stopped, so the instance is spent.
      instance = new WebAssembly.Instance(module, {});
      throw e;
    }
    if (memory.buffer.byteLength > RETAINED_MEMORY) instance = new WebAssembly.Instance(module, {});
    if (json.startsWith('{"error":')) throw new Error(`mdstruct: ${JSON.parse(json).error}`);
    return json;
  }

  return {
    parseJson,
    /** The parsed document, as `JSON.parse` returns it. */
    parse: (input) => JSON.parse(parseJson(input)),
  };
}
