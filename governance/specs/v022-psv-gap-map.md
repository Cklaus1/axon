# v0.22 PSV gap map — what exists, what the dev host can prove, what must be built

Status: **ANALYSIS, written before any PSV code** (operator direction, 2026-09-28). This maps
`v022-protected-suite-verdict.md` (PSV-1..7) onto the code at `b590f536`. It changes nothing and
certifies nothing.

Sources:
- a read-only survey of axon-s3-veto and micode-v022-s5d;
- the survey's load-bearing claims re-checked by hand. Each is marked ✔ where it is used.

## Invariant: readiness does not move

Even with every PSV row green on the development host:

```
protected_backend            NOT_RUN
g01_on_protected_backend     NOT_RUN
pci_on_protected_backend     NOT_RUN
PROTECTED_VERIFIER_READY     NOT_READY
```

These change only when the operator-controlled environment produces signed certification evidence.

| The dev host proves | Only the protected host proves |
|---|---|
| protocol semantics | the trusted issuer |
| the negative matrix | the operator-owned observer |
| mutation sensitivity | the qualified backend |
| guest behaviour | the exact installed verifier |
| receipt joins | actual protected execution |

## Headline

The protected path cannot run a check today. `select()` offers `linux-microvm-protected` only
`interpreter_run`, and refuses `registered_check` with "it does not run registered checks or
produce a verdict" (`crates/axon-fabric/src/backend.rs:918-924`). The guest runs
`axon run /work/job/program.ax` (`profiles/linux-microvm/guest-init.sh:143-144`) and reports only
an exit code.

Two further facts, both ✔:
- The pinned guest `axon` was built at `75faf48d` (`profiles/linux-microvm/manifest.json`). That
  commit does not descend from PCI-certified `31413ca7`, so the guest binary lacks the certified PCI
  interpreter.
- Fabric attests any registered-check verdict whose backend is protected **or whose effect ceiling
  is empty** (`crates/axon-fabric/src/signing.rs:47`). Nothing in the attestation distinguishes a
  guest-path verdict from a local one except the `backend_profile_ref` string.

## The matrix

Every clause has two rows: what the development host must establish, and what only the protected
host can supply (PROTECTED_ONLY).

Classifications: **ALREADY_PROVEN** (mechanism and tests exist), **PARTIAL**, **MISSING**,
**PROTECTED_ONLY**.

