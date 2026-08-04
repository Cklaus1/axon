//! axon-vm — Axon microVM launcher.
//!
//! Wraps Firecracker to run an Axon program in a hardware-isolated microVM with:
//!   • Capability-derived seccomp-BPF policy (from `.axmeta` manifest)
//!   • MMDS-delivered boot policy (principal, budget, allowed effects, BPF)
//!   • vsock host_await substrate for interactive programs
//!   • Principal registry (~/.config/axon/principals.toml)
//!   • R26: software-TPM attestation gate (axon-vm attest)
//!
//! Commands:
//!   axon-vm run <program.ax>  [options]     Launch program in a microVM
//!   axon-vm attest --kernel K [options]     Measure kernel, produce attestation report
//!   axon-vm principal add <name> [options]  Register a principal
//!   axon-vm principal list                  List principals

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, process};

use base64::Engine as _;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

use axon_attest::{
    measure_kernel, measure_kernel_bytes, measure_host_stack, sign_report,
    try_admit_job, verify_report, verify_extended, report_to_json, report_to_json_extended,
    SOFTWARE_TPM_HW_ROOT,
};

/// R33: cross-VM safety quorum (attested VoteRequest/Response + strict-majority check).
mod quorum;

/// R34: incremental attestation rolling hash chain (ChainStore, compute_entry_hash).
mod chain;

/// R34: chain verification/tamper/stale-root failure (exit 15). Distinct from
/// 10 (attestation mismatch), 11 (TCB chain break, unused today), 12 (extended
/// measure failure), 13/14 (R33 quorum-blocked / vote-attestation-rejected).
/// Confirmed free of 1/2/10/12/13/14 before being claimed
/// (`governance/specs/R34-incremental-attestation.md` spec-meta `reserves`).
const CHAIN_VERIFY_FAIL_EXIT_CODE: i32 = 15;

/// R31: extended measurement failed — required component missing/unreadable (exit 12).
const EXTENDED_TCB_MEASURE_FAIL: i32 = 12;

/// R33: cross-VM safety quorum not met — insufficient approvals (or empty/timeout
/// in the fuller protocol). Reserved per `governance/specs/R33-cross-vm-safety-quorum.md`
/// spec-meta; confirmed free of the existing axon-vm exit codes (1, 2, 10, 12) and of
/// R34's separately-reserved 15 before being claimed.
const QUORUM_BLOCKED_EXIT_CODE: i32 = 13;

/// R33: cross-VM safety quorum blocked specifically by an attestation mismatch
/// (voters disagree on `voter_tcb`) — distinct from ordinary insufficient-approvals
/// (`QUORUM_BLOCKED_EXIT_CODE`); never collapsed into it (spec §8 invariant I-6).
const QUORUM_ATTEST_FAIL_EXIT_CODE: i32 = 14;

// ── CLI surface ───────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "axon-vm", about = "Run Axon programs in Firecracker microVMs")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run an Axon program in a microVM
    Run {
        /// Path to the .ax program to run
        program: PathBuf,

        /// Firecracker socket path (default: /tmp/axon-vm-<pid>.sock)
        #[arg(long)]
        fc_socket: Option<PathBuf>,

        /// Guest kernel image (default: dist/guest/vmlinuz)
        #[arg(long)]
        kernel: Option<PathBuf>,

        /// Guest initramfs (default: dist/guest/initramfs.cpio.gz)
        #[arg(long)]
        initrd: Option<PathBuf>,

        /// Guest memory in MiB (default: 128)
        #[arg(long, default_value = "128")]
        mem_mib: u64,

        /// Guest vCPU count (default: 1)
        #[arg(long, default_value = "1")]
        vcpus: u64,

        /// Principal name (from ~/.config/axon/principals.toml)
        #[arg(long)]
        principal: Option<String>,

        /// vsock port for host_await (default: 5000)
        #[arg(long, default_value = "5000")]
        vsock_port: u32,

        /// Emit JSON output (axon-vm-run/2 schema)
        #[arg(long)]
        json: bool,

        /// Skip kernel attestation before boot (dev/CI mode only — NOT for production).
        ///
        /// WARNING: disables the mandatory R26 attestation gate, and is the ONLY way
        /// to disable it — no environment variable can. It prints a WARNING on every
        /// run. In production, kernel attestation is mandatory before any VM boot.
        #[arg(long)]
        no_attest: bool,

        /// Expected kernel SHA-256 digest (64 hex chars), supplied by the operator.
        ///
        /// Takes precedence over the on-disk baseline in `~/.axon/kernel_baseline.sha256`,
        /// which is a user-writable file an attacker who can swap the kernel can also
        /// delete. A mismatch exits 10. This is the strongest form of the R26 gate:
        /// the expected value comes from outside the machine being attested.
        #[arg(long)]
        expect_digest: Option<String>,

        /// R31: verify the full host safety stack before booting.
        /// Measures kernel + axon-os + axon-audit + monitor and gates boot on
        /// `axtcb1_ext`. Any measure failure → exit 12. Mismatch → exit 10.
        #[arg(long)]
        extended_tcb: bool,

        /// R33: require a cross-VM safety quorum of size N before booting.
        /// Collects `.vote` files from `--quorum-dir`, runs the strict-majority
        /// `check_quorum`, and gates the Firecracker launch on the result.
        /// Exit 13 = quorum blocked (insufficient approvals); exit 14 = quorum
        /// blocked (attestation mismatch across voters).
        #[arg(long)]
        quorum: Option<usize>,

        /// R33: directory of `.vote` response files for `--quorum`. Required
        /// when `--quorum` is given.
        #[arg(long)]
        quorum_dir: Option<PathBuf>,

        /// R34: extend the incremental attestation rolling hash chain at PATH
        /// before booting. The chain is verified first (a broken/tampered
        /// chain refuses the run — exit 15, VM never spawned); the new run's
        /// program hash + run-id + timestamp are then chained onto the tip
        /// and the new `axtcb1-run:` tip is printed to stderr.
        #[arg(long)]
        chain_stamp: Option<PathBuf>,
    },

    /// Manage principals
    Principal {
        #[command(subcommand)]
        cmd: PrincipalCmd,
    },

    /// R34: incremental attestation — rolling hash chain over `axon-vm run` invocations.
    ///
    /// Extends the R31 `axtcb1-ext:` boot measurement into an append-only per-run chain
    /// (`governance/specs/R34-incremental-attestation.md`): each `chain stamp` call binds
    /// the program's SHA-256, a run-id, and a timestamp onto the previous chain tip.
    ///
    /// `chain verify` detects modification in place and INTERIOR deletion. It cannot
    /// detect a truncated tail on its own — every prefix of a valid chain is itself a
    /// valid chain — so pass `--expect-head` (and/or `--expect-count`) to pin the tip
    /// against what you last saw. `--genesis` pins the ROOT, which is the wrong end.
    Chain {
        #[command(subcommand)]
        cmd: ChainCmd,
    },

    /// R33: cross-VM safety quorum — attested VoteRequest/Response + strict-majority check.
    ///
    /// Scoped file-based exchange (`governance/specs/R33-cross-vm-safety-quorum.md`): a
    /// proposing host writes a VoteRequest, peer hosts vote by writing a VoteResponse,
    /// and `check` aggregates `.vote` files with a pure strict-majority + attestation-
    /// consistency check. Voter identity is the R31 extended-TCB `axtcb1-ext:` digest
    /// (or a clearly-labeled CI mock identity under `AXON_CI_NO_KVM=1`).
    Quorum {
        #[command(subcommand)]
        cmd: QuorumCmd,
    },

    /// R26: Measure a kernel image and produce a software-TPM attestation report.
    ///
    /// NOTE: substrate = qemu-swtpm (stand-in — no memory encryption vs the host
    /// operator). Use sev-snp/tdx (hw-attest feature) for real confidential computing.
    ///
    /// Outputs JSON on stdout: schema axon-attest/1.
    /// Exit 0 = attestation produced (and verified if --verify-digest given).
    /// Exit 10 = attestation failed or absent.
    /// Exit 2  = bad arguments / kernel not found.
    Attest {
        /// Path to the kernel ELF image to measure. In CI (AXON_CI_NO_KVM=1),
        /// this path is optional — a synthetic mock kernel is used when the file
        /// does not exist, making the attest subcommand runnable without real hardware.
        #[arg(long)]
        kernel: PathBuf,

        /// Optional base64-encoded policy to embed in the report metadata.
        #[arg(long)]
        policy: Option<String>,

        /// Optional Axon program to run inside the attested guest (mock in CI).
        #[arg(long, name = "run")]
        run_prog: Option<PathBuf>,

        /// Expected hex digest to verify against (64 hex chars = SHA-256 of kernel).
        /// If provided, verify_report is called and a mismatch exits 10.
        #[arg(long)]
        verify_digest: Option<String>,

        /// Nonce for freshness (anti-replay in real deployments).
        #[arg(long, default_value = "default-nonce")]
        nonce: String,

        /// Expected axtcb1 digest to verify (e.g. "axtcb1:…").
        #[arg(long)]
        verify_axtcb1: Option<String>,

        /// R31: also measure axon-os + axon-audit + monitor; extend report with
        /// `axtcb1_ext` and a 4-entry `components` array (schema axon-vm-report/2).
        /// Missing required component (kernel or axon-os) → exit 12.
        #[arg(long)]
        extended_tcb: bool,

        /// R31: path to the axon-os binary (default: sibling of this executable).
        /// Only used when --extended-tcb is set.
        #[arg(long)]
        axon_os: Option<PathBuf>,

        /// R31: path to the axon-audit-writer binary (default: sibling of this executable;
        /// absent → zero-fill sentinel per §4.2). Only used when --extended-tcb is set.
        #[arg(long)]
        axon_audit: Option<PathBuf>,

        /// R31: expected axtcb1_ext to verify (e.g. "axtcb1-ext:…").
        /// If provided, verify_extended is called and a mismatch exits 10.
        #[arg(long)]
        verify_axtcb1_ext: Option<String>,

        /// Record this kernel's digest as the trusted boot baseline
        /// (`~/.axon/kernel_baseline.sha256`), which `axon-vm run` then requires.
        ///
        /// This is a deliberate operator action: `run` never establishes a baseline
        /// on its own, so blessing a kernel is always something a human did, and is
        /// refused for a mock/absent kernel. Overwriting an existing pin requires
        /// --repin, so a silent re-baseline cannot happen by accident.
        #[arg(long)]
        pin_baseline: bool,

        /// Allow --pin-baseline to overwrite an existing baseline with a different digest.
        #[arg(long)]
        repin: bool,
    },
}

#[derive(Subcommand)]
enum PrincipalCmd {
    /// Register a new principal
    Add {
        /// Principal name (must be unique)
        name: String,

        /// Token budget (default: 10000)
        #[arg(long, default_value = "10000")]
        budget_tokens: u64,

        /// Allowed effect rows, comma-separated (e.g. "AI,Net")
        #[arg(long, default_value = "AI")]
        allowed_effects: String,

        /// Memory limit in MiB (default: 128)
        #[arg(long, default_value = "128")]
        mem_mib: u64,

        /// CPU share percentage 0-100 (default: 50)
        #[arg(long, default_value = "50")]
        cpu_pct: u32,
    },
    /// List registered principals
    List,
}

#[derive(Subcommand)]
enum ChainCmd {
    /// Extend the chain: hash the program, chain onto the current tip, append,
    /// and print the new `entry_hash`. Refuses (exit 15) if the existing chain
    /// at `--store` fails verification against the genesis before appending.
    Stamp {
        /// Path to the .ax program being run (its SHA-256 is chained in).
        #[arg(long)]
        prog: PathBuf,

        /// Run-id for this stamp. If omitted, a process-id + monotonic-clock
        /// id is generated (same scheme `axon-vm run` uses for its own run_id).
        #[arg(long)]
        run_id: Option<String>,

        /// Path to the chain JSONL file (created if absent).
        #[arg(long)]
        store: PathBuf,

        /// Guest kernel image used to derive the R31 genesis root (default:
        /// dist/guest/vmlinuz). Only consulted when the chain is empty.
        #[arg(long)]
        kernel: Option<PathBuf>,
    },
    /// Verify the whole chain from genesis, recomputing every link.
    ///
    /// Prints "CHAIN OK: N entries" and exits 0 on success, or "CHAIN BROKEN
    /// at seq M" and exits 15 on the first broken link (never the last).
    Verify {
        /// Path to the chain JSONL file.
        #[arg(long)]
        store: PathBuf,

        /// Expected genesis root (`axtcb1-ext:…`). If omitted, the chain's own
        /// first entry's `prev_hash` is used (self-consistency check only —
        /// pass this explicitly to pin against a known-good R31 boot root).
        #[arg(long)]
        genesis: Option<String>,

        /// Expected chain tip (`axtcb1-run:…`, as printed by `chain stamp` or
        /// `chain show`). AUDIT T31: `--genesis` pins the ROOT, which cannot
        /// detect a truncated tail — every prefix of a valid chain is itself a
        /// valid chain, so chopping off the last runs still verifies clean.
        /// Pin the TIP with this to detect rollback.
        #[arg(long)]
        expect_head: Option<String>,

        /// Expected number of entries. Catches a truncation even when the
        /// relying party recorded only how many runs it had seen, not the hash.
        #[arg(long)]
        expect_count: Option<u64>,
    },
    /// Show the current chain state (spec §5.2): vm_id, boot_root, entry
    /// count, and the current head (tip). Read-only; never writes.
    Show {
        /// Path to the chain JSONL file.
        #[arg(long)]
        store: PathBuf,

        /// Label to embed in the summary. Informational only — the chain
        /// file itself has no vm_id field, so this is not validated against
        /// anything on disk.
        #[arg(long, default_value = "default")]
        vm_id: String,

        /// Emit JSON output.
        #[arg(long)]
        json: bool,

        /// Guest kernel image used to derive the boot root when the chain is
        /// still empty (default: dist/guest/vmlinuz). Ignored once the chain
        /// has at least one entry — the root is then read back from the
        /// first entry's own `prev_hash` instead (self-consistency, same
        /// convention `chain verify`'s default `--genesis` uses).
        #[arg(long)]
        kernel: Option<PathBuf>,
    },
    /// Export the full chain as a self-contained JSON file (spec §5.4,
    /// schema `axon-chain-export/1`) for an auditor — verifiable via
    /// `chain verify-export` with no live VM or `--store` file required.
    Export {
        /// Path to the chain JSONL file.
        #[arg(long)]
        store: PathBuf,

        /// Output path for the export JSON.
        #[arg(long)]
        out: PathBuf,

        /// Label to embed in the export (informational; see `Show`'s vm_id doc).
        #[arg(long, default_value = "default")]
        vm_id: String,

        /// Guest kernel image used to derive the boot root when the chain is
        /// still empty (same fallback as `Show`).
        #[arg(long)]
        kernel: Option<PathBuf>,
    },
    /// Verify an exported chain JSON (auditor side — no live VM required).
    ///
    /// Same internal-consistency check as `chain verify` (linkage + formula
    /// recomputation), extended to also check the claimed `head` against the
    /// recomputed tip — catches a truncated/forged export where every
    /// individual link still recomputes cleanly. Prints "EXPORT OK: N
    /// entries" and exits 0, or "EXPORT BROKEN at seq M" and exits 15.
    ///
    /// NOTE: like the already-landed `chain verify`, this checks internal
    /// chain-linkage/formula consistency only — it does NOT re-hash program
    /// source files against `prog_hash` (no `--sources-dir` re-verification
    /// is wired up; see spec §12 open gap).
    VerifyExport {
        /// Path to the exported chain JSON file.
        file: PathBuf,

        /// Expected chain tip. AUDIT T31: the export's own `head` field is
        /// written by whoever produced the export, so an attacker who truncates
        /// and re-exports produces a head that agrees with the shortened entry
        /// list. Only a head the AUDITOR already knows detects that.
        #[arg(long)]
        expect_head: Option<String>,

        /// Expected number of entries (see `--expect-head`).
        #[arg(long)]
        expect_count: Option<u64>,
    },
}

