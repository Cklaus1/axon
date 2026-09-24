# axon-vm

The confidential microVM substrate: a Firecracker launcher, BPF policy
generation, attestation, a principal registry and cross-VM quorum. It ships as
the `axon-vm` CLI and, since v0.22 (B262), as a library target.

## Two entry points that do not enforce the same things

| entry | where | what runs before launch |
|---|---|---|
| `axon-vm run` (CLI) | `src/main.rs` `cmd_run` | null-grant refusal; an env override may only narrow the grant; kernel attestation against a pinned baseline (no TOFU; `AXON_CI_NO_KVM` is not a bypass); extended-TCB compare; quorum |
| `axon_vm::run_in_firecracker` (library) | `src/lib.rs`, `src/firecracker.rs` | **none of the above.** Only the launch path was moved |

The library entry point **does not enforce `cmd_run`'s grant and attestation
gates**. `MmdsPayload.allowed_effects` is still an `Option`. The guest kernel
fails closed on a null policy, so the impact today is "may boot an unattested
kernel", not "runs without policy". The gap is **latent**: the only callers
are `src/main.rs` and `tests/lib_launch.rs`. `axon-fabric` imports only
`BACKEND_PROFILE`, and its `axon-metal-fc-nojailer` backend is eligible for
nothing. Any future library caller would inherit none of the gates.

Recorded as `governance/cortex-v015/DISCREPANCIES.md` D-019. Open, Stage 3.

## What the library's profile is

`BACKEND_PROFILE` (`axon-metal-fc-nojailer`) describes a jailer-less
Firecracker process that `axon-vm` spawns directly. It has no chroot, no
dedicated uid/gid and no cgroup, and memory is capped only by the balloon. The
guest is the **custom Axon guest kernel, NOT Linux**. `qualified_protected` is
`false`, and `satisfies_protected_linux_microvm()` returns false. The Linux
profile (`profiles/linux-microvm/`, B263) does **not** go through this crate:
it uses `scripts/fc_linux_profile.sh` and the jailer.

## Open defects

* **The library skips the pre-launch gates** (above, D-019).
* **ACF-G22: child and sockets leak on post-spawn errors.** In
  `src/firecracker.rs`, every `?` after the Firecracker child is spawned
  returns without killing the child or removing the API and vsock sockets.
  Cleanup happens only on the success path. This code is pre-existing and was
  moved verbatim. The vsock UDS name is pid-keyed
  (`/tmp/axon-vm-vsock-<pid>.sock`), so two concurrent launches in one process
  collide.
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
