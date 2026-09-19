#!/usr/bin/env python3
"""Phase-1 analysis. INTEGRITY FIRST — no outcome number is printed until the
integrity checks pass, because a comparison over a broken instrument is worse
than no comparison."""
import json, pathlib, sys
from collections import Counter, defaultdict

HERE = pathlib.Path(__file__).parent
rows = [json.loads(l) for l in (HERE / "episodes.jsonl").read_text().splitlines() if l.strip()]
meta = json.loads((HERE / "run_meta.json").read_text())

print("=" * 66)
print("PHASE 1 — INSTRUMENTATION INTEGRITY (the acceptance criteria)")
print("=" * 66)

checks = {}
checks["episodes_recorded_equals_reported"] = (len(rows) == meta["episodes_run"])
checks["arms_are_balanced"] = (
    Counter(r["arm"] for r in rows)["off"] == Counter(r["arm"] for r in rows)["on"]
)
checks["every_episode_evaluable"] = all(r["evaluable"] for r in rows)
checks["every_precondition_held"] = all(r["precondition_broken"] for r in rows)
REQUIRED = ["model", "weights_dtype", "quantization", "repo", "repo_commit", "task",
            "cortex_config", "seed", "max_new_tokens", "wall_s", "tool_calls",
            "retries_used", "outcome", "evaluable", "score", "failure_category",
            "prompt_tokens", "completion_tokens", "patch_sha256"]
checks["all_required_fields_present"] = all(
    all(k in r for k in REQUIRED) for r in rows
)
# Paired: each task must appear the same number of times in each arm, or a
# difference could come from task mix rather than treatment.
per = defaultdict(Counter)
for r in rows:
    per[r["task"]][r["arm"]] += 1
checks["tasks_paired_across_arms"] = all(c["off"] == c["on"] for c in per.values())
# The treatment must actually differ from the control at the input.
checks["arms_differ_in_prompt"] = all(
    next(x["prompt_chars"] for x in rows if x["task"] == t and x["arm"] == "on")
    != next(x["prompt_chars"] for x in rows if x["task"] == t and x["arm"] == "off")
    for t in per
)
# Determinism: identical (task, arm, seed) must give an identical patch.
seen = {}
dup_ok = True
for r in rows:
    k = (r["task"], r["arm"], r["seed"])
    if k in seen and seen[k] != r["patch_sha256"]:
        dup_ok = False
    seen[k] = r["patch_sha256"]
checks["same_seed_same_patch"] = dup_ok

for k, v in checks.items():
    print(f"  {'PASS' if v else '** FAIL **':<11} {k}")
integrity_ok = all(checks.values())
print(f"\n  INTEGRITY: {'PASS' if integrity_ok else 'FAIL'}")
if not integrity_ok:
    print("\nOutcome comparison SUPPRESSED — the instrument is not trustworthy yet.")
    sys.exit(1)

print()
print("=" * 66)
print("PAIRED OUTCOMES (identical model, tasks, tools, budgets)")
print("=" * 66)
by_arm = defaultdict(list)
for r in rows:
    by_arm[r["arm"]].append(r)
for arm in ("off", "on"):
    a = by_arm[arm]
    rep = sum(1 for r in a if r["outcome"] == "repaired")
    print(f"  cortex {arm.upper():<3}  n={len(a):<3} repaired={rep:<3} "
          f"({rep/len(a)*100:.0f}%)  tokens/ep={sum(r['completion_tokens'] for r in a)/len(a):.0f}  "
          f"wall/ep={sum(r['wall_s'] for r in a)/len(a):.2f}s  "
          f"tools/ep={sum(r['tool_calls'] for r in a)/len(a):.1f}")

print("\n  per-task (paired):")
for t in sorted(per):
    o = [r for r in rows if r["task"] == t and r["arm"] == "off"]
    n = [r for r in rows if r["task"] == t and r["arm"] == "on"]
    so, sn = sum(r["score"] for r in o), sum(r["score"] for r in n)
    same = all(
        a["patch_sha256"] == b["patch_sha256"] for a, b in zip(o, n)
    )
    print(f"    {t:<20} off={so:.0f}/{len(o)}  on={sn:.0f}/{len(n)}  identical_patch={same}")

print("\n  failure categories:")
for arm in ("off", "on"):
    c = Counter(r["failure_category"] for r in by_arm[arm] if r["failure_category"])
    print(f"    {arm}: {dict(c) or 'none'}")

ident = sum(
    1 for t in per
    for a, b in zip([r for r in rows if r["task"] == t and r["arm"] == "off"],
                    [r for r in rows if r["task"] == t and r["arm"] == "on"])
    if a["patch_sha256"] == b["patch_sha256"]
)
total_pairs = sum(len([r for r in rows if r["task"] == t and r["arm"] == "off"]) for t in per)
print(f"\n  pairs where ON and OFF produced a BYTE-IDENTICAL patch: {ident}/{total_pairs}")
