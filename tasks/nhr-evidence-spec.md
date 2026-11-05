# nhr-evidence - batch 4 Lane B: the NH-R1 §9 enabling evidence (NE-1..NE-9)

**Status: DRAFT (after Step 1 review). Build authorized by the human 2026-11-05 ("use recommendations, build batch 4"); built on
`b4/nhr`, integrated on `b4/int`. Enabling `--unattended` is still NOT authorized (§10 Q3 of `unattended-auth-spec.md`).**

**Step 1 review** (2026-11-04; `AuditNhr` NEA-1..6, `DevloopNhr` NED-1..7; each re-checked by the drafter, markers inline):

| id | finding | disposition |
|---|---|---|
| NEA-1 | S09u needs a `governance/gate-lanes.json` row (`scripts/gate_lane.py:272-273`) | accepted: NE-1, §5 |
| NEA-2 | `s14_a_run_in_a_user_namespace_refuses` prints `SKIP` and passes (`tests/unattended.rs:1465-1467`) | accepted: S09u fails on a `SKIP ` line |
| NEA-3 | the NE-2 precedent is M4403 (`v022_g01_mutations.py:13867`), not M481 (axon-fabric) | accepted |
| NEA-4 | an lstat->open swap still errors `tree()`, so no terminal record is written (`auth.rs:979-984`) | accepted: fd-valued design (O_PATH + fstat), M4819 |
| NEA-5 | `auth.rs:505`/`envelope_preview.rs:78` are the combined fs_read+fs_write call | accepted: separate fs_write loop |
| NEA-6 | M4818 hits M4074's site | accepted: kept, as a second killing test of that guard |
| NED-1 | an `unattended` cortex builds in about 15 s; probes need not wait for NE-1 | accepted: built (14 s), NE-2 end to end observed |
| NED-2 | lane tests take 49.6 s warm, not 5-15 min | accepted: §4 |
| NED-3 | the receipt digest ignores features (`scorecard.py:89-99`); waive vs regress | accepted; regress per Main's ruling (§6 D6) |
| NED-4 | host prerequisite: uid 0 and an `/etc/axon` mount point | accepted: NE-1 |
| NED-5 | the Q1 paid run is under-specified (model, cap, opt-in) | accepted: Q1 text |
| NED-6 | sequencing against the other lanes and the receipt order | accepted: §5 |
| NED-7 | `s10_e3_measure_hard_grace` is `#[ignore]`d | accepted: NE-1 acceptance |

Subject: collect the evidence `tasks/unattended-auth-spec.md` §9 (`:459-466`) asks for, and close the test gaps O-NHR-01/02/03,
WITHOUT enabling anything. `--unattended` keeps refusing by default (exit 29). **Enabling stays a separate human decision**
(§10 Q3, `unattended-auth-spec.md:5-6`), taken on the artifacts in §3's checklist. No item here grants, widens or enables anything.

Base: `b4/specs` = main 9300d9fd. Every anchor is file:line at 9300d9fd. Reference binaries:
`/home/cklaus/projects/axon-lanes/.doc-build/b4-draft/debug/{axon,cortex,axon-intent}` (built from 9300d9fd). That `cortex` was
built WITHOUT the `unattended` feature: `cortex repair --unattended` prints `unauthorized: this cortex was built without unattended
support (feature 'unattended')` and exits 29. `[REVISED: NED-1]` A pre-fix cortex WITH `unattended` builds in about 15 s
(`CARGO_TARGET_DIR=/home/cklaus/projects/axon-lanes/.doc-build/NhrEvidence cargo build --locked -p axon-cortex --features
unattended --bin cortex`: 14.1 s here). The NE-2 end-to-end probe used it. The NE-6 probes use the same build before any fix. NE-1
only gates the tests. Probe files are in `/var/tmp/r51-tmp/NhrEvidence/`. The host runs as uid 0. No model was called, and nothing
was spent.

## 0. Source entries

| item | entry | sev |
|---|---|---|
| NE-1 | new, found while drafting: no gate stage runs `--features unattended`, so §9's "S-tests and full gate green" has no gate behind its first half | new (P0) |
| NE-2 | new, found while drafting: a FIFO under a write root hangs an unattended run after its envelope is spent and before the watchdog is armed | new (P0) |
| NE-3 | O-NHR-01 (`tasks/opportunities.md:3307-3311`): the `.unrecorded` / exit-29 path has no test | medium (P1) |
| NE-4 | O-NHR-02 (`:3313-3316`): the terminal record on the E3 watchdog path has no test | medium (P1) |
| NE-5 | O-NHR-03 (`:3318-3321`): the axon-os `require_approval` half of S12 is untested | low (P2) |
| NE-6 | `unattended-auth-spec.md:3`: "stays PARTIAL until the N1-N4 fixes are verified"; O-NHR-06 (`:3352-3353`): N8 "probes 1-5 still to be recorded" | P1 |
| NE-7 | §9 bullet 3 (`unattended-auth-spec.md:465-466`), `tasks/r6-ledger-spec.md` §5 C1: a clean C1 audit over the S-test runs and a dry-run series | P1 |
| NE-8 | §9 bullet 2 (`:464`): a read-only adversarial security review, every finding dispositioned; O-NHR-06 (`:3370-3371`): no independent end-to-end probe of F2-F4 | P1 |
| NE-9 | O-NHR-05 (`:3333-3337`) and O-NHR-06 (`:3339-3371`): classify the residuals | P2 |

### Out of scope (logged, not dropped)

