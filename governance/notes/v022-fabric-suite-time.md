# Where the axon-fabric suite's time goes, and how it got faster (amendment 69)

Workstream fabricfast (C9 round 4b). The axon-fabric full suite is the dominant
cost of every paired-disable record: the harness runs it as a CONSUMER suite
(`cargo test -q -p axon-fabric -- --test-threads=1 --show-output`,
`CONSUMER_FLAGS` in `scripts/v022_paired_disable.py`) for every row whose owner
crate it consumes, plus once per clean-tree baseline. This note is the
measurement, the analysis, and what was changed. No test was weakened: no bound
that is the property under test was shortened, and every remaining bound only
FAILS a test.

## 1. Measurement (before: 16500980)

Host: 31 cores, 23 GB, WSL2, shared with the other C9 workstreams. Every run
below was taken holding one of the two heavy-job slots (`/var/tmp/c9-heavy.sh`),
so at most one other heavy job ran beside it; the load average is recorded per
run. Each target ran through `cargo test` itself (cargo's own runtime env and
cwd), serially, exactly as the harness runs the suite.

| run | tree | how | wall | load (start/end) |
|---|---|---|---|---|
| m1 | 16500980 | serial, `-q` | **1472 s** | 4.6 / 6.3 |
| m2 | 16500980 | serial, per-test times (`--report-time`) | **1440 s** | 2.8 / 6.3 |

The task statement's "~466 s for psv_dispatch, ~21 min whole suite idle" is
reproduced: psv_dispatch 495-529 s, the whole suite 24-25 min. The harness's
own history has the loaded numbers: psv_dispatch 925-1319 s and the suite past
2400 s under the 6-shard paired-disable (amendment 66).

### Per binary (m2, seconds; 645 tests, 0 skipped, 0 failed)

| binary | tests | s | mean s/test |
|---|---|---|---|
| psv_dispatch | 48 | 494.6 | 10.3 |
| privileged_launcher | 64 | 214.9 | 3.4 |
| readiness | 40 | 202.5 | 5.1 |
| readiness_attribution | 40 | 182.1 | 4.5 |
| readiness_launch | 16 | 96.7 | 6.0 |
| grant_registry_authority | 15 | 67.9 | 4.5 |
| one_read | 12 | 47.9 | 4.0 |
| guest_provenance | 29 | 36.1 | 1.2 |
| cortex_via_fabric | 4 | 15.2 | 3.8 |
| trust_root_ownership | | 14.5 | |
| check_effects | 24 | 9.2 | |
| submit | 34 | 8.3 | |
| qualification | 41 | 7.0 | |
| the other 24 targets (lib, bins, doc, 18 test files) | | < 6 each, 33 s together | |

### Slowest tests (m2)

| test | s | what the time is |
|---|---|---|
| psv_dispatch `every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason` | 39.6 | real work: one full guest-path submission per forgery (a loop) |
| psv_dispatch `every_defective_observation_refuses_the_launch` | 38.9 | real work: one observed launch per defect (a loop) |
| psv_dispatch `every_defective_observation_launches_nothing_on_the_direct_route` | 25.6 | as above, direct route |
| readiness_launch `the_record_suite_is_the_suite_the_launch_ran` | 24.1 | real work: certified fixture + launches |
| psv_dispatch `an_observed_verdict_that_is_not_protected_carries_no_bundle` | 23.4 | real work |
| psv_dispatch `an_observer_key_that_holds_another_role_is_refused_at_every_observation` | 22.1 | real work (a loop) |
| psv_dispatch `a_guest_under_a_policy_the_manifest_does_not_name_yields_no_verdict` | 22.1 | real work |
| psv_dispatch `an_inadmissible_observed_launch_is_never_protected_and_carries_no_bundle` | 19.3 | real work |
| readiness `a_production_verifier_built_from_a_dirty_tree_certifies_nothing` | 16.9 | two PRODUCTION builds of axon-fabric (incremental) + namespace runs |
| psv_dispatch `a_launcher_that_changed_during_the_launch_yields_no_verdict` | 14.2 | real work |
| readiness_launch `the_certified_observation_is_of_the_runs_launch` | 14.1 | real work |
| readiness `any_change_to_source_scripts_or_manifests_invalidates_it` | 14.1 | real work (a loop of edits) |
| the next 20 (12-13 s each, psv_dispatch helper-route verdict tests) | | real work: one helper-route launch each |
| one_read `one_components_signature_does_not_certify_another_through_a_fifo` | 9.0 | **5.3 s of it a fixed wait** (FIFO feeder, §3) |
| one_read `a_record_served_twice_by_a_fifo_certifies_nothing` | 9.0 | **5.3 s fixed wait** |
| one_read `a_preflight_served_twice_by_a_fifo_certifies_nothing` | 7.7 | **5.3 s fixed wait** |