| PSV | Class | Existing mechanism | What the dev host can prove | Missing mechanism | Protected-host-only evidence | Negative tests |
|---|---|---|---|---|---|---|
| PSV-1 operator suite, not the candidate's | **PARTIAL** | Local path: suite pinned by id + WorkspaceVersion + entry (`submit.rs:740-823`); intake enforces `task_acceptance` and the exact `check-suite:` argv (`axon-loop/src/intake.rs:820-870`); candidate rubric refused (`signing.rs:32-49`) | The guest runs exactly the registered suite and test, and nothing the candidate names | A guest runner that executes a suite at all (M1); the registry origin (F2 below) | — | Local: intake refusal tests. Guest: none |
| PSV-1 | PROTECTED_ONLY | — | — | — | The suite registry installed under operator control on the protected host | — |
| PSV-2 sealed, digest-bound inputs | **MISSING** (guest) | Local: separate materialisation (`submit.rs:805-810`), `AXON_PATH` + `with_sealed_dir` (`submit.rs:1295-1303`). Guest: one `--program` file (`backend.rs:1225-1231`). The launcher has an unused `--put` (`scripts/fc_linux_profile.sh:148, 383-388`) | Two trees delivered separately; the guest re-digests both and refuses on any mismatch with the launch manifest | Tree transport + in-guest digest check + launch manifest (M1) | — | None |
| PSV-3 PCI inside the guest | **MISSING** (guest); **ALREADY_PROVEN** (local, PCI certification `31413ca7`) | Completion key and token are host-only (`submit.rs:1275, 1297, 1464-1487`); `linux_receipt` hard-codes `ReceiptVerification::NotRequested` (`submit.rs:1590-1600`) | A trusted runner in the guest produces per-test completion tokens under a fresh key; a pass without them is Unknown | A guest `axon` ≥ `31413ca7`; key delivery to the runner, never the candidate; host-side token check on the guest report (M1, M2) | That the qualified guest image carries that runner (a new B263 run of the rebuilt image) | Local: PCI certification suite. Guest: none |
| PSV-4 Fabric signs only a guest-path verdict | **PARTIAL** | `attestation_decision` (`signing.rs:32-49`): registered check, operator suite, not replayed, and protected backend OR empty ceiling; B263 re-checked at dispatch (`submit.rs:1258-1269`, `backend.rs:606-837`) | A protected attestation is issued only for a verdict carrying guest completion evidence; a local verdict can never be labelled protected | Protected verdict format + tightened decision: a verdict is attested as protected only when it is guest-produced and completion-verified (M2) | That the signing key is the operator-installed verifier's (see F9) | Replay refusal exists. The dev-labelled-protected case has no test at this layer |
| PSV-5 the receipt binds everything | **PARTIAL** | Present: candidate `input_workspace_ref` (`submit.rs:358`); task/trial/attempt/operation (`submit.rs:346-349`, `attestation.rs:56-60`); verifier issuer and key (`attestation.rs:52-53, 115-117`); local suite ref (`submit.rs:1329-1332`). Absent: guest image/kernel/runtime digests (computed in `LinuxQualification`, `backend.rs:358-379`, never emitted), suite on the linux receipt, observation digest, launch-manifest digest | Every field present, signed, and joined by intake/EVL; each join refuses on a single-field change | Receipt fields + loop-side joins (M2, M4) | — | Present: wrong verifier key; mismatched trial/attempt/operation (`axon-loop` intake and attestation tests). The rest have none |
| PSV-6 fresh observer evidence joins the launch | **MISSING** | ADR-002 prose only (`governance/specs/v022-ADR-002-preflight-observer.md:37-49`). The `TrustAuthority::Observer` signature domain exists (`backend.rs:194-218`). The nearest analogue, the G32 `ExecutionContextReceipt` window + epoch (`axon-loop/src/evl.rs:510-524`), has no nonce, digests or launch manifest | Schema; observer-domain signature; nonce, epoch and age; one use; exact join to the launch manifest; Fabric can consume but not mint | PreflightObservation + nonce store + Fabric consumption + intake join (M3) | An observer deployed with an operator-authorised key in `/etc/axon/trust/observer/` | None |
| PSV-7 development cannot pass as protected | **PARTIAL** | `PROTECTED_PROFILES` (`axon-loop-contracts/src/lib.rs:73`); EVL treats a dev backend on either leg as Unverifiable (`evl.rs:880-905`); admission/pointer class gates; readiness (`readiness.rs`) | The loop refuses protected class unless it verifies qualification, launch manifest and observation, not a profile name | Loop-side verification of the qualification evidence (0 references to `qualification-` in `axon-loop/src`, ✔) (M4) | The B263 qualification of the protected host | ALREADY_PROVEN: `a_protected_plan_counts_only_protected_backends`, `each_d3_leg_on_a_development_backend_counts_nothing` (`axon-loop/tests/protected_class.rs:76, 325`); readiness adversarial suite |

## Trusted producer of every load-bearing field

A protected field is sound only if its producer is outside the reach of MiCode, the candidate,
the repository and any caller-supplied path. **F-rows marked GAP are fields that can still
originate from one of those today.**