| what | why not here | where it goes |
|---|---|---|
| Enabling `--unattended` (opt-in files, the enabling decision record) | A separate human decision after §3's checklist is complete (§10 Q3) | human |
| `tree()` (`crates/axon-cortex/src/auth.rs:833-857`) reads each pinned file whole (`std::fs::read`, `:852`), and the main thread's terminal walk runs after `w.disarm()` (`bin/cortex.rs:1619-1621`, record at `:1882-1899`). A huge file that a check wrote under a write root makes the end of the run unbounded in time and memory, outside E3's deadline `[INFERENCE: from the code; not run]` | A bounded or streaming walk is a design change to R6's tree. NE-2 removes only the unbounded *block* | integrator: new O-NHR entry beside O-NHR-02 |
| O-NHR-04 (refusal coverage does not scan `crates/axon-intent`) | Not §9 evidence. `scripts/v022_refusal_coverage.py` belongs to Lane A | stays open |
| A non-Linux unattended build (O-NHR-06 N3 portability) | Lane B is tests and evidence. Darwin has not built since KG1 (`opportunities.md:3359-3364`) | NE-9, needs-human Q3 |

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P0 | NE-1, NE-2 | NE-1: without it, "full gate green" says nothing about the S-tests, so a §9 claim rests on an unenforced check. NE-2: a property E3 claims (a hard deadline) and R6 claims (every spent envelope has a record) both fail for a workspace with one FIFO under a write prefix |
| P1 | NE-3, NE-4, NE-6, NE-7, NE-8 | Each is a §9 artifact that does not exist today (§3) |
| P2 | NE-5, NE-9 | NE-5: S12's axon-os half is already enforced by two layers, but only unit-tested. NE-9: classification and confirmation, no code |

## 2. Items

### NE-1 (P0) The gate runs the NH-R1 S-tests

**Anchors.** The `unattended` feature is `["dep:axon-os", "ai"]` (`crates/axon-cortex/Cargo.toml:36`). The only cortex feature stage
is S09: `cargo clippy/test -p axon-cortex --features ai` (`scripts/gate.sh:930-937`). A search for `unattended` in `scripts/gate.sh` and
`.github/workflows/` finds nothing. Today the S-tests run only inside g01 rows (`_NHT = '--features unattended --test unattended'`,
`v022_g01_mutations.py:13137`) and in lane runs.

**Reproduction.** Read, not run: grep `unattended|all-features` over `scripts/gate.sh` and `.github/workflows/` gives 0 hits.
CiHygiene confirmed that its lane adds no such stage (CI has no root and no `/etc/axon`).

**Change** (integrator-owned `scripts/gate.sh` and `governance/gate-lanes.json`). Add stage S09u "cortex unattended lint + S-tests"
after S09, in the `lane_runs` pattern (`gate.sh:930-937`):
`cargo clippy --locked -p axon-cortex --all-targets --features unattended -- -D warnings` and `cargo test --locked -p axon-cortex
--features unattended` (lib `cas::`/`auth::` tests, `--test unattended`, `--test watchdog`, `--test cli` incl. `n3_*`,
`--test envelope_preview`). `[REVISED: NEA-1]` Add the registry row after S09's (`governance/gate-lanes.json:47-48`):
`{"id": "S09u", "banner": "cortex unattended lint + S-tests", "class": "selectable", "split": "per-target", "split_cmd": "cargo
test --locked -p axon-cortex --features unattended", "lint_unit": true}`. `python3 scripts/gate_lane.py check` stays clean.
`[REVISED: NED-4]` Host prerequisite: uid 0 and an existing `/etc/axon` directory. It is a mount point only: the tests mount a
private tmpfs over it inside `unshare -m` and never write the host's (`tests/unattended.rs:312-327`). This host has both
(`/etc/axon` root 0755, empty). CI never runs `gate.sh`, so this costs nothing there. When either is missing, the stage FAILS with
that reason. It never skips green. `[REVISED: NEA-2]` The stage also FAILS if any test-output line starts with `SKIP `, because
`s14_a_run_in_a_user_namespace_refuses` returns early on such a line when `unshare --user` fails (`tests/unattended.rs:1462-1467`).
So S09u is green only when the N7 S-test really ran. The stage runs `cargo test` with `-- --nocapture`, so the `println!` reaches
the log.

**Acceptance.** A strict gate log shows S09u with every `s*_`/`r6_`/`n3_`/`a3_` test passed and no `SKIP ` line. A gate run as
non-root fails S09u by name. `[REVISED: NED-7]` The one `#[ignore]`d test, `watchdog.rs:332` `s10_e3_measure_hard_grace`, is a
measurement (`CORTEX_E3_SAMPLES`), not an S-test, and stays ignored.

### NE-2 (P0) A FIFO under a write root hangs an unattended run after spend

**Anchors.**
- Phase A walks every write prefix and fs pin for symlinks ONLY: `unattended.rs:208-233` `no_symlink_at_or_under` refuses
  `is_symlink()` and descends dirs. Any other type passes, and a root that is not a dir returns `Ok` (`:213-216`). Called from
  `auth.rs:500-505` (phase A) and `envelope_preview.rs:74-78` (mint preview).
- Phase B consumes the envelope as its last step (`auth.rs:632-633`). Then `record_started` (`bin/cortex.rs:971`) runs
  `tree(workspace, roots)` over the write prefixes plus the fs_write pins (`auth.rs:887-893`, `:902`).
- `tree()` values every entry that is neither a symlink nor a dir by `std::fs::read(&p)` (`auth.rs:852`). On a FIFO with no writer,
  `open(O_RDONLY)` blocks.
- The watchdog is armed only after phase B and `record_started` (`bin/cortex.rs:978-1025`). Nothing bounds the block.

**Reproduction.**
- Observed, the primitive (`/var/tmp/r51-tmp/NhrEvidence/fifo`): `mkfifo ws/src/pipe`, then a read the way `std::fs::read` does it
  (open O_RDONLY, read). Output: `blocked 2.0s on open(O_RDONLY) of a FIFO (what std::fs::read does)` (cut by SIGALRM).
