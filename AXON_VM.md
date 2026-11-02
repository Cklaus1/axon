# axon-vm: Axon-Aware MicroVM Launcher

Architecture sketch, API mapping, and gap analysis for a Firecracker-based launcher
that derives its security policy directly from Axon's type system.

> **guest-fs GF-5 (2026-11-02): the `syscall_hint` → seccomp path below is DELETED.**
> `axon build --emit-manifest` no longer emits `syscall_hint`; the `.axmeta` carries
> `"capabilities"` (the labels `axon caps` reports) instead. `axon-vm` no longer builds a
> `seccomp_bpf_b64` from the manifest (`bpf_allowlist` and its syscall table are gone), and
> the bare-metal kernel's unused `seccomp_hint` field is gone. That allowlist was never
> enforced (the only backend `axon-vm` boots ignored it) and, applied, it killed every
> interpreter at `execve` while granting `openat` for `IO`. The one syscall profile of the
> Linux guest is `axon-guest-init`'s compiled-in `fs:none` filter (GF-3; see
> `profiles/linux-microvm/README.md`). The diagram and gap list below are the original
> sketch and are kept as history.

---

## Core Insight

Firecracker gives us the VM boundary. Axon's compiler gives us the policy.
Neither is enough alone — together they produce two independent enforcement layers:

> *Historical (pre-GF-5) diagram.* The left column (`syscall_hint` list → BPF bytecode →
> `seccomp_bpf_b64` → `prctl(PR_SET_SECCOMP)`) was never enforced and no longer exists; see the
> note at the top. Nothing derives a seccomp filter from the manifest.

```
@[contained] + effect rows
        │
        ▼
  axon build --emit-manifest
        │
        ▼
   .axmeta sidecar ──────────────────────────────────────────┐
        │                                                      │
        ▼                                                      ▼
  syscall_hint list                                  contained specs
        │                                                      │
        ▼                                                      ▼
  BPF bytecode (seccompiler)                     MMDS payload (principal,
        │                                        budget, allowed_effects,
        │                                        seccomp_bpf_b64)
        ▼                                                      │
  passed to guest via MMDS ◄─────────────────────────────────┘
        │
        ▼
  Axon runtime reads MMDS at PID-1 init
  prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &bpf)
        │                                        │
        ▼                                        ▼
  Guest kernel enforces           Jailer cgroup enforces
  syscall allowlist               CPU/mem/Principal budget
  (language-layer policy          (OS-layer policy independent
   compiled to hardware)           of guest kernel)
```

**Key finding from VMM research**: Firecracker does NOT apply seccomp to guest
processes — the KVM hardware boundary (VT-x/AMD-V) means guest syscalls never
reach host BPF evaluation. The seccomp filter must be self-applied by the Axon
runtime inside the guest via `prctl(PR_SET_SECCOMP)`. This is architecturally
clean: the compiler emits the policy, the runtime enforces it at boot.

---

## System Components

> *Historical (pre-GF-5) diagram.* The launcher's "compiles syscall_hint → BPF bytecode" step,
> the "BPF payload" / `seccomp_bpf_b64` it would put in MMDS, and the guest's "annotation-derived
> BPF" do not exist. The only guest syscall filter today is `axon-guest-init`'s compiled-in
> `fs:none` filter on the Linux profile (GF-3).

