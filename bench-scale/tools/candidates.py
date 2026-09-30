#!/usr/bin/env python3
"""Draw each corpus's candidate targets in a fixed random order.

Usage: candidates.py <typescript.js> <count>

Code candidates come from definitions.cjs, and vault ones from sections.py.
Tests, fixtures, generated types, examples and sites are left out. So are
spans outside [MIN_LINES, MAX_LINES], which keep a target one readable block.
The seed is the corpus name, so a rerun draws the same order.
"""

import json
import random
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
MIN_LINES, MAX_LINES = 8, 80
CODE = re.compile(r"\.(ts|tsx|mts|cts|js|jsx|mjs|cjs)$")
# Paths no target comes from: tests, fixtures, generated or vendored code,
# demos and docs sites.
SKIP = re.compile(
    r"(^|/)(tests?|__tests__|fixtures?|e2e|playgrounds?|sandbox|website|sites|examples?|"
    r"benchmarking|dist|build|vendor|node_modules|styled-system|scripts|docs)(/|$)"
    r"|\.(test|spec|d)\.[cm]?[jt]sx?$"
)
# The vault's general notes only, so no target sits in client or personal folders.
# Attachments under `<note>.files/` are reports, not notes.
VAULT = re.compile(r"^(20 cards|30 notes|35 experiments)/(?!.*\.files/).+\.md$")
# Headings that hold a template's reference list rather than prose.
LISTS = {"glossary", "links", "references", "related", "see also", "sources",
         "глоссарий", "источники", "ссылки"}
# Notes named for a client or the employer stay out, so no task quotes them.
CLIENTS = re.compile(r"remi|decagon|betterup|verifyo|evil ?martians", re.I)


def files(corpus):
    out = subprocess.run(["git", "ls-files"], cwd=corpus["path"], capture_output=True, text=True)
    listed = out.stdout.split("\n") if out.returncode == 0 else [
        str(p.relative_to(corpus["path"])) for p in corpus["path"].rglob("*.md")]
    if corpus["unit"] == "markdown":
        return sorted(f for f in listed if VAULT.match(f) and not CLIENTS.search(f))
    return sorted(f for f in listed if CODE.search(f) and not SKIP.search(f))


def enumerate_targets(corpus, typescript):
    listed = "\n".join(files(corpus))
    if corpus["unit"] == "markdown":
        cmd = [sys.executable, str(HERE / "tools" / "sections.py"), str(corpus["path"])]
    else:
        cmd = ["node", str(HERE / "tools" / "definitions.cjs"), typescript, str(corpus["path"])]
    out = subprocess.run(cmd, input=listed, capture_output=True, text=True, check=True)
    return [json.loads(line) for line in out.stdout.splitlines()]


def main():
    typescript, count = sys.argv[1], int(sys.argv[2])
    spec = json.loads((HERE / "corpora.json").read_text())
    root = Path(spec["root"])
    for name, corpus in spec["corpora"].items():
        corpus = {**corpus, "path": root / corpus["dir"]}
        targets = [t for t in enumerate_targets(corpus, typescript)
                   if MIN_LINES <= t["end"] - t["start"] + 1 <= MAX_LINES
                   and t["name"].strip().lower() not in LISTS]
        random.Random(name).shuffle(targets)
        print(f"{name}: {len(targets)} eligible", file=sys.stderr)
        for t in targets[:count]:
            print(json.dumps({"corpus": name, **t}, ensure_ascii=False))


if __name__ == "__main__":
    main()
