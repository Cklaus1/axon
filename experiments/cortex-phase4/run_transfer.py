#!/usr/bin/env python3
"""Phase 4 — the transfer test Phase 3b could not run.

Same arms as Phase 2/3a (`blind` vs `both`), on PYTHON tasks calibrated to a
non-ceilinged control. If grounded iteration only helps a language the model has
never seen, the effect should vanish here.
"""
import argparse, hashlib, json, os, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase1"))
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase3"))
os.environ.setdefault("HF_HUB_OFFLINE", "1")
import runner as R
from transfer import run_py, extract_py, PROMPT, FEEDBACK

HERE = pathlib.Path(__file__).parent
TASKS = json.loads((HERE / "py_tasks.json").read_text())


def episode(model, t, arm, idx, args):
    t0 = time.time()
    rec = {"episode": idx, "arm": arm, "task": t["name"], "language": "python",
           "model": model.name, "seed": args.seed + idx * 97,
           "control_pass_rate": t["control_pass_rate"],
           "max_attempts": args.attempts, "prompt_tokens": 0,
           "completion_tokens": 0, "gen_wall_s": 0.0, "tool_calls": 0}
    pre_ok, _ = run_py(t["broken"], t["hidden"], t["name"])
    rec["tool_calls"] += 1
    rec["precondition_broken"] = not pre_ok
    base = PROMPT.format(intent=t["intent"], broken=t["broken"])
    prompt, patch, solved = base, "", False
    for a in range(args.attempts):
        raw, u = model.generate_sampled(prompt, args.max_new_tokens, rec["seed"] + a)
        for k in ("prompt_tokens", "completion_tokens", "gen_wall_s"):
            rec[k] += u[k]
        patch = extract_py(raw)
        ok, out = run_py(patch, t["hidden"], t["name"])
        rec["tool_calls"] += 1
        if ok:
            solved = True; rec["attempts_used"] = a + 1; break
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
    ap.add_argument("--per-arm", type=int, default=100)
    ap.add_argument("--attempts", type=int, default=3)
    ap.add_argument("--max-new-tokens", type=int, default=500)
    ap.add_argument("--seed", type=int, default=55000)
    args = ap.parse_args()
    model = R.Model("Qwen/Qwen2.5-Coder-7B-Instruct")
    out = HERE / "transfer4.jsonl"; out.write_text("")
    started, n = time.time(), 0
    for i in range(args.per_arm):
        t = TASKS[i % len(TASKS)]
        for arm in ("off", "on"):
            rec = episode(model, t, arm, n, args)
            with out.open("a") as f:
                f.write(json.dumps(rec) + "\n")
            n += 1
    meta = {"phase": "4-transfer", "language": "python", "episodes": n,
            "wall_min": round((time.time() - started) / 60, 2),
            "attempts": args.attempts, "decoding": "sampled", "temperature": 0.7,
            "tasks": [t["name"] for t in TASKS],
            "task_control_rates": {t["name"]: t["control_pass_rate"] for t in TASKS},
            "model": model.name}
    (HERE / "transfer4_meta.json").write_text(json.dumps(meta, indent=2))
    print(json.dumps(meta, indent=2))


if __name__ == "__main__":
    main()
