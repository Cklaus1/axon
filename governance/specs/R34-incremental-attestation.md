# Tech Spec — R34: Incremental Attestation (rolling hash chain — every `axon-vm run` extends the chain)

**Spec ID:** `R34-incremental-attestation`
**Status:** 🔧 Implementing (2026-07-18) — core rolling-hash chain (S1–S3 scope: `chain.rs`
formula + `ChainStore` append/verify + CLI `chain stamp`/`chain verify`/`run --chain-stamp`)
landed; S4 (export/import), S6 (`chain show`/`export`/`verify-export`), S7 (R33 `VoteRequest`
integration) NOT started — see spec-meta `status-claim` and §14 evidence ledger below.

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
  R33 spec text though R33 itself is not yet landed in code; 15 was genuinely unclaimed)
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
- `governance/specs/R33-cross-vm-safety-quorum.md` (in progress) — `VoteRequest` gains an optional
  `proposer_chain_tip` and `required_prog_hashes` extension for chain-aware quorum voting (§4)
**Audience:** an implementer who builds *strictly* against this document and reads only it.

> **Read this framing first.** R34 does **not** change the boot-time measurement algebra R31
> established, nor the quorum voting protocol R33 defines, nor any Axon-level containment. What R34
> changes is the *temporal scope* of the attestation guarantee: from "what software is running at
> boot" to "what programs ran, in what order, since boot." The load-bearing guarantee is:
> **each run irreversibly extends the chain** — a relying party who knows the current chain tip
> `axtcb1_run_N` can verify the complete ordered execution history by reconstructing the chain from
> program sources; no reordering, substitution, or removal of any run goes undetected.

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey: boot, run 3 programs, verify chain is valid and length = 3 | §5, §7 | `acc_a1_smoke_chain_journey` |
| **A2** | Tamper detection: replacing a run's entry changes all subsequent chain values | §4.2, §7 | `acc_a2_chain_tamper_detected` |
| **A3** | Quickstart commands execute verbatim: `axon-vm chain show` / `axon-vm chain verify` work | §8, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Chain verification is pure: no side effects; deterministic; no I/O in the core | §4.3, §7 | `acc_a4_hermetic_isolated_timeout` |
| **A5** | Order-sensitivity: same programs in different order produce different chain | §4.2, §7 | `acc_a5_order_sensitive` |
| **A6** | Append-only enforcement: re-extending from an older root is refused | §4.4, §7 | `acc_a6_chain_mandatory_append_only` |
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
the VM's chain shows that the compliance-checking program ran immediately before the operation.
With R34, the proposing VM includes its current `axtcb1_run_N` in the R33 quorum `VoteRequest`;
each voter can inspect the chain history and refuse to vote YES if the required compliance program
hash does not appear immediately before the operation (§4.5).

### 1.2 What it explicitly does NOT do

- **No program execution control.** R34 records what ran; it does not block or gate what may run.
  Execution policy remains the job of R27 (kill-switch), R11 (capabilities), and R33 (quorum).
  R34 is a forensic and audit primitive, not an enforcement primitive — but it composes with R33
  to create enforcement (§4.5).
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

### 1.3 Interface & tech constraints

- **Interface:** extends `axon-vm` with `chain show`, `chain verify`, `chain export`,
  `chain verify-export` subcommands; adds `--chain` and `--verify-chain EXPECTED_TIP` flags
  to `axon-vm run`.
- **Language/crate:** extends `crates/axon-attest` with `chain.rs` (pure chain logic) and
  `chain_store.rs` (I/O seam). Allowed new deps: none — `sha2`, `serde_json`, `serde` are
  already in scope; `uuid` is already used for run-ids elsewhere.
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

### 4.5 Integration with R33 (quorum chain-awareness)

R33's `VoteRequest` (§3.1 of R33) gains two optional fields:

```rust
// Extension for R34 (added to VoteRequest, both fields optional for backward compat):
proposer_chain_tip:   Option<String>,   // current axtcb1_run_N of the proposing VM
required_prog_hashes: Vec<String>,      // hex(sha256(source)) values that must appear in the chain
                                        // before the current tip, in the declared order
```

When `required_prog_hashes` is non-empty, each voting VM (§4.2 of R33):
1. Receives the proposer's exported chain alongside the `VoteRequest`.
2. Verifies the exported chain against `proposer_chain_tip` (calls `verify_chain` over the
   supplied entries; any chain error → vote NO with `score = 0`).
3. Checks that each hash in `required_prog_hashes` appears in the verified chain, in order, as
   a contiguous or non-contiguous subsequence of `prog_hash` values leading up to the current tip.
4. If the required sequence is absent: votes NO with `score = 0` and records
   `VoteResponse.blocked_reason = "chain_requirement_not_met"`.

