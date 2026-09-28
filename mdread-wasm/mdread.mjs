// Loads mdread.wasm and folds or unfolds Markdown in-process. Runs in Bun, Node,
// and browsers: it needs only WebAssembly, TextEncoder, and TextDecoder.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

// Wasm memory never shrinks. An instance that one large document left holding
// more than this is replaced, so the next document does not carry its peak.
const RETAINED_MEMORY = 64 * 2 ** 20;

/**
 * Instantiate the module once, then call `read` per document.
 *
 * A call that traps, such as on a document nested deeper than the engine's
 * stack allows, throws the engine's error and replaces the instance, so the
 * next call is unaffected.
 *
 * @param {BufferSource | WebAssembly.Module} source the bytes of mdread.wasm, or a compiled module
 */
export async function load(source) {
  const module =
    source instanceof WebAssembly.Module ? source : await WebAssembly.compile(source);
  let instance = new WebAssembly.Instance(module, {});

  /**
   * The text `mdread - [address]` prints for this content: the folded heading
   * tree, or the one section `options.address` names. A string is encoded as
   * UTF-8; bytes pass through as they are, as `mdread -` reads its stdin.
   * Throws where the CLI exits non-zero, with the message it prints.
   */
  function read(content, options = {}) {
    const bytes = typeof content === "string" ? encoder.encode(content) : content;
    const flags = encoder.encode(JSON.stringify(options));
    const { memory, alloc, dealloc, read_json } = instance.exports;
    let response;
    try {
      const put = (block) => {
        // Pointers come back as signed i32; `>>> 0` reads them as unsigned.
        const ptr = alloc(block.length) >>> 0;
        new Uint8Array(memory.buffer, ptr, block.length).set(block);
        return ptr;
      };
      const bytesPtr = put(bytes);
      const flagsPtr = put(flags);
      const frame = read_json(bytesPtr, bytes.length, flagsPtr, flags.length) >>> 0;
      dealloc(bytesPtr, bytes.length);
      dealloc(flagsPtr, flags.length);
      // Reading can grow memory, which detaches old views, so take fresh ones.
      const len = new DataView(memory.buffer).getUint32(frame, true);
      response = JSON.parse(decoder.decode(new Uint8Array(memory.buffer, frame + 4, len)));
      dealloc(frame, len + 4);
    } catch (e) {
      // A trap leaves the stack pointer where it stopped, so the instance is spent.
      instance = new WebAssembly.Instance(module, {});
      throw e;
    }
    if (memory.buffer.byteLength > RETAINED_MEMORY) instance = new WebAssembly.Instance(module, {});
    if ("error" in response) throw new Error(`mdread: ${response.error}`);
    return response.text;
  }

  return { read };
}
