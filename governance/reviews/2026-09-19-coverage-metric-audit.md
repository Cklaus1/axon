# Coverage-metric audit — "named in a test file" is not "executed by a test"

Date: 2026-09-19 · Worktree branch: `sweep/audit` · Tree audited: `fccc4bc`

## What this audits

An earlier sweep looked for uncovered builtins by asking:

> is this builtin's name present in any file under `crates/`, `examples/`,
> `scripts/`, `tests/` that is a test file, a `.ax` example, or a harness script?

That query passes a builtin whose name appears in a comment, in a Rust
`format!` string, in a `.ax` file nothing runs, or in a function that is defined
and never called. This audit measures how big that gap is. It does not fix it.

## 0. The earlier sweep, reproduced

`BUILTINS` in `crates/axon-core/src/builtins.rs` holds **343** entries.
Re-running the mention query over the same corpus reproduces the sweep exactly:

* **324 / 343** "covered" (name mentioned somewhere in the corpus)
* **19** not mentioned — which is the 15 builtins the sweep handed to the four
  implementation agents, plus `E`, `Var`, `P`, `Chan::new` (pseudo-builtins that
  are never written as a plain `name(` call).

Tightening the query from "mentioned" to "mentioned in call shape `name(`
outside a comment" removes only **3** names (`eprint`, `http_post`,
`port_in_u8`). Static call-shape analysis is therefore worth almost nothing
here: 321 of 343 names appear call-shaped somewhere.

## 1. Method: measure execution, don't argue about it

`Interp::call_builtin` (`crates/axon-core/src/interp/builtins.rs:454`) is the
**single** dispatch point for every builtin in the tree-walking interpreter —
`crates/axon-core/src/interp/eval.rs:1011` is its only caller. I instrumented
that one function to append each builtin name (once per process) to a file named
by an env var, rebuilt, and ran `cargo test --workspace -j 6 --no-fail-fast`.
The probe was then reverted; the tree is back at `fccc4bc` byte-for-byte.

Child processes inherit the environment (`cli_run.rs`'s `axon()` helper never
calls `env_clear`), so every `axon` the suite spawns — including the ~50 harness
scripts it shells out to — reports into the same file.

### The probe was wrong the first time, and the mutation caught it

The first probe used `writeln!(f, "{name}")` on an unbuffered `File`. That
issues **two** write syscalls (name, then newline), so concurrent `axon`
processes interleaved and merged pairs of names into unparseable tokens —
measured: **611** such tokens, including `str_char_slicemax_value`. That one
lost `str_char_slice` from the executed set, and it was a **mutation** (§2) that
exposed it, not inspection. Fixed to a single `write_all` of `"{name}\n"` and
re-measured. All numbers below are from the corrected run.

## 2. Result

**291 of 343** builtins are executed at least once through the interpreter
during a full `cargo test --workspace`. **52** are not. Of those 52, **33** were
scored COVERED by the mention sweep.

Baseline: the tree at `fccc4bc` already fails 2 tests
(`axon-os --test acceptance`: `acc_a2_example_jobs_run_and_overreach_denied`,
`acc_a6b_verdict_seed_and_run_id_are_sealed_into_the_chain_p6_exit_03`). Verified
pre-existing on a pristine checkout before any instrumentation.

The 52 break into five populations:

| population | n | reading |
|---|---|---|
| **A. Bare-metal HAL** — `hlt` `sti` `cli` `lidt` `port_in_u8` `port_out_u8` `volatile_{load,store}_u{8,16,32,64}` `fn_addr` `ptr_from_addr` `atomic_{load,store,cas,fetch_add}_i64` `zephyr_console_putc` | 19 | Not an interpreter concern by design; their path is codegen + QEMU. `scripts/qemu_boot_test.sh` and `scripts/timer_irq_qemu_test.sh` ARE wired in (`integration_fixtures.rs:1755`, `:1801`). |
| **B. eBPF** — `bpf_map_lookup_elem` `bpf_map_value_add` `bpf_ktime_get_ns` `bpf_get_smp_processor_id` | 4 | See §4 — the only harness that runs them is invoked by nothing. |
| **C. TEE** — `tee_seal` `tee_unseal` `tee_in_enclave` `tee_attest_measurement` | 4 | Same: see §4. |
| **D. Probe blind spot** — `E` `Var` `P` `Chan::new` | 4 | Intercepted in `eval.rs` before `call_builtin`, so the probe cannot see them. `E[…]`/`Var[…]`/`P(…)` are genuinely exercised by `examples/stdlib/distribution.ax` and `examples/stdlib/phase14_tests.ax`, both glob-swept. Not a coverage gap — a limit of my instrument. |
| **E. Interp-reachable, "covered", never executed** | 10 | The real finding. `ai_extract_uncertain_f64` `axon_concat` `eprint` `format` `gaussian_sample` `http_post` `json_get_str` `json_parse` `temporal_is_valid` (+ `str_char_slice` — which turned out to be a probe artifact, see §2). |

