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
print(f"Trials: {n}")
if meta.get("stratification_met") is False:
    print(f"  !! STRATIFICATION NOT MET — asked {meta.get('per_class')} per class, "
          f"got {meta.get('per_class_achieved')}")
    print("     Classes below target exhausted the corpus. Their per-class rates")
    print("     are not comparable with the others, and the overall figures are")
    print("     weighted by whatever the corpus could supply.")
elif meta.get("stratification_met") is None:
    print("  (stratification not recorded — run predates the check)")
print()

# 1. LOCALIZATION COVERAGE — was the true symbol ever put in front of the
#    generator at all?
# AN UNREADABLE VERDICT IS NOT A LOCALIZATION MISS.
#
# The harness identifies cortex's verdict as the LAST LINE of stdout and
# collapses any parse failure to `{}` — so a crash, a timeout, or a stray
# trailing line yields `attempted: []`, which reads here as "the ranker never
# put the true symbol in front of the generator". That charges an
# infrastructure failure to localization, which is precisely the confusion the
# three-factor decomposition exists to prevent.
#
# The signature is internally contradictory and therefore detectable after the
# fact: proposals were made, so SOME candidate was attempted, yet the attempted
# list is empty. Trials carrying it are unevaluated, not failed.
contradictory = [t for t in trials
                 if (not t["attempted"] and (t.get("proposals") or 0) > 0)
                 or t.get("rank_read") is False
                 or t.get("verdict_read") is False]
if contradictory:
    print(f"  !! {len(contradictory)} trial(s) have proposals but an EMPTY "
          f"attempted list.")
    print( "     The verdict could not be read, so these are UNEVALUATED, not")
    print( "     localization misses. Excluded from coverage below:")
    for t in contradictory:
        print(f"       {t['file']}::{t['symbol']} "
              f"({t.get('proposals')} proposals, outcome={t.get('outcome')})")
    trials = [t for t in trials if t not in contradictory]
    n = len(trials)

attempted = [t for t in trials if t["target_attempted"]]
print(f"  true target among attempted candidates : {len(attempted)}/{n}  {pct(len(attempted), n)}")

# 2. GENERATOR SUCCESS GIVEN THE TARGET WAS AVAILABLE — the term this
#    experiment exists to estimate.
solved = [t for t in trials if t["exit"] == 0 and t["file_clean"]]
solved_attempted = [t for t in attempted if t["exit"] == 0 and t["file_clean"]]
# `attempted` CONFLATES two states, and the conditional metric this
# experiment exists to estimate depends on which one is meant:
#
#   candidate_selected  — the controller entered an episode for it
#   generator_invoked   — the model was actually asked for a body
#
# They are not equivalent: `attempted.push()` runs BEFORE the episode, so a
# selected candidate can terminate without ever reaching the generator (early
# termination, authority refusal, budget). Conditioning generator CAPABILITY on
# selection charges the generator for episodes it never saw.
#
# So report both. The first needs no candidate<->proposal join and is always
# available; the second needs to know whether the TRUE target specifically
# reached the generator, which is exactly the join that fails closed above.
print(f"  P(repair | true target SELECTED)       : {len(solved_attempted)}/{len(attempted)}"
      f"  {pct(len(solved_attempted), len(attempted))}")