- `[REVISED: NED-1]` Observed end to end, pre-fix (`/var/tmp/r51-tmp/NhrEvidence/e2e/probe.sh`, the unattended cortex built at
  9300d9fd, $0 `ai:m` against `https://127.0.0.1:9`). The workspace holds `broken.ax` and `out/pipe` (a FIFO), and the envelope's
  write prefixes are `broken.ax` and `out`. First, the reference `axon-intent authorize-repair` minted the envelope (`mint rc=0`), so
  the preview admits the FIFO too. Then the run went through `timeout -s KILL 60 unshare -m` with a private tmpfs `/etc/axon`
  holding a 0600 opt-in. Output:
  ```
  cortex rc=124 after 60s          (killed by timeout; no stdout, no stderr)
  spent store: e1f4…821.used  run-e761…cf10.lock  store-id
  ledger bytes: 0
  broken.ax unchanged: yes
  FINDING consumed-no-record envelope e1f4740b6171beef5b8e2d3293257821: a consumed envelope with no run record (truncation)
  ```
  So the envelope is spent, no started record was written, and the run never exits by itself: E3 never armed. C1 reports
  "truncation", the tamper reading, for what is a hang. (A first try with a 0644 opt-in refused at phase A, exit 29, nothing spent.)

**Fix** (`crates/axon-cortex/src/{unattended.rs,auth.rs,envelope_preview.rs,cas.rs}`; the new code is reached only under `feature = "unattended"`):
1. Refuse before consume. A new `un::only_plain_entries_at_or_under(workspace, rel)` refuses a root, or any entry under it, that is
   not a regular file, a dir or (already refused) a symlink: `E1s: \`<path>\` is a <fifo|socket|char device|block device>, not a
   regular file`. Called for the write prefixes inside the existing loop (`auth.rs:501-503`; `envelope_preview.rs:75-77`).
   `[REVISED: NEA-5]` `:505` and `envelope_preview.rs:78` are the combined `no_symlink_in_fs_pins(fs_read, fs_write)` call, so the
   fs_write pins get a separate loop after each: `for p in &e.scopes.fs_write { un::only_plain_entries_at_or_under(Path::new("/"),
   <p without its leading '/'>)? }` (`&r.pins.fs_write` in the preview). The minter then refuses what phase A refuses. fs_read pins
   are NOT touched: `tree()` never walks them, so no new refusal is added there.
2. Never block, and never error, in the walk. `[REVISED: NEA-4]` `tree()` (`auth.rs:833-857`) values every entry that is neither a
   dir nor a symlink (per its `symlink_metadata`, `:839`) through a new `cas::read_plain(path) -> io::Result<Plain>`, with
   `Plain::{Bytes(Vec<u8>), Kind(&'static str)}`. It opens `O_PATH|O_NOFOLLOW|O_CLOEXEC` (no driver `open`, never blocks) and
   `fstat`s that fd. For a regular file it reopens `/proc/self/fd/<n>` `O_RDONLY|O_NONBLOCK|O_CLOEXEC`: the same inode, no
   second path lookup. Any other type gives `Kind("fifo"|"socket"|"char device"|"block device"|"symlink")`, valued
   `sha256("type:<kind>")` the way `link:` values a symlink (`:842-845`). So a swap between `tree()`'s lstat and its open is valued,
   not an error, and the terminal record is still written. `cas::read_regular` (`cas.rs:338-355`, the F8 guard, g01 M4403
   `v022_g01_mutations.py:13867`) is unchanged `[REVISED: NEA-3]`. `/proc` is already a phase-A dependency (N7 reads
   `/proc/self/uid_map`), so this adds no new platform assumption. This covers entries that a check creates during the run, which
   phase A never saw (the terminal walk).