| # | Field | Must be produced by | Produced today by | Status |
|---|---|---|---|---|
| F1 | candidate tree digest (WorkspaceVersion) | Fabric, over the tree in its store | Fabric (`input_workspace_ref`, `submit.rs:358`) | OK on the host. **GAP** in the guest: never re-digested there |
| F2 | suite id@version#entry, suite digest | the operator suite registry | Fabric's `--check-registry PATH` (`bin/axon-fabric.rs:359`), a **path the caller supplies** | **GAP**: the file that DEFINES the operator suite comes from a caller-supplied path. It must be operator-owned (the `/etc/axon/trust` discipline) or bound into the qualification |
| F3 | test id | the experiment's `task_acceptance` | request `argv[1]` from the caller; intake later checks it against `task_acceptance` | PARTIAL: joined at intake, but nothing makes the guest run exactly it |
| F4 | completion proof | the trusted guest runner, keyed by Fabric | host-local runner only | **GAP** (missing on the guest path) |
| F5 | guest image / kernel / runtime digests | Fabric's launch configuration, pinned by the qualified manifest | `result.json`, written by the **launcher**, which is `--linux-launcher PATH` (`bin/axon-fabric.rs:369`), a repository script (`scripts/fc_linux_profile.sh`) | **GAP**: the manifest is pinned by the qualification record's sha; the launcher is not. A replaced launcher could report any digests and any exit. It must be operator-installed and pinned like the verifier |
| F6 | launch manifest digest | Fabric | — | **GAP** (missing) |
| F7 | preflight observation | the observer, under `/etc/axon/trust/observer/` | — | **GAP** (missing) |
| F8 | qualification (B263) | operator trust root `/etc/axon/trust/qualification/` | Fabric verifies it (operator root, ✔ `b590f536`); the loop never reads it | PARTIAL: verified by Fabric only; the loop trusts the profile name |
| F9 | verifier identity / key | operator trust root `/etc/axon/trust/verifier/` | the loop store config: `trusted_verifiers` + `verifier_keys` (`axon-loop/src/store.rs:48, 67`), a file written by whoever sets up the experiment | **GAP**: the keys that authenticate every verdict are not under the operator root |
| F10 | trial / attempt / operation | the experiment (axon-loop), bound by Fabric | request → Fabric binding → attestation (`attestation.rs:56-60`) | OK |
| F11 | backend profile | Fabric, signed in the attestation | Fabric | OK as a claim; its truth depends on F5, F8 and M4 |
| F13 | Fabric attestation signing key | the operator host config, readable only by the Fabric UID | named INSIDE the caller-supplied check registry (`signer.key_path`, resolved relative to it, `bin/axon-fabric.rs:151-181`) | **GAP**: the key that signs every verdict is located by a caller path. Closed by O1 (`v022-psv-protocol.md` §2) |
| F12 | readiness verdict | the operator-installed verifier | `/etc/axon/trust/verifier.json`-pinned `verify-readiness` (`b590f536`) | OK |

## The mechanisms to build (four, plus two origin fixes)

| # | Mechanism | Closes |
|---|---|---|
| **M1** | **Guest verdict runner.** Launch manifest (`axon-launch-manifest/1`); candidate and suite trees delivered as separate inputs; in-guest re-digest and refusal; `axon test` of exactly the registered test under the PCI completion key; report + tokens + manifest digest out. Needs a guest `axon` rebuilt at or after `31413ca7` | PSV-1, PSV-2, PSV-3 (guest), F1, F3, F4, F6 |
| **M2** | **Protected verdict + Fabric dispatch.** `registered_check` on the protected profile; host-side completion check (Unknown without it); receipt names launch manifest, guest digests, suite; protected attestation only for a guest-produced, completion-verified verdict | PSV-4, PSV-5 (producer side) |
| **M3** | **PreflightObservation.** Schema; observer-domain `axon-evidence-signature/2`; custodian nonce + epoch + age; one-use store; Fabric consumes, cannot mint | PSV-6, F7 |
| **M4** | **Loop-side protected joins.** Intake/EVL verify the launch-manifest digest, the observation and the guest digests, joined receipt to manifest to observation; protected class requires all of them, not a profile name. The guest digests are joined to the B263 qualification only producer-side, by Fabric's dispatch (`psv::prepare` from the qualified profile manifest); the loop carries no qualification record and joins only the receipt's `qualification-sha256` to the manifest's (protocol amendment 42) | PSV-5 (consumer side), PSV-7, F8 |
| O1 | **Operator-pinned launcher and suite registry.** The protected launcher and check registry resolve from operator-owned locations (or are bound into the qualification), never a caller path | F2, F5 |
| O2 | **Verifier keys from the operator root.** In the protected class, verifier keys come from `/etc/axon/trust/verifier/`; the store may narrow, never add | F9 |