```
┌─────────────────────────────────────────────────────────────────┐
│  HOST                                                           │
│                                                                 │
│  axon-vm launch goal.ax                                         │
│       │                                                         │
│       ├─ reads goal.axmeta (--emit-manifest output)            │
│       ├─ compiles syscall_hint → BPF bytecode (seccompiler)    │
│       ├─ allocates uid/gid for Principal isolation             │
│       ├─ prepares TAP device + network namespace               │
│       │                                                         │
│       ├─ calls Firecracker REST API (Unix socket)              │
│       │     PUT /machine-config                                 │
│       │     PUT /boot-source  (prebuilt axon-guest kernel)     │
│       │     PUT /drives       (axon rootfs or initramfs)       │
│       │     PUT /network-interfaces                             │
│       │     PUT /vsock        (for host_await channel)         │
│       │     PUT /mmds/config                                    │
│       │     PUT /mmds         (principal, budget, BPF payload) │
│       │     PUT /actions  →  InstanceStart                     │
│       │                                                         │
│       └─ execs jailer (wraps Firecracker):                     │
│             --uid <principal-uid>                               │
│             --cgroup cpu.max="<budget_cpu>"                     │
│             --cgroup memory.max=<budget_mem>                    │
│             --netns <principal-netns>                           │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  FIRECRACKER VMM (jailed)                                │  │
│  │  seccomp on VMM threads (built-in, per-thread BPF)       │  │
│  │  cgroup limits enforce Principal budget at host level     │  │
│  │                                                           │  │
│  │  ┌────────────────────────────────────────────────────┐  │  │
│  │  │  GUEST (KVM VM)                                    │  │  │
│  │  │                                                    │  │  │
│  │  │  axon-guest-init (PID 1, static musl)              │  │  │
│  │  │    1. GET http://169.254.169.254/  (MMDS)          │  │  │
│  │  │    2. parse principal, budget, seccomp_bpf_b64     │  │  │
│  │  │    3. prctl(PR_SET_NO_NEW_PRIVS, 1)                │  │  │
│  │  │    4. prctl(PR_SET_SECCOMP, FILTER, &bpf)          │  │  │
│  │  │    5. exec axon goal.ax (interpreter or binary)    │  │  │
│  │  │                                                    │  │  │
│  │  │  Axon program runs under:                          │  │  │
│  │  │    - language-level @[contained] (compiler)        │  │  │
│  │  │    - guest-kernel seccomp (annotation-derived BPF) │  │  │
│  │  │    - host cgroup limits (Principal budget)         │  │  │
│  │  │    - VM hardware boundary (KVM)                    │  │  │
│  │  │                                                    │  │  │
│  │  │  host_await ←──vsock──→ axon-vm host listener      │  │  │
│  │  └────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Firecracker API Usage Map

| Firecracker endpoint | What axon-vm populates | Source |
|---|---|---|
| `PUT /machine-config` | vcpu_count, mem_size_mib from Principal budget | `.axmeta` risk + budget tokens |
| `PUT /boot-source` | Prebuilt axon-guest kernel image path | Build artifact |
| `PUT /drives` | Minimal rootfs (8 MB ext4) or initramfs-in-kernel | Build artifact |
| `PUT /network-interfaces` | TAP device for outbound Net effect calls | `.axmeta` effect_union contains "Net" |
| `PUT /vsock` | guest_cid=3, uds_path per VM instance | Always; needed for host_await |
| `PUT /mmds/config` | V2, ipv4=169.254.169.254 | Always |
| `PUT /mmds` | principal, allowed_effects, budget_tokens (no manifest-derived `seccomp_bpf_b64` since guest-fs GF-5) | `.axmeta` + Principal registry |
| `PUT /actions InstanceStart` | Boot trigger | Always |
| `PATCH /vm {"state":"Paused"}` | Before snapshot for fast-restart pool | Snapshot pool manager |
| `PUT /snapshot/create` | Capture warmed Axon runtime state | Snapshot pool manager |
| `PUT /snapshot/load` | Resume from pool (<8ms) instead of cold boot | axon-vm launch --fast |

Endpoints **not used**: `/balloon`, `/cpu-config`, `/entropy`, `/hotplug/memory`,
`/serial` (disabled for latency). Network interface only provisioned when
`effect_union` contains `Net` or `AI` — a pure Axon binary gets no TAP device.

---

## MMDS Payload Schema

The MMDS data store (link-local HTTP at 169.254.169.254) is the boot-time channel
from launcher to guest runtime. No vsock needed before seccomp is applied.

```json
{
  "latest": {
    "axon": {
      "schema": "axon-vm-mmds/1",
      "principal": "alice",
      "allowed_effects": ["AI", "Net"],
      "budget_tokens": 10000,
      "budget_cpu_ms": 5000,
      "source_hash": "a3f9c2...",
      "seccomp_bpf_b64": "<base64-encoded BPF bytecode>",
      "run_id": "b3e7f1a2-..."
    }
  }
}
```

The guest reads this via MMDS V2 (requires session token PUT first, then GET).
The `seccomp_bpf_b64` field was meant to carry a compiler-derived BPF program. *Historical:*
since guest-fs GF-5 no launcher derives one from the manifest; the field stays in the schema and
`axon-guest-init` applies it only if an operator supplies one. `source_hash` lets the guest
verify it's running the binary that was approved and manifested.

---

## Guest-Side Runtime Changes (axon-rt)

The Axon runtime needs one new startup sequence before entering user code:

> *Historical (pre-GF-5) sketch.* No `axon-rt` init applies a manifest-derived filter; the
> `seccomp_bpf_b64` it decodes is never produced by `axon-vm` (see the note at the top).

```rust
// In axon-rt/src/init.rs (new file)
fn apply_vm_policy() -> Result<(), Error> {
    // MMDS V2: get token
    let token = http_get("http://169.254.169.254/latest/api/token",
                         &[("X-metadata-token-ttl-seconds", "60")])?;
    // Read axon policy
    let payload: MmdsAxonPayload = http_get_json(
        "http://169.254.169.254/latest/axon",
        &[("X-metadata-token", &token)])?;

    // Apply seccomp — point of no return for syscall policy
    let bpf = base64_decode(&payload.seccomp_bpf_b64)?;
    prctl_no_new_privs()?;
    prctl_set_seccomp_filter(&bpf)?;

    // Record principal for provenance log
    set_current_principal(&payload.principal);
    Ok(())
}
```

If MMDS is unreachable (not running under axon-vm), skip silently — the runtime
works identically in all existing modes. The MMDS check is a soft init, not a
hard gate.

---

## vsock host_await Protocol

Currently `host_await` uses a worker thread for the native substrate. Under
axon-vm, a vsock channel replaces the thread — the Axon program suspends to
the host, the host performs the I/O (e.g., an LLM call), and resumes the guest:

```
Guest (Axon)                    Host (axon-vm)
─────────────────               ──────────────────
host_await(request)
  → serialize request
  → write to AF_VSOCK(cid=2, port=7000)
  → suspend (block on read)    recv request on uds vsock socket
                                perform I/O (ai_complete, etc.)
                                write response to vsock
  recv response ←──────────────
  → deserialize
  → resume with value
