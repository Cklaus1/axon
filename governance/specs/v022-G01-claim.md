# G01 — verification authenticity: the claim, its evidence, and what it does not cover

Status: **NOT REGISTERED.** This is the exact statement a registration of
**G01-r22-independent-issuer** would certify ("verifier receipts are authenticated through
trusted issuer lookup and bind profile/configuration/artifact/verifier revision;
subject-generated, stale or cross-task evidence is rejected even if its JSON and hashes
validate").

It does not certify the rest of the G01 family. Each of those gates stays PARTIAL until its own
clauses hold:
- **G01-r22-registered-check** needs a "real protected host path"; the microVM emits no verdict
  yet.
- **G01-r22-verifier-separation** needs the subject unable to edit the verifier; that needs the
  D1 protected environment.
- **G01-r22-unknown-outcome** needs distinct non-success states "through the bridge"; under D12
  no real MiCode trial reaches evaluation.

**Registration rule.** The candidate is frozen: exact Axon and MiCode SHAs, clean trees, this
document. It is registered when all of the following hold:
- every clause below has named evidence;
- every mutation in `scripts/v022_g01_mutations.py` is killed at the frozen Axon SHA;
- the real-binary interop gate and the named MiCode G01 gates pass against the frozen pair;
- one final independent re-audit of the frozen pair finds zero claim-level blockers.

Nothing is edited during that review. Its findings are classified:
- **BLOCKER:** directly falsifies a clause below, or a prerequisite it explicitly names.
- **MAJOR-ADJACENT:** a real defect outside this claim. It is recorded in the sweep and the
  false-green ledger, and it does not block.
- **MINOR:** a proof, test or documentation weakness that does not falsify a clause.
- **FUTURE:** hardening beyond the v0.22 gate.

History:
- Re-audit 1 (wf_f213aa49): failed.
- Re-audit 2 (wf_c11193f8): failed.
- Re-audit 3 (wf_85472584, at 87273274): failed, with three blockers, all since fixed. The fixes are FG-053 to FG-057.
- Re-audit 4 (wf_0c9149f9, at 917fe199): failed on one blocker. A revoked verifier's verdicts
  still counted at admission and activation (fixed: c0770ae8, FG-058). Four of the five roles
  reported REGISTER; every minor is addressed below or stated as a limitation.
- Re-audit 5 (wf_7e55371c, at cf6c779b): failed on two blockers.
  - A `:` in the caller-named state dir split `AXON_PATH` (fixed in 1db8ead8).
  - Modules fell through to ambient directories (fixed in 1db8ead8, with the new
    `AXON_PATH_EXCLUSIVE`).
  - Its majors are closed too: cross-scope reuse, the ceiling proven applied (both 7c3f2fe5), and
    a repository `.env` setting `axon.*` (MiCode dd4ea0a9).
- Final re-audit of frozen candidate 1 (wf_5f02e4cb, axon ac76b14c + micode dd4ea0a9): three
  roles REGISTER, two DO_NOT_REGISTER, each on one executed blocker.
  - A symlink chain in the candidate escaped the tree: clause 2's module confinement.
  - A `break` escaping a candidate function counted as a pass: clause 4, and "subject-generated
    evidence is rejected".
  - Both are fixed in 4e7851bc (FG-062, FG-063). Candidate 2 is frozen after this.
- Final re-audit of frozen candidate 2 (wf_2f0bc4c3, axon e42fe160): four roles REGISTER, one
  DO_NOT_REGISTER on one executed blocker. FG-063's fix stopped a `break` at the test boundary
  only; a candidate's `break` still landed in a loop the operator's test owned. Loop control no
  longer crosses any function boundary (fixed: bcf9c0a7, FG-064). Candidate 3 is frozen after this.

## The claim

A verification verdict (passed or failed) counts toward evaluation, admission
or promotion only if all of the following hold.

1. **It is authenticated.** An Ed25519 `acf-receipt-attestation/1` is signed by Fabric's
   operator-configured signer. It binds:
   - the schema, the issuer and the key id;
   - the request and receipt digests;
   - the operation, task, trial, attempt and execution ids.

   One Fabric verification decides one trial in ONE scope: intake refuses a verification
   receipt already recorded under another scope.

   The public key must be registered in the store's `verifier_keys`. A missing key, a missing
   attestation or a key mismatch refuses the verdict. The check is repeated at admission and at
   every re-derivation (activation, rollback). A verifier the operator has since untrusted or
   re-keyed no longer vouches for anything.
2. **Fabric signs only a verdict it just produced.** It signs a `registered_check` of an
   operator-registered suite (`check:<id>`), and only when all of these hold:
   - this process produced the receipt. A replay is never signed, because the journal path is
     the caller's.
   - the workload could not read the key: it ran in the protected microVM, or its admitted grant
     gives it no effect at all.
   - the check ran from an empty environment, apart from the ceiling, the trial caches and the
     module path. The empty ceiling is applied by the executor, not merely intended.
   - its modules resolved ONLY from the suite and the candidate (`AXON_PATH_EXCLUSIVE`), never
     from the trial cache's `~/.axon/lib` or the interpreter's own library; a state dir that
     would split the module path (a `:`) is refused before anything is written. The candidate
     and the suite hold plain files and directories only: a tree containing a symbolic link is
     refused before launch, so no link can lead a module lookup out of the tree.
3. **The verifier is trusted and independent.** Evaluation is the only door where a verdict
   counts, and it judges by this subject set:
   - the trial's observer;
   - the evaluation request's subject issuers;
   - the proposer of each arm policy.

   Intake cannot know an evaluation's subject issuers. It judges by the observer and the
   proposer of the episode's own policy. That is a subset of evaluation's set, so it fails
   closed. The issuer must be trusted and outside the set. The check must not have run as a
   subject principal.
4. **What ran is what the operator pinned.** This covers:
   - the verifier revision (executable and digest) and its compute profile;
   - exactly one suite identity, `check-suite:<id>@<version>#<entry>`, pinned for that verifier;
   - the task's own registered acceptance check: that suite identity plus the exact test name.
   - that test counts as passed only if it COMPLETED. Loop control never crosses a function
     boundary: a `break`/`continue` with no loop of its own in a candidate function is a failure
     there. It can neither unwind the test nor end a loop the operator's test owns.

   The requester chooses neither the test, nor the version, nor the entry file. The suite's own
   modules resolve before the candidate's. No repository-controlled configuration source (MiCode
   project TOML, or a `.env` discovered from the working directory) may set `axon.*`, so the
   judged repository cannot choose its own verifier configuration.