All of population E has a working arm inside `call_builtin` (checked
individually) — these are not codegen-only builtins the interpreter declines.

### How the mention query scored them

The false-positive mechanism is blunt. `format` is scored covered by **144**
Rust files — every one of them a `format!` macro. `cli` is scored covered by
**20** Rust files and 4 test files — the x86 `cli` instruction, matched against
the English word and against `cli_run`. `eprint` is scored covered by Rust
`eprintln!` neighbourhoods and by two harness scripts that name it in an
exclusion list.

## 3. Mutation sample — 8 survivors out of 10

Static "probably never executed" is a guess. To convert it to a measurement I
mutated `call_builtin` so that a call to any of 10 named builtins returns
`Flow::Panic("AUDIT-MUTANT: <name> was CALLED")`, rebuilt, and ran the **whole**
workspace suite. Every edit asserted its anchor matched exactly once before
writing; every restore was `git checkout -- <file>`.

Result: `645 passed; 1 failed` in `cli_run` plus the 2 pre-existing `axon-os`
failures. **One** mutant was killed.

| builtin | mutant killed? | by |
|---|---|---|
| `str_char_slice` | **KILLED** | `cli_run::character_access_is_indexed_by_character_not_byte` |
| `char_is_space` | **KILLED** (isolated re-run) | same test — see below |
| `axon_concat` | survived | — |
| `json_parse` | survived | — |
| `json_get_str` | survived | — |
| `temporal_is_valid` | survived | — |
| `ai_extract_uncertain_f64` | survived | — |
| `gaussian_sample` | survived | — |
| `eprint` | survived | — |
| `tee_unseal` | survived | — |

**Batched mutation interferes.** `char_is_space` appeared to survive only
because `str_char_slice` panicked earlier in the same fixture program, so its
call site was never reached. Mutating `char_is_space` alone kills that test.
Any batched mutation result has to be read with this in mind — I re-ran the one
case where two mutants shared a program.

The 8 survivors are independently corroborated: the corrected execution probe
reports each of them as never called, and the mutation reports each as never
observed. Two instruments, same answer.

### Three survivors, with the mechanism named

* **`axon_concat`** — appears in 10 `.ax` files. Its live call site is
  `examples/property_test.ax:28`, inside `@[test] @[forall] fn
  concat_len_additive`. That file has **no `fn main`**, so
  `scripts/all_examples_parity.sh` skips it (`grep -q "fn main" || continue`),
  and nothing anywhere runs `axon test examples/property_test.ax`. Four property
  tests, shipped, executed by nothing. (`cli_run.rs:7541
  forall_property_test_passes_and_shrinks` tests the `@[forall]` feature against
  its own inline source, not this file.)
* **`gaussian_sample`** — one call site, `examples/stdlib/distribution.ax:131`,
  inside `fn dist_sample_gaussian`. `dist_sample_gaussian` has **zero callers in
  the entire repository**. The module is glob-swept and green (§5); the function
  inside it is dead. This is a Phase-13 sampling builtin the phase table lists as
  Complete.
* **`eprint`** — a core I/O builtin. No test in the workspace calls it. Every
  "mention" that scored it covered is a Rust `eprintln!` or an exclusion-list
  entry in a wasm harness.

## 4. Two harness scripts that nothing invokes

`scripts/ebpf_verify.sh` and `scripts/tee_sim_run.sh` are referenced by no test,
no gate, and no aggregator. `parity_all.sh` globs `scripts/*_parity.sh` and does
not match either name; `gate.sh` and the acceptance gates do not name them.
Their only references outside themselves are prose: `ENVIRONMENTS.md`,
`SESSION_STATUS.md`, `governance/specs/R23-ebpf-target.md`,
`governance/specs/R24-tee-target.md`, and older review files.

That is the whole execution story for populations B and C (8 builtins). What
coverage they do have is compile-time only — e.g.
`integration_fixtures.rs:2091 r23_bpf_counter_clean` calls `check_fixture`, which
runs parse → resolve → infer → check and **never executes** the program.

`check_fixture` (`integration_fixtures.rs:9`) is in fact the single largest
source of nominal coverage in this tree: a `.ax` file under
`crates/axon-core/tests/fixtures/` is a test-file mention by the sweep's
definition, and a type-check by what actually happens to it. `char_access.ax`,
`temporal_basic.ax`, `ai_extract_uncertain.ax`, `phase48_string_builder.ax`,
`phase49_numeric_format.ax`, `r24_tee_unseal_e1810.ax` are all in this class.

