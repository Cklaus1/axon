# Unattended repair authorization (NH-R1) — DRAFT spec

**Status: DRAFT. Unattended execution is NOT authorized by this document.** Building it requires a human go-ahead.
Enabling it after the build (§9) requires a second, separate human decision.

**Source decisions:**
- `tasks/repair-spec.md:211-222` — NH-R1: `[DECIDED by human 2026-10-18: A — build a new axon-approval schema, specified separately before unattended repair is enabled]`.
- The enforcement-gate scope addition relayed on 2026-10-18 (§6).
- `tasks/opportunities.md` O-S1-03 / -05 / -07 / -09 / -13 are the enforcement prerequisites.

**Code facts:** cited at worktree `fu-int` (f26e9506 tree). Mapped in the session's approval map; anchors re-checked
where quoted.

## 1. Problem

Today the operator's command line is the authority for `cortex repair` (`tasks/repair-spec.md:221`). An unattended
run — one no human launches by hand — has no authority source at all.

The per-candidate token `axon-approval/3` (`crates/axon-os/src/approval.rs:117`) is the wrong shape:
- it binds entry bytes, so every patch would need a fresh approval;
- an approval minted automatically is the machine approving itself.

Separately, several routes let a run exceed the limits an operator would sign (`tasks/opportunities.md` O-S1-03, -05,
-07, -09, -13). A valid token over limits nobody can enforce is a false assurance. So this spec gates unattended runs on
**enforcement**, not only on a signature.

## 2. Non-goals

- **Per-candidate approval is unchanged.** `axon-approval/3`, `axon ast approve`, `axon-intent approve`, the H3 flag
  `--program` and Fabric's H2 refusal stay exactly as they are.
- **The envelope accepts no result.** A repair accepted under an envelope still needs the existing human approval to
  deploy (§5.6).
- The following are out of scope:
  - multi-party approval and R24's `axon-approval/2`;
  - USD metering inside the loop (the budget is counted in proposals and time);
  - Fabric unattended (refused, §6 E2);
  - R6 (`tasks/r6-ledger-spec.md`).
- No new authority for `cmd:` generators. Only `ai:` is admitted in unattended mode (§6 E9).

## 3. Threat model

| Actor | Trust | Can |
|---|---|---|
| Operator (key holder) | trusted | mint envelopes |
| `cortex` binary (verifier, walker) | trusted code, same uid as the operator's service account | read the key, verify, run the walk |
| Generator model output (patch text), ranker/audit model output | **untrusted** | choose patch bytes |
| Check program (the file under repair, its tests) | **untrusted** (it runs candidate code) | anything its fence and the ambient sandbox allow |
| `cmd:` generator process (an agent with tools) | **untrusted** | anything the uid can do → excluded in unattended mode (E9) |
| Local filesystem/state store | trusted to the uid | — |

Facts and their consequences:
- **G5 keys are symmetric.** Any key holder can mint (`tasks/gaps-spec.md:154-200`). So "the runner never issues its
  own authority" is enforced in two ways:
  - **structurally**: there is no mint code in any crate `cortex` links (§5.2);
  - **by isolation**: no untrusted actor can read the key or the spent store (E6, E9).
- **Upgrade path (needs-human, §10 Q1):** an asymmetric signature (ed25519) would make self-minting cryptographically
  impossible for the verifier. The human requirement names G5 key derivation, so the DRAFT keeps HMAC.

## 4. Schema `axon-approval/4` (repair envelope)

**Schema id.** `/2` is reserved for R24 and `/3` is in use, so the new schema is `/4`
(`approval.rs:117-126`). `KNOWN_TOKEN_SCHEMAS` gains `/4` only in the envelope verifier. `verify_approval` (program runs)
refuses `/4` with "an axon-approval/4 repair envelope is not a program approval". The envelope verifier refuses `/1` and
`/3` the same way. Both directions get a test: cross-protocol confusion.

**Serialization.** One JSON object, `serde(deny_unknown_fields)`. This closes D-026 for this schema: an unknown field
is a refusal, never dropped. The canonical bytes are cl22 canonical JSON of every field except `mac`
(`crates/axon-loop-contracts/src/canonical.rs`; the same form as `promote.py:101`).

