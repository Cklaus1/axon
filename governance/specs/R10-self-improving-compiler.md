# Tech Spec — R10: Self-Improving Compiler

**Status:** 📝 Draft (2026-06-01) — *the verification harness is fully specifiable now; the perf-measurement half is R1-gated. Safety governance is specified regardless.*
**Requirement:** `../REQUIREMENTS.md` R10 — *Self-improving compiler: learned optimization passes graduated from AI-discovered asm.*
**Decisive fork (from `README.md`):** *Verification of a graduated pattern.* Before an AI-discovered optimization can be added as a compiler pass, how is it **proven** correct + faster on the full corpus, not just the discovering program? Decide the verification harness (equivalence checking + benchmark corpus + regression gate) **first** — an unverified self-modifying compiler is the single highest-risk component in the whole PRD. Hard dependency on R1 (native) + I-12. **→ Resolved below: the harness is fully specified; correctness is verifiable *now* via the interpreter oracle; perf-measurement is R1-gated and explicitly not claimed.**

---

## 1. Motivation

R10 is 0% (❌ Not started): no profiler, no pattern store, no graduation pipeline. It is also, per the spec README, *"the single highest-risk component in the whole PRD"* — a compiler that modifies itself can, if unverified, silently miscompile every downstream program or grant itself capabilities (I-12 violation). The fork is therefore correctly stated: **specify the verification harness before any discovery machinery exists**, so that an AI-discovered "optimization" cannot graduate into a pass without clearing a gate that is itself part of the TCB.

The key lever that makes this spec *possible now* — rather than fully blocked on R1 — is **I-2: the interpreter is the reference semantics.** A candidate optimization is a program→program (or IR→IR) transform. Its **correctness** is: *for every input in the corpus, the transformed program produces the same observable result as the original.* That equivalence is checkable by running both through the **interpreter** (`interp.rs`) — no native build required. Only the **"faster"** half of "correct + faster" needs R1 (you can't time a native binary that doesn't build). So this spec:

- **Fully specifies the correctness harness, corpus, regression gate, and graduation governance** (settle-able now).
- **Specifies but does not claim the perf-measurement gate** (R1-blocked, §12 Q1).
- Treats the graduation gate as a **TCB component** with I-12 as a hard, tested invariant.

Design-only; no code until **Reviewed** (Gate 1). This is the most conservative of the four specs by intent.

---

## 2. Requirement link

`../REQUIREMENTS.md` **R10** (0%, ❌ Not started). Quoted acceptance:

> *A discovered pattern verified correct + faster, added as a pass, applied automatically.*

This decomposes into four gates, only the first three of which are R1-independent:
1. **Correctness gate** — transformed ≡ original on the whole corpus (interpreter oracle). *R1-independent.*
2. **Safety gate** — the pass cannot weaken the TCB / grant capabilities (I-12). *R1-independent.*
3. **Regression gate** — adding the pass breaks no existing test; it is reversible. *R1-independent.*
4. **Perf gate** — the pass is measurably faster on native, no regression on any corpus member. *R1-GATED.*

