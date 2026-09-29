# Agent benchmark

Does an agent find the right part of a codebase or a vault in fewer tokens with `mdseek` than with `rg` and Read alone? This benchmark answers that for crux goal MDAGENTS-6, and its design follows decision MDAGENTS-12.

## Frozen files

`MANIFEST.sha256` pins every file below. After the freeze commit, none of them changes; only the tool does. Check with:

```sh
shasum -a 256 -c bench/MANIFEST.sha256
```

- `tasks.jsonl`: 24 tasks, each a question with one gold file and line range.
- `corpora.json`: five corpora pinned by git commit, or by a content hash for the vault snapshot.
- `prompts/baseline.md`, `prompts/tool.md`: the two prompt templates.
- `bin/seek`: the wrapper through which tool-side agents call `target/release/mdseek`.
- `bench.py`: rendering, collection, grading and aggregation.
- `README.md`: this policy.

## Corpora and results

The corpora live in `~/.cache/mdagents-bench/corpora`, so a reboot keeps them. `bench.py check` confirms each one still matches its pin. `results/` stays out of git, because agent reports quote the vault's notes. The aggregate's printed table is the record.

## Tasks

There are 4 tasks each for Rust (md-for-agents), TypeScript (writer-harness), Python (requests) and Ruby (rack), plus 8 for Markdown (a vault snapshot).

- Separate authoring agents wrote the questions in plain words. No question names the target's identifier, file or heading, or copies 3 or more words from it.
- Parsers set the gold spans: Python's `ast`, Ruby's Prism, and heading ranges for Markdown. Rust and TypeScript spans were read by hand and checked with `sed`.
- The tasks are kept apart from the tuning data. The code query set (MDAGENTS-11) drops any commit whose target overlaps a gold span. The vault tuning sets use note descriptions and whole notes; these tasks target sections, with questions written by hand.

## Runs

Each task runs 3 times per side, so each side has 72 runs.

- **Agent.** A fresh Explore subagent on Sonnet, launched from the benchmark session with the prompt `bench.py render <label> <task>` prints. Render keeps each prompt under `results/<label>/prompts/` and the description `bench <label> <task> r<n>`.
- **Baseline side.** The label is `baseline`. It searches with `rg` and reads with Read.
- **Tool side.** The labels are `tool-r0` to `tool-r3`, one per improve-and-rerun round. It may also call `bench/bin/seek`, whose agent-facing usage comes from `seek usage` at render time. Before a round, write its label to `results/ROUND`, so judge timings land beside its runs.
- **Collection.** `bench.py collect <label> <transcripts dir>` reads each run's transcript: the prompt, every API call's usage, every command, and the final answer. It checks that the prompt matches the one render kept. It flags a tool outside Bash, Read, Grep and Glob. It also flags any pipeline stage that runs `mdsearch`, `mdread`, `vault-query`, `curl` or `wget`, judged by the program name rather than by any mention. The baseline also may not run `mdseek` or `seek`.

## Grading

A run passes when its last `ANSWER: <path>:<start>-<end>` line meets all three conditions:

- It names the gold file.
- Its range overlaps the gold range.
- Its range spans at most twice the gold span plus 20 lines.

A task is solved when most of its runs pass, which means 2 of 3.

## Accounting

These rules follow jevgrep's accounting policy, adapted to subagents.

- **Total tokens.** Sum every API call's uncached input, cache writes, cache reads and output. This is what the model processed, and it is the verdict's measure. A nested `claude -p` cannot sign in from the session sandbox, so dollar cost is not available (crux note MDAGENTS-5S6Q).
- **Peak context.** The largest single call, kept per run as `peak`. It matches the `subagent_tokens` figure a background Agent result reports. For one 20-call authoring run, that figure read 114,086, while its calls processed 1,972,941 tokens. So the reported figure understates cost and does not decide.
- **Weighted tokens.** Cache writes count ×1.25, cache reads ×0.1 and output ×5, relative to one uncached input token. This approximates the bill, but it does not decide.
- **Every run counts.** A run with a wrong answer or a flag still counts toward the tokens.
- **Judge wall-clock.** It is summed from `results/<label>/judge.jsonl`, where `mdseek` logs each judge call's inference time as `ms`, not its time in a queue. The judge runs locally, so its cost is time, reported next to the tokens.

## Verdict

`bench.py verdict baseline tool-rN` prints WIN when the tool side solves at least as many tasks with fewer total tokens.

- Otherwise the tool improves and round N+1 reruns the tool side, up to `tool-r3`.
- If `tool-r3` still does not win, the result is LOSS.
- The baseline runs once, before the tool exists.
