// Runs every fixture through mdstruct.wasm and writes the file
// release-fixtures.sh writes for the native binary, `<name>.mdstruct.stdout`,
// so the two outputs diff.
//
//   node scripts/wasm-fixtures.mjs MDSTRUCT_WASM OUTPUT_DIR   (or bun)

import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { load as loadMdstruct } from "../mdstruct-wasm/mdstruct.mjs";

const [mdstructPath, output] = process.argv.slice(2);
if (!mdstructPath || !output) {
  throw new Error("usage: wasm-fixtures.mjs MDSTRUCT_WASM OUTPUT_DIR");
}

const mdstruct = await loadMdstruct(await readFile(mdstructPath));

const fixtures = "scripts/fixtures";
await mkdir(output, { recursive: true });
for (const file of (await readdir(fixtures)).filter((f) => f.endsWith(".md")).sort()) {
  const name = basename(file, ".md");
  // Raw bytes, as the CLI reads stdin: no decoding step to drop a BOM.
  const bytes = new Uint8Array(await readFile(join(fixtures, file)));
  await writeFile(join(output, `${name}.mdstruct.stdout`), `${mdstruct.parseJson(bytes)}\n`);
}