Dependencies: **R1** (native build — gate 4 only), **I-2** (interpreter oracle — the lever for gates 1–3), **I-12** (self-mod can't weaken TCB — gate 2).

---

## 3. Surface (what the user / system invokes)

R10 is mostly **internal machinery**, not user-facing language surface. The surface is CLI + a pass manifest:

```
axon improve discover <corpus>      # NEW: run discovery, propose candidate passes (writes proposals/)
axon improve verify <proposal>      # NEW: run the FULL gate (correctness+safety+regression; perf if R1)
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
perf_status  = "unmeasured"               # unmeasured | faster | gated-on-R1
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
| **G4 Performance** | native-compiled `P(c)` is faster than `c` for ≥1 corpus member and slower for none, beyond a noise threshold | native binary timing | **YES (R1)** |

**Decision:** G1–G3 are the **gate that can run today**; G4 is recorded as `gated-on-R1` and a pass may graduate with `perf_status: unmeasured` **only** if explicitly flagged (a *correctness-preserving* refactor pass), never as a *performance* claim. A pass advertised as an optimization cannot graduate `faster` until G4 runs on a working native build. This is the honest split.

### 4.2 The corpus (fork resolution — "the full corpus, not just the discovering program")

**Decision:** the corpus is the **versioned, content-addressed set of all `examples/**/*.ax` (71 files today) plus a frozen regression set**, hashed as `axc1:…`. A pass is verified against a *named corpus hash*; its manifest entry pins that hash. Verifying against "just the discovering program" is structurally impossible — `verify` iterates the whole corpus or fails.

- **Why the examples corpus:** it is already the language-identity set (ROADMAP §2.7), already type-checks clean (`all_examples_typecheck_clean`), and spans the feature surface. Growth: every new feature adds a corpus member, so coverage compounds.
- **Rejected:** verifying on the discovering program only (the exact overfitting failure the fork names — rejected by construction); a synthetic random-program fuzzer *instead of* the corpus (kept as an *additive* G1 input, §12 Q2, not a replacement — random programs lack the human-meaningful semantics the corpus encodes).

### 4.3 What "correct" means precisely

`interp(P(c)) == interp(c)` compares the **observable tuple**: `(exit_code, stdout, ordered provenance events)`. Nondeterministic inputs are pinned: `AXON_SEED` fixes RNG (BUG_HUNT #11), `AXON_AI_MOCK=1` fixes AI calls (R3), so a corpus run is deterministic and the equivalence is exact, not statistical. A pass that changes observable output for any corpus member fails G1 — there is no "close enough."

### 4.4 The verification record

`verify` emits an immutable `axv1:…` record: `(pass_hash, corpus_hash, per-member G1 result, G2 capability diff, G3 suite result, G4 perf or "gated-on-R1", timestamp, tool versions)`. It is the audit artifact gate-3 of graduation checks. Stored append-only (R4 provenance discipline); the manifest references it by hash.

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
| Candidate claims `faster` but R1 unavailable | **W1410**; may graduate only as `perf_status: unmeasured`, never `faster`. |
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
| **W1410** | `faster` claimed but G4 (native perf) unavailable (R1 stalled) | `` perf gate skipped (native build unavailable, R1) — `{name}` may graduate only as `unmeasured`, not `faster` `` |

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
- [ ] **Parity (interp↔codegen):** G1 is interpreter-only by design (the oracle); G4's native timing is the codegen side and is **R1-gated** — the parity test is written but `#[ignore]`d until R1 lands.
- [ ] **Journey/red-team:** the headline attack — an AI proposes a pass that is faster *because* it drops a bounds check, changing behavior on one adversarial corpus member. G1 must catch it (E1401). This is the test that proves the whole mechanism is worth having.

## 9. Acceptance criteria (the done gate)

R10 advances from 0% on the **harness slice** (R1-independent) when **all** pass:

- [ ] `identity_pass_verifies_and_output_changing_pass_is_rejected` passes (G1, E1401).
- [ ] `capability_adding_pass_is_rejected_I12` passes (G2, E1402 — the safety core).
- [ ] `regression_breaking_pass_is_rejected` passes (G3, E1403).
- [ ] `overfit_pass_passing_on_one_member_is_caught` passes (the fork's central concern).
- [ ] `graduation_requires_multisig` passes (E1404 — I-12 governance).
- [ ] `verify_record_is_deterministic` passes.
- [ ] `ai_judged_correctness_is_rejected` passes (E1406 — correctness is the oracle, not AI).

**Perf slice (R1-GATED, NOT claimable now):**
- [ ] *Blocked.* "verified … faster" (the requirement's perf half) requires R1's native build for G4. Until then a pass can be proven *correct + safe + non-regressing* but only *measured faster* once R1 lands. Stated plainly.

R10 may rise 0% → ~30% on the harness slice (correctness/safety/regression verifiable, perf gated). The full "verified correct + faster, applied automatically" acceptance needs R1.

## 10. Performance budget

The harness itself: `verify` runs the interpreter over the corpus (71 programs) — seconds, acceptable for an offline graduation step, not a hot path. No budget on the *discovery* side (it's offline AI work). The *graduated passes* must show net speedup (G4) — that's the perf claim, R1-gated.

## 11. Rollout & rollback

- **Decomposed, reversibility is a first-class gate:** (1) corpus hashing + observable-tuple comparator; (2) the four-gate `verify` (G1–G3 now, G4 stub); (3) the manifest + `graduate`/`revert` + multi-sig; (4) discovery (the AI proposer — last, and unprivileged). Each reverts to a green tree. `improve revert` is itself gate-3 (a graduated pass must be removable to byte-identical baseline).
- **Blast radius — the highest in the PRD:** a wrong pass that slips the harness miscompiles programs. Mitigations stacked: G1 exact-equivalence on the whole corpus, G2 capability firewall, G3 full suite, multi-sig graduation, boot attestation, and `perf_status` honesty so an unmeasured pass never claims speed. A pass is *fail-closed*: not in the attested manifest → never runs.
- **R1 dependency:** the perf gate (G4) and any *native* application of passes are gated. The harness ships and proves correctness/safety on the interpreter regardless — so R10's safety machinery exists *before* R1 unblocks the perf flywheel, which is the correct ordering (build the firewall before the fire).

## 12. Open questions

Blocking the perf slice (out of our hands now):
- **Q1 (R1 native build):** G4 (measurably-faster) and native pass application require `cargo build -p axon-core` (codegen) to finish — `BUILD_DIAGNOSIS.md` / `CODEGEN_WRAPPER_PROTOTYPE.md` (prototyped, unvalidated). Until R1 lands, R10 = correctness/safety harness only, no perf claims. **The documented gate.**

Blocking the discovery slice (resolve before building the proposer):
- **Q2 (discovery source — depends on R3):** the AI proposer that *generates* candidate passes depends on the R3 AI primitive (model routing, provenance). The harness (this spec's core) is independent of *how* candidates arise — it verifies anything. Discovery is the last slice, after R3. *Blocks the proposer, not the harness.*

Non-blocking:
- **Q3 (IR-level vs AST-level passes):** the harness verifies observable equivalence regardless of the layer a pass operates at; the layer choice is a discovery/perf concern (IR passes need R1 anyway). Deferred.
- **Q4 (`@[optimizable]` hints):** letting `.ax` authors mark hot functions for targeted passes — a future ergonomic, not needed for the safety harness. Deferred.
- **Q5 (corpus growth governance):** who adds to the verification corpus, and the rule that a corpus member can only be *added* (never silently removed, lest coverage shrink). Likely an I-17 companion rule. Deferred.
- **Q6 (I-17 adoption):** propose "no pass runs unless attested + green-verified; the compiler cannot graduate its own passes" for the invariants file when this implements.