5. **The semantic joins hold.**
   - The identity join covers task, trial, attempt, operation and execution.
   - The request tree, the receipt's input, the sidecar's verified tree and the episode's output
     must be the same tree, for failed verdicts too.
   - The receipt is supervisor-observed.
   - Result and matched count equal the receipt's.
6. **ADR-001 D3.**
   - A scope listed in `config.protected_scopes` freezes as a protected evaluation.
   - In a protected evaluation, a trial counts only if its verification receipt came from a
     `PROTECTED_PROFILES` backend. That backend is inside the signed receipt, so this is
     authenticated.
   - The execution receipt's backend is checked the same way, but only as a filter. It is a
     producer claim, so it can only make fewer trials count.
   - In a protected scope, every activation and every rollback of an admission-backed policy
     needs a re-derived admission whose journalled evaluation is protected. No mechanism-test
     fixture is ACTIVATED there. One already active when the scope was protected stays until
     the next transition; see Known gaps.
   - In every scope, a rollback re-validates its predecessor now: a baseline's issuer must
     still be trusted, and an admission must still re-derive (ADR-001 §5).
7. **Evaluation reads only intaken evidence.**
   - A delivered episode counts only if intake recorded it in the scope.
   - Each counted verdict records the request, receipt and attestation digests, the issuer and
     the key id.
   - The intake record names the attestation and key that authenticated it.

## Evidence

