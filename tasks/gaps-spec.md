# Gap closure: five tracked defects (G1-G5)

Source: the user's ranked list (2026-10-15), taken from `AXON-COMPLETENESS.json` rows. Base: `main` b11841c8,
branch `gaps/int`. Each item was re-measured against the code by a read-only scout before this spec was written;
where the tracker row was stale, the item is rescoped to what is actually still open and the row is corrected.
Step 1 adversarial review (reviewer agent, 19 findings, 2026-10-15) is folded in below as `[REVISED: …]` markers;
the original text is commit 9602cc11.

Shared acceptance for every item `[REVISED: added, review #10]`: `scripts/gate.sh` shows no new failure against the
baseline; `python3 -B scripts/v022_refusal_coverage.py` and `scripts/v022_g01_mutations.py` stay green, with every
new refusal site carrying a killed mutation row or a reasoned exemption and every moved anchor re-pinned;
`AXON-COMPLETENESS.json` rows corrected and `AXON-COMPLETENESS.md` regenerated (integrator, last).

## G1 — Approval binds the whole import graph (P0, security)

Rows: `approval binds IMPORT GRAPH`, `ast review shows import origin`, `import resolution determinism`,
and the import half of `crate axon-intent` / `crate axon-os`.

Defect (demonstrated): `axon ast approve main.ax` hashes only the entry bytes (`main.rs` `cmd_ast_approve`,
`program_digest_hex`); `axon deploy` compares only the entry bytes (`cmd_deploy` ~8786) and then
`load_use_decls` re-resolves every `use` from the ambient search path. Rewriting an imported module (for example
to declare `write:[/] net:[*] exec:any`) leaves the approval valid. The intent→os token
(`axon-intent` `program_digest`, `axon-os` `verify_approval`) has the same blind spot: `axon-os` spawns
`axon run`, which resolves imports freely (with `AXON_PATH` forwarded for non-hermetic grants).

Items:
- **G1a — closure digest from the loader.** `load_module_recursive` records each module it actually merged
  (name, chosen path, bytes, sealed bit) and the recording is threaded out through `run_check_pipeline_located`
  `[REVISED: review #15, 8 callers]`. One function computes
  `imports_digest = "axsha256:" + sha256(concat over modules sorted by name of name ␟ axh1-hash ␟ sealed ␞)`
  `[REVISED: sealed bit added, review #15 — loading depends on sealed_module_dirs]`, using `lockfile::module_hash`.
  Empty closure = sha256 of the empty string, defined and tested. A `use` that resolves nowhere because no search
  directory exists (silently skipped at `lib.rs:582`) is refused when a digest is being computed
  `[REVISED: review #15]`. The digest covers name + content, never the absolute path. It MUST come from the
  loader's recorded choices, not `resolve_use_files_transitive`, so it cannot disagree with what ran.
- **G1b — `ast approve` / `deploy` bind it.** `ast approve` now parses and loads the program and refuses on a parse
  error or E0901 `[REVISED: review #15 — approve never parsed]`; it writes `axon-ast-approved/2` with `hash` (entry),
  `imports_digest`, and `modules:[{name,path,hash,sealed}]` (audit trail: which file was chosen). `deploy` hashes
  the bytes it parsed (not a second read) and recomputes the digest from the program it is about to run; mismatch
  refuses on the existing tamper path (exit 8, `blocked_approval`, message names the changed/added/removed module).
  `[REVISED: was "/1 records non-blocking", review #1 BLOCKER — that would let an edited entry file deploy]`
  A `/1` record still binds the entry hash: mismatch is exit 8 exactly as today; a match yields `approved:false`
  with "approved before import binding — re-approve", non-blocking (identical to having no record, so no downgrade
  gain). Legacy FNV records unchanged.
- **G1c — `axon imports <file> [--json]`.** Prints the loader-recorded closure:
  `{"schema":"axon-imports/1","file","imports_digest","modules":[{name,path,hash,sealed}]}`; unresolved import →
  exit 2. This is the "nothing records which file was chosen" fix and the interface G1e uses.