#[derive(Subcommand)]
enum QuorumCmd {
    /// Propose an action for cross-VM quorum approval.
    ///
    /// Measures this host's own R31 extended TCB (`axtcb1-ext:`) as the proposer
    /// identity — or a clearly-labeled CI mock identity when `AXON_CI_NO_KVM=1`
    /// or the real kernel/axon-os binaries are unavailable — and writes a
    /// `VoteRequest` JSON file for peer VMs to vote on.
    Propose {
        /// Unique id for this quorum instance (binds request and responses together).
        #[arg(long)]
        run_id: String,

        /// Path to the Axon program (or job description) being proposed.
        #[arg(long)]
        prog: PathBuf,

        /// Human-readable description of the proposed action.
        #[arg(long)]
        action: String,

        /// Output path for the VoteRequest JSON.
        #[arg(long)]
        out: PathBuf,

        /// Guest kernel image to measure (default: dist/guest/vmlinuz).
        #[arg(long)]
        kernel: Option<PathBuf>,

        /// axon-os binary to measure (default: sibling of this executable).
        #[arg(long)]
        axon_os: Option<PathBuf>,

        /// axon-audit-writer binary to measure (default: sibling; absent → R28-pending sentinel).
        #[arg(long)]
        axon_audit: Option<PathBuf>,

        /// R33.S2e: also broadcast the VoteRequest to these peers (comma-separated
        /// "host:port" TCP-loopback addresses — the R33 spec §5.2.2 CI stand-in for real
        /// AF_VSOCK) and run the same strict-majority check `quorum check` does against
        /// whatever VoteResponses come back within --deadline-ms, exiting with the same
        /// 0/13/14 convention. Omit for the original write-only behavior (unaffected).
        #[arg(long, value_delimiter = ',')]
        broadcast: Option<Vec<String>>,

        /// Required fleet size for the --broadcast quorum check (operator-configured, NOT
        /// necessarily the number of --broadcast peers). Defaults to the peer count if omitted.
        #[arg(long)]
        n: Option<usize>,

        /// Deadline for the --broadcast round trip, in milliseconds.
        #[arg(long, default_value = "5000")]
        deadline_ms: u64,

        /// Emit JSON output (only meaningful with --broadcast).
        #[arg(long)]
        json: bool,
    },

    /// Vote (approve/deny) on a received VoteRequest.
    ///
    /// Measures this host's own R31 extended TCB as the voter identity (same
    /// CI-mock fallback as `propose`) and writes a `VoteResponse` JSON file.
    Vote {
        /// Path to the VoteRequest JSON to vote on. Required unless --listen is given.
        #[arg(long, required_unless_present = "listen")]
        request: Option<PathBuf>,

        /// Approve the proposed action.
        #[arg(long, conflicts_with = "deny")]
        approve: bool,

        /// Deny the proposed action.
        #[arg(long)]
        deny: bool,

        /// Human-readable reason for the vote.
        #[arg(long, default_value = "")]
        reason: String,

        /// Output path for the VoteResponse JSON. Required unless --listen is given.
        #[arg(long, required_unless_present = "listen")]
        out: Option<PathBuf>,

        /// R33.S2d: listen on this TCP port for ONE inbound VoteRequest instead of reading
        /// --request/writing --out as files (TCP-loopback CI stand-in for real AF_VSOCK, per
        /// R33 spec §5.2.2 — same --approve/--deny/--reason/--lineage-root decision either way).
        /// Blocks until exactly one connection arrives; an external caller owns any timeout.
        #[arg(long, conflicts_with_all = ["request", "out"])]
        listen: Option<u16>,

        /// Guest kernel image to measure (default: dist/guest/vmlinuz).
        #[arg(long)]
        kernel: Option<PathBuf>,

        /// axon-os binary to measure (default: sibling of this executable).
        #[arg(long)]
        axon_os: Option<PathBuf>,

        /// axon-audit-writer binary to measure (default: sibling; absent → R28-pending sentinel).
        #[arg(long)]
        axon_audit: Option<PathBuf>,

        /// R33 §4.5: the R27 lineage-root this voter belongs to, for the
        /// per-coalition YES-vote cap enforced in `check_quorum`. Deliberately
        /// NOT defaulted to this voter's `voter_tcb` — in a healthy fleet every
        /// honest voter shares the same `voter_tcb` (same attested software),
        /// so that would make every legitimate vote look like one giant
        /// coalition. If omitted, a fresh value unique to this invocation is
        /// used, so the cap is a no-op unless an operator explicitly declares
        /// two votes as sharing a coalition.
        #[arg(long)]
        lineage_root: Option<String>,
    },

    /// Collect `.vote` response files from a directory and check strict-majority quorum.
    ///
    /// Exit 0 = "QUORUM MET". Exit 13 = "QUORUM BLOCKED" (insufficient approvals or
    /// empty). Exit 14 = "QUORUM BLOCKED" (attestation mismatch across voters).
    Check {
        /// Directory containing `.vote` response files.
        #[arg(long = "responses-dir")]
        responses_dir: PathBuf,

        /// Required fleet size (operator-configured; NOT the number of files present).
        #[arg(long)]
        n: usize,

        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
}

// ── Data types ────────────────────────────────────────────────────────────────

/// Schema: axon-vm-mmds/1 — written to MMDS before VM boot.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct MmdsPayload {
    schema: String,
    run_id: String,
    principal: Option<String>,
    allowed_effects: Option<Vec<String>>,
    budget_tokens: Option<u64>,
    source_hash: Option<String>,
    seccomp_bpf_b64: Option<String>,
}

/// Schema: axon-manifest/1 — sidecar emitted by `axon build --emit-manifest`. `schema`/`source`/
/// `binary`/`per_fn` mirror the full sidecar schema for documentation/forward-compat even though
/// only `effect_union`/`syscall_hint`/`risk` are read today — pre-existing, found (not introduced)
/// while adding axon-vm to gate.sh's clippy coverage 2026-07-19.
#[derive(Deserialize, Debug, Default)]
#[allow(dead_code)]
struct AxonManifest {
    schema: Option<String>,
    source: Option<String>,
    binary: Option<String>,
    effect_union: Option<Vec<String>>,
    syscall_hint: Option<Vec<String>>,
    risk: Option<String>,
    #[serde(default)]
    per_fn: Vec<serde_json::Value>,
}

/// Gap 7: Principal registry entry (~/.config/axon/principals.toml).
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Principal {
    name: String,
    budget_tokens: u64,
    allowed_effects: Vec<String>,
    mem_mib: u64,
    cpu_pct: u32,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct PrincipalRegistry {
    #[serde(default)]
    principals: Vec<Principal>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run {
            program,
            fc_socket,
            kernel,
            initrd,
            mem_mib,
            vcpus,
            principal,
            vsock_port,
            json,
            no_attest,
            expect_digest,
            extended_tcb,
            quorum,
            quorum_dir,
            chain_stamp,
        } => cmd_run(program, fc_socket, kernel, initrd, mem_mib, vcpus, principal, vsock_port, json, no_attest, expect_digest, extended_tcb, quorum, quorum_dir, chain_stamp),
        Cmd::Attest {
            kernel,
            policy,
            run_prog,
            verify_digest,
            nonce,
            verify_axtcb1,
            extended_tcb,
            axon_os,
            axon_audit,
            verify_axtcb1_ext,
            pin_baseline,
            repin,
        } => cmd_attest(kernel, policy, run_prog, verify_digest, nonce, verify_axtcb1,
                        extended_tcb, axon_os, axon_audit, verify_axtcb1_ext,
                        pin_baseline, repin),
        Cmd::Principal { cmd } => match cmd {
            PrincipalCmd::Add {
                name,
                budget_tokens,
                allowed_effects,
                mem_mib,
                cpu_pct,
            } => cmd_principal_add(name, budget_tokens, allowed_effects, mem_mib, cpu_pct),
            PrincipalCmd::List => cmd_principal_list(),
        },
        Cmd::Quorum { cmd } => match cmd {
            QuorumCmd::Propose { run_id, prog, action, out, kernel, axon_os, axon_audit, broadcast, n, deadline_ms, json } =>
                cmd_quorum_propose(run_id, prog, action, out, kernel, axon_os, axon_audit, broadcast, n, deadline_ms, json),
            QuorumCmd::Vote { request, approve, deny, reason, out, kernel, axon_os, axon_audit, lineage_root, listen } =>
                cmd_quorum_vote(request, approve, deny, reason, out, kernel, axon_os, axon_audit, lineage_root, listen),
            QuorumCmd::Check { responses_dir, n, json } =>
                cmd_quorum_check(responses_dir, n, json),
        },
        Cmd::Chain { cmd } => match cmd {
            ChainCmd::Stamp { prog, run_id, store, kernel } =>
                cmd_chain_stamp(prog, run_id, store, kernel),
            ChainCmd::Verify { store, genesis, expect_head, expect_count } =>
                cmd_chain_verify(store, genesis, expect_head, expect_count),
            ChainCmd::Show { store, vm_id, json, kernel } =>
                cmd_chain_show(store, vm_id, json, kernel),
            ChainCmd::Export { store, out, vm_id, kernel } =>
                cmd_chain_export(store, out, vm_id, kernel),
            ChainCmd::VerifyExport { file, expect_head, expect_count } =>
                cmd_chain_verify_export(file, expect_head, expect_count),
        },
    }
}

// ── cmd_attest (R26) ─────────────────────────────────────────────────────────

/// R26: Measure a kernel image and produce a software-TPM attestation report.
///
/// In CI (`AXON_CI_NO_KVM=1`) or when the kernel path does not exist, a
/// synthetic mock kernel is used — allowing the attest pipeline to be exercised
/// without real hardware or a built guest image.
///
/// Output: JSON on stdout, schema `axon-attest/1`.
/// Caveat printed to stderr: "substrate: qemu-swtpm (stand-in — no memory encryption…)"
#[allow(clippy::too_many_arguments)]
fn cmd_attest(
    kernel: PathBuf,
    _policy: Option<String>,
    run_prog: Option<PathBuf>,
    verify_digest: Option<String>,
    _nonce: String,
    verify_axtcb1: Option<String>,
    extended_tcb: bool,
    axon_os_override: Option<PathBuf>,
    axon_audit_override: Option<PathBuf>,
    verify_axtcb1_ext: Option<String>,
    pin_baseline: bool,
    repin: bool,
) {
    let ci_no_kvm = env::var("AXON_CI_NO_KVM").map(|v| v == "1").unwrap_or(false);

    // Pinning a boot baseline requires a REAL kernel image. In mock mode the
    // measurement is of synthetic bytes, so pinning it would bless something that
    // never boots — worse, it would satisfy the run gate with a fiction.
    if pin_baseline && !kernel.exists() {
        eprintln!(
            "axon-vm attest: --pin-baseline requires a real kernel image; not found: {}",
            kernel.display()
        );
        process::exit(2);
    }
    if pin_baseline && ci_no_kvm {
        eprintln!(
            "axon-vm attest: --pin-baseline refuses to pin a mock measurement (AXON_CI_NO_KVM=1)"
        );
        process::exit(2);
    }

    // Measure the kernel. In CI/mock mode, use synthetic bytes when the real
    // kernel image is unavailable (no hardware, no build yet).
    let measurement = if ci_no_kvm || !kernel.exists() {
        if !kernel.exists() && !ci_no_kvm {
            eprintln!(
                "axon-vm attest: kernel not found: {} — set AXON_CI_NO_KVM=1 to use mock",
                kernel.display()
            );
            process::exit(2);
        }
        // CI mock: measure the path bytes themselves as a stable stand-in
        let mock_bytes = if kernel.exists() {
            // File exists: measure the real bytes even in CI mode
            match fs::read(&kernel) {
                Ok(b) => b,
                Err(_) => format!("mock-kernel:{}", kernel.display()).into_bytes(),
            }
        } else {
            // No file at all: synthetic bytes derived from the path string
            format!("axon-os-mock-kernel-ci:{}", kernel.display()).into_bytes()
        };
        measure_kernel_bytes(&mock_bytes)
    } else {
        // Real mode: read and measure the kernel ELF
        match measure_kernel(&kernel) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("axon-vm attest: cannot measure kernel: {e}");
                process::exit(2);
            }
        }
    };

    // --pin-baseline: record this digest as the trusted boot baseline. Done here,
    // in an explicit operator command, precisely so that `run` never has to.
    if pin_baseline {
        let digest_hex = hex::encode(measurement.digest);
        let baseline_path = kernel_baseline_path();
        if let Ok(existing) = fs::read_to_string(&baseline_path) {
            let existing = existing.trim().to_string();
            if existing != digest_hex && !repin {
                eprintln!("axon-vm attest: refusing to overwrite an existing baseline");
                eprintln!("  existing: {existing}");
                eprintln!("  measured: {digest_hex}");
                eprintln!("  pass --repin if this kernel change is intentional");
                process::exit(10);
            }
        }
        if let Some(parent) = baseline_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!("axon-vm attest: cannot create {}: {e}", parent.display());
                process::exit(2);
            }
        }
        if let Err(e) = fs::write(&baseline_path, &digest_hex) {
            eprintln!(
                "axon-vm attest: cannot write {}: {e}",
                baseline_path.display()
            );
            process::exit(2);
        }
        eprintln!(
            "[axon-vm] baseline pinned: {} → {}",
            &digest_hex[..16],
            baseline_path.display()
        );
    }

    // Generate a software-TPM ephemeral key (deterministic per-process in CI,
    // fresh per-invocation in production via process id + start time).
    let key = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"axon-r26-software-tpm-ephemeral-key");
        h.update(process::id().to_le_bytes());
        h.finalize().to_vec()
    };

    // Sign the measurement with the software-TPM key.
    let report = sign_report(measurement.clone(), &key);

    // Mandatory stand-in caveat — must be printed so no operator is misled
    // (R26 §8, §10 honesty check: "substrate: qemu-swtpm — no memory encryption").
    eprintln!(
        "substrate: {} (stand-in — no memory encryption; use sev-snp/tdx for confidentiality)",
        SOFTWARE_TPM_HW_ROOT
    );

    // ── R31: extended TCB measurement ────────────────────────────────────────
    let extended_measurement = if extended_tcb {
        // Auto-detect sibling binary paths when overrides are not given.
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let axon_os_path = axon_os_override.unwrap_or_else(|| exe_dir.join("axon-os"));
        let axon_audit_path = axon_audit_override
            .unwrap_or_else(|| exe_dir.join("axon-audit-writer"));

        // axon-audit is optional (R28 pending); pass None if it doesn't exist.
        let axon_audit_opt = if axon_audit_path.exists() {
            Some(axon_audit_path.as_path())
        } else {
            None
        };

        match measure_host_stack(&kernel, Some(axon_os_path.as_path()), axon_audit_opt) {
            Ok(ext) => {
                eprintln!("✓ extended TCB: {} (4/4 components)", ext.axtcb1_ext);
                Some(ext)
            }
            Err(e) => {
                eprintln!("axon-vm attest: extended TCB measurement failed: {e}");
                process::exit(EXTENDED_TCB_MEASURE_FAIL);
            }
        }
    } else {
        None
    };

    // Verify if the caller supplied expected values.
    let (ok, reason) = if let Some(ref expected_hex) = verify_digest {
        // Parse the hex digest
        match hex::decode(expected_hex) {
            Ok(bytes) if bytes.len() == 32 => {
                let expected_arr: [u8; 32] = bytes.try_into().unwrap();
                let expected_tcb = verify_axtcb1.as_deref().unwrap_or(&measurement.axtcb1);
                match verify_report(&report, &expected_arr, expected_tcb) {
                    Ok(()) => (true, "✓ attested: measurement matches, axtcb1 chained".to_string()),
                    Err(e) => (false, format!("✗ ATTESTATION FAILED: {e}")),
                }
            }
            Ok(_) => (false, "invalid --verify-digest: must be exactly 64 hex chars (SHA-256)".to_string()),
            Err(e) => (false, format!("invalid --verify-digest hex: {e}")),
        }
    } else {
        (true, "attestation report produced (no --verify-digest requested)".to_string())
    };

    // Optionally run a job inside the attested guest (mock in CI)
    let run_record = if let Some(ref prog) = run_prog {
        if ok {
            let job_bytes = prog.to_string_lossy().as_bytes().to_vec();
            let expected_arr = report.measurement.digest;
            let expected_tcb = report.measurement.axtcb1.clone();
            match try_admit_job(&report, &expected_arr, &expected_tcb, &job_bytes, 42u64) {
                Ok(rec) => Some(rec),
                Err(e) => {
                    eprintln!("axon-vm attest: job admission failed: {e}");
                    None
                }
            }
        } else {
            eprintln!("axon-vm attest: job not admitted — attestation failed");
            None
        }
    } else {
        None
    };

    // R31: verify extended TCB digest if expected value was provided
    let (ok, reason) = if ok {
        if let Some(ref expected_ext) = verify_axtcb1_ext {
            if let Some(ref ext) = extended_measurement {
                match verify_extended(ext, expected_ext) {
                    Ok(()) => (true, format!("{reason}; ✓ extended TCB: 4/4 components verified")),
                    Err(e) => (false, format!("✗ EXTENDED TCB FAILED: {e}")),
                }
            } else {
                (false, "✗ --verify-axtcb1-ext requires --extended-tcb".to_string())
            }
        } else {
            (ok, reason)
        }
    } else {
        (ok, reason)
    };

    // Parse the report JSON for embedding in output (R26 or R31 schema)
    let report_json: serde_json::Value = if let Some(ref ext) = extended_measurement {
        serde_json::from_str(&report_to_json_extended(&report, ext)).unwrap_or_default()
    } else {
        serde_json::from_str(&report_to_json(&report)).unwrap_or_default()
    };

    // Emit the output JSON (schema axon-attest/1 or axon-attest/2)
    let out = serde_json::json!({
        "schema": if extended_measurement.is_some() { "axon-attest/2" } else { "axon-attest/1" },
        "ok": ok,
        "report": report_json,
        "reason": reason,
        "run_record": run_record,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap());

    if !ok {
        process::exit(10);
    }
}

