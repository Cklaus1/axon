# R51 — One interpreter for v0.22's sealed runs and R50's bytecode engine

**Spec ID:** `R51-sealed-engine-unification`
**Status:** Draft (2026-10-10). Not reviewed.
**Risk class:** Structural and security-critical (re-homes the PSV-1 runtime seal onto a rewritten data model; adds a refusal on the default engine).
**Author / date:** 2026-10-10, after merging `origin/main` (5864c423) into the v0.22 head (`c9r16/integrate15`, c9ed2ddd) was found to be a re-implementation, not a merge.

```spec-meta
id: R51-sealed-engine-unification
status-claim: Draft
depends-on: R50-register-vm
blocks: none
blocked-by: none
supersedes: none
related: R50-register-vm, v022-protected-suite-verdict, v022-psv-protocol, v022-ADR-003-protected-certification-deferral, post-c9-hardening
conflicts-with: none
reserves: none planned (a sealed-run engine refusal reuses an existing exit code; §6)
evidence: none
```

R47–R49 are reserved by `R46-in-flight-operations.md` §2, so this spec takes R51.

---

### 1. Motivation

Two lines of work diverged at `abe2ca36`:

- **main** made `axon run` fast. AX-40/46/47/53/54/55 rewrote the interpreter's data model
  (`Value::Struct(Rc<StructVal>)` holding `Fields(Vec<(Sym, Value)>)`, `Enum(Rc<EnumVal>)`,
  `Closure(Rc<ClosureVal>)`, `Handle(Rc<HandleVal>)`, Rc'd `Tuple`/`Decimal`, `SizedInt` carrying `IntWidth`;
  `Env { vars: Vec<(Sym, Value)> }` with slot-resolved access), and R50 added a bytecode engine that is the
  default (`Engine::DEFAULT = Engine::Vm`).
- **v0.22** (branch `v022/veto`) made a candidate's verdict trustworthy. The PSV protocol runs candidate code
  against a hidden check; the PSV-1 runtime seal (taint carried by the evaluator, seal edges where sealed code
  reaches operator code, the PICK bit, the sealed-only static check) is written against the OLD model:
  `Env.vars: Vec<(String, Value, u8)>` with a per-binding taint byte, `HashMap` struct fields, unboxed enums
  and closures, and edges on the tree path only (`seal_call`, `seal_dispatch`, `seal_width`, `seal_global`,
  `seal_refine`, the return casts, `SEALED_*_MARK`).

Git interleaves the two APIs hunk by hunk; no selection of hunks compiles and keeps both. Keeping two
interpreters is not an option either: every feature, fix and builtin would be written twice, v0.22 would keep
falling behind (137 commits at the time of writing), and main would never carry the protections.

**The hazard.** R50's compiled bodies (`run_body_vm`, `exec`) and fast calls (`Op::CallFast`,
`CallFastLocalInt` → `call_fast` / `call_fast_arg`) skip `call_fn_in`'s dispatch, where every seal edge lives.
A naive merge would run sealed candidate code on the default engine with no seal edges and no taint: PSV-1
silently off, every test green.

### 2. Requirement link

Closes the "not merged with origin/main" note on `engineering_qualified` in `governance/v022_release_claims.json`
(v0.22 branch). Acceptance anchors: the engine-pin test (§8 T1), `scripts/vm_parity.sh` and
`scripts/vm_perf_gate.sh` unchanged (§10), the v0.22 PSV-1 suites green on the unified tree (§8 T3-T6).

### 3. Surface

No language change. One behaviour change on an existing flag:

| Situation | Today on main | After R51 |
|---|---|---|
| `axon run` / `goal` / `test` without `--seal` | VM by default | unchanged |
| a sealed run (`axon test --seal`, the axon-psv runner, the sealed check child) | n/a on main (no seal) | tree-walker with taint, always |
| a sealed run with `AXON_ENGINE=vm` | n/a | **refused** with a stated message; never silently falls back, never runs on the VM |