## Dependency order

Implementation follows dependencies, not clause numbers:

```
guest input transport                     M1   (launch manifest, --put of two trees)
  ↓
separate candidate + suite digest binding M1   (in-guest re-digest, refuse on mismatch)
  ↓
guest registered-suite execution          M1   (rebuilt guest axon ≥ 31413ca7)
  ↓
guest PCI completion proof                M1   (key to runner only; tokens out)
  ↓
protected verdict format                  M2
  ↓
Fabric attestation / receipt join         M2 + O1
  ↓
observer / preflight join                 M3
  ↓
loop-side joins                           M4 + O2
  ↓
negative matrix complete (all rows discriminating)
```

## Adversarial matrix — written before the mechanisms

Each case must eventually be REFUSED at the named point and FOR that reason; a test asserts the
reason, not just the refusal. "Today" is the state at `b590f536`.

| # | Case | Must be refused by | Discriminating mechanism | Today |
|---|---|---|---|---|
| A1 | wrong candidate tree | guest runner digest check; receipt `input_workspace_ref` join | M1, M4 | host join only |
| A2 | wrong suite (id, version or digest) | guest runner; intake suite join | M1, M4, O1 | intake only (local) |
| A3 | wrong test id | guest runner runs only the manifest's test; intake `task_acceptance` | M1, M4 | intake only |
| A4 | wrong trial | intake identity join | existing | **ALREADY_PROVEN** |
| A5 | wrong attempt | intake identity join | existing | **ALREADY_PROVEN** |
| A6 | wrong guest image | launch-manifest ↔ observation ↔ receipt join | M2, M3, M4 | launcher pin only (a repository script, F5) |
| A7 | wrong kernel / runtime | same as A6 | M2, M3, M4 | same as A6 |
| A8 | stale observer evidence | Fabric at launch: age (`max_age_s`) + one-use custodian nonce; loop: epoch join (A30) — NOT age (C9 correction) | M3, M4 | proven at Fabric (negative-matrix A8, A15, A30) |
| A9 | observer signature from the wrong authority domain | `RULE:authority-domain` | exists (`b590f536`), must be wired to M3 | mechanism proven; no observation to apply it to |
| A10 | wrong verifier | intake attestation verification | existing + O2 | **ALREADY_PROVEN** for the key; key origin is F9 |
| A11 | replayed completion proof | fresh per-run key; token bound to operation + launch manifest | M1, M2 | local: fresh key per run (PCI); guest: missing |
| A12 | missing completion proof | verdict Unknown, never attested as protected | M1, M2 | local proven; guest missing |
| A13 | development backend labelled protected | protected attestation needs guest evidence; loop needs qualification + manifest + observation | M2, M4 | loop class gate by NAME only |
| A14 | guest verdict without a protected preflight | intake: no observation ⇒ not protected | M3, M4 | missing |
| A15 | replayed observation (reused nonce) | one-use nonce store; since amendment 50 the CUSTODIAN's, spent by the root helper (A84) | M3 | proven at Fabric and at the root boundary (A15, A84) |
| A16 | replaced launcher reporting chosen digests or exit | operator-pinned launcher | O1 | **open** (F5) |
| A17 | caller-supplied suite registry | operator-owned registry | O1 | **open** (F2) |
| A18 | verifier key planted in the loop store | protected class reads keys only from the operator root | O2 | **open** (F9) |
| A19 | completion key reachable by the candidate in the guest | runner holds the key; candidate sealed per PCI | M1 | local proven; guest missing |

## Where this leaves PSV_PROTOCOL_PROVEN

- Rows ALREADY_PROVEN at the dev layer: PSV-7's backend-class gate, plus A4, A5 and A10.
- Everything else needs M1–M4 and O1–O2.
- Nothing here moves readiness.

## Progress

This section is appended as mechanisms land; the rows above stay as they were at `b590f536`.