**Fail-first tests.**
- `tests/unattended.rs` `s6_a_special_file_under_a_write_root_is_refused`, copying `s6_a_pin_reaching_the_ledger_or_store_through_a_symlink_is_refused`
  (`:1249-1304`). Case a: `mkfifo scratch/pipe` under the fs_write pin. Case b: a write prefix that is itself a FIFO in the
  workspace. Each run goes through a wall limit (new `run_unshared` option: `timeout -s KILL 60` before the cortex argv) and
  `refused_over(.., "not a regular file", "ATTACK: NH-R1 a FIFO under a write root spent the envelope", ..)` (`:400-417`).
  Pre-fix: the run is killed at 60 s (the e2e probe saw `timeout`'s exit 124) and the envelope is `.used`, so the test fails.
- `auth::tests::the_pinned_tree_never_blocks_on_a_special_file`: `tree()` over a dir that holds a FIFO, run on a thread and joined
  with a 5 s limit. It asserts the FIFO's value `sha256("type:fifo")` (ATTACK: the pinned tree blocked on a FIFO).
- `[REVISED: NEA-4]` `cas::tests::a_special_file_read_is_valued_by_its_kind`: `read_plain` on a FIFO and on `/dev/null` (the
  swap's end state, reached without the race) returns `Kind`, not `Err`, and a regular file returns its bytes (ATTACK: a special
  file in a pinned tree failed the walk).

**Acceptance.** The three tests pass. Attended runs are byte-identical (S11 `cli::s11_attended_json_is_byte_identical_to_the_pre_nh_r1_build`).
No tracked fixture holds a special file, so no current S-test changes verdict.

### NE-3 (P1) The `.unrecorded` / exit-29 path end to end (O-NHR-01)

**Anchors.** `auth.rs:910-931`: a failed started append calls `rl::mark_unrecorded` (`axon-os/src/run_ledger.rs:721-735`, a `<id>.unrecorded`
record) and returns the reason, and `bin/cortex.rs:971` exits 29 through `unauthorized` (`:278-281`). The library half is tested
(`run_ledger/tests.rs` `a_failed_started_append_writes_nothing_and_the_spent_store_names_it`). The binary half is not, because
"the lane tests run as root, so a permission-based append failure cannot be induced" (`opportunities.md:3309-3310`).

**Reproduction.** Root CAN be made to fail the append, with no source hook: ENOSPC on a full tmpfs. Observed
(`/var/tmp/r51-tmp/NhrEvidence/enospc`, in `unshare -m`: a 64 KiB tmpfs, a 0700 `ledger/` dir with `ledger-id` and an empty
`ledger.jsonl` bind-mounted over the ledger path, then a filler `cat /dev/zero > lfs/fill`):
```
tmpfs  64  64  0 100% .../enospc/lfs
append failed: [Errno 28] No space left on device
size 0 read ok: id
```
Reads still work and the append writes 0 bytes. The C1 side was observed with the reference `axon-intent` on hand-made records
(`/var/tmp/r51-tmp/NhrEvidence/c1`): `.used` with no record gives `FINDING consumed-no-record ... (truncation)`, rc 1. Adding
`<id>.unrecorded` gives `"detail":"the started append failed (exit 29, nothing ran): probe: ledger torn"`, rc 1.

**Test** (fail-first is not possible: no defect is known; this pins a consumer-visible contract). `tests/unattended.rs`
`r6_a_failed_started_append_marks_the_envelope_unrecorded`. `run_unshared` gains an optional `pre` shell snippet (run after the
`/etc/axon` mount) and stops `exec`ing, so it can record `wc -c ledger.jsonl` to `s.root/ledger-size` after cortex exits. The
snippet does what the probe above did. The checker is the `s10` wrapper pattern (`:1606-1614`), writing `ops/check-ran` on `test`.
Asserts: exit 29; stderr has `the run ledger's started record could not be written` and `No space left on device`; the store holds
`<id>.used` and `<id>.unrecorded`, whose `reason` names ENOSPC; `ledger-size` is 0; `broken.ax` is byte-identical; `ops/check-ran`
is absent (nothing ran); `rl::audit` over the outer ledger and store gives exactly `consumed-no-record`, with detail `started append
failed`. Plausible bug it catches: a refactor that keeps going after the failed append, or exits without the marker, so C1 reports
truncation and the operator sees a tamper, not a full disk.

### NE-4 (P1) The terminal record on the E3 watchdog path (O-NHR-02)

**Anchors.** The watchdog's `fire` closure writes the terminal record under the write lock before `_exit(20)`
(`bin/cortex.rs:1009-1020`; `watchdog.rs:91-102`). The lock-free stall path writes none and says so (`bin/cortex.rs:988-992`).
`s10_a_witness_run_stalled_past_the_deadline_is_stopped_by_the_grace` (`tests/unattended.rs:1604-1644`) already stalls a spent run
to the watchdog (exit 20) and reads back no record. M4217/M4218 (`v022_g01_mutations.py:13173-13180`) cover only the main-thread
path, through `s8` (`:477-495`).

**Test** (extend `s10_a_witness_run_stalled_...`; no added wall time). After the existing asserts:
`run_records(&s)` (`:500-507`) kinds == `["Started","Terminal"]`; terminal `exit_code == Some(20)`, `outcome ==
Some("budget_exhausted")`, `writes == Some(vec![])` (the witness stall comes before any write); `pinned_tree_after ==
pinned_tree_before`; `rl::audit` is clean. Pre-change, M4815-M4817 survive, because nothing reads the ledger in s10. **Limit:** a
journal holding a restore on the E3 path is not reached here. The unattended generator is `ai:` against `https://127.0.0.1:9`
(`:335-339`), so no candidate is written before the deadline. That case stays covered at the library level (`tests/watchdog.rs:247`
`s10_e3_a_stalled_run_is_stopped_restored_and_reported_by_the_grace`).

### NE-5 (P2) S12's axon-os half: an `axon-approval/4` envelope is not a job approval (O-NHR-03)

**Anchors.** `approval.rs:285-298` refuses `/4` by name before parsing. That is M4074, killed by the unit test
`approval::tests::a_repair_envelope_is_not_a_program_approval` (`:583-602`). Every job route converges on `approval::authorize`
(`:817-856`): `axon-os run` (`cli.rs:345-372`, exit 8) and `supervisor::run` (`supervisor.rs:114`). If M4074 is applied, the
envelope still fails `TokenView` parsing as "malformed approval token", so only the reason changes and the exit code does not.

**Test.** `crates/axon-os/tests/acceptance.rs` `s12_a_repair_envelope_is_not_a_job_approval`, after
`approval_policy_and_evidence_are_independent` (`:105-246`), reusing its job builder. A valid, signed `/4` envelope (from
`axon_os::repair_envelope::MintKey`) is written as `j.approval` for a `require_approval = true` job, and with and without
`AXON_APPROVAL_KEY`. Expected: exit 8, stdout names `axon-approval/4 repair envelope is not a program approval`, and the program's
side-effect file is absent. The positive control (case 5 there) still runs. Same through `axon-os replay` of the archived job.

### NE-6 (P1) Verify N1-N4 (and re-review 2 N1/N7/N8)

| N | claim | fix anchor @9300d9fd | g01 rows (test) | last kill | probe |
|---|---|---|---|---|---|
| N1 | proxy variables in the real env refuse (exit 29, nothing spent) | `unattended.rs:54-58` (`HTTP_PROXY`..`ALL_PROXY` in the refused list) | M4520-M4522 (`s6_a_proxy_variable_is_refused`, `tests/unattended.rs:1012`) | killed at a7d4c961 (`nhrfixc-g01.json`) | P-N1 |
| N2 | the opt-in is read root-held, in no dir another uid owns or can write, not via a symlink | `auth.rs:264-334` `host_opt_in`, `:335` `open_root_held` | M4523-M4527 (`auth::tests::an_opt_in_in_a_directory_another_user_controls_refuses` `auth.rs:1113`; `s14_a_host_that_did_not_opt_in_refuses` `:1367`) | killed at a7d4c961 | P-N2 |
| N3 | EINVAL from `renameat2` is reported by name. Attended runs take the Q4 rename, unattended runs refuse | `cas.rs:367-450` (`rename2`, `Swap`, the `cfg(target_os = "linux")` `renameat2`) | M4532-M4539 (`cas::tests::a_filesystem_without_exchange_refuses_clearly` `cas.rs:1067`, `an_attended_run_without_exchange_takes_the_q4_rename` `:1131`; `n3_an_attended_repair_on_ntfs3_repairs_the_file` `tests/cli.rs:3769`) | killed at a7d4c961 / eccff7df | P-N3 (attended half run, below) |
| N4 | a pin reaching the ledger/store through a symlink refuses; pins outside the workspace are walked | `unattended.rs:317-330` `no_symlink_in_fs_pins`; `auth.rs:505`; `envelope_preview.rs:78` | M4528-M4531 (`s6_a_pin_reaching_the_ledger_or_store_through_a_symlink_is_refused` `:1249`; `a_symlink_under_an_outside_pin_produces_no_preview` `tests/envelope_preview.rs:343`; `s6_a_pin_over_the_store_or_a_key_is_refused` `:1155`) | killed at a7d4c961 / 431259f9 | P-N4 |
| 2-N1/N7/N8 | https only; initial user namespace; exclusive Q4 placement | `axon-ai/src/lib.rs:290-294`; `auth.rs:223-262`; `cas.rs:469-478` + Q4 sites | M4540-M4550 | killed at bcda772c (`nhrd-g01.json`) | P-N8 |

Each row's find string occurs exactly once at 9300d9fd (checked: `text.count(find) == 1` for M4216-M4218 and M4520-M4552). The
`last kill` results are lane runs on branch commits, not on main. **Verification = re-run them at the integrated head** (§4).

**Probes** (`[REVISED: NED-1]` with the pre-fix `unattended` cortex built per the header, then again after NE-2 at the integrated
head; $0; root; `/etc/axon` as a private tmpfs, the `run_unshared` pattern; envelopes minted with the reference `axon-intent
authorize-repair --cortex <unattended cortex>`, as in `e2e/probe.sh`):
P-N1: `HTTPS_PROXY=http://127.0.0.1:$PORT` with a local `nc -l` listener: exit 29, 0 bytes reach the listener, store still `.issued`.
P-N2: opt-in under a 0777 `/etc/axon`, and the opt-in dir as a symlink: exit 29 each, nothing spent. P-N4: an fs_write pin holding
`l -> <run ledger>`: exit 29, nothing spent. P-N3 unattended: the same envelope on an ntfs3 workspace: refused, naming
`RENAME_EXCHANGE`, file unchanged. P-N8: the five re-review-2 N8 cases, which (`[INFERENCE]`: the NhrSecurity2 report holding
"probes 1-5" is not in the repo) map to M4540, M4541, M4542/M4548, M4549, M4550, re-run as `cas::tests` plus one attended
ntfs3 repair. Transcripts go into this spec's §8 and a CHANGELOG line.

**Observed now (P-N3, attended half), reference binaries:** `unshare -m`, ntfs3 loop mount of a 16 MiB `mkfs.ntfs -F -Q` image,
`cortex repair --workspace $mnt/ws --file broken.ax --symbol double --check hidden_completion --write-prefix broken.ax --generator
"literal:\n    n * 2\n" (real newlines) --axon $B/axon`. Output: `rc=0`, `verified_done after 3 step(s) — repaired \`double\``,
`grep -c "n \* 2"` = 1, `ls -A ws` = `broken.ax` (no temp left). The stderr line `audit openrouter:... unavailable:
OPENROUTER_API_KEY not set` shows no model call was made. (A first try with literal backslash-n gave `rc=21 no_progress`. That
was my quoting, not a defect.)

**Status-line edit (Lane C or integrator):** after the §4 re-run is all-killed and P-N1..P-N8 are recorded, change
`unattended-auth-spec.md:3` "It stays PARTIAL until the N1-N4 fixes are verified" to "N1-N4 verified <date> (g01 <file>, probes
in `tasks/nhr-evidence-spec.md` §8)". It stays PARTIAL in the sense that enabling is not authorized.

