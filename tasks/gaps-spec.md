# Gap closure: five tracked defects (G1-G5)

Source: the user's ranked list (2026-10-15), taken from `AXON-COMPLETENESS.json` rows. Base: `main` b11841c8,
branch `gaps/int`. Each item was re-measured against the code by a read-only scout before this spec was written;
where the tracker row was stale, the item is rescoped to what is actually still open and the row is corrected.

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
  (name, chosen path, bytes) — `load_use_decls_recorded` (or equivalent) returns them. One function computes
  `imports_digest = "axsha256:" + sha256(concat over modules sorted by name of name ␟ axh1-hash ␞)`, using
  `lockfile::module_hash`. The digest covers name + content, never the absolute path (machine-independent; the
  r22 byte-identical check depends on that). The digest MUST come from the loader's recorded choices, not the
  separate `resolve_use_files_transitive`, so it cannot disagree with what ran.
- **G1b — `ast approve` / `deploy` bind it.** `ast approve` writes `axon-ast-approved/2` with `hash` (entry),
  `imports_digest`, and `modules:[{name,path,hash}]` (audit trail: which file was chosen). `deploy` recomputes the
  digest from the program it is about to run and refuses on mismatch on the existing tamper path (exit 8,
  `blocked_approval`, message names the changed module). `/1` records stay "not approved, non-blocking", exactly
  as legacy records are today (no downgrade gain: identical to having no record).
- **G1c — `axon imports <file> [--json]`.** Prints the loader-recorded closure:
  `{"schema":"axon-imports/1","file","imports_digest","modules":[{name,path,hash}]}`; unresolved import → exit 2.
  This is the "nothing records which file was chosen" fix and the interface G1e uses.
- **G1d — run-time binding.** `axon run` honours `AXON_REQUIRE_IMPORTS_DIGEST=<digest>`: after loading, it
  compares the loader-recorded digest and refuses (exit 8, no program effects) on mismatch. Check happens in the
  same process that loads the modules (no check/use gap). Registered in `env_registry.rs`, `AXON_REFERENCE.md`
  regenerated, a control row added. Native `axon build`: a built binary has no run-time module loading, so the
  variable set at `axon build` time is refused (E0910-style, like the other interp-only controls) rather than
  silently ignored.
- **G1e — intent→os token binds the closure (depends on G1c, G1d, G5a).** `axon-intent approve` obtains
  `imports_digest` by running `$AXON_BIN imports <program> --json` and puts it in the token; `axon-os` verifies it
  as part of the token and passes it to the child as `AXON_REQUIRE_IMPORTS_DIGEST`.
- **G1f — ast review shows origin.** Each fn in `cmd_ast_review` carries `origin`: `null` for the entry file,
  the module path for imported fns (`span::source_path_of(f.span.source)`); the human view tags `[from <path>]`.
  Schema bumps to `axon-ast-review/3` (population/shape change; /1→/2 precedent); `axon-web` and docs updated.

Acceptance: the demonstrated attack (approve, rewrite an imported module, deploy) is refused on both paths
(`ast approve`→`deploy`, `axon-intent approve`→`axon-os run`); an unchanged program still deploys/runs; a fail-first
test for each path; mutation check: dropping the digest comparison turns the test red.

## G2 — Cancellation reaches the whole job (P0, security) — fix landed, proof missing

Tracker row is stale: `821cf767` (2026-09-20) spawns with `process_group(0)` and kills with `killpg`. Still open:
- **G2a — proof.** Regression tests: timeout path and kill-latch path each kill a grandchild
  (`/proc/<pid>` oracle with cmdline check, grandchild stdio to `/dev/null`), a production-path test through
  `AxonCoreRuntime::run_sandboxed`/`supervise`, and a bystander-survives test. Mutation-verified: removing
  `process_group(0)` or replacing `killpg` with `child.kill()` turns them red.
