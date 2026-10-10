You are the independent final reviewer for ONE v0.22 acceptance gate in a FROZEN candidate: **{GATE}**.

## The gate (judge it LITERALLY; this is the whole claim)
> {CLAIM}

## What is frozen (do NOT edit anything in these trees)
- Axon: /home/cklaus/projects/aicoding/axon-s3 at bd3637ab (branch v022/stage3). Run `git -C /home/cklaus/projects/aicoding/axon-s3 rev-parse HEAD` for the full SHA.
- MiCode: /home/cklaus/projects/aicoding/micode-v022-wt at e7fdb739.
- The proof run on the frozen pair is in /tmp/claude-0/-home-cklaus-projects-axon/0c0c427a-f596-461b-8fbf-e136e336604e/scratchpad/bind2b/. It holds:
  - summary.txt, manifest.json and mutations.json (scope `all`, including the new `binding` rows M101–M115);
  - 1e.log, from scripts/v022_stage5_gates.sh; its rows for {GATE} name the tests that evidence it.

## History you need (so you judge what changed, not what was already decided)
This is candidate 2. Candidate 1 (review wf_d788c05a-be2) did not register the three G11 gates. Its executed blockers, and what candidate 2 changed:

1. **A transition issuer was not separated from what it promotes.** The proposer (ranker), evaluator or a subject, when listed as an admitter, could issue the ACTIVATE or rollback.
   - Change: `admission::issuer_independent`.
   - Change: `designate_baseline` now applies the role checks.
2. **Activation and rollback did not recheck withdrawn authority** (clearing monitor, context observer, verifier pins, task acceptance).
   - Change: `admission::derive` rechecks, against CURRENT config, each counted verdict's pins, using `intake::check_pins` over the check documents stored at intake.
   - Change: in a protected class it also rechecks each counted trial's context observer and key (`TrialResult.context_signed_by`).
   - Change: in a protected class it rechecks each cleared trial's monitor and key (`Event::SafetyReport.key_id`).
3. **Arm economics omitted the Fabric execution cost.**
   - Change: EVL adds one UNKNOWN execution component per Fabric-executed trial, holding its reservation as liability.
   - Change: operator decision ADR-001 D4 says "monetary economics are REPORT-ONLY until complete metered attempt receipts exist; UNKNOWN cost never becomes zero". The rule grammar therefore gains `economic_threshold = "report_only"`: economics are recorded and decide nothing, as frozen in the plan.
   - Consequence: a frozen cost criterion over executed trials is INCONCLUSIVE.
4. **Rollback did not recheck profile qualification.** Covered by (2): the pins include the backend profile.

Also changed: ADR-001 §3.6, the population issued before execution. This responds to a MAJOR-ADJACENT best-of-k finding executed by two reviewers.
- `plan assign`: an independent admitter journals trials and attempt ids after the freeze.
- `evaluate` requires the request to equal the issued population, counts only the issued attempt, and, in a protected class, requires the observer-signed preflight to follow the issue.

## Context
- ADR-001 (governance/specs/v022-ADR-001-experiment-integrity.md) applies:
  - D1: this WSL host is DEVELOPMENT and makes no protected claims;
  - D3: the local backend is dev-only;
  - D4: see above.
- ADR-002 (governance/specs/v022-ADR-002-preflight-observer.md): distinct keys per role, and an observer holds no verifier, admitter or monitor role.
- **Recorded residual (ADR-002 territory, not a new finding):** a subject can re-run the SAME issued trial/attempt identity on a fresh Fabric journal and intake the run it prefers. Closing that needs the nonce-bound, observer-signed launch of ADR-002 on the protected host.
- **Recorded residual (ADR-001 §3.4):** role identities are compared by name, not key fingerprint.
- Classify a claim-level consequence of either residual honestly. Do not rediscover them as new.

## Classification
- **BLOCKER:** a concrete, preferably EXECUTED way the frozen code falsifies a sentence of the gate. Quote that sentence.
- **MAJOR-ADJACENT:** a real defect outside this gate's wording.
- **MINOR:** a proof, test or documentation weakness that falsifies nothing.
- **FUTURE:** hardening beyond v0.22.

Do not reclassify an ADR-accepted limitation as a blocker unless the gate's wording is actually falsified.

## Rules of engagement
- Read-only against the frozen trees. For executed probes, copy what you need into /var/tmp/bind2-final-{GATE}/, use your own CARGO_TARGET_DIR there, and `rm -rf` it before you finish.
- Wrap every build or test in `source /home/cklaus/projects/aicoding/axon-s3/scripts/lib_bounded_run.sh; bounded_run 16G 1800 <cmd>`, and set TMPDIR=/var/tmp/axon-test-tmp and LLVM_SYS_170_PREFIX=/usr/lib/llvm-17.
- Never read, create or request any private signing key. Never touch branches main, tui, origin or v014-reconciled, or other worktrees.
- Mark `executed: true` only for what you actually reproduced.
- Verdict REGISTER if and only if you found zero BLOCKERs against {GATE}.