- **G1d — run-time binding.** `AXON_REQUIRE_IMPORTS_DIGEST=<digest>`: every executing verb that loads modules
  (`run`, `test`, `goal`, `session`, `deploy`) compares the loader-recorded digest after loading and refuses
  (exit 8, no program effects) on mismatch; a verb that executes without the shared loader refuses when the variable
  is set `[REVISED: was run only, review #9]`. The check happens in the process that loaded the modules. The
  variable is removed from the process environment immediately after the check, so a nested `axon run` started by
  the approved program via `exec` is not refused `[REVISED: review #8, host.rs:283-287 passes the full env]`.
  Registered in `env_registry.rs`; `AXON_REFERENCE.md` regenerated; a control row with all engine states; native:
  refused at `axon build` time AND by an `axon-rt` REFUSED row at run time (`axon-rt/src/lib.rs:2062` table), and
  added to `scripts/build_outcome_regression.sh` `[REVISED: was build-time only, review #9 — house rule]`.
- **G1e — intent→os token binds the closure (depends on G1c, G1d, G5a).** `axon-intent approve` obtains
  `imports_digest` by running `$AXON_BIN imports <program> --json` **with the child's environment shape**
  (`env_clear`, the same `AXON_PATH`/`HOME`/cwd rules and binary as `AxonCoreRuntime::run_sandboxed`), via one
  `axon-os` helper shared by both `[REVISED: review #7 — approver env ≠ child env]`. `axon-os` verifies it as part of
  the token and sets `AXON_REQUIRE_IMPORTS_DIGEST` on the child after `env_clear`, in both reproducible branches;
  `Runtime::run_sandboxed`'s signature change reaches `runtime.rs:122`, MockRuntime, the isolation probes and
  `axon-fabric/src/submit.rs:917`.
- **G1g — Fabric route `[REVISED: added, review #3 BLOCKER]`.** Fabric admits through
  `axon_os::supervise_requiring` with a no-op probe and executes elsewhere. Its grant tokens migrate to `/3` in the
  same change as G1e/G5a (tests in `axon-fabric/tests/grant_authority.rs`, mutation rows M1746/M1766-M1769
  re-pinned), and the token's `imports_digest` reaches whatever Fabric executor runs the program, or the Fabric
  route refuses tokens whose program has imports — whichever the code supports without a new protocol; the choice
  and reason are recorded.
- **G1f — ast review shows origin.** Each fn in `cmd_ast_review` carries `origin`: `null` for the entry file, the
  module NAME for imported fns `[REVISED: was path, review #16 — absolute paths would reach axon-web clients]`; the
  human view tags `[from <module>]`. `[REVISED: schema stays axon-ast-review/2, review #16 — the field is additive
  and the fn population is unchanged; a bump would break axon-web and docs for no reader benefit]`.

Acceptance: the demonstrated attack (approve, rewrite an imported module, deploy) is refused on the `ast approve`
→ `deploy` path and on the `axon-intent approve` → `axon-os run` path; an unchanged program (with imports, hermetic
and non-hermetic grants) still deploys/runs; fail-first test per path; mutation: dropping the digest comparison turns
the test red.

## G2 — Cancellation reaches the whole job (P0, security) — fix landed, proof missing

Tracker row is stale: `821cf767` (2026-09-20) spawns with `process_group(0)` and kills with `killpg`.
`[REVISED: sequencing, review #19 — G2d first, then G2c, then G2a with its rows]`
- **G2d — supervisor death must not orphan the job.** Since the job left the terminal's process group, Ctrl-C/
  SIGTERM/SIGHUP to `axon-os` kills the supervisor and leaves the job running. `[REVISED: design, review #13]`:
  install handlers only for signals not already ignored (keeps `nohup`); a fixed-size registry of active job
  pgids (parallel `run_bounded_capped` callers exist in tests), with signals blocked across spawn+register and the
  mask restored in the child; the handler (async-signal-safe only) SIGKILLs every registered group, guarded
  `pid > 1` `[REVISED: G2b's guard moved here, review #17 — in kill_group it can never trigger]`, and sets a flag;
  the normal poll loop then observes the death, the supervisor writes the run record as killed-by-signal and
  cleans staging, and only then exits 128+signal. A slot is cleared before the child is reaped.
