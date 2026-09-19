#!/usr/bin/env python3
"""Phase-1 paired episode runner: Cortex-OFF baseline vs Cortex-ON treatment.

ACCEPTANCE CRITERIA for Phase 1 are instrumentation integrity and
reproducibility — NOT solver improvement. Nothing here reports whether Cortex
helps; `analyze.py` does that, and only once the integrity checks pass.

The two arms differ in ONE thing: whether the model is given grounded
observation + independent verification (ON) or the broken program alone (OFF).
Model, weights, tasks, tools, decoding, token budget and retry count are held
identical, because otherwise any difference could come from those instead.
"""
import argparse, json, os, pathlib, subprocess, sys, tempfile, time, hashlib

os.environ.setdefault("HF_HUB_OFFLINE", "1")
AXON = "/home/cklaus/projects/axon/target/debug/axon"
HERE = pathlib.Path(__file__).parent


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=120, **kw)


def repo_commit():
    r = sh(["git", "-C", "/home/cklaus/projects/axon", "rev-parse", "HEAD"])
    return r.stdout.strip()[:12]


def axon_version():
    return sh([AXON, "--version"]).stdout.strip()


class Model:
    """Greedy, seeded, single local model. Deterministic by construction."""

    def __init__(self, name):
        import torch
        from transformers import AutoModelForCausalLM, AutoTokenizer
        self.torch = torch
        self.name = name
        t0 = time.time()
        self.tok = AutoTokenizer.from_pretrained(name)
        self.model = AutoModelForCausalLM.from_pretrained(
            name, dtype=torch.bfloat16, device_map="cuda:0"
        )
        self.load_s = time.time() - t0
        self.dtype = "bfloat16"

    def generate(self, prompt, max_new_tokens, seed):
        msgs = [{"role": "user", "content": prompt}]
        text = self.tok.apply_chat_template(msgs, tokenize=False, add_generation_prompt=True)
        ins = self.tok(text, return_tensors="pt").to(self.model.device)
        self.torch.manual_seed(seed)
        t0 = time.time()
        with self.torch.no_grad():
            out = self.model.generate(
                **ins, max_new_tokens=max_new_tokens, do_sample=False,
                pad_token_id=self.tok.eos_token_id,
            )
        wall = time.time() - t0
        gen = out[0][ins["input_ids"].shape[1]:]
        return (
            self.tok.decode(gen, skip_special_tokens=True),
            {"prompt_tokens": int(ins["input_ids"].shape[1]),
             "completion_tokens": int(gen.shape[0]),
             "gen_wall_s": round(wall, 3)},
        )


def _sampled(self, prompt, max_new_tokens, seed, temperature=0.7, top_p=0.95):
    """Temperature sampling with a RECORDED seed.

    Phase 1 used greedy decoding, where the seed is inert — repeated episodes of
    one (task, arm) reproduced the same bytes, so n=50 was an effective n=10.
    Sampling makes a repeat a genuine sample; recording the seed keeps it
    reproducible.
    """
    msgs = [{"role": "user", "content": prompt}]
    text = self.tok.apply_chat_template(msgs, tokenize=False, add_generation_prompt=True)
    ins = self.tok(text, return_tensors="pt").to(self.model.device)
    self.torch.manual_seed(seed)
    t0 = time.time()
    with self.torch.no_grad():
        out = self.model.generate(
            **ins, max_new_tokens=max_new_tokens, do_sample=True,
            temperature=temperature, top_p=top_p,
            pad_token_id=self.tok.eos_token_id,
        )
    wall = time.time() - t0
    gen = out[0][ins["input_ids"].shape[1]:]
    return (
        self.tok.decode(gen, skip_special_tokens=True),
        {"prompt_tokens": int(ins["input_ids"].shape[1]),
         "completion_tokens": int(gen.shape[0]),
         "gen_wall_s": round(wall, 3),
         "decoding": "sampled", "temperature": temperature, "top_p": top_p},
    )


Model.generate_sampled = _sampled


def extract_code(raw):
    """Take the first fenced block, else the whole reply. Recorded either way."""
    if "```" in raw:
        parts = raw.split("```")
        if len(parts) >= 2:
            body = parts[1]
            if body.startswith("axon"):
                body = body[4:]
            elif body.startswith("rust"):
                body = body[4:]
            return body.strip(), True
    return raw.strip(), False


def run_axon_test(body, tests, tag):
    with tempfile.TemporaryDirectory() as d:
        p = pathlib.Path(d, f"{tag}.ax")
        p.write_text(body + "\n" + tests)
        r = sh([AXON, "test", str(p)])
        return r.returncode == 0, (r.stdout + r.stderr)[:2000]


def axon_check(body, tag):
    """The grounded observation the ON arm gets. Real compiler output."""
    with tempfile.TemporaryDirectory() as d:
        p = pathlib.Path(d, f"{tag}.ax")
        p.write_text(body)
        r = sh([AXON, "check", str(p)])
        return r.returncode == 0, (r.stdout + r.stderr)[:1500]


PROMPT_OFF = """Repair this Axon program.

Intent: {intent}

```axon
{broken}
```

Reply with ONLY the corrected function inside one ```axon code block. No prose."""

PROMPT_ON = """Repair this Axon program.

Intent: {intent}

OBSERVED STATE (from the compiler, not a guess):
- type check: {check_state}
- visible test `{vis_name}`: FAILING
- compiler output:
{check_out}

FAILING TEST OUTPUT:
{test_out}

```axon
{broken}
```

Your patch will be applied to a COPY and verified against tests you cannot see,
so a change that only satisfies the visible test will not pass.

Reply with ONLY the corrected function inside one ```axon code block. No prose."""


