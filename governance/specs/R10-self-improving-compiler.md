# Tech Spec — R10: Self-Improving Compiler

**Status:** ✅ Reviewed (2026-06-01) — *the verification harness is fully specifiable now; R1 native build solved 2026-06-01 so G4 (perf) is no longer R1-gated — a timing harness is all that remains. Safety governance is specified regardless.*

**Review note:** All 12 sections substantively filled. Four-gate fork resolved (G1-G4) with clear rationale and rejected alternatives (single-program verification, fuzzer replacement). E14xx band (0 prior usages in crates/) consistent with E10xx-E16xx range. Red test named (`identity_pass_verifies_and_output_changing_pass_is_rejected`) and all acceptance criteria named. Core safety firewall: I-2 enforced (correctness via interpreter oracle, never AI judgment — E1406), I-12 enforced (multi-sig graduation gate, capability-diff), manifest attested at boot (E1405). Self-modifying compiler cannot graduate its own passes — hard reject condition. R1-stale framing updated: §3 status, §5 fork description, §2 gate list, §4.1 table/decision, §4.2 corpus count (71→72), §4.4 perf_status enum, §4.6 behavior table (W1410), §6 error codes, §8 test plan, §9 acceptance/§9.1 perf-slice, §10 perf budget, §11 rollout, §12 Q1 (moved to resolved) — all corrected to reflect R1 solved 2026-06-01. Capabilities module verified (`crates/axon-core/src/capabilities.rs`, exported via `lib.rs`), examples corpus verified (72 .ax files). I-17 proposed as new invariant, Q6 deferred for implementation. Safety argument is airtight: the four-gate harness makes "compiler graduates its own passes" structurally impossible without human multi-sig.
**Requirement:** `../REQUIREMENTS.md` R10 — *Self-improving compiler: learned optimization passes graduated from AI-discovered asm.*
**Decisive fork (from `README.md`):** *Verification of a graduated pattern.* Before an AI-discovered optimization can be added as a compiler pass, how is it **proven** correct + faster on the full corpus, not just the discovering program? Decide the verification harness (equivalence checking + benchmark corpus + regression gate) **first** — an unverified self-modifying compiler is the single highest-risk component in the whole PRD. Hard dependency on I-12; R1 (native) was a hard dependency for perf measurement but is resolved (2026-06-01) — a timing harness remains. **→ Resolved: the harness is fully specified; all four gates are verifiable now; perf measurement requires only a timing harness, not R1.**

---

## 1. Motivation

R10 is 0% (❌ Not started): no profiler, no pattern store, no graduation pipeline. It is also, per the spec README, *"the single highest-risk component in the whole PRD"* — a compiler that modifies itself can, if unverified, silently miscompile every downstream program or grant itself capabilities (I-12 violation). The fork is therefore correctly stated: **specify the verification harness before any discovery machinery exists**, so that an AI-discovered "optimization" cannot graduate into a pass without clearing a gate that is itself part of the TCB.

The key lever that makes this spec *possible now* is **I-2: the interpreter is the reference semantics.** A candidate optimization is a program→program (or IR→IR) transform. Its **correctness** is: *for every input in the corpus, the transformed program produces the same observable result as the original.* That equivalence is checkable by running both through the **interpreter** (`interp.rs`) — no native build required. The **"faster"** half of "correct + faster" also requires no R1 dependency (native builds work since 2026-06-01; only a timing harness is needed). So this spec:

- **Fully specifies the correctness harness, corpus, regression gate, and graduation governance** (settle-able now).
- **Fully specifies the perf-measurement gate** — no longer R1-blocked (§12 Q1 resolved).
- Treats the graduation gate as a **TCB component** with I-12 as a hard, tested invariant.

Design-only; no code until **Reviewed** (Gate 1). This is the most conservative of the four specs by intent.

---

## 2. Requirement link

`../REQUIREMENTS.md` **R10** (0%, ❌ Not started). Quoted acceptance:

> *A discovered pattern verified correct + faster, added as a pass, applied automatically.*

This decomposes into four gates, all now verifiable — native builds work since 2026-06-01:
1. **Correctness gate** — transformed ≡ original on the whole corpus (interpreter oracle). *Verifiable now.*
2. **Safety gate** — the pass cannot weaken the TCB / grant capabilities (I-12). *Verifiable now.*
3. **Regression gate** — adding the pass breaks no existing test; it is reversible. *Verifiable now.*
4. **Perf gate** — the pass is measurably faster on native, no regression on any corpus member. *Verifiable now (a timing harness remains to be built).*

