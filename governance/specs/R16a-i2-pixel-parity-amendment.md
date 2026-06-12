# Invariant-Change Proposal — I-2 amended for non-textual (pixel) output

**Spec ID:** `R16a-i2-pixel-parity-amendment` (invariant-change proposal for `ARCHITECTURE_INVARIANTS.md` I-2; required by `R16-axon-ui.md` §7 / §12 Q5)
**Status:** Reviewed — **ratified as a standing decision; APPLICATION deferred to the Slice-2 commit** (per the process step 3 below)
**Risk class:** Structural (touches a load-bearing invariant)
**Author / date:** cklaus, 2026-06-12

> This document follows `ARCHITECTURE_INVARIANTS.md` §"How to change an invariant":
> (1) a proposal stating the invariant, why it must change, and the blast radius; (2) an enumeration of every
> test/spec/example that depends on it; (3) the invariant edit itself **lands in the same commit as the code
> change** (Slice 2 of R16) — *not* in this proposal; (4) never change the invariant implicitly. This doc is
> steps (1)+(2) and the exact text for (3). It is the ratification record the Slice-2 commit links back to.

---

### 1. Motivation

R16 (Axon UI) renders to **pixels on a GPU**, which `I-2` as written cannot govern. I-2 says *"the interpreter
defines correct behavior and codegen is the bug … every dual-path feature carries a parity test"* — and the
parity mechanism throughout Axon is **byte-equality of stdout + exit code** (42 `scripts/*_parity.sh`, ~285
parity assertions in `cli_run.rs`). A rendered frame has no stdout to byte-compare; the interpreter cannot
rasterize at all. Applying I-2 literally to UI would either (a) block UI forever, or (b) be quietly ignored —
the exact "silent rot" step 4 of the process forbids. The amendment scopes I-2 honestly: **byte-parity still
governs all textual/scalar output (unchanged); non-textual output gets a substitute oracle.** I-2 already
anticipated this with its built-in clause *"(until a spec says otherwise)"* — this proposal is that spec,
made explicit in the invariant text.

### 2. Requirement link

`REQUIREMENTS.md` **R16** (Axon UI). `R16-axon-ui.md` §7 lists I-2 as the one invariant this requirement
*changes*, and §12 Q5 marks the amendment a **hard gate**: *"the I-2 amendment for pixel output must be
ratified through the `ARCHITECTURE_INVARIANTS.md` change process before any rasterizing slice merges."* This
doc discharges that gate.

### 3. Surface — the exact text change

**I-2 today (`ARCHITECTURE_INVARIANTS.md:17-19`):**

> **I-2 — The interpreter is the reference semantics.** Where interpreter and codegen disagree, the
> *interpreter* defines correct behavior and codegen is the bug (until a spec says otherwise). Every
> dual-path feature carries a parity test (`TESTING_STANDARD.md` seam rule).

**I-2 amended (the text to land in the Slice-2 commit — additive final sentence, nothing removed):**

> **I-2 — The interpreter is the reference semantics.** Where interpreter and codegen disagree, the
> *interpreter* defines correct behavior and codegen is the bug (until a spec says otherwise). Every
> dual-path feature carries a parity test (`TESTING_STANDARD.md` seam rule). **For non-textual output
> (rendered pixels / GPU frames — R16 Axon UI), byte-equal stdout parity is undefined: the interpreter
> remains authoritative for the `View`/`Scene3D` tree and its computed layout box-model (byte-comparable; a
> dual-path parity test on the box-model is still required), while final rasterization is validated by
> golden-image snapshot at the wgpu layer within an SSIM tolerance — not by interp↔codegen byte-equality.
> Codegen is interp-only for UI until a raster-parity story exists (E0910 refuses `axon build` of `@[ui]`).**

**Why this is a *scoping refinement*, not a weakening:** the amendment **relocates** parity rather than
abandoning it. The interpreter stays the reference oracle for everything byte-comparable — including the
*layout box-model*, which is interpreter-computed and dual-path-parity-tested exactly like every existing
feature. Only the final raster step (which has no textual representation to compare) moves to golden-image.
No existing guarantee is loosened.

### 4. Semantics (what changes, what does not)