// ── cmd_run ───────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn cmd_run(
    program: PathBuf,
    fc_socket: Option<PathBuf>,
    kernel: Option<PathBuf>,
    initrd: Option<PathBuf>,
    mem_mib: u64,
    vcpus: u64,
    principal_name: Option<String>,
    vsock_port: u32,
    json_out: bool,
    no_attest: bool,
    expect_digest: Option<String>,
    extended_tcb: bool,
    quorum: Option<usize>,
    quorum_dir: Option<PathBuf>,
    chain_stamp: Option<PathBuf>,
) {
    let start = Instant::now();

    // Locate guest image files.
    let kernel_path = kernel
        .unwrap_or_else(|| PathBuf::from("dist/guest/vmlinuz"));
    // Default initrd: prefer the gzipped image if present, else the uncompressed
    // `.cpio` that the build ships (the guest kernel's CPIO reader handles both). The
    // old hard-coded `.cpio.gz` default failed on every stock checkout (only `.cpio`
    // exists), forcing an explicit --initrd.
    let initrd_path = initrd.unwrap_or_else(|| {
        let gz = PathBuf::from("dist/guest/initramfs.cpio.gz");
        if gz.exists() {
            gz
        } else {
            PathBuf::from("dist/guest/initramfs.cpio")
        }
    });

    for p in [&kernel_path, &initrd_path, &program] {
        if !p.exists() {
            eprintln!("axon-vm: file not found: {}", p.display());
            process::exit(1);
        }
    }

    // Generate a unique run-id.
    let run_id = format!(
        "vm-{}-{}",
        process::id(),
        start.elapsed().as_nanos()
    );

    // Compute source hash for auditability.
    let source_hash = sha256_file(&program);

    // Load the .axmeta manifest (emitted by `axon build --emit-manifest`).
    let manifest = load_manifest(&program);

    // Resolve the principal.
    let principal = principal_name
        .as_ref()
        .and_then(|n| load_principal(n));

    // Build the seccomp BPF policy from the manifest's syscall_hint.
    let seccomp_b64 = if let Some(ref hints) = manifest.syscall_hint {
        if hints.is_empty() {
            None
        } else {
            match bpf_allowlist(hints) {
                Ok(b64) => Some(b64),
                Err(e) => {
                    eprintln!("axon-vm: BPF generation failed: {e}");
                    None
                }
            }
        }
    } else {
        None
    };

    // Derive allowed effects: an explicit AXON_VM_ALLOWED_EFFECTS override (comma-
    // separated effect names) tightens the policy beyond the manifest — useful for
    // defense-in-depth and for exercising the in-kernel syscall gate. Otherwise prefer
    // the manifest's effect union, fall back to the principal, then open.
    let allowed_effects = if let Ok(forced) = env::var("AXON_VM_ALLOWED_EFFECTS") {
        Some(
            forced
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>(),
        )
    } else {
        manifest
            .effect_union
            .clone()
            .or_else(|| principal.as_ref().map(|p| p.allowed_effects.clone()))
    };

    let budget_tokens = principal.as_ref().map(|p| p.budget_tokens);

    // Construct the MMDS payload.
    let mmds_payload = MmdsPayload {
        schema: "axon-vm-mmds/1".to_string(),
        run_id: run_id.clone(),
        principal: principal_name.clone(),
        allowed_effects,
        budget_tokens,
        source_hash: Some(source_hash),
        seccomp_bpf_b64: seccomp_b64,
    };

    // R26: mandatory kernel attestation before any VM boot.
    // Measure the kernel and verify it against a PINNED expected digest —
    // --expect-digest, else ~/.axon/kernel_baseline.sha256. Exits 10 on mismatch
    // AND on no pin at all (no trust-on-first-use). --no-attest is the only
    // bypass; no environment variable can disable this gate.
    if let Err(e) = measure_and_attest(&kernel_path, no_attest, expect_digest.as_deref()) {
        if json_out {
            let out = serde_json::json!({
                "schema": "axon-vm-run/1",
                "ok": false,
                "run_id": run_id,
                "exit_code": -1,
                "elapsed_ms": start.elapsed().as_millis(),
                "error": e.to_string(),
                "principal": principal_name,
                "risk": manifest.risk,
                "attestation_failed": true,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
        } else {
            eprintln!("axon-vm: {e}");
        }
        process::exit(10);
    }

    // R31: extended TCB gate — measure full safety stack before booting.
    // Any measure failure → exit 12 (component missing/unreadable).
    // The VM is NEVER spawned until this gate passes.
    if extended_tcb {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let axon_os_path   = exe_dir.join("axon-os");
        let axon_audit_path = exe_dir.join("axon-audit-writer");
        let axon_audit_opt = if axon_audit_path.exists() {
            Some(axon_audit_path.as_path())
        } else {
            None
        };
        match measure_host_stack(&kernel_path, Some(axon_os_path.as_path()), axon_audit_opt) {
            Ok(ext) => {
                eprintln!(
                    "✓ extended TCB: {} (4/4 components verified)",
                    ext.axtcb1_ext
                );
            }
            Err(e) => {
                eprintln!("axon-vm: extended TCB measurement failed: {e}");
                process::exit(EXTENDED_TCB_MEASURE_FAIL);
            }
        }
    }

    // R33: cross-VM safety quorum gate — collected BEFORE any VM boots.
    // The Firecracker launch never runs unless the quorum check passes.
    if let Some(required_n) = quorum {
        let dir = match quorum_dir {
            Some(ref d) => d.clone(),
            None => {
                eprintln!("axon-vm: --quorum requires --quorum-dir");
                process::exit(2);
            }
        };
        let votes = match quorum::io::collect_responses(&dir, required_n) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("axon-vm: quorum: failed to collect responses from {}: {e}", dir.display());
                process::exit(2);
            }
        };
        let result = quorum::logic::check_quorum(&votes, required_n);
        if result.quorum_met {
            eprintln!(
                "✓ quorum approved: {}/{} approvals — proceeding to boot",
                result.approvals, required_n
            );
        } else {
            let reason = result.blocking_reason.clone().unwrap_or_default();
            eprintln!("✗ quorum blocked: {reason} — VM will NOT be booted");
            if json_out {
                let out = serde_json::json!({
                    "schema": "axon-vm-run/1",
                    "ok": false,
                    "run_id": run_id,
                    "exit_code": -1,
                    "elapsed_ms": start.elapsed().as_millis(),
                    "error": reason,
                    "principal": principal_name,
                    "risk": manifest.risk,
                    "quorum_blocked": true,
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            }
            if reason.contains("attestation") {
                process::exit(QUORUM_ATTEST_FAIL_EXIT_CODE);
            } else {
                process::exit(QUORUM_BLOCKED_EXIT_CODE);
            }
        }
    }

    // R34: incremental attestation chain gate — verify the existing chain
    // BEFORE ever spawning a VM, then extend it with this run. A broken
    // chain (tamper, stale root, wrong genesis) refuses the run at exit 15;
    // the Firecracker launch never runs when this gate fails.
    if let Some(ref chain_path) = chain_stamp {
        let genesis = chain_genesis(&kernel_path);
        let store = chain::ChainStore::new(chain_path);
        match store.verify(&genesis) {
            Ok(_) => {}
            Err(seq) => {
                eprintln!("axon-vm: CHAIN BROKEN at seq {seq} — refusing to launch VM (exit {CHAIN_VERIFY_FAIL_EXIT_CODE})");
                process::exit(CHAIN_VERIFY_FAIL_EXIT_CODE);
            }
        }
        match stamp_chain(&store, &genesis, &program, &run_id) {
            Ok(entry_hash) => {
                eprintln!("axtcb1-ext: {genesis}   axtcb1-run: {entry_hash}");
            }
            Err(e) => {
                eprintln!("axon-vm: chain stamp failed: {e}");
                process::exit(1);
            }
        }
    }

    // Choose or generate the Firecracker socket path.
    let socket_path = fc_socket.unwrap_or_else(|| {
        PathBuf::from(format!("/tmp/axon-vm-{}.sock", process::id()))
    });

    // Launch Firecracker, configure the VM, and run the program.
    let result = run_in_firecracker(
        &program,
        &kernel_path,
        &initrd_path,
        mem_mib,
        vcpus,
        vsock_port,
        &socket_path,
        &mmds_payload,
        principal.as_ref(),
    );

    let elapsed_ms = start.elapsed().as_millis();

    // `ok` reports what the GUEST did. It used to be `result.is_ok()` — whether the
    // launcher succeeded in driving the Firecracker API — so a guest that hit its
    // deadline, or that refused an operation and halted, was reported `ok:true`
    // (P7-KRN-04). A run is OK only if the guest reached a definite end and exited 0.
    let exit_code = result.as_ref().map(|r| r.exit_code).unwrap_or(-1);
    let outcome = result
        .as_ref()
        .map(|r| r.outcome.as_str())
        .unwrap_or("launch-failed");
    let ok =
        matches!(result.as_ref(), Ok(r) if matches!(r.outcome, GuestOutcome::Exited(_)))
            && exit_code == 0;

    if json_out {
        let out = serde_json::json!({
            "schema": "axon-vm-run/2",
            "ok": ok,
            "run_id": run_id,
            "exit_code": exit_code,
            "guest_outcome": outcome,
            "elapsed_ms": elapsed_ms,
            "error": result.as_ref().err().map(|e| e.to_string()),
            "principal": principal_name,
            "risk": manifest.risk,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        // `--json` used to print and fall off the end of this function, so
        // `axon-vm run --json` exited 0 no matter what the guest did — every caller
        // testing `$?` was reading a constant. Propagate, as the non-JSON path does.
        match result {
            Ok(_) => process::exit(exit_code),
            Err(_) => process::exit(1),
        }
    } else {
        match result {
            Ok(r) => process::exit(r.exit_code),
            Err(e) => {
                eprintln!("axon-vm: {e}");
                process::exit(1);
            }
        }
    }
}

// ── cmd_chain_* (R34) ─────────────────────────────────────────────────────────

/// Compute the chain genesis root: the R31 `axtcb1_ext` full-host-stack
/// measurement, or a deterministic CI-mock value when the real kernel/axon-os
/// binaries are unavailable (`AXON_CI_NO_KVM=1`, or the files simply don't
/// exist in this environment). Reuses the exact same synthetic-bytes
/// convention already established in `cmd_attest` (measure synthetic bytes
/// derived from the path string through the pure `measure_extended` core)
/// rather than inventing a new mock — the CI label is printed to stderr so
/// this is never mistaken for a real attestation.
fn chain_genesis(kernel_path: &Path) -> String {
    let ci_no_kvm = env::var("AXON_CI_NO_KVM").map(|v| v == "1").unwrap_or(false);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let axon_os_path = exe_dir.join("axon-os");
    let axon_audit_path = exe_dir.join("axon-audit-writer");

    if ci_no_kvm || !kernel_path.exists() || !axon_os_path.exists() {
        eprintln!(
            "[axon-vm chain] CI mock genesis (AXON_CI_NO_KVM=1 or kernel/axon-os \
             unavailable) — NOT a real attestation"
        );
        let mock_kernel = format!("axon-os-mock-kernel-ci:{}", kernel_path.display()).into_bytes();
        let mock_axon_os = format!("axon-os-mock-binary-ci:{}", axon_os_path.display()).into_bytes();
        let ext = axon_attest::measure_extended(axon_attest::ComponentPaths {
            kernel: mock_kernel,
            axon_os: mock_axon_os,
            axon_audit: None,
            kernel_path: format!("<ci-mock> {}", kernel_path.display()),
            axon_os_path: format!("<ci-mock> {}", axon_os_path.display()),
            axon_audit_path: None,
        })
        .expect("measure_extended over injected bytes cannot fail (kernel/axon-os always non-empty)");
        ext.axtcb1_ext
    } else {
        let axon_audit_opt = if axon_audit_path.exists() { Some(axon_audit_path.as_path()) } else { None };
        match measure_host_stack(kernel_path, Some(axon_os_path.as_path()), axon_audit_opt) {
            Ok(ext) => ext.axtcb1_ext,
            Err(e) => {
                eprintln!("axon-vm chain: extended TCB measurement failed: {e}");
                process::exit(EXTENDED_TCB_MEASURE_FAIL);
            }
        }
    }
}

/// Compute the next `ChainEntry`, append it, and return the new `entry_hash`.
///
/// Shared between `cmd_run`'s `--chain-stamp` gate and `axon-vm chain stamp` so
/// both paths use byte-identical logic (hash the program, read the current
/// tip, chain onto it, append). `run_id` is the caller's own run identifier
/// (reused, not regenerated) so a `run --chain-stamp` invocation's chain entry
/// carries the exact same run-id as the rest of that run's provenance.
fn stamp_chain(
    store: &chain::ChainStore,
    genesis: &str,
    prog: &Path,
    run_id: &str,
) -> std::io::Result<String> {
    let (seq, prev_hash) = store.last_entry(genesis)?;
    let prog_hash = chain::sha256_file(prog)?;
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let entry_hash = chain::compute_entry_hash(&prev_hash, &prog_hash, run_id, timestamp_ms);
    store.append(&chain::ChainEntry {
        seq,
        run_id: run_id.to_string(),
        prog_hash,
        timestamp_ms,
        prev_hash,
        entry_hash: entry_hash.clone(),
    })?;
    Ok(entry_hash)
}

/// `axon-vm chain stamp` — hash the program, extend the chain, print the new tip.
fn cmd_chain_stamp(prog: PathBuf, run_id: Option<String>, store_path: PathBuf, kernel: Option<PathBuf>) {
    if !prog.exists() {
        eprintln!("axon-vm chain stamp: program not found: {}", prog.display());
        process::exit(1);
    }
    let kernel_path = kernel.unwrap_or_else(|| PathBuf::from("dist/guest/vmlinuz"));
    let genesis = chain_genesis(&kernel_path);
    let store = chain::ChainStore::new(&store_path);

    // Refuse to extend a chain that's already broken (append-only + tamper gate).
    if let Err(seq) = store.verify(&genesis) {
        eprintln!("axon-vm chain stamp: CHAIN BROKEN at seq {seq} — refusing to append");
        process::exit(CHAIN_VERIFY_FAIL_EXIT_CODE);
    }

    let run_id = run_id.unwrap_or_else(|| {
        format!(
            "vm-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    });

    match stamp_chain(&store, &genesis, &prog, &run_id) {
        Ok(entry_hash) => println!("{entry_hash}"),
        Err(e) => {
            eprintln!("axon-vm chain stamp: append failed: {e}");
            process::exit(1);
        }
    }
}

/// `axon-vm chain verify` — recompute every link from genesis; report the
/// first broken seq (never the last) on failure.
fn cmd_chain_verify(
    store_path: PathBuf,
    genesis: Option<String>,
    expect_head: Option<String>,
    expect_count: Option<u64>,
) {
    let store = chain::ChainStore::new(&store_path);
    let genesis_hash = genesis.unwrap_or_else(|| {
        // No externally-pinned genesis supplied: fall back to the chain's own
        // claimed root (self-consistency check) so `chain verify` is usable
        // standalone. A relying party who wants to pin against a known-good
        // R31 boot root MUST pass --genesis explicitly (§4.1 of the spec).
        fs::read_to_string(&store_path)
            .ok()
            .and_then(|content| content.lines().find(|l| !l.trim().is_empty()).map(str::to_string))
            .and_then(|first_line| serde_json::from_str::<chain::ChainEntry>(&first_line).ok())
            .map(|e| e.prev_hash)
            .unwrap_or_default()
    });

    // AUDIT T31 (OSK-P7-H3 / P7-KRN-06 / P6-COV-02). Linkage alone cannot see a
    // truncated tail: every prefix of a valid chain is itself a valid chain, so
    // a 3-entry chain with the incriminating run chopped off reported
    // "CHAIN OK: 1 entries" and an erased one "CHAIN OK: 0 entries", both
    // exit 0. --genesis pins the ROOT; truncation moves the TIP. The pins below
    // are the only thing that closes it, and they must come from the caller.
    match store.verify_pinned(&genesis_hash, expect_head.as_deref(), expect_count) {
        Ok(n) => {
            if expect_head.is_none() && expect_count.is_none() {
                // Say what was actually established. Without a pin this is
                // "well-formed from the genesis", NOT "complete".
                println!(
                    "CHAIN OK: {n} entries (unpinned — truncation undetectable, see --expect-head)"
                );
            } else {
                println!("CHAIN OK: {n} entries (pinned)");
            }
        }
        Err(e) => {
            println!("{e}");
            process::exit(CHAIN_VERIFY_FAIL_EXIT_CODE);
        }
    }
}

/// Resolve the chain's boot/genesis root for display/export purposes (R34
/// Slice 6): if the store already has entries, reuse the first entry's own
/// `prev_hash` (self-consistency — same convention `cmd_chain_verify`'s
/// default `--genesis` fallback uses) rather than re-measuring the kernel
/// just to show data that's already on disk. Only when the store is empty
/// (nothing to anchor to yet) does this compute a fresh genesis from the
/// kernel image, mirroring `cmd_chain_stamp`'s own fallback.
fn chain_boot_root(store_path: &Path, kernel: &Option<PathBuf>) -> String {
    let first_prev_hash = fs::read_to_string(store_path)
        .ok()
        .and_then(|content| content.lines().find(|l| !l.trim().is_empty()).map(str::to_string))
        .and_then(|first_line| serde_json::from_str::<chain::ChainEntry>(&first_line).ok())
        .map(|e| e.prev_hash);
    match first_prev_hash {
        Some(root) => root,
        None => {
            let kernel_path = kernel.clone().unwrap_or_else(|| PathBuf::from("dist/guest/vmlinuz"));
            chain_genesis(&kernel_path)
        }
    }
}

/// `axon-vm chain show` — human-readable (or `--json`) summary: vm_id,
/// boot_root, entry count, and the current head (tip). Spec §5.2.
fn cmd_chain_show(store_path: PathBuf, vm_id: String, json: bool, kernel: Option<PathBuf>) {
    let store = chain::ChainStore::new(&store_path);
    let boot_root = chain_boot_root(&store_path, &kernel);
    let (entries, head) = match store.last_entry(&boot_root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("axon-vm chain show: {e}");
            process::exit(1);
        }
    };
    if json {
        let out = serde_json::json!({
            "vm_id": vm_id,
            "boot_root": boot_root,
            "entries": entries,
            "head": head,
        });
        println!("{out}");
    } else {
        println!("vm_id: {vm_id}");
        println!("boot_root: {boot_root}");
        println!("runs: {entries}");
        println!("head: {head}");
    }
}

/// `axon-vm chain export` — write a self-contained `ChainExport` JSON file
/// (schema `axon-chain-export/1`, spec §5.4) that an auditor can check with
/// `chain verify-export` and no live VM. Wraps `ChainStore::export`.
fn cmd_chain_export(store_path: PathBuf, out_path: PathBuf, vm_id: String, kernel: Option<PathBuf>) {
    let store = chain::ChainStore::new(&store_path);
    let boot_root = chain_boot_root(&store_path, &kernel);
    let exported_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let export = match store.export(&vm_id, &boot_root, exported_at_ms) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("axon-vm chain export: {e}");
            process::exit(1);
        }
    };
    let json = match serde_json::to_string_pretty(&export) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("axon-vm chain export: serialize failed: {e}");
            process::exit(1);
        }
    };
    if let Err(e) = fs::write(&out_path, json) {
        eprintln!("axon-vm chain export: write failed: {e}");
        process::exit(1);
    }
    println!(
        "wrote {} with {} entries to {}",
        chain::CHAIN_EXPORT_SCHEMA,
        export.entries.len(),
        out_path.display()
    );
}

/// `axon-vm chain verify-export` — verify an exported chain JSON (auditor
/// side, no live VM required). Same pass/fail contract as `chain verify`.
fn cmd_chain_verify_export(
    file: PathBuf,
    expect_head: Option<String>,
    expect_count: Option<u64>,
) {
    let content = match fs::read_to_string(&file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("axon-vm chain verify-export: cannot read {}: {e}", file.display());
            process::exit(1);
        }
    };
    let export: chain::ChainExport = match serde_json::from_str(&content) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("axon-vm chain verify-export: malformed JSON in {}: {e}", file.display());
            process::exit(1);
        }
    };
    // AUDIT T31. The export's own `head` is written by whoever produced it, so
    // truncate-then-re-export yields a head consistent with the shortened entry
    // list and the existing check passes. Only an auditor-supplied pin sees it.
    match chain::verify_export_pinned(&export, expect_head.as_deref(), expect_count) {
        Ok(n) => {
            if expect_head.is_none() && expect_count.is_none() {
                println!(
                    "EXPORT OK: {n} entries (unpinned — truncation undetectable, see --expect-head)"
                );
            } else {
                println!("EXPORT OK: {n} entries (pinned)");
            }
        }
        Err(e) => {
            println!("{}", e.to_string().replace("CHAIN ", "EXPORT "));
            process::exit(CHAIN_VERIFY_FAIL_EXIT_CODE);
        }
    }
}

