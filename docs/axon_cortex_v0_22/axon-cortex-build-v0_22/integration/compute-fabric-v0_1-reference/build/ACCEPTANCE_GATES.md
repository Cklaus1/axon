# Acceptance gates

**All 55 proposed product gates are NOT_RUN.** This inventory defines implementation obligations. There is no stub product test that returns PASS. Package/schema examples and source syntax checks are reported separately.

## Evidence rules

A gate record binds gate ID, source revision, implementation test path, invoker/CI job, backend profile/config digest, environment, timestamps, actual assertions, output artifact hashes and result. Tests must include allowed positive controls and adversarial negative cases. PASS requires executed nonzero assertions and the appropriate physical target; SKIPPED/UNKNOWN/BLOCKED/NOT_RUN are never PASS. A test must be connected to a real invoker, not merely listed.

M3/M4 gates apply only to their extension. An unsupported extension remains unsupported rather than blocking the initial native release. Required M1 gates cannot be skipped because a later backend might be stronger.

## ACF-G00 — source-binding

**Owner:** ACF-T00. **Milestone:** M0. **Environment:** offline. **Result:** NOT_RUN.

**Fixture:** Package against wrong source or missing newer package.

**Acceptance:** Compare reviewed hashes and actual checkout; report every drift; require an approved mapping before editing.

## ACF-G01 — legacy-regression

**Owner:** ACF-T00. **Milestone:** M0. **Environment:** rust+existing-hosts. **Result:** NOT_RUN.

**Fixture:** Existing behavior and formats regress.

**Acceptance:** Run existing relevant Rust tests and host gates; missing prerequisites are NOT_RUN/BLOCKED, never PASS.

## ACF-G02 — strict-wire

**Owner:** ACF-T01. **Milestone:** M0. **Environment:** rust. **Result:** NOT_RUN.

**Fixture:** Unknown fields, duplicate escaped keys, malformed/oversized values.

**Acceptance:** Refuse before dispatch and emit typed parse/validation evidence.

## ACF-G03 — authority-convergence

**Owner:** ACF-T01. **Milestone:** M0. **Environment:** rust. **Result:** NOT_RUN.

**Fixture:** New run/replay/attach/fork route bypasses approval.

**Acceptance:** Denied or revoked request causes zero backend effects on every public route.

## ACF-G04 — identity-separation

**Owner:** ACF-T01. **Milestone:** M0. **Environment:** rust. **Result:** NOT_RUN.

**Fixture:** Same task/arm repeated, transport retry and new attempt confused.

**Acceptance:** New trials/attempts are unique; identical operation retry deduplicates; changed inputs under same operation ID refuse.

## ACF-G05 — profile-evidence

**Owner:** ACF-T01. **Milestone:** M0. **Environment:** rust. **Result:** NOT_RUN.

**Fixture:** Independent capability flags imply untested combinations.

**Acceptance:** Only exact fresh verified host/profile/config combinations admit required protection; unknown is not supported.

## ACF-G06 — no-weak-fallback

**Owner:** ACF-T01. **Milestone:** M0. **Environment:** rust. **Result:** NOT_RUN.

**Fixture:** Required enclosure unavailable or feature disabled.

**Acceptance:** Protected request is explicitly unsupported/refused, not routed to process_scoped.

## ACF-G07 — artifact-authority-binding

**Owner:** ACF-T01. **Milestone:** M0. **Environment:** rust. **Result:** NOT_RUN.

**Fixture:** Executable/dependency/workspace/placement changed after approval.

**Acceptance:** Readmission required; mismatch has no realized effect.

## ACF-G08 — concurrent-staging

**Owner:** ACF-T02. **Milestone:** M1. **Environment:** linux. **Result:** NOT_RUN.

**Fixture:** Two requests share PID/stem and execute simultaneously.

**Acceptance:** No wrapper/socket/output collision; each attempt reads and deletes only its own private resources.

