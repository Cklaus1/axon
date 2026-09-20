#!/usr/bin/env python3
"""Report the real-model run as three rates, not one headline.

    overall repair
      = localization coverage
      x generator success GIVEN the target was attempted
      x controller acceptance correctness

A single "repair rate" hides which factor moved. A failure whose true target
was never among the candidates is a localization result; counting it against
the generator would send the next round of work to the wrong component.

Successes are split four ways because 70% can mean excellent first-shot
reasoning or mediocre first-shot reasoning with an excellent recovery loop,
and those imply opposite investments.
"""
import json
import statistics
import sys

data = json.load(open(sys.argv[1] if len(sys.argv) > 1 else "real_model.json"))
meta, trials = data["meta"], data["trials"]
n = len(trials)
if not n:
    print("no trials")
    raise SystemExit(1)


def pct(a, b):
    return f"{100*a/b:5.1f}%" if b else "    n/a"


print(f"Real model: {meta['model']}")
print(f"Cortex commit {meta['cortex_commit'][:12]} · driver {meta['driver_sha256_16']} "
      f"· candidates={meta['candidates']}")
print(f"Trials: {n}\n")

# 1. LOCALIZATION COVERAGE — was the true symbol ever put in front of the
#    generator at all?
attempted = [t for t in trials if t["target_attempted"]]
print(f"  true target among attempted candidates : {len(attempted)}/{n}  {pct(len(attempted), n)}")

# 2. GENERATOR SUCCESS GIVEN THE TARGET WAS AVAILABLE — the term this
#    experiment exists to estimate.
solved = [t for t in trials if t["exit"] == 0 and t["file_clean"]]
solved_attempted = [t for t in attempted if t["exit"] == 0 and t["file_clean"]]
print(f"  generator success | target attempted   : {len(solved_attempted)}/{len(attempted)}"
      f"  {pct(len(solved_attempted), len(attempted))}")

def segments(t):
    """Proposals grouped by the candidate they were made for.

    The rejected-attempts list is per-episode, so a `priors` count returning
    to 0 marks a new candidate. Segment i corresponds to `attempted[i]`.
    """
    segs, cur = [], []
    for c in t["per_proposal"]:
        if (c["priors"] or 0) == 0 and cur:
            segs.append(cur)
            cur = []
        cur.append(c)
    if cur:
        segs.append(cur)
    return segs


def split_on_target(t):
    """(before, on) — proposals spent reaching the true target, and on it.

    Everything before the target's own episode is LOCALIZATION-SEARCH cost,
    not generator recovery: each candidate episode resets to its baseline and
    the rejected bodies are candidate-local, so nothing a wrong candidate
    learned crosses over. Calling those "wasted" is the right reading for THIS
    design, and would stop being right if feedback ever crossed candidates.
    """
    segs = segments(t)
    if not t["target_attempted"] or t["symbol"] not in t["attempted"]:
        return [c for seg in segs for c in seg], []
    i = t["attempted"].index(t["symbol"])
    before = [c for seg in segs[:i] for c in seg]
    on = segs[i] if i < len(segs) else []
    return before, on


for t in trials:
    _b, _o = split_on_target(t)
    t["_before"], t["_on"] = _b, _o
    t["_final_proposals"] = len(_o) if _o else len(_b)
    t["_cost_before"] = sum(c["cost_usd"] or 0 for c in _b)
    t["_cost_on"] = sum(c["cost_usd"] or 0 for c in _o)


# 3. RECOVERY — does the feedback loop earn its complexity?
multi = [t for t in attempted if len(t["_on"]) > 1]
recovered = [t for t in multi if t["exit"] == 0 and t["file_clean"]]
print(f"  recovery | first proposal rejected     : {len(recovered)}/{len(multi)}"
      f"  {pct(len(recovered), len(multi))}\n")

# PROPOSALS ARE ATTRIBUTED TO CANDIDATES, not summed across the run.
#
# A trial at localization rank 2 spends proposals on the WRONG candidate
# first. Counting `proposals > 1` as "recovered after feedback" would credit
# the retry loop for work the generator never needed — it may have first-shot
# the correct target after the walk moved on. Cost of a bad rank and
# difficulty for the generator are different things, and this is where they
# get confused.
#
# The rejected-attempts list is per-episode, so `priors` returning to 0 marks
# the start of a new candidate. The last segment is the candidate that was
# live when the run ended — the one a verdict is about.
first_shot = [t for t in solved_attempted if t["_final_proposals"] == 1]
after_fb = [t for t in solved_attempted if t["_final_proposals"] > 1]
failed_fb = [t for t in attempted if t not in solved_attempted and t["_final_proposals"] > 1]
gen_err = [t for t in trials if t["outcome"] == "needs_input"]
print(f"    first-shot repair       : {len(first_shot)}")
print(f"    recovered after feedback: {len(after_fb)}")
print(f"    failed after feedback   : {len(failed_fb)}")
print(f"    generator unavailable   : {len(gen_err)}\n")

# THE INCREMENTAL VALUE OF RECOVERY. If most verified repairs required a
# rejected proposal first, the loop is not wrapping a model call — it is
# converting a mediocre first attempt into a verified solution by structured
# iteration, and that is the part worth its complexity.
if solved_attempted:
    print(f"  recovery's share of all verified repairs: {len(after_fb)}/{len(solved_attempted)}"
          f"  {pct(len(after_fb), len(solved_attempted))}\n")

