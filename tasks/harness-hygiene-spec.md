# DRAFT — Parity-harness and test-reliability residue (H1-H7)

**Status: BUILT 2026-10-17 on `main` 8afc76ba + d2d91d0b (H1-H7). Stamped 2026-10-29 by tasks/docs-truth-spec.md DT-5.**

**This draft MUST pass a Step 1 adversarial review before any build.** Nothing here is reviewed yet.
[REVISED: Step 1 adversarial review done 2026-10-17 (ReviewHarness) against `gaps/int` 43fa1857, which is 71d6f503
plus the four draft specs; no code changed between them. Every line number below was re-checked on 43fa1857. The
review changes are marked inline. Verdict: H1, H2, H4, H5 and H7 can be built as revised. H3 and H6 are buildable
only with the corrections below: H3's premise "these harnesses build axon-rt themselves" does not hold for
`wasm_aot_run` or `wasi_env_control_gate`, and two of its acceptance runs would not reach the code they claim to test.
No item is dropped.]

Source: `tasks/opportunities.md` § "gaps run (2026-10-15)", bullets at :2500-2501 (load-dependent test) and
:2502-2508 (parity harness leftovers from G4). Base: `main` 71d6f503 (`gaps/int`). Every item was re-measured
against this tree on 2026-10-17; line numbers are from it. Claims not observed are tagged [INFERENCE].

Mission: a parity harness reports SKIP only for a reason it has proven, says that reason, fails with the
compiler's own words, and leaves nothing in the worktree; one known-flaky lib test stops depending on which
other tests share its process.