| Field | Type | Meaning / binding |
|---|---|---|
| `schema` | `"axon-approval/4"` | |
| `envelope_id` | 32 lowercase hex (128-bit, OS RNG at mint) | **run identity**; single-use (§5.4) |
| `issued_by` | string | operator label (unauthenticated, as in G5; recorded, not trusted) |
| `issued_unix`, `expires_unix` | u64 | validity window; `expires_unix − issued_unix ≤ MAX_TTL_S` (§10 Q2; DRAFT 86 400) |
| `task.workspace` | canonical absolute path | |
| `task.file` | workspace-relative path, no `..` | |
| `task.symbol` | string | |
| `task.pristine_sha256` | hex | sha256 of the file bytes the envelope was minted against |
| `write_prefixes` | [string], non-empty, each ⊆ workspace, none `*` | permitted write paths |
| `capabilities.fence_sha256` | hex | `Fence::sha256()` (`crates/axon-cortex/src/fence.rs:226-229`) of the fence `cortex grant` derives for this task, **including the pinned scopes of E1** |
| `capabilities.fence` | object | `Fence::to_json` (fence.rs:233-243) as minted, for audit; the digest is authoritative |
| `check.digest` | hex | sha256(cl22 of §4.1) |
| `check.name` | string | hidden check fn name |
| `budget.max_proposals` | u32 ≥ 1 | required |
| `budget.deadline_ms` | u64 ≥ 1 | required; hard (E3) |
| `budget.check_timeout_ms` | u64 | |
| `budget.candidates` | u32 | |
| `generator` | `{kind:"ai", model}` | identity; only `ai:` (E9) |
| `ranker`, `audit` | `{kind, model}` or null | identity, or absent |
| `limits.no_widen` | `true` (constant) | states the rule of §5.3; present so a reader sees it in the token |
| `decision` | `"authorize-operate"` | the only value; not "approve", not "accept" |
| `mac` | `"axenv1:" + hex HMAC-SHA256(envelope subkey, canonical)` | keyed only — there is no unkeyed form |

### 4.1 Check digest input

```
{check_name, check_fn_sha256, checker_exe_sha256, argv_shape, check_suite_ref | null, fence_sha256}
```

| Input | Definition |
|---|---|
| `check_fn_sha256` | sha256 of the check fn's source span in the pristine file. The span is found by the same lexer-aware `symbol_body` scanner FU-7 makes the single one. |
| `checker_exe_sha256` | the sha of the pinned interpreter: `register_pinned` (`crates/axon-cortex/src/bin/cortex.rs:362-392`), or the registry executor's sha |
| `argv_shape` | `"axon-test-local/1"` (runner.rs local check command `<axon> test <ws/rel> --json --filter NAME --completion-key-stdin`) |
| `check_suite_ref` | registry mode only |

### 4.2 Key

`ApprovalKey::derive` (`approval.rs:59-70`) gains a third 32-byte subkey under the domain string
`"axon-approval repair-envelope v1"`, derived by the same `axon_attest::hmac_sha256` construction from
`AXON_APPROVAL_KEY`.

The domain is registered:
- in `crates/axon-fabric/tests/signing_domains.rs` (pairwise-distinct and golden);
- with a KAT next to `approval_key_derivation_known_answer` (`approval.rs:591-607`).

**Key source in unattended mode.** A key FILE `--envelope-key <path>`, checked like `custodian::check_store`
(`crates/axon-fabric/src/custodian.rs:184-210`): owner uid = euid, `mode & 0o077 == 0`.
- The env var is refused in unattended mode: an env var is inherited by children, a file is not.
- The key file and the spent store sit outside every write prefix and outside the workspace. This is checked, and a
  violation is a refusal.

## 5. Mint, verify, consume

### 5.1 Mint

`axon-intent authorize-repair --workspace W --file F --symbol S --check C --write-prefix P… --max-proposals N --deadline-ms D --generator ai:MODEL [--ranker …] --ttl-s T --by OP --envelope-key K --out PATH`.

