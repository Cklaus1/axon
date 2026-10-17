# DRAFT — Parity-harness and test-reliability residue (H1-H7)

**This draft MUST pass a Step 1 adversarial review before any build.** Nothing here is reviewed yet.

Source: `tasks/opportunities.md` § "gaps run (2026-10-15)", bullets at :2500-2501 (load-dependent test) and
:2502-2508 (parity harness leftovers from G4). Base: `main` 71d6f503 (`gaps/int`). Every item was re-measured
against this tree on 2026-10-17; line numbers are from it. Claims not observed are tagged [INFERENCE].

Mission: a parity harness reports SKIP only for a reason it has proven, says that reason, fails with the
compiler's own words, and leaves nothing in the worktree; one known-flaky lib test stops depending on which
other tests share its process.

Shared acceptance: `bash scripts/test_harness_skip.sh` green; `scripts/gate.sh --fast` shows no new failure
against the baseline; `python3 -B scripts/v022_refusal_coverage.py` and `scripts/v022_g01_mutations.py` stay
green (any anchor these edits move is re-pinned in the same commit); one strict parity run at the end (G-PAR).

## H1 — two harnesses build into the worktree whatever CARGO_TARGET_DIR says

Current: `wasm_host_await_parity.sh:62-64` and `browser_compute_parity.sh:82-85` set
`CARGO_TARGET_DIR="target/interp-only"`. Both `cd "$ROOT"` first (:26-27, :33-34), so the path resolves to
`$ROOT/target/interp-only` and overrides a caller's outside target dir: a lane building elsewhere gets a full
second `axon-core` build inside the worktree. Observed: `gaps-int/target/interp-only/` exists (mtime
2026-10-15 04:31). It is gitignored (`.gitignore:7` `/target/`), so this is cost and clutter, not an admission
risk.
Change: one helper in `scripts/lib/axon_bin.sh`, `interp_target_dir` = `"$(cargo_target_dir)/interp-only"`
(`cargo_target_dir`, axon_bin.sh:35, already resolves env, config and workspace default); both harnesses use it
for the build and for `NATIVE`. Rationale: same stable path for cargo reuse, under whatever dir the caller chose.
The existing `target/interp-only/` dirs in worktrees are left alone (user state).
Acceptance (fail-first): a shape row in `scripts/test_harness_skip.sh` refuses any `CARGO_TARGET_DIR=` whose
value is a relative literal in `scripts/*.sh` (red on today's tree at those two lines); behavioural check once:
`CARGO_TARGET_DIR=/var/tmp/r51-tmp/h1 bash scripts/wasm_host_await_parity.sh` in a scratch copy creates no
`$ROOT/target/interp-only`. Mutation: restoring the literal turns the shape row red.

## H2 — parity_all prints the wrong line as the skip reason

Current: `parity_all.sh:120` takes the last non-empty line; `:139` prints it as the reason. The detector
(`harness_skip.sh:185-192`, rule 3) accepts a skip line up to two lines before the end, so for the table's own
row "prose skip then build error" (`test_harness_skip.sh:32`) the printed reason is
`error: IR verification failed`, not the skip.
Change: `harness_skip_reason <output>` in `harness_skip.sh` returns the line the rule matched (final line if it
decided, else the last column-0 skip line in the window), sharing `_skip_re`/`_verdict_re`; `parity_all.sh:139`
uses it. `test_harness_skip.sh` gains a reason column for skip rows. The Rust table (`cli_run.rs:33426-33464`)
does not print reasons, so reason rows are shell-only; the "add a row to both tables" comments
(`test_harness_skip.sh:3-5`, `harness_skip.sh:169-171`, `cli_run.rs:33422-33425`) say so.
Acceptance (fail-first): the reason row for :32 expects `foo_parity: native build failed — skipping` (red today
if checked against `tail -1`). Mutation: `harness_skip_reason` returning the final line turns it red.

## H3 — "link unavailable" / "nothing linked" skips never prove the link is unavailable

Current: `try_link_wasm` (`crates/axon-core/src/main.rs:3265-3369`) returns `None` silently when `$HOME`, the
rustup toolchains dir, `rust-lld` (:3302) or the wasi libc (:3334) is missing, and returns `None` after printing
`wasm link failed (...)` (:3352-3356) when the linker ran and failed. `find_wasm_axon_rt` returning `None`
(:3314) is not reported; the link proceeds without the runtime and fails on undefined `__axon_rt_*`. In every
case `axon target build` exits 0 with no `.linked.wasm`. The harnesses read "no `.linked.wasm`" as an absent
toolchain without looking at stderr:
`wasm_aot_run_parity.sh:138,159`, `wasm_aot_stdout_parity.sh:91`, `wasm_examples_parity.sh:79-92`,
`wasm_browser_parity.sh:84,144`, `wasm_browser_io_parity.sh:80,148`, `wasm_browser_examples_parity.sh:78`,
`wasm_malloc_abi_parity.sh:151`, and the same shape in `wasi_env_control_gate.sh:61`.
Demonstrated: `/var/tmp/r51-tmp/parity_strict.log` (2026-10-15 03:58, before ca49d537 fixed the runtime lookup)
shows five SKIPs, all caused by the compiler not finding its own runtime: `wasm_aot_run` "nothing linked",
`wasm_browser_examples`, `wasm_browser_io`, `wasm_browser`, `wasm_examples` "link unavailable", plus 29
per-example "not linkable" in `wasm_aot_stdout`. Re-observed 2026-10-17: an older codegen `axon`
(`.doc-build/cortex-heldout/debug/axon`) on `fn main() -> i64 { 0 }` exits 0 and prints `wasm link failed …
undefined symbol: __axon_rt_seed_rng`.
Change:
- Compiler: each silent `None` prints one stable line `wasm link unavailable: <rust-lld|wasi libc|rustup
  toolchains> not found (searched <paths>)`; a `find_wasm_axon_rt` miss prints `wasm runtime not found:
  libaxon_rt.a for <subdir> (searched <paths>)` before linking. No exit-code change (object-only stays a
  successful build; [INFERENCE] other callers rely on that).
- Shell: `wasm_link_unavailable <stderr>` in `harness_skip.sh` is true only on the `wasm link unavailable:`
  marker. A missing runtime is never a skip: these harnesses build `axon-rt` themselves. `wasm_aot_run_parity.sh`
  is the one that does not, and gains the same `cargo_build_or_skip … target:wasm32-wasip1 cargo build -p axon-rt
  --target wasm32-wasip1` the others run. At every listed site: no `.linked.wasm` + marker → SKIP with that
  line as the reason; otherwise FAIL printing the build stderr (first 20 lines). Per-example object-only results
  in the sweeps are H6.
Acceptance (fail-first, needs one codegen `axon`, see G-BIN): (a) runtime unreachable (binary copied outside
any target dir, run from a scratch cwd, `AXON_WASM_RT` unset) → `wasm_aot_run`, `wasm_examples`,
`wasm_malloc_abi` exit 1 naming `wasm runtime not found` (today: SKIP); (b) `HOME=/nonexistent` → SKIP whose
reason is the `wasm link unavailable:` line; (c) normal env → PASS. Mutation: `wasm_link_unavailable` always
true turns (a) red.

## H4 — six harnesses fail a build without the shared FAIL shape; three drop the compiler's stderr

Re-measured: the opportunity note is half right. `decimal_parity.sh:31-35`, `suspend_resume_parity.sh:41-44`
and `browser_compute_parity.sh:82-84` send `cargo build` stderr to `/dev/null` and print only "build FAILED".
`wasm_fs_parity.sh:62,66`, `wasm_parity.sh:70,74` and `vm_parity.sh:136-137` do NOT discard stderr (it reaches
`parity_all.sh:111,126`'s FAIL dump), but bypass `cargo_build_or_skip` and print messages that do not name the
harness ("native build failed").
Change: all six call `cargo_build_or_skip <name> interp …` (wasm32 legs: `target:wasm32-wasip1`, already probed
at `wasm_fs:47`, `wasm_parity:54`); the two private `$WORK/interp-target` builds keep their dir.
`vm_parity.sh`'s build failure exits 1 instead of 2: its only caller, `gate.sh:384`, tests non-zero.
Acceptance (fail-first, throwaway, no commit): with a syntax error injected into a `--no-default-features`-built
source file in a scratch copy, each of the three stderr-dropping harnesses prints the rustc `error[...]` line
(today: absent). Static: a shape row in `test_harness_skip.sh` refuses `cargo build … 2>/dev/null` inside
`scripts/*_parity.sh`, which also catches the ~40 copies of the same shape G4 left in the build log
(`/var/tmp/r51-tmp/parity_builds.txt`), [INFERENCE] most of them since converted; the build run counts what is
left and either converts each one or names it as an exception in the row.

## H5 — two cli_run tests re-implement the skip rule they are meant to check

Current: `cli_run.rs:171-205` (`a_failing_harness_is_never_a_skip_gate_04`) checks only that its fixture
contains "skipping" and what a local `last_line` closure returns; `:213-239`
(`a_skip_that_elaborates_is_still_a_skip`) re-implements the window rule as a closure. Their own comment
(:207-211) and `skip_rule_table`'s (:33418-33420) say this is how they stayed green while the real function was
wrong. Every shape they cover is a row of `skip_rule_table` (`detectors_agree_on_every_shape`, :33431): the
elaborated skip (:33446), the mid-run note (:33458), the real skip (:33433). The exit-status rule
(`harness_skipped`, :90-95) is exercised by no test (no `should_panic` in the file). No script or gate references
either name (grep over the tree: only old `tasks/*.txt` test logs).
Change: delete module `harness_skip_rules` (:164-240); add one `#[should_panic(expected = "a failure is never a
skip")]` test calling the real `harness_skipped` with a non-zero `ExitStatus` (`ExitStatusExt::from_raw(256)`)
and output ending `…: SKIP — x`; fix the :33418-33420 comment.
Acceptance: test count changes by −2 +1; mutation: deleting the `assert!` at :90-95 turns the new test red.

## H6 — wasm_aot_stdout: a per-example build failure is a skip held only by a floor

Current: `wasm_aot_stdout_parity.sh:87-88` discards stderr and calls any non-zero `target build` "SKIP (wasm build
unavailable)"; `:91` calls any missing `.linked.wasm` "not linkable". Only `FLOOR=25` (:119; comment says 29 of
42 today) catches a regression, so up to four examples can stop building or linking silently.
`wasm_examples_parity.sh:107` has the same shape (counted "object-only", floor-held).
Change (both sweeps): build stderr captured. Non-zero exit: `native_build_unavailable` → case SKIP; a codegen
refusal (`E0910`, the honest no-lowering error, `codegen/asi.rs:384` shape) → counted `REFUSED`, printed;
anything else → case FAIL with stderr. Exit 0 without `.linked.wasm`: H3 marker → case SKIP; otherwise
object-only, handled per NH1. The floors stay as a second guard.
Step 0 of the build (G-BIN binary): a census of the 42 examples' outcomes, recorded in the commit message. The
2026-10-17 attempt used `.doc-build/gaps-int/debug/axon` and every example answered E0907 (that binary is
interp-only), so today's split of refused / object-only / linked is unmeasured.
Acceptance (fail-first): an example given a codegen-only fault (scratch copy, throwaway) turns the harness red
instead of reading `SKIP <n>`; mutation: mapping the "anything else" branch back to SKIP passes that run → red.

