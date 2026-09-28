// Loads mdread.wasm and folds or unfolds Markdown in-process. Runs in Bun, Node,
// and browsers: it needs only WebAssembly, TextEncoder, and TextDecoder.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/**
 * Instantiate the module once, then call `read` per document.
 *
 * @param {BufferSource | WebAssembly.Module} source the bytes of mdread.wasm, or a compiled module
 */
export async function load(source) {
  const instance =
    source instanceof WebAssembly.Module
      ? await WebAssembly.instantiate(source, {})
      : (await WebAssembly.instantiate(source, {})).instance;
  const { memory, alloc, dealloc, read_json } = instance.exports;

  /**
   * The text `mdread - [address]` prints for this content: the folded heading
   * tree, or the one section `options.address` names. Throws where the CLI
   * exits non-zero, with the message it prints.
   */
  function read(content, options = {}) {
    const request = encoder.encode(JSON.stringify({ ...options, content }));
    // Pointers come back as signed i32; `>>> 0` reads them as unsigned.
    const ptr = alloc(request.length) >>> 0;
    new Uint8Array(memory.buffer, ptr, request.length).set(request);
    const frame = read_json(ptr, request.length) >>> 0;
    dealloc(ptr, request.length);
    // Reading can grow memory, which detaches old views, so take fresh ones.
    const len = new DataView(memory.buffer).getUint32(frame, true);
    const response = JSON.parse(decoder.decode(new Uint8Array(memory.buffer, frame + 4, len)));
    dealloc(frame, len + 4);
    if ("error" in response) throw new Error(`mdread: ${response.error}`);
    return response.text;
  }

  return { read };
}