## ACF-G09 — legacy-verdict

**Owner:** ACF-T02. **Milestone:** M1. **Environment:** rust+interpreter. **Result:** NOT_RUN.

**Fixture:** Fault, returned integer, timeout, cancel and CLI JSON paths.

**Acceptance:** Preserve existing verdict and exit meanings with actual tests; do not map every zero exit to verification.

## ACF-G10 — bounded-process-io

**Owner:** ACF-T02. **Milestone:** M1. **Environment:** linux. **Result:** NOT_RUN.

**Fixture:** Endless output, invalid encoding and slow subprocess.

**Acceptance:** Bounded memory/output/deadline, attributable truncation and complete resource cleanup.

## ACF-G11 — aggregate-budget

**Owner:** ACF-T03. **Milestone:** M1. **Environment:** rust+fault-injection. **Result:** NOT_RUN.

**Fixture:** Concurrent children and restart oversubscribe parent.

**Acceptance:** Atomic reservations preserve every configured bound, with no refund of settled spend on restore.

## ACF-G12 — operation-journal

**Owner:** ACF-T03. **Milestone:** M1. **Environment:** fault-injection. **Result:** NOT_RUN.

**Fixture:** Crash before/after dispatch or receipt write.

**Acceptance:** Durable operation identity survives; same operation does not duplicate known effect; uncertain state remains unresolved.

## ACF-G13 — unknown-effect

**Owner:** ACF-T03. **Milestone:** M1. **Environment:** fake-remote. **Result:** NOT_RUN.

**Fixture:** External effect possibly completed without receipt.

**Acceptance:** OutcomeUnknown requires reconciliation or approval; no blind retry/provider failover.

## ACF-G14 — cancel-descendants

**Owner:** ACF-T03. **Milestone:** M1. **Environment:** linux+fake-remote. **Result:** NOT_RUN.

**Fixture:** Detached children, blocked imports, late remote replies.

**Acceptance:** Stop prevents new work, reaches supported descendants under bound; unknown remote stop is visible.

## ACF-G15 — fencing

**Owner:** ACF-T03. **Milestone:** M1. **Environment:** rust+fault-injection. **Result:** NOT_RUN.

**Fixture:** Expired lease/stale epoch tries execute/promote/resume.

**Acceptance:** Refuse stale operation and keep cancellation latched; active operator grant required to reset.

## ACF-G16 — cost-liability

**Owner:** ACF-T03. **Milestone:** M1. **Environment:** rust+fake-remote. **Result:** NOT_RUN.

**Fixture:** Invoice missing, cancel uncertain or estimation differs.

**Acceptance:** Separate settled/reserved/unresolved liability; no unknown cost represented as zero.

## ACF-G17 — durable-workspace

**Owner:** ACF-T04. **Milestone:** M1. **Environment:** filesystem. **Result:** NOT_RUN.

**Fixture:** Delete original workspace after capture.

**Acceptance:** Restored content/metadata matches declared scope; absent/unobserved/excluded states remain distinct.

## ACF-G18 — materializer-escape

**Owner:** ACF-T04. **Milestone:** M1. **Environment:** linux. **Result:** NOT_RUN.

**Fixture:** Traversal, symlink/hardlink/device and archive expansion.

**Acceptance:** No access outside approved root; unsupported entries refuse; quota checks are effective.

## ACF-G19 — tenant-storage

**Owner:** ACF-T04. **Milestone:** M1. **Environment:** rust+filesystem. **Result:** NOT_RUN.

**Fixture:** Cross-tenant artifact/cache/checkpoint handle.

**Acceptance:** Read/write/export denied independent of guessed content hash or handle.

## ACF-G20 — retention-gc

**Owner:** ACF-T04. **Milestone:** M1. **Environment:** fault-injection. **Result:** NOT_RUN.

**Fixture:** GC during capture/restore or active branch.