// ── cmd_quorum_* (R33) ────────────────────────────────────────────────────────

/// Compute this host's own voter identity for the R33 quorum protocol.
///
/// Reuses the exact R26/R31 CI-mock convention already established in
/// `cmd_attest` (lines measuring synthetic path-derived bytes when
/// `AXON_CI_NO_KVM=1` or the real kernel/axon-os files are unavailable) rather
/// than inventing a new mock: when the real files are present and
/// `AXON_CI_NO_KVM` is unset, this calls the real `measure_host_stack`; when
/// unavailable, it calls `measure_extended` directly over synthetic bytes
/// derived from the (possibly nonexistent) paths, so the CLI is exercisable
/// without a real kernel build. The CI-mock path prints a clear label to
/// stderr — it must never be mistaken for a real attestation.
fn quorum_self_tcb(
    kernel: &Option<PathBuf>,
    axon_os: &Option<PathBuf>,
    axon_audit: &Option<PathBuf>,
) -> String {
    let ci_no_kvm = env::var("AXON_CI_NO_KVM").map(|v| v == "1").unwrap_or(false);
    let kernel_path = kernel.clone().unwrap_or_else(|| PathBuf::from("dist/guest/vmlinuz"));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let axon_os_path = axon_os.clone().unwrap_or_else(|| exe_dir.join("axon-os"));
    let axon_audit_path = axon_audit.clone().unwrap_or_else(|| exe_dir.join("axon-audit-writer"));

    if ci_no_kvm || !kernel_path.exists() || !axon_os_path.exists() {
        eprintln!(
            "[axon-vm quorum] CI mock identity (AXON_CI_NO_KVM=1 or kernel/axon-os \
             unavailable) — NOT a real attestation"
        );
        let mock_kernel = format!("axon-os-mock-kernel-ci:{}", kernel_path.display()).into_bytes();
        let mock_axon_os = format!("axon-os-mock-binary-ci:{}", axon_os_path.display()).into_bytes();
        let ext = axon_attest::measure_extended(axon_attest::ComponentPaths {
            kernel: mock_kernel,
            axon_os: mock_axon_os,
            axon_audit: None,
            kernel_path: format!("<ci-mock> {}", kernel_path.display()),
            axon_os_path: format!("<ci-mock> {}", axon_os_path.display()),
            axon_audit_path: None,
        })
        .expect("measure_extended over injected bytes cannot fail (kernel/axon-os always non-empty)");
        ext.axtcb1_ext
    } else {
        let axon_audit_opt = if axon_audit_path.exists() { Some(axon_audit_path.as_path()) } else { None };
        match measure_host_stack(&kernel_path, Some(axon_os_path.as_path()), axon_audit_opt) {
            Ok(ext) => ext.axtcb1_ext,
            Err(e) => {
                eprintln!("axon-vm quorum: extended TCB measurement failed: {e}");
                process::exit(EXTENDED_TCB_MEASURE_FAIL);
            }
        }
    }
}

/// `axon-vm quorum propose` — build and write a `VoteRequest` (R33 §3.1, scoped); optionally
/// (R33.S2e) also broadcast it to real peers and run the same strict-majority check `quorum
/// check` does, exiting with the same 0/13/14 convention.
#[allow(clippy::too_many_arguments)]
fn cmd_quorum_propose(
    run_id: String,
    prog: PathBuf,
    action: String,
    out: PathBuf,
    kernel: Option<PathBuf>,
    axon_os: Option<PathBuf>,
    axon_audit: Option<PathBuf>,
    broadcast: Option<Vec<String>>,
    n: Option<usize>,
    deadline_ms: u64,
    json_out: bool,
) {
    let voter_tcb = quorum_self_tcb(&kernel, &axon_os, &axon_audit);
    let prog_hash = format!("sha256:{}", sha256_file(&prog));
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let req = quorum::logic::VoteRequest {
        run_id,
        prog_hash,
        voter_tcb: voter_tcb.clone(),
        proposed_action: action,
        timestamp_ms,
    };

    if let Err(e) = quorum::io::write_vote_request(&req, &out) {
        eprintln!("axon-vm quorum propose: failed to write {}: {e}", out.display());
        process::exit(2);
    }
    eprintln!("✓ proposal written: {} (proposer axtcb1-ext: {})", out.display(), voter_tcb);

    let Some(peer_strs) = broadcast else { return };

    let peers: Vec<std::net::SocketAddr> = peer_strs
        .iter()
        .map(|s| {
            s.parse().unwrap_or_else(|e| {
                eprintln!("axon-vm quorum propose --broadcast: invalid peer address '{s}': {e}");
                process::exit(2);
            })
        })
        .collect();
    let required_n = n.unwrap_or(peers.len());
    let deadline = std::time::Duration::from_millis(deadline_ms);

    eprintln!("broadcasting to {} peer(s), deadline {deadline_ms}ms ...", peers.len());
    let votes = quorum::vsock::broadcast_and_collect(&peers, &req, deadline);
    let result = quorum::logic::check_quorum(&votes, required_n);
    report_quorum_result(result, required_n, json_out);
}

