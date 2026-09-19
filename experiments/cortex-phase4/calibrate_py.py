#!/usr/bin/env python3
"""Measure each Python candidate's CONTROL pass-rate; keep the usable band."""
import json, os, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase1"))
sys.path.insert(0, str(pathlib.Path(__file__).parent.parent / "cortex-phase3"))
os.environ.setdefault("HF_HUB_OFFLINE", "1")
import runner as R
from transfer import run_py, extract_py, PROMPT
from py_candidates import C

HERE = pathlib.Path(__file__).parent
LO, HI, N = 0.0, 0.85, 8

def main():
    model = R.Model("Qwen/Qwen2.5-Coder-7B-Instruct")
    keep, rep = [], []
    for name, broken, hidden, intent in C:
        if run_py(broken, hidden, name)[0]:
            rep.append({"name": name, "admitted": False, "reason": "not_broken"}); continue
        p = PROMPT.format(intent=intent, broken=broken)
        passes = sum(1 for s in range(N)
                     if run_py(extract_py(model.generate_sampled(p, 400, 9000 + s)[0]), hidden, name)[0])
        rate = passes / N
        ok = LO <= rate <= HI
        if ok:
            keep.append({"name": name, "broken": broken, "hidden": hidden,
                         "intent": intent, "control_pass_rate": rate})
        rep.append({"name": name, "admitted": ok, "control_pass_rate": rate,
                    "reason": None if ok else ("too_easy" if rate > HI else "too_hard")})
    (HERE / "py_tasks.json").write_text(json.dumps(keep, indent=2))
    (HERE / "py_calibration.json").write_text(json.dumps(rep, indent=2))
    for r in rep:
        rt = r.get("control_pass_rate")
        print(f"  {'KEEP' if r['admitted'] else 'drop':<5} {r['name']:<22} "
              f"control={('%.2f' % rt) if rt is not None else ' -- '}  {r.get('reason') or ''}")
    print(f"\nadmitted {len(keep)}/{len(C)}")

if __name__ == "__main__":
    main()
