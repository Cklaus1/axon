# G01 — verification authenticity: the claim, its evidence, and what it does not cover

Status: **NOT REGISTERED.** This is the exact statement a registration of the
G01-r22-* gate family would certify. An independent re-audit checks it clause by
clause. The registry row is written only when that re-audit finds every clause
holding and the mutation run is fully killed at the commit being registered.

History:
- Re-audit 1 (wf_f213aa49): failed.
- Re-audit 2 (wf_c11193f8): failed.
- Re-audit 3 (wf_85472584, at 87273274): failed, with three blockers, all since fixed. The fixes are FG-053 to FG-057.

## The claim

A verification verdict (passed or failed) counts toward evaluation, admission
or promotion only if all of the following hold.

1. **It is authenticated.** An Ed25519 `acf-receipt-attestation/1` is signed by Fabric's
   operator-configured signer. It binds:
   - the schema, the issuer and the key id;
   - the request and receipt digests;
   - the operation, task, trial, attempt and execution ids.

   The public key must be registered in the store's `verifier_keys`. A missing key, a missing
   attestation or a key mismatch refuses the verdict.
2. **Fabric signs only a verdict it just produced.** It signs a `registered_check` of an
   operator-registered suite (`check:<id>`), and only when all of these hold:
   - this process produced the receipt. A replay is never signed, because the journal path is
     the caller's.
   - the workload could not read the key: it ran in the protected microVM, or its admitted grant
     gives it no effect at all.
   - the check ran from an empty environment, apart from the ceiling, the trial caches and the
     module path.
3. **The verifier is trusted and independent.** The subject set is the same at intake and in
   evaluation:
   - the trial's observer;
   - the evaluation request's subject issuers;
   - the proposer of each arm policy.

   The issuer must be trusted and outside that set. The check must not have run as a subject
   principal.
4. **What ran is what the operator pinned.** This covers:
   - the verifier revision (executable and digest) and its compute profile;
   - exactly one suite identity, `check-suite:<id>@<version>#<entry>`, pinned for that verifier;
   - the task's own registered acceptance check: that suite identity plus the exact test name.

   The requester chooses neither the test, nor the version, nor the entry file. The suite's own
   modules resolve before the candidate's.
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
     fixture is served there.
7. **Evaluation reads only intaken evidence.**
   - A delivered episode counts only if intake recorded it in the scope.
   - Each counted verdict records the request, receipt and attestation digests, the issuer and
     the key id.
   - The intake record names the attestation and key that authenticated it.

## Evidence

| Clause | Tests (unit/integration) | Mutations | Real binaries (`loop_interop_gate.sh`) |
|---|---|---|---|
| 1 | `attestation.rs` unit tests; `intake.rs` authenticated and each-rule tests; `evidence_laundering.rs` unauthenticated-verdict | M02 M03 M08 | §8: attestation kept, key, operation; no-attestation refused; impostor key refused |
| 2 | `signing.rs` unit tests; `attestation.rs` (Fabric) replay and forged-journal; environment test | M01 M40–M42 M44 | §8 (2): effectful grant withheld; replay unsigned |
| 3 | `intake.rs` each-rule and proposer tests; `evidence_laundering.rs` observer at both doors; `redteam.rs` NS4 | M05 M10 M16 M37–M39 | §8 (3): check run as the observer refused |
| 4 | `intake.rs` pinned, task-only, two-version and entry tests; `check_effects.rs` shadowing | M04 M06 M07 M26–M30 M43 | §8 (4): another pinned revision refused; task and filter pin cases |
| 5 | `intake.rs` join test (failed-verdict tree, supervisor, one suite) | M09 M21–M25 M31–M36 | §8: tree, identity and supervisor on real bytes |
| 6 | `protected_class.rs` | M11–M13 M17–M20 | not exercised (see gaps) |
| 7 | `evidence_laundering.rs` intake-only and cites-evidence tests; `intake.rs` records attestation and key | M14 M15 | §8 (7): the record names the key id and the stored attestation |

The mutation run lives in `scripts/v022_g01_mutations.py`, with results in
`governance/proofs/v022-g01/`. It covers the listed guards only. It is not a
proof that no other guard exists.

## Accepted limitations (ADR-001; not claimed)

- **No operator-custodied authority root.** `verifier_keys`, `verifier_pins`, `task_acceptance`
  and `protected_scopes` are store config writable by the store's uid (D2/D5/D6). Key custody is
  a same-uid file mode, with no epoch or nonce.
- **Registries are caller-named.** The grant registry and check registry are paths the caller
  names. So the check principal in clause 3 is a name the caller can choose (it is refused only
  when it names a subject). The materialized run directory sits under a caller-named state dir,
  so a same-uid process could rewrite it mid-run.
- **The DEV environment makes no protected claim (D1).**

## Known gaps (not claimed; tracked)

- **No real protected verdict exists yet.** The Linux microVM profile emits no suite verdict
  (no filter, no suite evidence, no output tree). Every protected positive test re-labels a local
  receipt, so clause 6 has no real producer. It fails closed: a protected evaluation counts
  nothing.
- **Protecting a scope is not retroactive.** A policy already active when the scope is protected
  stays active until the next transition (ADR-001 §5 activation revalidation, pending).
- **Clause 6 has no real-binary coverage.** The interop gate never freezes or activates in a
  protected scope.
- **Parser hardening is pending.** The verdict is parsed from merged stdout and stderr. Under the
  empty ceiling, candidate code cannot print, and a failing named test's own result line cannot
  be suppressed, so a fail cannot become a pass (reasoned, not executed). A dedicated result
  channel is still pending.
- **Two guards are equivalent mutants.** No test can make them fire alone, and each is documented
  where it stands:
  - `verify_check_evidence`'s role-upgrade rule (X18);
  - EVL's cited-only-if-counted filter (X23).