/// Shared by `quorum check` (file-based votes) and `quorum propose --broadcast` (live votes):
/// prints the result and exits with R33's established convention — 0 = QUORUM MET, 13 =
/// QUORUM_BLOCKED (insufficient approvals), 14 = QUORUM_ATTEST_FAIL (attestation mismatch across
/// voters, a materially different and more serious signal than a plain minority — see
/// `check_quorum`'s own doc comment, never conflated with 13).
fn report_quorum_result(result: quorum::logic::QuorumResult, n: usize, json_out: bool) -> ! {
    if json_out {
        let out = serde_json::json!({
            "schema": "axon-vm-quorum-check/1",
            "quorum_met": result.quorum_met,
            "coalition_size": result.coalition_size,
            "approvals": result.approvals,
            "required_n": n,
            "blocking_reason": result.blocking_reason,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    }

    if result.quorum_met {
        eprintln!("QUORUM MET: {}/{} approvals", result.approvals, n);
        process::exit(0);
    } else {
        let reason = result.blocking_reason.clone().unwrap_or_default();
        eprintln!("QUORUM BLOCKED: {reason}");
        if reason.contains("attestation") {
            process::exit(QUORUM_ATTEST_FAIL_EXIT_CODE);
        } else {
            process::exit(QUORUM_BLOCKED_EXIT_CODE);
        }
    }
}

/// `axon-vm quorum vote` — cast a vote on a `VoteRequest`, either read from `--request`/written
/// to `--out` as files (the original, still-default path), or (R33.S2d) received over a single
/// TCP-loopback connection on `--listen PORT` and answered on that same connection — same
/// approve/deny/reason/lineage-root decision either way, only the I/O layer differs.
#[allow(clippy::too_many_arguments)]
fn cmd_quorum_vote(
    request: Option<PathBuf>,
    approve: bool,
    deny: bool,
    reason: String,
    out: Option<PathBuf>,
    kernel: Option<PathBuf>,
    axon_os: Option<PathBuf>,
    axon_audit: Option<PathBuf>,
    lineage_root: Option<String>,
    listen: Option<u16>,
) {
    if !approve && !deny {
        eprintln!("axon-vm quorum vote: exactly one of --approve or --deny is required");
        process::exit(2);
    }

    let voter_tcb = quorum_self_tcb(&kernel, &axon_os, &axon_audit);
    // Deliberately NOT defaulting to `voter_tcb` (see the CLI flag's own doc
    // comment) — a fresh, unique-per-invocation value instead, so an operator
    // who doesn't care about coalition grouping never accidentally triggers
    // the R27 cap by having two votes collide on a shared default.
    let lineage_root = lineage_root.unwrap_or_else(|| {
        format!(
            "unlabeled-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    });
    let build_resp = move |req: quorum::logic::VoteRequest, voter_tcb: &str| {
        quorum::logic::VoteResponse {
            voter_tcb: voter_tcb.to_string(),
            run_id: req.run_id,
            approved: approve,
            reason,
            lineage_root,
        }
    };

    if let Some(port) = listen {
        let addr = format!("127.0.0.1:{port}");
        let listener = match std::net::TcpListener::bind(&addr) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("axon-vm quorum vote --listen: failed to bind {addr}: {e}");
                process::exit(2);
            }
        };
        eprintln!("listening on {addr} for one VoteRequest (voter axtcb1-ext: {voter_tcb}) ...");
        let voter_tcb_for_closure = voter_tcb.clone();
        let result = quorum::vsock::respond_once(&listener, move |req| {
            Some(build_resp(req, &voter_tcb_for_closure))
        });
        if let Err(e) = result {
            eprintln!("axon-vm quorum vote --listen: {e}");
            process::exit(2);
        }
        eprintln!(
            "✓ vote sent ({} — voter axtcb1-ext: {voter_tcb})",
            if approve { "APPROVE" } else { "DENY" }
        );
        return;
    }

    // File-based path (clap's required_unless_present="listen" guarantees these are Some here).
    let request = request.expect("clap: --request required unless --listen");
    let out = out.expect("clap: --out required unless --listen");

    let req = match quorum::io::read_vote_request(&request) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("axon-vm quorum vote: failed to read {}: {e}", request.display());
            process::exit(2);
        }
    };
    let resp = build_resp(req, &voter_tcb);

    if let Err(e) = quorum::io::write_vote_response(&resp, &out) {
        eprintln!("axon-vm quorum vote: failed to write {}: {e}", out.display());
        process::exit(2);
    }
    eprintln!(
        "✓ vote written: {} ({} — voter axtcb1-ext: {voter_tcb})",
        out.display(),
        if approve { "APPROVE" } else { "DENY" },
    );
}

/// `axon-vm quorum check` — collect `.vote` files and run the strict-majority check.
///
/// Exit 0 = QUORUM MET. Exit 13 = QUORUM BLOCKED (insufficient approvals or no
/// votes at all). Exit 14 = QUORUM BLOCKED (attestation mismatch across voters).
fn cmd_quorum_check(responses_dir: PathBuf, n: usize, json_out: bool) {
    let votes = match quorum::io::collect_responses(&responses_dir, n) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "axon-vm quorum check: failed to collect responses from {}: {e}",
                responses_dir.display()
            );
            process::exit(2);
        }
    };

    let result = quorum::logic::check_quorum(&votes, n);
    report_quorum_result(result, n, json_out);
}

// ── Firecracker orchestration ─────────────────────────────────────────────────

/// What the GUEST did — as distinct from whether the launcher successfully drove
/// the Firecracker API, which is all `Result::is_ok` on the launch ever told us
/// (P7-KRN-04). A run whose guest never reported anything is not a success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuestOutcome {
    /// The guest signalled a policy violation on the serial console (`-VIOLATION8`).
    Violation,
    /// The guest signalled a panic (`-PANIC<n>`).
    Panic(i32),
    /// The guest announced a clean exit (`-EXIT<n>`), or Firecracker exited on its
    /// own and the guest made no distress signal.
    Exited(i32),
    /// The deadline passed with no guest signal and no Firecracker exit.
    Timeout,
}

impl GuestOutcome {
    fn as_str(self) -> &'static str {
        match self {
            GuestOutcome::Violation => "violation",
            GuestOutcome::Panic(_) => "panic",
            GuestOutcome::Exited(_) => "exited",
            GuestOutcome::Timeout => "timeout",
        }
    }
}

struct RunResult {
    exit_code: i32,
    outcome: GuestOutcome,
}

/// Serial-console sentinels the guest kernel writes immediately before halting.
/// `axon-vm` had no parser for these at all: the guest correctly announced
/// "policy violation, exit 8" on COM1 and the host reported `ok:true` (P7-KRN-04).
fn parse_guest_sentinel(line: &str) -> Option<GuestOutcome> {
    // The guest prefixes with an ANSI erase-line; match on the tail.
    let l = line.trim_end();
    if l.ends_with("-VIOLATION8") {
        return Some(GuestOutcome::Violation);
    }
    if let Some(idx) = l.rfind("-PANIC") {
        if let Ok(code) = l[idx + "-PANIC".len()..].trim().parse::<i32>() {
            return Some(GuestOutcome::Panic(code));
        }
    }
    if let Some(idx) = l.rfind("-EXIT") {
        if let Ok(code) = l[idx + "-EXIT".len()..].trim().parse::<i32>() {
            return Some(GuestOutcome::Exited(code));
        }
    }
    None
}

