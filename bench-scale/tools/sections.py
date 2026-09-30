#!/usr/bin/env python3
"""Print every leaf section of a Markdown corpus as JSON lines.

Usage: sections.py <corpus root> < file-list

A leaf section is a heading with no deeper heading under it. Its span runs
from the heading line to the last non-blank line before the next heading.
Fenced code blocks and frontmatter hold no headings.
"""

import json
import re
import sys
from pathlib import Path

HEADING = re.compile(r"^(#{1,6})\s+(.+?)\s*#*\s*$")
FENCE = re.compile(r"^\s*(```|~~~)")


def headings(lines):
    """Yield (index, level, text) for each ATX heading outside code and frontmatter."""
    fence, start = None, 0
    if lines and lines[0].strip() == "---":
        for i in range(1, len(lines)):
            if lines[i].strip() in ("---", "..."):
                start = i + 1
                break
    for i in range(start, len(lines)):
        m = FENCE.match(lines[i])
        if m:
            fence = None if fence == m.group(1) else fence or m.group(1)
            continue
        if fence:
            continue
        h = HEADING.match(lines[i])
        if h:
            yield i, len(h.group(1)), h.group(2)


def main():
    root = Path(sys.argv[1])
    for file in filter(None, sys.stdin.read().split("\n")):
        lines = (root / file).read_text(encoding="utf-8").split("\n")
        found = list(headings(lines))
        for k, (i, level, text) in enumerate(found):
            nxt = found[k + 1] if k + 1 < len(found) else None
            if nxt and nxt[1] > level:
                continue  # it has a subsection, so it is no leaf
            end = nxt[0] if nxt else len(lines)
            while end > i + 1 and not lines[end - 1].strip():
                end -= 1
            print(json.dumps({"path": file, "name": text, "kind": "section",
                              "start": i + 1, "end": end}, ensure_ascii=False))


if __name__ == "__main__":
    main()