| Clause | Tests (unit/integration) | Mutations | Real binaries (`loop_interop_gate.sh`) |
|---|---|---|---|
| 1 | `attestation.rs` unit tests; `intake.rs` authenticated, each-rule and one-scope tests; `evidence_laundering.rs` unauthenticated-verdict; `evl_admission.rs` revoked verifier | M02 M03 M08 M50 M54 | §8: attestation kept, key, operation; no-attestation refused; impostor key refused |
| 2 | `signing.rs` unit tests; `attestation.rs` (Fabric) replay, forged-journal, environment and applied-ceiling tests; `check_effects.rs` exit-0 rule, `:` state dir, no ambient modules | M01 M40–M42 M44 M49 M52 M53 M55 M57 | §8 (2): effectful grant withheld; replay unsigned |
| 3 | `intake.rs` each-rule and proposer tests; `evidence_laundering.rs` observer at both doors; `redteam.rs` NS4 | M05 M10 M16 M37–M39 M46 | §8 (3): check run as the observer refused |
| 4 | `intake.rs` pinned, task-only, two-version and entry tests; `check_effects.rs` shadowing | M04 M06 M07 M26–M30 M43 M47 M56 M58 | §8 (4): another pinned revision refused; task and filter pin cases |
| 5 | `intake.rs` join test (failed-verdict tree, supervisor, one suite, role upgrade) | M09 M21–M25 M31–M36 M45 | §8: the cited-receipt join, with the genuine attestation, refused by "digests to"; positive tree, identity and supervisor on real bytes |
| 6 | `protected_class.rs` (including a protected-class mechanism fixture and rollback revalidation) | M11–M13 M17–M20 M51 | not exercised (see gaps) |
| 7 | `evidence_laundering.rs` intake-only and cites-evidence tests; `intake.rs` records attestation and key | M14 M15 M48 | §8 (7): the record names the key id and the stored attestation |

The mutation run lives in `scripts/v022_g01_mutations.py`, with results in
`governance/proofs/v022-g01/`. The authoritative run is the one named for the commit being
registered. Superseded runs are kept under `superseded/`. The run covers the listed guards only;
it is not a proof that no other guard exists.

A kill means the named test failed. For M24 and M27–M29 the recorded kill shows the forged input
still refused, but by a second rule. That is redundancy by design (defense in depth): the kill
proves the named test depends on the guard, not that the guard alone stands between a forgery
and a count.

MiCode's side is defense in depth, because intake re-verifies either way. It cites no unattested
verdict, and a repository cannot choose its own verification. Both are rows in
`scripts/v022_micode_gates.sh`: it cites no unattested verdict (`fabric_check`), and no project
TOML or `.env` may set `axon.*` (`config`).

## Accepted limitations (ADR-001; not claimed)

- **No operator-custodied authority root.** `verifier_keys`, `verifier_pins`, `task_acceptance`
  and `protected_scopes` are store config writable by the store's uid (D2/D5/D6). Key custody is
  a same-uid file mode, with no epoch or nonce.
- **Registries are caller-named.** The grant registry and check registry are paths the caller
  names. So the check principal in clause 3 is a name the caller can choose (it is refused only
  when it names a subject). The materialized run directory sits under a caller-named state dir,
  so a same-uid process could rewrite it mid-run.
- **The DEV environment makes no protected claim (D1).**
- **The attestation binds no loop scope.** It carries no tenant or family. Intake refuses a
  verification receipt already recorded under another scope (clause 1), so reuse is refused by
  the store, not by the signature. Binding the scope is an attestation v2 item (after D1/D2/D5).
- **G01 authenticates the verdict, not the tree's provenance.** It authenticates the verdict
  about a tree and a task's check. It does not authenticate:
  - that the arm's policy produced that tree;
  - the arm id;
  - the cost or corpus role.

  Those rest on producer-written documents (D12 local execution, `bind_acf` over unsigned
  execution documents).
- **Fabric re-runs an operation on a fresh journal.** The journal is caller-named, so a new
  journal re-runs and re-signs an operation already run elsewhere. Uniqueness per attempt comes
  from intake's identity rule, which records one set of bytes per trial identity, not from
  Fabric.