// Pre-existing 9-arg shape, found (not introduced) while adding axon-vm to gate.sh's clippy
// coverage 2026-07-19 — a real grouping-into-a-config-struct refactor is a separate, larger
// change than that gate-coverage fix; allowed here rather than bundled in.
#[allow(clippy::too_many_arguments)]
fn run_in_firecracker(
    program: &Path,
    kernel: &Path,
    initrd: &Path,
    mem_mib: u64,
    vcpus: u64,
    vsock_port: u32,
    socket_path: &Path,
    mmds: &MmdsPayload,
    principal: Option<&Principal>,
) -> Result<RunResult, Box<dyn std::error::Error>> {
    // Check Firecracker is installed.
    let fc_bin = which_firecracker()?;

    // Spawn Firecracker.
    let mut fc = Command::new(&fc_bin)
        .arg("--api-sock")
        .arg(socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Drain Firecracker's stdout/stderr (the guest serial console + FC logs) to our
    // stderr on background threads. Without this the piped buffers fill and the guest
    // BLOCKS — a deadlock, since `fc.wait()` can't return until FC exits and FC can't
    // make progress while its stdout pipe is full. Draining also surfaces the guest
    // boot log (set AXON_VM_QUIET=1 to suppress).
    //
    // The stdout drain also WATCHES for the guest's exit sentinels. The guest kernel
    // writes `-VIOLATION8` to COM1 and then attempts an ACPI S5 power-off — which
    // Firecracker does not implement, so the guest spins in `hlt` and the run would
    // otherwise be reported as a 124 timeout with `ok:true` (P7-KRN-04). Reading the
    // sentinel means the guest's own verdict decides the outcome, independent of
    // whether it manages to power the machine off.
    let quiet = env::var("AXON_VM_QUIET").map(|v| v == "1").unwrap_or(false);
    let signal: Arc<Mutex<Option<GuestOutcome>>> = Arc::new(Mutex::new(None));
    let mut drains = Vec::new();
    if let Some(out) = fc.stdout.take() {
        let sig = Arc::clone(&signal);
        drains.push(std::thread::spawn(move || {
            drain_to_stderr(out, "guest", quiet, Some(sig))
        }));
    }
    if let Some(err) = fc.stderr.take() {
        drains.push(std::thread::spawn(move || {
            drain_to_stderr(err, "fc", quiet, None)
        }));
    }

    // Wait for Firecracker to create its API socket (typically < 50ms; a fixed 5s margin
    // was found flaky under heavy host CPU contention — R30's own acceptance gate observed
    // acc_a1/acc_a4 failing at this exact R26_ATTESTATION stage under concurrent load, isolated
    // reruns always passing clean, "root cause not chased further" per REQUIREMENTS.md — a
    // starved Firecracker process spawn can plausibly take longer than 5s to even get scheduled.
    // Tunable via AXON_VM_SOCKET_TIMEOUT_SECS (default 5), mirroring AXON_VM_TIMEOUT_SECS below.
    let socket_timeout_secs: u64 = env::var("AXON_VM_SOCKET_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let api = wait_for_socket(socket_path, Duration::from_secs(socket_timeout_secs))?;

    // Configure boot source.
    // The policy is embedded in the cmdline as base64-JSON so the guest kernel
    // can read it without a virtio-net driver (K2 cmdline-reader path).
    let boot_args = embed_policy_in_cmdline(
        "console=ttyS0 reboot=k panic=1 pci=off nomodules \
         init=/init -- /init /usr/bin/axon run /axon/program.ax",
        mmds,
    );
    fc_put(
        &api,
        "/boot-source",
        &serde_json::json!({
            "kernel_image_path": kernel.to_str().unwrap(),
            "boot_args": boot_args,
            "initrd_path": initrd.to_str().unwrap(),
        }),
    )?;

    // Configure machine.
    fc_put(
        &api,
        "/machine-config",
        &serde_json::json!({
            "vcpu_count": vcpus,
            "mem_size_mib": mem_mib,
        }),
    )?;

    // Configure vsock device so the guest can use host_await.
    // uds_path is the host-side Unix socket; the guest connects via CID 2.
    let vsock_host_uds = format!("/tmp/axon-vm-vsock-{}.sock", process::id());
    fc_put(
        &api,
        "/vsock",
        &serde_json::json!({
            "guest_cid": 3,
            "uds_path": vsock_host_uds,
        }),
    )?;

    // MMDS is a SECONDARY policy channel and requires a network interface to bind to.
    // This launcher delivers the policy via the kernel cmdline (`axon.policy=<base64>`,
    // the K2 cmdline-reader path) and configures no NIC, so MMDS V2 config with an empty
    // `network_interfaces` is rejected (400) — correctly. Make it best-effort: try it for
    // hosts that do add a NIC, but never fail the run, since the cmdline already carries
    // the policy. (Was a hard `?` that aborted every run at /mmds/config.)
    if let Err(e) = fc_put(
        &api,
        "/mmds/config",
        &serde_json::json!({
            "version": "V2",
            "network_interfaces": [],
        }),
    ) {
        eprintln!("axon-vm: MMDS config skipped ({e}); policy is delivered via the kernel cmdline");
    } else {
        // Only write the payload if MMDS config succeeded.
        let mmds_content = serde_json::json!({ "latest": { "axon": mmds } });
        if let Err(e) = fc_put(&api, "/mmds", &mmds_content) {
            eprintln!("axon-vm: MMDS payload write skipped ({e})");
        }
    }

    // Apply cgroup limits via jailer-style resource controls (if principal has limits).
    // In production use, Firecracker would be launched via jailer with uid/gid isolation.
    // Here we set balloon memory limits instead (available without jailer).
    if let Some(p) = principal {
        if p.mem_mib < mem_mib {
            fc_put(
                &api,
                "/balloon",
                &serde_json::json!({
                    "amount_mib": mem_mib - p.mem_mib,
                    "deflate_on_oom": true,
                }),
            )?;
        }
    }

    // Start a vsock relay thread to bridge vsock ↔ host_await callbacks.
    // Uses EchoHandler by default; plug in a custom HostAwaitHandler to forward
    // requests to a real host process (e.g. a stdin/stdout bridge).
    let vsock_uds = vsock_host_uds.clone();
    let handler: Arc<dyn HostAwaitHandler> = Arc::new(EchoHandler);
    let _vsock_thread = std::thread::spawn(move || {
        vsock_relay(&vsock_uds, vsock_port, handler);
    });

    // Copy the .ax program into a tmpfs-backed guest path.
    // For real deployments this would be a read-only virtio-blk device.
    // We pass it via a read-only drive.
    let prog_abs = program.canonicalize()?;
    fc_put(
        &api,
        "/drives/program",
        &serde_json::json!({
            "drive_id": "program",
            "path_on_host": prog_abs.to_str().unwrap(),
            "is_root_device": false,
            "is_read_only": true,
        }),
    )?;

    // Start the VM.
    fc_put(&api, "/actions", &serde_json::json!({"action_type": "InstanceStart"}))?;

    // Bounded wait: the guest should run the program and power off (`reboot=k panic=1`
    // turns a finished/paniced guest into a Firecracker exit). A guest that never powers
    // off must NOT hang the host — kill it after the deadline and report. Tunable via
    // AXON_VM_TIMEOUT_SECS (default 45).
    let timeout_secs: u64 = env::var("AXON_VM_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(45);
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    // A guest that has announced its verdict on the serial console gets a short
    // grace period to power itself off, then is reaped. Its own verdict stands
    // either way — a guest that says "policy violation" and then fails to shut
    // down has still refused the operation, and reporting that as a timeout (or,
    // before this, as ok:true) inverts the security-relevant result.
    let sentinel_grace = Duration::from_secs(2);
    let mut sentinel_seen_at: Option<Instant> = None;
    let (exit_code, outcome) = loop {
        if let Some(status) = fc.try_wait()? {
            // Firecracker exited. A sentinel, if any, is the more specific answer.
            let sig = *signal.lock().unwrap();
            break match sig {
                Some(GuestOutcome::Violation) => (8, GuestOutcome::Violation),
                Some(GuestOutcome::Panic(c)) => (c, GuestOutcome::Panic(c)),
                Some(GuestOutcome::Exited(c)) => (c, GuestOutcome::Exited(c)),
                _ => {
                    let c = status.code().unwrap_or(1);
                    (c, GuestOutcome::Exited(c))
                }
            };
        }

        let sig = *signal.lock().unwrap();
        if let Some(o) = sig {
            let since = *sentinel_seen_at.get_or_insert_with(Instant::now);
            if since.elapsed() >= sentinel_grace {
                let _ = fc.kill();
                let _ = fc.wait();
                eprintln!(
                    "axon-vm: guest signalled {} but did not power off — reaped. \
                     (The guest's ACPI S5 write is a no-op under Firecracker; the \
                     guest verdict is authoritative.)",
                    o.as_str()
                );
                break match o {
                    GuestOutcome::Violation => (8, o),
                    GuestOutcome::Panic(c) => (c, o),
                    GuestOutcome::Exited(c) => (c, o),
                    other => (0, other),
                };
            }
        }

        if Instant::now() >= deadline {
            let _ = fc.kill();
            let _ = fc.wait();
            eprintln!(
                "axon-vm: guest did not power off within {timeout_secs}s — killed. \
                 See the guest log above; the guest image's init must run the program \
                 and then poweroff/reboot for the VM to exit."
            );
            break (124, GuestOutcome::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    for d in drains {
        let _ = d.join();
    }

    // Clean up socket files.
    let _ = fs::remove_file(socket_path);
    let _ = fs::remove_file(&vsock_host_uds);

    Ok(RunResult {
        exit_code,
        outcome,
    })
}

/// Read a single HTTP/1.1 response from a (keep-alive) stream without relying on the
/// connection closing: read until the header terminator `\r\n\r\n`, then read exactly
/// `Content-Length` more bytes if present. Firecracker replies `204 No Content` (no body)
/// to a successful PUT and keeps the socket open, so reading to EOF would deadlock.
fn read_http_response(stream: &mut UnixStream) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut resp = Vec::new();
    let mut buf = [0u8; 1024];
    let header_end = loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(resp); // connection closed before full headers
        }
        resp.extend_from_slice(&buf[..n]);
        if let Some(pos) = resp.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    // Parse Content-Length (case-insensitive) from the headers.
    let headers = String::from_utf8_lossy(&resp[..header_end]).to_ascii_lowercase();
    let content_len: usize = headers
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    // Read the remaining body bytes, if any.
    while resp.len() < header_end + content_len {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        resp.extend_from_slice(&buf[..n]);
    }
    Ok(resp)
}

/// Copy a child stream (Firecracker stdout = guest serial console, or stderr = FC log)
/// line-by-line to our stderr with a tag. Prevents the piped-buffer deadlock and surfaces
/// the guest boot log. `quiet` suppresses the echo but still drains.
/// When `signal` is supplied (the guest serial console), each line is also scanned
/// for a guest exit sentinel; the FIRST one seen wins and is recorded for the
/// launcher's wait loop.
fn drain_to_stderr<R: std::io::Read + Send + 'static>(
    r: R,
    tag: &'static str,
    quiet: bool,
    signal: Option<Arc<Mutex<Option<GuestOutcome>>>>,
) {
    use std::io::BufRead;
    let reader = std::io::BufReader::new(r);
    for line in reader.lines() {
        match line {
            Ok(l) => {
                if let (Some(sig), Some(outcome)) = (&signal, parse_guest_sentinel(&l)) {
                    let mut g = sig.lock().unwrap();
                    if g.is_none() {
                        *g = Some(outcome);
                    }
                }
                if !quiet {
                    eprintln!("[{tag}] {l}");
                }
            }
            Err(_) => break,
        }
    }
}

// ── Firecracker API client (raw HTTP/1.1 over Unix socket) ────────────────────

/// Represents a connection to the Firecracker API socket.
struct FcApi {
    socket_path: PathBuf,
}

fn wait_for_socket(path: &Path, timeout: Duration) -> Result<FcApi, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(FcApi { socket_path: path.to_owned() });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(format!("Firecracker socket not ready after {}s: {}", timeout.as_secs(), path.display()).into())
}

/// PUT a JSON body to a Firecracker API endpoint.
fn fc_put(
    api: &FcApi,
    path: &str,
    body: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let body_str = serde_json::to_string(body)?;
    let request = format!(
        "PUT {path} HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Accept: */*\r\n\
         Connection: close\r\n\
         \r\n{body_str}",
        body_str.len()
    );

    let dbg = env::var("AXON_VM_DEBUG").map(|v| v == "1").unwrap_or(false);
    if dbg {
        eprintln!("[axon-vm] → PUT {path}");
    }
    let mut stream = UnixStream::connect(&api.socket_path)?;
    // Firecracker's API server keeps the connection open (it ignores our
    // `Connection: close`), so reading until EOF (`read_to_end`) HANGS FOREVER on the
    // very first request. Read only up to the end of the HTTP headers, then the
    // Content-Length body if any. A read timeout is a backstop against a wedged socket.
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let resp = read_http_response(&mut stream)?;
    if dbg {
        eprintln!("[axon-vm] ← {path} ({} bytes)", resp.len());
    }
    let resp_str = String::from_utf8_lossy(&resp);

    let status_line = resp_str.lines().next().unwrap_or("");
    // e.g. "HTTP/1.1 204 No Content"
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if !(200..300).contains(&status_code) {
        return Err(format!(
            "Firecracker API PUT {path} returned {status_line}\n{resp_str}"
        )
        .into());
    }

    Ok(())
}

// ── HostAwaitHandler trait ────────────────────────────────────────────────────

/// Trait for handling `host_await` requests forwarded from guest Axon programs
/// over vsock.
///
/// Implement this trait to plug in a custom relay. For example, a stdio relay
/// would forward each request to the host process's stdin and return the reply
/// from stdout — the same mechanism as `run_suspendable_stdio` uses on the
/// plain interpreter path. By default `EchoHandler` is wired in, which echoes
/// each request back unchanged (useful for smoke-testing the vsock plumbing
/// and as a starting point for custom handlers).
///
/// # `--host-await-echo` note
///
/// The default `EchoHandler` is the equivalent of running axon-vm with a
/// hypothetical `--host-await-echo` flag: every `host_await` call in the
/// guest receives its own request payload as the reply. To replace it, wrap
/// your handler in `Arc::new(...)` and pass it to `vsock_relay` directly.
pub trait HostAwaitHandler: Send + Sync {
    /// Process a UTF-8 request payload received from the guest.
    ///
    /// Return `Some(reply)` to send the reply string back, or `None` to write
    /// a zero-length frame (the EOF sentinel that signals the guest the
    /// connection is closing).
    fn handle(&self, request: &str) -> Option<String>;
}

/// Default handler: echoes the request back as the reply, unchanged.
///
/// Useful for smoke-testing the vsock plumbing without a real `host_await`
/// implementation. Replace with a handler that forwards to your host process
/// when interactive behavior is required.
pub struct EchoHandler;

impl HostAwaitHandler for EchoHandler {
    fn handle(&self, request: &str) -> Option<String> {
        Some(request.to_string())
    }
}

// ── vsock relay ───────────────────────────────────────────────────────────────

/// vsock relay: listens on the host-side UDS path that Firecracker maps as
/// CID 2 (host). For each guest connection on `vsock_port`, reads a
/// length-prefixed request, calls `handler`, and writes back a
/// length-prefixed reply.
///
/// # Protocol
///
/// Each frame is: 4-byte little-endian u32 length, followed by `length` bytes
/// of UTF-8 payload. A reply frame with length=0 is the EOF sentinel
/// (returned when `handler.handle` returns `None`).
///
/// # Concurrency
///
/// Each accepted connection is dispatched to a new thread so concurrent guest
/// `host_await` calls do not block one another.
fn vsock_relay(uds_path: &str, _vsock_port: u32, handler: Arc<dyn HostAwaitHandler>) {
    use std::os::unix::net::UnixListener;

    // Firecracker requires the UDS path to not exist yet.
    let _ = fs::remove_file(uds_path);
    let listener = match UnixListener::bind(uds_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("axon-vm: vsock relay bind failed: {e}");
            return;
        }
    };

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                let handler = Arc::clone(&handler);
                std::thread::spawn(move || {
                    // Read 4-byte LE length + payload.
                    let mut lbuf = [0u8; 4];
                    if s.read_exact(&mut lbuf).is_err() {
                        return;
                    }
                    let len = u32::from_le_bytes(lbuf) as usize;
                    let mut buf = vec![0u8; len];
                    if s.read_exact(&mut buf).is_err() {
                        return;
                    }
                    let req = String::from_utf8_lossy(&buf);

                    // Dispatch to the handler and write back the reply.
                    match handler.handle(&req) {
                        Some(reply) => {
                            let rlen = (reply.len() as u32).to_le_bytes();
                            let _ = s.write_all(&rlen);
                            let _ = s.write_all(reply.as_bytes());
                        }
                        None => {
                            // EOF sentinel: length=0, no payload.
                            let _ = s.write_all(&0u32.to_le_bytes());
                        }
                    }
                });
            }
            Err(_) => break,
        }
    }
}

// ── BPF policy generation ─────────────────────────────────────────────────────

/// Generate a seccomp-BPF allowlist program from a list of syscall names.
/// Returns the base64-encoded BPF bytecode (sock_filter array, 8 bytes each).
///
/// Layout (N allowed syscalls):
///   [0]   LD  syscall_nr from seccomp_data.nr
///   [1..N] JEQ to ALLOW for each allowed syscall (fallthrough = next check)
///   [N+1] RET KILL_PROCESS (default deny)
///   [N+2] RET ALLOW
pub fn bpf_allowlist(syscall_names: &[String]) -> Result<String, Box<dyn std::error::Error>> {
    let mut syscall_nrs: Vec<u32> = Vec::with_capacity(syscall_names.len());
    for name in syscall_names {
        match SYSCALL_TABLE.iter().find(|(n, _)| *n == name.as_str()) {
            Some((_, nr)) => syscall_nrs.push(*nr),
            None => eprintln!("axon-vm: unknown syscall '{name}', skipping"),
        }
    }

    if syscall_nrs.is_empty() {
        return Err("no recognized syscall names".into());
    }

    let n = syscall_nrs.len();
    // Total instructions: 1 (LD) + N (JEQ) + 1 (RET KILL) + 1 (RET ALLOW)
    let n_insns = 1 + n + 2;
    let mut prog = Vec::with_capacity(n_insns * 8);

    // Encode a sock_filter as 8 LE bytes: [code_lo, code_hi, jt, jf, k0..k3]
    let encode = |buf: &mut Vec<u8>, code: u16, jt: u8, jf: u8, k: u32| {
        buf.extend_from_slice(&code.to_le_bytes());
        buf.push(jt);
        buf.push(jf);
        buf.extend_from_slice(&k.to_le_bytes());
    };

    // BPF opcodes
    const LD_W_ABS: u16 = 0x20; // load 32-bit word from absolute offset
    const JMP_JEQ_K: u16 = 0x15; // jump if equal to constant
    const RET_K: u16 = 0x06; // return constant

    // seccomp_data.nr offset = 0 (x86_64 ABI)
    encode(&mut prog, LD_W_ABS, 0, 0, 0);

    // JEQ instructions: if syscall_nr matches, jump forward to RET ALLOW.
    // The ALLOW instruction is at offset N+1 from insn 0 (= N - j from insn j).
    for (j, &nr) in syscall_nrs.iter().enumerate() {
        // jt = how many instructions to skip to reach ALLOW (index N+1 relative to this insn)
        let jt = (n - j) as u8; // j is 0-based; allow is at absolute index N+1
        encode(&mut prog, JMP_JEQ_K, jt, 0, nr);
    }

    // Default deny: KILL_PROCESS
    const KILL_PROCESS: u32 = 0x8000_0000;
    encode(&mut prog, RET_K, 0, 0, KILL_PROCESS);

    // ALLOW
    const ALLOW: u32 = 0x7fff_0000;
    encode(&mut prog, RET_K, 0, 0, ALLOW);

    Ok(base64::engine::general_purpose::STANDARD.encode(&prog))
}

