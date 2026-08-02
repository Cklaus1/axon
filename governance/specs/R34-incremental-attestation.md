# Tech Spec — R34: Incremental Attestation (rolling hash chain — every `axon-vm run` extends the chain)

**Spec ID:** `R34-incremental-attestation`
**Status:** 🔧 Implementing (re-verified 2026-07-18) — core rolling-hash chain (S1–S3 scope:
`chain.rs` formula + `ChainStore` append/verify + CLI `chain stamp`/`chain verify`/
`run --chain-stamp`) landed; S4 (export/import) landed 2026-07-18 — `ChainStore::export` +
`verify_export` + `ChainExport` (schema `axon-chain-export/1`), sharing one verification core
(`verify_entries`) with `ChainStore::verify` so the live-store and exported-JSON paths cannot drift
apart. **S6 (`chain show`/`export`/`verify-export` CLI subcommands) landed 2026-07-18** — wires
S4's library functions into `axon-vm`'s CLI (`chain show [--json]`, `chain export --out`,
`chain verify-export`); the acceptance gate's new §8 exercises the full show→export→verify-export
journey against the real binary, including a head-only-tampered export (every individual link
still recomputes cleanly, only the claimed tip is forged) to prove `verify_export`'s extra
head-consistency check, not just per-link verification. S7 (R33 `VoteRequest` integration) NOT
started — see spec-meta `status-claim` and §14 evidence ledger below. **Corrected 2026-07-31
(review fold-in):** R33 *is* landed in code; §4.5 rewritten against the landed R33 shapes and is
now the sole normative source for S7 (with its own pinned acceptance tests); as-built divergences
from the original §2/§3/§5 design text are reconciled in the new §12. **Second (adversarial)
fold-in 2026-07-31:** corrected the first fold-in's own errors — the false "`uuid` already
in-scope" premise of the §12.3 fix (new dep now authorized in §1.3), the §12.5 "one of which"
clause that let insufficient remedy #1 qualify for the enforcement upgrade, and the §4.5
`verify_entries` (private fn) pointer — plus recorded in §12.1 that `seq` is unverified as built
(no SequenceGap; I-1 unenforced), added a normative size cap on the §4.5 embedded export
(unbounded voter-side allocation), aligned §1.1's "immediately before" claim with §4.5's
subsequence semantics, and recorded two fail-open genesis behaviors.
**Third (ASI-trajectory) fold-in 2026-07-31:** this pass asked which of R34's *assumptions* an
optimizing (not merely careless) generator invalidates. Four expiring assumptions are now
recorded explicitly rather than left implicit — that a run is recorded at all (§8.2
run-without-stamp; the framing box narrowed to *recorded* runs), that a program's identity is its
source bytes (§4.2 scope note + §12.6 authority/AI envelope), that chains stay small (§4.5/§8.4
scaling limit; §12.9), and that a human auditor re-hashes and reads every entry (§8.2
auditor-capacity non-closure; §12.8). Four scoped future slices (S9–S12) and open questions
§12.6–§12.11 carry the strengthening levers; no kill-gate or fail-closed posture was weakened.

```spec-meta
id: R34-incremental-attestation
status-claim: Implementing
depends-on: R31-extended-tcb-attestation
blocks: none
blocked-by: none
supersedes: none
related: R33-cross-vm-safety-quorum, R28-capability-audit-ledger
conflicts-with: none
reserves: exit code 15 (CHAIN_VERIFY_FAIL_EXIT_CODE) — confirmed free at claim time (grepped
  crates/ for `exit(1[0-9])`/`_EXIT_CODE.*1[0-9]`; 10/12 used by R26/R31, 13/14 reserved by
  R33 spec text; corrected 2026-07-31: R33 IS now landed in code — `quorum/logic.rs` +
  `quorum/vsock.rs`, and 13/14 are real constants in `crates/axon-vm/src/main.rs`;
  15 was genuinely unclaimed)
evidence: scripts/r34_acceptance_gate.sh
```

**Implements:** the execution-history gap identified after R31: R31 proves what *software* is running
at boot, but says nothing about what *programs* were executed after boot. A VM whose R31 report is
valid could run an arbitrary sequence of `.ax` programs after attestation, and the relying party
would have no cryptographic record of what ran in what order. R34 closes this gap by extending the
`axtcb1-ext:` root into a rolling, append-only hash chain: each `axon-vm run PROG.ax` irreversibly
extends the chain, binding the program source hash, its position in the sequence, and its run-id
into the chain value. A relying party who holds the current chain tip and the full run history can
reconstruct and verify the complete execution sequence.
**Depends on:**
- `governance/specs/R31-extended-tcb-attestation.md` — `axtcb1-ext:` is the chain's genesis root;
  `crates/axon-attest` is extended with chain logic; the `axon-vm` binary gains `chain` subcommands
- `governance/specs/R28-capability-audit-ledger.md` — the chain file uses the same append-only JSONL
  format as the R28 audit ledger; `~/.axon/chains/<vm-id>/chain.jsonl` parallels the ledger path
- `governance/specs/R33-cross-vm-safety-quorum.md` — landed in code (corrected 2026-07-31; the
  earlier "in progress" note was stale). NOTE: the R33 spec itself contains **no**
  `proposer_chain_tip`/`required_prog_hashes` text and names no chain-aware voter tests — the
  `VoteRequest` extension for chain-aware quorum voting is specified **solely by §4.5 of this
  document**, which is the normative source for S7
**Audience:** an implementer who builds *strictly* against this document and reads only it.

> **Read this framing first.** R34 does **not** change the boot-time measurement algebra R31
> established, nor the quorum voting protocol R33 defines, nor any Axon-level containment. What R34
> changes is the *temporal scope* of the attestation guarantee: from "what software is running at
> boot" to "what programs ran, in what order, since boot." The load-bearing guarantee is:
> **each recorded run irreversibly extends the chain** — a relying party who knows the current
> chain tip `axtcb1_run_N` can verify the ordered history of the *recorded* runs by reconstructing
> the chain from program sources; no reordering, substitution, or removal of any **recorded** run
> goes undetected.
>
> **The word "recorded" is load-bearing (narrowed 2026-07-31, third fold-in).** As built, chain
> participation is a per-invocation opt-in: `axon-vm run` stamps only when the invoker passes
> `--chain-stamp PATH`, and the store path is caller-chosen (§12.1, §12.7). A run that never
> stamps leaves a chain that verifies perfectly and is indistinguishable from a chain where that
> run never happened — no tampering, no root, no fabrication required. Against a proposer that
> controls its own `axon-vm` invocations, "complete execution history" means "the history the
> proposer chose to record." §8.2 names this; §12.7 carries the enforcement lever. Do not restate
> this guarantee without the qualifier.

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey: boot, run 3 programs, verify chain is valid and length = 3 | §5, §7 | `acc_a1_smoke_chain_journey` |
| **A2** | Tamper detection: replacing a run's entry changes all subsequent chain values | §4.2, §7 | `acc_a2_chain_tamper_detected` |
| **A3** | Quickstart commands execute verbatim: `axon-vm chain show` / `axon-vm chain verify` work | §8, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Chain verification is pure: no side effects; deterministic; no I/O in the core | §4.3, §7 | `acc_a4_hermetic_isolated_timeout` |
| **A5** | Order-sensitivity: same programs in different order produce different chain | §4.2, §7 | `acc_a5_order_sensitive` |
| **A6** | Append-only enforcement: re-extending from an older root is refused | §4.4, §7 | `acc_a6_chain_mandatory_append_only` (**naming caveat, 2026-07-31 third fold-in:** "mandatory" here means *append-only is mandatory*, i.e. a stale-root append is refused. It does **not** mean recording is mandatory — nothing tests or enforces that a run stamps at all (§8.2, §12.7). If the S10 mandatory-recording slice lands, rename this check to `acc_a6_append_only_stale_root_refused` and give `..._mandatory_recording` to the new gate, so the name stops asserting a property nothing checks.) |
| **Core** | Source privacy: chain stores `sha256(source)`, not the source text | §3.1 | `source_hash_committed_not_source` |
| **Core** | R31 composition: chain root equals the R31 `axtcb1-ext:` at boot time | §4.1 | `chain_composes_with_r31` |
| **Core** | Auditor reconstruction: given chain + sources, all intermediate hashes verify | §4.3 | `auditor_can_reconstruct` |
| **Core** | Export/import round-trip: chain exported to JSON, re-imported, verified → pass | §5.4 | `chain_exported_and_imported` |
| **Gate** | The acceptance gate itself fails if any check above is missing or stubbed | §-Gate | `scripts/r34_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does

R31 proved the *host safety stack* at boot time: kernel + kill-switch binary + audit writer +
compliance monitor are all the expected versions, bundled into `axtcb1-ext:`. This is a strong
start — a relying party can verify no binary has been swapped since the VM image was built.

But boot-time attestation has a gap: it says nothing about what programs the VM executed
*after* boot. An attested VM could run an unsafe `.ax` program, a financial operation, or a
data exfiltration script after the R31 attestation report was produced, and the relying party
would have no cryptographic evidence either way.

R34 introduces a **rolling hash chain** that extends across every `axon-vm run` invocation:

- At boot, the chain is seeded with the R31 `axtcb1-ext:` value — the full-stack boot measurement
  becomes the chain's genesis block, anchoring the entire run history to the hardware-attested boot.
- Each time `axon-vm run PROG.ax` executes, the CLI computes a new chain value that incorporates
  the previous chain tip, the hash of the program source, a fresh UUID run-id, and a timestamp.
  The new value is appended to `~/.axon/chains/<vm-id>/chain.jsonl` and printed to stderr
  alongside the boot `axtcb1-ext:`.
- An auditor holding the full `chain.jsonl` and the source files for each run can reconstruct
  every intermediate chain value and verify the entire execution history: what ran, in what order,
  with no gaps or substitutions.

Concrete motivating scenario: a regulated deployment requires that before any financial operation,
the VM's chain shows that the compliance-checking program ran before the operation.
With R34, the proposing VM includes its current chain tip in the R33 quorum `VoteRequest`;
each voter can inspect the chain history and refuse to vote YES if the required compliance program
hash does not appear **somewhere in the chain before the operation, in the declared order**
(§4.5 step 4 — a subsequence check, NOT an adjacency check: arbitrary runs may sit between the
compliance run and the tip, and the compliance run may be arbitrarily old. A deployment that
needs "immediately before" semantics requires §4.5's suffix-mode extension, which is not yet
specified as landed — corrected 2026-07-31, second fold-in; the earlier "immediately before"
wording here promised strictly stronger semantics than §4.5's sole normative check delivers.
Note also §4.5's enforcement caveat: as built this is deterrence/forensics against a dishonest
proposer, not prevention).

### 1.2 What it explicitly does NOT do

- **No program execution control.** R34 records what ran; it does not block or gate what may run.
  Execution policy remains the job of R27 (kill-switch), R11 (capabilities), and R33 (quorum).
  R34 is a forensic and audit primitive, not an enforcement primitive — it composes with R33
  toward enforcement only once entries are bound to attested execution (§4.5 caveat, §12.5).
- **No source confidentiality beyond hash commitment.** The chain stores `sha256(source)`, not
  the source text. The source text remains on disk (or not) at the operator's discretion. An
  auditor verifying the chain must separately obtain the source files — R34 does not distribute
  or store them.
- **No defense against a root-compromised VM rewriting the chain file.** A VM operator with root
  access can truncate or rewrite `chain.jsonl` before export. R34 detects tampering from the
  outside (a relying party can check the chain), but not from a compromised measurement host.
  The mitigation (external log publication) is R35, out of scope.
- **No real-time ordering guarantees.** Timestamps in `ChainEntry` are informational and can be
  faked by a compromised VM. The chain guarantees *append-order*, not wall-clock order.
- **No new isolation, no new binary formats.** R34 extends `axon-vm` and `crates/axon-attest`;
  it does not change R26 isolation, R27 kill-switch logic, or R31 measurement algebra.
- **No guarantee that a run is recorded (added 2026-07-31, third fold-in).** Stamping is opt-in
  per invocation (`--chain-stamp PATH`, caller-chosen store). R34 records the runs it is told
  about. Making recording unavoidable is §12.7 / slice S10, not a property of the landed chain.
- **No commitment to the authority or nondeterminism envelope (added 2026-07-31, third
  fold-in).** The §4.2 preimage binds `sha256(source_bytes)` only. Two runs of the same source
  under different principals, different `allowed_effects` (including the
  `AXON_VM_ALLOWED_EFFECTS` override), different budgets, a different or absent seccomp policy,
  a different `AXON_SEED`, or a different AI model/provider are **chain-identical**. For
  AI-authored or `ai_complete`-driven programs — where the typed `.ax` is an IR rather than the
  behavior-determining artifact — the source hash is a weak identity. §12.6 / slice S9 carries
  the `aux_hash` fix.

### 1.2b Stated limits that expire with scale (added 2026-07-31, third fold-in)

These are not defects in the primitive; they are assumptions the design rests on that stop
holding as run volume and machine authorship grow. They are stated here so no reader has to
infer them.

- **Human-auditor capacity is an unautomated link.** §1.1's "an auditor holding the full
  `chain.jsonl` and the source files can reconstruct what ran" describes a manual, per-entry,
  linear-in-N task with no summarization, policy predicate, or risk ranking in R34. At agent run
  volumes, and with machine-generated program bodies, "an auditor reads all N programs" is not a
  reachable state. §8.2 records this as an explicit non-closure; §12.8 carries the lever
  (per-entry `effect_union` / risk in a machine-checkable summary, so auditing is a query).
- **Chain size is assumed small.** §4.5 caps the embedded export at 65,536 entries and fails
  closed above it; §8.4 contemplates millions. Chains are per-boot and append-only with no
  pruning or checkpoint, so an agent loop crosses the cap and the chain-aware voting path then
  returns NO permanently for the life of the boot. §12.9 carries the lever.
- **Compliance-by-source-hash is assumed stable.** §4.5's `required_prog_hashes` presumes a
  small, pre-known, byte-stable set of programs — which regeneration via
  `axon intent compile --gen`, a model-version bump, or even a formatter run invalidates (§8.4
  forbids normalization). §12.10 carries the property-based alternative.

### 1.3 Interface & tech constraints

- **Interface:** extends `axon-vm` with `chain show`, `chain verify`, `chain export`,
  `chain verify-export` subcommands; adds `--chain` and `--verify-chain EXPECTED_TIP` flags
  to `axon-vm run`.
- **Language/crate:** extends `crates/axon-attest` with `chain.rs` (pure chain logic) and
  `chain_store.rs` (I/O seam). Allowed new deps: the `uuid` crate (`v4` feature) or `getrandom`,
  in `axon-vm` only, solely for the §12.3 `run_id` fix — `sha2`, `serde_json`, `serde` are
  already in scope. *(Corrected 2026-07-31, second fold-in: the earlier text claimed "`uuid` is
  already used for run-ids elsewhere" AND "Allowed new deps: none" — both wrong. No workspace
  crate depends on `uuid` (the only source mention is a prose doc-comment in
  `axon-ledger/src/store.rs`), and the landed run-ids are pid+nanos, so the §12.3 fix genuinely
  requires a new dependency; it is authorized here.)*
- **Perf/security:** chain extension is pure after byte injection (source bytes injected by the
  CLI layer; clock and UUID injected by caller). The JSONL append is an atomic file-append
  (O_APPEND). Chain verification reads the file sequentially and recomputes each step — no random
  access, no in-memory buffering of the full file needed.
- **Fail closed on every ambiguity:** a missing or corrupt `chain.jsonl`, a `chain_before` value
  that does not match the previous entry's `chain_after`, or a program source that does not match
  its committed `prog_hash` all cause verification to return `Err`, never a partial pass.

---

## §2 — Architecture & modules

R34 extends `crates/axon-attest/src/` — the pure measurement crate R31 established — and adds
a chain I/O module parallel to R28's ledger writer.

```
crates/axon-attest/src/
  chain.rs         NEW: pure chain logic.                                      [PURE]
                   `extend_chain(prev: &str, prog_hash: &[u8;32],
                                 run_id: &str, timestamp_ms: u64) -> String`
                   `verify_chain(entries: &[ChainEntry], boot_root: &str,
                                 sources: &[(seq, &[u8])]) -> Result<(), ChainError>`
                   `reconstruct_chain(boot_root: &str, entries: &[ChainEntry]) -> Vec<String>`
                   No I/O. All inputs injected by caller.
  chain_store.rs   NEW: I/O seam for JSONL append/read; wraps chain.rs.       [I/O seam]
                   `ChainStore { path }` — `append(entry)`, `load() -> Vec<ChainEntry>`,
                   `head() -> Option<str>`, `boot_root() -> Option<str>`.
                   Uses O_APPEND file writes (atomic on POSIX); reads are sequential.
  chain_export.rs  NEW: JSON export/import for auditor delivery.               [PURE + I/O seam]
                   `export_chain(store, vm_id, out_path)` / `verify_export(path, sources_dir)`.