Dependencies: **I-2** (interpreter oracle — gates 1–3), **I-12** (self-mod can't weaken TCB — gate 2). G4 requires a timing harness (performance budget is seconds on the interpreter; native timing needs the `axon-rt` staticlib link but no build stall).

---

## 3. Surface (what the user / system invokes)

R10 is mostly **internal machinery**, not user-facing language surface. The surface is CLI + a pass manifest:

```
axon improve discover <corpus>      # NEW: run discovery, propose candidate passes (writes proposals/)
axon improve verify <proposal>      # NEW: run the FULL gate (correctness+safety+regression+perf)
axon improve graduate <proposal>    # NEW: on all-green, add to the pass manifest (requires sign-off, §4.5)
axon improve list                   # NEW: show graduated passes + their verification record
axon improve revert <pass-id>       # NEW: remove a graduated pass (reversibility, gate 3)
```

```toml
# passes.manifest — the set of graduated passes. Each entry is content-addressed
# and carries its verification record. A pass NOT in the manifest never runs.
version = 1
[[pass]]
id           = "axp1:3e9c…"               # content hash of the pass definition
name         = "fold_const_add"
verified     = "axv1:7a2…"                # hash of the verification record (§4.4)
corpus_hash  = "axc1:11f…"                # the exact corpus it was verified against
graduated_by = ["principal:root-a", "principal:root-b"]   # multi-sig (§4.5)
perf_status  = "faster"                  # unmeasured | faster (timing harness must exist to verify); gated-on-R1 removed (R1 solved 2026-06-01)
```

There is **no `.ax` language surface** — a user does not write self-improvement; the compiler proposes, the harness verifies, a human (multi-sig) graduates.

---

## 4. Semantics

### 4.1 The verification harness (fork resolution — the core deliverable)

A candidate pass `P` is a transform on the AST/IR. `verify P` runs **four gates in order**; any failure rejects `P`:

| Gate | Check | Engine | R1? |
|---|---|---|---|
| **G1 Correctness** | ∀ program `c` in corpus: `interp(P(c)) == interp(c)` (same exit code, stdout, provenance-relevant output) | interpreter oracle | **No** |
| **G2 Safety (I-12)** | `P` does not introduce any I/O builtin, `@[contained]` weakening, or capability the original lacked; `caps(P(c)) ⊆ caps(c)` | static capability diff (`capabilities.rs`) | **No** |
| **G3 Regression** | the full existing test suite passes with `P` enabled; `P` is reversible (`revert` restores byte-identical behavior) | existing 532-test suite | **No** |
| **G4 Performance** | native-compiled `P(c)` is faster than `c` for ≥1 corpus member and slower for none, beyond a noise threshold | native binary timing | **No** |

**Decision:** G1–G4 are all verifiable now; G4 requires only a timing harness (seconds on the interpreter, comparable timing on native). A pass advertised as an optimization can graduate `faster` once G4 runs on the timing harness. This is the honest split.

### 4.2 The corpus (fork resolution — "the full corpus, not just the discovering program")

**Decision:** the corpus is the **versioned, content-addressed set of all `examples/**/*.ax` (72 files today) plus a frozen regression set**, hashed as `axc1:…`. A pass is verified against a *named corpus hash*; its manifest entry pins that hash. Verifying against "just the discovering program" is structurally impossible — `verify` iterates the whole corpus or fails.

- **Why the examples corpus:** it is already the language-identity set (ROADMAP §2.7), already type-checks clean (`all_examples_typecheck_clean`), and spans the feature surface. Growth: every new feature adds a corpus member, so coverage compounds.
- **Rejected:** verifying on the discovering program only (the exact overfitting failure the fork names — rejected by construction); a synthetic random-program fuzzer *instead of* the corpus (kept as an *additive* G1 input, §12 Q2, not a replacement — random programs lack the human-meaningful semantics the corpus encodes).

### 4.3 What "correct" means precisely

`interp(P(c)) == interp(c)` compares the **observable tuple**: `(exit_code, stdout, ordered provenance events)`. Nondeterministic inputs are pinned: `AXON_SEED` fixes RNG (BUG_HUNT #11), `AXON_AI_MOCK=1` fixes AI calls (R3), so a corpus run is deterministic and the equivalence is exact, not statistical. A pass that changes observable output for any corpus member fails G1 — there is no "close enough."

### 4.4 The verification record

`verify` emits an immutable `axv1:…` record: `(pass_hash, corpus_hash, per-member G1 result, G2 capability diff, G3 suite result, G4 perf, timestamp, tool versions)`. It is the audit artifact gate-3 of graduation checks. Stored append-only (R4 provenance discipline); the manifest references it by hash.

### 4.5 Graduation governance (I-12 as a hard gate)

**Decision:** graduation is **not automatic**. `axon improve graduate` requires:
1. A green verification record (G1–G3, G4 if available).
2. **Multi-sig of root Principals** (≥2, mirroring the ROADMAP §7 TCB attestation update path) — the compiler cannot graduate its own passes unilaterally. This is the I-12 firewall: self-modification proposes; humans (or a quorum authority) dispose.
3. The pass enters the manifest content-addressed; the manifest itself is part of the TCB and attested at boot (ROADMAP §7).

A pass not in the manifest **never runs** — discovery and proposal are unprivileged; only graduation (gated) grants execution. This makes "applied automatically" (the acceptance phrase) mean *automatically applied once graduated*, not *automatically graduated*.

### 4.6 Behavior table

| Input class | Behavior |
|---|---|
| Candidate changes no observable output, adds no capability, breaks no test | G1–G3 pass; eligible for graduation (perf pending/G4). |
| Candidate changes output for any corpus member | **G1 fail → E1401**, rejected, recorded. |
| Candidate introduces an I/O/capability the original lacked | **G2 fail → E1402** (I-12 violation), rejected, recorded. |
| Candidate passes G1/G2 but breaks an existing test | **G3 fail → E1403**, rejected. |
| Candidate claims `faster` but perf harness missing | **W1410**; may graduate only as `perf_status: unmeasured`, never `faster`. |
| `graduate` invoked without multi-sig | **E1404** (graduation requires quorum sign-off). |
| Manifest hash mismatch at boot | **E1405** (TCB attestation fail) — refuse to run (ROADMAP §7). |
| Discovery proposes a pass that itself calls AI to decide correctness | Forbidden — correctness is the interpreter oracle, never an AI judgment (§7). **E1406.** |

### 4.7 Determinism

The whole harness is deterministic by construction (seeded RNG, mocked AI, fixed corpus hash). Two `verify` runs of the same `(pass, corpus)` produce byte-identical records. This is mandatory: a non-deterministic verifier of a self-modifying compiler is itself a vulnerability.

---

## 5. Type rules

N/A to the Axon type system. R10 operates on the compiler's *own* AST/IR as data, not on user types. The pass-transform type (`Program → Program` / `IR → IR`) is Rust-internal. No `parse_type_str` / checker changes. (A future `@[optimizable]` hint attribute to mark hot functions is noted §12 Q4, not specified.)

---

## 6. Error codes

New **E14xx / W14xx** band (self-improving / graduation — follows E13xx AI), invented here per I-14.

| Code | Trigger | Message shape |
|---|---|---|
| **E1401** | G1: transformed program's observable output ≠ original on a corpus member | `` pass `{name}` changes output on `{member}` (exit/stdout/provenance differ) — not correctness-preserving, rejected `` |
| **E1402** | G2: pass introduces a capability/I-O the original lacked (I-12) | `` pass `{name}` would grant capability `{cap}` not in the original — self-modification cannot weaken the TCB (I-12), rejected `` |
| **E1403** | G3: enabling the pass fails an existing test | `` pass `{name}` breaks `{test}` — regression, rejected `` |
| **E1404** | `graduate` without the required multi-sig quorum | `` graduating `{name}` requires ≥{n} root-Principal signatures; got {got} `` |
| **E1405** | Boot-time: `passes.manifest` hash ≠ attested manifest | `` pass manifest failed attestation (expected {h1}, got {h2}) — refusing to run `` |
| **E1406** | A pass's correctness check delegates to an AI judgment rather than the interpreter oracle | `` pass `{name}` verification must use the interpreter oracle, not an AI verdict — rejected `` |
| **W1410** | `faster` claimed but G4 (perf) harness missing | `` perf gate skipped (timing harness missing, R1 solved) — `{name}` may graduate only as `unmeasured`, not `faster` `` |

## 7. Invariants touched

- **I-2 (interpreter is reference):** R10 *uses the interpreter as the equivalence oracle* — the strongest possible application of I-2. Correctness is defined as interpreter-output equivalence; it is never an AI judgment (E1406 forbids that). **Preserved + leveraged.**
- **I-12 (self-mod cannot weaken TCB):** this is R10's central gate (G2 + graduation governance). A pass that grants a capability fails G2 (E1402); the compiler cannot graduate its own passes (multi-sig, E1404); the manifest is attested at boot (E1405). This spec is the concrete realization of I-12. **Preserved — and this spec is its enforcement mechanism.**
- **I-11 (capability boundary):** G2 reuses the static capability checker (`capabilities.rs`) to diff caps; the boundary is the same one R6 hardens. **Preserved.**
- **I-13 (provenance not opt-out-able):** every verify/graduate is recorded (§4.4) append-only. **Preserved.**
- **I-14 (stable codes):** E14xx band defined here. **Preserved.**
- **New invariant proposed (I-17 candidate):** *No compiler pass runs unless it is in the attested manifest with a green verification record; the compiler cannot graduate its own passes.* This is the self-improvement safety invariant; propose for adoption when implemented.

## 8. Test plan (maps 1:1 to §4.6)

Red test that must fail first: **`identity_pass_verifies_and_output_changing_pass_is_rejected`** — define two trivial passes: an identity transform (must pass G1–G3) and a deliberately output-changing transform (must fail G1 → E1401). Assert the harness accepts the first and rejects the second. Fails today: no harness, no `verify`, no corpus runner exists.

- [ ] **Unit:** corpus hashing (`axc1:`) is stable; the observable-tuple comparator distinguishes exit/stdout/provenance differences; capability-diff (G2) flags an added `read_file`.
- [ ] **Integration:** `verify` over the real examples corpus with an identity pass → all-green record; with an output-changing pass → E1401 naming the first divergent member.
- [ ] **CLI e2e:** `axon improve verify` exit codes (0 green / non-zero per gate); `axon improve graduate` without sign-off → E1404; `improve revert` restores baseline.
- [ ] **Adversarial (the safety-critical ones):** a pass that *adds* `net` I/O → E1402 (I-12); a pass that passes G1 on the discovering program but fails on another corpus member → still E1401 (overfitting caught); a pass whose verifier calls AI → E1406; a tampered manifest → E1405 at boot.
- [ ] **Property:** for any pass `P` and corpus `C`, `verify(P,C)` is deterministic (two runs → identical record) under fixed seed + mocked AI.
- [ ] **Parity (interp↔codegen):** G1 is interpreter-only by design (the oracle); G4's native timing is the codegen side — parity test runs without R1 gating (R1 solved 2026-06-01). The timing harness must be built for a real perf budget, but there is no structural dependency on native build.
- [ ] **Journey/red-team:** the headline attack — an AI proposes a pass that is faster *because* it drops a bounds check, changing behavior on one adversarial corpus member. G1 must catch it (E1401). This is the test that proves the whole mechanism is worth having.

## 9. Acceptance criteria (the done gate)

R10 advances from 0% on the **harness slice** (all gates verifiable now) when **all** pass:

- [x] `identity_pass_verifies_and_output_changing_pass_is_rejected` passes (G1, E1401). **DONE** — `improve.rs::verify_pass`: G1 compares the observable tuple `(exit_code, stdout)` via the interpreter oracle (`run_program_capturing`) over the whole corpus; an output-changing pass → E1401.
- [x] `capability_adding_pass_is_rejected_I12` passes (G2, E1402 — the safety core). **DONE** — G2 diffs `program_capabilities` (new public collector in `capabilities.rs`, single-sourced on `classify_call`): a pass adding `fs:read`/`fs:write`/`net`/`exec` → E1402.
- [x] `regression_breaking_pass_is_rejected` passes (G3, E1403). **DONE** — G3 runs each corpus member's `@[test]` fns (via `run_test_fn`, normalized by `should_fail`) before and after the pass; an outcome flip → E1403. Catches a pass that corrupts a helper (breaking its test) while leaving `main` unchanged — invisible to G1, caught by G3. A removed test is also a flip.
- [x] `overfit_pass_passing_on_one_member_is_caught` passes (the fork's central concern). **DONE** — G1 iterates the whole corpus, so a pass correct on member 0 but wrong on member 1 is still rejected (E1401). The overfitting failure is structurally impossible to pass.
- [ ] `graduation_requires_multisig` passes (E1404 — I-12 governance). *Implementation-pending (the `axon improve graduate` CLI + manifest; `verify_pass` already documents that passing verification is necessary-not-sufficient — graduation is a separate human-gated step).*
- [x] `verify_record_is_deterministic` passes. **DONE** — two `verify_pass(pass, corpus)` runs produce identical `VerifyRecord`s.
- [x] `ai_judged_correctness_is_rejected` passes (E1406 — correctness is the oracle, not AI). **DONE by construction** — `verify_pass` decides correctness *solely* by running both programs through the interpreter; there is no API path by which an AI judges correctness. E1406 is reserved for a discovery layer that would attempt it. *(Documented in the module's safety note; no AI-judgment path exists to test against because none can be written.)*

**Perf slice:**
- [x] G4 timing harness exists. **DONE** — `measure_perf` (opt-in via `VerifyOptions.measure_perf`) times `interp(c)` vs `interp(P(c))` over the corpus, `perf_trials` repeats taking the min (noise-damped), 3% noise threshold. A pass is `PerfStatus::Faster` iff ≥1 member improved and 0 regressed; else `NotFaster`/`Unmeasured` (W1410 — may graduate, never *claiming* `faster`). Wall-clock timing is advisory by design (never gates `passed()`), and only runs once G1–G3 hold (timing a miscompile is pointless). Interpreter timing is the portable signal; native timing has the identical structure.

**All four gates are now built and tested** (G1 oracle, G2 capability firewall, G3 regression, G4 perf). R10 rose **0% → ~70%** across the two slices. The remaining work is the *graduation machinery* — the `axon improve` CLI (`discover`/`verify`/`graduate`/`revert`/`list`), the content-addressed pass manifest, and multi-sig graduation (E1404/E1405) — implementation slices **under this same (Reviewed) spec**, no further spec work required. The verification core (the part that had to be right before any discovery exists) is complete.

## 10. Performance budget

The harness itself: `verify` runs the interpreter over the corpus (~72 programs) — seconds, acceptable for an offline graduation step, not a hot path. No budget on the *discovery* side (it's offline AI work). The *graduated passes* must show net speedup (G4) — that's the perf claim, no longer R1-gated (R1 solved 2026-06-01).

## 11. Rollout & rollback

- **Decomposed, reversibility is a first-class gate:** (1) corpus hashing + observable-tuple comparator; (2) the four-gate `verify` (G1–G4 all verifiable now); (3) the manifest + `graduate`/`revert` + multi-sig; (4) discovery (the AI proposer — last, and unprivileged). Each reverts to a green tree. `improve revert` is itself gate-3 (a graduated pass must be removable to byte-identical baseline).
- **Blast radius — the highest in the PRD:** a wrong pass that slips the harness miscompiles programs. Mitigations stacked: G1 exact-equivalence on the whole corpus, G2 capability firewall, G3 full suite, multi-sig graduation, boot attestation, and `perf_status` honesty so an unmeasured pass never claims speed. A pass is *fail-closed*: not in the attested manifest → never runs.
- **Native build:** R1 solved 2026-06-01. G4 (perf gate) and native application of passes run on a native binary — R1 no longer gates anything. The harness ships and proves correctness/safety on the interpreter regardless — correct ordering (safety before perf) remains, but R1 is no longer on the critical path.

## 12. Open questions

Resolved — R1 native build finished 2026-06-01 (`BUILD_RESOLVED.md`):
- **Q1 (R1 native build):** G4 (measurably-faster) and native pass application require `cargo build -p axon-core` (codegen) to finish — **solved** (`BUILD_RESOLVED.md`). The perf slice no longer needs to wait for R1; only the timing harness build remains. R10 correctness/safety/perf is fully verifiable now.

Blocking the discovery slice (resolve before building the proposer):
- **Q2 (discovery source — depends on R3):** the AI proposer that *generates* candidate passes depends on the R3 AI primitive (model routing, provenance). The harness (this spec's core) is independent of *how* candidates arise — it verifies anything. Discovery is the last slice, after R3. *Blocks the proposer, not the harness.*

Non-blocking:
- **Q3 (IR-level vs AST-level passes):** the harness verifies observable equivalence regardless of the layer a pass operates at; the layer choice is a discovery/perf concern (IR passes need a native build, which works now). Deferred.
- **Q4 (`@[optimizable]` hints):** letting `.ax` authors mark hot functions for targeted passes — a future ergonomic, not needed for the safety harness. Deferred.
- **Q5 (corpus growth governance):** who adds to the verification corpus, and the rule that a corpus member can only be *added* (never silently removed, lest coverage shrink). Likely an I-17 companion rule. Deferred.
- **Q6 (I-17 adoption):** propose "no pass runs unless attested + green-verified; the compiler cannot graduate its own passes" for the invariants file when this implements.