- **G2b — `pid <= 1` guard** in `kill_group`, matching `axon-intent/src/synth.rs` (a `killpg(0|1)` would signal
  the supervisor's own group or everything).
- **G2c — no hang after kill.** A descendant that leaves the group (`setsid`) and keeps stdout/stderr open blocks
  the drain join forever after the kill. Bound the post-kill drain (the job's result is already decided).
- **G2d — supervisor death must not orphan the job.** Since the job is no longer in the terminal's process group,
  Ctrl-C/SIGTERM/SIGHUP to `axon-os` kills the supervisor and leaves the job running. Forward those signals to the
  active job group (async-signal-safe `killpg`), then die with the original signal.

Acceptance: tests above green and mutation-verified; tracker row corrected.

## G3 — Multi-file diagnostic locations (P1, correctness) — CLI fixed, LSP open

Tracker row is stale: `Span` carries `SourceId`, imported modules are interned, the CLI renders against the
span's own file, `diagnostic_location_gate.sh` is green and wired (strict tier). Still open:
- **G3a — LSP.** `lsp.rs` `push_diagnostics` converts every span against the open document's text, so an error in
  an imported module is placed at a clamped/wrong spot in the open file, and `&source[..clamped]` can panic off a
  UTF-8 boundary. Spans whose source is another file are published to that file's URI (LSP `publishDiagnostics`
  is per-URI) at the correct line, never clamped into the open document; offset→position never panics.
- **G3b — verify the gate** passes on this tree by running it directly, and correct the tracker row.

Acceptance: an LSP test opens `main.ax` importing a broken `lib.ax` and gets the diagnostic on `lib.ax`'s URI and
line; a test with a non-boundary offset does not panic.

## G4 — Parity harnesses misclassify build failures (P1, correctness of evidence)

Tracker row is partly stale: `scripts/lib/harness_skip.sh` exists, the two detectors agree, `handler_resume`/
`exit_code` were fixed. Still open:
- **G4a — the four deny-list harnesses** (`dict`, `arr_reduce`, `bitwise_cast`, `parse_or`): only
  `IR verification failed|LLVM ERROR|E0910` FAILs, any other build error SKIPs and the harness prints PASS. Switch to
  the allow-list (`native_build_unavailable`) — any other build failure is a FAIL with stderr — and add a ran/OK
  guard so an all-skip run is a harness SKIP, not PASS.
- **G4b — skips after every prerequisite passed.** AOT-wasm legs (`wasm_aot_env`, `wasm_str_abi`,
  `wasm_malloc_abi`, `wasm_aot_run`, `wasm_browser`, `wasm_browser_io`), interp-only builds reported as
  "interp build unavailable" (no LLVM dependency, so never a missing toolchain), `wasm_host_await`,
  `wasm_browser_interp`, `smt_mint`/`smt_discharge` ("build failed — skipping"), and the ~45 harnesses that print
  "codegen build unavailable (LLVM absent)" on any `cargo build` failure without proving LLVM is absent. Rule: skip
  only on a proven-missing prerequisite (`native_build_unavailable`, `rust_target_installed`, `command -v`,
  llvm-config absent, libz3 absent); any other build failure is a FAIL showing stderr. One shared helper.
- **G4c — one skip detector source of truth.** Move `harness_says_skip` into `harness_skip.sh`; add the missing
  shell table test that mirrors `cli_run.rs`'s `skip_rule_table` (its comment claims one exists); fix the stale
  doc comments (`parity_all.sh:131-136`, `cli_run.rs:79-84`).
- **G4d — floor.** `gate.sh --strict` runs `parity_all.sh` with `AXON_HARNESS_STRICT=1` (any non-allow-listed skip
  fails), and the default `EXPECT_MIN_PASS` is raised from 40 to the real count minus allow-listed skips; the
  variable stays overridable (ci.yml uses it).
- **G4e — `cargo test <filter>` exit status as proof** in `r26`, `r33`, `r34` acceptance gates: use
  `cargo_test_must_run`.

Acceptance: each converted harness, fed a forced non-toolchain build failure, reports FAIL (shown by a throwaway
run with a broken stub); a strict `parity_all.sh` run on this host is green with the same pass count as baseline.

## G5 — Approval tokens and run records are forgeable (P0, security)

Confirmed open at b11841c8: `axon-intent` and `axon-os` tokens are unkeyed `axtok1:` sha256 over public inputs;
`axon-os` does not check `schema`; `record::verify` seeds the chain from the record's own `manifest_digest` and
never reads the `.axjob`; `RunRecord.approval` is set after the seal, so it can be rewritten freely.
- **G5a — keyed tokens.** One approval key: `AXON_ATTEST_KEY` (hex, ≥16 bytes), derived
  `k = HMAC(op_key, "axon-approval key v1")`, following `axon-loop` `LedgerKey` (malformed/short → refuse; no
  ephemeral fallback). One token formula, owned by `axon-os` (as `canonical_grant` already is), used by
  `axon-intent`: `axmac1:` + HMAC over `schema ␟ pd ␟ gd ␟ imports ␟ by ␟ decision ␟ risk`, constant-time compare
  (`axon_attest::constant_time_eq` made public). The verifier's key decides: key set → only `axmac1` accepted;
  no key → `axmac1` refused (cannot check), `axtok1` accepted with the existing "integrity only" label.
- **G5b — schema check** in `axon-os` (`TokenView.schema`, unknown schema refused).
- **G5c — run-record binding.** `axon-os verify <record>` reads the archived sibling `<run_id>.axjob` (or
  `--job <path>`), recomputes `manifest_digest`, refuses on mismatch (exit 11) or when no job is found.
  `approval` is set before the seal and included in it. Under a key the seal is an HMAC (`axrec3:`) and a keyed
  verifier refuses `axrec2`.
- Token schema: G1e and G5a ship together as one bump to `axon-approval/3` (`/2` is reserved by R24's
  `MultiApprovalToken`), carrying `imports_digest`. `/1` tokens are refused with a "re-approve" message, because a
  `/1` token cannot bind imports.

Acceptance: a token forged without the key is refused when a key is set; an `axtok1` token is refused by a keyed
verifier; a bad schema is refused; a record whose job does not match is refused; a rewritten `approval` field is
refused; the r22 gate still passes (byte-identical tokens across runs, since HMAC is deterministic).

## Out of scope / open

- Adding the entry file's directory to the module search path (`import resolution determinism` row) changes which
  program a source means and would make `/tmp` (axon-web staging) a search directory. Candidate: needs-human.
- Other supervisors that kill only their direct child (`axon-cortex` runner/generate, `axon-psv`, `axon-reflex`,
  `axon-fabric` git_data, `axon-vm` firecracker) — not in the tracker row; log to opportunities.