This is the load-bearing R34 × R33 composition: a regulated deployment can require
`required_prog_hashes = [sha256(compliance_check.ax)]` in the `VoteRequest`, ensuring that no
action can receive quorum approval unless the proposing VM's chain proves the compliance-checking
program ran before the proposed action.

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
| **S7** | R33 `VoteRequest` extension fields + chain-aware voter logic (§4.5); backward compat (fields optional). | R33 voter tests (named in R33; not duplicated here) |
| **S8** | Acceptance gate `scripts/r34_acceptance_gate.sh`. | Gate exits 0 with every §0 check green. |

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
- **Fork-the-chain.** An attacker wants to create a branch: entries 0–2 on the main chain, then
  re-extend from entry 1 to hide a different run. `ChainStore::StaleRoot` refuses any `append`
  where `chain_before` is not the current file head. The JSONL file remains linear.

### 8.2 Threats R34 does NOT close (named, not hidden)

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
  with any other seq layout is `Malformed`.
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
  implementation, not a benchmark.
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
cp examples/hello.ax /tmp/hello_orig.ax
echo "// tampered" >> examples/hello.ax
axon-vm chain verify --sources-dir examples/ --vm-id demo-vm
# → ✗ chain verify failed at seq=0: SourceMismatch (prog_hash does not match source bytes); exit 15
cp /tmp/hello_orig.ax examples/hello.ax

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
- Tampering any entry (hash, linkage, removal, reorder) is detected by `chain verify`
  (`acc_a2` passes for all 4 tamper variants).
- Attempting to re-extend from a stale root is refused (`acc_a6` passes).
- Chain root equals R31 `axtcb1-ext:` boot measurement (`chain_composes_with_r31` passes).
- Export → import → verify round-trip succeeds (`chain_exported_and_imported` passes).
- Auditor reconstruction recomputes all intermediate hashes (`auditor_can_reconstruct` passes).
- `scripts/r34_acceptance_gate.sh` exits 0 with every §0 check green.

Only then is R34 done.

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

## §13 — Dependency DAG

| Node | Depends-on / blocked-by | Gate (named test or script) | Status |
|---|---|---|---|
| R34.S1 | R31-extended-tcb-attestation | `chain_composes_with_r31`, `entry_hash_deterministic`, `different_prog_hash_different_entry_hash` (`crates/axon-vm/src/chain.rs`) | landed |
| R34.S2 | R34.S1 | `verify_ok_three_entries`, `verify_detects_tampered_entry_hash`, `verify_empty_chain_ok`, `verify_wrong_genesis_breaks_at_zero`, `verify_malformed_json_line_is_clear_error` | landed |
| R34.S3 | R34.S1, R34.S2 | `ChainStore::append` (O_APPEND JSONL) exercised by S2's multi-entry tests; no separate flock/concurrency gate built this pass | landed (append-only single-writer; concurrent-writer flock NOT built — see §12 note) |
| R34.S4 (export/import) | R34.S3 | — | todo |
| R34.S5 (`axon-vm run --chain-stamp` CLI) | R34.S1–S3 | `scripts/r34_acceptance_gate.sh` step 2 (stamp via real CLI) | landed |
| R34.S6 (`chain show`/`export`/`verify-export` subcommands) | R34.S4 | — | todo (only `chain stamp`/`chain verify` built, not the full §5.2 surface) |
| R34.S7 (R33 `VoteRequest` chain fields) | R33-cross-vm-safety-quorum | — | todo (R33 itself not landed in code yet, per parallel track) |
| R34.S8 (acceptance gate) | R34.S1–S3, R34.S5 | `scripts/r34_acceptance_gate.sh` | landed (covers the core stamp/verify/tamper/wrong-genesis path; does NOT cover the full §0 A1–A6 quickstart-jq journey verbatim — that requires S4/S6) |

## §14 — Evidence ledger

| Claim | Verify command | Expected | Last verified (commit @ date) | Result |
|---|---|---|---|---|
| Core chain formula (§4.2) implemented byte-for-byte; deterministic; order/tamper/genesis-sensitive | `cargo test -p axon-vm --no-default-features chain::` | 11 passed, 0 failed | `ee485e8` @ 2026-07-18 | PASS |
| `scripts/r34_acceptance_gate.sh` — stamp twice, verify OK, corrupt, re-verify BROKEN exit 15, wrong-genesis exit 15 | `bash scripts/r34_acceptance_gate.sh` | exit 0, "R34 acceptance gate: ALL CHECKS PASSED" | `ee485e8` @ 2026-07-18 | PASS |
| No regression in R26/R31 attestation tests | `cargo test -p axon-attest` | all pass | `ee485e8` @ 2026-07-18 | PASS |
| Full axon-vm crate suite unaffected (incl. concurrently-landing R33 quorum tests) | `cargo test -p axon-vm --no-default-features` | 37 passed, 0 failed | `ee485e8` @ 2026-07-18 | PASS |
| Full workspace suite — **known pre-existing gap, NOT caused by R34**: `wasm_browser_examples_run_identically_via_js_host` (axon-core browser/wasm example-parity harness) fails independent of this change (0/34 R34 files touched; failure is a browser-target linking regression, unrelated to axon-vm/axon-attest) | `cargo test --workspace --no-default-features` | 418/419 axon-core tests pass, 1 pre-existing unrelated failure | (observed, not caused by `ee485e8`) @ 2026-07-18 | KNOWN GAP (pre-existing) |