## 5. Vacuous-pass sweeps

I checked every `read_dir`-driven sweep in the Rust tests and every glob loop in
`scripts/`.

**The Rust-side sweeps are all guarded**, and explicitly so —
`stdlib_module_acceptance_suites_pass` (`cli_run.rs:19362`) requires `passed > 0`
per module with a comment explaining why; `every_goal_example_compiles_and_runs`
requires `files.len() >= 5`; `asi_demo_set_runs_without_crashing` requires
`> 20`; `run_and_check_emit_identical_diagnostics_across_a_corpus` requires
`compared >= 7`; `w0007_has_no_false_positives_on_the_shipped_corpus` requires
`scanned > 100`; `fmt_is_idempotent_…` requires `checked >= 12`. Nothing to
report there.

**`scripts/wasm_aot_stdout_parity.sh:92`** — every per-file failure mode in its
loop is a `skip`, and the tail reads:

```
if [ "$pass" -eq 0 ]; then echo "…: nothing ran — skipping"; exit 0; fi
```

Demonstrated, not inferred: `AXON=/bin/false bash
scripts/wasm_aot_stdout_parity.sh` prints `0 match, 0 differ, 42 skipped` and
**exits 0**. Its two siblings guard exactly this case with an explicit floor
(`wasm_examples_parity.sh:31 FLOOR=25`, `wasm_browser_examples_parity.sh:24
FLOOR=28`); this one does not.

Both consumers do absorb it: `parity_all.sh` classifies by the last output line
(`skip|unavailable` → SKIP) and holds `EXPECT_MIN_PASS=40`, and the `cli_run`
wrapper asserts the `PASS` line is present. So this is a vacuously-green **exit
code** rather than a vacuously-green suite. The residual is that one harness
silently converting from PASS to all-SKIP moves the aggregator from 44 to 43
against a floor of 40 — absorbed without a word.

**`scripts/all_examples_parity.sh`** has no floor on `$total`. Every example
that fails `$AXON check` is counted `by_design` and skipped; if `check` began
failing across the board the harness would print
`all_examples_parity: PASS — 0 examples native==interp` and exit 0. The script
front-loads strong probes against the known cause (an interp-only binary at the
shared `target/debug/axon` path) with a long comment about having been burned
twice, so the realistic path to this is narrower. **Not demonstrated** — I could
not find a cheap lever to force it, and I am not reporting it as a measured
result.

## 6. What this audit cannot establish

* **Native/codegen execution is invisible to it.** The probe and the mutations
  both live in the interpreter. A builtin exercised only through `axon build` +
  the produced binary — which is how populations A, B and C are meant to be
  exercised — reads as "not executed" here and may be perfectly well covered.
  For the bare-metal set (A) I checked the QEMU harnesses are wired in, but I
  did **not** verify that each individual instruction is asserted on.
* **It measures execution, not assertion.** A builtin can be called by
  `asi_demo_set_runs_without_crashing` — which asserts only "did not crash" —
  and count as executed here while nothing checks its return value. A
  return-a-wrong-value mutation would separate those; I ran only panic-on-call
  mutations. The 291 figure is therefore an **upper** bound on meaningful
  coverage.
* **The mutation sample is 10 of 343**, chosen from the population the probe
  flagged, so it is not a random sample and its survival rate does not
  generalize to the other 291.
* **`E` / `Var` / `P` / `Chan::new`** bypass `call_builtin` and are structurally
  invisible to the probe. Others may be too; I found these four by reading
  `eval.rs`, not by an exhaustive search for interception sites.
* **One run, one machine.** Two `axon-os` tests already fail at `fccc4bc`, and
  two Android harnesses skipped (`target/harness-skips.log`). Anything those
  would have covered is unmeasured here.
* **The 15 builtins assigned to other agents were deliberately not investigated**
  beyond confirming my reproduction of the sweep matches theirs.

## 7. Bottom line

Of 343 builtins, the mention sweep scores 324 covered. Execution measurement
puts 291 as actually called by the full test suite, and of the 33 that the sweep
called covered while nothing executed them, 10 are ordinary interpreter builtins
with no bare-metal excuse. On a 10-builtin mutation sample drawn from that
population, **8 mutants survived the entire workspace test suite**.

The sweep's error rate is not uniform — it is concentrated where a builtin's
name is also a common English or Rust word (`cli`, `format`, `eprint`), where a
`.ax` file is type-checked rather than run (`tests/fixtures/`), where a file has
no `fn main` (`examples/property_test.ax`), and where a harness exists but
nothing calls it (`ebpf_verify.sh`, `tee_sim_run.sh`).

No fixes are included in this commit. It is the measurement only.
