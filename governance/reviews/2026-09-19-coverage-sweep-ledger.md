# Coverage sweep ledger — pinned baseline fccc4bc

Surfaces with NO test / example / harness that executes them (15, measured
against `BUILTINS` in `crates/axon-core/src/builtins.rs`, excluding mentions
in `builtins.rs` itself and in generated `AXON_REFERENCE.md`).

One domain per worktree. No two agents share a checkout.

| Agent | Branch | Assigned surfaces |
|---|---|---|
| numeric | `sweep/numeric` | `volatile_load_u16`, `volatile_load_u32`, `volatile_load_u64`, `bpf_get_smp_processor_id`, `bpf_ktime_get_ns` |
| kernel | `sweep/kernel` | `principal_spend`, `scheduler_done_count`, `supervisor_restarts` |
| json | `sweep/json` | `json_arr_f64`, `json_arr_from_f64`, `json_get_i64`, `json_path_json` |
| uncertain | `sweep/uncertain` | `uncertain_confidence`, `uncertain_deterministic`, `uncertain_dyn_f64` |
| audit | `sweep/audit` | none — audits the COVERAGE METRIC itself (see below) |

The audit agent exists because "named in a test file" is not "executed by a
test". The 15 above are the surfaces named NOWHERE executable; a builtin
named in a comment, a doc string, or a `BUILTINS`-adjacent table reads as
covered under the same query and may be exercised by nothing. That gap is
the same absent-vs-passed collapse this sweep is looking for, one level up.

## Outcome

All 15 assigned surfaces now have semantic coverage. Four of five agents
reported; the coverage-metric audit is still running.

| Surface | Verdict |
|---|---|
| `volatile_load_u16/u32/u64` | verified-sound — interp deliberately E0910-refuses (raw MMIO cannot be emulated), native lowers inline; no differential exists, so the native half checks width/endianness/zero-extension against itself at a real readable address |
| `bpf_get_smp_processor_id`, `bpf_ktime_get_ns` | verified-sound — the table's helper IDs were unit-tested, what the BACKEND emits was not; now asserted at the encoded-instruction level with a no-helper control |
| `principal_spend` | verified-sound — readout and both enforcers (`principal_authorize`, the LLM gateway) agree; that agreement was the named risk |
| `scheduler_done_count` | verified-sound — snapshot, not cumulative; `done + failed` accounts for every spawned fiber |
| `supervisor_restarts` | verified-sound — per-supervisor, counted on the production `supervisor_run` path, cross-checked against an independent counter |
| `json_arr_f64`, `json_arr_from_f64` | verified-sound — round trip is bit-exact over 8 values incl. -0.0, 1e300, 1e-300; non-finite emits `null`, never invalid JSON |
| `json_get_i64` | **DEFECT, fixed** — an integer too large for i64 was reported as "not an integer" |
| `json_path_json` | verified-sound — five distinct failure shapes; empty path is the empty KEY, leaf is re-serialised (both now pinned) |
| `uncertain_deterministic` | verified-sound — confidence 1.0, tag 0; does not survive combination (min-propagation), as documented |
| `uncertain_confidence` | verified-sound as code, doc corrected — it is a no-op, and claimed to record a confidence on a surrounding value that does not exist |
| `uncertain_dyn_f64` | **DEFECT, fixed** — interp/native divergence on `source_tag` (0 vs 2), an I-2 violation |

## Defects found, and what they have in common

Three, plus one the orchestrator took from an agent's adjacent-findings list:

1. `json_get_i64` — out-of-range integer mislabelled "not an integer"
2. `json_path_i64` — the SAME mechanism at a second site, collapsing three
   distinct failures into one message
3. `uncertain_dyn_f64.source_tag` — the interpreter reported a runtime-sourced
   value as user-constructed, while native reported it correctly. The fail-OPEN
   direction, on a provenance field, in the reference engine
4. `json_keys` doc claimed document order; the parser returns sorted

None is a crash. Every one is a WRONG STATEMENT ABOUT A RESULT — the same
class the last several sweeps have landed in, and the reason "it has no
coverage" was worth asking about at all.