**Acceptance:** Referenced artifacts remain available; incomplete captures never become restorable; cleanup is observable.

## ACF-G21 — vm-cli-parity

**Owner:** ACF-T05. **Milestone:** M1. **Environment:** rust+kvm. **Result:** NOT_RUN.

**Fixture:** Library extraction changes existing CLI/result semantics.

**Acceptance:** Existing axon-vm tests and relevant R-series gates pass without changing their expectations.

## ACF-G22 — launch-failure-cleanup

**Owner:** ACF-T05. **Milestone:** M1. **Environment:** linux+kvm+fault-injection. **Result:** NOT_RUN.

**Fixture:** Error at each VMM/socket/device/relay acquisition.

**Acceptance:** No untracked live resources; cleanup failures stay journaled for reconciliation.

## ACF-G23 — real-linux-workload

**Owner:** ACF-T06. **Milestone:** M1. **Environment:** kvm. **Result:** NOT_RUN.

**Fixture:** VM starts or kernel demo halts without running target.

**Acceptance:** Actual Linux guest executes pinned Axon/registered fixture and returns expected artifact/result; demo cannot satisfy gate.

## ACF-G24 — loaded-artifact

**Owner:** ACF-T06. **Milestone:** M1. **Environment:** kvm. **Result:** NOT_RUN.

**Fixture:** Swap rootfs/program/interpreter after measurement.

**Acceptance:** Loaded content binding mismatch blocks execution/result admission; source_hash label alone is insufficient.

## ACF-G25 — guest-policy-channel

**Owner:** ACF-T06. **Milestone:** M1. **Environment:** kvm. **Result:** NOT_RUN.

**Fixture:** Absent/empty/malformed/mismatched boot policy.

**Acceptance:** Exact Linux guest/init protocol fails closed without development bypass; demo uses separate profile.

## ACF-G26 — scope-preservation

**Owner:** ACF-T06. **Milestone:** M1. **Environment:** kvm. **Result:** NOT_RUN.

**Fixture:** Path/host-specific grant projected into VM policy.

**Acceptance:** Allowed operations succeed and out-of-scope realized reads/writes/network fail; unsupported axis refuses.

## ACF-G27 — host-enclosure

**Owner:** ACF-T06. **Milestone:** M1. **Environment:** privileged-linux+kvm. **Result:** NOT_RUN.

**Fixture:** Guest/VMM requests exceed UID/cgroup/device/socket rights.

**Acceptance:** Target profile proves host and workload bounds; balloon or kernel hash alone cannot pass.

## ACF-G28 — offline-network

**Owner:** ACF-T06. **Milestone:** M1. **Environment:** privileged-linux+kvm. **Result:** NOT_RUN.

**Fixture:** Native descendants attempt DNS/raw IP/metadata/proxy egress.

**Acceptance:** No unauthorized traffic leaves the tested boundary; positive permitted controls still work.

## ACF-G29 — cortex-vertical

**Owner:** ACF-T07. **Milestone:** M1. **Environment:** rust+interpreter+kvm. **Result:** NOT_RUN.

**Fixture:** Registered check through real fabric executor.

**Acceptance:** Expected check executes, output links to exact input/profile and operator workspace remains unchanged.

## ACF-G30 — forged-result

**Owner:** ACF-T07. **Milestone:** M1. **Environment:** rust+interpreter. **Result:** NOT_RUN.

**Fixture:** Worker prints fake summaries/receipts/late duplicate results.

**Acceptance:** Untrusted output cannot supply independent success; trusted result framing binds attempt and test inventory.

## ACF-G31 — nonvacuous-check

**Owner:** ACF-T07. **Milestone:** M1. **Environment:** rust+interpreter. **Result:** NOT_RUN.

**Fixture:** Missing grader, zero match, crash or schema error.

**Acceptance:** No independent PASS; preserve failed versus did-not-run versus unknown distinctions.

## ACF-G32 — protected-evaluator

