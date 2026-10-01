// Checks what the JavaScript loader does itself, which the fixture comparison
// cannot reach: the stack the module reserves, and recovery after it traps.
//
//   node --test scripts/wasm-loaders.test.mjs   (after the wasm build)

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { load as loadMdstruct } from "../mdstruct-wasm/mdstruct.mjs";

const wasm = (name) =>
  readFile(new URL(`../target/wasm32-unknown-unknown/wasm/${name}_wasm.wasm`, import.meta.url));

// Deeper than any engine's stack reaches, so the module traps wherever it runs.
const tooDeep = `# H\n\n${"> ".repeat(200_000)}x\n`;

/** The instance's memory at start, which holds the stack wasm-ld reserved. */
async function initialMemory(name) {
  const module = new WebAssembly.Module(await wasm(name));
  return new WebAssembly.Instance(module, {}).exports.memory.buffer.byteLength;
}

test("mdstruct.wasm reserves the CLI's 8 MiB stack", async () => {
  assert.ok((await initialMemory("mdstruct")) >= 8 * 2 ** 20);
});

test("mdstruct: a trap leaves the next call as a fresh module answers it", async () => {
  const mdstruct = await loadMdstruct(await wasm("mdstruct"));
  const fresh = await loadMdstruct(await wasm("mdstruct"));
  // The engine's own error, not a framed one: the module never returned.
  assert.throws(() => mdstruct.parseJson(tooDeep), (e) => !e.message.startsWith("mdstruct:"));
  assert.equal(mdstruct.parseJson("# A\n\nhello\n"), fresh.parseJson("# A\n\nhello\n"));
});