Two were invisible to existing tests for a structural reason worth recording:
the `uncertain` bug is hidden by its own covered siblings (the non-`dyn`
constructors agree at 0 in both engines), and the `json_keys` claim is
untestable with alphabetical fixture keys, which is what the fixture had.

## Open, with an owner needed — NOT fixed

Found while probing, outside the finder's assigned domain, left alone
deliberately rather than folded into an unrelated commit:

* **A `@[bpf]` `if`-as-expression silently returns 0.** `fn f(ctx) -> i64 {
  if ctx > 0 { 7 } else { 3 } }` builds clean and emits `r0 = 0; exit` — not 7,
  not 3, no diagnostic. `codegen/bpf.rs` `lower_if` discards both branch values
  by design (a documented Slice-1 limitation) and the tail substitutes zero.
  A documented limitation that produces a silent wrong answer should be an
  E2301 refusal instead. This is the most serious open item here.
* **`ai_extract_uncertain_*` has the same `source_tag` class** as the fixed
  `uncertain_dyn_*`: codegen stamps 1, interp stamps 0. NOT demonstrable
  differentially — reading the tag out of `Result<Uncertain<i64>,_>` is
  E0910-refused natively — so it is asserted here as a code reading, not a
  measurement.

## Backend invariant established by this episode

> **No object file may be emitted from LLVM IR that has not passed LLVM module
> verification.**

Stated independently of the `if`-expression bug that exposed it. It is a
backend admission gate, not a regression workaround — the same shape as the
artifact admission gate: a transition (IR becomes a shipped artifact) that
nothing asserted was valid.

Measured state after the fix — every object-emitting entry point in
`codegen/output.rs` verifies first:

| entry point | verifies |
|---|---|
| `compile_to_binary` | yes, via `compile_to_binary_target` |
| `compile_to_binary_target` | yes |
| `compile_to_shared_lib` | yes |
| `compile_to_object_for_triple` | yes |
| `compile_to_freestanding_obj` | yes |
| `compile_to_freestanding_binary` | yes |
| `compile_to_wasm_object` | yes |
| `emit_bpf_object` | **yes — added here; it was the only hole** |

Still unverified, recorded not fixed: `write_ir`, `emit_bitcode`,
`emit_llvm_ir`. These emit IR/bitcode rather than objects. Bitcode is consumed
by other tools, so `emit_bitcode` is the one of the three that most deserves
the same gate.

## Reusable audit heuristic (the highest-yield thing this episode produced)

> **Search for documented "limitations" that COMPILE SUCCESSFULLY and
> substitute a default, zero, or null value. Prioritise those above explicit
> refusals.**

A refusal is visible: the build fails, the author learns, nothing ships. An
accepted program that returns a fabricated value is invisible by construction,
and the documentation makes it look considered. The BPF `if` had a spec
citation, a comment explaining the choice, and a clean build — and returned
neither branch.

The probe shape, in order of signal:

1. grep the backends for comments containing "Slice-1", "for now", "not yet",
   "yields no value", "unsupported" — near code that does NOT return an error
2. of those, keep the ones where the surrounding code still produces an
   artifact (an object, a value, an exit 0)
3. of those, keep the ones that substitute a literal — `const_zero()`,
   `unwrap_or_default()`, `Ok(None)` flowing into a default, `null`
4. write the smallest program that reaches it and check whether the answer is
   fabricated

Steps 1-3 are mechanical and could be a gate. Contrast with the inverse
question ("which features are unsupported?"), which surfaces the refusals —
the cases that are already safe.

### Second heuristic, from the mutation pass

> **A defensive branch that no input can reach is worse than no branch.** It
> tells every later reader — and every model trained on the file — that an edge
> case is handled, when the control flow makes it unreachable.

Found here as `then_open && else_open`: it looked like careful handling of a
terminating branch, and no program could produce that state, because
`Expr::Return` yields `None` and `Expr::Block` breaks on a terminator. A
mutation that deletes such a branch SURVIVES, which is the detection signal —
a survived mutation is a question about the code before it is a question about
the test.

Both are candidates for Cortex skill crystallisation: they are strategies about
where to look, not facts about Axon, so they should transfer to other compilers
and runtimes.