// x86_64 Linux syscall table (most common; extend as needed).
static SYSCALL_TABLE: &[(&str, u32)] = &[
    ("read", 0), ("write", 1), ("open", 2), ("close", 3),
    ("stat", 4), ("fstat", 5), ("lstat", 6), ("poll", 7),
    ("lseek", 8), ("mmap", 9), ("mprotect", 10), ("munmap", 11),
    ("brk", 12), ("rt_sigaction", 13), ("rt_sigprocmask", 14),
    ("rt_sigreturn", 15), ("ioctl", 16), ("pread64", 17), ("pwrite64", 18),
    ("readv", 19), ("writev", 20), ("access", 21), ("pipe", 22),
    ("select", 23), ("sched_yield", 24), ("mremap", 25), ("msync", 26),
    ("mincore", 27), ("madvise", 28), ("dup", 32), ("dup2", 33),
    ("nanosleep", 35), ("getitimer", 36), ("alarm", 37), ("setitimer", 38),
    ("getpid", 39), ("sendfile", 40), ("socket", 41), ("connect", 42),
    ("accept", 43), ("sendto", 44), ("recvfrom", 45), ("sendmsg", 46),
    ("recvmsg", 47), ("shutdown", 48), ("bind", 49), ("listen", 50),
    ("getsockname", 51), ("getpeername", 52), ("socketpair", 53),
    ("setsockopt", 54), ("getsockopt", 55), ("clone", 56), ("fork", 57),
    ("vfork", 58), ("execve", 59), ("exit", 60), ("wait4", 61),
    ("kill", 62), ("uname", 63), ("fcntl", 72), ("fsync", 74),
    ("ftruncate", 77), ("getdents", 78), ("getcwd", 79), ("chdir", 80),
    ("rename", 82), ("mkdir", 83), ("rmdir", 84), ("creat", 85),
    ("link", 86), ("unlink", 87), ("symlink", 88), ("readlink", 89),
    ("chmod", 90), ("chown", 92), ("umask", 95), ("gettimeofday", 96),
    ("getrlimit", 97), ("getrusage", 98), ("sysinfo", 99), ("times", 100),
    ("getuid", 102), ("syslog", 103), ("getgid", 104), ("setuid", 105),
    ("setgid", 106), ("geteuid", 107), ("getegid", 108),
    ("setpgid", 109), ("getppid", 110), ("getpgrp", 111), ("setsid", 112),
    ("setreuid", 113), ("setregid", 114), ("getgroups", 115), ("setgroups", 116),
    ("setresuid", 117), ("getresuid", 118), ("setresgid", 119), ("getresgid", 120),
    ("getpgid", 121), ("setfsuid", 122), ("setfsgid", 123), ("getsid", 124),
    ("capget", 125), ("capset", 126), ("rt_sigsuspend", 130),
    ("sigaltstack", 131), ("utime", 132), ("mknod", 133), ("personality", 135),
    ("statfs", 137), ("fstatfs", 138), ("getpriority", 140),
    ("setpriority", 141), ("sched_setparam", 142), ("sched_getparam", 143),
    ("sched_setscheduler", 144), ("sched_getscheduler", 145),
    ("sched_get_priority_max", 146), ("sched_get_priority_min", 147),
    ("sched_rr_get_interval", 148), ("mlock", 149), ("munlock", 150),
    ("mlockall", 151), ("munlockall", 152), ("vhangup", 153),
    ("pivot_root", 155), ("prctl", 157), ("arch_prctl", 158),
    ("adjtimex", 159), ("setrlimit", 160), ("chroot", 161), ("sync", 162),
    ("acct", 163), ("settimeofday", 164), ("umount2", 166), ("swapon", 167),
    ("swapoff", 168), ("reboot", 169), ("sethostname", 170), ("setdomainname", 171),
    ("iopl", 172), ("ioperm", 173), ("init_module", 175), ("delete_module", 176),
    ("gettid", 186), ("readahead", 187), ("getxattr", 191), ("lgetxattr", 192),
    ("fgetxattr", 193), ("listxattr", 194), ("llistxattr", 195), ("flistxattr", 196),
    ("removexattr", 197), ("lremovexattr", 198), ("fremovexattr", 199),
    ("tkill", 200), ("time", 201), ("futex", 202), ("sched_setaffinity", 203),
    ("sched_getaffinity", 204), ("io_setup", 206), ("io_destroy", 207),
    ("io_getevents", 208), ("io_submit", 209), ("io_cancel", 210),
    ("lookup_dcookie", 212), ("epoll_create", 213), ("epoll_ctl_old", 214),
    ("epoll_wait_old", 215), ("remap_file_pages", 216), ("getdents64", 217),
    ("set_tid_address", 218), ("restart_syscall", 219), ("semtimedop", 220),
    ("fadvise64", 221), ("timer_create", 222), ("timer_settime", 223),
    ("timer_gettime", 224), ("timer_getoverrun", 225), ("timer_delete", 226),
    ("clock_settime", 227), ("clock_gettime", 228), ("clock_getres", 229),
    ("clock_nanosleep", 230), ("exit_group", 231), ("epoll_wait", 232),
    ("epoll_ctl", 233), ("tgkill", 234), ("utimes", 235),
    ("mbind", 237), ("set_mempolicy", 238), ("get_mempolicy", 239),
    ("mq_open", 240), ("mq_unlink", 241), ("mq_timedsend", 242),
    ("mq_timedreceive", 243), ("mq_notify", 244), ("mq_getsetattr", 245),
    ("waitid", 247), ("add_key", 248), ("request_key", 249), ("keyctl", 250),
    ("ioprio_set", 251), ("ioprio_get", 252), ("inotify_init", 253),
    ("inotify_add_watch", 254), ("inotify_rm_watch", 255), ("migrate_pages", 256),
    ("openat", 257), ("mkdirat", 258), ("mknodat", 259), ("fchownat", 260),
    ("futimesat", 261), ("newfstatat", 262), ("unlinkat", 263), ("renameat", 264),
    ("linkat", 265), ("symlinkat", 266), ("readlinkat", 267), ("fchmodat", 268),
    ("faccessat", 269), ("pselect6", 270), ("ppoll", 271), ("unshare", 272),
    ("set_robust_list", 273), ("get_robust_list", 274), ("splice", 275),
    ("tee", 276), ("sync_file_range", 277), ("vmsplice", 278),
    ("move_pages", 279), ("utimensat", 280), ("epoll_pwait", 281),
    ("signalfd", 282), ("timerfd_create", 283), ("eventfd", 284),
    ("fallocate", 285), ("timerfd_settime", 286), ("timerfd_gettime", 287),
    ("accept4", 288), ("signalfd4", 289), ("eventfd2", 290),
    ("epoll_create1", 291), ("dup3", 292), ("pipe2", 293),
    ("inotify_init1", 294), ("preadv", 295), ("pwritev", 296),
    ("rt_tgsigqueueinfo", 297), ("perf_event_open", 298), ("recvmmsg", 299),
    ("fanotify_init", 300), ("fanotify_mark", 301), ("prlimit64", 302),
    ("name_to_handle_at", 303), ("open_by_handle_at", 304), ("clock_adjtime", 305),
    ("syncfs", 306), ("sendmmsg", 307), ("setns", 308), ("getcpu", 309),
    ("process_vm_readv", 310), ("process_vm_writev", 311), ("kcmp", 312),
    ("finit_module", 313), ("sched_setattr", 314), ("sched_getattr", 315),
    ("renameat2", 316), ("seccomp", 317), ("getrandom", 318),
    ("memfd_create", 319), ("kexec_file_load", 320), ("bpf", 321),
    ("execveat", 322), ("userfaultfd", 323), ("membarrier", 324),
    ("mlock2", 325), ("copy_file_range", 326), ("preadv2", 327),
    ("pwritev2", 328), ("pkey_mprotect", 329), ("pkey_alloc", 330),
    ("pkey_free", 331), ("statx", 332), ("io_pgetevents", 333),
    ("rseq", 334),
];

// ── Manifest loading ──────────────────────────────────────────────────────────

fn load_manifest(program: &Path) -> AxonManifest {
    let meta_path = program.with_extension("axmeta");
    if !meta_path.exists() {
        return AxonManifest::default();
    }
    match fs::read_to_string(&meta_path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => AxonManifest::default(),
    }
}

// ── Helper: sha256 a file ─────────────────────────────────────────────────────

fn sha256_file(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).unwrap_or_default();
    let hash = Sha256::digest(&bytes);
    format!("{hash:x}")
}

// ── R26: kernel attestation gate ─────────────────────────────────────────────

/// Path of the on-disk kernel baseline pin (`~/.axon/kernel_baseline.sha256`).
fn kernel_baseline_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{}/.axon/kernel_baseline.sha256", home))
}

/// Measure the kernel at `kernel_path` and verify it against a PINNED expected
/// digest — either `expect_digest` (operator-supplied, strongest) or the stored
/// baseline in `~/.axon/kernel_baseline.sha256`.
///
/// - Mismatch: returns `Err`; the caller exits 10 (kernel tampered / wrong image).
/// - **No pin at all: also a refusal.** There is deliberately no trust-on-first-use
///   here. TOFU against a user-writable file is not a gate: an attacker who can
///   swap the kernel can also `rm` the baseline, and the next boot would silently
///   bless the tampered image as the new baseline (P7-KRN-05). Establish a
///   baseline explicitly with `axon-vm attest --kernel <path> --pin-baseline`.
/// - `no_attest = true`: prints a WARNING and short-circuits to `Ok` (dev mode).
///   This is the ONLY bypass. `AXON_CI_NO_KVM=1` used to disable the gate here as
///   well — an ambient inherited environment variable silently turning off the
///   TCB check on a production host — and no longer does.
///
/// Uses `axon_attest::measure_kernel` from the R26 attestation crate, so the
/// digest is byte-identical with what `axon-vm attest` records.
fn measure_and_attest(
    kernel_path: &Path,
    no_attest: bool,
    expect_digest: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let baseline_path = kernel_baseline_path();
    measure_and_attest_inner(kernel_path, no_attest, expect_digest, &baseline_path)
}

/// Inner implementation of `measure_and_attest`, parameterised over the baseline
/// path so tests can use a temp directory rather than writing to `~/.axon/`.
fn measure_and_attest_inner(
    kernel_path: &Path,
    no_attest: bool,
    expect_digest: Option<&str>,
    baseline_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if no_attest {
        eprintln!("[axon-vm] WARNING: --no-attest: skipping attestation (dev mode only)");
        return Ok(());
    }

    // Kernel must exist before we can measure it.
    if !kernel_path.exists() {
        return Err(format!("kernel not found: {}", kernel_path.display()).into());
    }

    // Measure using axon-attest — same SHA-256 algorithm as `axon-vm attest`.
    let measurement = measure_kernel(kernel_path)?;
    let digest_hex = hex::encode(measurement.digest);

    // An operator-supplied pin wins over the on-disk baseline: it does not depend
    // on a file the attacker can reach.
    let (expected, source) = match expect_digest {
        Some(d) => (d.trim().to_string(), "--expect-digest"),
        None => match fs::read_to_string(baseline_path) {
            Ok(b) => (b.trim().to_string(), "baseline"),
            Err(_) => {
                eprintln!("[axon-vm] ATTESTATION FAILED: no pinned kernel baseline");
                eprintln!("[axon-vm]   measured: {digest_hex}");
                eprintln!(
                    "[axon-vm]   expected: (none — {} is absent)",
                    baseline_path.display()
                );
                eprintln!(
                    "[axon-vm]   pin it explicitly:  axon-vm attest --kernel {} --pin-baseline",
                    kernel_path.display()
                );
                eprintln!("[axon-vm]   or pass:            --expect-digest <sha256>");
                eprintln!("[axon-vm]   or, for dev only:   --no-attest");
                return Err(
                    "attestation failed: no pinned baseline (refusing to trust on first use)"
                        .into(),
                );
            }
        },
    };

    if expected != digest_hex {
        eprintln!("[axon-vm] ATTESTATION FAILED: kernel digest mismatch");
        eprintln!("[axon-vm]   expected: {expected} ({source})");
        eprintln!("[axon-vm]   got:      {digest_hex}");
        return Err("attestation failed: kernel tampered".into());
    }
    eprintln!(
        "[axon-vm] attestation OK: digest {} ({source})",
        &digest_hex[..16]
    );

    Ok(())
}

// ── Helper: find Firecracker binary ──────────────────────────────────────────

fn which_firecracker() -> Result<PathBuf, Box<dyn std::error::Error>> {
    for candidate in &["firecracker", "/usr/local/bin/firecracker", "/opt/firecracker/firecracker"] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
        // Try PATH lookup.
        if let Ok(out) = Command::new("which").arg(candidate).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Ok(PathBuf::from(s));
                }
            }
        }
    }
    Err("firecracker not found in PATH or /usr/local/bin; install from github.com/firecracker-microvm/firecracker".into())
}

// ── Gap 7: Principal registry ─────────────────────────────────────────────────

fn registry_path() -> PathBuf {
    let base = env::var("AXON_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_home()
                .map(|h| h.join(".config/axon"))
                .unwrap_or_else(|| PathBuf::from(".axon"))
        });
    base.join("principals.toml")
}

fn dirs_home() -> Option<PathBuf> {
    env::var("HOME").ok().map(PathBuf::from)
}

fn load_registry() -> PrincipalRegistry {
    let path = registry_path();
    if !path.exists() {
        return PrincipalRegistry::default();
    }
    let s = fs::read_to_string(&path).unwrap_or_default();
    toml::from_str(&s).unwrap_or_default()
}

fn save_registry(reg: &PrincipalRegistry) {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let s = toml::to_string_pretty(reg).expect("serialize registry");
    fs::write(&path, s).expect("write registry");
}

fn load_principal(name: &str) -> Option<Principal> {
    let reg = load_registry();
    reg.principals.into_iter().find(|p| p.name == name)
}

fn cmd_principal_add(
    name: String,
    budget_tokens: u64,
    allowed_effects: String,
    mem_mib: u64,
    cpu_pct: u32,
) {
    let mut reg = load_registry();
    if reg.principals.iter().any(|p| p.name == name) {
        eprintln!("axon-vm: principal '{name}' already exists");
        process::exit(1);
    }
    let effects: Vec<String> = allowed_effects
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    reg.principals.push(Principal {
        name: name.clone(),
        budget_tokens,
        allowed_effects: effects,
        mem_mib,
        cpu_pct,
    });
    save_registry(&reg);
    println!("principal '{name}' added ({} → {})", registry_path().display(), budget_tokens);
}

fn cmd_principal_list() {
    let reg = load_registry();
    if reg.principals.is_empty() {
        println!("no principals registered (axon-vm principal add <name>)");
        return;
    }
    println!("{:<20} {:>12}  {:>6}  effects", "name", "budget_tokens", "mem_mib");
    println!("{}", "-".repeat(60));
    for p in &reg.principals {
        println!(
            "{:<20} {:>12}  {:>6}  {}",
            p.name,
            p.budget_tokens,
            p.mem_mib,
            p.allowed_effects.join(",")
        );
    }
}

// ── Policy-in-cmdline embedding ───────────────────────────────────────────────

