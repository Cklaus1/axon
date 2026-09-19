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
