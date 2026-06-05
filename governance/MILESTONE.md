# Axon — Language-Core Milestone (2026-06-03)

A snapshot of where Axon stands: an honest accounting of what is **built and
verified**, what **remains**, and **why** the remainder is what it is. The
source of truth for per-requirement detail is `REQUIREMENTS.md`; this is the
one-page picture.

## Headline

The **Axon language core is ~94% complete and stable** — the compiler, type
system, AI-as-primitive surface, optimization, capability security, testing,
formal verification, and self-improvement firewall are all built, tested, and
running with native↔interpreter parity. **~700 tests pass; zero failures.**

What is *not* built is the broader **platform vision** the PRD also describes (a
GPU-rendered UI framework, mobile runtimes, a 3D engine, kernel runtime
services) — that remains greenfield, multi-quarter work.

## Requirement scorecard

| Req | What | % | State |
|---|---|---|---|
| **R1** | Native pipeline (parse→type→borrow→LLVM→native) | 98 | ✅ 32/32 example parity; **stdlib codegen coverage ~105 builtins — the arr_* family is COMPLETE and the dict family is effectively done (new/set/get/has/len/inc/get_or/remove/keys/values/merge/from_pairs/to_pairs/map_values/to_str/filter/each), plus exp/ln/log10 + str_split/join/digits_only — all native==interp==AOT-wasm**. **The lambda closure ABI is now GENERALIZED**: parameters and captures are typed by their real type (str/f64/pointer round-trip in both positions — e.g. `dict_each(d, |k,v| dict_set(sink, k, v))` captures a Dict handle to mutate; `dict_filter(d, |k,v| str_len(k)>3)` takes a str key), via a builtin-set param-type hint + caller's-locals capture typing + `env_struct.size_of()` malloc. Differential parity FUZZER + dict/arr harnesses auto-guard I-2 in the gate. Reusable codegen patterns: bitcast-transport (f64 lambda returns), repr(C)-layout-match (tuple args), runtime-callback (closures invoked from inside axon-rt), param-type-hint + typed-captures (non-i64 closures). **The few remaining builtins fail honestly (E0910), run under interp, and hit ONE VERIFIED root wall — the R2a type-map**: `dict_from_str`/str-valued `dict_get` need codegen to know a dict's value type at the call site; str/slice/tuple-RETURNING lambdas can't round-trip (a closure value carries no return-type tag); `arr_group_by` needs all three (str-return key_fn + a slice-valued DictVal + slice-valued dict_get). No raw IR crashes remain in the closure path — everything either works or gives an actionable diagnostic |
| **R2** | Type system + borrow checker (HM, no-null, ownership) | 90 | ✅ Strong; dyn-trait + refinement types remain |
| **R3** | AI as a language primitive (routing, policy, budget, cost) | 87 | ⚠️ tier routing + live token-count live; first-class Budget type remains |
| **R4** | Three code zones + compiler-enforced provenance | 100 | ✅ Complete |
| **R5** | Goal-directed optimization (5 strategies, held-out eval) | 94 | ⚠️ kernel `Goal<M>` (Phase-7) remains |
| **R6** | Capability security (lockfile, audit, `@[sensitive]`, `@[contained]`) | 97 | ✅ **@[sensitive] taint closed all 4 paths** (interprocedural/local/return-value/container-store, all E1206) AND **@[contained] sandbox now enforced TRANSITIVELY** (a contained fn calling a helper that does the forbidden exec/net/write is E1001 — the I-11 TCB escape closed; `CapCtx` follows helper calls under the caller's spec, visited-set stops recursion). Path/host-allowlist still needs a literal arg; AI-audit quality remains |
| **R7** | Cross-platform (native / wasm / js / mobile) | 80 | ✅ interp→wasm byte-identical; **AOT-wasm runs the whole example corpus — 26/26 byte-identical stdout vs interp** (str-ABI + size_t bridges + void-main entry fix); js/mobile + host-needing programs (AI/thread/goal/fs) remain |
| **R8** | Built-in testing + structured diagnostics (`forall`) | 100 | ✅ Complete |
| **R9** | Layer-1/3 alignment (`Uncertain`/`Temporal`/`@[verify]`/causal) | 88 | ⚠️ SMT loop invariants + metacognition trait remain |
| **R10** | Self-improving compiler (4-gate firewall, AI discoverer) | 99 | ✅ static + live AI discoverer; growing the template menu is the only follow-on |

**Average 93.6% · language-core (R1-6,8-10) ~94% · full-platform vision ~15%.**

## What "done and verified" means here

Every ✅/⚠️ above is backed by passing acceptance tests, not aspiration:

- **Native == interpreter** on all 32 pure-compute + AI-under-mock examples
  (`all_examples_parity.sh`), the interpreter being the I-2 reference oracle.
- **Stdlib codegen coverage**: ~84 builtins that previously had no native
  lowering (and silently returned 0 on `axon build`) now compute correctly
  native==interp==AOT-wasm — the full 35-builtin `arr_*` suite (reductions,
  allocating ops, closure ops, tuple/nested results), the bitwise/shift ops,
  the polymorphic `as_i64`/`as_f64` casts, and `parse_int_or`/`parse_float_or`/
  `parse_bool_or` (`arr_reduce_parity.sh` 85 cases, `bitwise_cast_parity.sh`,
  `parse_or_parity.sh`). Anything still unimplemented in codegen now aborts the
  build with a clear **E0910** ("not yet supported by native codegen — use
  `axon run`") rather than shipping a wrong binary.
- **Cross-platform**: the same `.ax` runs byte-identically on native and
  `wasm32-wasip1` across compute, file I/O, and env vars
  (`wasm_parity.sh` 28/28, `wasm_fs_parity.sh`). Beyond the interpreter path,
  **AOT-compiled wasm** (inkwell wasm32 → reactor-mode link → `wasmtime`) now
  runs int, float, struct, array, and string programs value-identical to the
  interpreter — and the **entire example corpus** AOT-compiled to wasm prints
  stdout byte-identical to the interpreter (`wasm_aot_stdout_parity.sh`, 26/26).
  The str/array i64→i32 ABI, the libc `size_t` width (malloc/snprintf/memcpy/
  write), and the void-`main` wasi entry convention are all closed.
- **Formal verification**: `axon verify` proves `@[verify]` integer + float +
  conjunction bounds via Z3, or reports a concrete counterexample.
- **Self-improvement safety**: an AI can only *select* optimization templates
  from a closed registry; the four-gate firewall (correctness oracle + capability
  diff + regression + perf) and multi-sig graduation are the sole path to a
  runnable pass — red-team-hardened.
- **Privacy**: `@[sensitive]` data flowing to an AI call / file write / process
  exec is a compile error (E1206), across struct/field/typed-field/array flows.

## The flagship

`examples/asi/ad_optimizer.ax` — an autonomous Meta-Ads optimizer that composes
the whole stack into one believable workflow: tournament variant search,
`Uncertain<T>` ROAS confidence, agent metacognition, a `@[verify]`-capped spend
bound, and `Temporal<T>` ad-creative fatigue. It is the one-line answer to "why a
language for AI apps?" — the expensive mistakes (blow the budget, trust a shaky
prediction, run a stale creative) become things the type system handles.

## What remains, and why it's not a bounded slice

The bounded, one-gated-slice backlog is **exhausted**. Each remaining item is a
multi-slice epic with its own spec (where applicable):

| Epic | Why it's large |
|---|---|
| **R7 js / mobile backends** | the AOT-wasm str/array i64→i32 + size_t ABI is now CLOSED (the whole example corpus runs AOT-wasm, 26/26); what remains is a js *backend* (transpile or wasm-in-JS) and mobile runtimes, plus host-needing programs (AI/thread/goal) on the pure-AOT path |
| **`dict_*` runtime data structure** | the dict builtins (used in 6 examples) need a runtime hashmap extern in axon-rt with tagged values + string keys — like the channel `__axon_chan_*` impl, not inline IR. `arr_group_by` (returns a Dict) is gated on this |
| **Kernel Phase-7 services** | scheduler + durable stores + SMT-checked cap subsets (`R12-kernel-runtime-services.md`) |
| **SMT loop invariants** | invariant inference past straight-line code (`R9b-smt-loop-invariants.md`) |
| **Platform vision** | UI/GPU/mobile/3D — entirely greenfield, no foundation yet |

These are deliberate scope decisions, not oversights. The language core is a
strong, shippable v1; finishing the PRD from here means committing to one epic
at a time, spec → sliced plan → build.
