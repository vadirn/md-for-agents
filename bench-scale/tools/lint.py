#!/usr/bin/env python3
"""Check each task's question against the authoring rules a script can test.

Usage: lint.py <tasks.jsonl>

It flags a question that shares a run of 3 words with its target, names an
identifier from the target, names a part of the target's path, or runs past
25 words. Every other rule is the author's judgment.
"""

import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
WORD = re.compile(r"[0-9a-zа-яё]+", re.I)
# A token that reads as code: snake_case, camelCase, or letters mixed with digits.
IDENT = re.compile(r"\b(?:\w+_\w+|[a-z]+[A-Z]\w*|[A-Za-z]+\d\w*)\b")


def words(text):
    return [w.lower() for w in WORD.findall(text)]


def trigrams(ws):
    return {tuple(ws[i:i + 3]) for i in range(len(ws) - 2)}


def main():
    spec = json.loads((HERE / "corpora.json").read_text())
    root = Path(spec["root"])
    bad = 0
    for line in filter(None, Path(sys.argv[1]).read_text().splitlines()):
        task = json.loads(line)
        gold, question = task["gold"], task["question"]
        corpus = spec["corpora"][task["corpus"]]
        lines = (root / corpus["dir"] / gold["path"]).read_text().split("\n")
        target = "\n".join(lines[gold["start"] - 1:gold["end"]])
        asked = set(words(question))
        problems = []
        shared = trigrams(words(question)) & trigrams(words(target))
        if shared:
            problems.append("shares " + ", ".join(" ".join(t) for t in sorted(shared)))
        named = sorted({i for i in IDENT.findall(target) if i.lower() in asked})
        if named:
            problems.append("names " + ", ".join(named))
        parts = {p.lower() for p in re.split(r"[/.\-_ ]", gold["path"]) if len(p) > 3}
        if parts & asked:
            problems.append("names path part " + ", ".join(sorted(parts & asked)))
        if len(question.split()) > 25:
            problems.append(f"{len(question.split())} words")
        bad += bool(problems)
        print(f"{task['id']:8} {'ok' if not problems else '; '.join(problems)}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
