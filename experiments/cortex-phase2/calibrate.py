#!/usr/bin/env python3
"""Measure each candidate's CONTROL pass-rate, keep only the usable band.

Two gates, in order:
  1. VALIDITY — broken fails visible AND hidden; reference fix passes both.
     A task failing this measures nothing, whatever its difficulty.
  2. DIFFICULTY — the control (Cortex OFF) must land in [lo, hi]. At 100% there
     is no headroom to detect improvement; at 0% nothing can be learned either.

Sampling is at temperature so repeats are genuine samples, not the same greedy
run counted N times — the defect that made Phase 1's n=50 an effective n=10.
"""
import json, os, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase1"))
os.environ.setdefault("HF_HUB_OFFLINE", "1")
import runner as R
from candidates import CANDIDATES

HERE = pathlib.Path(__file__).parent
SAMPLES = int(os.environ.get("CAL_SAMPLES", "8"))
LO, HI = 0.25, 0.85     # keep tasks the control neither always nor never solves


def main():
    model = R.Model("Qwen/Qwen2.5-Coder-7B-Instruct")
    out_dir = HERE / "tasks"
    out_dir.mkdir(exist_ok=True)
    report = []
    for name, broken, fixed, vis, hid, intent in CANDIDATES:
        valid = {
            "broken_fails_visible": not R.run_axon_test(broken, vis, name)[0],
            "broken_fails_hidden": not R.run_axon_test(broken, hid, name)[0],
            "fixed_passes_visible": R.run_axon_test(fixed, vis, name)[0],
            "fixed_passes_hidden": R.run_axon_test(fixed, hid, name)[0],
        }
        if not all(valid.values()):
            report.append({"name": name, "admitted": False,
                           "reason": "invalid", "failed": [k for k, v in valid.items() if not v]})
            continue

        prompt = R.PROMPT_OFF.format(intent=intent, broken=broken)
        passes, distinct = 0, set()
        for s in range(SAMPLES):
            raw, _ = model.generate_sampled(prompt, 320, 4000 + s)
            code, _ = R.extract_code(raw)
            distinct.add(code)
            if R.run_axon_test(code, hid, name)[0]:
                passes += 1
        rate = passes / SAMPLES
        admitted = LO <= rate <= HI
        if admitted:
            (out_dir / f"{name}.json").write_text(json.dumps({
                "name": name, "intent": intent, "broken": broken,
                "reference_fix": fixed, "visible_test": vis, "hidden_test": hid,
                "control_pass_rate": rate, "calibration_samples": SAMPLES,
            }, indent=2))
        report.append({"name": name, "admitted": admitted,
                       "control_pass_rate": rate,
                       "distinct_replies": len(distinct),
                       "reason": None if admitted else ("too_easy" if rate > HI else "too_hard")})
    (HERE / "calibration.json").write_text(json.dumps(
        {"band": [LO, HI], "samples_per_task": SAMPLES, "tasks": report}, indent=2))
    for r in report:
        mark = "KEEP" if r["admitted"] else "drop"
        rate = r.get("control_pass_rate")
        rs = f"{rate:.2f}" if rate is not None else " -- "
        print(f"  {mark:<5} {r['name']:<24} control={rs}  "
              f"distinct={r.get('distinct_replies','-')}  {r.get('reason') or ''}")
    print(f"\nadmitted: {sum(1 for r in report if r['admitted'])}/{len(report)}")


if __name__ == "__main__":
    main()
