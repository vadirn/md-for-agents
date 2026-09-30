#!/usr/bin/env python3
"""Agent benchmark for mdseek: prepare, run, grade, account, aggregate.

Usage:
  bench.py check                      corpora match their pins
  bench.py render <label> <task>      print the prompt for one run, and keep it
  bench.py collect <label> <dir>      read subagent transcripts into results
  bench.py aggregate <label>...       grade and print one row per label
  bench.py verdict <base> <tool>      WIN or NOT WIN, by the README's rule

<label> is `baseline` or `tool-r<N>`; the baseline label renders the baseline
prompt and every other label the tool prompt. A run's subagent description must
read `bench <label> <task> r<n>`, which is how `collect` finds it. See README.md.
"""

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RESULTS = HERE / "results"
SEEK = HERE / "bin" / "seek"
RUNS_PER_TASK = 3
UNITS = {
    "code": "definition (a function, method, class or similar block)",
    "markdown": "section of a note (a heading and the text under it)",
}
# Weights relative to the price of one uncached input token.
WEIGHTS = {"input": 1.0, "cache_write": 1.25, "cache_read": 0.1, "output": 5.0}
# Tools a benchmark agent may call. Anything else is a violation.
ALLOWED_TOOLS = {"Bash", "Read", "Grep", "Glob", "SubagentHandback"}
# Commands that search through an index or reach the web. The baseline may run
# none of them; the tool side may run only `seek`, the wrapper around mdseek.
BARRED = {"mdsearch", "mdread", "vault-query", "mdseek", "seek", "curl", "wget"}
TOOL_ALLOWED = {"seek"}


def load_corpora():
    spec = json.loads((HERE / "corpora.json").read_text())
    root = Path(spec["root"])
    return {name: {**c, "path": root / c["dir"]} for name, c in spec["corpora"].items()}


def load_tasks():
    lines = (HERE / "tasks.jsonl").read_text().splitlines()
    return {t["id"]: t for t in map(json.loads, filter(None, lines))}


def tree_sha256(path):
    """Hash every Markdown file's relative path and contents, in sorted order."""
    h = hashlib.sha256()
    for f in sorted(p for p in path.rglob("*.md") if p.is_file()):
        h.update(str(f.relative_to(path)).encode() + b"\0")
        h.update(hashlib.sha256(f.read_bytes()).hexdigest().encode() + b"\n")
    return h.hexdigest()


def git(path, *args):
    return subprocess.run(["git", *args], cwd=path, capture_output=True, text=True).stdout.strip()


def check():
    ok = True
    for name, c in load_corpora().items():
        if "git" in c:
            head, dirty = git(c["path"], "rev-parse", "HEAD"), git(c["path"], "status", "--porcelain")
            good = head == c["git"] and not dirty
            got = head[:12] + (" dirty" if dirty else "")
        else:
            got = tree_sha256(c["path"])
            good = got == c["tree_sha256"]
            got = got[:12]
        ok &= good
        print(f"{name:4} {'ok' if good else 'CHANGED'}  {c['dir']}  {got}")
    return 0 if ok else 1


def tool_usage():
    out = subprocess.run([str(SEEK), "usage"], capture_output=True, text=True, check=True)
    return out.stdout.strip()


def side_of(label):
    return "baseline" if label == "baseline" else "tool"


def commands_run(command):
    """The program names a shell command runs: the first word of each pipeline
    stage, after any `VAR=value` assignments, without its directory."""
    names = []
    for stage in re.split(r"\|\||&&|[|;\n]|\$\(|`", command):
        words = stage.strip().split()
        while words and re.match(r"^\w+=", words[0]):
            words.pop(0)
        if words:
            names.append(words[0].strip("'\"()").rsplit("/", 1)[-1])
    return names


def render_label(label, task_id):
    """Render a run's prompt and keep it under results/<label>/prompts, so a
    later change to the tool's usage text cannot fail an earlier round's check."""
    text = render(side_of(label), task_id)
    kept = RESULTS / label / "prompts" / f"{task_id}.md"
    kept.parent.mkdir(parents=True, exist_ok=True)
    kept.write_text(text + "\n")
    return text