### NE-7 (P1) C1 clean over the S-test runs and a dry-run series

**Anchors.** `rl::audit` (`run_ledger.rs:815-821`), the 13 findings (`:114-128`), `axon-intent audit-runs --spent-store --run-ledger
[--envelopes] [--json]` (exit 0 clean, 1 findings, 2 usage). Every S-test ledger is deleted at test end (`remove_dir_all`, e.g.
`tests/unattended.rs:496`), so "C1 clean over the S-test runs" is never observed today.

**Change.**
1. `tests/unattended.rs` gains `fn c1(s, e) -> Vec<&'static str>` (finding names from `rl::audit` over `s.ledger`/`s.store`, with
   `e`'s digest). Every test that reaches consume asserts it right after its run: `[]` for s8, s5 concurrency, s10 witness (NE-4),
   s13; `["consumed-no-record"]` for NE-3. Any other finding fails with `ATTACK: R6 C1 found <name> over an S-test run`.
2. Dry-run series, $0: a script run by the integrator (not a tracked file): 10 envelopes minted with the reference `axon-intent` over
   copies of `fixtures/broken.ax`, run with the NE-1 cortex against `AXON_AI_BASE_URL=https://127.0.0.1:9` (each ends at its first
   proposal, as in `s8`), then one `axon-intent audit-runs --json` over the shared store and ledger: expect `findings: []`, 10 runs,
   10 consumed. A paid series with accepted repairs is needs-human Q1.

**Observed now:** the reference `axon-intent audit-runs` over a fresh store and ledger gives `clean: no findings`, rc 0, and over the
hand-made `.used`/`.unrecorded` cases as quoted in NE-3.

### NE-8 (P1) The read-only adversarial security review (§9 bullet 2)

One read-only reviewer (`security-reviewer`), scope: A0-A4, F1-F12 and N1-N8 fixes, R6 L1-L3, and NE-2..NE-5 as landed, at the
integrated head. Unlike NhrSecurity2 ("code only, no probes", `opportunities.md:3354`), it MUST run end-to-end probes of F2-F4 (the
gap at `:3370-3371`) with the NE-1 binary, $0 generators only. Output: a findings table with a disposition for each (fixed / accepted
by a named decision / logged). Cost: subscription tokens, $0 marginal, wall 1-2 h `[INFERENCE]`. It runs after NE-1..NE-7 land, so
that it reviews the code §9 would enable.

### NE-9 (P2) O-NHR-05 and O-NHR-06 classified

| residual (anchor) | class | basis |
|---|---|---|
| O-NHR-05: terminal `accepted_refusal` always null (`auth.rs:971`); early-exit `outcome` = the exit kind (`bin/cortex.rs:286-301`); tree roots = write prefixes + fs_write pins (`auth.rs:887-893`) | confirm (needs-human Q2) | each is consistent with R6 §3. NE-2 relies on the third |
| E5/F3 fd writer after the exchange (`opportunities.md:3342-3343`; spec `:395-400`) | accepted, decided | §10 Q4 `[DECIDED 2026-10-19]` ("never silent" = the post-rename check) |
| F2 build: provider/base-URL/key/model env not refused (`:3344-3347`) | accept with stated limit (Q3) | the endpoint host is pinned and https is refused otherwise (2-N1, M4546/M4547); the model is pinned by the generator |
| N8 probes 1-5 not recorded (`:3352-3353`) | close now | NE-6 P-N8 |
| N3 portability: non-Linux `renameat2` (`:3358-3364`) | accept with stated limit (Q3): unattended is Linux-only | `auth.rs:241` reads `/proc/self/{uid,gid}_map`; an unreadable map refuses `[INFERENCE: on a non-Linux build phase A refuses at N7]` |
| N5 two stores, one file, concurrent (`:3365-3366`) | accept with stated limit (Q3) | the CAS reports overlap (exit 22) `[INFERENCE, as logged]`; operator rule: one spent store per host |
| N6 `renameat2` re-resolves; a surviving `setsid` child (`:3367-3369`) | accept: unreachable in v1 | needs an `exec` grant, and E11 refuses `exec` (§10 Q2, `unattended.rs:258-264`) |
| no independent end-to-end probe of F2-F4 (`:3370-3371`) | close now | NE-8 |

## 3. §9 evidence checklist

| §9 requirement (`unattended-auth-spec.md`) | artifact that proves it | exists at 9300d9fd? | produced by |
|---|---|---|---|
| All S-tests green (`:463`) | strict gate log with S09u passed | **no** (no gate stage runs them) | NE-1 |
| Full gate green (`:463`) | strict `scripts/gate.sh --strict` log at the integrated head | no (needs this batch's head) | integrator, §4 |
| N1-N4 verified (status line `:3`) | g01 `--only` M4520-M4552 all killed at head; P-N1..P-N8 transcripts | partly (lane-branch kills only) | NE-6 |
| R6 L1-L3 built (`:465`) | `r6-ledger-spec.md:3` BUILT; L2 gaps closed by tests | built; O-NHR-01/02 untested | NE-3, NE-4 |
| C1 clean over the S-test runs (`:465`) | the `c1()` asserts in S09u's log | **no** | NE-7.1 |
| C1 clean over a dry-run series (`:465-466`) | `audit-runs --json` output, 10 runs, `findings: []` | **no** | NE-7.2 (+Q1) |
| Read-only adversarial review, every finding dispositioned (`:464`) | review report + disposition table | no (NhrSecurity1/2 were pre-fix and code-only) | NE-8 |
| Known hangs/defects on the admitted path closed | NE-2 tests green | **no** | NE-2 |
| S12 both halves (`:419`) | `s8` (`:465-473`) + NE-5 test | half | NE-5 |
| The enabling decision itself | a human decision record + per-host opt-in | no, and NOT produced by this spec | human (§10 Q3) |

## 4. Gates and cost

| what | command | cost |
|---|---|---|
| lane tests | `cargo test --locked -p axon-cortex --features unattended` (root); `cargo test -p axon-os --test acceptance` | `[REVISED: NED-2]` measured by DevloopNhr: 49.6 s warm (unattended 5.8 s, cli 7.6 s, check_executor 14.4 s), +15 s rebuild after a src edit. NE-2/3/4 add ≤ 60 s worst case (pre-fix `timeout -s KILL 60`). Cold dep build not measured |
| g01 | one `--only`: M4216-M4218, M4520-M4552 (33), M4810-M4819 (NE-2..NE-5) | measured: an NH `--only` of 91 rows ran 16:40:11-16:47:38 on 2026-10-31 (`nhrfixc-g01.log` birth/mtime), ≈ 7.5 min `[INFERENCE: comparable]`; AuditNhr ran 4 N rows in 73.7 s at 9300d9fd |
| probes | NE-6 P-N1..P-N8, NE-7.2 dry run | $0, `[INFERENCE: < 15 min]` |
| security review | NE-8 | $0 marginal, 1-2 h `[INFERENCE]` |
| cortex receipt regress | `scorecard.py regress --cap-usd 0.50`, then `receipt --check` (`gate.sh:639-641`) | `[REVISED: NED-3]` **≈ $0.13, once.** The receipt digest covers every `crates/axon-cortex/src/*.rs` and `src/bin/*.rs` except `ai.rs`/`label.rs`, whatever their features (`scorecard.py:89-99`). NE-2 edits four digest files: `unattended.rs`, `cas.rs`, `auth.rs`, `envelope_preview.rs`. The last receipt cost $0.132553 (2026-10-31, haiku-5.5). Regress, not `waive` (§6 D6). NE-1/3/4/5/7 change no src |
| strict gate | `scripts/gate.sh --strict` once at the integrated head, with S09u | ≈ 44 min (2663 s, batch 3) + S09u `[REVISED: NED-2]` ≈ 1-2 min (clippy + 50 s tests) |
| paid model calls | the receipt regress; Q1 if answered yes | ≈ $0.13 + Q1 (≈ $0.005 expected, $0.10 external cap) |

## 5. Sequencing and lanes

| order | owner | items | files |
|---|---|---|---|
| 1 | Lane B | NE-2, NE-3, NE-4, NE-7.1 | `crates/axon-cortex/src/{unattended.rs,auth.rs,envelope_preview.rs,cas.rs}` (NE-2 only: the e2e probe proves the defect); `crates/axon-cortex/tests/unattended.rs` |
| 1 | Lane B | NE-5 | `crates/axon-os/tests/acceptance.rs` |
| 2 | integrator | NE-1 stage, g01 rows/markers, CHANGELOG, refusal-coverage anchors for NE-2's new `E1s: ... not a regular file` string (`v022_refusal_coverage.py` is Lane A's: append after Lane A merges) | `scripts/gate.sh`, `governance/gate-lanes.json` `[REVISED: NEA-1]`, `scripts/v022_g01_mutations.py`, `scripts/v022_attack_markers.py`, `CHANGELOG.md`, `tasks/opportunities.md` (stamps: O-NHR-01/02/03 closed by NE-3/4/5, O-NHR-06 classified, new O-entry from §0) |
| 3 | integrator | `[REVISED: NED-6]` after Containment, ci-hygiene CH-4/CH-8 and NE-2 have all merged: one receipt regress, the g01 `--only`, the NE-6 probes, NE-7.2, then one strict gate. The receipt and P-N3 run `--axon`, so they come after the axon changes, and the evidence is taken on the shipped axon | - |
| 4 | reviewer | NE-8 | read-only |
| 5 | Lane C / integrator | `unattended-auth-spec.md:3` status text (NE-6) | spec status line |

`[REVISED: NED-6]` No file overlaps another batch-4 lane: Lane B touches no `crates/axon-core` file and no workflow. S09u is the only
`gate.sh` edit in the batch. M4810-M4829 are reserved for Lane B (ci-hygiene uses M4830+). The integrator confirms Containment's
ids (block M4790-M4809) before appending after M4786.

Rows go after the M4786 row (`v022_g01_mutations.py:15102-15105`; the list closes at `:15106`). Markers go after `'M4786'`
(`v022_attack_markers.py:4179`).

### g01 rows (M4810-M4829; M4810-M4819 used, M4820-M4829 unused)

Find strings for NE-2 are post-fix code `[INFERENCE: exact text fixed at build]`. Each has an `ATTACK:` marker and its own test.

| id | item | file | mutation | crate / test args | killing test (ATTACK) |
|---|---|---|---|---|---|
| M4810 | NE-2 | `unattended.rs` | `only_plain_entries_at_or_under` admits every non-dir entry | axon-cortex `--features unattended --test unattended` | `s6_a_special_file_under_a_write_root_is_refused` (a FIFO under a write root spent the envelope) |
| M4811 | NE-2 | unattended.rs | the root itself is not type-checked | same | same, case b |
| M4812 | NE-2 | `cas.rs` | `read_plain`'s `O_PATH` open becomes `O_RDONLY` (`[REVISED: NEA-4]`) | axon-cortex `--features unattended --lib` | `auth::tests::the_pinned_tree_never_blocks_on_a_special_file` (the pinned tree blocked on a FIFO) |
| M4813 | NE-3 | auth.rs | `match rl::mark_unrecorded(&a.store, a.euid, &e.envelope_id, &why, now_unix()) {` -> `match Ok::<(), String>(()) {` | `--features unattended --test unattended` | `r6_a_failed_started_append_marks_the_envelope_unrecorded` (a failed started append left no .unrecorded marker) |
| M4814 | NE-3 | `bin/cortex.rs` | `:971` M4216's find line, `axon_cortex::auth::record_started(a, &b).unwrap_or_else(\|why\| unauthorized(&why));` -> `axon_cortex::auth::record_started(a, &b).unwrap_or_else(\|why\| eprintln!("{why}"));` | same | same (a run went on after its started append failed) |
| M4815 | NE-4 | bin/cortex.rs | `exit_code: axon_cortex::watchdog::EXIT_DEADLINE,` -> `exit_code: 0,` | same | `s10_a_witness_run_stalled_...` (the watchdog's terminal record names another exit code) |
| M4816 | NE-4 | bin/cortex.rs | `outcome: "budget_exhausted".into(),` -> `outcome: "done".into(),` | same | same (the watchdog's terminal record names another outcome) |
| M4817 | NE-4 | bin/cortex.rs | `:1011-1013`, three lines (`record_terminal(` occurs 4 times; with `Ending {` and `exit_code: ...EXIT_DEADLINE,` it occurs once): `axon_cortex::auth::record_terminal(` -> `let _ = (`, so the call becomes a tuple | same | same (the watchdog path wrote no terminal record) |
| M4818 | NE-5 | `axon-os/src/approval.rs` | M4074's site: `.is_ok_and(\|s\| s.schema == crate::repair_envelope::ENVELOPE_SCHEMA)` -> `.is_ok_and(\|s\| s.schema.is_empty())` | axon-os `--test acceptance` | `s12_a_repair_envelope_is_not_a_job_approval` (a repair envelope was not refused by name on the job path). `[REVISED: NEA-6]` A second killing test for M4074's guard, on the job path. The two rows' replacements differ, so both stay |
| M4819 | NE-2 | `cas.rs` | `[REVISED: NEA-4]` `read_plain` returns `Err` for a non-regular fd instead of `Kind` | axon-cortex `--features unattended --lib` | `cas::tests::a_special_file_read_is_valued_by_its_kind` (a special file in a pinned tree failed the walk) |

The M4813-M4817 find strings each occur once at 9300d9fd (checked). `[REVISED: NEA-4]` The lstat->open swap race itself has no
row: no deterministic test reaches it. M4819 pins its end state (a special file reached by `read_plain`) instead.

## 6. Engineering decisions

1. NE-2 refuses at phase A (before consume, nothing spent) AND hardens `tree()`. Phase A sees only the pre-run tree; the
   terminal walk sees what the check made. Refusal is limited to write roots, so fs_read pins gain no new refusal.
   `--unattended` is not enabled anywhere, so no program that works today can be refused.
2. NE-3 induces the failure with ENOSPC on a private tmpfs, not with a source hook: no test-only code path in `src`.
3. NE-4 extends the existing stall test rather than adding one, so it costs no extra wall time (`opportunities.md:3315`).
4. NE-1 fails when it is not root, and fails on any `SKIP ` line `[REVISED: NEA-2]`. It never skips green.
5. `[REVISED: NEA-4]` D5: the walk values a special file by the type its fd reports (`O_PATH` + `fstat`), and never errors on one.
   An erroring walk leaves a spent envelope with no terminal record. `O_PATH` also avoids running a device driver's `open` on a
   swapped-in char device. Rejected alternative: keep `read_regular` and log the race as a residual. That keeps a known path to a
   recordless spent envelope.
6. `[REVISED: NED-3, Main ruling]` D6: the receipt is a `regress` (≈ $0.13), not a `waive`. NE-2 changes four digest files, and §9
   evidence should not rest on a waiver.
7. `[REVISED: NED-6]` D7: the receipt, the g01 re-run, the probes and the strict gate run once, after every batch-4 lane that
   touches cortex src or axon has merged.

## 7. Needs-human

| Q | question | recommendation |
|---|---|---|
| Q1 | Must §9's "dry-run series" include accepted repairs (needs a paid `ai:` model)? | Yes `[REVISED: NED-5]`: 3 runs of `ai:<policy model, haiku-5.5>` on `fixtures/broken.ax`, each inside `unshare -m` with a private tmpfs `/etc/axon` holding the opt-in (never the host's `/etc/axon`: a host opt-in would be a partial enable). Spend is capped externally: an OpenRouter key with a $0.10 credit limit, checked after the runs (no `--cap-usd` exists on the `ai:` repair path: `grep cap_usd\|max_cost` in `crates/axon-cortex/src`, `crates/axon-ai/src` finds none). Expected ≈ $0.005 (receipt `cost_per_verified` $0.0013). Then `audit-runs` over those records with NE-7.2's 10 $0 runs. Without them C1 never audits an `accepted` terminal record or the `accepted-mismatch` path |
| Q2 | Confirm O-NHR-05's three L2 choices in `r6-ledger-spec.md` §3 | Confirm all three as built. The integrator adds one `[DECIDED]` line |
| Q3 | Accept the stated limits: F2 env (host+https pinned), Linux-only, N5 one spent store per host, N6 unreachable under Q2 | Accept, with one USAGE paragraph naming the four limits before enabling |

`[DECIDED by human 2026-11-05: Q1 yes, Q2 confirm (recorded in r6-ledger-spec.md §3), Q3 accept with the USAGE paragraph.]`

## 8. Verification log (draft 9300d9fd, 2026-11-04)

Observed: the reference `cortex` refuses `--unattended` (built without the feature); NE-2 FIFO block (2.0 s, cut); NE-3 ENOSPC as
root; C1 `audit-runs` on a fresh store (clean, rc 0) and on hand-made `.used` (consumed-no-record "truncation", rc 1) and
`.unrecorded` ("started append failed", rc 1); P-N3 attended ntfs3 repair (rc 0, repaired, no temp); find-string uniqueness for
M4216-M4218, M4520-M4552 and M4813-M4817; prior kill results in `/var/tmp/r51-tmp/{nhrfixc-g01,nhrfixc-g01b,nhrfixc-g01c,nhrd-g01,nhre-g01}.json`;
the gate has no `unattended` stage (grep).

Step 1 review, re-checked by the drafter 2026-11-04:
- Re-read: `gate_lane.py:272-273` and `gate-lanes.json:47-48` (NEA-1); `tests/unattended.rs:1462-1467` (NEA-2); `v022_g01_mutations.py:13867-13869`
  and `:1451` (NEA-3); `auth.rs:979-984` (NEA-4); `auth.rs:501-505`, `envelope_preview.rs:75-78` (NEA-5); `:13242-13245` (NEA-6);
  `scorecard.py:89-99` (NED-3).
- Run: a cortex built with `unattended` at 9300d9fd in 14.1 s (own target dir); the NE-2 pre-fix end-to-end probe (`e2e/probe.sh`:
  mint rc 0, run killed at 60 s, envelope `.used`, ledger 0 bytes, C1 consumed-no-record), and no cortex process left afterwards.
- Taken from DevloopNhr's run, not re-run: the 49.6 s lane-test time (NED-2). From AuditNhr: `--only=M4520,M4523,M4528,M4532` 4/4
  killed in 73.7 s at 9300d9fd.

Not observed: post-fix behaviour, the NE-3/NE-4 tests, the NE-6 unattended probes, and everything marked `[INFERENCE]`. No model
call, nothing spent.