**Owner:** ACF-T07. **Milestone:** M1. **Environment:** linux+kvm. **Result:** NOT_RUN.

**Fixture:** Candidate edits grader/policy/manifests or accesses secret.

**Acceptance:** Protected files and authority stay outside writable scope; candidate cannot change required gates.

## ACF-G33 — evidence-envelope

**Owner:** ACF-T07. **Milestone:** M1. **Environment:** rust. **Result:** NOT_RUN.

**Fixture:** Old record replay or missing provenance in new attempt.

**Acceptance:** Old digests unchanged; new sidecar binds episode, trial, authority, inputs, result and evidence origin.

## ACF-G34 — branch-isolation

**Owner:** ACF-T08. **Milestone:** M2. **Environment:** linux+kvm. **Result:** NOT_RUN.

**Fixture:** Parallel A/B branches share writable data or budget.

**Acceptance:** Private branches and carved limits; one child cannot observe/write sibling secrets or spend sibling allocation.

## ACF-G35 — promotion-cas

**Owner:** ACF-T08. **Milestone:** M2. **Environment:** fault-injection. **Result:** NOT_RUN.

**Fixture:** Two candidates promote from same expected base.

**Acceptance:** Only one current approved CAS wins; stale candidate conflicts and must reverify after rebase.

## ACF-G36 — external-branch-effects

**Owner:** ACF-T08. **Milestone:** M2. **Environment:** fake-remote+kvm. **Result:** NOT_RUN.

**Fixture:** Speculative branch sends production mutation.

**Acceptance:** Denied by default; explicit approved effects use separate IDs/reconciliation; losing branch implies no rollback claim.

## ACF-G37 — repeat-trial-fairness

**Owner:** ACF-T08. **Milestone:** M2. **Environment:** rust+benchmark. **Result:** NOT_RUN.

**Fixture:** Same base/task/arm repeats with differing warmth/seeds.

**Acceptance:** Independent trial/attempt lineage retained, cache/warm/backend factors recorded and all attempts counted.

## ACF-G38 — checkpoint-completeness

**Owner:** ACF-T09. **Milestone:** M3. **Environment:** kvm+fault-injection. **Result:** NOT_RUN.

**Fixture:** Crash mid-capture or live unpinned volume.

**Acceptance:** Only complete verified manifest published; capture consistency and omissions are explicit.

## ACF-G39 — restore-authority

**Owner:** ACF-T09. **Milestone:** M3. **Environment:** kvm. **Result:** NOT_RUN.

**Fixture:** Checkpoint predates revoke/cancel/budget spend.

**Acceptance:** Fresh identity/rebind/current epoch enforced before egress; no authority/budget resurrection.

## ACF-G40 — checkpoint-compatibility

**Owner:** ACF-T09. **Milestone:** M3. **Environment:** kvm. **Result:** NOT_RUN.

**Fixture:** Wrong VMM/CPU/kernel/disk/device/GPU state.

**Acceptance:** Explicit unsupported/incompatible result; no implicit cross-provider RAM migration.

## ACF-G41 — restore-failure

**Owner:** ACF-T09. **Milestone:** M3. **Environment:** kvm+fault-injection. **Result:** NOT_RUN.

**Fixture:** Fail after some disks/memory/network are restored.

**Acceptance:** Original/destination state and remaining resources are accurately reported and fenced.

## ACF-G42 — wasm-parity

**Owner:** ACF-T10. **Milestone:** M3. **Environment:** rust+wasm-host+browser. **Result:** NOT_RUN.

**Fixture:** Axon interpreter/WASM artifact on hosted engine.

**Acceptance:** Supported workload outputs/refusals match reference under stated replay conditions; browser ABI tests still pass.

## ACF-G43 — wasm-bounds

**Owner:** ACF-T10. **Milestone:** M3. **Environment:** wasm-host. **Result:** NOT_RUN.

