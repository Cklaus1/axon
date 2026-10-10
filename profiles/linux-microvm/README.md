# Linux microVM profile (`linux-microvm-protected`, B263 / ACF-T06): enclosure-only, NOT qualified as protected

> **Status as of 2026-09-25 (`v022/stage3-l5b-launcher-policy`). Read this before
> you rely on the name.** The profile id says "protected". What exists is
> weaker than that:
>
> * **The guest enforces a boot policy (Stage 3, D5 + S3-5).** `guest-init.sh`
>   execs the workload under a static `axon-guest-init`, which reads
>   `axon.policy=<base64 axon-vm-mmds/1 JSON>` from `/proc/cmdline` and
>   REFUSES (exit 1, program never runs) when the policy is absent, `{}`,
>   labels-only, malformed, has a duplicate key or the cmdline may be
>   truncated; otherwise it exports the effect ceiling (`AXON_ALLOWED_EFFECTS`,
>   SandboxViolation exit 8) and token cap. `fc_linux_profile.sh --policy`
>   validates the policy, embeds it, records `policy_sha256`, and marks a run
>   whose serial `B263-POLICY sha=` differs INADMISSIBLE (exit 25). Path/host
>   scope is NOT enforced: a request for it is refused (x2, "unsupported axis
>   refuses", operator default D8). Fabric's own x1/x2 refusals (`backend.rs`)
>   are owned by lane L5c and are not changed here.
> * **Qualification after S3-2/S3-5** on a **WSL2 host with nested KVM**
>   (decision D2; the L0 Hyper-V hypervisor is outside the boundary): x1a–x1e,
>   x2 and g4 are asserted rows; x3 (the L0 boundary) and x4 (the trusted
>   evidence issuer) stay BLOCKED, so the result is still `PASS_WITH_BLOCKED`
>   at best. The record for this commit range is in the operator's untracked
>   `.axon-v022/evidence/b263/`.
> * **The record is unsigned.** It was produced by an unauthenticated local
>   root shell, and no issuer key exists (x4). `axon-fabric`'s
>   `qualification()` used to enable dispatch on `FAIL == 0` plus a manifest
>   match, ignoring BLOCKED (D-020, FG-042). It now refuses: it requires a
>   detached Ed25519 signature from a key in `trusted_issuers/` (empty — the
>   operator holds the signing key, D6), no unwaived BLOCKED assertion,
>   freshness, engine digests and clean trees. **So Fabric refuses this
>   profile until a signed re-qualification exists (S3-6).**
> * **The artifacts are rebuilt from a clean tree.** `manifest.json` records
>   `source.axon_git_rev_at_build` and `source.axon_tree_dirty_at_build`
>   (`false` for the stage3-l5b rebuild); the earlier image was built dirty at
>   `4cceb892`.
> * **It deviates with `acpi=off`.** See "Known deviation" below. The deviation
>   is recorded here and in `kernel-overlay.config`, not in `manifest.json`.
> * **Firecracker and jailer are digest-checked at launch (S3-2).** The
>   manifest pins `engine.firecracker_sha256` / `engine.jailer_sha256`; a
>   mismatch refuses before anything is acquired (exit 22, g4).
> * **Nothing automated runs it.** `gate.sh` and CI invoke neither
>   `fc_linux_profile.sh` nor `b263_qualify.sh`, since both need root. The
>   `axon-fabric` tests use a stand-in launcher.
>   *(Superseded 2026-09-24, 54f41c3: `gate.sh --strict` now runs
>   `b263_qualify.sh`. It fails the gate on PASS_WITH_BLOCKED — which, with
>   x3/x4 recorded BLOCKED unconditionally, is the only result a fully
>   equipped host can currently produce — and records a SKIP (non-result)
>   when root, KVM or the guest artifacts are absent. CI still does not run it.)*
>
> **Do not describe or treat this profile as a qualified protected microVM.**
> Recorded as `governance/cortex-v015/DISCREPANCIES.md` D-020.
>
> The evidence files live in the operator's untracked
> `.axon-v022/evidence/b263/`. The qualifying record is `20260924T080432Z.json`.
> Two earlier runs had FAILs (`075735Z`: b2 and c4; `075923Z`: b2), and
> `080319Z` also shows 32/0/4. They are **not in the
> repository**.

A pinned Linux guest and a Firecracker **jailer** launch that together run ONE
Axon program offline. `scripts/b263_qualify.sh` measures the host and guest
boundaries.

Qualification host: WSL2, kernel 6.18, nested KVM (operator decision D2). Every
evidence record says `host=WSL2-nested`, with the caveat that **the L0
hypervisor (Hyper-V) is outside the qualified boundary**.

## Files

| Path | What |
|---|---|
| `kernel.pin` | Kernel version, URL, tarball sha256 (checked against kernel.org `sha256sums.asc`), config + overlay sha256, busybox sha256 |
| `kernel-6.1-firecracker-v1.10.1.config` | Firecracker v1.10.1 `microvm-kernel-ci-x86_64-6.1.config`, byte for byte |
| `kernel-overlay.config` | The ONE deviation (`VIRTIO_MMIO_CMDLINE_DEVICES=y`) and why it is there |
| `guest-init.sh` | `/init` (PID 1) in the rootfs: the guest side of the protocol below |
| `manifest.json` | sha256 + size of every built artifact (a copy of `dist/guest-linux/manifest.json`) |
| `fixtures/` | Programs the qualification harness runs in the guest |

Build (the outputs are gitignored and go under `dist/guest-linux/`):

```bash
AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh      # [--kernel-only|--rootfs-only]
```

Every pinned input is digest-checked **before** it is used, so a mismatch fails
the build. On this host the build is reproducible: repeat builds gave identical
`vmlinux` and `rootfs.sqfs` digests. The squashfs is built with
`-all-time 0 -mkfs-time 0 -all-root`, and the kernel with fixed `KBUILD_*`
identity values.

| Artifact | sha256 |
|---|---|
| `vmlinux` (Linux 6.1.188, ELF) | see `manifest.json` → `artifacts.vmlinux` |
| `rootfs.sqfs` (busybox + axon + /init, read-only) | `artifacts["rootfs.sqfs"]` |
| `axon` (static-pie musl, `--no-default-features`, `--locked`) | `artifacts.axon` |

**Known deviation, root cause not established:** a kernel built here from the
unmodified config fails ACPI table load, and then virtio-blk probe fails, so no
root device is found. Firecracker's own prebuilt 6.1.102 does boot on this
host. The profile therefore boots with `acpi=off` and `virtio_mmio` command-line
device discovery. Details are in `kernel-overlay.config`.

## Launch interface — `scripts/fc_linux_profile.sh`

The contract below is what Fabric drives (`axon-fabric` `backend.rs`). `axon-vm` does not drive it: the `axon-vm` library is the jailer-less custom-kernel path. It must run as root,
because the jailer needs root to drop to the profile uid.

```
fc_linux_profile.sh --program PROG.ax --out DIR [options]
fc_linux_profile.sh --reap ID               # destroy what a dead launcher left behind
fc_linux_profile.sh --verify-result DIR     # re-check the output binding of a finished run
```

### Inputs

| Flag | Default | Meaning |
|---|---|---|
| `--program FILE` | required | Becomes `/work/job/program.ax`, run by `axon run` |
| `--policy FILE` | required | Capability policy, schema `axon-vm-mmds/1`. Validated before anything is acquired (strict JSON: no duplicate keys, no unknown keys, grantable effect names only, must constrain something via `allowed_effects` / `budget_tokens` / `seccomp_bpf_b64`). Path/host scope keys (`fs_write`, `net_hosts`, …) are REFUSED as an unsupported axis (ACF-G26, operator default D8) — never passed through, since the guest parser ignores unknown keys. Embedded as exactly one `axon.policy=<standard padded base64 of the file's exact bytes>` cmdline word; the whole cmdline must be ≤ 2046 bytes or the launch is refused. `result.json` records `policy_sha256` |
| `--out DIR` | required | Must be new or empty; receives every output |
| `--put SRC:DEST` | — | Extra input file, placed at `/work/DEST` (repeatable) |
| `--vcpus N` / `--mem-mib N` | 1 / 256 | Guest machine size |
| `--cg-mem-max BYTES` | mem + 128 MiB | Host cgroup v2 `memory.max` for the VMM (`memory.swap.max=0` always) |
| `--cg-pids-max N` | 16 | Host cgroup `pids.max` for the VMM |
| `--cg-cpu-max "Q P"` | `100000 100000` | Host cgroup `cpu.max` |
| `--workspace-mib N` | 64 | Size of the workspace drive, which is also the output-size cap (plus `RLIMIT_FSIZE` on the VMM) |
| `--timeout-s N` | 60 | Wall clock; when it expires the VMM's cgroup is killed with `cgroup.kill` |
| `--id ID` | random `b263-…` | Jail id. It also names the cgroup, the chroot and the netns |
| `--manifest FILE` / `--artifacts-dir DIR` | `dist/guest-linux/…` | Where the pins and the artifacts are read from |
| `--fc-bin FILE` / `--jailer-bin FILE` | `/usr/local/bin/{firecracker,jailer}` | Engine binaries. They must still match `manifest.json` `engine.firecracker_sha256` / `engine.jailer_sha256`; the flags exist so qualification (g4) can prove a swapped engine is refused |
| env `FC_PROFILE_TEST_POLICY_WORD` / `FC_PROFILE_TEST_EMBED_POLICY` | — | Qualification hooks (x1a–x1d): replace the policy word verbatim (the guest's own refusal becomes observable; no policy sha is recorded, so the run is never admissible), or embed a file other than the one recorded. Every hooked run lists them in `result.json` `test_hooks` |
| env `FC_PROFILE_INJECT_FAIL` | — | `after-chroot` \| `after-netns` \| `after-launch`: fault injection for cleanup tests |

### Fixed by the profile, not configurable

- Jailer with uid/gid `axonb263` (a system user). Chroot is `/srv/axon-b263/firecracker/<id>/root`, cgroup v2 is `/sys/fs/cgroup/axon-b263/<id>`, and `--new-pid-ns` is not used.
- Firecracker runs with `--no-api` (no API socket) and its default seccomp filter. The host observed `Seccomp: 2` on every thread and `NoNewPrivs: 1`.
- The VMM is started with `env -i`, so no variable from the launching shell reaches it (no provider keys, no `AXON_*`). No MMDS is configured.
- `network-interfaces: []`. The VMM also joins an **empty network namespace** (lo only).
- Drives: `vda` is `rootfs.sqfs` (`is_read_only`, file mode 0444, owned by root). `vdb` is `workspace.img` (ext4, rw, owned by the jail uid).
- Guest command line: `console=ttyS0 reboot=k panic=1 pci=off acpi=off root=/dev/vda rootfstype=squashfs ro init=/init`.
- Inside the guest, the workload runs under `env -i` in the cgroup `/job`, with `pids.max=32` and `memory.max` = guest RAM − 48 MiB, exec'd by `/usr/bin/axon-guest-init` (static musl, default features, sha256 pinned in `manifest.json` as `artifacts.axon-guest-init`), which applies the cmdline policy first.

The kernel and rootfs are checked against the manifest twice: once in `--artifacts-dir` before anything is acquired, and again as the copies inside the chroot.
Firecracker and jailer are checked against the manifest's `engine` pins before anything is acquired, then exec'd from a private root-only copy whose digest is re-checked (no check-then-exec window on the original paths). After the run the jailer's own chroot copy of firecracker and, when observed, `/proc/<vmm>/exe` are re-hashed; a mismatch is `engine-unbound` (exit 26).

### Outputs (in `DIR`)

| File | Content |
|---|---|
| `result.json` | `axon-linux-microvm-result/1`. It holds: `status`, `exit_code`, `workload_exit`, `outputs{path: sha256, bytes}`, `output_bound`, the serial digests, `workspace_in/out_sha256`, `rootfs_after_run_sha256`, the limits, `host_observed` (uid/gid/seccomp/threads/root/cgroup/netns of the live VMM), `cgroup_final` (memory/pids/cpu counters), and `cleanup{acquired, left_behind, complete}` |
| `out/` | The guest's `/work/out`, extracted with **debugfs** (the host never mounts the guest filesystem): `stdout`, `stderr`, `exit`, `guest.json`, plus any files the program wrote |
| `workspace.img` | The returned drive itself |
| `serial.log` | The guest console, captured independently of the drive |
| `launch.json` | Written at launch time: `id`, `vmm_pid`, `cgroup`, `chroot`, `netns`. A supervisor uses it to observe the VMM, or to `--reap` it |

### Exit status

| Code | Status | Meaning |
|---|---|---|
| 0 | `ok` | Workload exit 0, and the output is bound |
| 10 | `workload-failed` | The workload ran and exited nonzero (for example, the guest OOM-killed it: 137) |
| 20 | `timeout` | Wall clock expired and the VMM was killed |
| 21 | `vmm-died` / `injected-failure:*` / `interrupted` | The VMM ended without completing, e.g. crashed, SIGKILLed, or OOM-killed on the host. No result is admitted |
| 22 | `launch-refused` | Refused: an artifact or engine digest does not match, the policy is absent/invalid/over-long, bad input, or not root. When refused in the pin block, `acquired` is `[]` |
| 23 | `output-unbound` | The drive's `/out/stdout` does not match the digest the guest printed on serial. The result is NOT admissible |
| 24 | `cleanup-incomplete` | `cleanup.left_behind` lists the leftovers. Treat it as a live resource |
| 25 | `policy-unbound` | The guest's first `B263-POLICY` serial line is not `sha=<policy_sha256>` — it reported another policy, or none. NOT admissible |
| 26 | `engine-unbound` | The firecracker the jailer placed in the chroot (or the running image) is not the pinned one. NOT admissible |

`result.json` `admissible` is `true` only for `ok` and `workload-failed`.

### Guest protocol (serial + workspace drive)

`guest-init.sh` prints `B263-BOOT`, `B263-VERSION`, then `B263-LOADED axon=<sha> program=<sha> init=<sha>`, `B263-POLICY sha=<sha256 of the decoded policy JSON>` (or `absent` / `undecodable` / `ambiguous words=N`), `B263-START`, `B263-OUT stdout=<sha> exit=<n>` and `B263-DONE`, and then calls `reboot -f`. A result is admissible only when the digest printed on serial, the digest re-extracted from the drive, and `result.json` all agree, AND the first `B263-POLICY sha=` line equals `result.json` `policy_sha256` (the first line, because it is printed before the workload starts). `--verify-result` re-derives both checks independently.

### Cleanup semantics

Cleanup runs on every exit path, including INT and TERM. It kills the cgroup, removes the cgroup directory, the chroot, any mounts under it, and the netns, then **re-observes** the host and records whatever is still there. Cancel is not cleanup: if the launcher itself is SIGKILLed, the VMM keeps running (measured). The supervisor must then call `--reap ID`, which is idempotent and exits 0 only if the host is clean afterwards.

## Qualification

```bash
sudo scripts/b263_qualify.sh [--evidence-dir DIR] [--keep]
```

Evidence is written to `/home/cklaus/projects/aicoding/axon/.axon-v022/evidence/b263/<UTC>.json` with schema `axon-b263-evidence/1`.

ACF-G25 (guest policy channel) is asserted by x1a–x1e: absent / `{}` / malformed / over-long policies are refused by the launcher before anything is acquired AND, with the launcher bypassed by a test hook, by `axon-guest-init` in the guest (the workload never runs: empty stdout, no file it writes); a serial policy digest that differs from the one the host recorded makes the run inadmissible; an `IO`-only ceiling turns a spawn into a SandboxViolation (exit 8) while `IO,Exec` runs it. ACF-G26 is recorded as **unsupported axis refuses** (x2, operator default D8, provisional): a path- or host-scoped policy is refused. Path/host projection is NOT implemented and x2 is not path enforcement.

**BLOCKED, not substituted:**

- The L0 hypervisor boundary (WSL2).
- A trusted evidence issuer: the evidence is unsigned.
