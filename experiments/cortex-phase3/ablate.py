#!/usr/bin/env python3
"""Phase 3a — component attribution, one variable at a time.

Phase 2's ON arm bundled TWO things into attempt N+1: the previous attempt, and
the compiler's diagnostic on it. The +27pp could come from either, or from
having any extra context at all. Four arms separate them:

  blind      same prompt again                      (Phase 2 OFF)
  prev       + your previous attempt, no diagnostic
  diag       + the diagnostic, WITHOUT the attempt
  both       + attempt and diagnostic               (Phase 2 ON)

`prev` is the control that matters: it adds context of the same SHAPE and
roughly the same length as `diag` but carries no information the model did not
already produce. If `prev` alone explains the gain, the effect is "more
context", not "grounded observation".
"""
import argparse, hashlib, json, os, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase1"))
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase2"))
os.environ.setdefault("HF_HUB_OFFLINE", "1")
import runner as R
from tasks_axon import TASKS

HERE = pathlib.Path(__file__).parent
BASE = """Repair this Axon program.

Intent: {intent}

```axon
{broken}
```

Axon is NOT Rust. Reply with ONLY the corrected program inside one ```axon code block. No prose."""

ARMS = ("blind", "prev", "diag", "both")


def build_followup(arm, base, prev, diag):
    if arm == "blind":
        return base
    if arm == "prev":
        return base + f"\n\nYour previous attempt was:\n```axon\n{prev[:700]}\n```\nIt was not accepted. Reply with ONLY the corrected program in one ```axon block."
    if arm == "diag":
        return base + f"\n\nThe Axon compiler REJECTED your previous attempt:\n{diag[:700]}\n\nUse that diagnostic. Reply with ONLY the corrected program in one ```axon block."
    return base + f"\n\nYour previous attempt was:\n```axon\n{prev[:700]}\n```\nThe Axon compiler REJECTED it:\n{diag[:700]}\n\nUse that diagnostic. Reply with ONLY the corrected program in one ```axon block."


def episode(model, task, arm, idx, args):
    name, broken, hidden, intent = task
    t0 = time.time()
    rec = {"episode": idx, "arm": arm, "task": name, "model": model.name,
           "seed": args.seed + idx * 97, "max_attempts": args.attempts,
           "prompt_tokens": 0, "completion_tokens": 0, "gen_wall_s": 0.0,
           "tool_calls": 0, "repo": "axon", "repo_commit": R.repo_commit()}
    pre_ok, _ = R.run_axon_test(broken, hidden, name)
    rec["tool_calls"] += 1
    rec["precondition_broken"] = not pre_ok
    base = BASE.format(intent=intent, broken=broken)
    prompt, patch, solved = base, "", False
    for a in range(args.attempts):
        raw, u = model.generate_sampled(prompt, args.max_new_tokens, rec["seed"] + a)
        for k in ("prompt_tokens", "completion_tokens", "gen_wall_s"):
            rec[k] += u[k]
        patch, _ = R.extract_code(raw)
        ok, out = R.run_axon_test(patch, hidden, name)
        rec["tool_calls"] += 1
        if ok:
            solved = True
            rec["attempts_used"] = a + 1
            break
        if a + 1 < args.attempts:
            chk_ok, chk_out = R.axon_check(patch, name)
            rec["tool_calls"] += 1          # every arm pays it; only some SEE it
            prompt = build_followup(arm, base, patch, chk_out if not chk_ok else out)
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
    ap.add_argument("--seed", type=int, default=31000)
    ap.add_argument("--max-wall-min", type=float, default=240.0)
    args = ap.parse_args()
    model = R.Model("Qwen/Qwen2.5-Coder-7B-Instruct")
    out = HERE / "ablation.jsonl"; out.write_text("")
    started, n, stop = time.time(), 0, "episode_budget"
    # Every arm sees the same task in the same round, so a stop cannot bias the
    # arm mix toward easier tasks.
    for i in range(args.per_arm):
        task = TASKS[i % len(TASKS)]
        for arm in ARMS:
            if (time.time() - started) / 60 >= args.max_wall_min:
                stop = "wall_clock_budget"; break
            rec = episode(model, task, arm, n, args)
            with out.open("a") as f:
                f.write(json.dumps(rec) + "\n")
            n += 1
        else:
            continue
        break
    meta = {"phase": "3a-ablation", "episodes": n, "stop_reason": stop,
            "wall_min": round((time.time() - started) / 60, 2), "arms": list(ARMS),
            "attempts": args.attempts, "decoding": "sampled", "temperature": 0.7,
            "tasks": [t[0] for t in TASKS], "model": model.name}
    (HERE / "ablation_meta.json").write_text(json.dumps(meta, indent=2))
    print(json.dumps(meta, indent=2))


if __name__ == "__main__":
    main()