**Fixture:** Infinite loop, memory growth, blocking/forbidden import.

**Acceptance:** Host interrupts/bounds each case; import cannot inherit supervisor authority.

## ACF-G44 — wasm-instance-isolation

**Owner:** ACF-T10. **Milestone:** M3. **Environment:** wasm-host. **Result:** NOT_RUN.

**Fixture:** Two tenants reuse instance/cache/host context.

**Acceptance:** No mutable cross-instance data or credentials; supported cleanup and cancellation verified.

## ACF-G45 — egress-bypass

**Owner:** ACF-T11. **Milestone:** M3. **Environment:** network-lab. **Result:** NOT_RUN.

**Fixture:** DNS change, redirect, raw IP, proxy/alternate protocol/old socket.

**Acceptance:** Every unsupported or disallowed path is blocked at actual boundary; current grant checked.

## ACF-G46 — secret-protocol

**Owner:** ACF-T11. **Milestone:** M3. **Environment:** network-lab. **Result:** NOT_RUN.

**Fixture:** Credential injection on authenticated service versus generic TLS tunnel.

**Acceptance:** Only supported broker protocol succeeds; secrets unavailable in worker logs/files/RAM except explicitly declared exposed mode.

## ACF-G47 — secret-checkpoint

**Owner:** ACF-T11. **Milestone:** M3. **Environment:** storage+network-lab. **Result:** NOT_RUN.

**Fixture:** Browser cookies/data in snapshot/shared warm pool.

**Acceptance:** Sensitive artifacts tenant-scoped, encrypted/retained/exported by policy; unauthorized reuse refused.

## ACF-G48 — remote-reconciliation

**Owner:** ACF-T12. **Milestone:** M3. **Environment:** real-provider+fault-injection. **Result:** NOT_RUN.

**Fixture:** Provision success then timeout or delayed stop.

**Acceptance:** No blind duplicate/fallback; observed state/usage reconciled with scoped provider identity.

## ACF-G49 — remote-export

**Owner:** ACF-T12. **Milestone:** M3. **Environment:** real-provider. **Result:** NOT_RUN.

**Fixture:** Residency/confidentiality policy conflicts with placement.

**Acceptance:** Data never exported; candidate profile filtered before provisioning/upload.

## ACF-G50 — remote-withdrawal

**Owner:** ACF-T12. **Milestone:** M3. **Environment:** real-provider. **Result:** NOT_RUN.

**Fixture:** SDK/config evidence changes or provider disabled.

**Acceptance:** New dispatch blocked; existing jobs drained/reconciled; no automatic unsafe transfer.

## ACF-G51 — remote-accounting

**Owner:** ACF-T12. **Milestone:** M3. **Environment:** real-provider. **Result:** NOT_RUN.

**Fixture:** Start/idle/storage/egress/retries/deletion/retention charges.

**Acceptance:** Usage origin and uncertainty preserved; completed-task cost includes all billed phases.

## ACF-G52 — routing-safety

**Owner:** ACF-T13. **Milestone:** M4. **Environment:** rust+benchmark. **Result:** NOT_RUN.

**Fixture:** Learned ranking prefers cheaper ineligible backend.

**Acceptance:** Hard filters remain authoritative; no model decision expands scope.

## ACF-G53 — task-economics

**Owner:** ACF-T13. **Milestone:** M4. **Environment:** benchmark. **Result:** NOT_RUN.

**Fixture:** Faster boot increases retries or loses quality.

**Acceptance:** Evaluate total accepted-task cost and quality with all trials, fixed baseline and policy/version provenance.

## ACF-G54 — operator-attach

**Owner:** ACF-T13. **Milestone:** M4. **Environment:** interactive-profile. **Result:** NOT_RUN.

**Fixture:** Human/agent race for terminal/desktop or expired attach token.

**Acceptance:** Per-profile scoped attach and exclusive control fence; capability remains unsupported until this gate runs.