## H7 — `operator_code_a_sealed_frame_runs_never_draws_the_operator_rng` races other tests for a global RNG

Tally (all logs under `/var/tmp/r51-tmp`): 22 of 28 `*gate*.log` contain the test; 21 ok, 1 FAILED
(`gc-gate-fast.log`, 2026-10-15 03:52, the gaps-cancel lane's `--fast`). Across every log that names it (gate,
fast, suite, lane): 63 ok, 1 FAILED. No log under `/home/cklaus/projects/axon-lanes` names it.
Failure (gc-gate-fast.log:1363-1366): the CONTROL at `tests_v022_b.rs:1460`, `Err("assertion failed: 652 !=
327")`: exactly one foreign draw between the suite's `srand(7)` and its next draw.
Mechanism: the operator kernel's stream is the process-global `static RNG_STATE: AtomicU64`
(`interp.rs:7417`; `Kernel::rng_next` → `next_rand_u64`, :1846-1848, :7528-7537; `srand` → `set_rand_seed`,
:7522-7525), deliberately process-wide so successive `@[test]` fns continue one stream (:1795-1799, "Veto gave
the operator kernel a per-`Interp` cell, which restarts the stream at each test"). `cargo test` runs lib tests
on parallel threads in one process, so any concurrent test that draws or reseeds through an operator kernel
lands between `srand(7)` and the assert. Load widens the window; it is not the cause. The sealed stream is
per-kernel (`Kernel::rng` cell) and race-free: `a_sealed_frame_draws_and_reseeds_only_its_own_stream`
(`kernel_tests.rs:198`) is not exposed. Other lib tests that seed the operator stream and depend on its values
(`taint_tests.rs:3754-3757`) are [INFERENCE] exposed the same way.
Change (test-side only; production keeps the vetoed-for semantics): a `#[cfg(test)]` thread-local override of
the operator stream, used by `next_rand_u64`/`set_rand_seed` when set and consulted nowhere else. It is switched
on inside the `on_deep_stack` closure of `sealed_outcome_rule` (`interp.rs:9219-9235`) the same way that closure
already sets `DISPATCH_RULE_OFF` / `TAINT_FORCE_ON`, and by any other test that seeds and asserts the operator
stream. Rationale: precedent for per-thread test switches exists there; no `cfg(not(test))` behaviour changes.
Acceptance (fail-first, deterministic): a `kernel_tests.rs` test enables the override on thread A, runs `srand(7)`
through the interpreter, has a scoped thread B call `next_rand_u64()` and joins it, then draws on A: expects 327
(today 652). Mutation: dropping the `cfg(test)` branch in `next_rand_u64` turns it red. Then the flaky test
passes 20/20 in `cargo test -p axon-core --lib --no-default-features` run alongside a concurrent loaded build.

## Gates and budgets

- G-SHELL: `bash scripts/test_harness_skip.sh`. Under 1 s, no build. Every edit to `harness_skip.sh`.
- G-LIB (H7): `cargo test -p axon-core --lib --no-default-features`. Wall time about 5 s of tests plus a lib
  rebuild (minutes; competes with any lane's cargo for CPU).
- G-CLI (H5): `cargo test -p axon-core --test cli_run harness_skip`. `cli_run.rs` is 39,963 lines, so the
  compile is the cost (several minutes, [INFERENCE] from size). Run once after H5 lands, not per edit.
- G-BIN (H3, H6): one codegen `axon` (LLVM 17) plus `axon-rt` for wasm32-wasip1, in
  `CARGO_TARGET_DIR=/home/cklaus/projects/axon-lanes/.doc-build/<lane>`. One build, reused by every H3/H6 check
  through `AXON=`/`AXON_RUN=`.
- G-PAR: `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` once, at the end, from the integration worktree. Budget
  **about 22 min** wall time, the strict run observed by the gaps run. It does LLVM links and wasm builds and
  takes the `axon_wasm_parity.lock` flock, so it competes with every concurrent cargo build: run it with no lane
  building. Expect 55 passed / 2 allow-listed skipped / 0 failed (`gaps-final-strict-gate.log:11153`), or the
  same plus a stated reason for any change. The cost comments at `gate.sh:1002` ("~2 min"), `:1321` ("7m49s",
  2026-08-04) and `:1336` ("~8 min") are corrected to the measured figure in the same change.
- `scripts/gate.sh --fast` once before merge (baseline comparison).

## Owned files

`scripts/lib/harness_skip.sh`, `scripts/lib/axon_bin.sh`, `scripts/test_harness_skip.sh`, `scripts/parity_all.sh`,
`scripts/{wasm_host_await,browser_compute,decimal,suspend_resume,wasm_fs,wasm,vm,wasm_aot_run,wasm_aot_stdout,
wasm_examples,wasm_browser,wasm_browser_io,wasm_browser_examples,wasm_malloc_abi}_parity.sh`,
`scripts/wasi_env_control_gate.sh`, `scripts/gate.sh` (comments only), `crates/axon-core/src/main.rs`
(`try_link_wasm`/`find_wasm_axon_rt` diagnostics only), `crates/axon-core/src/interp.rs` (cfg(test) RNG override),
`crates/axon-core/src/interp/kernel_tests.rs`, `crates/axon-core/tests/cli_run.rs` (:85-240, :33418-33464),
`CHANGELOG.md`, `tasks/opportunities.md` (strike the two bullets).

## Needs human

- **NH1 — object-only examples in the two wasm sweeps.** Today an example that builds but does not link is a
  "frontier" result, held only by a floor with 4 (stdout) or more (examples) of slack. Option A: a checked-in list
  (`scripts/wasm_object_only.txt`, name + reason, the `parity_allowed_skips.txt` shape); an unlisted object-only
  example FAILs and a listed one that now links FAILs as a stale entry. Option B: keep the floor. A costs a list
  edit whenever an example is added that does not link on wasm. **Recommend A**: the floor is the "held only by
  its floor" defect.

## Non-goals

Converting the remaining `2>/dev/null` build shapes outside the ten parity harnesses listed (H4 counts and
reports them); making the production operator RNG per-thread or per-`Interp` (vetoed, interp.rs:1798-1799);
`wasm_examples_parity` writing `.wasm` files into `examples/` (:105-112; separate item if wanted);
`try_link_wasm` finding rustup only under `$HOME/.rustup` (`RUSTUP_HOME` ignored, :3273-3274); promoting
parity_all into the default gate.

## Open questions

- Does any lib test rely on a sealed run sharing the operator stream with the code around it? H7's override is
  set inside `sealed_outcome_rule` only; a grep of the tests that call it after `srand` settles this.
- Is an in-process concurrent `Interp` reachable in production (axon-web, dashboard)? If it is, the same race is
  a product bug, not only a test one. [INFERENCE] not checked; out of scope here, log it if found.

## Dropped on re-measurement

None dropped. H4 is narrowed: three of its six harnesses do pass stderr through (evidence above); they are kept
for the shared FAIL shape, not for lost stderr.