# RANK x OUTCOME. Does a lower-ranked candidate merely cost more proposals, or
# does it also make the generator likelier to fail? Cost and capability are
# different problems with different fixes.
print("  localization rank vs outcome:")
print(f"    {'rank':<8}{'first-shot':>11}{'recovered':>11}{'failed':>9}")
for r in (1, 2, 3):
    at_r = [t for t in attempted if t["localization_rank"] == r]
    fs = sum(1 for t in at_r if t in solved_attempted and t["_final_proposals"] == 1)
    rc = sum(1 for t in at_r if t in solved_attempted and t["_final_proposals"] > 1)
    fl = len(at_r) - fs - rc
    if at_r:
        print(f"    {r:<8}{fs:>11}{rc:>11}{fl:>9}")
absent = [t for t in trials if not t["target_attempted"]]
if absent:
    print(f"    {'>3/none':<8}{'—':>11}{'—':>11}{len(absent):>9}   (never put to the generator)")
print()

false_succ = [t for t in trials if t["exit"] == 0 and not t["file_clean"]]
dirty = [t for t in trials if t["exit"] != 0 and not t["workspace_restored"]]
print(f"  verified AND file clean : {len(solved)}/{n}  {pct(len(solved), n)}")
print(f"  FALSE SUCCESSES         : {len(false_succ)}")
print(f"  workspace dirty on fail : {len(dirty)}\n")

if solved:
    props = [t["proposals"] for t in solved]
    costs = [t["cost_usd"] for t in solved]
    walls = [t["wall_s"] for t in solved]
    print(f"  median proposals / solve: {statistics.median(props):.0f} total, "
          f"{statistics.median([len(t['_on']) for t in solved]):.0f} on the true target")
    print(f"  median cost / solve     : ${statistics.median(costs):.3f}")
    print(f"  median wall / solve     : {statistics.median(walls):.0f}s")
print(f"  total spend             : ${sum(t['cost_usd'] for t in trials):.2f}\n")

# WHERE THE MONEY GOES: finding the function, or fixing it.
#
# Two different optimisations hide behind one "repair cost". Coverage
# optimisation puts the truth somewhere in the top-k; RANKING optimisation
# puts it earlier. Two systems with identical top-3 coverage can differ
# enormously here, and only this split says which one you have.
ranks = [t["localization_rank"] for t in trials if t["localization_rank"] > 0]
if ranks:
    mrr = sum(1 / r for r in ranks) / len(trials)
    print("  localization economics:")
    print(f"    mean true-target rank   : {statistics.mean(ranks):.2f}")
    print(f"    median true-target rank : {statistics.median(ranks):.0f}")
    print(f"    MRR (over all trials)   : {mrr:.3f}")
    print(f"    mean proposals BEFORE target: {statistics.mean([len(t['_before']) for t in trials]):.2f}")
    print(f"    mean proposals ON target    : {statistics.mean([len(t['_on']) for t in trials]):.2f}")
    print(f"    mean $ BEFORE target        : ${statistics.mean([t['_cost_before'] for t in trials]):.3f}")
    print(f"    mean $ ON target            : ${statistics.mean([t['_cost_on'] for t in trials]):.3f}")
    tot_b = sum(t["_cost_before"] for t in trials)
    tot_o = sum(t["_cost_on"] for t in trials)
    if tot_b + tot_o:
        print(f"    ranking waste is {100*tot_b/(tot_b+tot_o):.0f}% of inference spend "
              f"(${tot_b:.2f} finding vs ${tot_o:.2f} fixing)")
    print()

# WRAPPER TAX. Output tokens are the repair; cache traffic is the coding-agent
# system prompt the driver happens to sit behind. Reported apart so a later
# minimal generator can be compared against the same Cortex prompt.
calls = [c for t in trials for c in t["per_proposal"]]
if calls:
    out_t = sum(c["output"] or 0 for c in calls)
    in_t = sum(c["input"] or 0 for c in calls)
    cache = sum((c["cache_create"] or 0) + (c["cache_read"] or 0) for c in calls)
    print(f"  {len(calls)} proposals · output {out_t} tok · prompt input {in_t} tok "
          f"· cache traffic {cache} tok")
    print(f"  cache is {100*cache/max(1,cache+in_t+out_t):.1f}% of all tokens — the harness's "
          f"system prompt, not the repair")

print("\n  by defect class:")
for cls in sorted({t["mutator"] for t in trials}):
    g = [t for t in trials if t["mutator"] == cls]
    ga = [t for t in g if t["target_attempted"]]
    gs = [t for t in ga if t["exit"] == 0 and t["file_clean"]]
    print(f"    {cls:<10} attempted {len(ga)}/{len(g)}  solved|attempted {len(gs)}/{len(ga)}")

print("\n  failures, by which component to blame:")
for t in trials:
    if t["exit"] == 0 and t["file_clean"]:
        continue
    if not t["target_attempted"]:
        why = f"LOCALIZATION (true rank {t['localization_rank'] or 'absent'}, tried {t['attempted']})"
    elif t["proposals"] == 0:
        why = "CONTROLLER/driver (no proposal was made)"
    else:
        why = f"GENERATOR ({t['proposals']} proposals, none verified)"
    print(f"    {t['file'].split('/')[-1]}::{t['symbol']} [{t['mutator']}] {t['outcome']} — {why}")