- **G2b** `[REVISED: folded into G2d, review #17]`.
- **G2c — bounded drains.** A descendant that leaves the group (`setsid`) and keeps stdout/stderr open blocks the
  drain join forever, after a kill and after a normal exit alike (`runtime.rs:427, 447-448`). Bound both joins
  `[REVISED: was post-kill only, review #17]`. The real interpreter uses `.output()` (`host.rs:284`) so this needs a
  non-interpreter job or an `exec`ed `setsid` to reach; the test uses a shell job.
- **G2a — proof.** Regression tests: timeout path and kill-latch path each kill a grandchild (`/proc/<pid>` oracle
  with cmdline check, grandchild stdio to `/dev/null`), a production-path test through
  `AxonCoreRuntime::run_sandboxed`/`supervise`, a bystander-survives test, and a signal test (SIGTERM to an
  `axon-os run` process kills the job and leaves a record). Mutation-verified: removing `process_group(0)` or
  replacing `killpg` with `child.kill()` turns them red. Refusal-coverage rows/exemptions for the new `signal`/
  `killpg` sites; the exact-text exemptions at `v022_refusal_coverage.py:3422-3423` updated in the same change.

Acceptance: tests above green and mutation-verified; tracker row corrected (it cites stale lines).

## G3 — Multi-file diagnostic locations (P1, correctness) — CLI fixed, LSP open

Tracker row is stale: `Span` carries `SourceId`, imported modules are interned, the CLI renders against the
span's own file, `diagnostic_location_gate.sh` is green and wired (strict tier). Still open:
- **G3a — LSP diagnostics.** `lsp.rs` `push_diagnostics` converts every span against the open document's text, so
  an error in an imported module lands at a clamped/wrong spot in the open file. `[REVISED: was publish to the
  other file's URI, review #14 — didClose/didChange never clear foreign URIs and two documents would overwrite each
  other]` A diagnostic whose span belongs to another file is reported on the open document at the `use` line that
  imports it, with `relatedInformation` carrying that file's URI and correct line; never a clamped position.
- **G3b — no panic.** Offset→position conversion never slices off a UTF-8 boundary (`lsp.rs:88, 885-894`, reachable
  in a single file too) `[REVISED: scope widened, review #14]`.
- **G3c — go-to-definition** to an imported symbol resolves against that symbol's own file, not the open document
  (`lsp.rs:723-741`, same defect class) `[REVISED: added, review #14]`.
- **G3d — verify the gate** passes on this tree by running it directly; correct the tracker row.

Acceptance: an LSP test (`AXON_PATH` set, a TYPE error in `lib.ax` — parse errors carry no span,
`lib.rs:920-926` `[REVISED: review #14]`) gets the diagnostic on `main.ax`'s `use` line with related information at
`lib.ax`'s line; definition jumps land in `lib.ax`; a non-boundary offset does not panic.

## G4 — Parity harnesses misclassify build failures (P1, correctness of evidence)

Tracker row is partly stale: `scripts/lib/harness_skip.sh` exists, the two detectors agree, `handler_resume`/
`exit_code` were fixed. `[REVISED: sequencing, review #19 — G4c, then G4a/G4b, then G4d]`
- **G4c — one skip detector source of truth.** Move `harness_says_skip` into `harness_skip.sh`; add the missing shell
  table test mirroring `cli_run.rs`'s `skip_rule_table` (its comment claims one exists); fix the stale doc comments
  (`parity_all.sh:131-136`, `cli_run.rs:79-84`).