```text
$ AXON_ENGINE=vm axon test --seal cand suite.ax
error: a sealed run executes on the tree engine only (AXON_ENGINE=vm cannot carry the seal); unset AXON_ENGINE
```

### 4. Semantics

#### Fork 1 — which engine runs sealed code

- (a) **Tree only, pinned. Chosen for this spec.** The seal machinery already exists for the tree path, has
  ~2290 active mutation rows and fifteen review rounds behind it. Re-homing it onto the new data model is the
  whole job; also re-homing it into a second engine doubles the security-critical surface.
- (b) Taint inside the VM. **Deferred** to a later spec (§12 Q1): only worth it if sealed runs prove too slow.
  Sealed runs are suite verdicts, not hot loops; R50's speed matters for `goal`/`@[adaptive]` re-runs, which
  are unsealed.

The pin is decided ONCE, before `Interp::build` chooses the engine: the seal state set in `main.rs` (and by the
axon-psv runner) is passed into the build, and `engine_at_build()` returns `Engine::Tree` when sealed, or refuses
when `AXON_ENGINE=vm` was requested. A second, independent check in `run_body` refuses a compiled body while
`seal.active` (defence in depth; one drift test proves every VM entry point — `run_body_vm`, `exec`,
`call_fast`, `call_fast_arg`, deferred compile in S9 — passes through it).

#### Fork 2 — where per-binding taint lives

- (a) Widen `Env.vars` to `(Sym, Value, u8)`. Rejected: changes main's hot-path layout for every run, sealed
  or not, and R50's perf gate measures exactly that path.
- (b) **A parallel taint vector `Option<Vec<u8>>` indexed by the same slot, allocated only in a sealed run.
  Chosen.** Unsealed runs carry `None` and pay one branch at most where taint is touched (and taint is touched
  only on the tree path, which unsealed runs mostly do not take). `define_var`/`assign_var` gain taint-aware
  twins used by the seal code; the name-check scan fallback keeps both vectors in step.

#### Fork 3 — taint and identity on Rc'd values

v0.22 keeps taint for shared objects (dicts, channels) in address-keyed tables, and copies values across seal
edges. Under main's model structs, enums, closures, tuples and handles are `Rc`-shared too, so:

- every value that crosses a seal edge is **deep-copied** (`Rc::make_mut` / explicit clone of the inner value),
  never shared by pointer, so a candidate cannot hold an alias to an operator value or vice versa;
- address-keyed taint tables extend to the `Rc` payloads that can be mutated in place (`StructVal` fields via
  `&mut` write-through, closure capture cells), with the existing reuse-after-free guard;
- the PICK bit attaches to the value as before; its edges (index chosen by the candidate, builtin selector,
  branch yield, place write, exit-decided result, sealed hand-back) are re-derived on the new constructors.

#### Fork 4 — what happens to v0.22's evidence