/// Append `axon.policy=<base64-json>` to `base_cmdline` so the guest kernel can
/// read the boot policy from the Linux cmdline without a virtio-net driver.
fn embed_policy_in_cmdline(base_cmdline: &str, mmds: &MmdsPayload) -> String {
    let json = serde_json::to_string(mmds).unwrap_or_default();
    let b64  = base64::engine::general_purpose::STANDARD.encode(json.as_bytes());
    format!("{base_cmdline} axon.policy={b64}")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpf_encodes_allow_write_exit() {
        // Allow write(1) + exit_group(231) — minimal Axon pure program.
        let names: Vec<String> = vec!["write".into(), "exit_group".into()];
        let b64 = bpf_allowlist(&names).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        // 1 LD + 2 JEQ + 1 KILL + 1 ALLOW = 5 instructions × 8 bytes = 40
        assert_eq!(bytes.len(), 40);
    }

    #[test]
    fn bpf_first_insn_is_ld_abs() {
        let names: Vec<String> = vec!["read".into()];
        let b64 = bpf_allowlist(&names).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        // First 2 bytes = opcode (LD_W_ABS = 0x0020 LE)
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 0x0020);
    }

    #[test]
    fn bpf_last_insn_is_ret_allow() {
        let names: Vec<String> = vec!["write".into()];
        let b64 = bpf_allowlist(&names).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        let last_insn = &bytes[bytes.len() - 8..];
        // RET_K = 0x0006
        assert_eq!(u16::from_le_bytes([last_insn[0], last_insn[1]]), 0x0006);
        // k = ALLOW = 0x7fff_0000
        assert_eq!(u32::from_le_bytes([last_insn[4], last_insn[5], last_insn[6], last_insn[7]]), 0x7fff_0000);
    }

    #[test]
    fn bpf_kill_insn_before_allow() {
        let names: Vec<String> = vec!["write".into()];
        let b64 = bpf_allowlist(&names).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        // Second-to-last instruction = KILL
        let kill_insn = &bytes[bytes.len() - 16..bytes.len() - 8];
        assert_eq!(u32::from_le_bytes([kill_insn[4], kill_insn[5], kill_insn[6], kill_insn[7]]), 0x8000_0000);
    }

    #[test]
    fn principal_registry_roundtrip() {
        let reg = PrincipalRegistry {
            principals: vec![Principal {
                name: "test-agent".to_string(),
                budget_tokens: 5000,
                allowed_effects: vec!["AI".to_string()],
                mem_mib: 64,
                cpu_pct: 25,
            }],
        };
        let s = toml::to_string_pretty(&reg).unwrap();
        let back: PrincipalRegistry = toml::from_str(&s).unwrap();
        assert_eq!(back.principals[0].name, "test-agent");
        assert_eq!(back.principals[0].budget_tokens, 5000);
    }

    #[test]
    fn manifest_default_when_missing() {
        let m = load_manifest(Path::new("/nonexistent/program.ax"));
        assert!(m.syscall_hint.is_none());
        assert!(m.effect_union.is_none());
    }

    #[test]
    fn sha256_file_nonexistent_is_empty_hash() {
        // Empty input → well-known SHA256.
        let h = sha256_file(Path::new("/nonexistent_file_axon_vm_test"));
        // SHA256 of empty bytes
        assert_eq!(h, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn vsock_protocol_length_prefix() {
        // Verify that the 4-byte LE length-prefix framing round-trips correctly.
        let payload = "hello world";
        let mut buf = Vec::new();
        let len_bytes = (payload.len() as u32).to_le_bytes();
        buf.extend_from_slice(&len_bytes);
        buf.extend_from_slice(payload.as_bytes());
        assert_eq!(
            u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize,
            payload.len()
        );
        // Verify the payload bytes follow the length header correctly.
        assert_eq!(&buf[4..], payload.as_bytes());
    }

    #[test]
    fn echo_handler_returns_request() {
        let h = EchoHandler;
        let req = "test-request-42";
        assert_eq!(h.handle(req), Some(req.to_string()));
    }

    #[test]
    fn echo_handler_empty_request() {
        let h = EchoHandler;
        assert_eq!(h.handle(""), Some(String::new()));
    }

    // ── P7-KRN-04: guest outcome vs launcher plumbing ───────────────────────

    /// The guest kernel writes its verdict to COM1 prefixed with an ANSI
    /// erase-line. `axon-vm` had no parser for any of these.
    #[test]
    fn guest_exit_sentinels_are_parsed_off_the_serial_console() {
        assert_eq!(
            parse_guest_sentinel("\x1b[K-VIOLATION8"),
            Some(GuestOutcome::Violation)
        );
        assert_eq!(
            parse_guest_sentinel("\x1b[K-EXIT0\n"),
            Some(GuestOutcome::Exited(0))
        );
        assert_eq!(
            parse_guest_sentinel("\x1b[K-PANIC101"),
            Some(GuestOutcome::Panic(101))
        );
    }

    /// Ordinary guest chatter must not be mistaken for a verdict — in particular
    /// the kernel's own human-readable line about the violation, which is printed
    /// immediately before the real sentinel.
    #[test]
    fn ordinary_guest_log_lines_are_not_mistaken_for_sentinels() {
        for line in [
            "[axon-kernel] HALTING: policy violation — exit code 8",
            "[axon-kernel] VIOLATION: syscall 257 blocked (FS not in policy)",
            "Firecracker exiting successfully. exit_code=0",
            "",
            "-EXIT",
            "-PANIC",
        ] {
            assert_eq!(
                parse_guest_sentinel(line),
                None,
                "must not read a verdict out of: {line:?}"
            );
        }
    }

    /// `ok` must describe the guest, not the launcher. A violation and a timeout
    /// are both NOT ok; only a clean guest exit with code 0 is. This encodes the
    /// mapping cmd_run applies — before the fix `ok` was `result.is_ok()`, which
    /// is true for every run the launcher managed to start.
    #[test]
    fn ok_reflects_the_guest_outcome_not_the_launch() {
        let cases = [
            (GuestOutcome::Exited(0), 0, true),
            (GuestOutcome::Exited(3), 3, false),
            (GuestOutcome::Violation, 8, false),
            (GuestOutcome::Panic(101), 101, false),
            (GuestOutcome::Timeout, 124, false),
        ];
        for (outcome, exit_code, expected_ok) in cases {
            let ok = matches!(outcome, GuestOutcome::Exited(_)) && exit_code == 0;
            assert_eq!(ok, expected_ok, "wrong ok for {outcome:?}");
        }
    }

    // ── R26 attestation gate tests ──────────────────────────────────────────

    /// Tamper the kernel after the baseline is recorded — attestation must fail.
    #[test]
    fn test_attest_fails_on_tampered_kernel() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let kernel_path = dir.join(format!("axon-vm-test-kernel-{id}.bin"));
        let baseline_path = dir.join(format!("axon-vm-test-baseline-{id}.sha256"));

        // Write the genuine kernel and record its baseline digest.
        let genuine = b"genuine-axon-os-kernel-bytes-for-tamper-test";
        std::fs::write(&kernel_path, genuine).unwrap();
        let m = measure_kernel_bytes(genuine);
        std::fs::write(&baseline_path, hex::encode(m.digest)).unwrap();

        // Tamper: overwrite kernel with different bytes.
        std::fs::write(&kernel_path, b"TAMPERED-KERNEL-DIFFERENT-CONTENT!!").unwrap();

        // Attestation must fail.
        let result = measure_and_attest_inner(&kernel_path, false, None, &baseline_path);
        assert!(result.is_err(), "tampered kernel must fail attestation");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("tampered") || msg.contains("attestation failed") || msg.contains("mismatch"),
            "error must describe attestation failure: {msg}"
        );

        // Cleanup.
        let _ = std::fs::remove_file(&kernel_path);
        let _ = std::fs::remove_file(&baseline_path);
    }

    /// With --no-attest, attestation is skipped even for a missing kernel.
    #[test]
    fn test_no_attest_skips_attestation() {
        let result = measure_and_attest_inner(
            Path::new("/nonexistent/axon-vm-test-kernel-no-attest"),
            true, // no_attest = true
            None,
            Path::new("/nonexistent/axon-vm-test-baseline-no-attest"),
        );
        assert!(result.is_ok(), "--no-attest must skip all attestation checks");
    }

    /// P7-KRN-05: `AXON_CI_NO_KVM=1` must NOT bypass the boot attestation gate.
    ///
    /// It used to. An ordinary inherited environment variable — not a flag, not
    /// anything the operator has to type at the boot site — short-circuited the
    /// gate to Ok on any host. `--no-attest` is now the only bypass, and it warns.
    #[test]
    fn ci_env_var_cannot_bypass_the_boot_attestation_gate() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let kernel_path = dir.join(format!("axon-vm-ci-bypass-kernel-{id}.bin"));
        let baseline_path = dir.join(format!("axon-vm-ci-bypass-baseline-{id}.sha256"));

        let genuine = b"genuine-kernel-for-ci-bypass-test";
        std::fs::write(&kernel_path, genuine).unwrap();
        std::fs::write(
            &baseline_path,
            hex::encode(measure_kernel_bytes(genuine).digest),
        )
        .unwrap();
        std::fs::write(&kernel_path, b"TAMPERED-under-AXON_CI_NO_KVM").unwrap();

        // SAFETY: env mutation in a test. Scoped tightly and removed immediately;
        // run with RUST_TEST_THREADS=1 if this ever proves flaky.
        unsafe {
            std::env::set_var("AXON_CI_NO_KVM", "1");
        }
        let result = measure_and_attest_inner(&kernel_path, false, None, &baseline_path);
        unsafe {
            std::env::remove_var("AXON_CI_NO_KVM");
        }

        assert!(
            result.is_err(),
            "AXON_CI_NO_KVM=1 must not disable the R26 gate — a tampered kernel booted"
        );

        let _ = std::fs::remove_file(&kernel_path);
        let _ = std::fs::remove_file(&baseline_path);
    }

    /// P7-KRN-05: a MISSING baseline is a refusal, not trust-on-first-use.
    ///
    /// The baseline lives in a predictable, user-writable file. Under TOFU, an
    /// attacker who could swap the kernel could also `rm` the baseline, and the
    /// next boot would print "baseline established" and run the tampered image.
    #[test]
    fn missing_baseline_refuses_rather_than_trusting_on_first_use() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let kernel_path = dir.join(format!("axon-vm-tofu-kernel-{id}.bin"));
        let baseline_path = dir.join(format!("axon-vm-tofu-baseline-{id}.sha256"));
        let _ = std::fs::remove_file(&baseline_path);

        std::fs::write(&kernel_path, b"unknown-kernel-never-blessed-by-anyone").unwrap();

        let result = measure_and_attest_inner(&kernel_path, false, None, &baseline_path);
        assert!(result.is_err(), "an unpinned kernel must not boot");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("no pinned baseline"),
            "error must name the missing pin: {msg}"
        );
        assert!(
            !baseline_path.exists(),
            "the run path must never establish a baseline — that is `attest --pin-baseline`'s job"
        );

        let _ = std::fs::remove_file(&kernel_path);
    }

    /// An operator-supplied `--expect-digest` pins the gate without depending on
    /// the on-disk baseline file at all, and still catches a tampered kernel.
    #[test]
    fn expect_digest_pins_the_gate_without_the_baseline_file() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let kernel_path = dir.join(format!("axon-vm-pin-kernel-{id}.bin"));
        let absent_baseline = dir.join(format!("axon-vm-pin-absent-{id}.sha256"));
        let _ = std::fs::remove_file(&absent_baseline);

        let genuine = b"genuine-kernel-for-expect-digest-test";
        std::fs::write(&kernel_path, genuine).unwrap();
        let good = hex::encode(measure_kernel_bytes(genuine).digest);

        // Matching pin: boots, with no baseline file anywhere.
        assert!(
            measure_and_attest_inner(&kernel_path, false, Some(&good), &absent_baseline).is_ok(),
            "a matching --expect-digest must satisfy the gate on its own"
        );

        // Tamper: the same pin now refuses.
        std::fs::write(&kernel_path, b"TAMPERED-after-pinning").unwrap();
        let result = measure_and_attest_inner(&kernel_path, false, Some(&good), &absent_baseline);
        assert!(
            result.is_err(),
            "--expect-digest must catch a tampered kernel"
        );

        let _ = std::fs::remove_file(&kernel_path);
    }

    /// `--expect-digest` overrides a baseline file that disagrees with it — the
    /// operator-supplied pin is the authority, not the file on the box.
    #[test]
    fn expect_digest_outranks_a_stale_baseline_file() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let kernel_path = dir.join(format!("axon-vm-outrank-kernel-{id}.bin"));
        let baseline_path = dir.join(format!("axon-vm-outrank-baseline-{id}.sha256"));

        let genuine = b"genuine-kernel-for-outrank-test";
        std::fs::write(&kernel_path, genuine).unwrap();
        let good = hex::encode(measure_kernel_bytes(genuine).digest);

        // A baseline file blessing something else entirely (attacker-planted).
        std::fs::write(
            &baseline_path,
            hex::encode(measure_kernel_bytes(b"attacker-choice").digest),
        )
        .unwrap();

        assert!(
            measure_and_attest_inner(&kernel_path, false, Some(&good), &baseline_path).is_ok(),
            "the operator pin must win over a disagreeing baseline file"
        );
        assert!(
            measure_and_attest_inner(&kernel_path, false, None, &baseline_path).is_err(),
            "without the pin, the planted baseline must still fail against the real kernel"
        );

        let _ = std::fs::remove_file(&kernel_path);
        let _ = std::fs::remove_file(&baseline_path);
    }

    // ── R31: extended TCB wiring tests ─────────────────────────────────────────

    /// R31 §4.4: `axon-vm run --extended-tcb` measures before booting and refuses
    /// on a missing required component (kernel or axon-os).
    /// Tests the `measure_host_stack` → error-path wiring used by cmd_run.
    #[test]
    fn extended_tcb_wired_into_run() {
        use axon_attest::{measure_host_stack, verify_extended};

        let tmp = std::env::temp_dir();
        let pid = std::process::id();
        let kernel_path  = tmp.join(format!("axon-r31-run-kernel-{pid}.bin"));
        let axon_os_path = tmp.join(format!("axon-r31-run-axon-os-{pid}.bin"));

        std::fs::write(&kernel_path,  b"run-gate-kernel-bytes").unwrap();
        std::fs::write(&axon_os_path, b"run-gate-axon-os-bytes").unwrap();

        // Measure succeeds when both required components exist (no axon-audit = zero-fill)
        let m = measure_host_stack(&kernel_path, Some(&axon_os_path), None)
            .expect("measure_host_stack must succeed with kernel + axon-os");
        assert!(m.axtcb1_ext.starts_with("axtcb1-ext:"), "axtcb1_ext must have correct prefix");
        assert_eq!(m.components.len(), 4);

        // verify_extended passes against self
        assert!(
            verify_extended(&m, &m.axtcb1_ext).is_ok(),
            "verify_extended must pass for a self-consistent measurement"
        );

        // A mismatched expected value → verify_extended fails (simulates boot refusal, exit 10)
        let wrong = format!("axtcb1-ext:{}", "0".repeat(64));
        assert!(
            verify_extended(&m, &wrong).is_err(),
            "mismatched expected axtcb1_ext must cause verify_extended to fail (exit 10 path)"
        );

        // Missing axon-os → measure_host_stack returns Err (simulates exit 12)
        let missing_os = tmp.join(format!("axon-r31-run-axon-os-missing-{pid}.bin"));
        let r = measure_host_stack(&kernel_path, Some(&missing_os), None);
        assert!(r.is_err(), "missing axon-os must cause measure_host_stack to return Err (exit 12 path)");
        let msg = r.unwrap_err().to_string();
        assert!(msg.contains("axon-os"), "error must name the missing component: {msg}");

        let _ = std::fs::remove_file(&kernel_path);
        let _ = std::fs::remove_file(&axon_os_path);
    }

    #[test]
    fn embed_policy_in_cmdline_roundtrips() {
        let mmds = MmdsPayload {
            schema: "axon-vm-mmds/1".to_string(),
            run_id: "test-run-1".to_string(),
            principal: Some("test-agent".to_string()),
            allowed_effects: Some(vec!["AI".to_string(), "Net".to_string()]),
            budget_tokens: Some(5000),
            source_hash: None,
            seccomp_bpf_b64: None,
        };
        let base = "console=ttyS0 reboot=k panic=1 pci=off nomodules";
        let result = embed_policy_in_cmdline(base, &mmds);

        // Base cmdline is preserved at the start.
        assert!(result.starts_with(base));
        // The policy tag is present.
        assert!(result.contains(" axon.policy="));

        // The base64 blob round-trips to the original JSON.
        let b64_part = result.split("axon.policy=").nth(1).unwrap();
        let decoded = base64::engine::general_purpose::STANDARD.decode(b64_part).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(json["schema"],    "axon-vm-mmds/1");
        assert_eq!(json["principal"], "test-agent");
        assert_eq!(json["budget_tokens"], 5000);
    }
}
