# Unattended repair authorization (NH-R1) — DRAFT spec (after Step 1 review)

**Status: DRAFT. Unattended execution is NOT authorized by this document.**
- Building it needs a human go-ahead.
- Enabling it after the build (§9) needs a second, separate human decision.

**Source decisions:**
- `tasks/repair-spec.md:211-222` — NH-R1: `[DECIDED by human 2026-10-18: A — build a new axon-approval schema, specified separately before unattended repair is enabled]`.
- The enforcement-gate scope addition of 2026-10-18 (§6).
- `tasks/opportunities.md` O-S1-03 / -05 / -07 / -09 / -13 are the enforcement prerequisites.

**Step 1 review:**
- `AdversarialSpecReviewer`: 2 blockers, 10 majors, 3 minors (F1–F15).
- `FeasibilityReviewer`: 2 blockers, 11 majors, 9 minors (D1–D22).
- Every disposition appears inline as `[REVISED: F#/D# …]`.
- Code anchors are at worktree `fu-int` (f26e9506 tree), and the reviewers re-read them.

## 1. Problem

Today the operator's command line is the authority for `cortex repair` (`tasks/repair-spec.md:221`). An unattended
run — one no human launches by hand — has no authority source at all.

The per-candidate token `axon-approval/3` (`crates/axon-os/src/approval.rs:117`) is the wrong shape:
- it binds entry bytes, so every patch would need a fresh approval;
- a token minted automatically is the machine approving itself.

Several routes let a run exceed the limits an operator would sign. A valid token over limits nobody can enforce is a
false assurance. So this spec gates unattended runs on **enforcement**, not only on a signature (§6).

## 2. Non-goals

- **Per-candidate approval is unchanged.** `axon-approval/3`, `axon ast approve`, `axon-intent approve`, H3's
  `--program` and H2 (Fabric refuses import-bound approvals) stay exactly as they are.
- **The envelope accepts no result.** A repair accepted under an envelope still needs the existing human approval to
  deploy (§5.7).
- The following are out of scope:
  - multi-party approval and R24's `/2`;
  - USD metering inside the loop (the budget is counted in proposals, steps and time);
  - Fabric unattended (E2);
  - `cmd:` generators (E9);
  - files using `exec` or native modules (E11, E4);
  - R6 (`tasks/r6-ledger-spec.md`).
- **Attended mode is byte-unchanged** (S11).

## 3. Threat model

| Actor | Trust |
|---|---|
| Operator (holds `AXON_APPROVAL_KEY`) | trusted; the only one who can mint |
| `cortex` binary acting as verifier/walker (same uid as its service account) | trusted code, but **holds no mint capability** (§4.2) |
| Generator / ranker / audit model output | untrusted |
| Check program (the file under repair and its tests, running candidate code) | untrusted; confined by fence + ambient sandbox only where §6 says that confinement is real |
| Workspace content (it may carry symlinks, F2) | untrusted |
| Concurrent same-uid processes | untrusted for file contents (E5); same-uid code that escapes confinement defeats any same-uid store, so confinement escapes are refused, not tolerated (E11, E4, E9, F2) |

**Signing** `[REVISED: F3, D3 — the HMAC verifier is a minter, and the runner would hold the master key (it could mint /3)]`:
- The envelope signature is **Ed25519**. Its seed comes from G5 derivation with domain separation:
  `seed = HMAC-SHA256(AXON_APPROVAL_KEY, "axon-approval repair-envelope v1")`, using the same `axon_attest::hmac_sha256`
  construction as `ApprovalKey::derive` (`approval.rs:59-70`).
- The runner holds only the **public key**: `axon-intent authorize-repair --export-verify-key` writes it once.
- Consequences:
  - self-minting by the runner is cryptographically impossible;
  - the runner never holds the master key, so it cannot derive the `/3` token subkey or the axon-os record subkey.
- `ring 0.17` is already a workspace dependency (`crates/axon-loop-contracts/Cargo.toml:12-14`, Ed25519 attestations).
- This reading of "G5 key derivation" is confirmed with the human in §10 Q1.

## 4. Schema `axon-approval/4` (repair envelope)

