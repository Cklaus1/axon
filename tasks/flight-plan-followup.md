# Flight plan — follow-up batch

**Mission:**
- build a powered, digest-pinned headroom held-out set;
- close the capability-walker and cortex correctness gaps S1 left open;
- fix R4's keep/remove rule before measuring, then measure;
- ship to `main` with no unattended authority granted.

**Plan options** (Running **Fastest at 90 %**. Want a different one?):
- **Fastest (90 %)**, active: 4 parallel lanes, one full gate and one regress on the integrated head. About 4–6 h AI
  time plus about 7 h model wall. *Quick checks while building, then one big check at the end.*
- **Most reliable:** a full gate after every lane and a regress per digest item. About 8–10 h plus 7.5 h. *Check
  everything fully after every step.*
- ⚠ **production-level:** FU-2 and FU-5 are capability/effect-gate code. The agreement fixtures, tamper suite and full
  gate stay mandatory.

**Engineering decisions (Step 2):**
- Headroom source → harder single-symbol tasks (multi / caller / subtle / refine / caps), because weaker models are
  unreachable.
- Contamination → 20 independently written programs. The check is descriptive, and the gain criterion stays pooled.
- Calibration → cells of ≥ 12 tasks from ≥ 3 programs, shrunk band [0.3, 0.7], 2-fold cross-fit, since raw k = 3 rates
  regress.
- Size → ≥ 20 test programs, ≤ 8 tasks each, n ≈ 160, h55 k = 5, h45 k = 1. Clustered power ≈ 0.88 for ≈ $16 (cap $25).
- Statistics → exact or Monte Carlo sign-flip on program sums, test-inversion CI, one `stats.py`.
- Integrity → `B/protected.py` pins for hx-test and hard-once, plus a hash-chained ledger. Only paid runs spend; runs
  are resumable; a re-roll is refused.
- k trials → `(id, trial)` keys in run_ab, analyse and scorecard, executed trial-major.
- FU-11 arms → concurrent T0/T1/T2, one grading axon (`--grade-axon`), rate-limited trials pairwise-dropped.
- FU-1d → `caps_widened` per symbol vs pristine. For arm B it is a fence alarm.
- FU-2 → program-wide walkers root every item in S1's `TypeHooks`. E1004 uses S1's per-fn attribution. No codegen
  manifest leg.
- FU-3 → a stopped check drain gives no verdict. A detached generator drain is Unavailable; an idle-stopped one keeps its
  bytes (NH1).
- FU-4 → one restore helper: write only over bytes in the episode's after_digests. Read error → no write.
- FU-5 → `interp.rs:7566` and `main.rs:6747` map NotUnicode to deny-all. axon-rt is already closed.
- FU-6 → canonicalize `--workspace` and `--axon`. M71 text unchanged.
- FU-7 → reuse `symbol_body`. A lex error means no reach.
- FU-10 → T2 = R4 fully reverted. KEEP needs a solve gain or an efficiency gain at p < 0.025 each; otherwise REMOVE and
  land the revert. MDE about 7 points.
- Receipt regress → once after FU-3/4/6/7, plus once more if FU-10r lands.

**needs-human:** none in this batch. NH-R1 and R6 are separate DRAFT specs, and unattended execution is not authorized.

**[REVISED] markers:** 57. The headline ones:
- FU-3's generator remedy reversed (blocker A#1);
- the `--check` gate stages replaced (blocker D#17);
- k-trial keying added (blocker D#9);
- cost cap re-derived (blocker D#15);
- power model gained a program effect (blocker D#1);
- `caps` data file dropped;
- FU-10 T2 is now a full revert.

**Critical path:** FU-1e integrity (tamper suite plus a refuse-before-call proof). FU-2's agreement fixtures are the
security proof.

**Shape:** 11 spec items, about 14 DAG nodes, 4 lanes. Paid runs: pilot ≤ $1, calibration ≤ $5, regress ≤ $1, FU-11
≤ $25 (list price on OAuth; real ranker spend about $1.5).

**Logged:**
- O-S1-19 (verify purity)
- O-S1-20 (native effects missing from `effect_union`)
- O-S1-21 (hardset `build.py` exit code)
