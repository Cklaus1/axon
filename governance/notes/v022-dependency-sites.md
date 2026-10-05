# v0.22 dependency refusal sites (C9 round 4c, workstream SITES, amendment 75)

Base: `c9r4c/fixes-rc` 64d9f436. After amendment 71 (the refusal gate's crate
set is the closure of the protected roots' normal workspace dependencies),
`scripts/v022_refusal_coverage.py --freeze` failed on 118 NOT_YET_SCANNED sites
in 18 files. Every site is judged here by reading its function and its
callers on the PROTECTED route (Fabric -> root helper -> launcher
`scripts/fc_linux_profile.sh` -> guest `/init` + axon-guest-init ->
axon-psv-runner -> `axon test` -> Fabric `psv::derive`; loop
intake/EVL/readiness), never by file or crate.

## Method (the facts every exemption rests on, each a grep)

* The protected crates use from **axon-cortex** only `parse_strict`,
  `ContractError`, and from `runner`: `CheckRegistry` (load, check, the
  suite id/reference functions), `LocalInterpreterExecutor` (the local, i.e.
  development, route: submit.rs builds it only when the profile is not the
  protected one, and a protected host runs nothing else, M265),
  `parse_axon_test_json`, `completion_token`, and the digest functions. The
  Cortex loop (Runner, generate, FabricSubmitExecutor) is constructed only by
  `bin/cortex.rs` and `cortex-policy-adapter`.
* From **axon-vm** only `BACKEND_PROFILE` and
  `firecracker::{MmdsPayload, embed_policy_in_cmdline}` (backend.rs); the
  protected launcher is the shell script, not axon-vm.
* From **axon-attest** only `hmac_sha256` (axon-loop store.rs); every other
  attest function is reached from axon-vm.
* **axon-audit**: no protected crate names it, but axon-core (the `axon`
  binary the guest execs) links it (preflight.rs, main.rs `set_ledger_path` /
  `flush_ledger`, interp/builtins.rs `append_global` / `append_ai_call`), so its
  functions are reachable there when `AXON_AUDIT_LEDGER` is set. The exemption
  holds for a different, checkable reason: the psv runner execs `axon test` with
  `env_clear()` (axon-psv runner.rs) and sets no `AXON_AUDIT_LEDGER`, and every
  audit failure in axon-core is only printed or discarded, never an exit code or
  verdict. (An earlier wording, "reached only via axon-os cli.rs", was textually
  wrong.)
* **axon-os**: Fabric enters through `supervise_requiring`
  (approval::authorize, its own AdmissionProbe runtime, gate::admit,
  record::build), `parse_manifest` (grants.rs, the operator's pinned grant
  files on a protected host: M277/M1098/M278), `scan_effects`, and
  `ledger::ResourceLedger::carve` (journal.rs carve_within only).

## Counts

| disposition | sites |
|---|---|
| ROW (new, M1760-M1769, all KILLED by their own attack) | 10 |
| EXEMPT: not on the protected route | 81 |
| EXEMPT: operator-authored on the protected route | 15 |
| EXEMPT: not a verdict property (the journal budget) | 3 |
| EXEMPT: named row (M84) | 1 |
| EXEMPT: nothing to admit | 1 |
| EXEMPT: usage | 1 |
| NOT A SITE: test code (module renamed to `tests`) | 6 |
| **total** | **118** |

Residual: none. `--freeze` rc 0 with NOT_YET_SCANNED empty.

The custodian usage-die (axon-custodian.rs:120) could not be exempted at the
base: the next site (M631's `check_store(..).unwrap_or_else(die)`, four lines
below) had the gate's block reaching up to the `_ =>` arm, so any anchor for
the usage arm was "exempt yet covered by M631". The store check is now its own
`if let Err(e) = .. { die(&e); }` (behaviour unchanged), M631 is re-anchored on
it and re-run (KILLED by its own attack), and the usage arm carries its own
USAGE exemption.

## Ledger

Line numbers are at the workstream head (the files' sites did not move from
the base except where a row's test file grew).

| file | line | function | disposition | row / reason |
|---|---|---|---|---|
| axon-cortex/src/runner.rs | 2466 | parse_axon_test_json | ROW | M1760: psv_dispatch an_output_with_no_summary_is_no_verdict |
| axon-cortex/src/runner.rs | 1825 | check_suite_id | ROW | M1761: check_executor a_registry_file_never_registers_a_suite_id_the_id_rule_refuses |
| axon-cortex/src/runner.rs | 1847 | parse_check_suite_ref | ROW | M1762: intake a_suite_reference_with_a_second_reading_is_never_pinned |
| axon-cortex/src/lib.rs | 321 | parse_strict (visit_map) | ROW | M1763: intake an_episode_holding_one_key_twice_is_never_recorded |
| axon-core/src/main.rs | 6204 | cmd_test (type errors) | ROW | M1764: psv_test_selection a_candidate_that_does_not_type_check_is_never_tested |
| axon-core/src/main.rs | 6398 | cmd_test (exit code) | ROW | M1765: psv_test_selection a_run_whose_test_failed_exits_nonzero |
| axon-os/src/approval.rs | 103 | verify_approval (decision) | ROW | M1766: grant_authority an_approval_token_admits_only_what_it_approved |
| axon-os/src/approval.rs | 110 | verify_approval (program) | ROW | M1767: same |
| axon-os/src/approval.rs | 114 | verify_approval (grant) | ROW | M1768: same |
| axon-os/src/approval.rs | 123 | verify_approval (token digest) | ROW | M1769: same |
| axon-attest/src/lib.rs | 101 | validate | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; DeviceSet::validate (the VM device set axon-vm admits) |
| axon-attest/src/lib.rs | 104 | validate | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; DeviceSet::validate (the VM device set axon-vm admits) |
| axon-attest/src/lib.rs | 107 | validate | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; DeviceSet::validate (the VM device set axon-vm admits) |
| axon-attest/src/lib.rs | 120 | validate_manifest_extra_device | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; DeviceSet::validate_manifest_extra_device |
| axon-attest/src/lib.rs | 234 | verify_report | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_report (axon-vm's attestation report check, also behind verify_and_admit) |
| axon-attest/src/lib.rs | 241 | verify_report | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_report (axon-vm's attestation report check, also behind verify_and_admit) |
| axon-attest/src/lib.rs | 251 | verify_report | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_report (axon-vm's attestation report check, also behind verify_and_admit) |
| axon-attest/src/lib.rs | 272 | verify_report | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_report (axon-vm's attestation report check, also behind verify_and_admit) |
| axon-attest/src/lib.rs | 288 | verify_report | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_report (axon-vm's attestation report check, also behind verify_and_admit) |
| axon-attest/src/lib.rs | 300 | verify_report | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_report (axon-vm's attestation report check, also behind verify_and_admit) |
| axon-attest/src/lib.rs | 598 | read_component_file | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; read_component_file, read only by measure_host_stack/measure_extended (axon-vm admit.rs, main.rs) |
| axon-attest/src/lib.rs | 604 | read_component_file | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; read_component_file, read only by measure_host_stack/measure_extended (axon-vm admit.rs, main.rs) |
| axon-attest/src/lib.rs | 671 | verify_extended | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs) |
| axon-attest/src/lib.rs | 674 | verify_extended | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs) |
| axon-attest/src/lib.rs | 680 | verify_extended | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs) |
| axon-attest/src/lib.rs | 688 | verify_extended | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs) |
| axon-attest/src/lib.rs | 699 | verify_extended | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs) |
| axon-attest/src/lib.rs | 704 | verify_extended | EXEMPT: NOT ON ROUTE | attest: only hmac_sha256 is called by protected crates; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs) |
| axon-audit/src/lib.rs | 117 | from_hex | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); the ledger key's hex decoding |
| axon-audit/src/lib.rs | 133 | hex_digit | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); the ledger key's hex decoding |
| axon-audit/src/lib.rs | 263 | open_keyed | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); Ledger::open_keyed |
| axon-audit/src/lib.rs | 270 | open_keyed | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); Ledger::open_keyed |
| axon-audit/src/lib.rs | 383 | verify_against_file | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); Ledger::verify_against_file |
| axon-audit/src/lib.rs | 393 | verify_against_file | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); Ledger::verify_against_file |
| axon-audit/src/lib.rs | 400 | verify_against_file | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); Ledger::verify_against_file |
| axon-audit/src/lib.rs | 513 | verify_chain_keyed | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); verify_chain_keyed |
| axon-audit/src/lib.rs | 520 | verify_chain_keyed | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); verify_chain_keyed |
| axon-audit/src/lib.rs | 536 | verify_chain_keyed | EXEMPT: NOT A VERDICT PROPERTY | audit: not a verdict property (axon-core links it, but the runner env_clear()s and sets no AXON_AUDIT_LEDGER; failures are only printed); verify_chain_keyed |
| axon-core/src/main.rs | 6186 | cmd_test (merge errors) | EXEMPT: OPERATOR-AUTHORED | the runner passes ONE file (the suite entry); the candidate is reached by `mod`, never merged; a merge error is a duplicate in the operator's suite file, pinned (M1061) |
| axon-cortex/src/generate.rs | 158 | validate | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; generate::validate, the Cortex patch generators' output check (bin/cortex.rs, ai.rs) |
| axon-cortex/src/generate.rs | 166 | validate | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; generate::validate, the Cortex patch generators' output check (bin/cortex.rs, ai.rs) |
| axon-cortex/src/generate.rs | 172 | validate | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; generate::validate, the Cortex patch generators' output check (bin/cortex.rs, ai.rs) |
| axon-cortex/src/generate.rs | 418 | propose | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; CommandGenerator::propose (bin/cortex.rs `cmd:` generators) |
| axon-cortex/src/generate.rs | 427 | propose | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; CommandGenerator::propose (bin/cortex.rs `cmd:` generators) |
| axon-cortex/src/generate.rs | 446 | propose | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; CommandGenerator::propose (bin/cortex.rs `cmd:` generators) |
| axon-cortex/src/runner.rs | 422 | observe | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; Runner::observe, the Cortex repair loop's observation (Runner is built by bin/cortex.rs and cortex-policy-adapter, neither a protected crate) |
| axon-cortex/src/runner.rs | 436 | observe | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; Runner::observe, the Cortex repair loop's observation (Runner is built by bin/cortex.rs and cortex-policy-adapter, neither a protected crate) |
| axon-cortex/src/runner.rs | 453 | observe | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; Runner::observe, the Cortex repair loop's observation (Runner is built by bin/cortex.rs and cortex-policy-adapter, neither a protected crate) |
| axon-cortex/src/runner.rs | 522 | authorize_action | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; Runner::authorize_action (the Cortex loop's typed action authority) |
| axon-cortex/src/runner.rs | 1412 | check_typed_authority | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; Runner::check_typed_authority, under authorize_action |
| axon-cortex/src/runner.rs | 1418 | check_typed_authority | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; Runner::check_typed_authority, under authorize_action |
| axon-cortex/src/runner.rs | 1424 | check_typed_authority | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; Runner::check_typed_authority, under authorize_action |
| axon-cortex/src/runner.rs | 1431 | check_typed_authority | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; Runner::check_typed_authority, under authorize_action |
| axon-cortex/src/runner.rs | 1440 | check_typed_authority | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; Runner::check_typed_authority, under authorize_action |
| axon-cortex/src/runner.rs | 1742 | resolve_executable | EXEMPT: NOT ON ROUTE | registry executors: dev branch only (M265); resolve_executable, under register_pinned/register_expected/verified_path |
| axon-cortex/src/runner.rs | 1915 | register_expected | EXEMPT: NOT ON ROUTE | registry executors: dev branch only (M265); register_expected, from the registry's `executors` (load) and host_executor (the local route) |
| axon-cortex/src/runner.rs | 1969 | load | EXEMPT: OPERATOR-AUTHORED | operator suite registry (M141/M142/M143); the registry's version tag |
| axon-cortex/src/runner.rs | 2015 | load | EXEMPT: OPERATOR-AUTHORED | operator suite registry (M141/M142/M143); a check's `visibility` (who may SEE its source; Fabric materializes the suite the same way either way) |
| axon-cortex/src/runner.rs | 2029 | load | EXEMPT: OPERATOR-AUTHORED | operator suite registry (M141/M142/M143); and dominated: the registered workspace_version_ref is compared for equality with the reference Fabric computes by importing the suite (submit.rs check_target, M1061), which a string that is not acf1:<64 hex> never equals |
| axon-cortex/src/runner.rs | 2035 | load | EXEMPT: OPERATOR-AUTHORED | operator suite registry (M141/M142/M143); a repeated id in the operator's own registry (register_check keeps the later entry, the operator's choice either way) |
| axon-cortex/src/runner.rs | 2208 | verified_path | EXEMPT: NOT ON ROUTE | LocalInterpreterExecutor: dev route only (M265); LocalInterpreterExecutor::verified_path |
| axon-cortex/src/runner.rs | 2221 | verified_path | EXEMPT: NOT ON ROUTE | LocalInterpreterExecutor: dev route only (M265); LocalInterpreterExecutor::verified_path |
| axon-cortex/src/runner.rs | 2280 | run_checks | EXEMPT: NOT ON ROUTE | LocalInterpreterExecutor: dev route only (M265); LocalInterpreterExecutor::run_checks' output bound |
| axon-cortex/src/runner.rs | 2322 | run_limited | EXEMPT: NOT ON ROUTE | LocalInterpreterExecutor: dev route only (M265); run_limited's PR_SET_PDEATHSIG in the local child (also an OS ERROR: the child is not exec'd) |
| axon-cortex/src/runner.rs | 2382 | run_limited | EXEMPT: NOT ON ROUTE | LocalInterpreterExecutor: dev route only (M265); run_limited's wall-clock kill (a RESOURCE BOUND: no status, no report) |
| axon-cortex/src/runner.rs | 2552 | new | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; FabricSubmitExecutor::new, Cortex's client of `axon-fabric submit` (bin/cortex.rs); its operator fields are what Cortex sends, and Fabric judges the request itself |
| axon-cortex/src/runner.rs | 2557 | new | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; FabricSubmitExecutor::new, Cortex's client of `axon-fabric submit` (bin/cortex.rs); its operator fields are what Cortex sends, and Fabric judges the request itself |
| axon-cortex/src/runner.rs | 2563 | new | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; FabricSubmitExecutor::new, Cortex's client of `axon-fabric submit` (bin/cortex.rs); its operator fields are what Cortex sends, and Fabric judges the request itself |
| axon-cortex/src/runner.rs | 2613 | submit_path | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; FabricSubmitExecutor::submit_path (the pin on the axon-fabric binary Cortex runs) |
| axon-cortex/src/runner.rs | 2740 | run_checks | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; FabricSubmitExecutor::run_checks, Cortex's reading of a receipt Fabric already decided; the loop takes Fabric's receipt through intake, never Cortex's reading of it |
| axon-cortex/src/runner.rs | 2753 | run_checks | EXEMPT: NOT ON ROUTE | Cortex loop code; not used by protected crates; FabricSubmitExecutor::run_checks, Cortex's reading of a receipt Fabric already decided; the loop takes Fabric's receipt through intake, never Cortex's reading of it |
| axon-cortex/src/runner.rs | 2766 | FabricSubmitExecutor::run_checks | EXEMPT: NAMED ROW (M84) | M84's mutation (`Some(_) / None => {}`) is the removal of this arm; also Cortex client code, not on the protected route |
| axon-fabric/src/bin/axon-custodian.rs | 120 | main (mode match `_` arm) | EXEMPT: USAGE | no mode, so no config to serve under. Anchor overlap with M631 fixed: the store check is its own `if let` (M631 re-anchored, re-run KILLED) |
| axon-os/src/coalition.rs | 96 | carve_for_member | EXEMPT: NOT ON ROUTE | Coalition: no non-test caller of Coalition::new (pub; only axon-os/tests/r27_acceptance.rs) |
| axon-os/src/coalition.rs | 105 | carve_for_member | EXEMPT: NOT ON ROUTE | Coalition: no non-test caller of Coalition::new (pub; only axon-os/tests/r27_acceptance.rs) |
| axon-os/src/coalition.rs | 114 | carve_for_member | EXEMPT: NOT ON ROUTE | Coalition: no non-test caller of Coalition::new (pub; only axon-os/tests/r27_acceptance.rs) |
| axon-os/src/coalition.rs | 138 | propose_vote | EXEMPT: NOT ON ROUTE | Coalition: no non-test caller of Coalition::new (pub; only axon-os/tests/r27_acceptance.rs) |
| axon-os/src/coalition.rs | 147 | propose_vote | EXEMPT: NOT ON ROUTE | Coalition: no non-test caller of Coalition::new (pub; only axon-os/tests/r27_acceptance.rs) |
| axon-os/src/ledger.rs | 74 | carve | EXEMPT: NOT A VERDICT PROPERTY | this is the journal's real spend check (admission budget guard), still not a verdict property; only via journal carve_within (journal BudgetExceeded is _JNV); the compute axis, the one carve_within carves (its refusal is the journal's BudgetExceeded, exempt above as NOT A VERDICT PROPERTY) |
| axon-os/src/ledger.rs | 82 | carve | EXEMPT: NOT A VERDICT PROPERTY | only via journal carve_within (journal BudgetExceeded is _JNV); and SELECTS NOTHING there: carve_within builds every ledger with this cap 0 and carves 0 on it (`ResourceLedger::new(lineage, cap, 0, 0)`, `budget: 0, persist_bytes: 0`), and 0 + 0 > 0 is false |
| axon-os/src/ledger.rs | 90 | carve | EXEMPT: NOT A VERDICT PROPERTY | only via journal carve_within (journal BudgetExceeded is _JNV); and SELECTS NOTHING there: carve_within builds every ledger with this cap 0 and carves 0 on it (`ResourceLedger::new(lineage, cap, 0, 0)`, `budget: 0, persist_bytes: 0`), and 0 + 0 > 0 is false |
| axon-os/src/manifest.rs | 105 | parse | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/manifest.rs | 117 | parse | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/manifest.rs | 142 | parse | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/manifest.rs | 176 | parse | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/manifest.rs | 187 | parse | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/manifest.rs | 208 | parse | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/manifest.rs | 363 | validate_prefixes | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/manifest.rs | 366 | validate_prefixes | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/manifest.rs | 376 | nonneg | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/profile.rs | 92 | parse | EXEMPT: OPERATOR-AUTHORED | operator grant file (M277/M1098/M278) |
| axon-os/src/record.rs | 257 | verify | EXEMPT: NOT ON ROUTE | axon-os: not reached from supervise_requiring/parse_manifest/scan_effects/ledger; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none |
| axon-os/src/record.rs | 262 | verify | EXEMPT: NOT ON ROUTE | axon-os: not reached from supervise_requiring/parse_manifest/scan_effects/ledger; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none |
| axon-os/src/record.rs | 274 | verify | EXEMPT: NOT ON ROUTE | axon-os: not reached from supervise_requiring/parse_manifest/scan_effects/ledger; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none |
| axon-os/src/record.rs | 284 | verify | EXEMPT: NOT ON ROUTE | axon-os: not reached from supervise_requiring/parse_manifest/scan_effects/ledger; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none |
| axon-os/src/record.rs | 293 | verify | EXEMPT: NOT ON ROUTE | axon-os: not reached from supervise_requiring/parse_manifest/scan_effects/ledger; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none |
| axon-os/src/replay.rs | 40 | replay | EXEMPT: NOT ON ROUTE | axon-os: not reached from supervise_requiring/parse_manifest/scan_effects/ledger; replay::replay, called only by cli.rs cmd_replay |
| axon-os/src/runtime.rs | 231 | create | EXEMPT: NOT ON ROUTE | axon-os: not reached from supervise_requiring/parse_manifest/scan_effects/ledger; StagingDir::create, used by AxonCoreRuntime (the `axon-os` binary's runtime: cli.rs); Fabric admits through its own AdmissionProbe (submit.rs), which stages nothing |
| axon-os/src/runtime.rs | 234 | create | EXEMPT: NOT ON ROUTE | axon-os: not reached from supervise_requiring/parse_manifest/scan_effects/ledger; StagingDir::create, used by AxonCoreRuntime (the `axon-os` binary's runtime: cli.rs); Fabric admits through its own AdmissionProbe (submit.rs), which stages nothing |
| axon-os/src/runtime.rs | 720 | declared_effects | EXEMPT: NOT ON ROUTE | axon-os: not reached from supervise_requiring/parse_manifest/scan_effects/ledger; AxonCoreRuntime::declared_effects; Fabric's AdmissionProbe implements declared_effects itself (submit.rs, the same deny-by-default unknown()) |
| axon-psv/src/bin/axon-psv-runner.rs | 25 | main | EXEMPT: NOTHING TO ADMIT | start() fails before any verdict.json (unbound: 27, M241) or when its one write failed (no file / a prefix derive refuses as malformed) |
| axon-vm/src/admit.rs | 284 | resolve_effect_grant | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; resolve_effect_grant, under admit_job |
| axon-vm/src/admit.rs | 310 | check_extended_tcb | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; check_extended_tcb, under admit_job |
| axon-vm/src/admit.rs | 322 | check_extended_tcb | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; check_extended_tcb, under admit_job |
| axon-vm/src/admit.rs | 386 | measure_and_attest_inner | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; measure_and_attest_inner (admit_job, and the `axon-vm` binary's run path) |
| axon-vm/src/admit.rs | 411 | measure_and_attest_inner | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; measure_and_attest_inner (admit_job, and the `axon-vm` binary's run path) |
| axon-vm/src/admit.rs | 423 | measure_and_attest_inner | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; measure_and_attest_inner (admit_job, and the `axon-vm` binary's run path) |
| axon-vm/src/firecracker.rs | 575 | wait_for_socket | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; wait_for_socket (axon-vm's own VMM driver) |
| axon-vm/src/firecracker.rs | 629 | fc_put | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; fc_put (axon-vm's Firecracker API client) |
| axon-vm/src/firecracker.rs | 779 | resolve | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; FirecrackerBin::resolve/at (axon-vm's VMM lookup) |
| axon-vm/src/firecracker.rs | 785 | at | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; FirecrackerBin::resolve/at (axon-vm's VMM lookup) |
| axon-vm/src/firecracker.rs | 790 | at | EXEMPT: NOT ON ROUTE | axon-vm: only BACKEND_PROFILE/MmdsPayload/embed_policy_in_cmdline used; FirecrackerBin::resolve/at (axon-vm's VMM lookup) |
| axon-cortex/src/lib.rs | 407 | cxg_c2_unknown_is_not_absent_is_not_empty | NOT A SITE: test code | the #[cfg(test)] module, renamed `contract_tests` -> `tests` so the gate's test-module rule applies |
| axon-cortex/src/lib.rs | 430 | cxg_c2_unknown_variant_and_field_refuse | NOT A SITE: test code | the #[cfg(test)] module, renamed `contract_tests` -> `tests` so the gate's test-module rule applies |
| axon-cortex/src/lib.rs | 435 | cxg_c2_unknown_variant_and_field_refuse | NOT A SITE: test code | the #[cfg(test)] module, renamed `contract_tests` -> `tests` so the gate's test-module rule applies |
| axon-cortex/src/lib.rs | 447 | cxg_c2_non_finite_and_duplicate_keys_refuse | NOT A SITE: test code | the #[cfg(test)] module, renamed `contract_tests` -> `tests` so the gate's test-module rule applies |
| axon-cortex/src/lib.rs | 457 | cxg_c2_non_finite_and_duplicate_keys_refuse | NOT A SITE: test code | the #[cfg(test)] module, renamed `contract_tests` -> `tests` so the gate's test-module rule applies |
| axon-cortex/src/lib.rs | 499 | cxg_c2_non_finite_and_duplicate_keys_refuse | NOT A SITE: test code | the #[cfg(test)] module, renamed `contract_tests` -> `tests` so the gate's test-module rule applies |