crates/axon-attest/tests/
  r34_acceptance.rs   The A1–A6 + Core acceptance checks (named per §0).
scripts/r34_acceptance_gate.sh   The pinned gate (§-Gate).
```

**I/O boundary is explicit.** `chain.rs` receives `&[u8]` byte slices, `&str` strings, and `u64`
values — never file paths or clocks. The CLI layer reads the source file, generates the UUID,
reads the monotonic clock, and injects them. This makes every chain step testable with in-memory
values, no filesystem fixture required for unit tests.

**Dependency graph (R34 additions only):**
```
cli [I/O] → chain_store [I/O seam] → chain [PURE]
cli [I/O] → chain_export [PURE + I/O seam] → chain [PURE]
```
Nothing new is impure beyond the CLI edge. No new cycles.

---

## §3 — Data model

### 3.1 `ChainEntry` — per-run chain record

```rust
struct ChainEntry {
    seq:          u64,    // 0-indexed position in the run sequence; monotonically increasing
    run_id:       String, // UUIDv4, fresh per run; prevents future-run precomputation
    prog_hash:    String, // hex(sha256(program_source_bytes)) — source committed, not stored
    chain_before: String, // "axtcb1_run_N-1" or boot_root for seq=0; links to previous tip
    chain_after:  String, // "axtcb1-run:<hex>" — the new chain value after this run
    timestamp_ms: u64,    // milliseconds since Unix epoch; informational only, NOT hashed
}
```

`prog_hash` commits to the source bytes without storing them, preserving source privacy. An
auditor verifying the chain must obtain the source file separately and recompute the hash.
`timestamp_ms` is excluded from the chain formula — it is a convenience field for human auditing,
not a security commitment. Including it in the hash would make reconstruction require exact
timestamp replay, which is impractical for a relying party.

### 3.2 `RunChain` — the full chain state for one VM instance

```rust
struct RunChain {
    vm_id:     String,           // identifier for this VM instance (from R31 deployment manifest)
    boot_root: String,           // R31 axtcb1-ext: at boot — the chain's genesis block
    entries:   Vec<ChainEntry>,  // all runs in append order; seq = 0, 1, 2, …
    head:      String,           // current chain tip = entries.last().chain_after (or boot_root if empty)
}
```

`boot_root` is the load-bearing link to R31: the chain's genesis value is the R31
full-stack boot measurement. This means a relying party who pins a specific `boot_root` value
knows both what software was running at boot (via R31) and the complete execution history since
then (via the run chain).

### 3.3 Chain file format — `~/.axon/chains/<vm-id>/chain.jsonl`

Each line is a JSON-serialized `ChainEntry`. The file is append-only; no line is ever modified
or removed. The path parallels R28's audit ledger (`~/.axon/audit/<vm-id>/ledger.jsonl`) and uses
the same JSONL discipline: one complete JSON object per line, newline-terminated, written via
O_APPEND. The first line (seq=0) is the genesis entry; its `chain_before` is the `boot_root`.

A companion metadata file `~/.axon/chains/<vm-id>/meta.json` records `vm_id` and `boot_root` and
is written once at VM boot (or at the first `axon-vm run --chain` invocation if the chain has not
been initialized). The `chain_store.rs` I/O seam reads `meta.json` to initialize a `RunChain`.

---

## §4 — Core logic

### 4.1 Chain genesis — linking to R31

Before the first `axon-vm run --chain` call, the VM's `boot_root` must be initialized:

```
boot_root = axtcb1_ext   // from the R31 extended measurement produced at boot
                          // (or from axon-vm attest --extended-tcb if the VM doesn't auto-attest at boot)
```

The `meta.json` file is written with `{ "vm_id": "…", "boot_root": "axtcb1-ext:…" }`. If
`meta.json` already exists and its `boot_root` differs from the current `axtcb1-ext:`, the CLI
MUST refuse and print `ChainError::BootRootMismatch` — a reboot (new `axtcb1-ext:`) produces a
new chain, not an extension of the old one. Each boot is a new chain; chains across reboots are
not merged.

`chain_composes_with_r31`: an empty chain's `head` equals `boot_root` equals `axtcb1-ext:`. The
first run's `chain_before` is the R31 value. An auditor who starts reconstruction from the pinned
R31 `axtcb1-ext:` and applies the run entries in order arrives at the same `head` as the VM
reports.

### 4.2 Chain extension formula (the authoritative specification; implementers MUST follow exactly)

Each `axon-vm run PROG.ax --chain` call extends the chain as follows. All byte operations are
explicit; there is no implicit encoding. Implementation MUST match this formula byte-for-byte.

**Step 1 — Hash the program source:**
```
prog_hash_bytes: [u8; 32] = SHA-256(program_source_bytes)
prog_hash_hex:   String   = lower_hex(prog_hash_bytes)      // 64 lowercase hex characters
```

**Step 2 — Decode the previous chain tip to bytes:**
```
prev_chain_str:  String  = current head (e.g., "axtcb1-run:aabbcc…" or "axtcb1-ext:…" for seq=0)
prev_chain_body: String  = strip_prefix(prev_chain_str, "axtcb1-run:") if present,
                           else strip_prefix(prev_chain_str, "axtcb1-ext:") if present
                           — exactly one of these prefixes must match; otherwise ChainError::PrefixMismatch
prev_chain_bytes: [u8; 32] = hex_decode(prev_chain_body)    // 32 bytes decoded from 64 hex chars
```

**Step 3 — Assemble the preimage:**
```
run_id_bytes:      &[u8] = run_id.as_bytes()                // UTF-8 UUID string bytes (36 bytes)
timestamp_le:      [u8; 8] = timestamp_ms.to_le_bytes()     // u64 little-endian
version_tag:       &[u8] = b"axon-run-v1\n"                 // 12 bytes; versions the protocol

preimage = version_tag
        || prev_chain_bytes   // 32 bytes
        || prog_hash_bytes    // 32 bytes
        || run_id_bytes       // 36 bytes (standard UUID string, e.g. "550e8400-e29b-41d4-a716-446655440000")
        || timestamp_le       // 8 bytes
```

Total preimage length: 12 + 32 + 32 + 36 + 8 = **120 bytes** (when run_id is a standard UUID string).

**Step 4 — Hash and format:**
```
chain_N_bytes: [u8; 32] = SHA-256(preimage)
axtcb1_run_N:  String   = "axtcb1-run:" + lower_hex(chain_N_bytes)
```

**Why this construction?**

- `version_tag` is prepended (not appended) so that a SHA-256 length-extension attack cannot
  append to a known chain value to produce a valid next value without knowing the full preimage.
  The version tag also future-proofs the protocol: `axon-run-v2\n` can change the formula.
- `prev_chain_bytes` binds each value to its predecessor — removing or reordering any entry
  changes all subsequent values.
- `prog_hash_bytes` binds the source content at each position — substituting a different program
  at position N changes the chain from N onward.
- `run_id_bytes` is a fresh UUID per run — it is unguessable before the run, so an attacker
  cannot precompute valid chain extensions for future runs.
- `timestamp_le` is included in the preimage (though not as a security commitment) to prevent
  two runs of the same program with the same run-id from accidentally producing the same chain
  extension (a degenerate collision). Timestamp is NOT the ordering proof — `seq` and the chain
  linkage provide ordering.
- The `"axtcb1-run:"` prefix distinguishes run-chain values from boot-root values (`"axtcb1-ext:"`
  and `"axtcb1:"`). A verifier that accidentally treats a run-chain tip as a boot measurement
  will see the prefix mismatch and must fail closed.

**What the v1 preimage does NOT bind (scope note, added 2026-07-31, third fold-in — read before
citing a chain entry as evidence of *what happened*):** the preimage commits to source bytes and
position only. It does **not** commit to the run's authority envelope — `cmd_run` builds
`MmdsPayload { run_id, principal, allowed_effects, budget_tokens, source_hash, seccomp_bpf_b64 }`
(including the `AXON_VM_ALLOWED_EFFECTS` env override, which may *widen or narrow* the manifest's
`effect_union`) roughly a hundred lines before it stamps the chain, and none of it enters the
hash. Nor does it commit to the nondeterminism envelope (`AXON_SEED`, `AXON_AI_PROVIDER`, model
id / tier). Consequence: one benign source approved and hash-listed once suffices forever —
every subsequent escalation (different principal, wider effects, larger budget, absent seccomp
policy, different model) is chain-invisible. The v2 remedy is one 32-byte `aux_hash` field
(§12.6, slice S9); it is also the prerequisite that makes §12.5 #2 (signed entries) worth
signing, since a signature over an envelope-free preimage attests only to source identity.
Per I-6 this requires the `axon-run-v2\n` bump — do NOT add fields to the v1 preimage.

### 4.3 Chain verification (auditor reconstruction)

`verify_chain(entries, boot_root, sources)` recomputes the chain from scratch:

```
prev = boot_root
for entry in entries (ordered by seq, 0-indexed contiguously):
    assert entry.chain_before == prev                          // linkage check
    assert entry.seq == expected_seq                           // no gaps
    src_bytes = sources[entry.seq]                             // caller provides source bytes
    assert hex(sha256(src_bytes)) == entry.prog_hash           // source commitment check
    recomputed = extend_chain(prev, sha256(src_bytes), entry.run_id, entry.timestamp_ms)
    assert recomputed == entry.chain_after                     // formula check
    prev = entry.chain_after