print( "        combined controller + generator path; no attribution needed")

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

    Candidate identity is NOT recoverable from proposal ORDER.

    `attempted.push()` happens BEFORE the episode runs, and several outcomes
    let the walk continue, so a candidate can be attempted and contribute zero
    proposals. Indexing blindly then reads a LATER candidate's proposals as an
    earlier one's: measured on a fixture with one silent candidate, a
    3-proposal on-target recovery was reported as 0 on-target proposals, the
    recovery rate went to n/a, and the whole spend landed in ranking waste —
    the finding-vs-fixing split exactly inverted.

    An earlier attempt at this anchored from the END, on the reasoning that a
    successful walk breaks on the true target so the target's segment must be
    last. That is still positional inference, and it is WRONG in a case it
    cannot detect: if the true target is itself silent and happens to be last,
    the last segment belongs to an EARLIER wrong candidate and is reported as
    on-target generator effort — a confident number with the sign flipped.

    So this fails closed. Either segment count equals candidate count (nothing
    was silent, and position carries identity because order is then the whole
    story), or attribution is UNMEASURED. The fix is not a better heuristic;
    it is for each proposal to CARRY its candidate, which is staged for the
    next run.
    """
    if not t["target_attempted"] or t["symbol"] not in t["attempted"]:
        return [c for seg in segments(t) for c in seg], []

    # 1. IDENTITY. Each proposal names the candidate it was made for, so
    #    ownership is read, not inferred, and a silent candidate is simply a
    #    candidate with no proposals rather than a corrupted join.
    named = [c for c in t["per_proposal"] if c.get("candidate")]
    if named and len(named) == len(t["per_proposal"]):
        on = [c for c in named if c["candidate"] == t["symbol"]]
        # "Before" means before the true target was REACHED, not merely every
        # other candidate: proposals made after it are not search cost.
        first = next((k for k, c in enumerate(named)
                      if c["candidate"] == t["symbol"]), len(named))
        before = [c for c in named[:first]]
        return before, on

    # 2. Position — admitted ONLY on a positive proof of completeness, not on
    #    the absence of an obvious problem:
    #
    #        number of candidate episodes == number of proposal segments
    #
    #    That is a real proof only because one episode yields exactly one
    #    segment. VERIFIED, not assumed: `rejected` is declared once per
    #    episode (runner.rs:642) and only ever pushed to, never cleared, so
    #    `priors == 0` occurs exactly once per candidate. If that ever changed
    #    — if the rejected list were reset mid-episode — a candidate would
    #    split into two segments, the counts could match WHILE a candidate was
    #    silent, and this check would pass on a corrupted join. The identity
    #    path above is what stops that premise from mattering.
    segs = segments(t)
    if len(segs) != len(t["attempted"]):
        t["_unattributable"] = True
        return [], []
    i = t["attempted"].index(t["symbol"])
    return [c for seg in segs[:i] for c in seg], segs[i]


for t in trials:
    _b, _o = split_on_target(t)
    t["_before"], t["_on"] = _b, _o
    t["_final_proposals"] = len(_o) if _o else len(_b)
    # None, not 0. A zero here would enter the spend split as a real
    # measurement of "no ranking waste" — the absent-vs-passed collapse in
    # its cheapest form.
    un = t.get("_unattributable")
    t["_cost_before"] = None if un else sum(c["cost_usd"] or 0 for c in _b)
    t["_cost_on"] = None if un else sum(c["cost_usd"] or 0 for c in _o)
    if un:
        t["_final_proposals"] = None


bad = [t for t in trials if t.get("_unattributable")]
reached = [t for t in attempted if not t.get("_unattributable") and t["_on"]]
if bad:
    print( "  P(repair | true target REACHED GENERATOR): UNMEASURED")
    print(f"        {len(bad)} trial(s) cannot prove the true target was asked")
    print( "        for a body at all, so the clean generator-capability term")
    print( "        is not available from this run.")
else:
    ok = [t for t in reached if t in solved_attempted]
    print(f"  P(repair | true target REACHED GENERATOR): {len(ok)}/{len(reached)}"
          f"  {pct(len(ok), len(reached))}")
    print( "        generator capability proper")

# A trial whose join failed must be SAID rather than folded into a rate.
if bad:
    print(f"  !! {len(bad)} trial(s) UNATTRIBUTABLE — a candidate was attempted")
    print("     but produced no proposal, so which proposals belong to the true")
    print("     target cannot be recovered. Excluded from the recovery rate and")
    print("     the finding-vs-fixing split; still counted everywhere else.")

# 3. RECOVERY — does the feedback loop earn its complexity?
multi = [t for t in attempted if not t.get("_unattributable") and len(t["_on"]) > 1]
recovered = [t for t in multi if t["exit"] == 0 and t["file_clean"]]
if bad:
    print("  recovery on true target                : UNMEASURED\n")
else:
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
att = [t for t in trials if not t.get("_unattributable")]
sa_att = [t for t in solved_attempted if not t.get("_unattributable")]
first_shot = [t for t in sa_att if t["_final_proposals"] == 1]
after_fb = [t for t in sa_att if t["_final_proposals"] > 1]
failed_fb = [t for t in attempted if not t.get("_unattributable")
             and t not in solved_attempted and t["_final_proposals"] > 1]
gen_err = [t for t in trials if t["outcome"] == "needs_input"]
# All four buckets need the join. Printing them as zeros when the join failed
# would read as "no recoveries happened" rather than "we cannot say".
if bad:
    print("    first-shot / recovered / failed-after-feedback: UNMEASURED")
else:
    print(f"    first-shot repair       : {len(first_shot)}")
    print(f"    recovered after feedback: {len(after_fb)}")
    print(f"    failed after feedback   : {len(failed_fb)}")
print(f"    generator unavailable   : {len(gen_err)}\n")

# THE INCREMENTAL VALUE OF RECOVERY. If most verified repairs required a
# rejected proposal first, the loop is not wrapping a model call — it is
# converting a mediocre first attempt into a verified solution by structured
# iteration, and that is the part worth its complexity.
if bad:
    print("  recovery's share of all verified repairs: UNMEASURED\n")
elif solved_attempted:
    print(f"  recovery's share of all verified repairs: {len(after_fb)}/{len(solved_attempted)}"
          f"  {pct(len(after_fb), len(solved_attempted))}\n")

# RANK x OUTCOME. Does a lower-ranked candidate merely cost more proposals, or
# does it also make the generator likelier to fail? Cost and capability are
# different problems with different fixes.
# The matrix splits each rank into first-shot vs recovered, which is the join
# again. Rank alone is sound; rank x OUTCOME is not.
print("  localization rank vs outcome:"
      + ("   UNMEASURED (candidate attribution failed)" if bad else ""))
print(f"    {'rank':<8}{'first-shot':>11}{'recovered':>11}{'failed':>9}")
for r in (1, 2, 3):
    at_r = [t for t in attempted if not t.get("_unattributable")
            and t["localization_rank"] == r]
    fs = sum(1 for t in at_r if t in sa_att and t["_final_proposals"] == 1)
    rc = sum(1 for t in at_r if t in sa_att and t["_final_proposals"] > 1)
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
    # TOTAL proposals is a count of model calls and needs no join. The
    # on-target share does need one, so it is dropped rather than shown as 0.
    on_target = ("" if bad else
                 f", {statistics.median([len(t['_on']) for t in solved]):.0f} "
                 f"on the true target")
    print(f"  median proposals / solve: {statistics.median(props):.0f} total{on_target}")
    print(f"  median cost / solve     : ${statistics.median(costs):.3f}")
    print(f"  median wall / solve     : {statistics.median(walls):.0f}s")
print(f"  total spend             : ${sum(t['cost_usd'] for t in trials):.2f}\n")

# TRANSPORT vs CAPABILITY.
#
# "Follows the output contract" and "knows how to repair the code" are
# different abilities, and a single solve rate averages them. A run whose
# driver mangles conventional output would look like a model that cannot
# repair — which is precisely the defect that voided the previous run.
#
# Degrades honestly: runs recorded before the driver reported extraction
# status say UNMEASURED rather than assuming the favourable value.
stat = [st for t in trials for st in (t.get("extraction_status") or [])]
stat = [x for x in stat if x]
if stat:
    valid = sum(1 for x in stat if x != "no_code_found")
    print("  transport:")
    print(f"    P(valid code proposal | response) : {pct(valid, len(stat))}")
    for k in ("clean_fence", "prose_plus_fence", "raw_code",
              "ambiguous_multiple_fences", "no_code_found"):
        c = stat.count(k)
        if c:
            print(f"      {k:<26}{c:>4}")
    if solved_attempted or attempted:
        print(f"    P(repair | valid proposal, true target): "
              f"{pct(len(solved_attempted), len(attempted))}")
else:
    print("  transport: UNMEASURED — this run predates the driver recording")
    print("             extraction status; a body alone cannot distinguish a")
    print("             strange model answer from a driver that mangled a")
    print("             conventional one.")
print()

# INVALID-OUTPUT COST. Transport worked, the code did not: a proposal that was
# extracted faithfully and still would not compile. Counted apart from both
# finding and fixing, because it is the model's error rather than the ranker's
# or the loop's — and apart from extraction failures, which are infrastructure
# faults that invalidate a run rather than costing it.
rev = [t.get("reverted") for t in trials if t.get("reverted") is not None]
if rev:
    print(f"  invalid-output proposals (extracted fine, would not compile): {sum(rev)}")
else:
    print("  invalid-output cost: UNMEASURED — this run did not retain the")
    print("             episode, and `patch_reverted` is the only record of a")
    print("             proposal that was read correctly and still broke the build.")
print()

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
    # Everything from here down joins proposals to a candidate. One failed
    # join makes the SPLIT unmeasurable — not smaller, unmeasurable — so the
    # whole block goes rather than reporting a partial split as a whole one.
    if bad:
        print(f"    proposals/cost before vs on target: UNMEASURED — candidate")
        print(f"      attribution failed on {len(bad)} of {len(trials)} trial(s).")
        print( "      A proposal does not carry the candidate it was made for,")
        print( "      so ownership was inferred from ORDER; where a candidate")
        print( "      produced no proposal that inference is wrong and cannot")
        print( "      be detected per-trial. Fixed for the next run by having")
        print( "      each proposal name its candidate.")
    else:
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