def render(side, task_id):
    task, corpora = load_tasks()[task_id], load_corpora()
    corpus = corpora[task["corpus"]]
    text = (HERE / "prompts" / f"{side}.md").read_text()
    fields = {
        "root": str(corpus["path"]),
        "corpus_desc": corpus["desc"],
        "question": task["question"],
        "unit": UNITS[corpus["unit"]],
        "seek": str(SEEK),
    }
    if side == "tool":
        fields["tool_usage"] = tool_usage()
    for key, value in fields.items():
        text = text.replace("{" + key + "}", value)
    return text.strip()


def read_transcript(path):
    calls, tools, commands, answer, prompt, handback = {}, [], [], "", None, None
    for line in path.read_text().splitlines():
        event = json.loads(line)
        message = event.get("message") or {}
        if event.get("type") == "user" and prompt is None:
            content = message.get("content")
            prompt = content if isinstance(content, str) else "".join(
                b.get("text", "") for b in content if b.get("type") == "text")
        if event.get("type") != "assistant":
            continue
        if message.get("id") and message.get("usage"):
            calls[message["id"]] = (message.get("model"), message["usage"])
        for block in message.get("content") or []:
            if block.get("type") == "text":
                answer = block["text"] or answer
            if block.get("type") != "tool_use":
                continue
            tools.append(block["name"])
            if block["name"] == "SubagentHandback":
                handback = block["input"].get("message", handback)
            elif block["name"] == "Bash":
                commands.append(block["input"].get("command", ""))
    # The handback is the answer; text after it, such as "Delivered.", is not.
    return calls, tools, commands, answer if handback is None else handback, prompt


def expected_prompt(label, task):
    kept = RESULTS / label / "prompts" / f"{task}.md"
    return kept.read_text().strip() if kept.exists() else render(side_of(label), task)


def collect(label, transcripts):
    side = side_of(label)
    barred = BARRED if side == "baseline" else BARRED - TOOL_ALLOWED
    pattern = re.compile(rf"^bench {re.escape(label)} (\S+) r(\d+)$")
    records = []
    for meta_path in sorted(Path(transcripts).glob("agent-*.meta.json")):
        meta = json.loads(meta_path.read_text())
        m = pattern.match(meta.get("description", ""))
        if not m:
            continue
        task, run = m.group(1), int(m.group(2))
        transcript = meta_path.with_name(meta_path.name.replace(".meta.json", ".jsonl"))
        calls, tools, commands, answer, prompt = read_transcript(transcript)
        usage = {"input": 0, "cache_write": 0, "cache_read": 0, "output": 0}
        models, peak = set(), 0
        for model, u in calls.values():
            models.add(model)
            call = [u.get(k, 0) for k in ("input_tokens", "cache_creation_input_tokens",
                                          "cache_read_input_tokens", "output_tokens")]
            for key, value in zip(usage, call):
                usage[key] += value
            peak = max(peak, sum(call))
        violations = sorted({t for t in tools if t not in ALLOWED_TOOLS})
        violations += sorted({name for c in commands for name in commands_run(c) if name in barred})
        records.append({
            "label": label, "task": task, "run": run, "agent": meta_path.name[6:-10],
            "models": sorted(filter(None, models)), "calls": len(calls),
            "tokens": sum(usage.values()), "peak": peak,
            "weighted": round(sum(WEIGHTS[k] * v for k, v in usage.items())),
            **usage, "tool_uses": len(tools) - tools.count("SubagentHandback"),
            "violations": violations,
            "prompt_ok": prompt is not None and prompt.strip() == expected_prompt(label, task),
            "answer": answer, "commands": commands,
        })
    out = RESULTS / label
    out.mkdir(parents=True, exist_ok=True)
    records.sort(key=lambda r: (r["task"], r["run"]))
    (out / "runs.jsonl").write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in records))
    print(f"{label}: {len(records)} runs collected into {out / 'runs.jsonl'}")
    bad = [r for r in records if not r["prompt_ok"] or r["violations"]]
    for r in bad:
        print(f"  {r['task']} r{r['run']}: prompt_ok={r['prompt_ok']} violations={r['violations']}")
    return 0


ANSWER = re.compile(r"ANSWER:\s*`?(.+?):L?(\d+)(?:\s*[-–—]\s*L?(\d+))?`?\s*$", re.M)


