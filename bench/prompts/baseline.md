Answer a navigation question about the folder {root}. It holds {corpus_desc}.

Question: {question}

Find the one {unit} that answers it. Search with `rg` (ripgrep) through the Bash tool, and read with the Read tool. Use no other search tool or index, no web access, and edit nothing.

End your reply with exactly one line:
ANSWER: <path relative to {root}>:<first line>-<last line>
