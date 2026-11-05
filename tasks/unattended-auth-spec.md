# Unattended repair authorization (NH-R1) — DRAFT spec (after Step 1 review)

**Status: PARTIAL 2026-10-31 on nhr/int: security fixes merged (F1-F12 fixed in 91114bcc..dcd60815 and be47b23e; A0-A4 and the §7 S-tests: 1093c268, 9a825ace, c4f4c12c, 219e1360, 9ed0ad16, 60651b37). Re-review N1-N4 fixed (nhr/fix-c 5e453868, `[REVISED: security re-review N#]` markers; N3 per the human ruling: attended runs take the §10 Q4 rename, unattended runs refuse); N5/N6 and N3 portability are recorded in O-NHR-06. Re-review 2 fixed on nhr/fix-d (`[REVISED: security re-review 2 N#]` markers): N1 scheme pin (https only), N7 initial user namespace, N8 exclusive Q4 placement. N1-N4 verified 2026-11-05 at ab2a650a (g01 M4216-M4218 and M4520-M4552 all killed, `/var/tmp/r51-tmp/b4-probe/b4-g01.json`; probes in `tasks/nhr-evidence-spec.md` §8). It stays PARTIAL in the sense that enabling is not authorized: `--unattended` refuses by default (exit 29); enabling needs the §9 evidence and a separate human decision.**
- `[DECIDED by human 2026-10-31: build go-ahead.]` Building it (A0-A4, E1-E7, §7 S-tests) is authorized.
- Enabling it after the build (§9) still needs the §9 evidence (S-tests and full gate green, a read-only adversarial
  security review, R6 L1-L3 built with a clean C1 audit) and then a second, separate human decision made on that evidence.

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
| `run_ledger_id` | the `ledger-id` of the runner's R6 run ledger (`tasks/r6-ledger-spec.md` §4), 32 lowercase hex `[REVISED: build 2026-10-31 (NhrCrates, R6 L1): added as R6 §4 announced; the axon-approval/4 KATs were recomputed independently in Python]` |
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
| `generator` | `{kind:"ai", model, provider, endpoint_host}`; the last two are compared against what axon-ai resolves (`crates/axon-ai/src/lib.rs:69-95, 241-255`) `[REVISED: D14]`. `[REVISED: security re-review 2 N1]` only the host was compared, so `AXON_AI_BASE_URL` (or `ANTHROPIC_BASE_URL`) `http://<pinned host>` sent the request and its API key in plaintext; phase A now refuses (exit 29) an endpoint that is not `https://` (`axon_ai::endpoint_is_https`; S6 `s6_a_plain_http_endpoint_is_refused`: a plain-HTTP listener at the pinned host gets no connection) |
| `decision` | `"authorize-operate"`, the only value |
| `sig` | `"axenv1:" + hex Ed25519 signature over the canonical bytes without `sig`` |

`[REVISED: security F4 — a relative pin resolved against the check child's cwd and skipped the phase-A symlink walk
and policy check]` Every `scopes.fs_read` / `scopes.fs_write` entry is a normalized **absolute** path. `validate`
(mint and verify, `crates/axon-os/src/repair_envelope.rs`) refuses a relative one with `scopes.<kind> entry "<pin>" is
relative: a pin must be absolute (…)`.

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

`axon-intent authorize-repair … --spent-store D --run-ledger L --verify-key K --run-record-key R --by OP --ttl-s T --out PATH`:
1. execs that preview, with `AXON_APPROVAL_KEY` removed from the child's environment `[REVISED: security F12]`;
2. refuses unless every gate row is `admit`;
3. records the sha256 of the cortex binary it invoked;
4. writes `<store>/<envelope_id>.issued` (§5.5);
5. writes the envelope with `create_new`.