- **G4a — the four deny-list harnesses** (`dict`, `arr_reduce`, `bitwise_cast`, `parse_or`): only
  `IR verification failed|LLVM ERROR|E0910` FAILs; any other build error SKIPs and the harness prints PASS. Switch to
  the allow-list (`native_build_unavailable`) per row — not `native_build_failed`, whose `harness_skip` exits the
  whole harness `[REVISED: review #18]`, copying `closure_capture_parity.sh:53-55,117-119` — and add an OK-count
  guard so an all-skip run is a harness SKIP, not PASS.
- **G4b — skips after every prerequisite passed.** AOT-wasm legs (`wasm_aot_env`, `wasm_str_abi`,
  `wasm_malloc_abi`, `wasm_aot_run`, `wasm_browser`, `wasm_browser_io`), interp-only builds reported as
  "interp build unavailable" (no LLVM dependency, so never a missing toolchain), `wasm_host_await`,
  `wasm_browser_interp`, `smt_mint`/`smt_discharge`, and the 43 parity harnesses (+2 other scripts) that print
  "codegen build unavailable (LLVM absent)" on any `cargo build` failure without proving LLVM is absent. Rule: skip
  only on a proven-missing prerequisite (`native_build_unavailable`, `rust_target_installed`, `command -v`,
  llvm-config absent, libz3 absent); any other build failure is a FAIL showing stderr. One shared helper.
- **G4d — floor.** The single call at `gate.sh:996` runs `parity_all.sh` with `AXON_HARNESS_STRICT=1` set inline
  `[REVISED: review #18 — the variable already exists]`; the default `EXPECT_MIN_PASS` is computed from the harness
  count minus the allow-list rather than hard-coded 40, and stays overridable `[REVISED: "ci.yml uses it" was wrong;
  it appears only in a ci.yml comment]`.
- **G4e — `cargo test <filter>` exit status as proof** at `r26:222`, `r33:110`, `r34:93-97` → `cargo_test_must_run`
  `[REVISED: narrowed, review #18 — r33/r34 already check named tests elsewhere]`.

Acceptance: each converted harness, fed a forced non-toolchain build failure, reports FAIL (throwaway run with a
broken stub); a strict `AXON_HARNESS_STRICT=1 parity_all.sh` run on this host is green with the baseline pass count.

## G5 — Approval tokens and run records are forgeable (P0, security)

Confirmed open at b11841c8: `axon-intent` and `axon-os` tokens are unkeyed `axtok1:` sha256 over public inputs;
`axon-os` does not check `schema`; `record::verify` seeds the chain from the record's own `manifest_digest` and
never reads the `.axjob`; `RunRecord.approval` is set after the seal, so it can be rewritten freely.
`[REVISED: sequencing, review #19 — G5a+G5b+G1e+G1g land as one token change; G5c last]`
- **G5a — keyed tokens.** `[REVISED: was AXON_ATTEST_KEY, review #6 — that key is axon-vm's with an ephemeral
  fallback; reusing it would silently flip axon-os to refusing for any operator who set it for axon-vm]` A dedicated
  `AXON_APPROVAL_KEY` (hex, ≥16 bytes), derived subkeys `HMAC(op_key, "axon-approval token v1")` and
  `HMAC(op_key, "axon-os record v1")`, following `axon-loop` `LedgerKey` (malformed/short → refuse; no ephemeral
  fallback), with a known-answer test. Threat model, documented: the key is symmetric, so every holder (the approving
  operator and the enforcing host) can mint; `approved_by` stays unauthenticated (per-approver signatures are
  `post-c9-keys-and-signatures`, out of scope). One token formula owned by `axon-os` (as `canonical_grant` is);
  `axon-intent`'s `compute_token_digest`/`verify_token` and `emit.rs`'s compile-path tokens delegate to it
  `[REVISED: call sites added, review #11]`. Token `axon-approval/3` (`/2` is reserved by R24) carries
  `imports_digest`. `[REVISED: unkeyed /3 named, review #2 BLOCKER]` Keyed: `axmac1:` + HMAC over
  `schema ␟ pd ␟ gd ␟ imports ␟ by ␟ decision ␟ risk`, constant-time compare (`axon_attest::constant_time_eq` made
  public, its exemption anchor re-pinned). Unkeyed: `axtok3:` + sha256 over the same fields. `approve` emits
  `axmac1` when the key is set, `axtok3` otherwise. The verifier's key decides: key set → only `axmac1`; no key →
  `axtok3` accepted with the "integrity only" label, `axmac1` refused (cannot be checked). The "integrity only"
  message in `axon-os/src/cli.rs:326,347-350` depends on whether a key is set.