| Step | State | Closes | Evidence |
|---|---|---|---|
| 0 schemas + O1 contract | frozen | — | `v022-psv-protocol.md` (`f784baa5`) |
| O1 | landed (dev) | F2, F5, F13; A16, A17, A20, A21 | `crates/axon-fabric/src/protected_host.rs`; `tests/protected_host.rs` (9); `tests/submit.rs::a_launcher_replaced_after_eligibility_never_runs`; trust preflight O1/A20 controls. 12/12 O1 mutants killed against a green baseline |

The O1 host-config path makes the pins sound. What it cannot show on the dev host is PROTECTED_ONLY:
the operator installing `/etc/axon/protected-host.json`, and the protected-mode preflight passing
against it.
| M1 step 1: shared recipe | landed | F1 (same digest code host + guest) | `crates/axon-workspace-recipe`; `tests/workspace.rs::the_guest_tree_digest_is_the_store_reference` (`655fb08f`) |
| M1 step 2: protocol crate | landed (dev) | A1, A2, A3, A11 at the format level | `crates/axon-psv` (6 tests): manifest canonical and verified only under the named digest; inputs checked separately and named; completion key bound to all 11 identities + secret; RFC 4231 HMAC. Mutation: 15 killed, 1 EQUIVALENT (`sort-nested`: no `preserve_order` in the build), not counted as killed |
| M1 step 3: trusted guest runner | landed (host-tested, real interpreter) | A1, A2, A3, A11, A12, A19 at the runner | `crates/axon-psv/src/runner.rs` + `bin/axon-psv-runner`; `tests/runner.rs` (9). Order: secret → manifest (exact named digest) → both tree digests → test identity → only then `axon test` of exactly that entry+test. S never leaves the runner; the child gets K as one stdin line then EOF, a cleared env, no-new-privs, and (as root) uid nobody. Mutation: 12/12 killed. Four survived the first set — including a FALSE GREEN in the custody test (the secret is not UTF-8, so `read_file` failed on encoding and never reached permissions; it now requires `Permission denied`). The uid-drop leg runs only as root; a non-root run prints that it was not exercised and does not count as proof of it |
| M1 step 4: guest image + real boot | landed (dev) | PSV-2 transport, PSV-3 in the guest; A1, A12, A19 in a REAL guest | PCI-lineage image from clean `859283ea` (build refuses a non-descendant of 31413ca7), pinned in `profiles/linux-microvm/manifest.json` and `governance/status/v022-psv-protocol.json`; three read-only input drives; `scripts/psv_guest_boot_test.sh` 6/6. The first real boots found two defects the host tests could not: the rootfs `/` was 0700 (nothing unprivileged could run) and the runner shared `/out/stdout` with /init. One unexplained, unreproduced VMM death is recorded as open |
| M2: Fabric protected dispatch | landed (dev, host stand-in running the REAL runner + interpreter) | PSV-4, PSV-5 producer side; A11, A12, A13 at Fabric; A1 through Fabric | `crates/axon-fabric/src/psv.rs` (prepare/derive); `submit` dispatches only an operator suite with a named test; the certified cortex parser + Fabric's OWN key decide; the class `development` / `guest-unobserved` / `protected` is inside the signed receipt, and the attestation rule is class-based. `tests/psv_dispatch.rs` (11). Mutation: 12 killed, 1 EQUIVALENT (`no-scrub`: the run dir, job copy included, is removed when submit returns), not counted as killed. A regression from M1 was found here: `06ec49e3` changed the committed profile manifest that a historical-record test judged against; fixed by pinning that record's manifest as a fixture |
| M3: preflight observation | landed (dev; stand-in observer proves the PROTOCOL, not the measurement) | PSV-6; A8, A9, A14, A15 at Fabric; A6/A7 as observed-vs-launch joins | `crates/axon-fabric/src/observer.rs` (custodian `NonceStore`: issue once, atomic consume, epoch, age; `observe`: pinned observer program, OBSERVER-domain signature under the OBSERVER root only, signer = claimed key, field-for-field join to Fabric's own manifest, freshness, epoch; since amendment 50 Fabric consumes nothing: the root helper re-verifies the observation and spends the nonce through `axon-custodian`); `axon-psv::PreflightObservation::joins`; O1 host config gains an optional `observer` section. A guest verdict is `protected` ONLY with a verified observation, and a defective observation refuses the LAUNCH. `tests/psv_dispatch.rs` (+5 incl. 9 defect modes, a genuine signed observation replayed, the store's rules). Mutation: 14/14 killed |