Shared acceptance: `bash scripts/test_harness_skip.sh` green; `scripts/gate.sh --fast` shows no new failure
against the baseline; `python3 -B scripts/v022_refusal_coverage.py` and `scripts/v022_g01_mutations.py` stay
green (any anchor these edits move is re-pinned in the same commit); one strict parity run at the end (G-PAR).
[REVISED: added, reason: `scripts/pci_delta.py --check` runs in the fast tier and fails any commit under
`crates/axon-core/src` that `governance/notes/pci-themes/` does not classify. H3 (`main.rs`) and H7 (`interp.rs`,
`interp/kernel_tests.rs`) are such commits. Each one gets `governance/notes/pci-themes/<hash8>.json`. Suggested text
for H3: `{"theme": "gaps harness hygiene", "text": "TOOLING (wasm link diagnostics on stderr only; no change in what
any run computes, accepts or links)"}`. Suggested text for H7: "TEST-ONLY (cfg(test) operator-RNG override; the
non-test build is unchanged)". Checked: neither edit lies in a refusal-coverage scope. The interp.rs region is
:5149-5494 (`v022_refusal_coverage.py:325-327`), and the main.rs entry fns are `cmd_test`/key/token only (:109-110). No
mutation or anchor row names `try_link_wasm`, `find_wasm_axon_rt`, `next_rand_u64`, `set_rand_seed` or
`harness_skip_rules`. M270 (`v022_g01_mutations.py:914`) anchors `Interp::rng_next` at interp.rs ~5224, which H7 does
not edit. `AXON-COMPLETENESS.json:704` describes `test_harness_skip.sh` as "mirroring skip_rule_table row for row".
H2 and H5 keep that true (see below), and the integrator updates the row text if it names the new reason column.]

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
[REVISED: added constraint, reason: `crates/axon-core/tests/harness_binaries.rs`
(`no_script_picks_a_binary_it_neither_built_nor_was_given`, scanner `script_spawn/mod.rs:228-308`) scans every script.
It accepts a binary path only under a directory the same script assigned to `CARGO_TARGET_DIR=`. The shape it accepts
is: assign once (`ITD="$(interp_target_dir)"`), build with `env CARGO_TARGET_DIR="$ITD" cargo build …`, and run
`NATIVE="$ITD/debug/axon"`. The assignment value `$ITD` is then in the script's own set
(`cargo_target_dir_assignment`), and the profile path's prefix contains no `target`. Any other spelling must be checked
against that scanner. Lane check: `cargo test -p axon-core --no-default-features --test harness_binaries`.]
Acceptance (fail-first): a shape row in `scripts/test_harness_skip.sh` refuses any `CARGO_TARGET_DIR=` whose
value is a relative literal in `scripts/*.sh` (red on today's tree at those two lines); behavioural check once:
`CARGO_TARGET_DIR=/var/tmp/r51-tmp/h1 bash scripts/wasm_host_await_parity.sh` in a scratch copy creates no
`$ROOT/target/interp-only`. Mutation: restoring the literal turns the shape row red.
[REVISED: confirmed by grep, these two lines are the only relative `CARGO_TARGET_DIR=` literals in `scripts/*.sh`.
`v022_stage5_verify.py:190` uses `os.path.join`. The shape row must skip comment lines.
Where it runs (the review found this): today `test_harness_skip.sh` runs ONLY from `parity_all.sh:79` (i.e. only
under `gate.sh --strict`). No fast-tier stage and no Rust test runs it. A shape row placed there would therefore not
guard a lane's `--fast` acceptance. Canon:
(1) The H1/H4 shape rows go in a separate, labelled `shape` section of `test_harness_skip.sh`, with its own FAIL line.
The detector rows stay row-for-row with `skip_rule_table`.
(2) `parity_all.sh:80`'s message changes from "the skip detector disagrees with its rule table" to name which section
failed.
(3) `gate.sh`'s fast tier runs `bash scripts/test_harness_skip.sh || fail "harness skip rules/shapes"` right after the
`vm_parity.sh` stage (:383-384). That costs 0.04 s (measured) and needs no build.]

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
[REVISED: confirmed against `parity_all.sh:111-146` and `harness_skip.sh:183-192`. Note: for the row at :34 ("skip
that elaborates"), the final line `…: this is a SKIP, not a pass.` decides, so its reason is that line and not the
`cannot codegen — skipping` line above it. That is accepted as canon: the rule reports the line that decided. The
reason row for :34 pins it, so a later change to "prefer the more specific line" is a visible decision. `parity_all`
still strips the `$name: ` prefix and cuts to 58 chars (:139), so the reason rows test the helper's output, not the
printed column.]

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
[REVISED: site list completed, reason: re-measured on 43fa1857.
- Two more silent `None`s in `try_link_wasm`. `.output().ok()?` at :3340 (rust-lld found but could not be spawned)
  and the final `else { None }` at :3366-3368 (the linker exited 0 but produced no file, or an empty one). Neither
  prints anything. Each must print one `wasm link failed: …` line. They must NOT print the `unavailable` marker,
  because the toolchain was found.
- More harness sites with the same shape. Per-case object-only: `wasm_browser_parity.sh:118`,
  `wasm_browser_io_parity.sh:107,141`. Per-example: `wasm_browser_examples_parity.sh:98-99`, which is a third sweep;
  see H6. Per-leg: `wasi_env_control_gate.sh:111-127`, where the AXON_SEED leg prints `skip  AXON_SEED leg: the random
  probe did not link` and the gate still PASSes.
- Do not confuse :3196-3200 with the new marker. It is the caller's stock "wasm object: … Link to a runnable .wasm
  needs `rust-lld` + a wasi libc" line, printed for EVERY `None`. It stays, and it must not contain the new marker
  text.]
Demonstrated: `/var/tmp/r51-tmp/parity_strict.log` (2026-10-15 03:58, before ca49d537 fixed the runtime lookup)
shows five SKIPs, all caused by the compiler not finding its own runtime: `wasm_aot_run` "nothing linked",
`wasm_browser_examples`, `wasm_browser_io`, `wasm_browser`, `wasm_examples` "link unavailable", plus 29
per-example "not linkable" in `wasm_aot_stdout`. Re-observed 2026-10-17: an older codegen `axon`
(`.doc-build/cortex-heldout/debug/axon`) on `fn main() -> i64 { 0 }` exits 0 and prints `wasm link failed …
undefined symbol: __axon_rt_seed_rng`.
[REVISED: confirmed. `parity_strict.log:111` reads `46 passed, 7 skipped, 4 failed`.]
Change:
- Compiler: each silent `None` prints one stable line `wasm link unavailable: <rust-lld|wasi libc|rustup
  toolchains> not found (searched <paths>)`; a `find_wasm_axon_rt` miss prints `wasm runtime not found:
  libaxon_rt.a for <subdir> (searched <paths>)` before linking. No exit-code change (object-only stays a
  successful build; [INFERENCE] other callers rely on that).
  [REVISED: details fixed, reason: code read.
  (a) `find` (:3275-3301) collapses "toolchains dir unreadable" into "rust-lld not found", because `read_dir(...).ok()?`
  returns None from the closure. To name "rustup toolchains", check `toolchains.is_dir()` before calling `find`.
  (b) "searched <paths>" for the runtime means: `AXON_WASM_RT` (wasip1 only, and only if set), each
  `compiler_target_dir()/<subdir>/{debug,release}` (`codegen/link.rs:1463`), and the cwd walk. Printing every
  ancestor of the walk is long, so print the start dir and "and its ancestors' target/".
  (c) No new `Command::new` and no rustup/cargo invocation in this path. killgroup-spec.md:26 relies on `main.rs`
  having exactly two `Command::new` sites (rust-lld, axon-vm). H3 only shifts their line numbers.
  (d) A missing runtime is reported even when the link then succeeds (a pure program may need no runtime symbol).
  This is diagnostic only. Harnesses read stderr only when no `.linked.wasm` exists.
  (e) The new lines go to stderr only. `cli_run.rs:34034` (`a_wasm_link_failure_reports_the_linkers_own_error`)
  checks `contains`, so extra lines do not break it.]
- Shell: `wasm_link_unavailable <stderr>` in `harness_skip.sh` is true only on the `wasm link unavailable:`
  marker. A missing runtime is never a skip: these harnesses build `axon-rt` themselves. `wasm_aot_run_parity.sh`
  is the one that does not, and gains the same `cargo_build_or_skip … target:wasm32-wasip1 cargo build -p axon-rt
  --target wasm32-wasip1` the others run. At every listed site: no `.linked.wasm` + marker → SKIP with that
  line as the reason; otherwise FAIL printing the build stderr (first 20 lines). Per-example object-only results
  in the sweeps are H6.
  [REVISED: premise corrected, reason: code read.
  - "These harnesses build axon-rt themselves" is half true. They build it into the CALLER's target dir
    (`cargo build -p axon-rt --target …` honours `CARGO_TARGET_DIR`). The compiler looks only in ITS OWN target dir
    (the `CACHEDIR.TAG` ancestor of the exe, `link.rs:1446-1457`) and then walks up from the cwd. The harnesses
    `cd "$ROOT"`, so the walk reaches `$ROOT/target`. The runtime is therefore found when (i) `$AXON` was built into
    the same dir the harness builds into (the `parity_all`/`gate.sh` case: no `AXON`, one `CARGO_TARGET_DIR`), or
    (ii) that dir is `$ROOT/target`.
  - Otherwise the runtime is not found. Example: `AXON=` names a binary in `.doc-build/A` while `CARGO_TARGET_DIR=B`.
    Two outcomes are possible there. A stale `$ROOT/target/wasm32-*` runtime may be linked; `gaps-int/target/` holds
    both wasm dirs today, so this is real on this host. Or nothing is found, and with H3 that becomes an honest FAIL
    naming `wasm runtime not found`. That is the intended result: it is a real misconfiguration, not an absent
    toolchain.
  - Canon: harnesses do NOT export `AXON_WASM_RT`. That would hide the compiler's own lookup (the ca49d537 path) from
    the very suite that exercises it, and it has no browser counterpart.
  - Stale-runtime linking through the cwd walk is logged to `tasks/opportunities.md`, not fixed here (see Non-goals).
  - `wasm_aot_run_parity.sh`: today it links its dict/array programs only because an earlier harness in
    `parity_all`'s alphabetical order (e.g. `wasm_aot_env_parity`) already built the wasip1 runtime into the same dir.
    The runtime build must go BEFORE `:57`. `:57` exports `CARGO_TARGET_DIR="$WORK/wasm-target"`, and a runtime built
    there is never searched. Nothing after :57 calls cargo: `axon target build` for wasm never invokes cargo, per
    `build_wasm_object_cli` and `try_link_wasm`. So the :57 export and its comment (:48-56) are dead and are deleted
    in the same change.
  - `wasi_env_control_gate.sh` builds no runtime either. It links `io.ax` (wasip1) and `pure.ax` (both targets) only
    because `gate.sh` runs it straight after `parity_all` (:1004 → :1014). It gains `cargo_build_or_skip
    wasi_env_control_gate target:wasm32-wasip1 cargo build -q -p axon-rt --target wasm32-wasip1`. Inside the leg-2
    branch (:132) it also builds the `wasm32-unknown-unknown` runtime. It must source `harness_skip.sh`, which it does
    not today. Its own rustup probe at :40-47 has the swallowed-status pipe shape that `rust_target_installed` exists
    to replace (`rc=$?` after `if !` is the negated status). It is converted to `rust_target_installed` in the same
    edit.
  - Per-case sites (`wasm_browser_parity.sh:118`, `wasm_browser_io_parity.sh:107,141`, `wasm_aot_run:138`): marker →
    case SKIP. `wasm runtime not found` → case FAIL. Any other object-only result → case FAIL, unless the case is
    listed in the NH1 list (decided A).
  - `wasi_env_control_gate.sh:126`: once leg 1 has linked, a probe that does not link is `bad`, not `skip`.]
Acceptance (fail-first, needs one codegen `axon`, see G-BIN): (a) runtime unreachable (binary copied outside
any target dir, run from a scratch cwd, `AXON_WASM_RT` unset) → `wasm_aot_run`, `wasm_examples`,
`wasm_malloc_abi` exit 1 naming `wasm runtime not found` (today: SKIP); (b) `HOME=/nonexistent` → SKIP whose
reason is the `wasm link unavailable:` line; (c) normal env → PASS. Mutation: `wasm_link_unavailable` always
true turns (a) red.
[REVISED: (a) and (b) rewritten, reason: as written, neither run reaches the code it claims to test.
- Old (a) fails three ways.
  - A copied binary still resolves `workspace_root()` to the baked `CARGO_MANIFEST_DIR` (`link.rs:1413-1438`). With
    no `CACHEDIR.TAG` above the copy, it searches `<that worktree>/target/wasm32-*`, which exists in `gaps-int`.
  - The harness `cd`s to `$ROOT` regardless of the caller's cwd.
  - The wasip1 harnesses (`wasm_examples`, `wasm_malloc_abi`, and `wasm_aot_run` after H3) build the runtime
    themselves and are then found through `$ROOT/target` when `CARGO_TARGET_DIR` is unset.
- New (a). Run in a `git archive` scratch copy S, with `$AXON` built into an outside dir A
  (`CARGO_TARGET_DIR=A cargo build -p axon-core --bin axon`; A has no `wasm32-*` subdirs). Run the harnesses with
  `CARGO_TARGET_DIR=B` (B ≠ A, outside S) and `AXON=A/debug/axon`. S has no `target/`. Expected:
  - `wasm_browser_parity`, `wasm_browser_io_parity` and `wasm_browser_examples_parity` exit 1 naming
    `wasm runtime not found` (today: SKIP "link unavailable"). The unknown-unknown target has no `AXON_WASM_RT`
    override, so nothing rescues the lookup.
  - The wasip1 harnesses exit 1 the same way.
  - Mutation: `wasm_link_unavailable` always true turns (a) red.
- Compiler-level (a′), cheap. `A/debug/axon target build --target wasm32-wasip1 p.ax`, run from a cwd under
  `/var/tmp/r51-tmp` with no `target/` above it. Expected: exit 0, stderr contains `wasm runtime not found:
  libaxon_rt.a for wasm32-wasip1`. Today: no such line.
- Old (b) fails. With `HOME=/nonexistent`, rustup and cargo lose their homes too: `rust_target_installed` returns 2,
  and the harness FAILs at its target probe before any link.
- New (b). Run with `RUSTUP_HOME="$HOME/.rustup" CARGO_HOME="$HOME/.cargo" HOME=/nonexistent` (wasmtime is
  `/usr/local/bin/wasmtime` on this host, so the `$HOME/.wasmtime` fallback is not needed). Expected: SKIP whose
  reason is the `wasm link unavailable: rustup toolchains not found …` line. [INFERENCE: rustup/cargo honour the two
  overrides without `$HOME`; one trial settles it. If not, (b) is checked at the compiler level only (`HOME=/nonexistent
  axon target build …` → marker on stderr, exit 0), plus a `wasm_link_unavailable` row in `test_harness_skip.sh`.]
- `test_harness_skip.sh` gains classifier rows (no build): marker → true; `wasm runtime not found` → false; `wasm link
  failed` → false; empty → false.]

## H4 — six harnesses fail a build without the shared FAIL shape; three drop the compiler's stderr

Re-measured: the opportunity note is half right. `decimal_parity.sh:31-35`, `suspend_resume_parity.sh:41-44`
and `browser_compute_parity.sh:82-84` send `cargo build` stderr to `/dev/null` and print only "build FAILED".
`wasm_fs_parity.sh:62,66`, `wasm_parity.sh:70,74` and `vm_parity.sh:136-137` do NOT discard stderr (it reaches
`parity_all.sh:111,126`'s FAIL dump), but bypass `cargo_build_or_skip` and print messages that do not name the
harness ("native build failed").
Change: all six call `cargo_build_or_skip <name> interp …` (wasm32 legs: `target:wasm32-wasip1`, already probed
at `wasm_fs:47`, `wasm_parity:54`); the two private `$WORK/interp-target` builds keep their dir.
`vm_parity.sh`'s build failure exits 1 instead of 2: its only caller, `gate.sh:384`, tests non-zero.
[REVISED: confirmed all six sites and the sole caller (`gate.sh:384`, `|| fail`; no Rust test runs `vm_parity.sh`).
Two additions:
- `vm_parity.sh` sources only `axon_bin.sh` (:130). It must also source `harness_skip.sh`.
- `vm_parity.sh` is in the FAST tier (`gate.sh:383-384`, no `AXON_BIN`, so it always builds). This change is
  exercised by every lane's `--fast`, not just by G-PAR.
The `env CARGO_TARGET_DIR=… cargo build` spelling (`wasm_host_await_parity.sh:62`) is the one that works through
`cargo_build_or_skip "$@"`.]
Acceptance (fail-first, throwaway, no commit): with a syntax error injected into a `--no-default-features`-built
source file in a scratch copy, each of the three stderr-dropping harnesses prints the rustc `error[...]` line
(today: absent). Static: a shape row in `test_harness_skip.sh` refuses `cargo build … 2>/dev/null` inside
`scripts/*_parity.sh`, which also catches the ~40 copies of the same shape G4 left in the build log
(`/var/tmp/r51-tmp/parity_builds.txt`), [INFERENCE] most of them since converted; the build run counts what is
left and either converts each one or names it as an exception in the row.
[REVISED: counted on 43fa1857 (non-comment lines of `scripts/*_parity.sh`). Exactly five remain: the three above, plus
`checked_arith_parity.sh:31` and `recursion_guard_parity.sh:40`, which both run `cargo build -q -p axon-rt 2>/dev/null
|| true`.
- Canon: convert both to `cargo_build_or_skip <name> interp cargo build -q -p axon-rt`. A host `axon-rt` build has no
  external prerequisite, so its failure is a FAIL. Both files are added to the owned list.
- The row must ignore comment lines: six wasm harnesses quote the old shape in comments, e.g.
  `wasm_aot_stdout_parity.sh:52`.
- Shapes outside `*_parity.sh` stay a non-goal. Today they are in `acceptance_gate.sh`, `axon_kernel_gate.sh`,
  `axon_session_demo.sh`, `fhir_roundtrip.sh`, `fix_codec.sh`, `modbus_roundtrip.sh`, `native_ffi_forge.sh`,
  `r22_acceptance_gate.sh`, `r23_acceptance_gate.sh`, `wasm_aot_link_probe.sh` and `wasm_browser_host_await.sh`.
  They are logged to `tasks/opportunities.md`. `r22_acceptance_gate.sh` is also used by approval-residue's
  acceptance, which is one more reason not to touch it here.
- The row lives in the `shape` section (see H1) and runs in `--fast`.]

## H5 — two cli_run tests re-implement the skip rule they are meant to check

Current: `cli_run.rs:171-205` (`a_failing_harness_is_never_a_skip_gate_04`) checks only that its fixture
contains "skipping" and what a local `last_line` closure returns; `:213-239`
(`a_skip_that_elaborates_is_still_a_skip`) re-implements the window rule as a closure. Their own comment
(:207-211) and `skip_rule_table`'s (:33418-33420) say this is how they stayed green while the real function was
wrong. Every shape they cover is a row of `skip_rule_table` (`detectors_agree_on_every_shape`, :33431): the
elaborated skip (:33446), the mid-run note (:33458), the real skip (:33433). The exit-status rule
(`harness_skipped`, :90-95) is exercised by no test (no `should_panic` in the file). No script or gate references
either name (grep over the tree: only old `tasks/*.txt` test logs).
[REVISED: "every shape they cover is a row" is FALSE for two shapes, reason: compared row by row against
`cli_run.rs:33431-33463` and `test_harness_skip.sh:27-41`.
- (1) The window BOUND. `a_skip_that_elaborates_is_still_a_skip`'s second half (:232-238) is a skip line more than
  three non-empty lines from the end. No row in either table tests the bound: the only mid-run row (:33458 / sh:39)
  is decided by its final `PASS` line under rule 2, before the window is consulted. Changing `take(3)` to `take(10)`
  (or `tail -3` to `tail -10`) breaks no row today.
- (2) A grandfathered prose skip as the FINAL line (`… (LLVM absent) — skipping`, :200). Every final-line skip row
  uses `: SKIP` or `this is a SKIP`. None uses the `skipping` alternative of `_skip_re` on the final line.
- Neither deleted test checks the real function, so deleting them loses no checking. But the claim of full coverage
  would be false. Canon: H5 adds two rows to BOTH tables (same text in each):
  - noskip `"foo_parity: native build failed — skipping\nfoo_parity: note a\nfoo_parity: note b\nfoo_parity: note c\n"`
    (window bound; column-0 non-verdict lines). Mutation: window 3 → 10 in either detector → red.
  - skip `"foo_parity: codegen build unavailable (LLVM absent) — skipping\n"`.]
Change: delete module `harness_skip_rules` (:164-240); add one `#[should_panic(expected = "a failure is never a
skip")]` test calling the real `harness_skipped` with a non-zero `ExitStatus` (`ExitStatusExt::from_raw(256)`)
and output ending `…: SKIP — x`; fix the :33418-33420 comment.
[REVISED: placement fixed, reason: G-CLI's filter.
- The new test goes INSIDE `mod skip_rule_table`, so one filter, `skip_rule_table`, selects exactly the two tests.
  The draft's G-CLI filter `harness_skip` matches only the module being deleted. After H5 it would select 0 tests,
  and `cargo test` exits 0 on 0 tests: a vacuous check.
- `ExitStatusExt` is `std::os::unix::process`, so the test is `#[cfg(unix)]`.
- `harness_skipped` takes `&std::process::Output`, so the test builds one with that status and empty stderr.]
Acceptance: test count changes by −2 +1; mutation: deleting the `assert!` at :90-95 turns the new test red.
[REVISED: plus the two new table rows (they live inside existing tests, so the count is unchanged by them).
`cargo test -p axon-core --no-default-features --test cli_run skip_rule_table` must report `2 passed`.]

## H6 — wasm_aot_stdout: a per-example build failure is a skip held only by a floor

Current: `wasm_aot_stdout_parity.sh:87-88` discards stderr and calls any non-zero `target build` "SKIP (wasm build
unavailable)"; `:91` calls any missing `.linked.wasm` "not linkable". Only `FLOOR=25` (:119; comment says 29 of
42 today) catches a regression, so up to four examples can stop building or linking silently.
`wasm_examples_parity.sh:107` has the same shape (counted "object-only", floor-held).
[REVISED: a third sweep has the same shape, reason: grep. `wasm_browser_examples_parity.sh:98-99` (`FLOOR=28`, :27,
"29/29") discards stderr, folds a failed build and a missing link into `objonly`, and is floor-held. It is added to H6
and to the owned files.]
Change (both sweeps): build stderr captured. Non-zero exit: `native_build_unavailable` → case SKIP; a codegen
refusal (`E0910`, the honest no-lowering error, `codegen/asi.rs:384` shape) → counted `REFUSED`, printed;
anything else → case FAIL with stderr. Exit 0 without `.linked.wasm`: H3 marker → case SKIP; otherwise
object-only, handled per NH1. The floors stay as a second guard.
[REVISED: (all three sweeps).
- The E0910 path is confirmed: `build_wasm_object_cli` prints each codegen error, then `error: N codegen error(s);
  wasm build aborted`, and exits 2 (`main.rs:3156-3165`).
- Added: exit 0 without `.linked.wasm` and stderr containing `wasm runtime not found` → harness FAIL (H3's rule), not
  object-only.
- NH1 is DECIDED A (see Needs human). Object-only examples are pinned in `scripts/wasm_object_only.txt`, keyed by
  target triple, because object-only is a property of example and target: rows `<triple> <example> <reason>`.
  `wasm_examples` and `wasm_aot_stdout` read the wasip1 rows, and `wasm_browser_examples` reads the unknown-unknown
  rows. The H3 per-case browser sites use the same file.
- An unlisted object-only example → FAIL. A listed example that now links → FAIL as a stale entry. A row without a
  reason → FAIL (same rule as `parity_allowed_skips.txt`, `parity_all.sh:49-63`).]
Step 0 of the build (G-BIN binary): a census of the 42 examples' outcomes, recorded in the commit message. The
2026-10-17 attempt used `.doc-build/gaps-int/debug/axon` and every example answered E0907 (that binary is
interp-only), so today's split of refused / object-only / linked is unmeasured.
[REVISED: the census covers all three sweeps (wasip1 ×2, unknown-unknown ×1) and the per-case browser programs. It
must use a codegen `axon` at or after ca49d537, with the wasm runtimes built into the same target dir. The census
output seeds `wasm_object_only.txt`. The floor comments (`wasm_aot_stdout:119`, `wasm_examples:34`,
`wasm_browser_examples:27`) are re-stated from it.]
Acceptance (fail-first): an example given a codegen-only fault (scratch copy, throwaway) turns the harness red
instead of reading `SKIP <n>`; mutation: mapping the "anything else" branch back to SKIP passes that run → red.
[REVISED: added the list checks. Removing one census-object-only row from `wasm_object_only.txt` → red ("unlisted
object-only"). Adding a row for an example that links → red ("stale entry").]

## H7 — `operator_code_a_sealed_frame_runs_never_draws_the_operator_rng` races other tests for a global RNG

Tally (all logs under `/var/tmp/r51-tmp`): 22 of 28 `*gate*.log` contain the test; 21 ok, 1 FAILED
(`gc-gate-fast.log`, 2026-10-15 03:52, the gaps-cancel lane's `--fast`). Across every log that names it (gate,
fast, suite, lane): 63 ok, 1 FAILED. No log under `/home/cklaus/projects/axon-lanes` names it.
Failure (gc-gate-fast.log:1363-1366): the CONTROL at `tests_v022_b.rs:1460`, `Err("assertion failed: 652 !=
327")`: exactly one foreign draw between the suite's `srand(7)` and its next draw.
[REVISED: confirmed. The log lines read verbatim as quoted. The lib run was `1057 passed; 1 failed … finished in
4.47s` (gc-gate-fast.log:1372).]
Mechanism: the operator kernel's stream is the process-global `static RNG_STATE: AtomicU64`
(`interp.rs:7417`; `Kernel::rng_next` → `next_rand_u64`, :1846-1848, :7528-7537; `srand` → `set_rand_seed`,
:7522-7525), deliberately process-wide so successive `@[test]` fns continue one stream (:1795-1799, "Veto gave
the operator kernel a per-`Interp` cell, which restarts the stream at each test"). `cargo test` runs lib tests
on parallel threads in one process, so any concurrent test that draws or reseeds through an operator kernel
lands between `srand(7)` and the assert. Load widens the window; it is not the cause. The sealed stream is
per-kernel (`Kernel::rng` cell) and race-free: `a_sealed_frame_draws_and_reseeds_only_its_own_stream`
(`kernel_tests.rs:198`) is not exposed. Other lib tests that seed the operator stream and depend on its values
(`taint_tests.rs:3754-3757`) are [INFERENCE] exposed the same way.
[REVISED: root cause CONFIRMED by code read. `next_rand_u64` is a plain load-xorshift-store (:7529-7536), and the
module comment at :7407-7408 still says "Single-threaded interpreter, so no CAS needed". Two corrections:
- taint_tests demoted. `taint_tests.rs:3754-3757` are `ctl[callback]` rows whose verdict is Refused/Ok. They pick
  `ops[min_i64(…, 0) + 1]`, a clamp that makes the selected op independent of the drawn value. They do not assert
  stream values. [INFERENCE from reading; not exposed in the way the H7 test is.]
- No production race, reason: every OS-thread site that runs the interpreter was traced.
  - `on_deep_stack` (:2937-2949): one scoped thread, joined before return.
  - `run_suspendable_values_inner` (:4182-4199): one worker interpreter. The host thread only services channel
    requests and never draws.
  - `axon test` runs its tests sequentially (`main.rs:6627-6665`, `.iter().map`).
  - `lib.rs:457`'s threads only parse.
  - No crate other than axon-core and axon-wasm calls the interpreter. axon-wasm is wasm32, single-threaded
    (`axon-wasm/src/lib.rs:240`).
  - The other `thread::Builder` sites (`ast.rs:1022`, `infer.rs:2889`, `lexer.rs:238`, `main.rs:920`,
    `axon-run.rs:26`) are big-stack trampolines, one at a time.
  So two operator kernels drawing concurrently happens only under `cargo test`. An out-of-tree embedder calling
  `run_program` from two threads would race; none exists in this tree.]
Change (test-side only; production keeps the vetoed-for semantics): a `#[cfg(test)]` thread-local override of
the operator stream, used by `next_rand_u64`/`set_rand_seed` when set and consulted nowhere else. It is switched
on inside the `on_deep_stack` closure of `sealed_outcome_rule` (`interp.rs:9219-9235`) the same way that closure
already sets `DISPATCH_RULE_OFF` / `TAINT_FORCE_ON`, and by any other test that seeds and asserts the operator
stream. Rationale: precedent for per-thread test switches exists there; no `cfg(not(test))` behaviour changes.
[REVISED: confirmed workable. `run_test_fn_inner` runs on the caller's thread (only `run_test_fn_outcome`, :4796,
wraps it in `on_deep_stack`), so a switch set inside the `sealed_outcome_rule` closure covers the whole run. The two
existing switches already depend on this.
Engineering canon:
- The override's state starts at 0, so the first draw seeds from `rng_seed()`, exactly as the global does.
- "Any other test that seeds and asserts the operator stream" is settled at build time by a census of the 9 `srand(`
  sites in `interp/{kernel_tests,taint_tests,tests_v022_b}.rs` and `taint.rs` (count measured). The census is listed in
  the commit message. Today's evidence names only `sealed_outcome_rule` (21 calls in `tests_v022_b.rs`).
- No source-text test pins `RNG_STATE`/`next_rand_u64` (grep over `crates/axon-core/{src,tests}`).
- The fn-item scanner (`interp.rs:9963`) reads interp sources up to `mod tests {` (:7846). A `#[cfg(test)]`
  thread_local placed beside `RNG_STATE` (:7417) is inside that range. It adds no closure-running fn, so the scanner
  is unaffected [INFERENCE from :10011-10077].
- The stale "Single-threaded interpreter, so no CAS needed" comment (:7407-7408) is corrected in the same edit to say
  what is true: single-threaded in production, and parallel only under `cargo test`, where the override applies.]
Acceptance (fail-first, deterministic): a `kernel_tests.rs` test enables the override on thread A, runs `srand(7)`
through the interpreter, has a scoped thread B call `next_rand_u64()` and joins it, then draws on A: expects 327
(today 652). Mutation: dropping the `cfg(test)` branch in `next_rand_u64` turns it red. Then the flaky test
passes 20/20 in `cargo test -p axon-core --lib --no-default-features` run alongside a concurrent loaded build.
[REVISED: two corrections.
- (1) "Runs `srand(7)` through the interpreter" on thread A would not work. Every public run entry goes through
  `on_deep_stack`, which spawns a NEW thread, and a thread-local set on A is invisible there. The test calls
  `set_rand_seed(7)` and `next_rand_u64()` directly on A. `kernel_tests` is a child module of `interp`
  (`interp.rs:7334`), so the private fns are reachable. The draw is then mapped through the same `random_i64(0, 1000)`
  reduction, or compared to the raw xorshift value of seed 7. "Today 652" cannot be observed literally, because the
  override does not exist today. The fail-first evidence is the mutation: drop the branch → B's draw lands on the
  shared state → A reads 652.
- (2) The "20/20 under load" run is DROPPED as evidence. The measured base rate is 1 failure in 64 runs, so 20 clean
  runs happen (63/64)^20 ≈ 73% of the time with NO fix. The deterministic test plus its mutation is the acceptance.]

## Gates and budgets

- G-SHELL: `bash scripts/test_harness_skip.sh`. Under 1 s, no build. Every edit to `harness_skip.sh`.
  [REVISED: dev-loop. Measured 0.036 s wall on 43fa1857. Resource: one core, no lock. Re-run on any edit to
  `scripts/lib/harness_skip.sh`, `test_harness_skip.sh`, or any `scripts/*.sh` (the shape rows). Nothing queues
  behind it. After H1's canon it also runs in `gate.sh --fast`.]
- G-LIB (H7): `cargo test -p axon-core --lib --no-default-features`. Wall time about 5 s of tests plus a lib
  rebuild (minutes; competes with any lane's cargo for CPU).
  [REVISED: dev-loop. Measured: 4.47 s of tests for 1058 lib tests (gc-gate-fast.log:1372). The rebuild is the cost:
  budget 5 min cold, 1 min warm [INFERENCE], on CPU plus the lane's target-dir lock. Re-run on each `interp.rs` /
  `kernel_tests.rs` edit, and once for the H7 mutation (budget 2 min). The same binary is built by `--fast`
  (`gate.sh:374`), so G-LIB and `--fast` share one build when run back to back.]
- G-CLI (H5): `cargo test -p axon-core --test cli_run harness_skip`. `cli_run.rs` is 39,963 lines, so the
  compile is the cost (several minutes, [INFERENCE] from size). Run once after H5 lands, not per edit.
  [REVISED: was filter `harness_skip` with default features → `cargo test -p axon-core --no-default-features --test
  cli_run skip_rule_table`, reason: the old filter selects 0 tests after H5 (vacuous green), and default features
  build a second, codegen-linked cli_run binary nobody else in the lane uses. The `--no-default-features` binary is
  the one `--fast` builds and runs. Cost: running the full cli_run takes 470-505 s (gatefast.log, the final strict
  gate log); the filtered run takes seconds. Compile budget 10 min cold [INFERENCE], about 1 min warm. Once after H5,
  plus once for the H5 mutation and once for the window-bound mutation (warm, about 1 min each).]
- G-BIN (H3, H6): one codegen `axon` (LLVM 17) plus `axon-rt` for wasm32-wasip1, in
  `CARGO_TARGET_DIR=/home/cklaus/projects/axon-lanes/.doc-build/<lane>`. One build, reused by every H3/H6 check
  through `AXON=`/`AXON_RUN=`.
  [REVISED: dev-loop.
  - Also `axon-rt` for `wasm32-unknown-unknown` (the browser sweeps and H3 (a)), both built into the SAME dir as
    `axon`. Otherwise the binary will not find them (see H3).
  - H3 (a) needs a SECOND, deliberately mismatched dir B. B holds only the runtimes; it is cheap, about a minute
    [INFERENCE].
  - Budget: codegen `axon` 10 min cold [INFERENCE], 1-2 min warm, CPU-bound. Re-run on every `main.rs` edit (H3 only).
    Harness-only edits need no rebuild.]
- G-PAR: `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` once, at the end, from the integration worktree. Budget
  **about 22 min** wall time, the strict run observed by the gaps run. It does LLVM links and wasm builds and
  takes the `axon_wasm_parity.lock` flock, so it competes with every concurrent cargo build: run it with no lane
  building. Expect 55 passed / 2 allow-listed skipped / 0 failed (`gaps-final-strict-gate.log:11153`), or the
  same plus a stated reason for any change. The cost comments at `gate.sh:1002` ("~2 min"), `:1321` ("7m49s",
  2026-08-04) and `:1336` ("~8 min") are corrected to the measured figure in the same change.
  [REVISED: dev-loop, figures re-measured from the logs.
  - Standalone strict `parity_all`: `real 21m42.007s`, 55/2/0 of 57 (`/var/tmp/r51-tmp/gaps-parity-int.log:114-117`,
    2026-10-15 04:44, host shared with other lanes). Budget 25 min.
  - Inside the strict gate: the whole `gate.sh --strict` took `real 43m24.721s` (`gaps-final-strict-gate.log:11295`),
    and parity_all is one stage of it (:11094-11154). The three non-strict full gates of the same run took
    27m39s-30m25s (gaps-wave1/wave2/baseline logs). That bounds the strict-only stages (parity_all, the WASI gate, the
    codegen clippy and the other `--strict` stages) at about 13-16 min of the 43m24s [INFERENCE: subtraction across
    different host loads; parity_all's own in-gate time is not printed].
  - The gate.sh comments get the measured standalone figure ("21m42s strict standalone, 55/2/0 of 57, 2026-10-15")
    and "part of a 43m24s --strict gate". `:1002`'s "~2 min" is wrong by an order of magnitude.
  - What it holds: CPU (LLVM links, wasm32 cargo builds) and the `${TMPDIR:-/tmp}/axon_wasm_parity.lock` flock. All
    lanes on this host share `TMPDIR=/var/tmp/r51-tmp`, so the lock is host-wide across lanes. The lock is held per
    harness, not for the whole run (each wasm harness takes it at its top). Any lane's `cli_run` wasm-harness tests
    (e.g. `wasm_host_await_parity`, which also runs under `--no-default-features`) queue behind it for one harness at a
    time.
  - What forces a re-run: any edit to `scripts/*_parity.sh`, `scripts/lib/*`, `main.rs` (wasm link), or `crates/axon-rt`.
    One run at the end of the lane is enough. Edits after it need a re-run only of the harnesses they touch.
  - Balanced run mode (human decision): the integrator's per-wave full gate is `gate.sh`, which runs `parity_all` only
    under `--strict` (`gate.sh:1004`). So G-PAR stays a separate integrator step for this lane's wave: run once, with
    no lane building. If the integrator's wave gate is `--strict`, that run IS G-PAR and no separate run is needed.]
- `scripts/gate.sh --fast` once before merge (baseline comparison).
  [REVISED: dev-loop. This is the lane's acceptance gate under Balanced mode. It now carries G-SHELL (H1 canon) and
  `vm_parity.sh` (H4). It is dominated by `cargo test -p axon-core --no-default-features`, including cli_run's
  470-505 s run. Budget 20 min [INFERENCE from the gatefast.log stage times]. Re-run once after the last edit.]
- [REVISED: added] `cargo test -p axon-core --no-default-features --test harness_binaries` (H1's scanner): a small test
  binary, budget 2 min cold [INFERENCE]. Run once after H1. It is also inside `--fast`.

## Owned files

`scripts/lib/harness_skip.sh`, `scripts/lib/axon_bin.sh`, `scripts/test_harness_skip.sh`, `scripts/parity_all.sh`,
`scripts/{wasm_host_await,browser_compute,decimal,suspend_resume,wasm_fs,wasm,vm,wasm_aot_run,wasm_aot_stdout,
wasm_examples,wasm_browser,wasm_browser_io,wasm_browser_examples,wasm_malloc_abi}_parity.sh`,
`scripts/wasi_env_control_gate.sh`, `scripts/gate.sh` (comments only), `crates/axon-core/src/main.rs`
(`try_link_wasm`/`find_wasm_axon_rt` diagnostics only), `crates/axon-core/src/interp.rs` (cfg(test) RNG override),
`crates/axon-core/src/interp/kernel_tests.rs`, `crates/axon-core/tests/cli_run.rs` (:85-240, :33418-33464),
`CHANGELOG.md`, `tasks/opportunities.md` (strike the two bullets).
[REVISED: added `scripts/{checked_arith,recursion_guard}_parity.sh` (H4), `scripts/wasm_object_only.txt` (new, NH1-A),
`governance/notes/pci-themes/<hash8>.json` (new, one per commit under `crates/axon-core/src`). `scripts/gate.sh` is no
longer comments-only: one new fast-tier line after :384 (H1 canon). `crates/axon-core/src/interp.rs` also gets the
:7407-7408 comment fix and the `sealed_outcome_rule` switch (:9231-9235). `CHANGELOG.md` and
`governance/notes/v022-pci-delta.md` are integrator-last shared files.]

## Lane plan

[REVISED: added section, Step 1.]
One lane, HARNESS. No dependency on the other three specs, and none on this one. No `Cargo.toml`/`Cargo.lock` change.

Order:
1. H5 (cli_run tests).
2. H2 → H1 → H4: these share `harness_skip.sh`, `test_harness_skip.sh` and `parity_all.sh`, so do them in this order.
   H2 adds the reason helper and the reason column. H1 adds the `shape` section and the gate line. H4 adds a shape row
   to that section.
3. H3. Compiler first, then harnesses. It needs G-BIN.
4. H6 after H3. It uses H3's marker and the `runtime not found` rule. Census first.
5. H7 at any point (independent files).
6. G-PAR last.
Sequenced pairs: H3 → H6 (marker); H1 → H4 (the shape section); H2 → H5 (both edit the comments at
`test_harness_skip.sh:3-5` and `cli_run.rs:33422-33425`, which say the two tables differ only by the reason column).

| Item | Files (line ranges on 43fa1857) |
|---|---|
| H1 | `scripts/lib/axon_bin.sh` (new fn after :35-45); `scripts/wasm_host_await_parity.sh:57-65`; `scripts/browser_compute_parity.sh:78-85`; `scripts/test_harness_skip.sh` (new `shape` section after :41, footer :43-47); `scripts/parity_all.sh:79-83`; `scripts/gate.sh` (one line after :384) |
| H2 | `scripts/lib/harness_skip.sh:162-192` (+ new fn); `scripts/parity_all.sh:114-140`; `scripts/test_harness_skip.sh:1-41`; `crates/axon-core/tests/cli_run.rs:33418-33425` (comment) |
| H3 | `crates/axon-core/src/main.rs:3211-3369`; `scripts/lib/harness_skip.sh` (new fn); `scripts/test_harness_skip.sh` (classifier rows); `scripts/wasm_aot_run_parity.sh:44-57,126-138,155-166`; `scripts/wasm_examples_parity.sh:72-93`; `scripts/wasm_browser_parity.sh:64-84,110-118,140-145`; `scripts/wasm_browser_io_parity.sh:60-80,99-107,124-148`; `scripts/wasm_browser_examples_parity.sh:58-79`; `scripts/wasm_malloc_abi_parity.sh:115-151`; `scripts/wasi_env_control_gate.sh:33-62,111-140` |
| H4 | `scripts/{decimal:31-35,suspend_resume:41-44,browser_compute:82-84,wasm_fs:57-66,wasm:63-74,vm:130-139,checked_arith:31,recursion_guard:40}_parity.sh`; `scripts/test_harness_skip.sh` (`shape` row) |
| H5 | `crates/axon-core/tests/cli_run.rs:164-240` (delete), `:33418-33464` (comment, two rows, new test); `scripts/test_harness_skip.sh` (two rows) |
| H6 | `scripts/wasm_aot_stdout_parity.sh:76-127`; `scripts/wasm_examples_parity.sh:30-36,95-131`; `scripts/wasm_browser_examples_parity.sh:25-28,81-120`; `scripts/wasm_object_only.txt` (new) |
| H7 | `crates/axon-core/src/interp.rs:7405-7417,7519-7538,9219-9236`; `crates/axon-core/src/interp/kernel_tests.rs` (appended test) |
| all | `governance/notes/pci-themes/<hash8>.json` (new, H3 and H7 commits); `tasks/opportunities.md`; integrator-last: `CHANGELOG.md`, `governance/notes/v022-pci-delta.md`, `AXON-COMPLETENESS.json:704` row text if it names the reason column |

Cross-spec contention, checked against the other three drafts on 2026-10-17:
- `scripts/gate.sh`. This spec edits comments at :1002, :1321, :1336 and adds one line after :384. lsp-residue edits
  :16-17, :356-362 and :786-798. killgroup edits :835-842. All hunks are disjoint. The nearest pair is lsp's :356-362
  and this spec's :384, which are 20 lines apart, so git merges them cleanly. The new fast-tier line adds no `-p`
  clippy line, so the clippy-coverage grep at :862 is unaffected.
- `crates/axon-core/src/main.rs`. approval-residue AR4/AR5 edit `import_closure_of` (:8425-8445), the session probe
  (:5320-5339) and `enforce_required_imports_digest` (:981-1005). This spec edits only :3211-3369. Disjoint.
  killgroup-spec.md:26 cites `main.rs:3317` (rust-lld `Command::new`). H3 shifts that line by a few lines but adds no
  `Command::new`, so killgroup's census claim stays true; only its line number goes stale.
- `crates/axon-core/tests/cli_run.rs`. approval AR4/AR5 add tests. This spec deletes :164-240 and edits
  :33418-33464. They are disjoint unless AR4/AR5 insert next to those ranges. They should append at the end of the
  file or next to the G1 tests (:3673-4003). Both lanes pay the cli_run compile.
- `crates/axon-core/src/interp.rs`. No other spec edits it (lsp: `lsp.rs`/`lib.rs`; approval: `main.rs`/`error.rs`/
  `env_registry.rs`). Both lanes that commit under `crates/axon-core/src` (harness, lsp, approval) add their own
  pci-theme files. The integrator regenerates `v022-pci-delta.md` after each merge.
- `scripts/r22_acceptance_gate.sh` is used by approval-residue. This spec deliberately leaves its `2>/dev/null` shapes
  alone (see H4).
- `Cargo.toml`/`Cargo.lock`: untouched here.

## Needs human

- **NH1 — object-only examples in the two wasm sweeps.** Today an example that builds but does not link is a
  "frontier" result, held only by a floor with 4 (stdout) or more (examples) of slack. Option A: a checked-in list
  (`scripts/wasm_object_only.txt`, name + reason, the `parity_allowed_skips.txt` shape); an unlisted object-only
  example FAILs and a listed one that now links FAILs as a stale entry. Option B: keep the floor. A costs a list
  edit whenever an example is added that does not link on wasm. **Recommend A**: the floor is the "held only by
  its floor" defect.
  [DECIDED by human, 2026-10-17: Option A, a checked-in list. Built as specified in H6. Per review, the list is keyed
  by target triple and also covers `wasm_browser_examples_parity` and the H3 per-case browser sites.]

## Non-goals

Converting the remaining `2>/dev/null` build shapes outside the ten parity harnesses listed (H4 counts and
reports them); making the production operator RNG per-thread or per-`Interp` (vetoed, interp.rs:1798-1799);
`wasm_examples_parity` writing `.wasm` files into `examples/` (:105-112; separate item if wanted);
`try_link_wasm` finding rustup only under `$HOME/.rustup` (`RUSTUP_HOME` ignored, :3273-3274); promoting
parity_all into the default gate.
[REVISED: added, all logged to `tasks/opportunities.md`:
- `find_wasm_axon_rt`'s cwd walk can link a STALE `$ROOT/target/wasm32-*/…/libaxon_rt.a` when the compiler's own dir
  has none (link.rs:1446-1457, main.rs:3245-3261). Observed precondition: `gaps-int/target/` holds both wasm dirs.
- The unknown-unknown runtime has no `AXON_WASM_RT`-style override (main.rs:3226).
- `decimal_parity`/`suspend_resume_parity` build a cold private `axon` under `$WORK/interp-target` on every run (a
  minutes-long cost per run, not a correctness defect).]

## Open questions

- Does any lib test rely on a sealed run sharing the operator stream with the code around it? H7's override is
  set inside `sealed_outcome_rule` only; a grep of the tests that call it after `srand` settles this.
  [REVISED: RESOLVED as canon. No test can rely on it deterministically today: under parallel `cargo test` the
  surrounding stream is already shared with every other test. Every `sealed_outcome_rule` caller runs the whole suite,
  including its `srand`, inside the sealed run's own thread (:9231-9235). The override starts at 0, so it seeds as the
  global does. The build's `srand(` census (H7) is the check.]
- Is an in-process concurrent `Interp` reachable in production (axon-web, dashboard)? If it is, the same race is
  a product bug, not only a test one. [INFERENCE] not checked; out of scope here, log it if found.
  [REVISED: RESOLVED by trace (see H7): not reachable in this tree. No crate besides axon-core and axon-wasm (wasm32)
  calls the interpreter. axon-core's interpreter threads are joined one at a time, and `axon test` is sequential.]

## Dropped on re-measurement

None dropped. H4 is narrowed: three of its six harnesses do pass stderr through (evidence above); they are kept
for the shared FAIL shape, not for lost stderr.
[REVISED: still none dropped. Demoted: H7's "20/20 under load" acceptance (not evidence; see H7) and H5's "every
shape is a table row" claim (two shapes were not; rows added). Added: three H3 sites, one H6 sweep, two H4 files.]