- **`/1` tokens** `[REVISED: was "refused", review #2/#5]`: a `/1` token cannot bind imports and its `axtok1` digest
  is unkeyed. Under a key it is refused (setting the new key is the operator's opt-in; no existing deployment has
  `AXON_APPROVAL_KEY`, so nothing that works today stops working). Without a key it is accepted only when the
  program's import closure is empty — the entry digest fixes the import set, so such a token is exactly as strong as
  before — and refused with "re-approve: this token predates import binding" otherwise.
- **G5b — schema check** in `axon-os` (`TokenView.schema`; unknown schema refused).
- **G5c — run-record binding.** `axon-os verify <record>` reads the archived sibling `<run_id>.axjob` (or
  `--job <path>`), recomputes `manifest_digest`, refuses on mismatch (exit 11) or when no job is found.
  `[REVISED: prefixes, review #4 BLOCKER]` `approval` is set on every return path (including the isolation-denial
  early return, `supervisor.rs:51-58`) before the seal and included in it: unkeyed seal `axrec3:`, keyed seal
  `axrmac1:`. Old `axrec2` records still verify without a key (labelled "approval not sealed"); a keyed verifier
  refuses `axrec2`/`axrec3` (downgrade protection, same opt-in argument as tokens). Replay of an archived run under
  a newly set key therefore refuses; documented.
- Gate hygiene `[REVISED: review #12]`: `r22_acceptance_gate.sh` fails (not tolerates) a missing `AXON_BIN` now that
  `approve` runs the interpreter, and pins/clears `AXON_APPROVAL_KEY` so the byte-identity check sees one key;
  tests that hand-mint tokens clear the key explicitly; the test names r22 requires stay defined.

Acceptance: a token forged without the key is refused when a key is set; `axtok1`/`axtok3` refused by a keyed
verifier; a bad schema refused; a record whose job does not match refused; a rewritten `approval` field refused;
r22 gate passes.

## needs-human (excluded from the autonomous run)

- **NH1 — search the entry file's directory for modules** (`import resolution determinism` row). Changes which
  program a source means, and axon-web stages files in world-writable `/tmp`, which would become a search directory.
  Recommendation: add it LAST in the order, skipped under `AXON_PATH_EXCLUSIVE=1`, and never for axon-web's staging
  dir. Not built. The determinism half that is a defect (no record of which file was chosen, no digest that
  distinguishes environments) is closed by G1a-G1c.

## Out of scope / open

- Other supervisors that kill only their direct child (`axon-cortex` runner/generate, `axon-psv`, `axon-reflex`,
  `axon-fabric` git_data, `axon-vm` firecracker) — not in the tracker row; logged to opportunities.

## Status (end of run)

| Item | Status | Lane / merge |
|---|---|---|
| G1a-d, f | done | gaps/core → 642be152 |
| G1e, G1g | done (Fabric refuses import-bound approvals; module check deny-by-default after review) | gaps/token |
| G2a, c, d | done | gaps/cancel |
| G3a-d | done (+ relative AXON_PATH URI fix from review; LSP tests wired into gate.sh) | gaps/lsp + integrator |
| G4a-e | done (+ wasm runtime lookup fix ca49d537 so strict parity holds with an outside target dir) | gaps/parity + integrator |
| G5 key, a, b, c | done | gaps/record, gaps/token |
| NH1 entry-directory module search | needs-human, not built | — |
