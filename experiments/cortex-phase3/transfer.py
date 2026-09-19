#!/usr/bin/env python3
"""Phase 3b — transfer: does the effect survive a language the model KNOWS?

The alternative explanation for Phase 2 is unflattering and plausible: the gain
may just be "show a model a compiler error for a language it has never seen".
If so it says nothing about grounded observation in general, and would not
transfer to Python — which this model knows extremely well.

Same design as Phase 2: identical model, tasks, attempts (3) and token limit;
the arms differ only in whether attempt N+1 sees the real interpreter error.
Verification is the same shape too — hidden tests the model never sees, run in
a subprocess, so a patch that games the visible symptom does not score.
"""
import argparse, hashlib, json, os, pathlib, subprocess, sys, tempfile, time
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase1"))
os.environ.setdefault("HF_HUB_OFFLINE", "1")
import runner as R

HERE = pathlib.Path(__file__).parent

# Python tasks. Each broken form must FAIL its hidden test — validated at start.
TASKS = [
    ("py_mutable_default",
     "def add_item(item, bucket=[]):\n    bucket.append(item)\n    return bucket\n",
     "assert add_item(1) == [1]\nassert add_item(2) == [2], 'default must not persist between calls'\n",
     "add_item(item) must return a list containing only that item on each call."),
    ("py_int_div",
     "def halves(n):\n    return n / 2\n",
     "assert halves(7) == 3\nassert halves(8) == 4\nassert isinstance(halves(8), int)\n",
     "halves(n) must return the integer half of n, truncating toward zero."),
    ("py_off_by_one",
     "def last_n(xs, n):\n    return xs[len(xs) - n - 1:]\n",
     "assert last_n([1,2,3,4], 2) == [3,4]\nassert last_n([1,2,3], 3) == [1,2,3]\n",
     "last_n(xs, n) returns the final n elements."),
    ("py_dict_keyerror",
     "def get_count(d, k):\n    return d[k]\n",
     "assert get_count({'a': 1}, 'a') == 1\nassert get_count({}, 'z') == 0, 'missing key must yield 0'\n",
     "get_count(d, k) returns d[k], or 0 when the key is absent."),
    ("py_string_reverse",
     "def rev(s):\n    return s[::1]\n",
     "assert rev('abc') == 'cba'\nassert rev('') == ''\n",
     "rev(s) returns the string reversed."),
    ("py_sum_wrong_init",
     "def total(xs):\n    t = 1\n    for x in xs:\n        t += x\n    return t\n",
     "assert total([1,2,3]) == 6\nassert total([]) == 0\n",
     "total(xs) sums the list; the empty list sums to 0."),
    ("py_sort_key",
     "def by_len(words):\n    return sorted(words)\n",
     "assert by_len(['ccc','a','bb']) == ['a','bb','ccc']\nassert by_len(['dd','e']) == ['e','dd']\n",
     "by_len(words) sorts the words by LENGTH, shortest first."),
]

PROMPT = """Repair this Python function.

Intent: {intent}

```python
{broken}
```

Reply with ONLY the corrected function inside one ```python code block. No prose."""

FEEDBACK = """

Your previous attempt was:
```python
{prev}
```
Running it FAILED:
{diag}

Use that error. Reply with ONLY the corrected function in one ```python block."""


def run_py(code, tests, tag):
    """Run code+tests in a subprocess. Returns (passed, combined output)."""
    with tempfile.TemporaryDirectory() as d:
        p = pathlib.Path(d, f"{tag}.py")
        p.write_text(code + "\n\n" + tests)
        try:
            r = subprocess.run([sys.executable, str(p)], capture_output=True,
                               text=True, timeout=20)
            return r.returncode == 0, (r.stdout + r.stderr)[:1200]
        except subprocess.TimeoutExpired:
            return False, "TIMEOUT"


def extract_py(raw):
    if "```" in raw:
        parts = raw.split("```")
        if len(parts) >= 2:
            b = parts[1]
            if b.startswith("python"):
                b = b[6:]
            return b.strip()
    return raw.strip()


def episode(model, task, arm, idx, args):
    name, broken, hidden, intent = task
    t0 = time.time()
    rec = {"episode": idx, "arm": arm, "task": name, "language": "python",
           "model": model.name, "seed": args.seed + idx * 97,
           "max_attempts": args.attempts, "prompt_tokens": 0,
           "completion_tokens": 0, "gen_wall_s": 0.0, "tool_calls": 0}
    pre_ok, _ = run_py(broken, hidden, name)
    rec["tool_calls"] += 1
    rec["precondition_broken"] = not pre_ok
    base = PROMPT.format(intent=intent, broken=broken)
    prompt, patch, solved = base, "", False
    for a in range(args.attempts):
        raw, u = model.generate_sampled(prompt, args.max_new_tokens, rec["seed"] + a)
        for k in ("prompt_tokens", "completion_tokens", "gen_wall_s"):
            rec[k] += u[k]
        patch = extract_py(raw)
        ok, out = run_py(patch, hidden, name)
        rec["tool_calls"] += 1
        if ok:
            solved = True
            rec["attempts_used"] = a + 1
            break
        if a + 1 < args.attempts and arm == "on":
            prompt = base + FEEDBACK.format(prev=patch[:700], diag=out[:700])
    rec.setdefault("attempts_used", args.attempts)
    rec.update(hidden_pass=solved, score=1.0 if solved else 0.0,
               patch_sha256=hashlib.sha256(patch.encode()).hexdigest()[:16],
               wall_s=round(time.time() - t0, 3),
               gen_wall_s=round(rec["gen_wall_s"], 3),
               evaluable=bool(rec["precondition_broken"]),
               outcome="repaired" if solved else "not_repaired")
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--per-arm", type=int, default=70)
    ap.add_argument("--attempts", type=int, default=3)
    ap.add_argument("--max-new-tokens", type=int, default=400)
    ap.add_argument("--seed", type=int, default=44000)
    args = ap.parse_args()

    bad = [t[0] for t in TASKS if run_py(t[1], t[2], t[0])[0]]
    if bad:
        print(f"REFUSING TO RUN — these tasks are not broken: {bad}", file=sys.stderr)
        return 2

    model = R.Model("Qwen/Qwen2.5-Coder-7B-Instruct")
    out = HERE / "transfer.jsonl"; out.write_text("")
    started, n = time.time(), 0
    for i in range(args.per_arm):
        task = TASKS[i % len(TASKS)]
        for arm in ("off", "on"):
            rec = episode(model, task, arm, n, args)
            with out.open("a") as f:
                f.write(json.dumps(rec) + "\n")
            n += 1
    meta = {"phase": "3b-transfer", "language": "python", "episodes": n,
            "wall_min": round((time.time() - started) / 60, 2),
            "attempts": args.attempts, "decoding": "sampled", "temperature": 0.7,
            "tasks": [t[0] for t in TASKS], "model": model.name}
    (HERE / "transfer_meta.json").write_text(json.dumps(meta, indent=2))
    print(json.dumps(meta, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
