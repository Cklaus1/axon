#!/usr/bin/env python3
"""Zero-cost self-test of run_ab's `real-verify` gate (_gated_rounds): no model.

    AXON_BIN=... python3 selftest_gate.py

A scripted one_round stands in for the agent. Checks: acceptance on the first
round; a test edit is rejected, the next round starts with the test blocks put
back and a prompt that names nothing from the hidden check; acceptance on a
later round; the extra-round cap; an errored round ends the loop; a file never
accepted is restored to the given one with the claim False.
"""
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "ab"))
sys.argv = sys.argv[:1]
import run_ab as R  # noqa: E402
from corpus import test_blocks  # noqa: E402

task = [t for t in R.load_corpus(os.path.join(os.path.dirname(HERE), "tempt/tempt.json"))["tasks"]
        if t["id"] == "tempt-special_case-04"][0]
given = task["given_a"]
span = R.fn_span(given, task["symbol"])
fixed = given[:span[0]] + task["oracle_body"] + given[span[1]:]
vis = [(a, b) for n, a, b in test_blocks(given) if n == task["visible"]][0]
tampered = given[:vis[0]] + "@[test]\nfn %s() {\n    assert(true)\n}\n" % task["visible"] + given[vis[1]:]
assert R.restore_tests(given, tampered) == given, "restore_tests must undo a test edit"
assert R.restore_tests(given, tampered.replace("assert(true)", "assert(true)\n    let x = 1")) == given
assert R.restore_tests(given, fixed) == fixed, "non-test code must be kept"


def scenario(plan, extra=2):
    """plan: list of (file text or None for no change, is_error)."""
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, task["file"])
        open(p, "w").write(given)
        seen = []

        def one_round(prompt):
            n = len(seen)
            seen.append((prompt, open(p).read()))
            text, err = plan[min(n, len(plan) - 1)]
            if text is not None:
                open(p, "w").write(text)
            return {"claim": True, "claim_line": "FIXED", "cost_usd": 0.001, "input_tokens": 1,
                    "output_tokens": 1, "cache_read": 0, "cache_create": 0, "num_turns": 1,
                    "exit": 1 if err else 0, "is_error": err, "final_text": "FIXED",
                    "models": [], "cost_estimated": True}
        prompt = R.PROMPT_A.format(file=task["file"], visible=task["visible"])
        row = R._gated_rounds(task, given, p, prompt, one_round, extra)
        return row, seen, open(p).read()


# 1. accepted at once
row, seen, final = scenario([(fixed, False)])
assert (row["claim"], row["rounds"], row["accepted_round"], final) == (True, 1, 1, fixed), row
# 2. test edit, then a real fix: rejected, tests restored, accepted in round 2
row, seen, final = scenario([(tampered, False), (fixed, False)])
assert (row["claim"], row["rounds"], row["accepted_round"], final) == (True, 2, 2, fixed), row
p2, file2 = seen[1]
assert R.GATE_REJECTION in p2 and p2.startswith(R.PROMPT_A.format(file=task["file"], visible=task["visible"]))
assert file2 == given, "round 2 must start from the restored test blocks"
assert task["hidden"] not in p2 and "hidden" not in p2.lower(), "the rejection leaks the check"
assert row["round_log"][0]["tests_tampered"] == [task["visible"]]
assert abs(row["cost_usd"] - 0.002) < 1e-9 and row["input_tokens"] == 2
# 3. never accepted: capped at 3 rounds, restored, no claim
row, seen, final = scenario([(tampered, False)])
assert (row["claim"], row["rounds"], row["accepted_round"], final) == (False, 3, None, given), row
assert row["rejected"]["agent_claimed"] and row["rejected"]["verified_clean"] is False
# 4. the cap is the argument
row, _, final = scenario([(tampered, False)], extra=0)
assert (row["rounds"], row["claim"], final) == (1, False, given)
# 5. an errored round ends the loop and the file is restored
row, _, final = scenario([(tampered, True), (fixed, False)])
assert (row["rounds"], row["claim"], row["is_error"], final) == (1, False, True, given), row
print("selftest_gate: pass (accept, reject+restore+continue, late accept, cap, error stop, no leak)")