def grade(task, corpus, answer):
    """Pass: the right file, a range overlapping the gold, and no wider than
    twice the gold span plus 20 lines."""
    found = ANSWER.findall(answer or "")
    if not found:
        return False
    path, start, end = found[-1]
    path = path.strip().strip("`'\"")
    root = str(corpus["path"])
    if path.startswith(root):
        path = path[len(root):].lstrip("/")
    path = path.removeprefix("./")
    start, end = int(start), int(end or start)
    gold = task["gold"]
    span = gold["end"] - gold["start"] + 1
    return (path == gold["path"] and start <= gold["end"] and end >= gold["start"]
            and end - start + 1 <= 2 * span + 20)


def summarize(label):
    tasks, corpora = load_tasks(), load_corpora()
    runs = [json.loads(l) for l in (RESULTS / label / "runs.jsonl").read_text().splitlines() if l]
    per_task = {t: [0, 0] for t in tasks}
    for r in runs:
        task = tasks[r["task"]]
        r["pass"] = grade(task, corpora[task["corpus"]], r["answer"])
        per_task[r["task"]][0] += r["pass"]
        per_task[r["task"]][1] += 1
    judge_log = RESULTS / label / "judge.jsonl"
    judge_ms = sum(json.loads(l).get("ms", 0) for l in judge_log.read_text().splitlines() if l) if judge_log.exists() else 0
    return {
        "label": label,
        "solved": sum(1 for p, n in per_task.values() if n and p * 2 > n),
        "tasks": len(tasks),
        "runs": len(runs),
        "passing": sum(r["pass"] for r in runs),
        "tokens": sum(r["tokens"] for r in runs),
        "weighted": sum(r["weighted"] for r in runs),
        "tool_uses": sum(r["tool_uses"] for r in runs),
        "violations": sum(1 for r in runs if r["violations"] or not r["prompt_ok"]),
        "judge_s": judge_ms / 1000,
        "per_task": per_task,
    }


def aggregate(labels, verbose=False):
    rows = [summarize(l) for l in labels]
    print(f"{'side':10} {'solved':>7} {'runs':>5} {'passing':>8} {'total tokens':>13} "
          f"{'weighted':>11} {'tool uses':>9} {'flagged':>7} {'judge s':>8}")
    for s in rows:
        print(f"{s['label']:10} {s['solved']:>4}/{s['tasks']:<2} {s['runs']:>5} "
              f"{s['passing']:>4}/{s['runs']:<3} {s['tokens']:>13,} {s['weighted']:>11,} "
              f"{s['tool_uses']:>9} {s['violations']:>7} {s['judge_s']:>8.1f}")
        if s["runs"] != s["tasks"] * RUNS_PER_TASK:
            print(f"  incomplete: expected {s['tasks'] * RUNS_PER_TASK} runs")
    if verbose:
        for task in rows[0]["per_task"]:
            print(f"  {task:8} " + "  ".join(f"{s['label']} {s['per_task'][task][0]}/{s['per_task'][task][1]}" for s in rows))
    return rows


def verdict(base, tool):
    b, t = summarize(base), summarize(tool)
    # A side missing runs sums fewer tokens, so no verdict reads it.
    short = [s for s in (b, t) if s["runs"] != s["tasks"] * RUNS_PER_TASK]
    for s in short:
        print(f"INCOMPLETE: {s['label']} has {s['runs']} runs, expected {s['tasks'] * RUNS_PER_TASK}")
    if short:
        return 1
    win = t["solved"] >= b["solved"] and t["tokens"] < b["tokens"]
    print(f"{'WIN' if win else 'NOT WIN'}: {tool} solved {t['solved']}/{t['tasks']} vs {base} "
          f"{b['solved']}/{b['tasks']}, total tokens {t['tokens']:,} vs {b['tokens']:,} "
          f"({(t['tokens'] - b['tokens']) / b['tokens']:+.1%})")
    return 0


def main(argv):
    cmd, args = (argv[0], argv[1:]) if argv else ("", [])
    if cmd == "check":
        return check()
    if cmd == "render" and len(args) == 2:
        print(render_label(*args))
        return 0
    if cmd == "collect" and len(args) == 2:
        return collect(*args)
    if cmd == "aggregate" and args:
        verbose = "-v" in args
        aggregate([a for a in args if a != "-v"], verbose)
        return 0
    if cmd == "verdict" and len(args) == 2:
        return verdict(*args)
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
