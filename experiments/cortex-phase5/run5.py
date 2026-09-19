#!/usr/bin/env python3
"""Phase 5 — independent tasks: real stdlib code, mechanical mutation, and the
REAL function as the oracle. Verification is DIFFERENTIAL on random inputs, so
neither the code, the expected behaviour, nor the test cases are mine.
"""
import argparse, hashlib, importlib, inspect, json, pathlib, random, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase1"))
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase3"))
import os
os.environ.setdefault("HF_HUB_OFFLINE", "1")
import runner as R
from transfer import extract_py
from build2 import SHAPES

HERE = pathlib.Path(__file__).parent
TASKS = json.loads((HERE / "stdlib_tasks.json").read_text())

PROMPT = """Repair this Python function so it behaves exactly as documented.

```python
{broken}
```

Its docstring states the intended behaviour. Exactly one thing in the body is
wrong. Reply with ONLY the corrected function inside one ```python code block."""

FEEDBACK = """

Your previous attempt was:
```python
{prev}
```
It still disagrees with the reference implementation:
{diag}

Reply with ONLY the corrected function in one ```python block."""


def oracle_check(cand_src, task, trials=60, seed=777):
    """Differential test against the REAL stdlib function."""
    mod = importlib.import_module(task["module"])
    ref = getattr(mod, task["func"])
    gen = SHAPES[task["shape_index"]]
    # Exec inside a COPY of the real module's globals. A stdlib function is not
    # self-contained — `shlex.split` needs the `shlex` class, `statistics.mean`
    # needs `_sum`/`_convert` — so exec'ing it in a bare namespace produces a
    # fragment that cannot run. Measured: with a bare namespace, the ORIGINAL
    # function failed its own oracle for 13 of 13 tasks, and every control rate
    # read 0.00. That was the harness failing, not the tasks being hard.
    ns = dict(vars(mod))
    try:
        exec(cand_src, ns)
    except Exception as e:
        return False, f"candidate does not import: {type(e).__name__}: {e}"
    cf = ns.get(task["func"])
    if cf is None:
        return False, f"candidate does not define {task['func']}"
    r = random.Random(seed)
    for _ in range(trials):
        args = gen(r)
        try:
            a = ("ok", ref(*args))
        except Exception as e:
            a = ("exc", type(e).__name__)
        try:
            b = ("ok", cf(*args))
        except Exception as e:
            b = ("exc", type(e).__name__)
        if a != b:
            return False, f"on input {args!r}: reference gave {a!r}, yours gave {b!r}"
    return True, "matches the reference on every trial"


def episode(model, task, arm, idx, args):
    t0 = time.time()
    rec = {"episode": idx, "arm": arm, "task": task["name"], "language": "python",
           "source": "cpython-stdlib", "module": task["module"], "func": task["func"],
           "model": model.name, "seed": args.seed + idx * 97,
           "max_attempts": args.attempts, "prompt_tokens": 0,
           "completion_tokens": 0, "gen_wall_s": 0.0, "tool_calls": 0}
    pre_ok, _ = oracle_check(task["broken"], task)
    rec["tool_calls"] += 1
    rec["precondition_broken"] = not pre_ok
    base = PROMPT.format(broken=task["broken"])
    prompt, patch, solved = base, "", False
    for a in range(args.attempts):
        raw, u = model.generate_sampled(prompt, args.max_new_tokens, rec["seed"] + a)
        for k in ("prompt_tokens", "completion_tokens", "gen_wall_s"):
            rec[k] += u[k]
        patch = extract_py(raw)
        ok, why = oracle_check(patch, task)
        rec["tool_calls"] += 1
        if ok:
            solved = True; rec["attempts_used"] = a + 1; break
        if a + 1 < args.attempts and arm == "on":
            prompt = base + FEEDBACK.format(prev=patch[:800], diag=why[:500])
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
    ap.add_argument("--calibrate", action="store_true")
    ap.add_argument("--per-arm", type=int, default=120)
    ap.add_argument("--attempts", type=int, default=3)
    ap.add_argument("--max-new-tokens", type=int, default=1600)
    ap.add_argument("--seed", type=int, default=66000)
    args = ap.parse_args()
    # Served model over HTTP, not a local load: the two inference servers
    # already on this box hold both GPUs, so a third in-process model blocks
    # forever on VRAM. See http_model.py.
    from http_model import HttpModel
    model = HttpModel()

    if args.calibrate:
        keep = []
        for t in TASKS:
            if oracle_check(t["broken"], t)[0]:
                continue                      # not actually broken
            p = PROMPT.format(broken=t["broken"])
            n = 6
            passes = sum(1 for s in range(n)
                         if oracle_check(extract_py(model.generate_sampled(p, args.max_new_tokens, 8800 + s)[0]), t)[0])
            rate = passes / n
            if rate <= 0.85:                  # exclude only a CEILINGED control
                t["control_pass_rate"] = rate
                keep.append(t)
            print(f"  {'KEEP' if rate <= 0.85 else 'drop'} {t['name']:<40} control={rate:.2f}")
        (HERE / "calibrated_tasks.json").write_text(json.dumps(keep, indent=2))
        print(f"\nadmitted {len(keep)}/{len(TASKS)}")
        return

    tasks = json.loads((HERE / "calibrated_tasks.json").read_text())
    out = HERE / "episodes5.jsonl"; out.write_text("")
    started, n = time.time(), 0
    for i in range(args.per_arm):
        t = tasks[i % len(tasks)]
        for arm in ("off", "on"):
            rec = episode(model, t, arm, n, args)
            with out.open("a") as f:
                f.write(json.dumps(rec) + "\n")
            n += 1
    meta = {"phase": 5, "episodes": n, "wall_min": round((time.time()-started)/60, 2),
            "source": "cpython-stdlib mutants, differential oracle",
            "attempts": args.attempts, "decoding": "sampled", "temperature": 0.7,
            "tasks": [t["name"] for t in tasks], "model": model.name}
    (HERE / "meta5.json").write_text(json.dumps(meta, indent=2))
    print(json.dumps(meta, indent=2))


if __name__ == "__main__":
    main()
