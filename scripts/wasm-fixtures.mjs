// Runs every fixture through mdstruct.wasm and mdread.wasm, and writes the files
// release-fixtures.sh writes for the native binaries, so the two outputs diff:
// `<name>.mdstruct.stdout`, and for each case in mdread-cases.txt,
// `<name>.mdread-<case>.stdout` and `.exit`, plus `.stderr` when the case fails.
//
//   node scripts/wasm-fixtures.mjs MDSTRUCT_WASM MDREAD_WASM OUTPUT_DIR   (or bun)

import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { load as loadMdread } from "../mdread-wasm/mdread.mjs";
import { load as loadMdstruct } from "../mdstruct-wasm/mdstruct.mjs";

const [mdstructPath, mdreadPath, output] = process.argv.slice(2);
if (!mdstructPath || !mdreadPath || !output) {
  throw new Error("usage: wasm-fixtures.mjs MDSTRUCT_WASM MDREAD_WASM OUTPUT_DIR");
}

const mdstruct = await loadMdstruct(await readFile(mdstructPath));
const mdread = await loadMdread(await readFile(mdreadPath));
const cases = (await readFile("scripts/mdread-cases.txt", "utf8"))
  .split("\n")
  .filter((line) => line && !line.startsWith("#"))
  .map((line) => {
    const [label, ...args] = line.trim().split(/\s+/);
    return { label, options: readOptions(args) };
  });

const fixtures = "mdread/tests/fixtures";
await mkdir(output, { recursive: true });
for (const file of (await readdir(fixtures)).filter((f) => f.endsWith(".md")).sort()) {
  const name = basename(file, ".md");
  // Raw bytes, as the CLI reads stdin: no decoding step to drop a BOM.
  const bytes = new Uint8Array(await readFile(join(fixtures, file)));
  await writeFile(join(output, `${name}.mdstruct.stdout`), `${mdstruct.parseJson(bytes)}\n`);

  // `mdread -` keeps a leading BOM in the text it reads, so the decoder must too.
  const content = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  for (const { label, options } of cases) {
    const out = join(output, `${name}.mdread-${label}`);
    let text;
    try {
      text = mdread.read(content, options);
    } catch (e) {
      // A trap is the module failing, not a reading the CLI rejects too.
      if (!e.message.startsWith("mdread: ")) throw e;
      // The CLI prints the message on stderr and exits 1, with nothing on stdout.
      await writeFile(`${out}.stdout`, "");
      await writeFile(`${out}.stderr`, `${e.message.slice("mdread: ".length)}\n`);
      await writeFile(`${out}.exit`, "1\n");
      continue;
    }
    // A note the CLI prints on stderr beside a reading is not the module's to
    // return, so a successful case writes no stderr to compare.
    await writeFile(`${out}.stdout`, text);
    await writeFile(`${out}.exit`, "0\n");
  }
}

/** The `read` options for the arguments `mdread -` takes after it. */
function readOptions(args) {
  const options = {};
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--full") options.full = true;
    else if (arg === "--depth" || arg === "--threshold") options[arg.slice(2)] = Number(args[++i]);
    else if (arg.startsWith("--") || "address" in options) throw new Error(`unsupported case argument: ${arg}`);
    else options.address = arg;
  }
  return options;
}