**Schema id.** `/2` is reserved (R24) and `/3` is in use, so the new schema is `/4` (`approval.rs:117-126`).
- `verify_approval` refuses `/4` with "an axon-approval/4 repair envelope is not a program approval".
- The envelope verifier refuses `/1` and `/3` the same way.

**Canonical form** `[REVISED: F12, D13 — cl22 lives in axon-loop-contracts, which depends on axon-cortex: a crate cycle]`:
- The cl22 writer and strict parser (`crates/axon-loop-contracts/src/canonical.rs:38-217`) move to a new leaf crate
  `axon-canonical`. `axon-loop-contracts` re-exports them (clean cutover; existing cl22 KATs must stay byte-identical).
- Verification order:
  1. strict parse (duplicate keys, floats refused);
  2. remove `sig`;
  3. `canonical_bytes`;
  4. Ed25519 verify;
  5. only then parse into the typed form, with `deny_unknown_fields` on every nested struct (closes D-026 for `/4`).
- Optional fields are always present, `null` allowed and absence refused, so there is one canonical form.
- `envelope_digest = "cl22:" + sha256(canonical_bytes(envelope incl. sig))` `[REVISED: F13]`.

| Field | Meaning / binding |
|---|---|
| `schema` | `"axon-approval/4"` |
| `envelope_id` | 32 lowercase hex, OS RNG at mint; **run identity**, single-use (§5.5) |
| `spent_store_id` | the id of the runner's spent store (§5.5) `[REVISED: F7]` |
| `issued_by` | operator label (recorded, not trusted) |
| `issued_unix`, `expires_unix` | validity window; `expires − issued ≤ MAX_TTL_S` (86 400, an engineering default `[REVISED: D-C, was Q2]`) |
| `task.workspace` | canonical absolute path |
| `task.file` | workspace-relative, normalized, no `..`, no symlink component (F2) |
| `task.symbol` | required |
| `task.pristine_sha256` | sha256 of the entry file bytes at mint |
| `task.imports_digest` | the import-closure digest `/3` binds (`crates/axon-os/src/runtime.rs:976-977`) `[REVISED: F5, D7]` |
| `write_prefixes` | cortex's patch authority: normalized relative paths, non-empty, each ⊆ workspace, none `*`, no symlink at or under them |
| `scopes` | `{fs_read:[..], fs_write:[..], net:[..]}`: the check child's pins for every kind the pristine program USES; non-`*`, normalized; `fs_write` disjoint from `task.file`, its import closure and `POLICY_FILES`/`POLICY_PREFIXES`/`POLICY_PATHS` (`runner.rs:77-109`) `[REVISED: F6, F5(d), D4]` |
| `capabilities.fence_sha256` | `Fence::sha256()` (`fence.rs:226-229`) of the pinned fence `cortex grant --envelope-preview` emits (§5.1). It covers the env map only, without static-grant `targets` `[REVISED: F4]`. |
| `capabilities.target_grant` | the static grant of `task.symbol` (`StaticGrant`, `fence.rs:270`) |
| `check.name`, `check.digest` | §4.1 |
| `budget` | `{max_proposals ≥1, deadline_ms ≥1, check_timeout_ms, candidates, max_steps}` `[REVISED: F8, D14 — steps were unbound]` |
| `generator` | `{kind:"ai", model, provider, endpoint_host}`; the last two are compared against what axon-ai resolves (`crates/axon-ai/src/lib.rs:69-95, 241-255`) `[REVISED: D14]` |
| `decision` | `"authorize-operate"`, the only value |
| `sig` | `"axenv1:" + hex Ed25519 signature over the canonical bytes without `sig`` |

The ranker and audit fields are dropped `[REVISED: D6, D8(b)]`:
- with `--symbol` required, the ranker is never called (`bin/cortex.rs:900-960`);
- unattended forces `--audit off`, since the audit "can change none of them" (`bin/cortex.rs:1173-1175`).

### 4.1 Check digest

`check.digest = sha256(cl22{check_name, checker_exe_sha256, argv_shape:"axon-test-local/1", check_suite_ref|null, fence_sha256, imports_digest})`.

`check_fn_sha256` was dropped `[REVISED: D7]`. The check fn is part of the entry file, which `pristine_sha256` binds.
The bytes are re-verified before every check (§5.4), which also removes the dependency on FU-7.