```

This replaces the WASM Asyncify path for VM-hosted Axon with a direct bidirectional
channel. Latency: ~300 µs vsock round-trip (scheduling overhead dominates, not data
copy). For AI calls where the LLM response takes 500ms–5s, this overhead is negligible.

---

## Fast-Restart Snapshot Pool

Cold boot (Firecracker + guest Linux init): 125–330 ms.
Snapshot restore to warmed Axon runtime: 3–8 ms.

The snapshot pool pre-warms N Axon runtime instances and snapshots them just before
user code runs. On `axon-vm launch`, the launcher restores from the pool instead of
cold-booting:

```
Pool manager (background)
  loop:
    cold-boot VM with axon-guest-init
    wait for "ready" signal on vsock (init applied seccomp, runtime loaded)
    PATCH /vm {"state":"Paused"}
    PUT /snapshot/create → pool/snapshot-{n}.{state,mem}
    PUT /vm.delete
    pool_size += 1

axon-vm launch goal.ax
  if pool_size > 0:
    PUT /snapshot/load {snapshot_path, mem_backend, resume_vm:true,
                        network_overrides: [new TAP],
                        vsock_override: new UDS path}
    PUT /mmds (goal-specific: principal, budget, seccomp, source_hash)
    resume → <8ms to executing user code
  else:
    cold boot → ~200ms
