You are the independent final reviewer for ONE v0.22 acceptance gate in a FROZEN candidate: **{GATE}**.

## The gate (judge it LITERALLY; this is the whole claim)
> {CLAIM}

## What is frozen (do NOT edit anything in these trees)
- Axon: /home/cklaus/projects/aicoding/axon-s3 at a9e4c269 (branch v022/stage3). Run `git -C /home/cklaus/projects/aicoding/axon-s3 rev-parse HEAD` for the full SHA.
- MiCode: /home/cklaus/projects/aicoding/micode-v022-wt at fc6221a5.
- The proof run on the frozen pair is in /tmp/claude-0/-home-cklaus-projects-axon/0c0c427a-f596-461b-8fbf-e136e336604e/scratchpad/bind5/. It holds:
  - summary.txt, manifest.json and mutations.json (scope `all`);
  - 1e.log (scripts/v022_stage5_gates.sh) and 5.log (scripts/v022_micode_gates.sh), whose rows for {GATE} name the tests that evidence it;
  - 4.log (scripts/loop_interop_gate.sh; section 8b is the real-binary evidence);
  - discriminator-micode-e7fdb739.log and discriminator-micode-3192e948.log (the same gate against the two previous MiCode revisions).

## History (so you judge what changed, not what was decided)

This is candidate 3. Candidate 1 (axon 6d3f0ff6 + micode e7fdb739, review wf_849bc606-7e8) had three blockers, all executed through the real `micode exec`:
1. A hung Fabric (MiCode's watchdog), a crashed Fabric and an unattested receipt all reached Axon as the same uncited `not_run`.
2. A run past its own deadline was recorded as `failed` + `not_run`, identical to a refusal.
3. SIGTERM/SIGINT wrote no sidecar, so a cancelled trial arrived as `missing`.

**Candidate 2** (axon 1486ef37 + micode 3192e948, review wf_bac07f9d-087) fixed all three candidate-1 blockers. It had ONE executed blocker: a provider that answers 200 and then sends nothing past its first-byte deadline, the realistic stall, is re-issued as a StreamDeath. Once the re-issues were spent, the session ended as `stream_death`, got no run_timed_out, and was judged NotRun, the same as a refusal.

**Candidate 3:**
- MiCode fc6221a5: `consume_stream_classified` reports the deadline phase when it converts a no-content deadline into a re-issuable death, and `stream_turn_with_reissue` restores `FailureShape::Timeout { phase }` once the re-issues are spent. A plain zero-byte drop still ends as StreamDeath.
- Unit test: `a_silent_provider_past_its_deadline_ends_as_a_timeout_not_a_drop`.
- Real-binary: interop 8b trial u-pong-4 is a provider that answers 200 and then stalls, past a 1 s first-byte deadline, on the first try and both re-issues. It ends failed + not_run + run_timed_out, and EVL judges it timed_out.
- Repetitions are now 4 (16 candidate trials).

The candidate-2 review's MINORs are recorded, not changed:
- the watchdog does not kill a hung Fabric child;
- a second signal exits without a sidecar;
- exhausted_dimension checks wall-clock last.
Judge whether any of them falsifies the claim.

The mechanisms described below are unchanged since candidate 2.

**Candidate 2, MiCode (3192e948):**
- `fabric_check` returns a typed failure class: CheckTimedOut, CheckEvidenceMissing, CheckUnverifiable or CheckRefused.
- The finish site records RunTimedOut on a wall-clock budget exhaustion or a provider timeout.
- The class travels INSIDE the pinned episode contract (the schema is a byte copy of the owner-locked package). It is one content marker in the not_run's `verification.evidence_refs`, `cl22({"not_run_reason": R, "by": "micode"})`, the same pattern as the existing not-produced refs.
- `micode exec` trips the session cancel token on the first SIGTERM/SIGINT: the turn finishes, then the sidecar is written with status `cancelled`. A second signal exits at once.
- Only a real cancel maps to `cancelled`: a degeneration trip is now failed, and a detach outcome_unknown.

**Candidate 2, Axon (1486ef37):**
- Intake recognises the marker by recomputing the closed set, and refuses any other uncited evidence ref.
- EVL maps the stated reason: run or check timeout → TimedOut, missing evidence → MissingEvidence, unverifiable → Unverifiable, refused → NotRun.
- How the run ended outranks the stated reason. A stated reason never makes a trial count.

**Real-binary evidence, interop section 8b:** a real EVO candidate, a frozen plan and an issued population of 16 candidate trials, each in its own worktree. Every kind has a real producer:
- SIGTERM mid-turn → cancelled;
- a 1 s wall-clock budget → timed_out;
- a provider silent after its headers past a 1 s first-byte deadline, on every re-issue → timed_out (candidate 3);
- Fabric hung past the watchdog → timed_out;
- a crashing Fabric → missing_evidence;
- Fabric with no signer → unverifiable;
- Fabric refusing an unregistered grant → not_run;
- Fabric's wall-time on a spinning check → timed_out (signed);
- an unmatched name → unmatched;
- the D12 pass and fail → unbound (never counted).

The evaluation is then judged by the real `evl evaluate`.

## Context
- ADR-001 (governance/specs/v022-ADR-001-experiment-integrity.md) applies:
  - D1: this WSL host is DEVELOPMENT and makes no protected claims;
  - D3: the local backend is dev-only;
  - D12: under the dev bridge only the acceptance CHECK goes through Fabric, and a D12 trial is judged but never counted.
- **Recorded residuals (do not rediscover them as new):**
  - a stated not-run reason is a producer claim on this dev host (same-uid producer, ADR-001 D1/D2), which is why it can only name a non-success kind;
  - identities are compared by name (ADR-001 §3.4).

## Classification
- **BLOCKER:** a concrete, preferably EXECUTED way the frozen code falsifies a sentence of the gate. Quote that sentence.
- **MAJOR-ADJACENT:** a real defect outside this gate's wording.
- **MINOR:** a proof, test or documentation weakness that falsifies nothing.
- **FUTURE:** hardening beyond v0.22.

Do not reclassify an ADR-accepted limitation as a blocker unless the gate's wording is actually falsified.

## Rules of engagement
- Read-only against the frozen trees. For executed probes, copy what you need into /var/tmp/bind5-final/, use your own CARGO_TARGET_DIR there, and `rm -rf` it before you finish.
- Wrap every build or test in `source /home/cklaus/projects/aicoding/axon-s3/scripts/lib_bounded_run.sh; bounded_run 16G 1800 <cmd>`, and set TMPDIR=/var/tmp/axon-test-tmp and LLVM_SYS_170_PREFIX=/usr/lib/llvm-17. This host has 23 GB of RAM, so run one heavy build at a time.
- Never read, create or request any private signing key. Never touch branches main, tui, origin or v014-reconciled, or other worktrees.
- Mark `executed: true` only for what you actually reproduced.
- Verdict REGISTER if and only if you found zero BLOCKERs against {GATE}.
