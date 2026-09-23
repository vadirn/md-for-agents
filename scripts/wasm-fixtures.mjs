// Runs every fixture through mdstruct.wasm and writes `<name>.mdstruct.stdout`,
// as release-fixtures.sh does for the native binary, so the two outputs diff.
//
//   node scripts/wasm-fixtures.mjs MDSTRUCT_WASM OUTPUT_DIR   (or bun)

import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { load } from "../mdstruct-wasm/mdstruct.mjs";

const [wasmPath, output] = process.argv.slice(2);
if (!wasmPath || !output) throw new Error("usage: wasm-fixtures.mjs MDSTRUCT_WASM OUTPUT_DIR");

const mdstruct = await load(await readFile(wasmPath));
const fixtures = "mdread/tests/fixtures";
await mkdir(output, { recursive: true });
for (const file of (await readdir(fixtures)).filter((f) => f.endsWith(".md")).sort()) {
  // Raw bytes, as the CLI reads stdin: no decoding step to drop a BOM.
  const bytes = new Uint8Array(await readFile(join(fixtures, file)));
  await writeFile(
    join(output, `${basename(file, ".md")}.mdstruct.stdout`),
    `${mdstruct.parseJson(bytes)}\n`,
  );
}