No test runs longer than 40 s; no test pays a timeout whose expiry is its
expected outcome. The 180 s socket-activation and client bounds of
`privileged_launcher.rs` (`ACTIVATION_POLLS`, `CLIENT_TIMEOUT_S`) are never
paid on a healthy run: the socket is polled every 50 ms and appears at once, a
refusing custodian closes the connection, and the slowest privileged_launcher
test is 10.9 s.

### What "real work" is: SHA-256 at opt-level 0

`strace -f -tt -e trace=execve,openat` of one 12.8 s psv_dispatch test
(`a_failing_test_is_failed_whatever_the_guest_claims`) shows ~30 process
spawns (git, cp, the custodian, the observer's python3 + `axon-fabric
sign-evidence`, the launcher stand-in, the interpreter) taking well under 2 s
together, and gaps of 0.4-2.5 s that follow each open of a large debug binary
(axon 80 MB, axon-fabric 43 MB, axon-protected-launcher 35 MB, axon-custodian
33 MB). Those are digests: the helper re-verifies every pinned artifact, the
custodian client verifies the custodian program on every reply (amendment
65), the fixtures pin every binary they name. Measured on this host, the `sha2`
crate at the dev profile's opt-level 0 hashes the 80 MB interpreter in
**1.03 s**; the same crate at opt-level 3 in **0.033 s** (same digest). The
debug builds of Fabric, the helper and the tests all hash at opt-level 0.

## 2. Why `--test-threads=1`, and what is shared ACROSS processes

`--test-threads=1` is needed INSIDE a test process: tests there share
process-global state that libtest's threads would race on.

| in-process shared state | where |
|---|---|
| the process environment (`set_var`/`remove_var` while other threads spawn children that inherit it) | `psv_dispatch.rs` (`PSV_VERIFY_ENV_PROBE`), `readiness_git_env.rs` (the `GIT_*` set), `src/privileged_launcher.rs` unit test |
| fork-inherited write descriptors: a file written in-process and exec'd while a sibling thread forks fails ETXTBSY | `std::fs::copy` + exec sites (`readiness.rs`); `tests/common/exec.rs` writes through a child `cp` for this reason |
| per-process statics (`OnceLock` production builds, `workspace_bin`'s cache) | harmless serially |

None of these crosses a process boundary, so running DIFFERENT processes at
the same time keeps every one of them as it is. What two concurrent test
processes of this package could share, and why each is not shared state:

| candidate | finding |
|---|---|
| fixed uids 4242/4243/4244 (Fabric, other, custodian) | only identities for `setpriv`; no test lists, signals or counts processes by uid (the only `pkill -f` patterns name the test's own tempdir), and every file they own is under a per-test tempdir |
| `/etc/axon` | the host's is an empty mount point that no test writes; every test that needs it mounts a tmpfs there inside `unshare -m --propagation private`, a namespace of its own |
| `/usr/bin/git` replaced (readiness) | a bind mount inside the same kind of private namespace |
| `<target>/debug/../production-build` (privileged_launcher's production helper/Fabric/custodian) | built by cargo, which locks the target dir; the binaries change only when sources change, never during a suite |
| `<target>/workspace-bins` (`script_spawn::workspace_bin`, every test that execs the interpreter or cortex without `AXON_BIN`/`CORTEX_BIN`) | **SHARED** (missed by the first audit; found by the first paired-disable cell to run sharded, M482's own-package baseline: `common/mod.rs:54` canonicalize, "No such file or directory"). cargo REPLACES an uplifted binary whenever it relinks it, and in a git WORKTREE it relinks the interpreter on EVERY build: axon-core's `build.rs` watches `../../.git/HEAD` and `../../.git/index`, which do not exist where `.git` is a file, and a missing rerun-if path reruns the script (measured: three no-op `cargo build -p axon-core --bin axon` gave three inodes; its comment says such a path is "simply ignored", which is wrong). One process's build removed the `debug/axon` another had just been handed. The race predates sharding wherever two harness cells share a target dir. **Fixed at the one primitive every such access goes through**: `workspace_bin` builds under an exclusive lock on the target dir and hands each process its OWN copy (`<target>/per-process/<pid>/<name>`, copied by a child `cp`; exited processes' copies removed under the lock), which no later build touches. The build script itself is unchanged (it is production code with its own evidence); a worktree still rebuilds the interpreter once per test process, ~1.5 s serialized under the lock |
| `<target>/debug/rows2-production-verifier` (readiness production verifiers) | **SHARED**: a fixed path whose `src` is deleted and re-copied, then built, then copied to fixed names. Two processes running this setup at once delete each other's sources mid-build. **Fixed**: an exclusive `flock` on `<base>/.lock` held for the whole setup, and the verifier copies are per process (`clean-axon-fabric-<pid>`, removed once that pid is gone). Only one test uses it, so a single sharded suite never runs it twice; the lock also covers two concurrent suite runs on one target dir (harness shards), which were already exposed |
| `crates/axon-fabric/pci_expected_fixture.txt` (check_effects) | written into the package cwd only when the guard under test is BROKEN, and only that test looks for it |
| TMPDIR, `$HOME/.cache` | every fixture is a `tempfile` dir with a unique name; `exec.rs` staging names carry pid + counter |
| ports | none: every socket is a unix socket under a per-test tempdir |
| the interpreter's provenance log | Fabric gives every job its own `HOME`/`XDG_CACHE_HOME` (`workspace.rs`), and no fabric test reads the log |
| the test binaries and `AXON_BIN` | only read and exec'd |

So the suite can run its test PROCESSES concurrently. `cargo test` cannot do
that (it runs binaries one after another), and cargo-nextest is not installed
here and would change the output format the harness parses.
`scripts/cargo_test_shards.py` does it with cargo itself (§4).

## 3. Waits (stage ii)

Each fixed wait was replaced by the positive signal the code under test (or
the fixture) emits, with a long bound kept only as an upper bound that FAILS:
`SETUP_BOUND` (600 s) in `tests/common/mod.rs`, polled by `wait_until`
(10 ms, backing off to 50 ms).

| site | old | new |
|---|---|---|
| `psv_dispatch.rs` `an_epoch_that_moves_while_the_observer_runs_refuses_the_launch` (coordinator priority: failed the CLEAN baseline at load ~100 on gpumaster, "the observer started") | poll 400 x 50 ms = 20 s for `observer-waiting`; then assert | ready on `observer-waiting`, or the submission thread FINISHING (a setup failure reported at once with its reason); SETUP_BOUND fails; `observer-go` is written on every path out (a drop guard), so a setup panic never holds the observer |
| `common/mod.rs` observer script, mode `wait` | waited 400 x 50 ms = 20 s for `observer-go`, then SIGNED AN OBSERVATION ANYWAY | waits 12000 x 50 ms (SETUP_BOUND), and an observer never told to go exits 1 (makes no observation) |
| `common/mod.rs` `try_start_custodian` | 400 x 25 ms = 10 s for the socket, then `Err("did not start listening")` -- which the refusal tests in `custodian.rs` read as the custodian REFUSING | ready when it listens, done when it EXITS (the refusal, at once, with its stderr); a custodian that does neither within SETUP_BOUND is a setup PANIC, never an `Err` |
| `one_read.rs` `fifo_serving` (3 tests) | the feeder gave up after 5 s with no reader -- paid by EVERY honest run (an honest reader opens once, so the second serve never pairs) and missed by a slow reader | the feeder serves until the test is done reading (`Feeder::join` after the decision returns; `Drop` too); SETUP_BOUND only stops a feeder whose test never finished. Saves ~5 s per test |
| `privileged_launcher.rs` `a_production_custodian_never_takes_its_config_from_a_path_its_caller_names` (`serves`) | 200 x 25 ms = 5 s; the control then read as "did not serve" | ends on the socket, or on the custodian exiting; SETUP_BOUND |
| `privileged_launcher.rs` `start_impostor` | 200 x 25 ms = 5 s | ready file, or the impostor exiting; SETUP_BOUND; setup assert kept |
| `privileged_launcher.rs` `a_custodian_executable_another_uid_can_rewrite_is_refused` | 200 x 25 ms = 5 s, NO assert: on expiry the client's connect error stood in for the pin refusal | socket, or exit; then a setup assert that it listens |
| `submit.rs` `sigkill_after_launch_reconciles_to_outcome_unknown_with_liability` (effect started) | 30 s | the start marker, or the Fabric exiting; SETUP_BOUND |
| `submit.rs` same test, the worker dying with its SIGKILLed supervisor | 10 s | 120 s. The bound is on the kernel delivering a death, not the property: an unowned worker never dies, so any bound finds it |
| `readiness.rs` production verifier's ETXTBSY retry | 50 x 100 ms = 5 s | SETUP_BOUND |
| `axon-cortex/tests/cli.rs` `cli_survives_a_generator_that_misbehaves` | ONE generator deadline (700 ms) for every case; elapsed < 30 s / < 10 s | the hang case keeps 700 ms (firing it IS the property; elapsed bound 300 s, far below the generator's 600 s sleep); the flood and silent cases, judged on their merits, get 120 s (a slow honest generator on a loaded host read as "did not answer"); the flood's no-deadlock bound is 100 s, below its own 120 s deadline so a deadlock still fails twice |
| `axon-core/tests/cli_run.rs` `r42_smoke_scenario_runs_end_to_end` | 30 s hang bound | 300 s |

Left unchanged, with the reason:

* `one_read.rs` `a_signature_fifo_with_no_writer_does_not_hang_readiness`:
  `recv_timeout(60 s)` IS the attack ("readiness hung"); it is already a
  fail-only bound far above the run's own time (~1 s).
* `privileged_launcher.rs` `ACTIVATION_POLLS`/`CLIENT_TIMEOUT_S` (180 s):
  already positive-signal polls with fail-only bounds (raised in round 4b).
* `axon-os` `r29_compliance.rs` (`< 2 s` violation detection), `r27_acceptance.rs`
  (`< 5 s` prompt return), `axon-vm` quorum `elapsed < deadline * 3`,
  `axon-intent` `synth.rs` (`< 2 s`): these bounds ARE the properties (a
  latency SLA, a deadline that must fire). They are load-sensitive; they are
  not shortened or removed here. If they must be made load-robust, it is by a
  design decision on what latency the product promises, not by a test edit.
* `journal.rs` child `sleep(120 s)`: the child waits to be killed; never paid.

## 3b. The digest cost (stage ii-b): `sha2` optimized in the dev profile

`Cargo.toml`: `[profile.dev.package.sha2] opt-level = 3`. Only that
dependency's codegen changes: the same code computes the same digest of the
same bytes (checked: the interpreter's sha256 is identical at both levels), no
workspace crate is optimized, so every guard a test or a mutation exercises is
compiled exactly as before, and the release profile (the guest image, the
operator's binaries) is untouched. Nothing in the tree inspects cargo profiles
(the guest-build checks read `.cargo/config*`, not `Cargo.toml` profiles).
The production builds the tests make themselves (privileged_launcher's
production helper, readiness's production verifiers) use the same workspace
profile and get the same speed-up.

## 4. Process-level parallelism (stage iii)

`scripts/cargo_test_shards.py`: builds with `cargo test --no-run
--message-format=json`, lists every test binary (`--list`), splits each into
shards of at most K tests, and runs each shard as `cargo test -q -p
axon-fabric <target> -- --test-threads=1 --show-output --exact <its tests>`,
up to N at once. Inside a shard nothing changes. The evidence it prints is the
evidence a serial run prints:

* every listed test runs once, by name; a shard whose libtest did not report
  `running <its count> tests` and a `test result:` is a FAILURE of the run (a
  lost test is never a pass);
* every shard's whole output is printed in cargo's target order, so
  `---- name stdout ----` (skip accounting), `name --- FAILED` and
  `test result: FAILED` read as before;
* exit status 0 only if every shard exited 0 and every count held; every
  shard runs even after one fails (a serial `cargo test` stops at the first
  failing binary, so a failing cell now names at least as much);
* on SIGTERM (the cell's `bounded_run` bound) every running shard's process
  group is killed and each unfinished shard is named as CUT.

`full_suite_ok` uses it for the packages in `SHARDED_PACKAGES` = {axon-fabric}
(the package audited in §2); every other package runs as before. The harness's
parsing is unchanged. Pinned by axon-core `harness_integrity`
`a_sharded_suite_run_reports_every_test_as_a_serial_run_does`.

## 5. Results

Same host; the "under load" runs had 32 busy loops (`sh -c 'while :; do :; done'`,
killed by PID after) on the 31 cores; load averages as recorded.

| configuration | idle | under load |
|---|---|---|
| BEFORE (16500980): serial, as the harness ran it | **1440-1472 s** (load 3-6; m1, m2) | not re-measured here: amendment 66 recorded the suite past 2400 s and psv_dispatch at 925-1319 s under the 6-shard paired-disable |
| waits (stage ii) + sharded runner (stage iii), WITHOUT the sha2 profile | 223 s (sh2; load 3 -> 9) | 376 s (sh3; load ~38) |
| AFTER: serial (stage ii + ii-b), as `cargo test -- --test-threads=1` | **218 s** (m3; load 2.8 -> 2.4; psv_dispatch 495 -> 46 s) | see §6 |
| AFTER: sharded (stage ii + ii-b + iii), as `full_suite_ok` now runs it | **38 s** (sh4; load 2.4 -> 5.8) | **61 s** (sh5; load 12 -> 33, 32 busy loops) |

The largest single factor is the digest cost (6.6x serially). Sharding then
divides what is left by the concurrency the host gives it (8 jobs; the
longest shard is ~17 s idle, guest_provenance, whose tests are git-bound).
Every run above listed and ran the same 645 tests (643 passed + 2 ignored),
0 failed, 0 skipped, except sh1 (an earlier sharded run, rc 101): its only
failures were the four cortex_via_fabric tests, whose nested `cargo build -p
axon-cortex` found a `workspace-bins` dir polluted by a mis-configured first
measurement of mine (a stable-toolchain build); the SERIAL run on the same
target failed identically, and removing that dir fixed both.

## 6. Evidence

See the commit messages of the stage commits for the exact commands and rcs
(changed tests 20/20 idle and 20/20 under load, mutation rows killed by their
own attacks, clippy/fmt/suites, harness_integrity and the refusal-coverage
gate).