### 4.2 Keys and domains

| Holder | Material | Use |
|---|---|---|
| Operator / minter | `AXON_APPROVAL_KEY` → `seed` (domain `"axon-approval repair-envelope v1"`) → Ed25519 private key | mint |
| Runner | `--verify-key <file>`: Ed25519 public key (32 bytes, hex) | verify |

The verify-key file:
- is checked like `custodian::check_store` (owner = euid, `mode & 0o077 == 0`, not a symlink);
- lives outside the canonical workspace and every canonical prefix.

`check_store` moves from axon-fabric down into axon-os, and axon-fabric re-exports it `[REVISED: D13]`.

Domains `[REVISED: F15, D12]`:
- axon-os exports `pub const` labels for the token, record, and repair-envelope domains (and R6's, if built). All of
  them enter `crates/axon-fabric/tests/signing_domains.rs` (pairwise-distinct and golden).
- A separate cross-protocol test checks that an envelope signature never verifies as an attestation of any registered
  domain, and the reverse.

## 5. Mint, verify, run

### 5.1 Mint `[REVISED: D5 — the minter had no source for the check digest or the gate]`

`cortex grant --envelope-preview --workspace W --file F --symbol S --check C --write-prefix P… [--read-prefix R…] [--fs-write-scope X…] [--net-host H…] --axon A --generator ai:M`
emits `{pinned fence, fence_sha256, target_grant, check_digest_input, check_digest, imports_digest, gate:[{id,status,basis}]}`.

It computes them with the **same functions** the runner calls at verification.

`axon-intent authorize-repair … --by OP --ttl-s T --out PATH`:
1. execs that preview;
2. refuses unless every gate row is `admit`;
3. records the sha256 of the cortex binary it invoked;
4. writes `<store>/<envelope_id>.issued` (§5.5);
5. writes the envelope with `create_new`.

### 5.2 Flags in unattended mode `[REVISED: F8 — the no-widen table was a denylist]`

`cortex repair --unattended --envelope E --verify-key K --spent-store D` accepts ONLY:
- `--workspace --file --symbol --check --generator --write-prefix`
- `--max-proposals --deadline-ms --check-timeout-ms --candidates --budget`
- `--axon --json`
- the four unattended flags above.

Any other flag → exit 29.

| Input | Rule |
|---|---|
| `--symbol` | required |
| `CORTEX_RANKER`, `CORTEX_AUDIT`, `CORTEX_LABEL_LOG`, `CORTEX_MODEL`, `AXON_CORTEX_GENERATOR_TIMEOUT_MS` | refused if set (`rank.rs:42`, `audit.rs:47`, `label.rs:23`) |
| `principal` | `envelope:<envelope_id>` |
| `write_prefixes` | CLI ⊆ envelope (component-wise, after normalization) |
| `max_proposals`, `deadline_ms`, `check_timeout_ms`, `candidates`, `--budget` | CLI ≤ envelope; an absent CLI value takes the envelope's |
| generator | equal, including the resolved provider and endpoint |

### 5.3 Verification `[REVISED: F4, D6 — children ran before verification; static grants are derived at ~:969, not :721]`

**Phase A** runs immediately after argument parsing, before the generator is built (`bin/cortex.rs:~686`) and before
`fabric_executor` (:716):

| # | Check |
|---|---|
| 1 | flag allowlist (§5.2) |
| 2 | parse + schema |
| 3 | verify-key file perms/location |
| 4 | signature |
| 5 | time: `issued ≤ now + 60 s`, `now < expires`, TTL ≤ max, `now + deadline_ms + HARD_GRACE ≤ expires` |
| 6 | §5.2 limit comparisons |
| 7 | `resolve_checker`'s sha256 of `--axon` = `checker_exe_sha256` **before** any spawn of it |
| 8 | task: canonical workspace and file, symlink walk (F2), read the entry file ONCE → sha = `pristine_sha256` |

**Phase B** runs after `derive_fence` (the only child before it is the pinned interpreter's static `axon caps`) and
before `fence_conflicts` (:738):

| # | Check |
|---|---|
| 9 | pinned fence = `fence_sha256`; target static grant = `target_grant`; `imports_digest`; `check.digest` |
| 10 | enforcement gate §6 |
| 11 | consume (§5.5) |

Every refusal:
- exits **29 `unauthorized`** `[REVISED: D1 — 28 is out_of_scope_suspected, bin/cortex.rs:1540-1541]`;
- writes nothing;
- starts no child that executes workspace code (check, generator).

A3 fixes the exit table at `bin/cortex.rs:20-28` to list 28 and 29.

**Build without support** `[REVISED: D15]`. Feature `unattended = ["dep:axon-os", "dep:axon-canonical", "dep:ring"]`.
In a build without it, `--unattended` exits 29 with "built without unattended support".

### 5.4 TOCTOU guards `[REVISED: F5]`

- The phase-A bytes are the walk's `original`. The re-read at `bin/cortex.rs:964` asserts the same sha, else exit 29.
- Before every hidden-check dispatch, cortex re-hashes:
  - the bytes about to be graded, outside the patched symbol span: the region outside the target symbol must equal
    pristine;
  - the import closure.
  A mismatch stops the walk (exit 29, restore per E5).
- The check child's FS_WRITE pins (`scopes.fs_write`) are disjoint from the task file, its imports and the policy paths,
  checked at mint and at step 9. `write_prefixes` is cortex's authority, never the test program's.

### 5.5 Single use `[REVISED: F7 — a fresh --spent-store re-admitted a spent envelope]`

The NonceStore semantics are kept whole (`crates/axon-fabric/src/observer.rs:29-185`):
- `<store>/store-id` is written once at store init; `spent_store_id` is bound in the envelope.
- The minter writes `<envelope_id>.issued`.
- Consume renames `.issued → .used` and records `{envelope_digest, spent_unix, pid}`, then fsyncs the file and the
  directory.

| Case | Result |
|---|---|
| unknown id | "never issued" |
| second use | "already used" |
| store-id mismatch | refusal |
| fresh empty store | refusal (no `.issued`) |

Consume is the last check, so a refused envelope stays unspent.

**Honest limit:** the store is same-uid. It holds only because §6 refuses every route by which untrusted code escapes
confinement (E4, E9, E11, F2).

`envelope_id` joins the episode id: `cortex-repair:{envelope_id}:{path}:{symbol}:{check}`, so it is unique per run.

### 5.6 Output

`cortex-repair/1` gains `authorization: {schema, envelope_id, envelope_digest, issued_by, expires_unix, enforcement:[{id,status,basis}]}`
in unattended mode only.

### 5.7 Authority, not acceptance

- Acceptance is the existing hidden-check and adjudication verdict.
- No `/3` token and no `.approved` record is produced.
- Test S12: an `axon-os` job with `require_approval = true` built from the repaired file is refused until a human mints
  `/3`.

## 6. Enforcement gate

Rules for the gate as a whole:
- A route whose limit cannot be enforced is **refused**. No row admits on trust.
- Phase-B rows are evaluated from data cortex holds before any untrusted code runs.
- Rows marked *build* are build-order prerequisites, not runtime checks `[REVISED: F9, D10]`.

| Id | Limit at risk | Today (anchor) | Rule (v1) |
|---|---|---|---|
| E1 | signed fs/net scopes (O-S1-03) | a used kind is left UNSET = unrestricted (`fence.rs:202-207`, test :394-403) | the fence takes the envelope `scopes` as pins for used kinds (`Fence::with_scope_pins`, A4); unused kinds stay EMPTY; `basis != Unknown`, `applied`; a used kind without a pin → refuse |
| E1s | symlink escape (F2) | scope and authority checks are lexical (`capabilities.rs:233-243`, `runner.rs:2549-2567`); writes follow links | refuse if `task.file`, any of its ancestors in the workspace, or anything at or under a write prefix or scope pin is a symlink (lstat walk, phase A/B); key and store paths compared after `canonicalize`. Resolve-beneath in the interpreter is a later item; until then this refusal stands. |
| E2 | Fabric fencing (O-S1-05) | Fabric sets `fence.not_applied()` (`bin/cortex.rs:716-726`) | `--fabric-*` is not in the allowlist → refuse |
| E3 | hard deadline (O-S1-07) | in-flight `ai:` up to 30 s past the deadline (`ai.rs:62-76`); checks not cut (USAGE :88-90); the audit is gated only at start (:1180-1200) | watchdog (below) |
| E4 | native-module scopes (O-S1-09) | native calls pass `scope_args = None` except `*_connect` (`eval.rs:1218-1235`); `fix` has an empty effect row | refuse if pristine caps contain any `native:` label (recorded by `Reach::note`, `capabilities.rs:633-636` at 7f110693 `[REVISED 2026-10-19: re-anchored from :577-606, found by the capability-surface draft]`). No interpreter strict mode in v1 `[REVISED: D9, F9 — strict env fails open on an older pinned axon]`. |
| E5 | foreign edits (O-S1-13) | 9 write/delete sites, FU-4 covers 3 | *build:* NH-R1 lands after FU-4, and every site goes through one CAS write helper (below) `[REVISED: F10, D10]` |
| E6 | cache writers bypassing scopes | ungated: AI replay cache, `run_start`, `AXON_DUMP_BINDINGS`, audit ledger, `AXON_RECORD` | the check child env is **exactly** the fence env plus the completion-key mechanism (`env_clear`, `runner.rs:3619-3635`), asserted in `registered_runner`. No `HOME`/`XDG_*`, so the provenance and dstore writers find no cache root (`provenance.rs:13-19`, `builtins.rs:403-411`); dstore files stay fail-closed (exit 22, O-S1-06). `[REVISED: F14, D11 — dropped the XDG_CACHE_HOME clause]` |
| E7 | guest/VM, AOT | MMDS has no scope fields (`firecracker.rs:80-91`) | only `LocalInterpreterExecutor` (`runner.rs:3404`); `--check-registry` is not in the allowlist → refuse |
| E8 | program sandboxes | inner sandboxes compare effects only (`builtins.rs:4318-4426`); ambient re-checked (:697-716) | not a bypass of the ambient (signed) scope; regression tests for write, read and net (`ambient_scope.rs:217-238` pattern) |
| E9 | untrusted processes holding authority | a `cmd:` generator is an arbitrary same-uid process | `ai:` only |
| E10 | non-UTF-8 ceiling (O-S1-09 narrow) | — | **dropped for the cortex route** `[REVISED: D10]`: the fence always sets the value as UTF-8 and `env_clear` drops inherited values (`runner.rs:3627-3632`). FU-5 covers direct interpreter use. |
| E11 | unscoped process spawn `[REVISED: F1, D2 — blocker]` | `exec` is gated only by the `Exec` effect (`builtins.rs:640-648, 1332-1356`); no scope (`builtins.rs:252-263`) | refuse if pristine caps contain `exec`, the effects contain `Exec`, or any module effect has `Exec`; candidate additions are already refused by `StaticGrant::excess` (`fence.rs:300-330`) |

### E3 watchdog `[REVISED: F11, D8]`

- A thread armed at step 11. The main thread holds the run's write lock (E5) across every write.
- At `started + deadline_ms` the watchdog:
  1. takes the lock;
  2. kills the process groups of the running check and generator;
  3. runs the FU-4 restore table through the CAS helper;
  4. emits the verdict (and R6's terminal record, if built);
  5. calls `_exit(20)`.
- An in-flight `ai:` HTTP call dies with the process, so no `axon-ai` API change is needed.
- After the bound, the only write is the CAS restore of the found bytes. No generator or check result is applied.
- `HARD_GRACE` bounds lock wait + restore + report. `[DECIDED by human 2026-10-19 (§10 Q4): not fixed in advance —
  set from S10's measured tail (p99 of lock wait + restore + report over the stall cases, run under the host's real
  load, not on an idle machine) plus a stated margin; the measured value and its load are recorded with S10.]`
- Every child (checks, per-proposal `axon caps`) is also bounded by `min(configured, remaining)`.

### E5 write helper `[REVISED: F10, D10]`

All workspace writes and deletes go through one helper:

| Site | Anchor |
|---|---|
| apply | `runner.rs:1045` |
| compile revert | `runner.rs:1790` |
| visible copy | `runner.rs:2386` |
| restore | `runner.rs:2767` |
| between-attempt restore | `bin/cortex.rs:1042` |
| exit restore | `bin/cortex.rs:1162` |
| exit-27 put-back | `bin/cortex.rs:1168` |
| settle | `bin/cortex.rs:1591` |
| sweep deletes | `runner.rs:1148-1150` |

The helper:
1. writes a temp file in the same directory;
2. re-hashes the target against the last digest cortex wrote or read;
3. renames;
4. checks `(dev, ino)` afterwards;
5. re-hashes the renamed file and compares it with the bytes it wrote. `[DECIDED by human 2026-10-19 (§10 Q4)]` A
   mismatch means a writer got in during the residual window: the run reports it (exit 22, both digests named) instead
   of claiming the write. A lost foreign edit is then visible, never silent.

A step-2 mismatch writes nothing; any mismatch stops the walk and exits 22 with the reason.

Supporting rules:
- An exclusive `flock` on a store-side lock keyed by the canonical file path means a second unattended run on the same
  file → exit 29.
- The visible copy goes to a private temp directory.
- **Residual window:** a non-cooperating writer between re-hash and rename. `[DECIDED by human 2026-10-19 (§10 Q4):
  accepted, on condition of step 5's post-rename check; POSIX has no atomic compare-and-rename.]`

## 7. Security and correctness tests (all must pass before §9)

Each test is fail-first where it guards a behaviour.

| # | Test |
|---|---|
| S1 | KATs: seed derivation, Ed25519 public key from seed, signature over a fixed envelope; the cl22 bytes of the KAT envelope equal `axon_loop_contracts::canonical_bytes` (re-export parity) |
| S2 | Altering each field → refused (table-driven over every field incl. nested); duplicate key; float; unknown field |
| S3 | Wrong verify key; an `/3`-shaped or HMAC "envelope"; the runner key type cannot mint (it holds a public key only: type-level, no signing method) |
| S4 | Expired; not yet valid; TTL above max; deadline outliving expiry |
| S5 | Replay: second use; fresh empty store; foreign `store-id`; 2–8 concurrent runs → exactly one proceeds (assert counts, not order); refused envelope stays `.issued` |
| S6 | Out of scope: workspace, file, symbol, pristine sha, imports digest, a wider write prefix, a larger budget or step count, another generator/provider/endpoint, changed checker sha, changed fence, changed target grant; each non-allowlisted flag; each `CORTEX_*` env selector |
| S7 | Cross-protocol: `/4` to `verify_approval`; `/3` to the envelope verifier; envelope signature vs every registered signing domain |
| S8 | No self-authority: a bad-signature envelope with `--axon` pointing at a canary script → the canary never runs; the child env dump equals the fence env exactly; envelope file byte-identical after the run; the spent dir gains exactly one rename `[REVISED: D3 — the cargo-metadata test was tautological]` |
| S9 | Key and store perms or ownership wrong, or a symlinked store → refused |
| S10 | Every E-row: refused case and admitted case. Specific cases: symlink under a write prefix / symlinked `task.file` / symlink to the key; E3 with a stalling fake `AXON_AI_BASE_URL` endpoint and a sleeping check (`deadline_ms ≤ 3 s`); the process exits by `deadline + HARD_GRACE` with found bytes restored; E5 with a concurrent writer and with two unattended runs on one file; E11 with an `exec`-using pristine file; E4 with a `native:gfx` file; E1 with read- and net-using files, admitted with pins and refused without |
| S11 | Attended mode unchanged: existing suites; `cortex-repair/1` byte-identical on a pinned task |
| S12 | Acceptance ≠ approval (§5.7) |
| S13 | TOCTOU: a pristine test rewrites the entry file or an imported module during the witness run → exit 29 |

g01 rows cover:
- the signature compare;
- every time compare;
- consume `rename`;
- every no-widen compare;
- every E-row refusal;
- the CAS re-hash.

## 8. Build plan (when authorized)

Ordering `[REVISED: D10]`: lands after FU-4 (E5). FU-7 is no longer needed (D7).

| Unit | Content | Stages (beyond the base lane) |
|---|---|---|
| A0 | `axon-canonical` leaf crate; `check_store` moved into axon-os | `cargo test -p axon-loop-contracts -p axon-fabric`; existing cl22 KATs byte-identical |
| A1 | `axon-os::repair_envelope` (types, verify, consume, store init); exported domain consts; S1–S4, S7, S9 | `cargo test -p axon-os`; `-p axon-fabric --test signing_domains` |
| A2 | `axon-intent authorize-repair` and `--export-verify-key`; `cortex grant --envelope-preview` | `cargo test -p axon-intent` |
| A3 | cortex `--unattended` (feature `unattended`), phases A/B, exit 29, output, allowlist; S5, S6, S8, S11–S13 | full cortex lane |
| A4 | E1 pins, E1s, E3 watchdog, E5 helper (all 9 sites), E6 assertion, refusals E2/E4/E7/E9/E11, E8 regressions; S10 | full cortex lane |
| A5 | Docs: USAGE, README, the `bin/cortex.rs:20-28` table, CHANGELOG; opportunities | `check_docs` |

Every `crates/*/src` commit gets:
- a pci theme, then the regen, then `pci_delta --check`;
- refusal-coverage anchors for each new refusal string;
- g01 rows in the same commit.

**Cost budget** (D §D):

| Check | One run | Scarce resource | Re-run trigger | Skip rule |
|---|---|---|---|---|
| A0–A2 unit tests | 1–3 min CPU | — | crate change | none |
| cortex lane | 10–15 min | serial slot on cortex files | each A3/A4 commit | docs-only |
| S10 E3 stall | about `deadline + HARD_GRACE` per case | CI wall | runner timing change | none |
| full gate | 27–32 min | integrated-head slot | any later commit | once at the head |
| paid receipt regress | ≈ $0.14 | money, quota | repair-digest change | once per integrated head; S11 byte-identity first |

## 9. Enabling

All of the following must hold, and then a human records the enabling decision (§10 Q3). Until then `--unattended`
**refuses by default**.
- All S-tests green and the full gate green.
- A read-only adversarial security review of A0–A4, with every finding dispositioned.
- R6 L1–L3 built, and its C1 audit is clean over the S-test runs and a dry-run series of unattended runs
  `[REVISED: F-R6, D17 — this makes C1's consumer real]`.

## 10. needs-human

| Q | Question | Default in this DRAFT |
|---|---|---|
| Q1 | The signature: Ed25519 seeded by G5 derivation (runner holds a public key; self-minting impossible) vs pure HMAC (runner holds a mint-capable subkey) | Ed25519-from-G5 |
| Q2 | v1 admission set: refuse files using `exec`, native modules, dstore; `ai:` generators only; fs-read/net admitted only with signed pins | as stated |
| Q3 | Who records the enabling decision after §9; is a per-host opt-in file also required? | human decision; refused by default |
| Q4 | Accept the residual CAS window (E5) and `HARD_GRACE` = 2 s as meeting "never overwrite another actor's edits" and "hard deadlines"? | yes, with both measured in S10 |

**Decisions recorded 2026-10-19** (`[DECIDED by human 2026-10-19: the recommendations below]`). They settle the
design questions only. Building NH-R1 still needs a separate human go-ahead, and enabling it still needs §9.
- **Q1 → Ed25519 seeded from G5.** An HMAC verifier can mint, so the runner could approve itself. R6 depends on this
  choice.
- **Q2 → the v1 admission set as stated.** Each refused item is a confinement escape this spec has no defence for
  (E4, E9, E11). Widening later is cheap; narrowing after an incident is not.
- **Q3 → a human records the enabling decision, AND each host needs its own opt-in file.** The decision says the
  build is fit; the host file says this machine agrees. The host is shared (other users' jobs held load ≈ 48 during
  the follow-up run), so one repo decision must not enable every checkout on every host. The opt-in file's path,
  owner/mode check and refusal string are A-phase build items (§8) and get an S-test.
- **Q4 → accept the residual window with E5 step 5's post-rename check; `HARD_GRACE` from S10's measured tail under
  real load, not a fixed 2 s.** The follow-up run's receipt flipped FAIL → PASS on wall time between two load levels,
  so a bound measured on an idle machine is not evidence.