| Output class | Oracle before | Oracle after | Change? |
|---|---|---|---|
| stdout text / `println` / interpolation | interp byte-equality | interp byte-equality | none |
| scalar return / exit code | interp byte-equality | interp byte-equality | none |
| panic / error messages | interp byte-equality | interp byte-equality | none |
| **`View`/`Scene3D` tree + layout box-model** | n/a (didn't exist) | **interp-authoritative, byte-comparable, parity-tested** | new (consistent with I-2 spirit) |
| **rendered pixels / GPU frame** | n/a (didn't exist) | **golden-image snapshot at wgpu, SSIM tolerance** | new (the actual amendment) |
| codegen of `@[ui]` | n/a | **refused, E0910 (interp-only)** — so interp↔codegen pixel divergence cannot even arise in v1 | new (sound-by-refusal) |

### 5. Blast radius — every dependent enumerated (process step 2)

**Direct I-2 enforcement (the parity harness):** 42 scripts —
`scripts/parity_all.sh` (the aggregator) plus `all_examples_parity.sh`, `exit_code_parity.sh`,
`checked_arith_parity.sh`, `str_utf8_parity.sh`, `to_str_parity.sh`, `arr_reduce_parity.sh`, `dict_parity.sh`,
`exec_parity.sh`, `provenance_parity.sh`, `agent_action_parity.sh`, `goal_input_parity.sh`,
`handler_resume_parity.sh`, `smt_discharge_parity.sh`, the `parse_*`/`random_i64`/`recursion_guard`/`assert_msg`/
`arr_panic_msg`/`i64_radix_panic`/`float_to_int`/`bitwise_cast`/`str_count` family, and the 12 `wasm_*_parity.sh`
(wasip1 + browser + AOT-stdout). **~285** parity assertions in `crates/axon-core/tests/cli_run.rs`
(+ `integration_fixtures.rs`).

**Finding (the load-bearing one): all 42 scripts + ~285 assertions compare stdout / exit-code / scalar
output. None render pixels (no rendering path exists). The amendment is additive — it introduces a *new*
output class and leaves every existing oracle byte-for-byte unchanged. Net change to existing tests: ZERO.**
(Reproduce: `grep -rl 'I-2' scripts/ crates/*/tests/` and inspect — every hit is a text/exit comparison.)

**Specs that cite I-2** (informational references; none are normatively changed): `R1`, `R1b–R1f`, `R3`,
`R3b/c`, `R4`, `R5`, `R6`, `R7`, `R7b`, `R7c`, `R9`, `R9b`, `R10`, `R12`, `R12b`, `R13`, `R14`, `R15`, `R16`,
`spec/compiler-phase5.md`, `spec/compiler-phase6.md`, `spec/worldmodel-loop.md`, `MILESTONE.md`,
`REQUIREMENTS.md`, `VISION.md`, `CHANGELOG.md`. Each uses I-2 as "interpreter == codegen on textual output,"
which the amendment preserves verbatim. **No spec edit is required by this amendment** beyond R16 (which
already carries it) and the I-2 line itself (at Slice 2).

**New tests the amendment *requires* (land with Slice 0/2, per R16 §8-§9):** `axon_ui_layout_boxmodel_is_deterministic`
(the box-model parity oracle), `axon_ui_renders_static_view_to_golden_png` (the golden-image oracle).

### 6. Type rules

N/A — this is an invariant/process change, not a type-system change.

### 7. Error codes

No new codes. Relies on existing **E0910** (codegen-unsupported → `axon build` of `@[ui]` is refused), which
is what guarantees interp↔codegen pixel divergence cannot arise while the amendment is in force.

### 8. Invariants touched

- **I-2** — amended (this proposal). Scoped, not weakened (§3).
- **I-15** (spec↔behavior no drift) — *preserved and exercised*: this proposal + the Slice-2 edit are the
  mechanism keeping the invariant text and the shipping UI behavior in sync.
- All other invariants — untouched.

### 9. Acceptance criteria (the ratification gate)

- [x] Proposal written stating I-2, the reason, and the exact replacement text (§1, §3).
- [x] Blast radius enumerated; existing-test impact shown to be zero (§5).
- [x] Standing decision recorded (this doc, Status line) — root-principal approval is the ratification act.
- [ ] **(deferred, Slice-2 commit)** The I-2 line in `ARCHITECTURE_INVARIANTS.md` is edited to the §3 text,
      *in the same commit* that lands Slice-2 rendering code, with this doc linked in the commit body
      (process step 3). Until that commit, `ARCHITECTURE_INVARIANTS.md` is intentionally **unmodified**.

### 10. Performance budget

N/A (governance change). The golden-image oracle's runtime cost is budgeted in `R16-axon-ui.md` §10.

### 11. Rollout & rollback

This proposal is a **doc-only commit** — zero code, zero behavior change, trivially revertible. The invariant
edit it authorizes ships later, gated to Slice 2, and is itself revertible with that slice. There is no point
at which `ARCHITECTURE_INVARIANTS.md` changes without rendering code in the same commit (process step 4).

### 12. Open questions

1. **SSIM tolerance value** — the golden-image acceptance needs a concrete threshold (e.g. ≥ 0.99) and a
   policy for cross-GPU/driver raster differences (reference images may differ per backend). *Resolve when
   Slice 0's headless-wgpu golden harness is built; record the chosen threshold in `R16-axon-ui.md` §10.*
2. **Per-backend golden images** — Vulkan vs Metal vs WebGPU may rasterize sub-pixel-differently; decide
   one-reference-with-loose-SSIM vs per-backend references. *Default: one reference, SSIM tolerance absorbs
   backend jitter; escalate to per-backend only if jitter exceeds tolerance.*
