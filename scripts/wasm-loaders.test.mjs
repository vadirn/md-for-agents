// Checks what the JavaScript loaders do themselves, which the fixture
// comparison cannot reach: recovery after the module traps, and how a
// JavaScript string crosses into the module.
//
//   node --test scripts/wasm-loaders.test.mjs   (after the wasm build)

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { load as loadMdread } from "../mdread-wasm/mdread.mjs";
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

test("mdread.wasm reserves the CLI's 8 MiB stack", async () => {
  assert.ok((await initialMemory("mdread")) >= 8 * 2 ** 20);
});

test("mdread: a trap leaves the next call as a fresh module answers it", async () => {
  const mdread = await loadMdread(await wasm("mdread"));
  const fresh = await loadMdread(await wasm("mdread"));
  assert.throws(() => mdread.read(tooDeep, { address: "1" }), (e) => !e.message.startsWith("mdread:"));
  assert.equal(mdread.read("# A\n\nhello\n"), fresh.read("# A\n\nhello\n"));
});

test("mdread: a string reads as its UTF-8 bytes do", async () => {
  const mdread = await loadMdread(await wasm("mdread"));
  const page = await readFile(new URL("../mdread/tests/fixtures/rich.md", import.meta.url), "utf8");
  for (const address of [undefined, "1", "links"]) {
    assert.equal(mdread.read(page, { address }), mdread.read(new TextEncoder().encode(page), { address }));
  }
});

test("mdread: half a surrogate pair reads as U+FFFD, as it reaches the CLI through a pipe", async () => {
  const mdread = await loadMdread(await wasm("mdread"));
  const cut = "# A\n\n" + "\u{1F600}".slice(0, 1) + "\n";
  assert.equal(mdread.read(cut, { address: "1" }), mdread.read("# A\n\n\uFFFD\n", { address: "1" }));
});

test("mdread: bytes that are not UTF-8 throw the CLI's message", async () => {
  const mdread = await loadMdread(await wasm("mdread"));
  assert.throws(
    () => mdread.read(new Uint8Array([0x23, 0x20, 0x41, 0x0a, 0xff, 0x0a])),
    { message: "mdread: stream did not contain valid UTF-8" },
  );
});