Open for M3 (PROTECTED_ONLY): a real observer that MEASURES the host independently of Fabric, deployed with an operator-authorised key in `/etc/axon/trust/observer/`. The custodian is now a separate component (amendment 50: `axon-custodian`, its own uid, socket-activated, the only issuer, store and spender of the nonce; the root helper spends it). What stays PROTECTED_ONLY is its DEPLOYMENT: the custodian uid, `/etc/axon/custodian.json`, the systemd units (examples in `profiles/protected-host/systemd/`) and a protected-mode trust preflight over them.
| O2: loop keys from the operator root | landed (dev) | F9; A18 (verifier + observer); key revocation at the root | `axon-loop-contracts::operator_trust` is now the ONE trust-root implementation; Fabric re-exports it. `Config::rooted_key`: for PROTECTED evidence, a store key counts only if `/etc/axon/trust/{verifier,observer}/` also holds it. The store narrows and never adds; development keeps store keys. Applied at intake (a receipt claiming `evidence-class:protected`), EVL's protected context, and admission's verifier + observer re-checks. The test root is per-thread and feature-gated (`test-trust-root`); production has no setter. Tests: planted verifier key (intake), planted observer key + operator-install control (EVL), revocation at the root (admission). Mutation: 6/6 killed |
| M4: loop-side protected joins | landed (dev) | PSV-5 consumer side, PSV-7; A13, A14 at the loop; the guest-interpreter join | `axon-loop-contracts::protected_evidence::check`, one pure check used by intake (for any receipt CLAIMING protected) and EVL (for every counted verdict in a protected evaluation). It requires: one class, `protected`; a protected backend; the launch manifest, observation, guest verdict and guest kernel/rootfs/axon digests each exactly once; and the guest interpreter equal to the request's pinned executable. Anything less is Unverifiable. It also found an M2 gap: the guest receipt's suite ref was not the canonical `check-suite:id@ver#entry` that intake pins compare, so every genuine protected receipt would have been refused at intake; fixed. The fixture executable digest was an arbitrary placeholder and is now the genuine acf1 identity, so the join can be tested. Mutation: 8/8 killed (one first "kill" was a compile error; re-run as a valid mutant) |
| Negative matrix A1-A21 | complete (dev) | all rows | `governance/specs/v022-psv-negative-matrix.md`: each row gives its refusal point(s), the reason asserted and `file::fn` tests; `scripts/psv_matrix_check.py` (in `gate.sh`) fails on a missing row or a stale citation (negative control: a renamed test fails it). New this cycle: Fabric-level A2 (suite changed under the guest) and A3 (a test the suite does not define). `psv_guest_boot_test.sh` is now in `gate.sh`, where SKIP is reported as a non-result |