def episode(model, task, arm, ep_idx, args):
    """One episode. Every field the protocol requires is recorded, including
    the ones that say the episode was unusable."""
    t0 = time.time()
    rec = {
        "episode": ep_idx,
        "arm": arm,
        "task": task["name"],
        "model": model.name,
        "weights_dtype": model.dtype,
        "quantization": "none",
        "repo": "axon",
        "repo_commit": repo_commit(),
        "axon_version": axon_version(),
        "cortex_config": ("observation+independent_verification" if arm == "on" else "none"),
        "seed": args.seed + ep_idx,
        "max_new_tokens": args.max_new_tokens,
        "max_retries": args.retries,
        "tool_calls": 0,
        "retries_used": 0,
    }

    # Pre-state. Both arms pay for it; only ON is SHOWN it — otherwise the arms
    # would differ in compute as well as in information.
    chk_ok, chk_out = axon_check(task["broken"], task["name"])
    vis_ok, vis_out = run_axon_test(task["broken"], task["visible_test"], task["name"])
    rec["tool_calls"] += 2
    rec["precondition_broken"] = (not vis_ok)

    prompt = (PROMPT_ON if arm == "on" else PROMPT_OFF).format(
        intent=task["intent"], broken=task["broken"],
        check_state=("clean" if chk_ok else "errors"),
        check_out=chk_out[:600], test_out=vis_out[:600], vis_name="t_visible",
    )
    rec["prompt_chars"] = len(prompt)

    attempt, patched, usage = 0, None, {}
    while attempt <= args.retries:
        raw, usage = model.generate(prompt, args.max_new_tokens, rec["seed"] + attempt)
        code, fenced = extract_code(raw)
        rec["fenced_reply"] = fenced
        # Applied to a COPY; the task's own source is never mutated.
        patched = code
        vis_pass, vis_o = run_axon_test(patched, task["visible_test"], task["name"])
        rec["tool_calls"] += 1
        if vis_pass or attempt == args.retries:
            rec["visible_pass"] = vis_pass
            rec["visible_output"] = vis_o[:400]
            break
        attempt += 1
        rec["retries_used"] = attempt

    # INDEPENDENT verification — the hidden tests, run regardless of what the
    # visible one said and regardless of any claim in the reply.
    hid_pass, hid_o = run_axon_test(patched, task["hidden_test"], task["name"])
    rec["tool_calls"] += 1
    rec["hidden_pass"] = hid_pass
    rec["hidden_output"] = hid_o[:400]

    rec.update(usage)
    rec["wall_s"] = round(time.time() - t0, 3)
    rec["patch_sha256"] = hashlib.sha256((patched or "").encode()).hexdigest()[:16]
    rec["patch_chars"] = len(patched or "")

    # Outcome + failure category. "evaluable" is the integrity flag: an episode
    # whose precondition did not hold measures nothing and must not be scored.
    rec["evaluable"] = bool(rec["precondition_broken"])
    if not rec["evaluable"]:
        rec["outcome"], rec["failure_category"] = "unevaluable", "precondition_not_broken"
    elif hid_pass:
        rec["outcome"], rec["failure_category"] = "repaired", None
    elif rec.get("visible_pass"):
        rec["outcome"], rec["failure_category"] = "overfit_visible", "passes_visible_fails_hidden"
    elif "error" in rec["visible_output"].lower() or "E0" in rec["visible_output"]:
        rec["outcome"], rec["failure_category"] = "not_repaired", "invalid_program"
    else:
        rec["outcome"], rec["failure_category"] = "not_repaired", "wrong_behavior"
    rec["score"] = 1.0 if hid_pass else 0.0
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="Qwen/Qwen2.5-Coder-7B-Instruct")
    ap.add_argument("--episodes", type=int, default=50)
    ap.add_argument("--max-wall-min", type=float, default=120.0)
    ap.add_argument("--max-new-tokens", type=int, default=320)
    ap.add_argument("--retries", type=int, default=0)
    ap.add_argument("--seed", type=int, default=1000)
    ap.add_argument("--out", default=str(HERE / "episodes.jsonl"))
    args = ap.parse_args()

    tasks = [json.loads(p.read_text()) for p in sorted((HERE / "tasks").glob("*.json"))]
    if not tasks:
        print("no tasks — run gen_tasks.py", file=sys.stderr)
        return 2

    model = Model(args.model)
    out = pathlib.Path(args.out)
    out.write_text("")
    started = time.time()
    n = 0
    stop_reason = "episode_budget"
    # PAIRED: the same task index runs in both arms back to back, so a stop on
    # the wall-clock budget cannot leave one arm with more of the easy tasks.
    for i in range(args.episodes // 2):
        task = tasks[i % len(tasks)]
        for arm in ("off", "on"):
            if (time.time() - started) / 60.0 >= args.max_wall_min:
                stop_reason = "wall_clock_budget"
                break
            rec = episode(model, task, arm, n, args)
            with out.open("a") as f:
                f.write(json.dumps(rec) + "\n")
            n += 1
            print(f"  ep{n:3d} {arm:3s} {task['name']:<20} {rec['outcome']:<16} "
                  f"{rec['wall_s']:>6.1f}s", flush=True)
        else:
            continue
        break

    meta = {
        "phase": 1, "episodes_run": n, "stop_reason": stop_reason,
        "wall_min": round((time.time() - started) / 60.0, 2),
        "model": args.model, "model_load_s": round(model.load_s, 1),
        "episode_budget": args.episodes, "wall_budget_min": args.max_wall_min,
        "decoding": "greedy", "seed_base": args.seed,
        "max_new_tokens": args.max_new_tokens, "retries": args.retries,
        "tasks": [t["name"] for t in tasks],
    }
    (HERE / "run_meta.json").write_text(json.dumps(meta, indent=2))
    print(json.dumps(meta, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
