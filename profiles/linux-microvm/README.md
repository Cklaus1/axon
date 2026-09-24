# Protected Linux microVM profile (`linux-microvm-protected`, B263 / ACF-T06)

A pinned Linux guest plus a Firecracker **jailer** launch that runs ONE Axon
program offline. Its host and guest boundaries are measured by
`scripts/b263_qualify.sh`.

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

The contract below is what axon-vm / Fabric should drive. It must run as root,
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
| env `FC_PROFILE_INJECT_FAIL` | — | `after-chroot` \| `after-netns` \| `after-launch`: fault injection for cleanup tests |

### Fixed by the profile, not configurable

- Jailer with uid/gid `axonb263` (a system user). Chroot is `/srv/axon-b263/firecracker/<id>/root`, cgroup v2 is `/sys/fs/cgroup/axon-b263/<id>`, and `--new-pid-ns` is not used.
- Firecracker runs with `--no-api` (no API socket) and its default seccomp filter. The host observed `Seccomp: 2` on every thread and `NoNewPrivs: 1`.
- The VMM is started with `env -i`, so no variable from the launching shell reaches it (no provider keys, no `AXON_*`). No MMDS is configured.
- `network-interfaces: []`. The VMM also joins an **empty network namespace** (lo only).
- Drives: `vda` is `rootfs.sqfs` (`is_read_only`, file mode 0444, owned by root). `vdb` is `workspace.img` (ext4, rw, owned by the jail uid).
- Guest command line: `console=ttyS0 reboot=k panic=1 pci=off acpi=off root=/dev/vda rootfstype=squashfs ro init=/init`.
- Inside the guest, the workload runs under `env -i` in the cgroup `/job`, with `pids.max=32` and `memory.max` = guest RAM − 48 MiB.

The kernel and rootfs are checked against the manifest twice: once in `--artifacts-dir` before anything is acquired, and again as the copies inside the chroot.

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
| 22 | `launch-refused` | Refused before anything was acquired: an artifact digest does not match, bad input, or not root |
| 23 | `output-unbound` | The drive's `/out/stdout` does not match the digest the guest printed on serial. The result is NOT admissible |
| 24 | `cleanup-incomplete` | `cleanup.left_behind` lists the leftovers. Treat it as a live resource |

### Guest protocol (serial + workspace drive)

`guest-init.sh` prints `B263-BOOT`, `B263-VERSION`, then `B263-LOADED axon=<sha> program=<sha>`, `B263-START`, `B263-OUT stdout=<sha> exit=<n>` and `B263-DONE`, and then calls `reboot -f`. A result is admissible only when the digest printed on serial, the digest re-extracted from the drive, and `result.json` all agree. `--verify-result` re-derives this check independently.

### Cleanup semantics

Cleanup runs on every exit path, including INT and TERM. It kills the cgroup, removes the cgroup directory, the chroot, any mounts under it, and the netns, then **re-observes** the host and records whatever is still there. Cancel is not cleanup: if the launcher itself is SIGKILLed, the VMM keeps running (measured). The supervisor must then call `--reap ID`, which is idempotent and exits 0 only if the host is clean afterwards.

## Qualification

```bash
sudo scripts/b263_qualify.sh [--evidence-dir DIR] [--keep]
```

Evidence is written to `/home/cklaus/projects/aicoding/axon/.axon-v022/evidence/b263/<UTC>.json` with schema `axon-b263-evidence/1`.

**BLOCKED, not substituted:**

- ACF-G25 (guest policy channel) and ACF-G26 (scope preservation): this profile has no boot-policy protocol.
- The L0 hypervisor boundary (WSL2).
- A trusted evidence issuer: the evidence is unsigned.