1. It runs `cortex grant` on the same task to obtain `fence.to_json` and `fence_sha256`. `grant_only` exists at
   `bin/cortex.rs:1705`.
2. It runs the §6 enforcement gate in dry mode and refuses to mint an envelope the runner would refuse.
3. It writes the token with create_new.

The minter lives ONLY in axon-intent. `cortex` does not depend on axon-intent. The structural check: `cargo tree -p
axon-cortex` contains no `axon-intent`, asserted in a test via `cargo metadata`, not via source text.

### 5.2 Verify (in `cortex repair --unattended --envelope PATH --envelope-key K --spent-store DIR`)

**Placement:** a new `axon-os` module `repair_envelope`, so it sits next to `ApprovalKey`. `axon-cortex` gains an
optional dependency on `axon-os`. That optional crate exposes `verify` and `consume`, and has no mint function.

**Verification point:**
- after flag parsing, `derive_fence` and static grants (`bin/cortex.rs:~721-726`);
- BEFORE `fence_conflicts` (`:738`, which already runs workspace tests), the hidden-check witness run (`~:778`), the
  ranker, and any generator call.

Nothing untrusted runs before verification. Checks, in order (first failure wins; one stable message each):

| # | Check | Refusal |
|---|---|---|
| 1 | Parse (`deny_unknown_fields`), schema `/4` | malformed / not an envelope |
| 2 | Key file perms and location | |
| 3 | MAC, constant-time (`axon_attest::constant_time_eq`) | "repair envelope MAC mismatch (altered or wrong key)" |
| 4 | `issued_unix ≤ now + 60 s` skew, `now < expires_unix`, `expires − issued ≤ MAX_TTL_S` | expired / not yet valid / TTL too long |
| 5 | `now + deadline_ms + HARD_GRACE ≤ expires_unix` | the run could outlive its authority |
| 6 | Task: canonical workspace, file, symbol equal; current file sha = `pristine_sha256` | out of scope: task |
| 7 | Limits (§5.3) | out of scope: widened limit `<name>` |
| 8 | Recomputed `fence_sha256` and `check.digest` equal | out of scope: capabilities/check changed |
| 9 | Enforcement gate §6 (E1–E9) | "unattended run refused: limit `<E#>` cannot be enforced on this route: …" |
| 10 | Consume (§5.4) | replayed |

Every refusal is exit **28 `unauthorized`** (new; 20–27 are taken, `bin/cortex.rs:20-28`), writes nothing to the
workspace, and starts no child process.

### 5.3 No widening

In unattended mode the envelope is the ceiling. Each CLI value must be at most the envelope's value; a CLI value that is
absent takes the envelope's.

| Limit | Rule |
|---|---|
| `write_prefixes` | CLI ⊆ envelope; each CLI prefix must lie under an envelope prefix |
| `max_proposals`, `deadline_ms`, `check_timeout_ms`, `candidates` | CLI ≤ envelope |
| generator / ranker / audit | identical |
| `--fabric-*`, `--check-registry` not bound in the envelope, `--label-source`, any flag that changes the check or executor | refused in unattended mode |

The runner never writes, renames or re-signs the envelope. A failed run cannot be resumed: a new run needs a new
envelope.

### 5.4 Single use (replay)

The spent store reuses the semantics of Fabric's `NonceStore` (`crates/axon-fabric/src/observer.rs:29-185`):
- `consume` creates `<DIR>/<envelope_id>.used` with `create_new` (O_EXCL);
- the file holds `{envelope_digest, spent_unix, pid}`;
- the directory is checked like `custodian::check_store`.

It runs as step 10, after every other check, so a refused envelope stays unspent. The second presentation of the same
`envelope_id` fails with "repair envelope `<id>` was already used". Of two concurrent runs, exactly one wins.

`envelope_id` joins the episode id: `cortex-repair:{envelope_id}:{path}:{symbol}:{check}`. Today the episode id is
deterministic (`crates/axon-cortex/src/runner.rs:1142-1145`). In unattended mode it is unique per run.

### 5.5 During and after the run