assert prev == head                                            // tip check
```

All four inner assertions must pass for every entry; a single failure anywhere returns
`ChainError` and the verification is refused — there is no partial-pass path.

`auditor_can_reconstruct`: given a `RunChain` and the source bytes for each entry, `verify_chain`
returns `Ok(())` and every intermediate `recomputed` value equals the corresponding `chain_after`
in the chain file.

`acc_a4_hermetic_isolated_timeout`: `extend_chain` and `verify_chain` make no syscalls; they are
pure functions over injected bytes. The test asserts this via the byte-injection discipline (no
file paths or clocks inside the functions).

### 4.4 Append-only enforcement

The chain store MUST refuse any write that would overwrite or modify an existing entry:

- `ChainStore::append(entry)` opens `chain.jsonl` with `O_APPEND | O_CREAT` and writes exactly
  one newline-terminated JSON line. It never truncates or rewrites the file.
- Before appending entry N, the store reads the current head and asserts
  `entry.chain_before == head`. If the caller passes a `chain_before` that does not match the
  current file head (e.g., attempting to re-extend from an older entry), the store returns
  `ChainError::StaleRoot` and the write is refused. This closes the fork-the-chain attack: an
  attacker cannot insert a branch at position K and append from there; they can only append to
  the current tip.

`acc_a6_chain_mandatory_append_only`: the test attempts to extend from `entries[1].chain_after`
when the current head is `entries[2].chain_after`; asserts `ChainError::StaleRoot`; asserts the
chain file is unmodified.

### 4.5 Integration with R33 (quorum chain-awareness) — REWRITTEN 2026-07-31 against the landed R33 shapes

> **Normative-source note (corrected 2026-07-31):** R33 is landed:
> `crates/axon-vm/src/quorum/logic.rs` defines
> `VoteRequest { run_id, prog_hash, voter_tcb, proposed_action, timestamp_ms }` and
> `VoteResponse { voter_tcb, run_id, approved, reason, lineage_root }` — a boolean
> `approved` + free-text `reason`; there is **no** 0–100 `score` field (explicitly scoped out
> per `logic.rs`) and **no** `VoteResponse.blocked_reason` field (in R33's own spec,
> `blocked_reason` lives on `QuorumResult`, not `VoteResponse`). The R33 spec contains no
> chain-extension text at all. **This section is the sole normative source for S7.** An earlier
> revision of this section targeted `score = 0` / `VoteResponse.blocked_reason` — fields that
> exist in neither spec nor code; that text is superseded.

`VoteRequest` gains three new fields, all `#[serde(default)]` for backward compatibility (an old
`.req` file or an old voter parses unchanged; `old_voter_ignores_unknown_fields` pins this):

```rust
// Extension for R34 (added to the landed VoteRequest in quorum/logic.rs):
#[serde(default)]
proposer_chain_tip:    Option<String>,       // current entry_hash tip ("axtcb1-run:…") of the proposing VM
#[serde(default)]
required_prog_hashes:  Vec<String>,          // hex(sha256(source)) values that must appear in the
                                             // chain before the tip, in the declared order
#[serde(default)]
proposer_chain_export: Option<ChainExport>,  // the proposer's exported chain, embedded
                                             // (schema axon-chain-export/1, §12.1 field names)
```

**Transport (decided 2026-07-31; was previously unspecified):** the chain export travels
*embedded in the `VoteRequest`* as `proposer_chain_export`. This keeps the landed single-frame
vsock protocol (`quorum/vsock.rs` reads exactly one `VoteRequest` JSON frame per connection)
and the single-`.req`-file voting path unchanged — no second frame, no side-channel file.

**Size bound (normative, added 2026-07-31, second fold-in):** the embedded export MUST be
bounded, because the landed `read_frame` (`quorum/vsock.rs`) reads a wire-supplied u32 length
and immediately allocates `vec![0u8; len]` — up to 4 GiB — before any validation, so an
unbounded `proposer_chain_export` hands a malicious proposer a voter-side OOM/stall vector.
An S7 implementation MUST (a) add a voter-side frame-size ceiling in `read_frame` (reject
frames larger than 16 MiB before allocating), and (b) cap `proposer_chain_export` at 65,536
entries; an oversize export → `approved = false`,
`reason = "chain_requirement_not_met: export too large"` (fail closed, never a crash or an
unbounded allocation). The embedded-export voting path is therefore **bounded, not streaming**
— it deliberately trades §8.4's long-chain streaming property for single-frame simplicity;
§8.4's long-chain bullet is annotated accordingly.

> **Scaling limit of the size bound (stated 2026-07-31, third fold-in — the cap is correct, its
> lifetime is not).** The fail-closed cap MUST NOT be raised or removed as a workaround; it is
> the right behavior for an unbounded wire allocation. But note what it implies: one entry = one
> run, chains are per-boot (§4.1) and append-only with no truncation, compaction, or checkpoint
> (§4.4). An agent loop of the kind Axon exists to host — `goal_run` iterations, per-task
> `axon intent compile`, self-improve cycles — crosses 65,536 runs in hours. Past that point
> **every chain-aware vote returns NO, permanently, for the lifetime of the boot**, and the only
> remedy available today is a reboot that discards exactly the history the mechanism exists to
> prove. The mechanism disables itself by being used successfully. There is no acceptance check
> at any N > 4 anywhere in §7 or §13, so this failure mode is currently untested as well as
> unhandled. **Normative for S7:** an S7 implementation MUST add an acceptance check that builds
> a chain of ≥100,000 entries and asserts the §4.5 voting path's behavior at that size
> (`voter_behavior_at_100k_entry_chain`) — as specified today it provably returns NO. Fixing the
> transport so it does not (Merkle inclusion proofs or checkpoint entries) is §12.9 / slice S12,
> which SHOULD land before S7 is relied on in any long-running deployment.

When `required_prog_hashes` is non-empty, each voting VM:
1. Requires `proposer_chain_export` to be present. A non-empty requirement with a missing export
   → `approved = false`, `reason = "chain_requirement_not_met: no chain export supplied"`
   (fail closed).
2. Verifies the embedded export via `chain::verify_export` — the voter's callable entry point
   (corrected 2026-07-31, second fold-in: the earlier text named `verify_entries`, which is
   module-**private** in `chain.rs` and unreachable from `quorum/logic.rs`; `pub fn
   verify_export` wraps that shared core and additionally performs the export's own
   head-consistency check in one call). Any chain error →
   `approved = false`, `reason = "chain_requirement_not_met: <chain error>"`.
3. Checks the export's head equals `proposer_chain_tip` (a separate check from step 2's
   internal head consistency — this one binds the export to the tip *claimed in the request*);
   mismatch → `approved = false`, `reason = "chain_requirement_not_met: tip mismatch"`.
4. Checks that each hash in `required_prog_hashes` appears in the verified chain, in order, as
   a contiguous or non-contiguous subsequence of `prog_hash` values leading up to the tip. If
   absent: `approved = false`, `reason = "chain_requirement_not_met"`.
   **Semantics note (added 2026-07-31, second fold-in):** this is a *subsequence* check, not an
   adjacency check — a chain `[compliance_check, evil1, evil2, …, tip]` satisfies it. It cannot
   express §1.1's original "immediately before" requirement (§1.1 now rewritten to match). A
   deployment needing adjacency requires an additional
   `required_prog_hashes_mode: subsequence | suffix` field, where `suffix` requires the declared
   hashes to be exactly the final entries before the tip, pinned by a
   `voter_rejects_nonsuffix_compliance_run` test — an optional S7 extension; if built, it must
   default to `subsequence` for backward compatibility.