`[REVISED: security F6 (mint side) — a pin over the spent store made a spent envelope replayable; one over a key file
let the check child replace the verify key or read the run-record key]` Before signing, the minter refuses (exit 2)
any `fs_write` or `fs_read` pin that overlaps (either lies within the other, component-wise) the canonical spent store,
`--verify-key` file or `--run-record-key` file, and any `fs_write` pin overlapping the run ledger (as phase A refuses
it). A pin is compared as written and through the canonical form of its deepest existing ancestor, so a symlinked pin
cannot hide. Helper: `repair_envelope::refuse_pin_overlap`; message `the <what> <path> overlaps scopes.<kind> pin
"<pin>": the check program could reach it`. The phase-A side is a cortex build item.

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
| `CORTEX_RANKER`, `CORTEX_AUDIT`, `CORTEX_LABEL_LOG`, `CORTEX_MODEL`, `AXON_CORTEX_GENERATOR_TIMEOUT_MS`, `AXON_AI_MOCK`, `AXON_AI_REPLAY`, `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` (either case) | refused if set (`rank.rs:42`, `audit.rs:47`, `label.rs:23`); never read afterwards. `[REVISED: security F2]` no `.env` is loaded: phase A consumes `axon-ai`'s dotenv once first (`axon_ai::forbid_dotenv`; `Err` if a lookup already ran -> exit 29), so neither `./.env` nor `$AXON_DOTENV` can set a variable the refusal never saw (S6 `s6_a_dotenv_steers_nothing`). `[REVISED: security build]` `AXON_AI_MOCK` and `AXON_AI_REPLAY` in the REAL environment refuse too (`unattended::REFUSED_ENV`): E6 scrubs only the check child's env, and the `ai:` generator runs in cortex's own process, where `AXON_AI_MOCK` answered it from the mock stub instead of the pinned endpoint (S6 `s6_a_real_env_model_override_is_refused`). `AXON_AI_REPLAY` is read only by the interpreter's `ai_complete`; no cortex child that executes code inherits the env (the check child and the static verbs `env_clear`; `axon check` inherits it but only type-checks), so it steered nothing observed. It is refused anyway, since an answer cache is not the pinned endpoint. `[REVISED: security re-review N1]` the proxy variables refuse too: `axon-ai`'s `reqwest` client reads them from the process env, so with the envelope's endpoint host pinned, `HTTP_PROXY` sent the model call - API key included - to a proxy nobody signed, which could answer with any patch (S6 `s6_a_proxy_variable_is_refused`; probed with a local listener and a dummy key: it received `POST http://api.anthropic.com/v1/messages` with `x-api-key`). Refused rather than ignored with `no_proxy()`: ignoring would change the attended client (gateway deployments behind a proxy) and run a route the operator did not ask for; refusing tells a host that needs a proxy why it never runs unattended. Not refused: `AXON_AI_PROVIDER` / `AXON_AI_BASE_URL` / `ANTHROPIC_BASE_URL` (they resolve the endpoint, which must equal the envelope's, row "generator"), the API keys, and `AXON_AI_MODEL_*` (tier defaults; the generator always passes the pinned model explicitly, `ai.rs:85`) |
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
| 7 | `resolve_checker`'s sha256 of `--axon` = `checker_exe_sha256` **before** any spawn of it. `[REVISED: security F7]` `--axon` is resolved ONCE (`runner::resolve_executable`, a bare name through PATH as the spawn would); that file is hashed, and the registry pins that file with that sha256 (`register_expected`), so the bytes bound are the bytes run (S-test `s6_a_bare_axon_runs_the_binary_that_was_hashed`) |
| 8 | task: canonical workspace and file, symlink walk (F2), read the entry file ONCE → sha = `pristine_sha256` |
| 8a | `[REVISED: security F4]` every `scopes.fs_read` / `scopes.fs_write` pin is absolute (`unattended::absolute_fs_pins`; the preview refuses one too): a relative pin resolves against the check child's cwd (the workspace) and skipped the symlink walk and the policy check |
| 8b | `[REVISED: security F6]` the canonical spent store, verify-key file and seal-key file are disjoint from every `fs_read` / `fs_write` pin, at, above or below (`unattended::outside_fs_pins`); the minter's counterpart is axon-intent's |
| 8c | `[REVISED: security re-review N4]` every `fs_read` / `fs_write` pin - inside the workspace or not - is walked for symlinks from `/`, at it and under it (`unattended::no_symlink_in_fs_pins`; the preview walks the same): only pins inside the workspace were walked, so a symlink under an outside write pin, made after minting, reached the run ledger or the spent store past 8b's canonical-path comparison (S6 `s6_a_pin_reaching_the_ledger_or_store_through_a_symlink_is_refused`, preview `a_symlink_under_an_outside_pin_produces_no_preview`). The run ledger is compared with `outside_fs_pins` like the store (canonical path, write pins) |

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
`[REVISED: implementation audit]` That refusal (and `grant --envelope-preview`'s) comes before any argument is parsed, so no usage error (exit 2) answers it first; test `cli::a3_the_default_build_refuses_unattended_before_parsing`.

### 5.4 TOCTOU guards `[REVISED: F5]`

- The phase-A bytes are the walk's `original`. The re-read at `bin/cortex.rs:964` asserts the same sha, else exit 29.
- Before every hidden-check dispatch, cortex re-hashes:
  - the bytes about to be graded, outside the patched symbol span: the region outside the target symbol must equal
    pristine;
  - the import closure.
  A mismatch stops the walk (exit 29, restore per E5).
- The check child's FS_WRITE pins (`scopes.fs_write`) are disjoint from the task file, its imports and the policy paths,
  checked at mint and at step 9. `write_prefixes` is cortex's authority, never the test program's.
- `[REVISED: security F5 - built]` Neither guard existed: disjointness checked the task file and policy paths only,
  and the closure was re-hashed once, after the witness run. Now `auth::fs_write_disjoint` takes the canonical module
  paths of `axon imports --json` (`auth::imports_closure`, the same call at preview and at step 9), and
  `auth::DispatchGuard::check` runs before EVERY check dispatch (`Runner::set_dispatch_guard`: the fence-conflict run,
  the witness run and every check of the walk). A mismatch restores the found bytes through the CAS helper if this
  run wrote the file, writes the terminal record and exits 29. "Outside the symbol span" is the pristine bytes before
  and after `runner::symbol_body`'s span (exactly what `splice_body` keeps). S-tests
  `s6_an_fs_write_pin_over_an_imported_module_is_refused`, `s13_an_import_changed_by_a_check_stops_the_next_dispatch`,
  and `s13_a_checker_that_rewrites_the_entry_file_during_the_witness_run_refuses` (outside / body cases).

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
| E12 | durable store `[REVISED: security F11]` | `dstore_*` writes a file under the cache dir whose path comes from a store key (`capabilities.rs:148-152`); blocked only because the child gets no `HOME` (E6) | refuse if pristine caps contain `dstore` (§10 Q2), its own gate row (`unattended::caps_rows`, test `e12_a_dstore_file_is_refused`) |

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
- `[REVISED: security F8 - built]` The watchdog is armed right after phase B (`bin/cortex.rs`, before the
  fence-conflict and witness runs, which execute workspace code); every static child before it (`derive_fence`'s and
  phase B's `axon caps`, `axon imports`) is bounded by `min(check_timeout, remaining)`, asked per child; every child
  after it is killed at the deadline. It kills before it waits for the lock. A second, lock-free timer at
  `max(deadline, firing) + 3/4 HARD_GRACE` kills, prints the "not restored" report (no terminal record: the journal
  is behind the lock) and calls `_exit(20)`, so a write lock that is never released (a stalled CAS read) cannot
  stretch the bound, and a restore that never ran is never claimed. A run that wrote nothing reports "was not
  written" instead of a restore. S10 cases: `s10_e3_a_held_write_lock_still_exits_by_the_grace`,
  `s10_e3_a_watchdog_armed_late_still_restores`, `s10_a_witness_run_stalled_past_the_deadline_is_stopped_by_the_grace`,
  `s10_a_static_child_stalled_before_phase_b_is_bounded_by_the_deadline`. CAS reads open `O_NOFOLLOW|O_NONBLOCK`
  and require a regular file (commit 947de502).

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

The helper `[REVISED: security F3, F9, F8 - a plain rename lost an edit made between step 2 and step 3 silently (step 5
re-hashed this run's own inode); a delete unlinked an edit made after its re-hash; a FIFO or a symlink in the target's
place stalled the re-hash under the write lock]`:
1. writes a temp file in the same directory;
2. re-hashes the target against the last digest cortex wrote or read;
3. exchanges the temp file and the target atomically (`renameat2(RENAME_EXCHANGE)`; `RENAME_NOREPLACE` when the target
   is expected absent), so the old inode moves to the temp name instead of being unlinked;
4. re-hashes that displaced inode against step 2's digest. A mismatch means a writer got in between steps 2 and 3:
   the displaced file is **kept** beside the target (`.cortex-cas-displaced-*`) and the write is reported (exit 22,
   both digests and the kept path named). The foreign edit is never lost and never silent.
5. checks `(dev, ino)` and re-hashes the placed file against the bytes it wrote: a write after the exchange is reported
   (exit 22, both digests named), never claimed.

A delete renames the target to a temp name, re-hashes what it moved, and unlinks only on a match; otherwise it renames
it back (or keeps it and names the kept path) and reports. Every read opens with `O_NOFOLLOW|O_NONBLOCK` and refuses
anything but a regular file.

A step-2 mismatch writes nothing; any mismatch stops the walk and exits 22 with the reason.

`[REVISED: security re-review N3, human ruling 2026-10-31]` steps 3 and the delete's rename need `renameat2` flags. A
filesystem without them answers EINVAL (ntfs3 does; ext2, vfat and tmpfs on this kernel support both), and a platform
other than Linux has no `renameat2` at all (the call is behind `cfg(target_os = "linux")`).
- **Unattended runs** refuse the write or delete (exit 22) with `the filesystem does not support
  renameat2(RENAME_EXCHANGE); the compare-and-swap write is refused and no plain rename replaces it`, naming the flag;
  the target keeps its bytes and no temp file is left (`cas::tests::a_filesystem_without_exchange_refuses_clearly`).
- **Attended runs** take the human-approved §10 Q4 procedure instead: step 2's re-hash against the last digest (an
  expected-absent target is re-checked absent just before the rename), a plain `rename(2)`, then step 5's re-hash of
  the renamed file against the bytes written; a mismatch is reported with both digests (exit 22), never claimed. A
  delete renames to a temp name, re-hashes and unlinks the same way, renaming a mismatch back. What it does NOT
  guarantee: there is no displaced inode for step 4 to re-hash, so an edit made through the path between step 2 and
  the rename is overwritten unreported - the residual window Q4 accepted. On an ntfs3 loop mount the attended repair
  of `broken.ax` exits 0 with the file repaired, as the base build (`d7690e63`) did
  (`cli::n3_an_attended_repair_on_ntfs3_repairs_the_file`); the fallback's own checks are the `cas::tests::*q4*` tests.
  `[REVISED: security re-review 2 N8]`: "re-checked absent, then a plain rename" was not exclusive - a file created
  between the check and the rename was overwritten, and step 5 saw only this run's inode, so nothing was reported.
  The same held for the delete's rename-back. Where the target is expected absent (a create, the rename-back) the
  file is now placed with `link(2)`, which refuses an existing target atomically (EEXIST, reported like any
  exclusive-rename refusal, the other file kept), then the temp name is unlinked. A filesystem without hard links
  (EPERM, EOPNOTSUPP, ENOSYS) refuses the write (exit 22, the reason names `renameat2(RENAME_NOREPLACE)` and hard
  links; nothing placed, no temp file left); it never falls back to an overwriting rename. Tests
  `a_file_created_at_the_q4_placement_is_not_clobbered`, `a_file_created_at_the_q4_rename_back_is_not_clobbered`
  (both inject the file after any check and right before placement) and
  `a_filesystem_without_hard_links_refuses_the_q4_placement`. Replacing an existing target still uses a plain
  rename: that is the Q4 window above.

Supporting rules:
- An exclusive `flock` on a store-side lock keyed by the canonical file path means a second unattended run on the same
  file → exit 29.
- The visible copy goes to a private temp directory.
- **Residual window** `[DECIDED by human 2026-10-19 (§10 Q4): accepted, on condition that it is never silent]`
  `[REVISED: security F3]`: what is guaranteed is that no foreign edit made through the target's path before the
  exchange is overwritten or deleted without a report and a kept copy. What is NOT guaranteed: that the target's path
  holds the foreign edit afterwards (it holds this run's bytes; the edit is in the kept file); and a writer that keeps
  an fd open on the old inode and writes through it AFTER step 4's re-hash writes into an inode no longer linked
  anywhere (step 4 matched, so it was unlinked): that write is lost unreported. Only a path-based writer is covered.

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
| S11 | Attended mode unchanged: existing suites; `cortex-repair/1` byte-identical on a pinned task `[REVISED: implementation audit]` `cli::s11_attended_json_is_byte_identical_to_the_pre_nh_r1_build`: the `--json` record (timing masked) equals a golden file from the pre-NH-R1 build d7690e63 |
| S12 | Acceptance ≠ approval (§5.7) |
| S13 | TOCTOU: a pristine test rewrites the entry file or an imported module during the witness run → exit 29 |
| S14 | Host opt-in (§10 Q3) `[REVISED: security F1, re-review N2, re-review 2 N7]`: no `/etc/axon/unattended-repair`, a wrong owner or mode, a non-regular file, one not naming exactly one decision record, or one under a directory that is not root-owned or that another user can write → exit 29, nothing spent; a run inside a user namespace (`unshare --user --map-root-user`) → exit 29 before the opt-in is read |

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
  `[REVISED: security F1 — built]`: phase A's first step reads `/etc/axon/unattended-repair`
  (`auth::OPT_IN_PATH`) with `O_NOFOLLOW|O_NONBLOCK` and refuses (exit 29, nothing read or spent) unless it is a
  regular file owned by the euid with `mode & 0o077 == 0` whose one non-comment line is
  `decision: <the decision record>`. The refusal starts `this host has not opted in to unattended repair:` and says
  what to write. The decision record named there is reported in the run's `authorization.host_opt_in`. S14
  (`s14_a_host_that_did_not_opt_in_refuses`) covers: no file, mode 0640, another owner, a directory, no decision,
  a line that is not the decision, two decisions. The file is not in the workspace, so the repo cannot write it.
  `[REVISED: security re-review N2]`: "owned by euid" alone let the gated uid opt itself in wherever it could write
  a directory on the path. The file is opened from `/` one component at a time with `O_NOFOLLOW` on every component
  (`auth::open_root_held`); every ancestor must be root-owned and not group/other-writable (a root-owned sticky
  directory is accepted above the parent, never as the parent). The path is a constant: tests reach it through a
  private mount namespace (a tmpfs on `/etc/axon`) or by calling `host_opt_in(path, euid)` directly, never through an
  environment variable. S14 adds an `/etc/axon` other users can write and one uid 1 owns; the unit test
  `an_opt_in_in_a_directory_another_user_controls_refuses` covers a non-root parent, a writable parent, a sticky
  parent and a symlinked ancestor.
  `[REVISED: security re-review 2 N7]`: ownership only means something in the initial user namespace; inside one
  with a uid mapping (e.g. `bwrap --unshare-user --uid 0`) the runner's own files read as root-owned and its own
  opt-in passed. Before the opt-in, phase A refuses (exit 29, nothing read or spent) unless `/proc/self/uid_map` and
  `/proc/self/gid_map` each hold exactly one mapping `0 0 4294967295` (whitespace-normalized; an unreadable map
  refuses) - `auth::initial_user_namespace`, unit test `only_the_initial_user_namespace_map_admits`, S14
  `s14_a_run_in_a_user_namespace_refuses` (skips with a printed reason where user namespaces are unavailable).
- **Q4 → accept the residual window with E5 step 5's post-rename check; `HARD_GRACE` from S10's measured tail under
  real load, not a fixed 2 s.** The follow-up run's receipt flipped FAIL → PASS on wall time between two load levels,
  so a bound measured on an idle machine is not evidence.
