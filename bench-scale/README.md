# Agent benchmark at scale

Does `mdseek` still save tokens, or find what `rg` misses, when the repository is large and the question is vague? This benchmark answers that for crux goal MDAGENTS-26. It keeps the frozen bench's method (`../bench/README.md`) and changes three things: the corpora, how targets are chosen, and how vague the questions are.

## Frozen files

`MANIFEST.sha256` pins every file below. After the freeze commit, none of them changes; only the tool does. Check with:

```sh
shasum -a 256 -c bench-scale/MANIFEST.sha256
```

- `tasks.jsonl`: 16 tasks, each a question with one gold file and line range.
- `corpora.json`: four corpora pinned by git commit, or by a content hash for the vault snapshot.
- `prompts/baseline.md`, `prompts/tool.md`: the frozen bench's two prompt templates, unchanged.
- `bin/seek`: the frozen bench's wrapper, unchanged.
- `bench.py`: the frozen bench's script, except that a run's description starts with `scale`.
- `tools/`: the scripts that drew the targets, set their spans and checked the questions.
- `README.md`: this policy.

## Corpora

Three code corpora hold 1.8 to 4.9 times the code lines of the frozen bench's largest, writer-harness (281 files, 42,593 lines):

- **panda**: Panda CSS, 1,074 files and 209,750 lines of TypeScript.
- **svelte**: Svelte, 2,746 files and 119,839 lines of JavaScript.
- **crux**: crux, 412 files and 75,947 lines of TypeScript.

The fourth corpus is the frozen bench's vault snapshot, reused unchanged.

All three code corpora are TypeScript or JavaScript, because `~/Documents` holds no large public Python or Ruby repository. The one large Ruby repository there, solaris, is an unlicensed employer repository with uncommitted changes, so it stays out.

## Tasks

There are 4 tasks per corpus.

- **Targets are drawn, not picked.** `tools/candidates.py` lists every eligible definition or leaf section and shuffles the list with the corpus name as the seed. It keeps definitions and sections of 8 to 80 lines. It leaves out tests, fixtures, generated types, examples, docs and sites. In the vault, it draws only from `20 cards`, `30 notes` and `35 experiments`, and leaves out notes named for a client or the employer.
- **Spans come from a parser, not from the tool.** `tools/definitions.cjs` sets code spans with the TypeScript compiler, from a definition's first token after its doc comment to its last line. `tools/sections.py` sets a leaf section's span from its heading to its last non-blank line.
- **Questions are vague by design.** A separate author per corpus took the candidates in order. It skipped one only when no question could single it out, or when a note section was private. Each question follows the frozen bench's rules: plain words, with no identifier, file or heading named and no run of 3 words copied. It adds three rules of its own. It is at most 25 words. It gives at most two concrete details, where the frozen bench's questions carry three or more. It prefers a user's words to the code's own. `tools/lint.py` checks every rule a script can test.
- **Every candidate yielded a question.** Each author wrote its 4 questions from candidates 1 to 4 and skipped none. `tools/candidates.drawn.jsonl` keeps the full draw of 12 per corpus.
- **One lint flag stands, as a false positive.** md-2 says "notes", which is also the name of the vault folder `30 notes`. The question uses it as an everyday word, and every vault note sits under such a folder, so the question stays unchanged.
- **Difficulty is never chosen by outcome.** No task was kept or dropped because either side passed or failed it. The tasks were frozen before either side ran.

## Runs, grading, accounting and verdict

These follow `../bench/README.md` unchanged, with 3 runs per task per side, so each side has 48 runs.

A run's subagent description reads `scale <label> <task> r<n>`, and its label is `baseline` or `tool-r<N>`. Before a tool round, write its label to `results/ROUND`.

`bench.py verdict baseline tool-rN` prints WIN when the tool side solves at least as many tasks with fewer total tokens. Here, unlike the frozen bench, the baseline may fail tasks, so the solved counts can decide.