| Review fixes (candidate 3) | landed (dev) | B1, B2, B3 and the five majors of review wf_d725935a-7ed; A22, A23, A24 | Protocol amendments 5–10. B1: the interpreter never collects a sealed module's `@[test]`, and the runner uses `--exact`. B3: inputs are re-materialized from the store into a private dir, and tree == version is enforced at `prepare`, in the manifest and in the guest (no symlinks, no omitted entries). B2: the `axon-psv-evidence/1` bundle, required at intake and joined over the exact documents under the operator observer root; MiCode keeps it beside the receipt. The majors: `matched_checks`, `verifier_sha256`, the guest-init and qualification refs, the secret scrubbed before verify, and verify with `env_clear`. Mutation: M219–M238 20/20 killed. M176 is retired as EQUIVALENT under `--exact` and not counted |
| Review fixes (candidate 4) | landed (dev) | the PSV-4 and PSV-7 blockers of review wf_1bc28496-38e; A25, A26 | Protocol amendments 11 and 12. PSV-4: the interpreter keys failures in their own domain; the runner and Fabric decide an outcome only from exactly one keyed line; Failed also needs a failing exit; `derive` reads only the launcher-bound verdict bytes. PSV-7: a fifth trust authority, `monitor`; a protected-scope clearance and admission's re-derivation need an operator-rooted monitor key. Mutation: M239–M245, 7/7 killed. A first set exposed two kills that did not rest on the guard they name, and one survivor; each was re-targeted to a scenario where that guard alone decides (a dying interpreter that leaves a lone unkeyed failure line) |
| Review fix (candidate 5) | landed (dev) | the PSV-3 blocker of review wf_ecfcd666-6c9; A27 | Amendment 13. The review read it from the code; it was then REPRODUCED in the real guest: under an Exec grant, candidate code recovered K from the axon process's memory, and HMAC under it matched the real pass token. Fix: a key-holding `axon test` is non-dumpable, spawns nothing, and reads K raw; the guest runner also strips `Exec`. Controls: without a key the parent opens the child's memory and `exec` spawns. Mutation: M246–M249, 4/4 killed. Yama in the guest kernel is a recorded operator follow-up |
| Review fix (candidate 6) | landed (dev) | the PSV-1 blocker of review wf_293dfdb6-9d8; A28 | Amendment 14. The module loader no longer falls through on an unreadable module, so a candidate cannot replace a suite module. The reviewer executed the attack to a keyed PASS. Mutation: M250 killed; reverting the fix reproduces the attack's keyed `ok` line. Real-guest `fall` case added. Candidate 5's review registered PSV-3, so the key-custody fix held |
| Dev-loop round 1 fixes | landed (dev) | five executed blockers of dev round wf_336353cb-a2b (PSV-1, PSV-6 ×2, PSV-7 ×2); A29–A32 | Amendments 15–18: sealed imports, the epoch join plus Fabric's post-observation recheck, protected admission re-verifying every verdict from its documents, and a Fabric-attested execution leg. Mutation: M251–M259, 9/9 killed (M257 first survived: its test signed with a key the store did not hold; reworked so the operator root alone decides). Candidate 6's certifying review had registered 8/8 on the same code; the dev round shows why the loop exists |
| Dev-loop round 2 fixes | landed (dev) | four blockers of dev round wf_7cb5856d-806 (PSV-1 regression of amendment 15, PSV-7 ×2, PSV-3); A33–A36 | Amendments 19–22: one file per module name (the round-1 mechanism was itself attackable), protected counters grounded in re-verified trials over the frozen population, clearance signatures re-verified, and no non-protected dispatch on a protected host. Mutation: M251 retargeted, M260–M265 new, all killed. Correction (C9 round 1, amendment 39): M262's kill was a refusal-reason mismatch, not its attack; it is now killed by the class DOWNGRADE attack, where it is the only guard |
| Dev-loop round 3 fixes | landed (dev) | five blockers of dev round wf_bf757240-925 (FIELD-ORIGIN, PSV-7 ×3, PSV-1); A37–A40 | Amendments 23–25: isolated launcher Python; each counted protected trial's stored record joined to its re-verified documents (arm policy, signed outcome, stored+re-verified context signature); and no RNG reseed from a sealed frame. Mutation: M266–M270 killed |
| C9 round 1: mutation evidence model | landed (dev; harness) | class (e), the harness judging itself (EQUIVALENCE findings of the C9 dev review) | Amendment 39. A row is KILLED only when the panic that fails its test matches its per-row attack marker (`scripts/v022_attack_markers.py`); otherwise REFUSED_ELSEWHERE, never counted killed. The failing panic is the last one on the thread. STALE needs a killed ACTIVE replacement. "N/N killed" in this map before amendment 39 counted any failure as a kill; the 308-row C9 dev run had 45 rows whose recorded kill was not their attack. New rows M375-M385; M204 ACTIVE again |