```

Security note (from VMM research): resuming the same snapshot image for multiple
VMs is safe for capability isolation purposes but repeats the entropy pool — the
guest must re-seed from `/dev/hwrng` (virtio-rng) after restore. The axon-guest-init
does this before reading MMDS.

---

## Gaps and Build Order

### Done
- [x] `axon build --emit-manifest` → `.axmeta` with effect_union + contained specs + capabilities (the `syscall_hint` list was removed, guest-fs GF-5)

### Required before axon-vm works

**Gap 1 — BPF bytecode emission (4 hours)** — *superseded (guest-fs GF-5): no manifest-derived BPF exists; see the note at the top.*
The `.axmeta` has a `syscall_hint` string list. The launcher needs to compile that
into actual BPF bytecode using the `seccompiler` crate. Either:
- `axon build --emit-seccomp` adds a `.axbpf` sidecar alongside `.axmeta`, or
- The launcher does the `seccompiler` compilation inline at launch time
Inline compilation is simpler (no new Axon CLI flag); the seccompiler step is <1ms.

**Gap 2 — axon-guest-init binary (4 hours)**
A minimal static Rust PID-1 binary that:
1. Re-seeds entropy from virtio-rng
2. GETs MMDS (169.254.169.254) with token
3. Applies seccomp BPF from MMDS payload (*historical*: none is manifest-derived since guest-fs GF-5; under `"fs": "none"` it installs its compiled-in filter instead)
4. Sets principal for provenance
5. Execs the Axon binary (or interpreter with the .ax file)
6. Handles SIGTERM/SIGCHLD (PID-1 requirements)
~150 lines of Rust. Lives in `crates/axon-guest-init/`.

**Gap 3 — Minimal guest image build (2 hours)**
Script to produce:
- `axon-guest-vmlinux` — stripped Linux 6.x with only virtio-{mmio,blk,net,vsock,rng}
- `axon-rootfs.ext4` — 8 MB ext4 containing only `/sbin/init` (axon-guest-init)
  and `/usr/bin/axon` (interpreter binary, musl-static)
Or: initramfs approach (no ext4 file needed; kernel+initramfs = one artifact).
Lives in `scripts/build-guest-image.sh`.

**Gap 4 — axon-vm launcher binary (1 day)**
`crates/axon-vm/` — a Rust binary with subcommands:
- `axon-vm launch <binary.ax> [--manifest binary.axmeta] [--principal alice] [--budget 10000]`
- `axon-vm pool start|stop|status`
- `axon-vm snapshot create|restore`
Calls Firecracker REST API via Unix socket (`reqwest` over `--unix-socket`, or
raw HTTP/1.1 write to the UDS). Invokes `jailer` for Principal isolation.
~600 lines of Rust.

**Gap 5 — Guest-side MMDS client in axon-rt (2 hours)**
`apply_vm_policy()` as sketched above. Soft-init: no-op when MMDS unreachable.
Adds a new optional dep on a minimal HTTP client (or raw TCP to 169.254.169.254:80).

**Gap 6 — vsock host_await substrate (4 hours)**
New branch in the `host_await` dispatch in `interp.rs`:
`if cfg!(target_env = "axon-vm") { vsock_await(...) } else { thread_await(...) }`
Or detect at runtime via an env var `AXON_VM_VSOCK_PORT`.

**Gap 7 — uid/Principal registry (2 hours)**
Simple file-based registry at `/etc/axon/principals.toml` mapping principal names
to reserved uid:gid pairs. `axon-vm launch --principal alice` looks up uid=1001
and passes it to jailer. The registry is immutable after provisioning (edit requires
root — this is intentional, it's a security boundary).

### Total to first working axon-vm launch
~2 days of ASI coding. The snapshot pool (Gap 4 pool subcommand) adds another day.

---

## Security Properties Summary

| Threat | Mitigation layer | Independent? |
|---|---|---|
| Axon code calls disallowed syscall | Linux profile only, under `"fs": "none"`: `axon-guest-init`'s compiled-in filter (path syscalls EACCES, non-x86_64 ABI killed). There is no effect-derived allowlist (the annotation-derived BPF was deleted in guest-fs GF-5) | Yes, for the path syscalls it covers; nothing else is filtered |
| Axon code contacts disallowed host | interpreter Net grant (exit 8); the protected Linux profile has no NIC | The missing NIC is; the grant is the language layer |
| Guest escapes to host filesystem | KVM hardware boundary | Yes |
| Principal exceeds CPU budget | host cgroup (jailer `--cgroup cpu.max`) | Yes — guest can't lie to cgroup |
| Principal exceeds memory budget | host cgroup `memory.max` hard cap | Yes |
| One VM affects another VM | unique uid:gid per VM, separate netns | Yes |
| Compiler checker bug in @[contained] | The interpreter's runtime grant, the `fs:none` filter (path syscalls only), KVM + cgroup | Partly: no OS layer covers non-path syscalls |

Three security boundaries: language (compiler + interpreter grants), OS (only the
`fs:none` filter above; no effect-derived seccomp), hardware (KVM + host cgroup).
The OS layer covers path syscalls only, so it is not a full third layer.

---

## Quick Reference — What axon-vm launch Does

```bash
axon build goal.ax --emit-manifest   # produces ./goal + ./goal.axmeta
axon-vm launch goal.ax \
  --manifest goal.axmeta \
  --principal alice \
  --budget-tokens 10000 \
  --budget-mem-mib 256 \
  --budget-cpu-pct 25
```

Internally:
1. Read `goal.axmeta` → effect_union, capabilities, contained specs, risk
2. (No manifest-derived seccomp: guest-fs GF-5 deleted the `syscall_hint` → BPF step.)
3. Allocate uid for principal "alice", set up TAP + netns (if Net/AI in effects)
4. POST Firecracker API calls (machine-config, boot-source, drives, vsock, mmds)
5. Populate MMDS with principal/budget payload (no manifest-derived BPF since guest-fs GF-5)
6. Exec jailer → Firecracker boots → guest init applies its policy → user code runs
7. Stream stdout/stderr back; collect exit code; tear down VM resources

Cold start: ~200 ms. From snapshot pool: ~8 ms.

See `TARGETS.md` for target comparison and `ENVIRONMENTS.md` for platform setup.
