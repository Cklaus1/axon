#!/usr/bin/env python3
"""Phase 2: does GROUNDED ITERATION beat BLIND ITERATION at equal budget?

Both arms get the same model, tasks, tools, decoding, token budget and the SAME
NUMBER OF ATTEMPTS. The single difference:

  OFF  attempt N+1 sees the same prompt as attempt N. Blind retry.
  ON   attempt N+1 sees the compiler's real verdict on attempt N. Grounded.

Holding the attempt count equal is what makes this a test of the INFORMATION
rather than of extra compute. Tokens and wall-clock are recorded per arm so the
cost of the longer ON prompt is visible rather than assumed away.
"""
import argparse, hashlib, json, os, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase1"))
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

FEEDBACK = """
Your previous attempt was:
```axon
{prev}
```
The Axon compiler REJECTED it:
{diag}

Use that diagnostic. Reply with ONLY the corrected program in one ```axon block."""


def episode(model, task, arm, ep_idx, args):
    name, broken, hidden, intent = task
    t0 = time.time()
    rec = {
        "episode": ep_idx, "arm": arm, "task": name,
        "model": model.name, "weights_dtype": model.dtype, "quantization": "none",
        "repo": "axon", "repo_commit": R.repo_commit(), "axon_version": R.axon_version(),
        "cortex_config": "grounded_iteration" if arm == "on" else "blind_iteration",
        "seed": args.seed + ep_idx * 97, "max_new_tokens": args.max_new_tokens,
        "max_attempts": args.attempts, "tool_calls": 0, "retries_used": 0,
        "prompt_tokens": 0, "completion_tokens": 0, "gen_wall_s": 0.0,
    }
    # Precondition: the task must really be broken, or the episode measures
    # nothing and must not be scored.
    pre_ok, _ = R.run_axon_test(broken, hidden, name)
    rec["tool_calls"] += 1
    rec["precondition_broken"] = not pre_ok

    prompt = BASE.format(intent=intent, broken=broken)
    patch, solved, attempts_used = "", False, 0
    for a in range(args.attempts):
        attempts_used = a + 1
        raw, usage = model.generate_sampled(prompt, args.max_new_tokens, rec["seed"] + a)
        rec["prompt_tokens"] += usage["prompt_tokens"]
        rec["completion_tokens"] += usage["completion_tokens"]
        rec["gen_wall_s"] += usage["gen_wall_s"]
        patch, _ = R.extract_code(raw)
        ok, out = R.run_axon_test(patch, hidden, name)
        rec["tool_calls"] += 1
        if ok:
            solved = True
            break
        if a + 1 < args.attempts:
            rec["retries_used"] += 1
            if arm == "on":
                # The ONLY difference between the arms.
                chk_ok, chk_out = R.axon_check(patch, name)
                rec["tool_calls"] += 1
                diag = chk_out if not chk_ok else out
                prompt = BASE.format(intent=intent, broken=broken) + FEEDBACK.format(
                    prev=patch[:700], diag=diag[:700])
            # OFF: prompt unchanged — a blind retry at the same budget.

    rec["attempts_used"] = attempts_used
    rec["hidden_pass"] = solved
    rec["score"] = 1.0 if solved else 0.0
    rec["patch_sha256"] = hashlib.sha256(patch.encode()).hexdigest()[:16]
    rec["patch_chars"] = len(patch)
    rec["wall_s"] = round(time.time() - t0, 3)
    rec["gen_wall_s"] = round(rec["gen_wall_s"], 3)
    rec["evaluable"] = bool(rec["precondition_broken"])
    if not rec["evaluable"]:
        rec["outcome"], rec["failure_category"] = "unevaluable", "precondition_not_broken"
    elif solved:
        rec["outcome"], rec["failure_category"] = "repaired", None
    else:
        chk_ok, _ = R.axon_check(patch, name)
        rec["outcome"] = "not_repaired"
        rec["failure_category"] = "wrong_behavior" if chk_ok else "invalid_program"
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="Qwen/Qwen2.5-Coder-7B-Instruct")
    ap.add_argument("--episodes", type=int, default=200)
    ap.add_argument("--max-wall-min", type=float, default=480.0)
    ap.add_argument("--max-new-tokens", type=int, default=400)
    ap.add_argument("--attempts", type=int, default=3)
    ap.add_argument("--seed", type=int, default=20000)
    ap.add_argument("--out", default=str(HERE / "episodes.jsonl"))
    args = ap.parse_args()

    model = R.Model(args.model)
    out = pathlib.Path(args.out); out.write_text("")
    started, n, stop = time.time(), 0, "episode_budget"
    for i in range(args.episodes // 2):
        task = TASKS[i % len(TASKS)]
        for arm in ("off", "on"):
            if (time.time() - started) / 60.0 >= args.max_wall_min:
                stop = "wall_clock_budget"; break
            rec = episode(model, task, arm, n, args)
            with out.open("a") as f:
                f.write(json.dumps(rec) + "\n")
            n += 1
            print(f"  ep{n:3d} {arm:3s} {task[0]:<18} {rec['outcome']:<13} "
                  f"att={rec['attempts_used']} {rec['wall_s']:>5.1f}s", flush=True)
        else:
            continue
        break
    meta = {"phase": 2, "episodes_run": n, "stop_reason": stop,
            "wall_min": round((time.time() - started) / 60.0, 2),
            "model": args.model, "episode_budget": args.episodes,
            "wall_budget_min": args.max_wall_min, "decoding": "sampled",
            "temperature": 0.7, "top_p": 0.95, "attempts_per_episode": args.attempts,
            "max_new_tokens": args.max_new_tokens, "seed_base": args.seed,
            "tasks": [t[0] for t in TASKS]}
    (HERE / "run_meta.json").write_text(json.dumps(meta, indent=2))
    print(json.dumps(meta, indent=2))


if __name__ == "__main__":
    main()
