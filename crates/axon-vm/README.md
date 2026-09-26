# axon-vm

The confidential microVM substrate: a Firecracker launcher, BPF policy
generation, attestation, a principal registry and cross-VM quorum. It ships as
the `axon-vm` CLI and, since v0.22 (B262), as a library target.

## One admission, two entry points (D-019)

| entry | where | what runs before launch |
|---|---|---|
| `axon_vm::admit` (library) | `src/admit.rs` | null-grant refusal; an env override may only narrow the manifest grant; kernel attestation against a pinned digest (no TOFU; `--no-attest` / `KernelPin::DevBypass` is the only bypass); extended-TCB compare against a pin (no TOFU) |
| `axon_vm::run_in_firecracker` (library) | `src/firecracker.rs` | requires a `LaunchSpec` holding an `AdmittedLaunch` — constructible ONLY by `admit` (private fields; a `compile_fail` doctest pins this) — and a `FirecrackerBin` resolved to an ABSOLUTE path (no bare/relative `firecracker`, no `which`) |
| `axon-vm run` (CLI) | `src/main.rs` `cmd_run` | calls `admit` (same exit codes and messages, pinned by `tests/cli_parity.rs`), then the CLI-only quorum (R33) and chain (R34) gates |

The gates were moved into `admit.rs`, not duplicated. The admitted grant is
baked into the `MmdsPayload` the admission carries and the attested kernel is
the kernel the launch boots, so a caller cannot pair an admission with a
different grant or image. `MmdsPayload.allowed_effects` is a `Vec`: a `null`
grant cannot be represented, and an EMPTY list is deny-all (the repo-wide `""`
= deny-all reading).

Still CLI-only: the quorum (R33) and attestation-chain (R34) gates. The kernel
is measured at admission and opened again by Firecracker at boot, so a swap
between the two is not detected (measure-then-use window).

## What the library's profile is

`BACKEND_PROFILE` (`axon-metal-fc-nojailer`) describes a jailer-less
Firecracker process that `axon-vm` spawns directly. It has no chroot, no
dedicated uid/gid and no cgroup, and memory is capped only by the balloon. The
guest is the **custom Axon guest kernel, NOT Linux**. `qualified_protected` is
`false`, and `satisfies_protected_linux_microvm()` returns false. The Linux
profile (`profiles/linux-microvm/`, B263) does **not** go through this crate:
it uses `scripts/fc_linux_profile.sh` and the jailer.

## Open defects

* **ACF-G22 — fixed.** After the Firecracker child is spawned, a `LaunchGuard`
  owns it and both socket paths: any early return kills it, REAPS it and
  removes the API and vsock sockets; the success path disarms the guard only
  after its own wait. The vsock UDS is `<api socket>.vsock`, not
  `/tmp/axon-vm-vsock-<pid>.sock`, so two launches in one process no longer
  collide, and it is bound synchronously before any later `?`. The socket
  wait is `LaunchSpec::socket_timeout` (the CLI still reads
  `AXON_VM_SOCKET_TIMEOUT_SECS`). Pinned by `tests/launch_cleanup.rs` with a
  stub firecracker script — no KVM needed. Still open: the vsock relay thread
  blocks in `accept` for the life of the process after its launch ends.
* **The live tests can still skip.** `tests/lib_launch.rs` and
  `tests/cli_parity.rs` skip when firecracker, `/dev/kvm` or the kernel is
  absent. Since 4c908c3 the skip is recorded to `target/harness-skips.log`,
  listed at the end of every `gate.sh` run, and fatal under
  `AXON_HARNESS_STRICT=1`; without that variable cargo still reports the test
  as ok, so a green run is not proof the guest booted.
* **The positive control relies on the vacuous allow path.** `lib_launch.rs`
  uses IO+FS → `Exited(0)` as its positive guest-verdict control, and that
  depends on the custom kernel's allow path, which is still open finding F161.
* **CLI goldens cover refusals only.** `tests/cli_parity.rs` pins help, usage
  and refusal paths captured from `4cceb89`. There is no golden for a
  successful run.

Sources: `.axon-v022/analysis/F_guest_vm.json` (B262) and
`E_hardening.json` (H07). Both are operator-side and untracked, **not in the
repository**.