**Identity-vs-property caveat on `required_prog_hashes` (added 2026-07-31, third fold-in).**
`required_prog_hashes` is *identity*-based compliance: a relying party pre-enumerates
`hex(sha256(source))` values. That works when programs are a small, stable, pre-known set of
artifacts. It does not survive machine-authored or regenerated programs — Axon's own Phase-10
`AXON_INTENT_GEN=1 axon intent compile goal.md` path emits LLM-authored bodies whose bytes change
on every regeneration and every model-version bump, and §8.4 forbids any normalization ("no
newline normalization, no BOM stripping … Operators MUST not post-process source files"), so even
a formatter run invalidates a pinned hash. Combined with this section's own enforcement caveat
(`chain stamp` appends without executing), the check is simultaneously **brittle against honest
churn and weak against a dishonest proposer** — the worst pairing. Keep `required_prog_hashes`
for the fixed-artifact case, but do NOT present it as *the* compliance mechanism. The
property-based alternative (`required_effect_ceiling` over a per-entry `effect_union`, and/or
requirement by approved-AST identity via the `<file>.ax.approved` sign-off record) is specified
as §12.10 / slice S11; both are semantic, survive regeneration, and are closer to what a voter
actually cares about. Neither weakens this section: a property requirement is *additional* to,
never a substitute for, the fail-closed steps 1–3 above.

**Pinned S7 acceptance checks (defined here, NOT in R33 — the previous pointer to "R33 voter
tests" was dangling; R33 names none):**
- `voter_rejects_missing_required_prog_hash` — required hash absent from a valid chain → NO.
- `voter_accepts_chain_with_required_subsequence` — required hashes present in order → this
  check passes (vote proceeds to R33's normal logic).
- `voter_rejects_tampered_chain_export` — one flipped `entry_hash`/`head` in the embedded
  export → NO with a chain-error reason.
- `voter_rejects_missing_chain_export` — non-empty requirement, `proposer_chain_export: None`
  → NO (fail closed, step 1).
- `old_voter_ignores_unknown_fields` — a pre-S7 `VoteRequest` JSON (no new fields) and a new
  `VoteRequest` parsed by pre-S7 semantics both round-trip; backward compat holds.

**Enforcement caveat (recorded 2026-07-31 — do not overstate this composition):** as landed,
chain entries are proposer-constructed and **unsigned**, and `axon-vm chain stamp` appends an
entry **without executing the program** (`cmd_chain_stamp`/`stamp_chain` are pure
hash-and-append). A merely *dishonest* proposer — no root compromise needed — can therefore
satisfy any `required_prog_hashes` check by stamping `compliance_check.ax` without running it,
or by fabricating the export JSON wholesale. The voter check in this section verifies *internal
chain consistency*, not that the runs happened. Until run-entries are bound to attested
execution or chain tips are externally anchored (R35) — see §12.5 for the open binding
mechanisms — this composition is a **deterrence/forensics** control (an audited proposer's lie
is durable, ordered evidence), **not prevention**. S7 MUST NOT be built or documented as an
enforcement primitive without one of the §12.5 binding mechanisms; §8.2 records the threat.

---

## §5 — API / CLI

### 5.1 `axon-vm run` extension

```bash
# Run a program and extend the chain (default when chain.jsonl exists for this vm-id)
axon-vm run PROG.ax --chain [--vm-id ID]

# Require a specific chain state before running (fail-closed: refuses if chain tip doesn't match)
axon-vm run PROG.ax --verify-chain axtcb1-run:abc123...

# The new chain tip is printed to stderr alongside the boot axtcb1-ext:
# stderr: "axtcb1-ext: axtcb1-ext:…  axtcb1-run-3: axtcb1-run:…"
```

When `--verify-chain EXPECTED_TIP` is passed, the CLI reads the current chain head before
running the program. If `head != EXPECTED_TIP`, the CLI exits 15
(`CHAIN_VERIFY_FAIL_EXIT_CODE`) without running the program. The check is fail-closed: a
missing or empty chain also fails (not silently treated as matching).

### 5.2 `axon-vm chain` subcommands

```bash
# Show the current chain for this VM (human-readable summary)
axon-vm chain show [--vm-id ID] [--json]
# Prints: vm_id, boot_root, entry count, head (current tip), last run details.

# Verify the full chain against provided source files
axon-vm chain verify --sources-dir DIR [--vm-id ID]
# For each chain entry, reads <DIR>/<entry.prog_hash>.ax (or --sources-map FILE for custom naming)
# Exits 0 if all entries verify; exits 15 on first verification failure.

# Export the full chain for an auditor
axon-vm chain export --out chain_export.json [--vm-id ID]
# Writes { vm_id, boot_root, entries: [...], head, exported_at_ms } as a single JSON object.

# Verify an exported chain (auditor side — no live VM required)
axon-vm chain verify-export chain_export.json --sources-dir DIR
# Same verification logic as `chain verify` but against an exported JSON file.
```

### 5.3 Exit-code additions (no collision with R26/R31/R33 codes 0–14)

| Code | Const | Meaning |
|---|---|---|
| 15 | `CHAIN_VERIFY_FAIL_EXIT_CODE` | Chain verification failed: tamper detected, stale root, or `--verify-chain` tip mismatch |

Code 15 is distinct from 10 (attestation mismatch), 11 (TCB chain break), 12 (extended measure
failure), 13 (quorum blocked), and 14 (vote attestation rejected). It signals a chain-level problem.
Never collapse 15 into any other code.

### 5.4 Export format (`chain_export.json`)

```json
{
  "schema": "axon-chain-export/1",
  "vm_id": "vm-alpha",
  "boot_root": "axtcb1-ext:…",
  "head": "axtcb1-run:…",
  "exported_at_ms": 1751000000000,
  "entries": [
    {
      "seq": 0,
      "run_id": "550e8400-e29b-41d4-a716-446655440000",
      "prog_hash": "deadbeef…",
      "chain_before": "axtcb1-ext:…",
      "chain_after": "axtcb1-run:…",
      "timestamp_ms": 1750999900000
    }
  ]
}
```

`chain_exported_and_imported`: export → re-read the JSON → call `verify_chain` over the entries
with `boot_root` from the JSON → assert `Ok(())`. The export is self-contained: a verifier needs
only the JSON and the source files, not a live VM.

---

## §6 — Build order (TDD; each slice: test first, seen to fail, then passes)

| Slice | Deliverable | Pinned check (written first) |
|---|---|---|
| **S1** | `chain.rs`: `extend_chain` formula (§4.2) exact; unit tests for each preimage field. | `acc_a5_order_sensitive`, `chain_composes_with_r31` |
| **S2** | `chain.rs`: `verify_chain` reconstruction + all four inner assertions; tamper tests. | `acc_a2_chain_tamper_detected`, `auditor_can_reconstruct` |
| **S3** | `chain_store.rs`: JSONL append-only write + `StaleRoot` guard + `head()` / `boot_root()`. | `acc_a6_chain_mandatory_append_only`, `source_hash_committed_not_source` |
| **S4** | `chain_export.rs`: export to JSON + `verify_export`; schema `axon-chain-export/1`. | `chain_exported_and_imported` |
| **S5** | `axon-vm run --chain` integration: source hash, UUID, timestamp injected; stderr output; `--verify-chain` flag (exit 15). | `acc_a1_smoke_chain_journey`, `acc_a4_hermetic_isolated_timeout` |
| **S6** | `axon-vm chain show/verify/export/verify-export` subcommands; quickstart commands. | `acc_a3_quickstart_commands_execute` |
| **S7** | R33 `VoteRequest` extension fields + chain-aware voter logic (§4.5); backward compat (fields serde-default). | `voter_rejects_missing_required_prog_hash`, `voter_accepts_chain_with_required_subsequence`, `voter_rejects_tampered_chain_export`, `voter_rejects_missing_chain_export`, `old_voter_ignores_unknown_fields` (all defined in §4.5 — corrected 2026-07-31; the previous "named in R33" pointer was dangling) |
| **S8** | Acceptance gate `scripts/r34_acceptance_gate.sh`. | Gate exits 0 with every §0 check green. |
| **S9** *(added 2026-07-31, third fold-in; §12.6 + §12.11)* | **`axon-run-v2` preimage bump — done once.** Add `aux_hash` (32 B) to the preimage and a `#[serde(default)] aux` field to `ChainEntry`, sourced from the already-computed `MmdsPayload` (principal, allowed_effects, budget_tokens, seccomp hash, seed, model ids, `origin: run\|stamp`); guest adopts the MMDS run_id; commit the R28 audit-ledger tip. v1 chains keep verifying via the version tag. | `v2_entry_binds_authority_envelope` (same source + different `allowed_effects` → different `entry_hash`), `v1_chain_still_verifies_after_v2_bump`, `chain_run_id_resolves_via_axon_trace_replay` |
| **S10** *(added 2026-07-31, third fold-in; §12.3 + §12.7)* | **Close the cheap paths: mandatory recording + run_id integrity.** `chain_required` in principal/manifest → `cmd_run` exits non-zero without `--chain-stamp`; canonical vm-id-derived store path; `--run-id` either validated (v4 UUID + not already in store) or removed in favor of an unhashed `--correlation-id`. | `run_without_chain_stamp_refused_when_chain_required`, `caller_supplied_run_id_rejected_or_absent`, `duplicate_run_id_refused` |
| **S11** *(added 2026-07-31, third fold-in; §12.8 + §12.10)* | **Content verification + property-based requirements.** Wire `--sources-dir` into BOTH `chain verify` and `chain verify-export` over a content-addressed store (`<dir>/<prog_hash>.ax`); add `required_effect_ceiling` (and/or approved-AST identity) to §4.5 alongside `required_prog_hashes`. | `verify_detects_source_mismatch` (restores §9 step 6), `verify_export_detects_source_mismatch`, `voter_rejects_entry_exceeding_effect_ceiling` |
| **S12** *(added 2026-07-31, third fold-in; §12.9)* | **O(1)+proof chain transport.** Merkle inclusion proofs or signed checkpoint entries so the §4.5 voting path stops scaling with chain length — after the §12.9 open question (who may checkpoint; how a voter distinguishes a checkpoint from a truncation) is answered in writing. | `voter_behavior_at_100k_entry_chain` (required by §4.5 even before S12 — it documents the current fail-closed behavior), `inclusion_proof_verifies_without_full_export` |

**Definition of "done" per slice:** the slice's named check existed, was seen to fail, now passes;
the full `axon-attest` suite is green; no workspace regression; R26, R31, and R33 baseline tests
still pass.

**Slice risk (S1):** the chain formula is a **protocol**. A single off-by-one in the preimage
assembly (wrong byte order for `timestamp_le`, wrong prefix stripped from `prev_chain_str`)
produces a different chain value that no external verifier will reconstruct. Write `acc_a5`
before implementing `extend_chain`: assert that swapping `prev_chain_bytes` and `prog_hash_bytes`
in the preimage produces a different result; assert that a big-endian timestamp also differs.
These tests are protocol-compliance probes.

**Slice risk (S2):** `verify_chain` must fail closed on a *gap* in `seq` values (e.g., entries
with seq 0, 1, 3 — missing seq 2). A chain with gaps must return `ChainError::SequenceGap`, not
silently skip the missing entry. Assert this in `acc_a2`.

---

## §7 — Test plan (happy + adversarial; every named test is normative)

**Unit (pure, fast — bytes injected; no filesystem):**

- `chain_composes_with_r31` — call `extend_chain` with `prev = "axtcb1-ext:deadbeef…"`;
  assert the resulting `axtcb1_run_0` starts with `"axtcb1-run:"` and differs from the input.
  Assert `strip_prefix("axtcb1-ext:", ...)` succeeds; assert `strip_prefix("axtcb1-run:", "axtcb1-ext:…")` fails with `PrefixMismatch` (prefix routing is correct).
- `acc_a5_order_sensitive` — run programs P1, P2, P3 in order A=[P1,P2,P3] and order B=[P2,P1,P3];
  compute chain for both sequences from the same `boot_root`; assert `chain_A_final ≠ chain_B_final`.
  Also assert `chain_A[0].chain_after ≠ chain_B[0].chain_after` (divergence at position 0).
- `auditor_can_reconstruct` — construct a 5-entry chain via `extend_chain` (5 calls); then call
  `verify_chain` with the same source bytes; assert `Ok(())`; assert every intermediate
  `chain_before` in the reconstructed entries matches the expected formula output.
- `acc_a2_chain_tamper_detected`:
  - Tamper `entries[2].prog_hash` (flip one hex digit); assert `verify_chain` returns
    `ChainError::SourceMismatch { seq: 2 }`.
  - Tamper `entries[2].chain_after` (flip one hex digit); assert `ChainError::LinkageMismatch { seq: 3 }`.
  - Remove `entries[2]` entirely (seq gap); assert `ChainError::SequenceGap { expected: 2 }`.
  - Swap `entries[2]` and `entries[3]` (reorder); assert `ChainError::LinkageMismatch { seq: 2 }`.
  - All four tamper variants MUST be exercised (anti-vacuous guard in the gate, §-Gate item 3).
- `source_hash_committed_not_source` — assert `ChainEntry` has no `source` or `source_text` field;
  assert `extend_chain` takes `prog_hash: &[u8;32]` (not `source: &str`); assert the JSONL line
  contains `"prog_hash"` and does not contain `"source"` or `"source_text"`.
- `acc_a4_hermetic_isolated_timeout` — assert `extend_chain` and `verify_chain` are callable with
  in-memory values only (no file paths, no `std::fs`, no `std::time`); assert both functions are
  `#[inline]`-eligible pure computations; assert they return the same result when called twice
  with identical inputs.

**Integration (real filesystem; `chain_store.rs`):**

- `acc_a6_chain_mandatory_append_only`:
  1. Write 3 entries to a temporary `chain.jsonl` via `ChainStore::append`.
  2. Attempt `ChainStore::append` with `chain_before = entries[1].chain_after` (stale root, not the current head).
  3. Assert `ChainError::StaleRoot`; assert the file has exactly 3 lines (unmodified).
- `chain_exported_and_imported`:
  1. Write a 4-entry chain to a temp dir.
  2. `chain export --out /tmp/chain_export.json`.
  3. Parse the JSON; assert `schema = "axon-chain-export/1"`; assert `entries | length = 4`.
  4. `chain verify-export /tmp/chain_export.json --sources-dir /tmp/sources/` (sources pre-populated).
  5. Assert exit 0. Flip one byte in `chain_export.json`'s first `chain_after` field; re-run; assert exit 15.

**User-journey smoke (A1 — real CLI):**

- `acc_a1_smoke_chain_journey`:
  1. Initialize chain: `axon-vm chain show --vm-id test-vm` on a fresh dir; assert head = boot_root.
  2. Run 3 programs: `axon-vm run hello.ax --chain`, `axon-vm run math.ax --chain`, `axon-vm run structs.ax --chain`.
  3. `axon-vm chain show --json`; assert `entries | length = 3`; assert `head` starts with `"axtcb1-run:"`.
  4. `axon-vm chain verify --sources-dir examples/`; assert exit 0.
  5. Flip one byte in `hello.ax`; re-run `chain verify`; assert exit 15.

**Quickstart (A3):** `acc_a3_quickstart_commands_execute` extracts the fenced commands from §8
and runs each verbatim against the built `axon-vm` binary with `AXON_CI_NO_KVM=1`; all exit 0
with the documented output patterns.

---

## §8 — Threat model / invariants / edge cases

### 8.1 Threats R34 closes

- **Run removal.** An attacker deletes `entries[2]` from the chain. `verify_chain` detects the
  `SequenceGap` and fails (exit 15). The relying party sees the chain tip has changed.
  *(As built the removal IS detected, but via the hash-linkage break, not a `SequenceGap` —
  no seq check exists in the landed verifier; see §12.1's seq bullet, recorded 2026-07-31.)*
- **Run substitution.** An attacker replaces `entries[2].prog_hash` with the hash of a benign
  program. `verify_chain` reconstructs the expected `chain_after` for seq=2 using the original
  formula and finds it differs from the stored value — `LinkageMismatch { seq: 3 }` (seq=3's
  `chain_before` no longer matches seq=2's stored `chain_after`).
- **Order attack.** An attacker reorders entries 2 and 3 to make the dangerous program appear to
  have run before the compliance check. Swapping the entries produces mismatched `chain_before`
  linkage at seq=2; `verify_chain` returns `LinkageMismatch { seq: 2 }`.
- **Future-run prediction.** An attacker wants to precompute a valid chain extension before a run
  occurs, to fake an entry. The `run_id` is a fresh UUIDv4 chosen at run time; without knowing
  the future UUID, the attacker cannot compute the valid `chain_after` value in advance.
  *(NOT closed as built — the landed `run_id` is a guessable pid+nanos scheme; see §-Notes and
  §12.3, recorded 2026-07-31.)*
- **Fork-the-chain.** An attacker wants to create a branch: entries 0–2 on the main chain, then
  re-extend from entry 1 to hide a different run. `ChainStore::StaleRoot` refuses any `append`
  where `chain_before` is not the current file head. The JSONL file remains linear.

### 8.2 Threats R34 does NOT close (named, not hidden)

- **Run-WITHOUT-stamp — a run that simply never joins the chain (recorded 2026-07-31, third
  fold-in; strictly cheaper than every other threat in this list).** Chaining happens only when
  the invoker passes `--chain-stamp PATH` (`crates/axon-vm/src/main.rs`, `if let Some(ref
  chain_path) = chain_stamp {`); the flag is `Option<PathBuf>`, read in exactly one place, and
  the store path is caller-supplied. Nothing in the tree makes stamping mandatory (§5.1's
  "default when chain.jsonl exists" design text was dropped along with vm-id addressing —
  §12.1). A proposer that controls its own `axon-vm` invocations therefore omits the flag, or
  points it at a second store, and the run leaves **no artifact at all**: the surviving chain
  verifies perfectly, passes `verify_export`'s head-consistency check, and is byte-identical to
  a world where that run never happened. This requires no tampering, no root, no fabrication,
  and no forged signature — so it defeats §12.5's #2 and #3 binding mechanisms too, which bind
  entries that *exist*. It is the cheapest attack on R34 and it was previously unnamed.
  Enforcement lever: §12.7 / slice S10 (`chain_required` in the principal/manifest + a canonical
  store path). Note the naming hazard flagged in §0: `acc_a6_chain_mandatory_append_only` tests
  append-only-ness, not mandatory recording.
- **Auditor capacity — "an auditor can reconstruct" is unautomated and linear in N (recorded
  2026-07-31, third fold-in).** Even with source re-hashing wired (§12.8), verifying *what ran*
  semantically means a human reading N programs, increasingly machine-generated ones. R34
  provides no summarization, no policy predicate, and no risk ranking, so the audit step does
  not scale with the run volume R34 is designed to record. This is a human-review-capacity
  non-closure, not a cryptographic one, and it is named here so it is not mistaken for a solved
  part of §1.1's reconstruction claim. Lever: §12.8 (per-entry `effect_union`/risk in the
  entry's committed `aux`, turning the audit into a machine-checkable query).
- **Authority/AI-envelope substitution (recorded 2026-07-31, third fold-in).** Because the
  preimage binds source bytes only (§4.2 scope note), an adversary needs one benign program
  approved and hash-listed once; re-running it under a different principal, a widened
  `AXON_VM_ALLOWED_EFFECTS`, a larger budget, no seccomp policy, or a different AI model
  produces a chain entry indistinguishable from the approved run. Lever: §12.6 / slice S9.
- **Stamp-without-run / fabricated export by a merely dishonest proposer (recorded 2026-07-31).**
  Chain entries are proposer-constructed and unsigned, and the landed `axon-vm chain stamp` verb
  appends an entry without executing the program. A dishonest (not root-compromised) proposer can
  stamp a compliance program it never ran, or fabricate an internally-consistent export from
  scratch, and pass the §4.5 voter check. The §4.5 composition is therefore
  deterrence/forensics, not prevention, until a §12.5 binding mechanism (run-path-only entry
  appends, signed/attested entries, or R35 external tip anchoring) lands.
- **Chain truncation before export.** A VM operator can truncate `chain.jsonl` to hide the last
  K entries before exporting it to the auditor. Mitigation: the relying party holds the last known
  chain tip; any export whose `head` differs from the known tip is suspicious. Full mitigation
  (publishing chain tips to an external log) is R35, out of scope.
- **Root-compromised VM rewriting the chain.** An attacker with root can delete and rewrite
  `chain.jsonl` from scratch, constructing a plausible-looking chain. R34 detects tampering of
  an existing chain by an outsider but cannot detect a root attacker who controls the chain file
  and the `boot_root` metadata. External log anchoring (R35) is the defense.
- **Fake timestamp ordering.** `timestamp_ms` is informational. A VM operator can set any
  timestamp. The chain guarantees append-order (`seq`), not real-time order.
- **Source file deletion.** After a run, the source file can be deleted. The chain retains only
  `sha256(source)` — the auditor cannot verify the chain without the source text. Operators who
  need auditability MUST retain source files alongside the chain. R34 does not mandate source
  retention.

### 8.3 Invariants (assert in tests)

- **I-1 (append-only seq).** `entries[i].seq == i` for all i. No gaps, no duplicates. A chain
  with any other seq layout is `Malformed`. *(NOT enforced as built — the landed verifier never
  checks `seq`; see §12.1's seq bullet, recorded 2026-07-31.)*
- **I-2 (linkage chain).** `entries[0].chain_before == boot_root`; `entries[i].chain_before ==
  entries[i-1].chain_after` for all i > 0. The chain is a linked list by construction.
- **I-3 (head consistency).** `head == entries.last().chain_after` (or `boot_root` if empty).
  A `RunChain` whose `head` does not match the last entry's `chain_after` is `Malformed`.
- **I-4 (prefix distinctness).** `boot_root` always starts with `"axtcb1-ext:"`; each
  `chain_after` always starts with `"axtcb1-run:"`. Mixing them is `PrefixMismatch`.
- **I-5 (source hash, not source).** `prog_hash` is always `hex(sha256(source_bytes))`; it is
  never the source text, a path, or any other encoding. The 64-character lowercase hex form is
  the only valid representation.
- **I-6 (formula immutability).** The preimage assembly in §4.2 — version tag, field order, byte
  widths, endianness — is a **protocol**. Any change requires a version bump to `axon-run-v2\n`
  and a new spec revision. Do not silently alter the formula and expect existing chain files to
  still verify.

### 8.4 Edge cases

- **Empty chain (no runs yet).** `head = boot_root`; `entries = []`. `chain show` prints the
  boot root and "0 runs recorded." `chain verify` with an empty chain and no source files
  returns `Ok(())` (trivially valid). `--verify-chain boot_root_value` on `axon-vm run` passes.
- **Very long chain (millions of entries).** `verify_chain` is O(N) sequential — it never loads
  the full file into memory. The `chain_store.rs` I/O seam reads JSONL line-by-line. The
  acceptance test need not test large N; the O(N) claim is asserted by inspection of the
  implementation, not a benchmark. *(Scope note, 2026-07-31: this streaming property applies to
  local `chain verify` only. The §4.5 embedded-export voting path deserializes the entire
  export in memory inside one JSON frame — it is bounded by §4.5's normative size cap, not
  streaming.)* *(Scaling limit, 2026-07-31 third fold-in: "millions of entries" and §4.5's
  65,536-entry fail-closed cap are consistent only for local `chain verify`. For the voting
  path, one entry = one run and chains are per-boot + append-only with no pruning or checkpoint,
  so an agent-rate workload crosses the cap within hours and chain-aware quorum then fails
  closed permanently until reboot. See §4.5's scaling-limit block and §12.9; the cap itself must
  not be raised as a workaround.)*
- **Concurrent `axon-vm run --chain` on the same `chain.jsonl`.** `O_APPEND` is atomic at the
  kernel boundary for line-sized writes on POSIX. However, two concurrent appends may produce
  two entries at the same `seq` (both computed from the same `prev_chain`), violating I-1.
  The `chain_store.rs` implementation MUST use a POSIX advisory lock (`flock`) around the
  read-head + compute + append cycle. Concurrent runs are serialized at the lock; this is by
  design (the chain is a linear sequence, not a DAG).
- **Program source encoding.** `program_source_bytes` are the raw bytes of the `.ax` file as
  read from disk — no newline normalization, no BOM stripping. The auditor must reproduce the
  exact same bytes. Operators MUST not post-process source files after the run (e.g., auto-format
  or re-encode). The chain is over the bytes that were executed, not a canonical form.

---

## §-Gate — `scripts/r34_acceptance_gate.sh` (pinned; FAILS if any §0 check missing or stubbed)

The gate is the single source of "done." It MUST:

1. **Presence check** — `grep` the R34 test sources and assert every named check from §0 exists:
   `acc_a1_smoke_chain_journey`, `acc_a2_chain_tamper_detected`, `acc_a3_quickstart_commands_execute`,
   `acc_a4_hermetic_isolated_timeout`, `acc_a5_order_sensitive`, `acc_a6_chain_mandatory_append_only`,
   `source_hash_committed_not_source`, `chain_composes_with_r31`, `auditor_can_reconstruct`,
   `chain_exported_and_imported`.
   Any missing name → **gate fails**.

2. **Anti-stub check** — assert no acceptance or adversarial test body is `#[ignore]`d /
   `todo!()` / `unimplemented!()` / `assert!(true)`. No carve-outs for R34: all dependencies
   (R31, R28 JSONL format) are already shipped.

3. **Anti-vacuous tamper check** — assert `acc_a2_chain_tamper_detected` exercises all four
   tamper variants: prog_hash flip, chain_after flip, entry removal (seq gap), and entry swap
   (reorder). A test that exercises fewer than 4 variants passes vacuously; the gate counts
   tamper variants by checking for distinct `ChainError` discriminants in the test body and fails
   if fewer than 4 are asserted.

4. **Formula protocol check** — assert `acc_a5_order_sensitive` contains at least two distinct
   input orderings of the same program set and asserts their final chain values differ AND their
   first entries' `chain_after` values differ (divergence at position 0, not just the tail).

5. **Boot-root linkage check** — assert `chain_composes_with_r31` explicitly constructs a
   `prev_chain_str` with the `"axtcb1-ext:"` prefix (not `"axtcb1-run:"`); the test body must
   contain the literal string `"axtcb1-ext:"` and verify the prefix-stripping path.

6. **R26/R31/R33 regression check** — run `cargo test -p axon-attest` and `cargo test -p axon-vm`
   with the existing R26, R31, and R33 test suites and assert all still pass. R34 must not
   regress any prior spec.

7. **Run** `cargo test -p axon-attest` (all R34 unit tests green) **and** execute the §9
   quickstart commands verbatim **and** run `acc_a1` driving the real CLI.

8. Exit 0 only if all of the above pass; print which check failed otherwise. Wire
   `r34_acceptance_gate.sh` into `gate.sh --strict`.
   *(Status 2026-07-31: NOT yet wired — `scripts/gate.sh` never invokes it, so the gate only
   kills when run by hand. This item remains required, tracked as open in §12.2. Note also that
   the as-built gate's presence-check list diverges from the §0 names — §12.2 reconciles which
   list is normative.)*

---

## §9 — Quickstart (these exact commands are executed by `acc_a3`)

```bash
# R34 Incremental Attestation — rolling hash chain

# 1. Initialize chain for this VM (seeded from R31 axtcb1-ext: at boot):
axon-vm chain show --vm-id demo-vm
# → vm_id: demo-vm
#   boot_root: axtcb1-ext:…
#   runs: 0
#   head: axtcb1-ext:…   (no runs yet; head = boot_root)

# 2. Run a program and extend the chain:
axon-vm run examples/hello.ax --chain --vm-id demo-vm
# stderr: axtcb1-ext: axtcb1-ext:…   axtcb1-run-0: axtcb1-run:…

# 3. Run two more programs (chain grows by 2 entries):
axon-vm run examples/math.ax --chain --vm-id demo-vm
axon-vm run examples/structs.ax --chain --vm-id demo-vm

# 4. Inspect the chain (3 entries, new head):
axon-vm chain show --vm-id demo-vm --json | jq '{entries: (.entries | length), head: .head}'
# → { "entries": 3, "head": "axtcb1-run:…" }

# 5. Verify the full chain against source files:
axon-vm chain verify --sources-dir examples/ --vm-id demo-vm
# → ✓ chain verified: 3/3 entries valid

# 6. Demonstrate tamper detection — change a source file and re-verify:
#    ⚠ NOT BUILT (corrected 2026-07-31, third fold-in). This step documents behavior the landed
#    binary does not have: `chain verify` takes no `--sources-dir` (`ChainCmd::Verify { store,
#    genesis }`), and neither `chain verify` nor `chain verify-export` re-hashes program sources
#    against `prog_hash` — the source re-hash was recorded as an accepted scope reduction in §13
#    S8. Both landed verifiers therefore check internal linkage ONLY, and no `SourceMismatch`
#    can be produced. Do not cite this step as evidence of content verification, and do not add
#    it to `acc_a3` until §12.8 / slice S11 wires `--sources-dir` in.
#    The landed source-independent tamper demo (chain-file corruption) is what
#    `scripts/r34_acceptance_gate.sh` exercises today.
# cp examples/hello.ax /tmp/hello_orig.ax
# echo "// tampered" >> examples/hello.ax
# axon-vm chain verify --sources-dir examples/ --vm-id demo-vm
# → intended: ✗ chain verify failed at seq=0: SourceMismatch; exit 15   [S11, not yet built]
# cp /tmp/hello_orig.ax examples/hello.ax

# 7. Demonstrate order-sensitivity — same programs, different order produces different chain:
axon-vm run examples/math.ax --chain --vm-id order-test-a
axon-vm run examples/hello.ax --chain --vm-id order-test-a
axon-vm run examples/hello.ax --chain --vm-id order-test-b
axon-vm run examples/math.ax --chain --vm-id order-test-b
axon-vm chain show --vm-id order-test-a --json | jq -r '.head'
axon-vm chain show --vm-id order-test-b --json | jq -r '.head'
# → two different "axtcb1-run:…" values

# 8. Export chain for an auditor:
axon-vm chain export --out /tmp/chain_export.json --vm-id demo-vm
# → wrote axon-chain-export/1 with 3 entries to /tmp/chain_export.json

# 9. Auditor verifies the exported chain (no live VM required):
axon-vm chain verify-export /tmp/chain_export.json --sources-dir examples/
# → ✓ export verified: 3/3 entries valid; head matches export.head

# 10. Require a specific chain state before running (fail-closed):
EXPECTED=$(axon-vm chain show --vm-id demo-vm --json | jq -r '.head')
axon-vm run examples/options.ax --chain --verify-chain "${EXPECTED}" --vm-id demo-vm
# → runs; chain now has 4 entries
axon-vm run examples/while.ax --chain --verify-chain "${EXPECTED}" --vm-id demo-vm
# → ✗ chain tip mismatch: expected axtcb1-run:…(old), got axtcb1-run:…(new); exit 15
```

---

## §-Definition of Done

**Per slice (S1–S8):** the slice's named checks existed, were seen to fail, now pass; the full
`axon-attest` suite is green; no regression in R26, R31, or R33 tests or the workspace.

**Per milestone (R34 complete):**
- `axon-vm run PROG.ax --chain` extends the chain; new `axtcb1-run:N` printed to stderr;
  entry appended to `chain.jsonl` (`acc_a1` passes).
- The chain formula in §4.2 is implemented byte-for-byte; same inputs → same output across
  any two invocations (`acc_a4` passes).
- Same programs in different order produce different chain values (`acc_a5` passes).
- Tampering any entry (hash, linkage, **interior** removal, reorder) is detected by
  `chain verify` (`acc_a2` passes for all 4 tamper variants).
- **Truncation of the TAIL is NOT detected by `chain verify` alone, and cannot be**
  (AUDIT T31, findings OSK-P7-H3 / P7-KRN-06 / P6-COV-02). Every prefix of a valid
  chain is itself a valid chain, so chopping off the last runs — the exact move an
  operator hiding a run would make — verified clean (`CHAIN OK: 1 entries`, exit 0),
  as did erasing the chain outright (`CHAIN OK: 0 entries`). The auditor path was
  equally blind: an attacker who truncates and re-exports emits a `head` consistent
  with the shortened entry list, so `verify-export`'s internal head check passed too.
  `--genesis` pins the ROOT; truncation moves the TIP.

  Closing it requires a pin the attacker does not control, which must come from
  outside this crate. `chain verify` / `chain verify-export` therefore accept
  `--expect-head <hash>` and `--expect-count <n>`; a relying party that records the
  tip it last saw detects any rollback (`pinned_verify_detects_a_truncated_tail`,
  `pinned_verify_export_detects_truncate_then_reexport`). Unpinned output now says
  so explicitly rather than implying completeness.

  **Open (needs a decision):** nothing in the system yet *stores* a pin, so the
  guarantee is only as good as the relying party's own bookkeeping. Where the tip
  should be persisted — R33 quorum state, the R28 ledger, or an external
  attestation service — is an architecture call, not a code fix.
- Attempting to re-extend from a stale root is refused (`acc_a6` passes).
- Chain root equals R31 `axtcb1-ext:` boot measurement (`chain_composes_with_r31` passes).
- Export → import → verify round-trip succeeds (`chain_exported_and_imported` passes).
- Auditor reconstruction recomputes all intermediate hashes (`auditor_can_reconstruct` passes).
- `scripts/r34_acceptance_gate.sh` exits 0 with every §0 check green.

Only then is R34 done.

*(Reconciliation note, 2026-07-31: several check names above belong to the accepted scope
reduction recorded in §13 S8 and do not exist as built — §12.2 defines which list is normative
for "R34 done".)*

---

## §-Notes for the implementer (do NOT deviate without updating this spec)

- **The chain formula is a protocol, not an implementation detail.** Every field in the preimage
  (order, byte width, endianness, prefix stripping, version tag position) is normative. If you
  find the formula inconvenient, update the spec and bump to `axon-run-v2\n`; do not silently
  adjust the preimage and expect existing chain files to remain verifiable.
- **Keep `chain.rs` pure.** It receives `&[u8]` and `&str` values; it returns `String` or
  `Result`. No `std::fs`, no `std::time`, no `uuid` generation inside the pure core. If you
  reach for a file path or a clock inside `chain.rs`, you are in `chain_store.rs`'s job.
- **The `"axtcb1-run:"` prefix is load-bearing.** Do not accept a `prev_chain_str` with an
  unexpected prefix (e.g., raw hex without a prefix) — it must be either `"axtcb1-ext:"` (for
  the genesis entry) or `"axtcb1-run:"` (for subsequent entries). Any other prefix is
  `PrefixMismatch` and MUST fail closed, not fall through to a hash with garbage bytes.
- **`run_id` must be unguessable.** Use `uuid::Uuid::new_v4()` (CSPRNG-seeded). Do not use
  sequential IDs, timestamps as IDs, or any deterministic scheme. The unpredictability of `run_id`
  is what prevents precomputation attacks.
  *(As-built violation, recorded 2026-07-31: both `cmd_run` and the `chain stamp` default
  generate `run_id` as `format!("vm-{pid}-{nanos}")` — pid + wall-clock nanos, a guessable
  deterministic scheme — and the CLI accepts an arbitrary caller-supplied `--run-id`. The §8.1
  "future-run prediction" closure is therefore NOT achieved as built; open fix tracked in
  §12.3.)*
- **`O_APPEND` + `flock` for concurrent safety.** The read-head + compute + append cycle must be
  atomic at the file level; use `flock(LOCK_EX)` before reading `head()` and release it after the
  `append()` write. Do not rely on `O_APPEND` alone to prevent seq collisions under concurrency.
- **Source bytes, not paths.** `chain store` passes `&[u8]` to `extend_chain`; the CLI reads the
  source file and injects the bytes. The chain does not record the source file path — only the
  hash. If the operator moves the source file, the auditor must locate the right bytes by hash.
- **R34 changes no R26 isolation, no R27 kill-switch, no R31 measurement algebra, no R33 voting
  logic.** If you are editing `measure.rs`, `latch.rs`, `aggregator.rs`, or `substrate.rs`, you
  are out of R34's scope. The R33 `VoteRequest` extension (§4.5) is an additive optional field
  change; it MUST be backward-compatible (old voters ignore unknown fields).

---

## §12 — As-built reconciliation & open questions (added 2026-07-31, review fold-in)

The §2/§3/§5 body above is the original design text; the implementation deliberately simplified
several of its shapes. Because this spec's audience contract (an implementer "who builds strictly
against this document and reads only it") makes the body normative, the divergences are recorded
here as **superseding** the body where they conflict. An S7 implementer MUST build against the
as-built shapes in §12.1, not the §2/§3/§5 originals.

### 12.1 Landed module, data-model, and CLI divergences (supersede §2/§3.1/§3.3/§5)

- **Module path:** the chain lives in a single `crates/axon-vm/src/chain.rs` (chain logic +
  store + export together). `crates/axon-attest/src/` contains only `lib.rs` — the §2 layout
  (`axon-attest/src/chain.rs` + `chain_store.rs` + `chain_export.rs`) was not built.
- **`ChainEntry` field names:** `prev_hash` / `entry_hash`, NOT §3.1's
  `chain_before` / `chain_after`. The `axon-chain-export/1` schema serializes
  `prev_hash`/`entry_hash`, so the §5.4 example JSON's field names are superseded — a voter
  built to §5.4's names cannot deserialize a genuine export.
- **Storage:** a caller-supplied `--store PATH` JSONL file. No `~/.axon/chains/<vm-id>/`
  directory layout, no per-vm-id addressing, no `meta.json`; the genesis is derived from the
  kernel image (`chain_genesis`) rather than read from a metadata file.
- **CLI flags:** `axon-vm run` takes `--chain-stamp PATH` (not `--chain`); there is no
  `--verify-chain EXPECTED_TIP` flag — the landed run-time precondition verifies the *whole
  chain* against genesis before stamping (broken chain → exit 15, VM never launches), not an
  expected-tip equality check.
- **Standalone stamping:** `axon-vm chain stamp PROG` appends an entry with no execution at all
  (see §8.2 stamp-without-run threat and §12.5).
- **`seq` is recorded but NOT verified (added 2026-07-31, second fold-in):** the landed
  verification core `verify_entries` checks only `prev_hash` linkage and the recomputed
  `entry_hash`. There is no `entry.seq == expected_seq` check, no `ChainError::SequenceGap`
  anywhere, and `seq` is not in the §4.2 preimage — so a chain whose seq fields are all 0,
  duplicated, or arbitrary verifies `Ok`, and the `Err(entry.seq)` failure report names an
  attacker-controlled value. §4.3's "no gaps" assertion, §6 S2's slice-risk requirement, §8.1's
  SequenceGap detection, and invariant I-1 are therefore all UNENFORCED as built; `seq` is
  unauthenticated informational metadata. Entry removal and reordering ARE still detected — via
  the hash-linkage break, not via seq. **Open fix:** either add the one-line seq-contiguity
  check to `verify_entries` (plus a seq-gap tamper test), or amend §4.3/§8.1/I-1 to state that
  ordering integrity rests solely on hash linkage and `seq` is informational. Note this is
  precisely the gap the waived §-Gate item 3 (4-tamper-variant counting, incl. seq-gap) would
  have surfaced — §12.2 reconciled gate *names* but silently dropped that anti-vacuous
  mechanism; do not treat the name reconciliation as covering it.
- **`chain verify` genesis default is fail-open (added 2026-07-31, second fold-in):** with no
  `--genesis`, the CLI uses the chain's own first entry's `prev_hash` as the genesis
  (self-consistency check only, per its own help text) — a wholly fabricated chain with an
  arbitrary genesis passes. This contradicts §1.3's "fail closed on every ambiguity" and §4.3's
  contract, which makes `boot_root` a required input. Relying-party/auditor contexts MUST pass
  `--genesis` explicitly; open fix: require `--genesis` (or at minimum warn loudly) in
  `chain verify`.
- **Mock-genesis fallback on missing files (added 2026-07-31, second fold-in):** `chain_genesis`
  returns the CI mock genesis ("NOT a real attestation") not only under `AXON_CI_NO_KVM=1` but
  whenever the kernel or `axon-os` binary path merely does not exist — a typo'd `--kernel`
  silently yields an R31-unanchored chain with only an stderr warning, and `run --chain-stamp`
  proceeds. Open fix: make the mock fallback opt-in (the env flag only); file absence should
  fail closed.

### 12.2 Normative test list + gate wiring (reconciles §0 / §-DoD / §-Gate vs. §13 S8)

- The **as-built check names in §13 and `scripts/r34_acceptance_gate.sh`** (e.g.
  `entry_hash_deterministic`, `verify_wrong_genesis_breaks_at_zero`, …) are **normative for the
  landed slices S1–S6/S8**. Of the §0 names, only `chain_composes_with_r31` and
  `chain_exported_and_imported` exist; the remaining `acc_a1..acc_a6` /
  `source_hash_committed_not_source` / `auditor_can_reconstruct` names are part of the accepted
  scope reduction recorded in §13 S8. "R34 done" therefore means: `r34_acceptance_gate.sh`
  exits 0, every §13 row is landed, and S7's §4.5-pinned tests pass — NOT the literal §0/§-DoD
  list, which is retained as the original design target.
- **Open:** `r34_acceptance_gate.sh` is not wired into `gate.sh --strict` (§-Gate item 8
  unmet). Until wired, the kill-gate runs only by hand.

### 12.3 Open: `run_id` unguessability violated as built

Both `cmd_run` and the `chain stamp` default use `format!("vm-{pid}-{nanos}")` — deterministic
and guessable — violating §-Notes and leaving §8.1's future-run-prediction threat open. Fix:
switch the default to `uuid::Uuid::new_v4()` — this requires **adding** the `uuid` crate
(`v4` feature) or `getrandom` to `axon-vm` as a new dependency, explicitly authorized in §1.3
*(corrected 2026-07-31, second fold-in: the earlier "`uuid` is already in-scope per §1.3" claim
was false — no workspace crate depends on `uuid` — and §1.3 simultaneously said "Allowed new
deps: none", so this fix as previously written was unbuildable)* — or amend §8.1/§-Notes to
drop the precomputation-resistance claim. Until fixed, do not cite precomputation resistance as
a property of the chain.

**The default-only fix is insufficient (corrected 2026-07-31, third fold-in).** The landed CLI
exposes `ChainCmd::Stamp { prog, run_id: Option<String>, store, kernel }` — an arbitrary
caller-supplied `--run-id`, validated for neither shape, entropy, nor uniqueness. Changing only
the *default* leaves the property entirely at the adversary's discretion: any actor that reads
this section simply passes `--run-id`, and a capable generator reading this spec will. That is
the spec assuming a careless rather than an optimizing counterparty. **The fix MUST therefore
be one of:** (a) validate — reject any `--run-id` that is not a well-formed v4 UUID or that is
already present in the store; or (b) remove the flag entirely and always generate internally,
adding a separate unhashed, informational `--correlation-id` field for callers who need to join
to their own records. Whichever is chosen, §8.1's future-run-prediction bullet MUST be restated
to say the property holds only for internally-generated ids. Tracked as slice S10.

### 12.4 Open: concurrent-writer `flock` not built

The §8.4/§-Notes requirement (advisory `flock` around read-head + compute + append) was not
built; concurrent stamps against the same `--store` file can collide at the same `seq`,
violating I-1. Single-writer operation is safe. This is the gap the §13 S3 status flags; it
needs either a dedicated slice or an explicit accepted-gap entry before R34 can claim
concurrency safety.

### 12.5 Open: binding chain entries to attested execution (prerequisite for §4.5-as-enforcement)

Candidate mechanisms. **The enforcement upgrade requires #2 or #3** (corrected 2026-07-31,
second fold-in — the earlier "one of which" wording wrongly allowed #1 alone to qualify):

1. *(Defense-in-depth only — necessary-but-NOT-sufficient.)* Restrict run-entry appends to the
   attested `run --chain-stamp` path (mark or gate the standalone `chain stamp` verb so its
   entries are distinguishable). This cannot deliver enforcement even if built, on two grounds
   visible in the landed code: (a) `cmd_run` verifies and stamps the chain **before** the
   Firecracker launch, so a run-path-only entry proves only that the CLI was invoked with that
   program — the VM may fail to launch or the program may never execute after the stamp; a
   post-execution completion record would also be needed; and (b) the proposer runs `axon-vm`
   on their own host and entries carry no signature, so a remote voter cannot distinguish an
   entry appended by the run path from one appended by any other code — a client-side
   restriction is remotely unverifiable.
2. Sign chain entries with a key bound to the R31 attested boot (unforgeable by a userland
   proposer) — adds relying-party-verifiable evidence.
3. R35-style external tip anchoring, making after-the-fact fabrication detectable by any relying
   party holding the anchored tip — adds relying-party-verifiable evidence.

§4.5 may be described (or built) as an enforcement primitive only once #2 or #3 lands; landing
#1 alone does NOT lift the §4.5 enforcement caveat.

### 12.6 Open: the v1 preimage has no extension point — do the `axon-run-v2` bump ONCE (added 2026-07-31, third fold-in)

§4.2's preimage is a fixed 5-field concatenation, I-6 declares it immutable absent an
`axon-run-v2\n` bump, and the landed `ChainEntry` (`crates/axon-vm/src/chain.rs`) has no
reserved or extensible field. Every upgrade this spec already contemplates therefore collides on
the same one-time protocol break: §12.5 #2 (signed entries), §12.5 #3 (external tip anchoring),
#1's run-path-vs-`chain stamp` origin discriminator, and the §4.2 authority/AI-envelope
commitment. Each is currently specified as a future edit to a formula the spec forbids editing —
which makes the enforcement upgrade far more expensive than it needs to be, and in practice
means it does not happen.

**Proposed resolution (decide before S7 is relied on; slice S9):** do the bump once, now.

```
preimage_v2 = b"axon-run-v2\n" || prev_chain_bytes(32) || prog_hash_bytes(32)
           || aux_hash(32) || run_id_bytes || timestamp_le(8)

aux_hash = SHA-256(canonical_json(aux))
aux      = { principal, allowed_effects, budget_tokens, seccomp_bpf_b64_hash,
             seed, ai_model_ids, origin: "run" | "stamp", … }   // open, serde-default object
```

`MmdsPayload` is already exactly this struct and is already computed before the stamp in
`cmd_run`, so the producer side is a few lines plus a `#[serde(default)] aux` field on
`ChainEntry` for the auditor to re-derive. v1 verification is retained for existing chains via
the version tag — the tag is what makes this safe. After the bump, #1/#2/#3 and the §12.8
audit-summary fields all become **additive** (new keys in `aux`) rather than further protocol
breaks. **Open question left deliberately unanswered here:** whether `aux` should be a fully
open object (maximally future-proof, but two producers can disagree on canonical JSON) or a
closed, versioned struct (deterministic, but each new field is a spec revision). Decide in the
S9 slice; do not decide it by implementation drift.

### 12.7 Open: making chain participation unavoidable (added 2026-07-31, third fold-in)

The §8.2 run-without-stamp threat has no mitigation as built, and the framing box's guarantee is
narrowed to *recorded* runs because of it. Two levers, both usable together (slice S10):

1. **Policy flag.** A `chain_required: true` field in the loaded `Principal` (the registry
   already parsed by `axon-vm`) or in the `.axmeta` manifest, making `cmd_run` exit non-zero
   when `--chain-stamp` is absent. This is a local-policy control: it binds an *honest* operator
   and an *administered* deployment, and it does not bind a proposer who edits their own
   principal file — say so wherever it is documented.
2. **Canonical store path.** Derive the store path from vm-id (the §3.3 `~/.axon/chains/<vm-id>/`
   layout the as-built simplification dropped) instead of accepting `--store PATH`, removing the
   "stamp into a second, discarded chain" variant.

**Open question:** neither lever is remotely verifiable by a voter, for the same reason §12.5 #1
is not — the proposer runs `axon-vm` on their own host. Remote unforgeability of *participation*
(as opposed to entry integrity) may be reducible to §12.5 #3 anchoring plus an expected-rate or
heartbeat commitment, or it may be genuinely out of reach without attested execution. R34 does
not answer this; it is recorded so no reader assumes S10 closes it.

### 12.8 Open: content verification and auditor scale (added 2026-07-31, third fold-in)

Two coupled gaps, tracked as slice S11:

- **Re-classify the source re-hash from "accepted scope reduction" to required.** §13 S8 records
  `--sources-dir` re-hashing as never wired into `chain verify`/`verify-export`, and the landed
  CLI confirms it. But the source re-hash is *the only content check in the entire design* —
  without it, `prog_hash` is an opaque 32-byte value with no tie to any program that exists, and
  both landed verifiers check only internal linkage. §9 step 6 (now corrected) documented an
  outcome the binary cannot produce. Wire `--sources-dir` into both verifiers, and use a
  content-addressed source store (`<dir>/<prog_hash>.ax`) so retention is mechanical rather than
  operator discipline (§8.2 already names source deletion as an open threat; content addressing
  makes it detectable rather than merely unmitigated).
- **Make the audit a query, not a reading assignment.** Pair the re-hash with a machine-checkable
  per-entry summary — `effect_union` (already parsed from `.axmeta` in `cmd_run`) and derived
  risk level, committed via §12.6's `aux_hash` so it is not merely advisory metadata. Then an
  auditor asks "did any entry exceed effect ceiling X?" instead of reading N generated programs.
  **Open question:** what the minimum sufficient summary is — effect union alone, or effects +
  budget + risk + model id — is a policy question this spec should not answer unilaterally;
  raise it with the R28/R29 owners, since the same summary wants to exist in the audit ledger.

### 12.9 Open: O(1)+proof chain transport before chains outgrow the cap (added 2026-07-31, third fold-in)

§4.5's fail-closed 65,536-entry cap is correct and MUST NOT be raised as a workaround; what is
missing is a transport that does not need the whole chain. Two candidate designs (slice S12):

(a) **Merkle-ize the chain**, so a voter receives `{ tip, inclusion proofs for the required
entries, length }` instead of the full export — O(log N) per required hash, and it composes with
§12.5 #3 anchoring (anchor the Merkle root).
(b) **Signed checkpoint entries** — an entry whose payload commits to the digest of all prior
entries, so an export may start from the last checkpoint rather than genesis.

**Open question:** (a) and (b) trade differently against §4.1's "each boot is a new chain, chains
are never merged" rule and against §4.4's append-only linearity; (b) in particular needs a rule
for who may emit a checkpoint and how a voter knows a checkpoint is not itself a truncation
(this is the §8.2 truncation threat wearing a hat). Do not build either until that rule is
written. Until S12 lands, §4.5's required `voter_behavior_at_100k_entry_chain` check documents
the real, currently-failing behavior — that check MUST NOT be waived to make the gate green.

### 12.10 Open: property-based compliance requirements alongside identity (added 2026-07-31, third fold-in)

Per §4.5's identity-vs-property caveat, add a requirement mode built on machinery already landed
(slice S11):

- `required_effect_ceiling: Vec<String>` — checked against a per-entry `effect_union` (committed
  via §12.6 `aux`), semantic and stable across regeneration.
- Requirement by **approved-AST identity** rather than source bytes, using the `<file>.ax.approved`
  sign-off record that `axon ast approve` already writes — stable across cosmetic regeneration.

Both are **additive to**, never substitutes for, §4.5 steps 1–3, and both remain subject to the
§4.5 enforcement caveat (a dishonest proposer can stamp a compliant-looking entry either way).
**Open question:** whether an approved-AST identity should be the AST's canonical digest or the
approval record's own hash — the former survives re-approval, the latter binds the human
sign-off. This bears on R22/R24's approval-gateway semantics; resolve jointly, not here.

### 12.11 Open: join the chain run_id to the provenance run-id (added 2026-07-31, third fold-in)

`stamp_chain`'s doc-comment states the intent — a `run --chain-stamp` entry "carries the exact
same run-id as the rest of that run's provenance" — but that is `axon-vm`'s `vm-{pid}-{nanos}`
id, propagated into the guest via MMDS, while the interpreter unconditionally mints its own
(`let run_id = generate_run_id();` in `crates/axon-core/src/main.rs`) before writing
`run_start { run_id, seed, src }` to the provenance log. The two are separate namespaces, so
`axon trace --replay <chain entry run_id>` cannot resolve, and the chain is disconnected from the
project's strongest landed evidence machinery: deterministic `(Trace, Seed)` replay,
`AXON_AI_REPLAY` memoized `ai_complete`, `axon trace --ai --json` (`axon-ai-audit/2` — effect_row,
principal, model, cost per call), and the R28 hash-linked `axon-audit` ledger.

**Proposal (slice S9, alongside the v2 bump):** have the guest adopt the MMDS run_id instead of
minting its own, and commit the run's audit-ledger tip (`axon-audit`'s `Ledger` is already
hash-linked with an `export_json`) into the entry's `aux_hash`. A chain entry then proves
"this exact execution, replayable, with this audited effect sequence" rather than "this file's
hash was appended" — and §4.5 gains an optional voter step that may require a *replayable* entry.
**Open question:** whether requiring replayability at vote time is meaningful when the proposer
also supplies the replay inputs — it likely reduces to §12.5 #2/#3 again. Record the answer here
before building the voter step.

## §13 — Dependency DAG

| Node | Depends-on / blocked-by | Gate (named test or script) | Status |
|---|---|---|---|
| R34.S1 | R31-extended-tcb-attestation | `chain_composes_with_r31`, `entry_hash_deterministic`, `different_prog_hash_different_entry_hash` (`crates/axon-vm/src/chain.rs`) | landed |
| R34.S2 | R34.S1 | `verify_ok_three_entries`, `verify_detects_tampered_entry_hash`, `verify_empty_chain_ok`, `verify_wrong_genesis_breaks_at_zero`, `verify_malformed_json_line_is_clear_error` | landed |
| R34.S3 | R34.S1, R34.S2 | `ChainStore::append` (O_APPEND JSONL) exercised by S2's multi-entry tests; no separate flock/concurrency gate built this pass | landed (append-only single-writer; concurrent-writer flock NOT built — see §12.4; reference fixed 2026-07-31, previously pointed at a nonexistent §12) |
| R34.S4 (export/import) | R34.S3 | `chain_exported_and_imported`, `empty_chain_export_verifies_ok`, `verify_export_detects_tampered_entry`, `verify_export_detects_tampered_head` (`crates/axon-vm/src/chain.rs`) | landed |
| R34.S5 (`axon-vm run --chain-stamp` CLI) | R34.S1–S3 | `scripts/r34_acceptance_gate.sh` step 2 (stamp via real CLI) | landed |
| R34.S6 (`chain show`/`export`/`verify-export` subcommands) | R34.S4 | `scripts/r34_acceptance_gate.sh` §8 (show empty/3-entries, export, verify-export genuine + head-tampered) | landed |
| R34.S7 (R33 `VoteRequest` chain fields) | R33-cross-vm-safety-quorum (LANDED: `quorum/logic.rs` + `quorum/vsock.rs` + exit codes 13/14 in `main.rs`) | `voter_rejects_missing_required_prog_hash`, `voter_accepts_chain_with_required_subsequence`, `voter_rejects_tampered_chain_export`, `voter_rejects_missing_chain_export`, `old_voter_ignores_unknown_fields` (§4.5) | todo — build strictly against §4.5-as-rewritten + §12.1 as-built shapes (corrected 2026-07-31: the "R33 not landed in code yet" claim was stale; the earlier row also pinned no gate) |
| R34.S8 (acceptance gate) | R34.S1–S3, R34.S5, R34.S6 | `scripts/r34_acceptance_gate.sh` | landed (covers stamp/verify/tamper/wrong-genesis + show/export/verify-export/head-tamper; does NOT run the exact §9 quickstart script verbatim via `jq` — no dedicated `acc_a3_quickstart_commands_execute`/`acc_a1_smoke_chain_journey` test names exist in this simplified implementation, and `--sources-dir` re-hashing of program source files against `prog_hash` was never wired into `chain verify`/`verify-export` — a known, already-accepted scope reduction from the full spec, unchanged by S6) |

| R34.S9 (`axon-run-v2` preimage + `aux_hash` + run_id join) | R34.S1–S6; §12.6 open question (open `aux` object vs. closed versioned struct) answered first | `v2_entry_binds_authority_envelope`, `v1_chain_still_verifies_after_v2_bump`, `chain_run_id_resolves_via_axon_trace_replay` | todo — added 2026-07-31 (third fold-in). Do the bump ONCE: §12.5 #1/#2/#3, §12.6 envelope, §12.8 audit summary all collide on this single I-6 break |
| R34.S10 (mandatory recording + run_id integrity) | R34.S5; §12.7 (does NOT close remote unverifiability — see §12.7's open question) | `run_without_chain_stamp_refused_when_chain_required`, `caller_supplied_run_id_rejected_or_absent`, `duplicate_run_id_refused` | todo — added 2026-07-31 (third fold-in); closes the §8.2 run-without-stamp path locally and the §12.3 opt-out |
| R34.S11 (source re-hash + property-based requirements) | R34.S4, R34.S6; §12.10 open question (canonical AST digest vs. approval-record hash) coordinated with R22/R24 | `verify_detects_source_mismatch`, `verify_export_detects_source_mismatch`, `voter_rejects_entry_exceeding_effect_ceiling` | todo — added 2026-07-31 (third fold-in). **Re-classifies §13 S8's "accepted scope reduction" as required**: without it neither landed verifier checks anything about content |
| R34.S12 (O(1)+proof transport) | R34.S7; §12.9 open question (checkpoint authority + checkpoint-vs-truncation) answered IN WRITING first | `voter_behavior_at_100k_entry_chain`, `inclusion_proof_verifies_without_full_export` | todo — added 2026-07-31 (third fold-in). SHOULD precede any long-running deployment relying on §4.5; the 65,536 cap must not be raised as a substitute |

## §14 — Evidence ledger

| Claim | Verify command | Expected | Last verified (commit @ date) | Result |
|---|---|---|---|---|
| Core chain formula (§4.2) implemented byte-for-byte; deterministic; order/tamper/genesis-sensitive | `cargo test -p axon-vm --no-default-features chain::` | 11 passed, 0 failed | `ee485e8` @ 2026-07-18 | PASS |
| `scripts/r34_acceptance_gate.sh` — stamp twice, verify OK, corrupt, re-verify BROKEN exit 15, wrong-genesis exit 15 | `bash scripts/r34_acceptance_gate.sh` | exit 0, "R34 acceptance gate: ALL CHECKS PASSED" | `ee485e8` @ 2026-07-18 | PASS |
| No regression in R26/R31 attestation tests | `cargo test -p axon-attest` | all pass | `ee485e8` @ 2026-07-18 | PASS |
| Full axon-vm crate suite unaffected (incl. concurrently-landing R33 quorum tests) | `cargo test -p axon-vm --no-default-features` | 37 passed, 0 failed; **41 passed after S4** | `ee485e8` @ 2026-07-18; re-verified this commit | PASS |
| S4 export/import: export round-trips through JSON losslessly; `verify_export` catches a tampered entry AND a tampered head (not just individual links); shares one verification core with the live-store path (`verify_entries`) so the two cannot drift apart | `cargo test -p axon-vm --no-default-features chain::` | 15 passed, 0 failed | `6a620aa` @ 2026-07-18 | PASS |
| S6 CLI wiring: `chain show`/`export`/`verify-export` against the real binary — show reports correct entry count/head on an empty AND a 3-entry chain; export produces a valid `axon-chain-export/1` file; verify-export passes on a genuine export and reports `EXPORT BROKEN at seq 3` (exit 15) when only the `head` field is tampered (every individual link still recomputes cleanly) | `bash scripts/r34_acceptance_gate.sh` §8 | all 5 §8 checks pass | this commit @ 2026-07-18 | PASS |
| Review fold-in 2026-07-31: R33 landed shapes confirmed (`quorum/logic.rs` VoteRequest/VoteResponse — no score/blocked_reason); no `proposer_chain_tip`/`required_prog_hashes` anywhere **outside this spec** (the grep necessarily matches this spec's own §4.5/§14 text — the original "anywhere in specs or crates" wording was self-contradictory, corrected in the second fold-in); `chain stamp` = hash+append with no execution; `run_id` default = `vm-{pid}-{nanos}` in both paths; gate REQUIRED_NAMES ≠ §0 names; `gate.sh` does not invoke `r34_acceptance_gate.sh`; no §12 existed | `grep -rn proposer_chain_tip governance/ crates/` (expect hits only in this file); `grep -n "fn cmd_chain_stamp\|vm-{}" crates/axon-vm/src/main.rs`; `grep -n r34 scripts/gate.sh` | all as recorded in §12 | working tree @ 2026-07-31 | VERIFIED (spec corrected; §4.5 rewritten; §12 added) |
| Second (adversarial) fold-in 2026-07-31: no `uuid` dep in any workspace Cargo.toml (§1.3/§12.3 claim was false); `verify_entries` checks only prev_hash linkage + recomputed entry_hash — no seq check, no SequenceGap (chain.rs) and is private (`fn`, not `pub`) while `verify_export` is `pub`; `read_frame` allocates `vec![0u8; len]` from a wire u32 with no cap (quorum/vsock.rs); `cmd_run` stamps before the Firecracker launch and entries are unsigned (main.rs); `chain verify` without `--genesis` self-consistency-only; `chain_genesis` mock-falls-back on missing kernel/axon-os files, not just `AXON_CI_NO_KVM=1` | `grep -n uuid crates/*/Cargo.toml` (no hits); `grep -n "fn verify_entries\|pub fn verify_export\|entry.seq ==" crates/axon-vm/src/chain.rs`; `grep -n "vec!\[0u8; len\]" crates/axon-vm/src/quorum/vsock.rs`; `grep -n "genesis: Option\|AXON_CI_NO_KVM" crates/axon-vm/src/main.rs` | all as recorded | working tree @ 2026-07-31 | VERIFIED (§1.1/§1.3/§4.5/§8.1/§8.3/§8.4/§12.1/§12.3/§12.5/§14 corrected) |
| Third (ASI-trajectory) fold-in 2026-07-31: `--chain-stamp` is `Option<PathBuf>` read in exactly ONE place and nothing makes stamping mandatory (§8.2 run-without-stamp); `cmd_run` builds `MmdsPayload{principal, allowed_effects, budget_tokens, seccomp_bpf_b64,…}` — incl. the `AXON_VM_ALLOWED_EFFECTS` widen/narrow override — ~100 lines BEFORE the chain stamp, and none of it enters the §4.2 preimage; `ChainCmd::Verify{store, genesis}` and `VerifyExport{file}` take no sources argument and the doc-comment states source re-hashing is not wired (so §9 step 6 documented unbuildable behavior); `ChainCmd::Stamp` accepts an arbitrary unvalidated `--run-id`; the interpreter mints its own provenance run-id (`generate_run_id()`) rather than adopting the MMDS one; `effect_union` is already parsed from the manifest and `<file>.approved` is already written by `ast approve` (levers for §12.10) | `grep -rn chain_stamp crates/ scripts/`; `grep -n "MmdsPayload {" -A 10 crates/axon-vm/src/main.rs`; `grep -n "AXON_VM_ALLOWED_EFFECTS" crates/axon-vm/src/main.rs`; `grep -n "Verify {\|VerifyExport {\|run_id: Option<String>" crates/axon-vm/src/main.rs`; `grep -n "generate_run_id\|run_start" crates/axon-core/src/main.rs`; `grep -n "effect_union\|\.approved" crates/axon-vm/src/main.rs crates/axon-core/src/main.rs` | all as recorded in §1.2/§1.2b/§4.2/§4.5/§8.2/§12.6–§12.11 | working tree @ 2026-07-31 | VERIFIED (framing box narrowed to *recorded* runs; 4 expiring assumptions stated; slices S9–S12 added; no gate or fail-closed posture weakened — the §4.5 size cap and every `approved=false` path are unchanged) |
| Full workspace suite — a `wasm_browser_*` test occasionally fails (`wasm_browser_examples_run_identically_via_js_host` here; `wasm_browser_println_matches_interp_via_js_host` in a later run) when run inside the full parallel suite; **NOT caused by R34** (0/34 R34 files touched) | `cargo test --workspace --no-default-features` | 418/419 axon-core tests pass in the full-suite run | (observed, not caused by `ee485e8`) @ 2026-07-18; **re-checked 2026-07-18**: `bash scripts/wasm_browser_io_parity.sh` directly → 6/6 PASS; `cargo test --exact` on the single test → PASS in 0.57s; immediate full-suite rerun → 419/419 PASS, 0 failed | **FLAKE under full-workspace parallel contention, NOT a stable pre-existing gap** — do not dismiss a red `wasm_browser_*` test as "known" without an isolated rerun first; if the isolated rerun also fails, that IS a real regression |