The `cortex-repair/1` JSON gains `authorization: {schema, envelope_id, envelope_digest, issued_by, expires_unix,
enforcement: [{id, status, basis}]}`. It is present only in unattended mode, and attended output is byte-unchanged.

### 5.6 Authority, not acceptance

- An accepted repair under an envelope is the check's verdict: the existing hidden-check and adjudication rules,
  unchanged.
- The envelope produces no `axon-approval/3` token and no `.approved` record.
- Test: a job with `require_approval = true` built from the repaired file is refused by `axon-os` until a human mints a
  `/3` token.

## 6. Enforcement gate (unattended refused wherever a limit cannot be enforced)

Each condition is evaluated in §5.2 step 9 from data cortex holds before any untrusted code runs. Each condition is also
a build item: until its prerequisite lands, the gate **refuses** that case. It never trusts it.

| Id | Limit at risk | Today (anchor) | Gate rule | Prerequisite to admit the case |
|---|---|---|---|---|
| E1 | signed fs write/read scopes (O-S1-03) | a scope kind the file uses is left UNSET = unrestricted (fence.rs test :394-403) | admit only if every used kind is pinned to the envelope's non-`*` prefixes in the fence env and `fence.basis != Unknown`, `fence.applied` | fence accepts envelope-supplied scope pins for used kinds; pins ⊆ write_prefixes for FS_WRITE |
| E2 | Fabric fencing (O-S1-05) | Fabric path sets `fence.not_applied()` (`bin/cortex.rs:716-726`) | any `--fabric-*` → refuse | none in this spec: Fabric unattended needs an equivalent fence (separate design) |
| E3 | hard deadline (O-S1-07) | in-flight `ai:` call up to 30 s past the deadline (`crates/axon-cortex/src/ai.rs:62-76`); checks not cut (USAGE `bin/cortex.rs:88-90`) | admit only with hard-deadline mode | a watchdog at `started + deadline_ms`: kills check and generator process groups, runs the `ai:` HTTP call with `timeout = remaining`, applies no result that arrives after the bound, exits 20. The overrun bound `HARD_GRACE` (DRAFT 2 s) is the worst case to restore and report. |
| E4 | native-module scopes (O-S1-09) | `eval_native_call` passes `scope_args = None` except `*_connect` (`crates/axon-core/src/interp/eval.rs:1218-1228`) | refuse if the file reaches any native fn without scope enforcement (`gfx`, `platform`, `fix`, non-connect `modbus`/`fhir`, …) per `axon caps` | interpreter: when any ambient FS/NET scope is set, a native call with no scope rule is DENIED (fail closed). This is a new env `AXON_SCOPE_STRICT=1` set by the unattended fence. |
| E5 | foreign edits (O-S1-13) | FU-4 restore table | admit only on a build where every write (patch apply, restore) is compare-and-swap on the last digest cortex wrote or read | FU-4 plus CAS on apply; a mismatch stops the walk, exit 22, and writes nothing |
| E6 | cache writers bypassing the write scope | AI replay cache, `run_start` stamp, `AXON_DUMP_BINDINGS`, audit ledger, `AXON_RECORD` are ungated (approval map §2) | the check child env (already `with_clean_env`, `bin/cortex.rs:416-420`) must not contain those vars; `XDG_CACHE_HOME` = per-run temp dir outside the workspace | an assertion in `registered_runner`, plus a test that dumps the child env |
| E7 | guest/VM and AOT routes | MMDS carries no scopes (`crates/axon-vm/src/firecracker.rs:80-91`); AOT refuses the vars | admit only the local interpreter executor (`LocalInterpreterExecutor`, `runner.rs:3404`); any other executor → refuse | none (fail closed) |
| E8 | program-created sandboxes | inner sandboxes compare effects only (`builtins.rs:4318-4426`); the ambient scope is re-checked (`builtins.rs:697-716`) | not a bypass of the signed (ambient) limits | regression test: inner `*` sandbox write outside the ambient scope → exit 8 (`ambient_scope.rs:227-235`) stays green; also for read and net |
| E9 | untrusted processes holding authority | a `cmd:` generator is an arbitrary same-uid process | unattended admits `ai:` generators only; the key, the spent store and `AXON_APPROVAL_KEY` never appear in any child env | none for `ai:`; `cmd:` needs a uid- or namespace-isolated launcher (separate design) |
| E10 | non-UTF-8 effect ceiling (O-S1-09 narrow) | FU-5 | admit only on a build containing FU-5 | FU-5 |