- **Unshipped suite modules fall through to the candidate.** A `mod` the operator's suite
  declares but does not ship resolves from the candidate. Suite completeness is the operator's
  registration duty; the suite's own modules always win (FG-052).

## Known gaps (not claimed; tracked)

- **No real protected verdict exists yet.** The Linux microVM profile emits no suite verdict
  (no filter, no suite evidence, no output tree). Every protected positive test re-labels a local
  receipt, so clause 6 has no real producer. It fails closed: a protected evaluation counts
  nothing.
- **Protecting a scope is not retroactive.** A policy already active when the scope is protected
  stays active until the next transition (ADR-001 §5 activation revalidation, pending).
- **Clause 6 has no real-binary coverage.** The interop gate never freezes or activates in a
  protected scope.
- **No real MiCode verdict counts in evaluation yet, and counting has no real-binary coverage.**
  Under D12, MiCode writes its execution refs as not-produced markers. Evaluation's `bind_acf`
  requires real ones, so every real MiCode trial is Unknown ("unbound ACF evidence"). It fails
  closed. The interop gate drives intake, not `evl evaluate`, admit or activate; counting is
  proved on fixture trials.
- **Revocation is not retroactive to the ACTIVE policy.** Untrusting or re-keying a verifier
  stops its verdicts at every later admission, activation and rollback. A policy already active
  stays active until the next transition (ADR-001 §5 activation revalidation).
- **Pins are checked when the verdict is authenticated.** Verifier pins and task acceptance
  (clause 4) are checked at intake and evaluation, not re-checked at admission. Withdrawing a pin
  later does not retract an evaluated verdict, whereas untrusting or re-keying the verifier does.
- **The admission scan does not see `use a::b`.** Such an import loads a module without a `mod`
  line. Resolution is confined to the suite and candidate (clause 2), and the empty ceiling
  denies effects at run time, so the scan is not the barrier.
- **Interpreter-side guards need a rebuilt binary.** Fabric tests execute the built `axon`
  interpreter. The mutation driver builds it once, so interpreter guards are mutated through their
  unit tests (M56). A pinned interpreter older than `AXON_PATH_EXCLUSIVE` would not honour it;
  Fabric does not probe that capability.
- **Two other test-semantics hazards are MAJOR-ADJACENT (candidate-2 review), not claimed.**
  - A candidate's `exit(0)` counts as a clean pass whenever the grant allows IO. Every locally
    signable check has an empty ceiling, and `exit` needs IO, so no signed verdict can arise from
    it today. It must be closed before the microVM produces suite verdicts.
  - A test that returns `Err`, for example via `?` over the candidate's output, counts as a
    pass. That depends on how the operator's own suite is written.

  Both are tracked in the sweep.
- **An untested legacy guard.** A counted verdict in an evaluation from before verdict citation
  is refused at admission, but no test drives that legacy shape.
- **Learning inputs are outside G01.** EVO `propose` reads caller-supplied episodes and trusts a
  verdict by issuer name, with no attestation or intake. It cannot choose the mutation, and a
  proposed candidate must still pass evaluation and admission.
- **Section 12's read row tests generic containment.** The read tool's absolute-path refusal
  applies with or without the closed-loop scope. Only the bash row exercises the scope. MiCode's
  closed-loop tool denial is a denylist (`bash`, `run_code`, `run_tests`, `check`), so a future
  tool that shells out must be added to it.
- **Parser hardening is pending.** The verdict is parsed from merged stdout and stderr. Under the
  empty ceiling, candidate code cannot print, and a failing named test's own result line cannot
  be suppressed, so a fail cannot become a pass (reasoned, not executed). A dedicated result
  channel is still pending.
- **One guard is an equivalent mutant.** No test can make EVL's cited-only-if-counted filter
  (X23) fire alone, and it is documented where it stands. Re-audit 4 showed X18, the intake
  role-upgrade rule, is not equivalent. It is the only intake guard for that case, and is now
  M45.