Code-anchored evidence does not survive a port. Rows anchored in `crates/axon-core/src/interp*` (~263) are
re-anchored and re-run; their kills count only on the re-run. EQUIVALENT retirements in `interp*` are RESET and
re-argued on the new code (amendments 53-122's arguments describe code that no longer exists). Evidence anchored
outside the interpreter (Fabric, axon-psv, custodian, observer, `ns_run`, guest build, governance scripts) carries
over unchanged.

### 5. Type rules

None.

### 6. Error codes

No new code. The sealed-run engine refusal exits with the existing usage/refusal code the sealed runner already
uses for a malformed sealed invocation (pinned by T1); it never exits 0.

### 7. Invariants touched

- **I-2 (tree-walker is the reference):** unchanged and strengthened — sealed runs are reference-engine runs.
- **PSV-1 runtime seal:** must hold on the unified tree exactly as narrowed on v0.22 (claim and NON-CLAIMS in
  `v022-protected-suite-verdict.md`); no new non-claims may be introduced by the port without a stated reason.
- **R50 parity and perf:** unsealed behaviour and cost unchanged.

### 8. Test plan

- **T1 engine pin:** a sealed run with `AXON_ENGINE=vm` refuses (message + exit code); a sealed run with the
  variable unset runs on the tree engine (asserted via `AXON_VM_TRACE=1` printing nothing); a drift test fails if
  any VM entry point does not consult the seal state.
- **T2 no regression:** `vm_parity.sh`, `vm_wasm_depth.sh`, `vm_perf_gate.sh` and main's cli_run suite pass
  unchanged.
- **T3 taint unit suite:** v0.22's `taint_tests.rs` ported; every case that was refused is refused, every honest
  control passes.
- **T4 oracle twin:** the 78-position x 8-placement existence-oracle twin (1248 runs) passes.
- **T5 receiver PICK:** the 11 honest controls pass; the `dict_get_or` / sealed generic `pick<T>` / sort-order /
  index/branch/match/assign/exit attacks are refused.
- **T6 runner legs:** axon-psv `sealed_frames` and `runner`, axon-fabric `psv_dispatch`, `check_effects`,
  `attestation`, `cortex_via_fabric`, `observer_service` green.
- **T7 mutation evidence:** all re-anchored `interp*` rows KILLED by their own attack, 0 REFUSED_ELSEWHERE,
  0 stale; refusal-coverage gate plain and `--freeze` exit 0; `pci_delta.py --check` PASS.
- **T8 Rc aliasing:** new attacks — a candidate keeps an alias to an operator struct/closure across a seal edge
  and mutates it, or the reverse — refused; honest code that shares its own values is unaffected.

### 9. Acceptance criteria

All of §8 green at one commit; a strict gate (`gate.sh --strict` under `run_managed --snapshot`) CITABLE at that
commit; one confirmation review round on PSV-1, PSV-2 and SENTINEL against the ported code with zero blockers.

### 10. Performance budget

Unsealed: no measurable change on `vm_perf_gate.sh`'s five programs (within the gate's existing tolerance).
Sealed: tree-engine speed (today's v0.22 cost); no target.

### 11. Rollout & rollback

Slices, each a commit that builds and passes its tests:

- **S0** merge everything outside the interpreter: Fabric, axon-psv, custodian, observer, launcher, `ns_run`,
  guest build, governance scripts and specs, registries (renumbered). The 54 non-interpreter conflicts already
  analysed (LAND15b, 2026-10-10) are the starting point.
- **S1** engine pin (Fork 1) + T1, landed on main BEFORE any seal code, so main can never run sealed code on the VM.
- **S2** taint channel on `Env` (Fork 2) + accumulator/pc/sticky taints; T3 subset.
- **S3** seal edges, casts, marks, dict snapshots on the Rc'd model (Fork 3); T3, T8.
- **S4** PICK bit and receiver dispatch rule; T5.
- **S5** static layer: `pin.rs`, sealed-only check, `Parser::eof_span`, per-item spans, resolver sealed set; T4.
- **S6** evidence: re-anchor and re-run the `interp*` rows, reset `interp*` equivalences; T6, T7; strict gate.
- **S7** confirmation review; fix to zero blockers.

Rollback: before S7, main keeps working with the seal absent (the pin refuses sealed runs it cannot protect).
`v022/veto` remains the reference for v0.22's protected behaviour until S7 passes.

### 12. Open questions

- **Q1** Do sealed runs need VM speed? Measure suite-verdict wall time at S6; if a realistic suite is too slow,
  write a spec for taint inside the VM.
- **Q2** Can the parallel taint vector be dropped entirely on the VM path (no allocation, no branch) by
  construction? S2 should measure.
- **Q3** Does deferred compile (R50 S9) or `AXON_VM_EAGER` change anything observable in a sealed run if a
  future change routes sealed code through compile? T1's drift test must cover S9's entry.

### 13. Dependency DAG

R50 (landed) → R51 S0 → S1 → S2 → S3 → {S4, S5} → S6 → S7.

### 14. Evidence ledger

Empty (Draft).