The gate's result list is emitted in `authorization.enforcement` (§5.5). Each row's `basis` names the fact checked,
e.g. `fence.applied=true`, `native=[]`.

## 7. Security and correctness tests (must all pass before §9)

Each test is fail-first where it guards a behaviour.

| # | Test |
|---|---|
| S1 | KATs: subkey derivation; MAC over a fixed envelope |
| S2 | Altering each field individually → MAC mismatch (table-driven over every field) |
| S3 | Wrong key; unkeyed (`axtok*`) envelope; env-var key in unattended mode → refused |
| S4 | Expired; not yet valid; TTL above max; deadline outliving expiry |
| S5 | Replay: sequential second use refused; 2–8 concurrent runs → exactly one proceeds; a refused envelope (wrong task) stays unspent |
| S6 | Out of scope: other workspace, file, symbol, pristine sha; a wider write prefix; a larger budget; another generator; changed check fn; changed checker sha; changed fence |
| S7 | Cross-protocol: `/3` as envelope refused; `/4` to `verify_approval` refused; unknown field refused |
| S8 | No self-authority: `cargo metadata` shows no axon-intent under axon-cortex. A dump of the child env (generator stub, check program, ranker stub) contains no key, path or spent dir. The runner never modifies the envelope (byte-identical after the run). |
| S9 | Key file and store perms: group- or world-readable, or wrong owner → refused |
| S10 | Each E-row: refusal case and admitted case. E3 with a stalling HTTP server (fake endpoint) and a sleeping check: the run ends by `deadline_ms + HARD_GRACE` and no write happens after the bound. E5 with a concurrent writer. E4 with a `gfx` call under a strict scope. |
| S11 | Attended mode unchanged: the existing cortex suites pass, and the attended `cortex-repair/1` JSON is byte-identical on a pinned task |
| S12 | Acceptance ≠ approval (§5.6) |

g01 mutation rows cover each refusal site (MAC compare, expiry, consume `create_new`, the no-widen comparisons, each E
rule).

## 8. Build plan (when authorized)

| Unit | Content |
|---|---|
| A1 | `axon-os::repair_envelope` (types, canonical form, verify, consume), subkey and KATs, `signing_domains` entry; S1–S4, S7, S9 |
| A2 | axon-intent `authorize-repair` minter |
| A3 | cortex `--unattended` wiring, exit 28, `authorization` output; S5, S6, S8, S11, S12 |
| A4 | Enforcement prerequisites E1, E3, E4, E5, E6 (code); E2, E7, E9 refusals; E8 regression; S10 |
| A5 | Docs (USAGE, README, `env_registry` for `AXON_SCOPE_STRICT`), opportunities, tracker |

**Paid-run implications.** A3 and A4 touch the repair digest (`axon-cortex/src/*.rs`), so one receipt regress is
needed. Attended behaviour must be unchanged (S11).

## 9. Enabling

All S-tests green, the full gate green, and a security review (a read-only adversarial reviewer on A1–A4) with every
finding dispositioned. Even then, `--unattended` ships **refusing by default** until a human records the enabling
decision (§10 Q3).

## 10. needs-human

| Q | Question | Default in this DRAFT |
|---|---|---|
| Q1 | HMAC (G5, as required) or ed25519 envelopes, which would make self-minting cryptographically impossible? | HMAC per the stated requirement, plus isolation (E6, E9) |
| Q2 | `MAX_TTL_S` | 86 400 s |
| Q3 | Who enables `--unattended` after §9, and is a per-host opt-in file required? | a human decision; refused by default |
| Q4 | Is `ai:`-only acceptable for v1, given that `cmd:` agents need an isolated launcher? | yes |
