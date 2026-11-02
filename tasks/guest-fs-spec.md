# guest-fs - DRAFT spec (CE-9 / NH-C8 = C)

**Status: DRAFT (after Step 1 review). Authorized for build by the human 2026-11-02 (NH-F4 A, NH-F5 A; §9).**

Verdict: C is FEASIBLE on the Linux-microvm profile, and I observed it end to end in a real Firecracker guest. C cannot be
shown on the bare-metal `axon-guest-kernel` backend, because that kernel does not run programs at all (FG-041).

Step 1 review: `AuditGF` raised GFA-1..6 and `DevloopGF` raised GFD-1..7. All 13 are accepted and applied below with
`[REVISED: <id> ...]` markers. The integrator's rulings of 2026-11-02 are applied with `[REVISED: ruling N]`. Needs-human ids
are renamed NH-G1..G4 → **NH-F1..F4**, because genviab-b2 uses NH-G*.

Author: `CE9Spike` (2026-11-02). Base: `b2/int` b3a9c56e (worktree `/home/cklaus/projects/axon-lanes/b2-ce9`, branch `b2/ce9`).
Reference binary: `/var/tmp/r51-tmp/b2-bin/axon` (built at ebbb4e02, default features).

Spike binaries: static musl `axon` and `axon-guest-init`, built from the `spike:` commits on this branch
(`CARGO_TARGET_DIR=/home/cklaus/projects/axon-lanes/.doc-build/CE9Spike-musl`, `--no-default-features`, `+crt-static`).
- Rev1 (`11e11c7f`): the guest runs in §2.
- Rev2 (`spike: rev2` commit): keeps `entry_dir` and the lock check in fd mode (the GF-1 design). It drives the §2.3 rev2
  corpus.
`[NOTE 2026-11-02, integration]` The `spike:` code commits are not merged into `main` (spike code, not authorized). Their code
changes against `b2/int` are kept as `tasks/guest-fs-spike/spike-code.diff`; branch `b2/ce9` (ae3af7ab) stays local.

Scratch is `/var/tmp/r51-tmp/CE9Spike/`. Harness scripts and raw results are committed under `tasks/guest-fs-spike/` (§10).

g01: **M4760-M4769** [REVISED: ruling 2]. §5 uses M4760-M4767 (active) and M4768 (EXEMPT); M4769 is spare.
Every line anchor is at **b3a9c56e** unless it says otherwise.

Theme: VISION Goal 2, "Code can do nothing until granted; the right to ... touch files is declared and bounded". Today every
microVM guest that runs a program holds file access:
- On the `axon-guest-kernel` backend, the grant must name `FS` because the interpreter opens its own source.
- On the Linux profile, no grant is needed: an `["IO"]` policy already lets the program read any guest file.

The spike shows that a supervisor which opens the program before the gate removes the substrate's need for `FS`. After that,
withholding file access changes nothing for a program that is eligible under the GF-3 selector.

[REVISED: GFA-1/GFA-2] The selector matters. A program that imports a module, or that uses the durable store, does need file
access. The selector refuses `fs: "none"` for it rather than letting it fail in the guest.

Reading guide:
- §2: the verdict and the before/after evidence.
- §3: the build items.
- §8: engineering decisions, including NH-F1..F3, decided per ruling 4.
- §9: the one question left for a human (NH-F4).

---

## 0. Source entries

| item | source | one line |
|---|---|---|
| GF-1 | CE-9 (`tasks/ceilings-spec.md:733-775`), CEA#9 | `axon run` cannot read its program from anything but a path (`axon run -` → rc 1) |
| GF-2 | CE-9, O034 (`tasks/opportunities.md:1264`) | `axon-guest-init` execs `axon run /work/job/program.ax`, so the workload's first file syscall is its own source |
| GF-3 | CE-9 finding 1, found in the spike | No guest policy can withhold file access from the workload. On the Linux profile, an `["IO"]` policy runs a program that reads `/init` |
| GF-4 | CE-9 finding 3, found in the spike | `axon_kernel_gate.sh` is red at base (good agent rc 2). On the bare-metal kernel the "program" never runs, and its FS demonstration is synthetic |
| GF-5 | found in the spike | If the manifest's `syscall_hint` seccomp were applied, it would kill the interpreter at `execve`. No launcher applies it |

### Out of scope (logged, not dropped)

**Bare-metal image build fails outside the main checkout.** `scripts/build-guest-image.sh` (default `axon` backend) fails with
`rust-lld: cannot find linker script /home/cklaus/projects/axon/crates/axon-guest-kernel/linker.ld`, rc 101.
- Cause: `crates/axon-guest-kernel/targets/x86_64-axon-metal.json:18` hard-codes the main checkout path, and the build uid
  cannot traverse it (`/home/cklaus/projects/axon` is 0750). [REVISED: GFA-6 anchor is under `crates/axon-guest-kernel/`]
- Why out of scope: a build-environment defect, not CE-9.
- Where it goes: a new opportunity entry at build time.

**Linux image build refused here.** `AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh` is refused on this host: rc 1
after 46 s (§10). [REVISED: GFD-2 / ruling 3]
- The kernel built (`dist/guest-linux/vmlinux`, 39M).
- The lineage check then failed. `build-guest-image.sh:181` calls `linux_profile_manifest.py --descends 31413ca7`. The
  `axon-fabric` git reader (`crates/axon-fabric/src/git_data.rs:191-192`) refuses any repository whose own config sets keys
  that "can redirect what git reports or run code". The shared repo config `/home/cklaus/projects/axon/.git/config` sets
  `blame.ignorerevsfile=.git-blame-ignore-revs`.
- The refusal is therefore correct behaviour on a host-config fault. Fixing it means removing that key from the shared
  `.git/config`, which is a human's call, not this spec's.
- Until it is fixed, no `dist/guest-linux` image (manifest + rootfs) exists on this host, and GFD-1 applies.
- Where it goes: NH-F5 (§9) and an opportunity entry.

**`bpf_allowlist` has no arch check.** It emits no `seccomp_data.arch` check (`axon-vm/src/main.rs:2100-2151`), so i386/x32
syscall numbers are matched as x86_64 ones.
- Disappears with GF-5's deletion (§8.9).

**D in the Linux kernel (Landlock).** Not needed for CE-9 (§2.4); optional hardening.
- Where it goes: NH-F4.

**Kernel-side execution on the bare-metal backend** (ELF load, VFS, ring 3).
- FG-041, Stage 3 (`governance/specs/R36-full-asi-os.md:54`).
- Where it goes: R36.

---

## 1. Priority tiers

| tier | items | meaning |
|---|---|---|
| P1 | GF-1, GF-2 | Load before gate. Adds a path; changes nothing observable for any program (parity-tested) |
| P1 | GF-3 | Withhold file access from a guest program the selector proves does not need it: the bound O034 asks for. Opt-in policy key (§8.8) |
| P2 | GF-4 | Make the kernel gate honest (§8.7) |
| P3 | GF-5 | Delete a seccomp path that is never exercised (§8.9) |

---

## 2. Spike verdict and evidence

### 2.1 Where the gate attaches

**Linux profile** (the only backend that runs Axon code in a guest).
- `/init` is `profiles/linux-microvm/guest-init.sh`. It execs `axon-guest-init /usr/bin/axon run /work/job/program.ax`
  (`guest-init.sh:183-184`).
- `axon-guest-init` forks. The child sets env (`axon-guest-init/src/main.rs:527-539`), applies the policy's
  `seccomp_bpf_b64` (`:544`, `apply_seccomp` `:675`), then execs (`:551`, `exec_process` `:731`).
- Seccomp filters are inherited across `execve`. So anything the child does **before** `apply_seccomp` is outside the gate,
  and everything the interpreter does is inside it.
- Load-before-gate fits that order with no new mechanism: open the program between fork and `apply_seccomp`, and let the fd
  survive the exec.

**Bare-metal `axon-guest-kernel`** (the `axon-vm` default, `dist/guest/vmlinuz`).
- Boot order: K2 policy (`main.rs:36-42`), then the K3 gate (`:44-46`; `enforce::init` installs an LSTAR handler covering
  every ring-3 `syscall`), then K5 `hypercall::wait_for_run` → `enforce::run_program` (`enforce.rs:338-379`).
- K5 runs no program. If the grant holds `FS`, it halts cleanly with exit 0 ("grant branch reached, no syscall issued",
  `:344-356`). Otherwise it issues one synthetic `openat` and the gate exits 8 (`:359-376`).
- Kernel-side loading would run in ring 0, so it would be pre-gate by construction. But there is no loader, no VFS and no
  ring 3 (FG-041), so C has nothing to attach to.
- `required_effect` (`enforce.rs:135-199`) classifies `close`/`fstat`/`lseek` as FS, `poll` as Net, `clone` as Exec and
  `getrandom` as Random. The fd-mode interpreter issues all of them (§2.3 trace). So even under C, an IO-only interpreter
  would trip the gate at its first `poll`.

### 2.2 Before (base behaviour, observed)

**Bare-metal backend via `axon-vm`.** The host `axon-vm` was built from this worktree; `axon-vm` itself is unchanged by the
spike.
- Image: `axon-guest-kernel`, built directly with `cargo build -p axon-guest-kernel -Z build-std ... --target
  targets/x86_64-axon-metal.json --release` (rc 0). `build-guest-image.sh` itself fails with rc 101 (Out of scope).
- Harness: `tasks/guest-fs-spike/axon_vm_run1.sh`, which runs `AXON_VM_ALLOWED_EFFECTS=<E> timeout 60 axon-vm run <prog>
  --kernel K --initrd I --expect-digest D --json`.

| # | command | rc | guest |
|---|---|---|---|
| B1 | no manifest, `AXON_VM_ALLOWED_EFFECTS=IO,FS`, `agent_task.ax` | **0** | "K5: policy GRANTS FS — grant branch reached, no syscall issued" |
| B2 | no manifest, `AXON_VM_ALLOWED_EFFECTS=IO` | **8** | "WITHHOLDS FS — issuing the real openat"; "syscall 257 blocked (FS not in policy)" |
| B3 | no manifest, unset | **2** | `no effect grant` (T48), no boot |
| B4 | manifest from `axon build --emit-manifest` (effect_union `["IO"]`), `IO,FS` | **2** | `may only narrow the manifest's effect grant ... Not in the manifest: FS` |
| B5 | manifest, unset (grant = `IO`) | **8** | synthetic openat blocked |
| B6 | manifest, `IO` | **8** | same |
| B7 | no manifest, `IO,FS`, `exit3.ax` (`println("PROGRAM-RAN"); exit(3)`; host `axon run` rc 3) | **0** | no `PROGRAM-RAN`, `exit_code 0`: the program never runs |
| B8 | `scripts/axon_kernel_gate.sh` (worktree; `dist/guest` = the image above; `HOME` in scratch) | **1** | `✗ good_agent.ax exited 2 (expected 0)` (Layer 2 wrote `agent_task.axmeta`, then B4); evil rc 8 ✓; "FAIL (4 passed, 1 failed)" |

This confirms CE-9 findings 1-3. Finding 3 is recorded as [INFERENCE] in ceilings-spec; B8 now shows it: the gate is red at
base.

**Linux profile.**
- Kernel: pinned 6.1.188, built from `profiles/linux-microvm/kernel.pin` plus the overlay with `make -j32 vmlinux`.
- Rootfs: a squashfs I assembled by hand from the spike binaries, the pinned busybox (sha matches the pin) and
  `guest-init.sh`.
- VMM: Firecracker v1.10.1 with `--no-api --config-file`, no jailer.
- Harness: `tasks/guest-fs-spike/vmrun.sh LABEL PROG MODE FILTER`.
- Policy: on the cmdline exactly as `fc_linux_profile.sh` embeds it (`axon.policy=<b64>`), with `allowed_effects ["IO"]`.

| # | mode / filter / program | workload rc | stdout |
|---|---|---|---|
| L0 | path (today) / none / `agent_task.ax` | **0** | `scores: a=70 b=5 c=70` ... |
| L6 | fd / none / `reader_init.ax` (`read_file("/init")`) | **0** | `READER-GOT 10548 bytes`: an IO-only policy lets the program read guest files |
| L8 | path / none / fixture `hello.ax` | **0** | `artifact written` |
| L1 | path / deny-path filter (EACCES) / `agent_task.ax` | **1** | `error reading /work/job/program.ax: Permission denied` |
| L9 | path / deny-path filter (KILL) / `agent_task.ax` | **159** | killed (SIGSYS) |

L1 and L9 show the substrate problem plainly: once file syscalls are withheld, today's path-mode run cannot load its own
program.

### 2.3 After (spike, observed)

**Spike code.**
- `axon run --source-fd N FILE` reads the program from the inherited fd and closes it; FILE is a label.
- `axon-guest-init --source PATH <bin> <args>` opens PATH in the forked child **before** `apply_seccomp`, without O_CLOEXEC,
  and appends `--source-fd N PATH`.

**Spike filter** (`tasks/guest-fs-spike/mkfilter.py`): 76 BPF instructions, 812 b64 bytes; the cmdline uses 1367 of 2046
bytes.
- It checks the arch and KILLs on anything other than x86_64.
- It ALLOWs the substrate: memory, threads, signals, fd-relative I/O (`read`/`write`/`close`/`fstat`/`lseek`/`fcntl`/
  `ioctl`/`poll`), `getrandom`, `clone`, `execve`.
- It returns EACCES for 28 file syscalls (`open`, `openat`, `stat`, `readlink`, `mkdir`, `unlink`, `rename`, ... `openat2`).
  [REVISED: GFA-5] Two of the 28, `getcwd` (79) and `getdents64` (217), take no path. They are listed as file syscalls, not
  path syscalls.
- It KILLs everything else.

**`execve` stays ALLOW.** [REVISED: GFA-5] Init execs the interpreter after the filter is installed, so the filter cannot deny
`execve`. The kernel opens an exec target without asking seccomp, so a workload that can exec can run any guest binary and
probe which paths exist. The exec'd binary inherits the filter, so its own opens get EACCES. Containment of exec therefore
rests on two things:
- the inherited filter;
- the GF-3 selector refusing `exec` caps and an `Exec` grant (§3 GF-3.3).

The phrase "every path syscall withheld" in rev1 overstated this: `execve`'s path is not withheld.

| # | mode / filter / program | workload rc | stdout / stderr |
|---|---|---|---|
| L2 | fd / deny-path EACCES / `agent_task.ax` | **0** | identical stdout to L0 (sha `9f4f50fa…`); "program /work/job/program.ax loaded before the gate (fd 3)", "seccomp applied (76 instructions)" |
| L3 | fd / EACCES / `reader_init.ax` | **0** | `READER-ERR Permission denied (os error 13)`: the read is refused and the program sees an `Err` |
| L5 | fd / KILL / `reader_init.ax` | **159** | killed: under KILL the interpreter dies at its own provenance `mkdir`, before the program's read |
| L4 | fd / KILL / `agent_task.ax` | **159** | same; KILL is not usable for the path class (§8.3) |
| L7 | fd / EACCES / `hello.ax` (fs:write) | **0** | `artifact FAILED Permission denied`: a false refusal if the filter were applied to a program with file capability (GF-3 selector) |
| L10 | fd / EACCES / `agent_task_evil.ax` | **2** | E1001 ×3 (check refuses, as on the host) |
| L11 | fd + `AXON_ALLOWED_FS_READ=` `AXON_ALLOWED_FS_WRITE=` / EACCES / `agent_task.ax` | **0** | identical stdout to L0 |
| L12 | same / `reader_init.ax` | **8** | `axon: sandbox violation: builtin read_file is not permitted to read path /init: the active sandbox restricts it to []` |
| L13 | same / `hello.ax` | **8** | `write_file ... /work/out/artifact.txt` refused (a false refusal if misapplied, hence the selector) |
| L0b, L2b | path/none and fd/EACCES on the rebuilt rootfs | **0, 0** | stdout sha `9f4f50fa…` both |

**L11/L12 are the layered result.**
- The honest IO-only program runs with no file syscalls and no FS grant.
- A file read by the program is refused by the interpreter's existing ambient path scope: exit 8, stated on stderr, before
  the read reaches the kernel filter.
- The seccomp EACCES is a backstop for anything the interpreter does not classify.
- L10-L13 were re-run on 2026-11-02 with identical results (`guest_matrix_2.txt`).

**The interpreter's own syscalls in fd mode** were traced on the host (`strace -f`, musl static, `AXON_ALLOWED_EFFECTS=IO`):
- `poll` on fds 0-2 at startup;
- 2 × `clone(CLONE_THREAD…)`;
- `fstat`/`lseek`/`close` on the source fd;
- `mkdir` + `open` of `$XDG_CACHE_HOME/axon/provenance.jsonl`;
- `readlink("/proc/self/exe")` (module search dirs);
- `ioctl(2, TIOCGWINSZ)`;
- `getrandom`.

Under EACCES, the provenance append and the binary-relative lib dir silently drop out. In the guest that log lives in tmpfs
`/tmp/cache` (`guest-init.sh:46`, `:183`) and is never exported, so the guest loses no audit it has today.

**Corpus differential, rev1 (superseded).** [REVISED: GFA-2]
- Rev1 claimed "538 eligible, 534 same, 0 filter-attributable differences". It compared two `--source-fd` runs of the rev1
  spike binary. That binary drops `entry_dir` and the lock check in fd mode, so neither leg measured module search or the
  lock check.
- Neither leg set the empty path scopes, so the layered L11 configuration was not covered either.
- Its "534 same" counted programs that failed identically on both legs: 254 non-zero, mostly `no main function`.
- The one eligible program that imports a module (`examples/modular/agent.ax`) "matched" by failing both times.
- Rev1 data stays in `corpus_results.tsv` for the record.

**Corpus differential, rev2** (`tasks/guest-fs-spike/corpus_diff2.sh`, results `corpus_results2.tsv`; 698 tracked `.ax` outside
`tasks/`/`docs/`; 44 s at `-P 16`). [REVISED: GFA-2 / ruling 2]
- Binary: the rev2 spike build, which keeps `entry_dir` and the lock check in fd mode (the GF-1 design).
- Each program ran three times with `AXON_AI_MOCK=1` and `timeout 10`:
  - (a) path mode, no filter, no scopes: today's guest run;
  - (a2) the same again, as a nondeterminism control;
  - (b) `--source-fd` under the EACCES filter, with `AXON_ALLOWED_FS_READ=` and `AXON_ALLOWED_FS_WRITE=`: the layered GF-3 run.
- Compared: rc and stdout digest, (a) vs (b).

Counts over the 538 programs whose `axon caps` lists no `fs:*`, `exec` or `native:*` (the rev1 selector):

| outcome | count | meaning |
|---|---|---|
| exit 0, identical stdout | **278** | real parity |
| failed identically under (a) and (b) | **253** | no parity evidence (both refuse before effects matter). 191 `no main function`, 33 empty stderr, the rest are runtime refusals or panics (verify, refinement, `break` outside a loop, ...). List: `corpus_bothfail2.txt` |
| (a) ≠ (a2) | **4** | nondeterministic without any filter (`ad_optimizer.ax` ×2, `distribution.ax`, `lifecycle.ax`). Unattributable |
| **(a) = (a2) ≠ (b)** | **3** | see below |

The 3 differences are filter-attributable, and the rev1 selector misses all three:
- `examples/modular/agent.ax`: rc 0 → **2**, `E0901 module scorelib ... Permission denied` (GFA-1). Module search opens a
  sibling file.
- `examples/asi/kernel_orchestrator.ax` and its hardset copy: rc 0 → **8**, `builtin dstore_clear is not permitted to read path
  .../axon/stores`.
  - The durable store reads and writes `$XDG_CACHE_HOME/axon/stores` (`capabilities.rs:65-73`, `IoKind::DurableStore`).
    `axon caps` labels it `dstore` (`:1569`), not `fs:*`.
  - With the scopes empty, it is a stated refusal.
  - Under the seccomp filter alone (rev1 corpus) the run printed the same stdout and an empty stderr (observed).
    [INFERENCE: the store's `mkdir`/`open` got EACCES and the store fell back silently, so the run lost durability without a
    message.] That is one more reason the scopes are part of GF-3, not optional.

The restated result is the **selector rule** that GF-3.3 adopts: a program is eligible iff
- its `axon caps` capabilities are a subset of {`(none)`, `env`, `net`},
- it resolves no module, and
- it passes `axon check`.

Under that rule there are **535 eligible**: 278 exit-0-identical, 253 both-fail, 4 nondeterministic, **0 differences**. The 3
programs above become launcher refusals, not guest failures. 80 programs have no caps result (check fails). The rest carry
`fs:*`/`exec`/`native:*`/`dstore` or a `mod`. All of those keep today's guest profile.

[INFERENCE] The `mod` detection in `corpus_diff2.sh` greps for a `mod NAME` line; the build uses the resolved set (GF-3.3).

### 2.4 Verdict

**C is feasible** on the backend that executes programs.
- A ~50-line `axon-guest-init` change and a ~70-line interpreter flag were enough to run a manifested-IO program in a
  Firecracker guest with every file syscall other than `execve` withheld (L2, L11).
- A second guest file is refused (L3 EACCES, L12 exit 8).
- D is not needed to close CE-9.

D's cost, for the record:

**Linux profile.**
- seccomp-BPF sees only register values. It cannot read the path string behind `openat`'s pointer, so a path-scoped `openat`
  cannot be written as a filter.
- The kernel routes are Landlock and `SECCOMP_RET_USER_NOTIF`.
  - Landlock is off in the pinned config: `# CONFIG_SECURITY_LANDLOCK is not set`
    (`kernel-6.1-firecracker-v1.10.1.config:2625`; `CONFIG_LSM` at `:2629`). It needs an overlay change, a kernel rebuild, a
    new `kernel.pin` and B263 re-qualification.
  - `USER_NOTIF` costs a supervisor round-trip per open, with a TOCTOU window on the path memory.
- The interpreter-level path scope **already exists** (`AXON_ALLOWED_FS_READ`/`_WRITE`, `env_registry.rs:55-56`; L12). So
  path-scoped D for Axon-level calls costs only GF-3's plumbing with non-empty prefixes.

**Bare-metal kernel.**
- Arguments arrive in registers (`rdi`/`rsi`). A path-scoped gate needs a bounds-checked copy of a NUL-terminated string from
  user memory, user-pointer validation against the page tables, and canonicalisation of `..` (there is no VFS).
- It is a slice of FG-041 and cannot be built before it. [INFERENCE: weeks; its own spec.]
- Register-only checks, such as `clone` flags to tell a thread from a fork, are cheap and will be needed there anyway (§2.1).

---

## 3. Items

### GF-1 (P1, decided-engineering) `axon run --source-fd N FILE`

**Anchors.**
- `crates/axon-core/src/main.rs`: `:306-341` `Command::Run`; `:6275` `cmd_run`; `:6304` `set_provenance_source`; `:6306`
  `read_source(&file)`; `:6333` `run_check_pipeline_located(.., entry_dir_of(&file))`; `:6368` lock check
  (`verify_lock_tamper`); `:8316` `read_source`; `:8527` `validate_ax_extension`.
- `interp/provenance.rs:597`: `append_run_start_jsonl` re-opens `src` to hash it.
- [REVISED: GFD-4] `interp.rs:7527-7532`: the `pub use provenance::{…}` re-export gains `append_run_start_jsonl_bytes`.

**Fix.**
- Add a `--source-fd N` flag, on `run` only.
- FILE stays required and keeps the `.ax` check. It still names the entry directory (module search, `axon.lock`) and labels
  diagnostics and provenance. It is never opened.
- The fd is read once to EOF as UTF-8, then closed.
- N < 3 is refused with rc 1 (`error: --source-fd N: stdin/stdout/stderr cannot carry the program`). So is an fd that `fstat`
  shows is not a regular file.
- `run_start` takes `src_hash` from the bytes read, through a new `append_run_start_jsonl_bytes`; the path variant delegates
  to it.

**Deviation from rev1.** Rev1 set `entry_dir = None` and skipped the lock check. That broke `mod` imports in fd mode
(observed: `mod bandit` → E0001 under fd, `IMPORT-OK 0` under path, `/var/tmp/r51-tmp/CE9Spike/linux/imp`).
- The build keeps both, as rev2 already does on this branch, so fd mode is byte-identical to path mode wherever the filesystem
  is visible.
- With no resolved module, the lock check never opens anything: `check_locked_imports` returns early on `resolved.is_empty()`
  (`main.rs:1854`), and an unreadable lock counts as "no lockfile" (`:1879`).

**Fail-first tests** (`crates/axon-core/tests/cli_run.rs`). They run under `--no-default-features`, as S16 runs them
(`gate.sh:1069`, `cargo test --locked -p axon-core --no-default-features`). [REVISED: GFD-7]

| test | asserts | base |
|---|---|---|
| `run_source_fd_matches_path_run` | same stdout, stderr and rc as `axon run FILE`, for `examples/flagship/agent_task.ax` and a `mod` program with a sibling module | rc 2, unknown flag |
| `run_source_fd_never_opens_file` | FILE names a path that does not exist (`/nonexistent/x.ax`), the fd carries a valid program → rc 0 | unknown flag |
| `run_source_fd_refuses_std_fds` | N=0 → rc 1 with the message | unknown flag |
| `run_source_fd_refuses_directory` | an fd that is not a regular file → refused | unknown flag |
| `run_source_fd_provenance_hashes_fd_bytes` | the `run_start` `src_hash` equals `module_hash(bytes)`, and the FILE label is stored | unknown flag |

### GF-2 (P1, decided-engineering) Load before gate in `axon-guest-init`

**Anchors.**
- `crates/axon-guest-init/src/main.rs`: `:112-124` (argv; bare default `/axon/program.ax`); `:523-552` `child_main` (env at
  `:527-539`, `apply_seccomp` `:544`, exec `:551`); `:731` `exec_process`.
- `profiles/linux-microvm/guest-init.sh:183-184`.

**Fix.**
- New flag: `axon-guest-init --source-fd N=PATH -- <binary> <args...>`.
- In the forked child, before env export and before `apply_seccomp`:
  1. `open(PATH, O_RDONLY|O_NOFOLLOW|O_CLOEXEC)`;
  2. `fstat` and require a regular file;
  3. `dup2` to exactly N (N ≥ 3);
  4. clear `FD_CLOEXEC` on N only.
- Any failure exits 1 before exec, with `[axon-guest-init] cannot open program PATH: <err>`.
- The argv is passed through untouched; the init script states the fd.
- `guest-init.sh` becomes, always, `axon-guest-init --source-fd 3=/work/job/program.ax -- /usr/bin/axon run --source-fd 3
  /work/job/program.ax $ARGS`. By GF-1 parity this changes nothing for any program unless GF-3 withholds file syscalls.
- The bare default (no args) changes the same way.

**Deviation from rev1.** The spike appended `--source-fd N PATH` to the argv. That breaks once `$ARGS` is non-empty, because
`run` takes trailing args.

**PSV branch unchanged.** [REVISED: GFA-6] PSV runs are a separate branch (`guest-init.sh:176-181`): they exec
`axon-psv-runner` with no `program.ax`. GF-2 leaves that branch as it is, and GF-3 refuses `fs: "none"` in PSV mode (GF-3.3).
[INFERENCE: the PSV runner reads `/in/*`.]

**Fail-first tests.**
- Unit (`axon-guest-init`, host): `source_fd_arg_parses`, `source_fd_rejects_std_fd`, `open_source_before_gate_dups_to_n`
  (temp file → fd N readable, CLOEXEC clear, original fd closed). On base these functions do not exist.
- KVM: a new case in `scripts/psv_guest_boot_test.sh`. The L0 program under GF-2 must give the same stdout digest as before.
  [REVISED: GFD-1]
  - S08 and b263 skip (77 and 4) on any host without a built `dist/guest-linux` image, including this one (root and KVM
    present): `psv_guest_boot_test.sh:46-47` skips when `dist/guest-linux/{manifest.json,rootfs.sqfs}` is missing.
  - This host cannot build that image (Out of scope: lineage refusal). So the GF-2 ordering test and the GF-3 launcher rows
    give a result only after `AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh` succeeds; until then they are
    non-results.
  - The "move the open after `apply_seccomp`" mutation is therefore the EXEMPT row M4768 (§5).

### GF-3 (P1, §8.8) The `fs` axis of the guest policy

**Anchors.**
- `axon-guest-init/src/main.rs`: `:97` `MmdsPayload`; `:82-86` `constrains_anything`; `:260` `POLICY_SCHEMA`; `:383`
  `WrongSchema`.
- `scripts/fc_linux_profile.sh` [REVISED: GFA-6]:
  - `:354-356`: "the guest's parser ignores unknown keys, so passing one through would read as enforced".
  - `:357` `SCOPE`: it contains `"fs"`, refused as an unsupported axis before `KNOWN` is consulted.
  - `:359-366`: `KNOWN`, and `GRANTABLE`, which has no `FS`.
  - `:375-376`: constrains-nothing.
  - `:83`: `CMDLINE_MAX=2046`.
- `env_registry.rs:55-56`.
- `capabilities.rs:1263` `program_capabilities`, the canonical static capability surface. Labels at `:1562-1571`: `fs:read`,
  `fs:write`, `net`, `exec`, `env`, `dstore`, plus `native:M`.

**Fix.**

**1. Policy key `"fs": "none"`** (absent = today). When it is `none`, `axon-guest-init`'s child:
- (a) exports `AXON_ALLOWED_FS_READ=` and `AXON_ALLOWED_FS_WRITE=`. Empty means deny every Axon-level read and write with
  exit 8 (L12, L13).
- (b) installs a **compiled-in** substrate filter: the spike's ALLOW / EACCES / KILL classes, plus the arch check. It is
  stacked with any `seccomp_bpf_b64` the policy also carries; stacked seccomp filters take the strictest result.
- (c) [REVISED: GFA-3 / ruling 2] prints `B263-FS none insns=<n>` on the serial stream after the filter is applied.

The substrate list lives in Rust next to `apply_seccomp`, not in the policy blob. That way the operator states the axis, the
guest TCB owns the syscall profile, and the cmdline budget is kept (1367 of 2046 bytes with the spike's blob inline).

**2. Schema `axon-vm-mmds/2`.** [REVISED: GFA-3 / ruling 2]
- A policy that carries `fs` must use schema `axon-vm-mmds/2`. Existing guests refuse it as `WrongSchema` (`:383`), so an old
  image can never silently ignore `fs`.
- The new guest accepts `/1` (no `fs` key allowed) and `/2`.
- `MmdsPayload` stays tolerant of other keys; the schema bump is the version gate.

**3. Launcher (`fc_linux_profile.sh`).** [REVISED: GFA-4]
- Remove `fs` from `SCOPE`, add it to `KNOWN` (value `"none"` only), and count `fs` in both constrains-nothing checks (launcher
  `:375` and the guest's `constrains_anything`), so a policy carrying only `fs` is valid.
- **Refuse** `fs: "none"` before anything is acquired, through the launcher's policy-refusal path, when any of these holds:

| condition | reason |
|---|---|
| `axon caps --json` lists any capability outside {`env`, `net`} (so any `fs:*`, `exec`, `dstore`, `native:*`) | [REVISED: GFA-2 rev2 corpus: `dstore`] L7, L13, kernel_orchestrator |
| `axon check` resolves any module | [REVISED: GFA-1] module search opens files; E0901 under the filter |
| `axon check` fails | no caps result; the run would fail anyway |
| `allowed_effects` contains `Exec` | [REVISED: GFA-5] `execve` is not withheld |
| PSV mode (`--psv-manifest-sha`, `fc_linux_profile.sh:209`) | [REVISED: GFA-6] no `program.ax`; the runner reads `/in/*` |

**4. Results.** The launcher records `fs` in `result.json` (the policy bytes carry the key). [REVISED: GFA-3] It **fails the
run** (status `policy-unbound`, like the existing policy-digest check) when the policy carried `fs` but the serial stream has
no `B263-FS` line.

**Fail-first tests.**

Unit (`axon-guest-init`, host, no KVM):
- `fs_none_filter_denies_path_syscalls`: fork, apply the composed filter, then `open("/etc/hostname")` → EACCES, `write(1)`
  ok, `getrandom` ok.
- `fs_none_filter_kills_non_x86_64_arch`: decode the BPF program and step it in a test interpreter with a non-x86_64 arch
  value.
- `fs_none_exports_empty_path_scopes`.
- `fs_key_requires_schema_v2`: `/1` + `fs` → refused.
- On base, none of these functions exist.

Launcher (`scripts/b263_qualify.sh`, new rows, KVM + image):

| policy + program | expected |
|---|---|
| `fs:none` + `agent_task.ax` | rc 0 with the L0 stdout digest and a `B263-FS` line |
| `fs:none` + `reader_init.ax` | workload exit 8 |
| `fs:none` + `hello.ax` | launcher refusal before boot |
| `fs:none` + `examples/modular/agent.ax` (`--put` the module) | launcher refusal before boot [REVISED: GFA-1] |
| `fs:none` + `allowed_effects` with `Exec` | launcher refusal [REVISED: GFA-5] |
| `fs:none` on an image whose guest predates `/2` | `WrongSchema` refusal [REVISED: GFA-3] |

Non-results on this host (GFD-1).

**Corpus differential.** [REVISED: GFD-5 / ruling 5] `corpus_diff2.sh`, promoted to `scripts/guest_fs_corpus.sh`. Over the
selector-eligible set it requires 0 differences between (a) and (b) where (a) = (a2), and it lists the nondeterministic and
both-fail programs explicitly. It joins `gate.sh` as a new selectable stage in `governance/gate-lanes.json`; it is host-only
and needs no KVM.

### GF-4 (P2, §8.7) `axon_kernel_gate.sh` Layer 3

**Anchors.**
- `scripts/axon_kernel_gate.sh`: `:80-84` (Layer 2 writes `agent_task.axmeta`); `:117-143` (good agent with `IO,FS`);
  `:145-168` (evil agent with `AI`; its exit 8 comes from the K5 synthetic `openat`, not from anything the evil program does).
- `axon-vm/src/admit.rs:268-296` (`OverrideWidens`).
- `axon-guest-kernel/src/enforce.rs:338-379`.

**Finding.**
- At base the gate is red (B8, rc 1).
- On the bare-metal backend, the good/evil outcome is decided by whether the grant names `FS`, never by the program (B7:
  `exit(3)` reports 0).
- Under C the bare-metal stub has no honest analogue: with no file syscalls, K5 has nothing to deny for either agent.

**Fix (§8.7).**
- Layer 3 runs the good/evil pair on the Linux profile with GF-2/GF-3:
  - good agent: `fs:none`, `["IO"]` → rc 0;
  - evil agent: `["IO"]` → rc 2 at check (as L10), or 8 at runtime if check is bypassed.
- The bare-metal K5 run stays as a separately labelled "gate installs" smoke check. It asserts only what it shows: the LSTAR
  gate denies a syscall whose effect is withheld.
- Wiring: [REVISED: GFD-5 / ruling 5] `axon_kernel_gate.sh` joins `gate.sh` as a selectable stage in `gate-lanes.json`. Like
  S08, it is KVM- and image-gated, and 77 is a non-result. The `gate.sh`/`gate-lanes.json` edits land after `b2/ce` merges
  (§7).

**Fail-first test.** The gate itself: rc 1 at base (B8), rc 0 after, on a host with a built image.

### GF-5 (P3, §8.9) The manifest `syscall_hint` → seccomp path

**Anchors.**
- `crates/axon-core/src/main.rs:8978-9068` `syscalls_for_effects`: `"io" | "fs"` (`:9000`) both grant `openat`, `mkdir`,
  `unlink`, `rename`, ... There is no `execve`, `poll`, `clone`, `getrandom` (unless Random) or `open`.
- `axon-vm/src/main.rs:1221-1235` builds `seccomp_bpf_b64` from it.
- `axon-vm/src/main.rs:2100-2151` `bpf_allowlist`: matches syscall numbers only, defaults to KILL_PROCESS, has no arch check.
- `axon-guest-kernel/src/mmds.rs:15`: `seccomp_hint // unused at kernel level`.

**Finding (observed, host).** I rebuilt the filter exactly as `bpf_allowlist` builds it (`tasks/guest-fs-spike/hintfilter.py`,
which parses `SYSCALL_TABLE` from the source). I applied it pre-exec, as `axon-guest-init` would, to the hint in
`agent_task.axmeta`:
- as-is: rc **159**, killed at `execve`;
- with `execve` added: rc 159, killed at `poll`;
- with the substrate added: killed at `open` (musl opens with `open`, not `openat`).

The only backend `axon-vm` launches ignores the field. So the manifest's seccomp has never confined a running interpreter. If
it were ever applied it would kill every program, and for an IO program it allows the very `openat` O034 is about.

**Fix (§8.9).** Delete `syscall_hint` emission and `bpf_allowlist`. GF-3's compiled-in substrate filter becomes the one
syscall profile. The manifest gains `"capabilities": [...]` (`program_capabilities`), which GF-3's selector reads.

Blast radius [REVISED: GFA-6; from `git grep -l syscall_hint` at b3a9c56e]:
- `crates/axon-core/src/main.rs` (emission, `syscalls_for_effects`);
- `crates/axon-vm/src/main.rs` (`load_manifest`, `bpf_allowlist`, `SYSCALL_TABLE`, the `seccomp_bpf_b64` derivation);
- the wire field `seccomp_hint` in `crates/axon-guest-kernel/src/mmds.rs:15` (+ `:184`, `:206`);
- `AXON_VM.md`;
- `examples/flagship/seccomp-agent.json`;
- `examples/flagship/compare_docker.sh`;
- `tasks/capability-surface-spec.md`;
- `tasks/opportunities.md` (stamp);
- two committed cortex docs, `docs/axon_cortex_v0_22/.../AXON_CORTEX_MASTER.md` and `.../review/SOURCE_EVIDENCE.md`. These are
  historical records: annotate them, do not edit them.

The untracked `docs/axon_cortex_v0_23/*` copies are not touched.

---

## 4. Diagnostics and exit codes

No new codes.
- GF-1 refusals are CLI errors, rc 1, the same class as `error reading FILE`.
- GF-3 program-level refusals reuse the existing sandbox violation, exit 8 (L12).
- Substrate denials surface as EACCES and have no exit code of their own.
- The launcher refusals in GF-3.3 and the missing-`B263-FS` failure use `fc_linux_profile.sh`'s existing policy statuses.

---

## 5. Shared mechanics

- **pci theme:** one per behaviour-changing commit touching `crates/axon-core/src` (GF-1, GF-5).
- **CHANGELOG:** one line per item.
- **g01 rows: M4760-M4768** (8 active + 1 EXEMPT; M4769 spare). [REVISED: GFD-6 / ruling 2] Each row names crate, `--test`
  target and test function, per `scripts/v022_g01_mutations.py`, and gets attack markers per
  `scripts/v022_attack_markers.py`.

| id | item | mutation | killed by |
|---|---|---|---|
| M4760 | GF-1 | open FILE instead of the fd | `axon-core` / `cli_run` / `run_source_fd_never_opens_file` |
| M4761 | GF-1 | drop the N < 3 check | `cli_run` / `run_source_fd_refuses_std_fds` |
| M4762 | GF-1 | hash the label path instead of the bytes | `cli_run` / `run_source_fd_provenance_hashes_fd_bytes` |
| M4763 | GF-1 | pass `entry_dir = None` in fd mode | `cli_run` / `run_source_fd_matches_path_run` (`mod` case) |
| M4764 | GF-2 | drop the `FD_CLOEXEC` clear | `axon-guest-init` unit / `open_source_before_gate_dups_to_n` |
| M4765 | GF-3 | path syscall → ALLOW | `axon-guest-init` unit / `fs_none_filter_denies_path_syscalls` |
| M4766 | GF-3 | drop the arch check | `axon-guest-init` unit / `fs_none_filter_kills_non_x86_64_arch` |
| M4767 | GF-3 | skip the empty path-scope export | `axon-guest-init` unit / `fs_none_exports_empty_path_scopes` |
| M4768 | GF-2 | move the open after `apply_seccomp` | **EXEMPT**: "killed only by S08 KVM boot (`psv_guest_boot_test.sh`); no cargo-test anchor" |

- **Docs:**
  - `spec/axon-for-llms.md` (`axon run --source-fd`);
  - `AXON_REFERENCE.md` (regenerated);
  - `profiles/linux-microvm/README.md` (policy table: `fs`, schema `/2`, the refusal list, `B263-FS`);
  - `AXON_KERNEL.md` (Layer 3, §8.7);
  - `AXON_VM.md` (GF-5);
  - `tasks/opportunities.md` (O034 stamp).

---

## 6. Gates and cost budget

| stage | what | wall | when |
|---|---|---|---|
| GF-1 | `cargo test --locked -p axon-core --no-default-features --test cli_run run_source_fd` [REVISED: GFD-7] | 1-3 min [INFERENCE: cached build] | each commit |
| GF-2/3 | `cargo test -p axon-guest-init` (3 tests today, 0.01 s run; host only; in S20 and clippy per `gate-lanes.json:61`, `gate.sh:1002`) | < 1 min | each commit |
| GF-3 | corpus differential rev2 (698 programs × 3 runs, `-P 16`) | **44 s observed** | GF-3, GF-5 |
| KVM | guest boot matrix (`vmrun.sh`-style; 4-5 boots observed in 1.1-1.6 s wall) + `psv_guest_boot_test.sh` / `b263_qualify.sh` rows | minutes [INFERENCE] | after GF-2/3, on a host with a built image |
| image | pinned Linux kernel (6.1.188) in `build-guest-image.sh` | **46 s observed** before the lineage refusal (§0) | once per image |
| image | [REVISED: GFD-3] rootfs + manifest rebuild (`AXON_KERNEL_BACKEND=linux build-guest-image.sh --rootfs-only`, `:344`) + b263 re-run, after **every** GF-2/GF-3 commit (both change bytes baked into the rootfs; S08 checks the manifest artifacts at `psv_guest_boot_test.sh:48-51`) | not timed [INFERENCE] | each GF-2/GF-3 commit |
| end | `scripts/gate.sh --strict` (integrator) | 43m24s observed (`gate.sh:1537`, per GFD) | integration |

[REVISED: GFD-3] b263 currently ends PASS_WITH_BLOCKED (x1-x4 BLOCKED, `gate.sh:1404-1406`). That fails the strict gate
whatever GF-3 does, so GF-3's b263 rows cannot turn that stage green on their own.

Paid calls: none (`AXON_AI_MOCK=1` throughout).

Build effort, in lines excluding tests and docs:

| item | estimate | notes |
|---|---|---|
| GF-1 | ~70 | spike: +78 `main.rs`, +21 `provenance.rs`, +6 `interp.rs` |
| GF-2 | ~60 | spike: +52 |
| GF-3 | ~200 | filter composition in Rust ~80, schema `/2` + `B263-FS` ~30, launcher key + refusal list ~70, env ~10 |
| GF-4 | ~60 | gate script |
| GF-5 | about −250 deleted, +20 added | |

[REVISED: GFD estimate, INFERENCE] Critical path is about 2-3 days after `b2/ce` merges, plus the image-build blocker (§0).

---

## 7. Sequencing and lanes

[REVISED: ruling 5] **The lane lands after the ceilings branch `b2/ce` merges into `b2/int`.** The `gate.sh` and
`gate-lanes.json` edits go in then.

| file | items | siblings |
|---|---|---|
| `crates/axon-core/src/main.rs` (`cmd_run`, `Command::Run`) | GF-1 | CeilingsBuild owns `main.rs` **flags** (CE-2 `--ceiling`); serial after `b2/ce` |
| `crates/axon-core/src/interp/provenance.rs:597` | GF-1 | CE-2 touches the same fn (ceilings-spec §7); serial after `b2/ce` |
| [REVISED: GFD-4] `crates/axon-core/src/interp.rs:7527-7532` (re-export) | GF-1 | CeilingsBuild owns `interp.rs`; serial after `b2/ce` |
| `crates/axon-guest-init`, `profiles/linux-microvm/guest-init.sh`, `scripts/fc_linux_profile.sh` | GF-2, GF-3 | none |
| [REVISED: GFD-4] `scripts/gate.sh`, `governance/gate-lanes.json` | GF-3 (corpus stage), GF-4 (kernel-gate stage) | CeilingsBuild owns them; edits land after `b2/ce` |
| `scripts/axon_kernel_gate.sh` | GF-4 | none [REVISED: GFD-4: its gate wiring is the row above] |
| `axon-vm/src/main.rs`, `axon-core/src/main.rs:8978-9068`, `axon-guest-kernel/src/mmds.rs` | GF-5 | none |

Order: GF-1 → GF-2 → GF-3 → GF-4 → GF-5.
- GF-1 and GF-2 change no program's behaviour by construction (parity tests).
- All of the lane waits for `b2/ce`.

---

## 8. Engineering decisions (decided)

1. **The source travels as an inherited fd, not stdin.** Stdin stays the program's: `read_line` must keep working, and
   `--source-fd` refuses fds 0-2.
2. **fd mode keeps FILE as the entry directory and keeps the lock check.** This deviates from rev1 (§3 GF-1); rev2 on this
   branch implements it.
3. **The substrate filter returns EACCES, not KILL, for file syscalls.**
   - Under KILL, the interpreter dies in its own best-effort provenance `mkdir` before any program code runs (L4, L5; host
     trace: `mkdir` = nr 83 killed).
   - Program-level file access is refused one layer up, with exit 8 (L12).
   - So EACCES only ever meets substrate bookkeeping, module search or the durable store (both refused by the selector,
     GF-3.3), or a classifier gap. [REVISED: GFA-1]
   - Non-file syscalls outside the allow list stay KILL. `execve` stays ALLOW, and exec containment rests on the inherited
     filter plus the selector (GFA-5).
4. **The init script states the fd number** (`--source-fd 3=PATH` and `run --source-fd 3`). There is no argv rewriting in
   init.
5. **The substrate syscall list is compiled into `axon-guest-init`.** Policies may add a stricter filter and can never loosen
   it.
6. **The selector input is `program_capabilities` plus the resolved module set, not `effect_union`.**
   - `effect_union` is `["IO"]` for both `agent_task.ax` (prints) and `reader_init.ax` (reads `/init`). I observed this with
     `axon build --emit-manifest` on both.
   - The capability rule is an allowlist (⊆ {`env`, `net`}), not a denylist: the rev2 corpus found `dstore`, which a
     denylist of `fs:*`/`exec`/`native:*` missed. [REVISED: GFA-1, GFA-2]
7. **Layer 3 of `axon_kernel_gate.sh` moves to the Linux profile** (was NH-G1 → NH-F1, option A). [REVISED: ruling 4]
   - The good/evil pair runs under GF-2/GF-3, and the bare-metal K5 run is kept as a "gate installs" smoke check labelled as
     such.
   - Why: on the bare-metal backend the asserted outcome is produced by the grant, not by the program (B7: `exit(3)` reports
     0). Option B, dropping the manifest to get green again, keeps asserting a program outcome no program produced. Option C
     keeps the gate red at base (B8).
   - Effect: the gate goes from red to green on a host with a built image, and the bare-metal claim narrows to what it shows.
8. **`fs: "none"` is an opt-in policy key** (was NH-G2 → NH-F2, option A). [REVISED: ruling 4]
   - Absent key = today's behaviour.
   - Why: under the GF-3.3 selector the rev2 corpus shows 0 filter-attributable differences over the 535 eligible programs,
     but 253 of them only show both-fail parity and 4 are nondeterministic.
   - Defaulting it on for every eligible program (option B) would turn any classifier gap into EACCES or exit 8 for runs that
     pass today. Rev1's selector already had two such gaps, `mod` and `dstore`, so a gap is not hypothetical.
   - Revisit B after one release of A, with the corpus stage as its gate.
9. **The manifest `syscall_hint` → seccomp path is deleted** (was NH-G3 → NH-F3, option A). [REVISED: ruling 4]
   - Why: it has never been applied by any launcher (the bare-metal guest ignores `seccomp_hint`), and if applied it kills
     every interpreter at `execve` (§3 GF-5). It also grants `openat` for `IO`.
   - Repairing it (option B) would mean a second syscall profile beside GF-3's compiled-in one, with no consumer. Keeping it
     (option C) keeps a claimed confinement that is not enforced.
   - Blast radius: §3 GF-5 (GFA-6's full list).

---

## 9. Needs-human (answer in one pass)

| # | question | options (blast radius) | recommendation |
|---|---|---|---|
| NH-F4 | Kernel-level path scoping (D) on the Linux profile? | **A** none: the interpreter path scope + substrate EACCES (this spec). **B** Landlock (kernel config overlay, rebuild, new `kernel.pin`, B263 re-qualification) for programs with scoped `fs:*`, so they too could run without broad file access. **C** `SECCOMP_RET_USER_NOTIF` supervisor (a round-trip per open, TOCTOU on path memory) | **A**; revisit B if a non-Axon workload ever runs in the guest, or if programs with `fs:*` need a guest bound |
| NH-F5 | [REVISED: ruling 3] The shared repo config `/home/cklaus/projects/axon/.git/config` sets `blame.ignorerevsfile`, so the Linux image build refuses (§0) and every KVM row in this spec is a non-result here. Remove the key? | **A** remove the key from the shared `.git/config` (git blame stops ignoring the listed revs locally; `--ignore-revs-file` still works per command). **B** build the image on another host. **C** leave it (GF-2/GF-3 KVM rows stay non-results on this host) | **A** (host config; not a code change) |

`[DECIDED by human 2026-11-02: NH-F4 A (no kernel-level path scoping; revisit B if a non-Axon workload runs in the guest or
programs with fs:* need a guest bound). NH-F5 A: the key was removed from the shared .git/config on 2026-11-02 (backup
/var/tmp/r51-tmp/git-config.bak-2026-11-02); git config --local --get blame.ignorerevsfile now exits 1. Build authorized.]`

NH-F1..F3 are decided: §8.7, §8.8, §8.9.

---

## 10. Verification log (2026-11-02; this host: `/dev/kvm` present, `/usr/local/bin/firecracker` = Firecracker v1.10.1)

| probe | result |
|---|---|
| B1-B7 `axon-vm run` matrix (bare-metal backend) | 0 / 8 / 2 / 2 / 8 / 8 / 0 (no `PROGRAM-RAN`) |
| B8 `scripts/axon_kernel_gate.sh` (worktree; `dist/guest` = spike-built image; `HOME` in scratch) | rc 1, "FAIL (4 passed, 1 failed)", good agent exit 2 |
| `scripts/build-guest-image.sh --kernel-only` (`AXON_GUEST_BUILD_UID=64321`) | rc 101, linker script path (Out of scope) |
| [REVISED: GFD-2 / ruling 3] `AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh` (`CARGO_TARGET_DIR=.doc-build/CE9Spike-linuximg`, `AXON_GUEST_BUILD_UID=64321`) | **rc 1, 46 s**; vmlinux built, then refused by the lineage check (§0); log `tasks/guest-fs-spike/build-linux-image.log` |
| direct `cargo build -p axon-guest-kernel ... --release` | rc 0 (`vmlinuz` sha `255f97e7…`) |
| pinned Linux kernel 6.1.188, spike build | `vmlinux` sha `3e091e41…` |
| L0-L13 Linux guest runs (`vmrun.sh`) | §2.2/§2.3 tables; verbatim in `tasks/guest-fs-spike/guest_matrix_1.txt` (L1-L9) and `guest_matrix_2.txt` (L10-L13, re-run 2026-11-02, identical results) |
| host fd-mode trace (strace) | §2.3 syscall list |
| host filter runs (`hostgate.py`): path+EACCES / fd+EACCES / fd+KILL / fd+EACCES reader / unfiltered reader | 1 / 0 / 159 / 0 `READER-ERR` / 0 `READER-GOT` |
| `AXON_ALLOWED_EFFECTS=IO` reader (host) | rc 0, file read: the effect ceiling does not cover files |
| `AXON_ALLOWED_FS_READ=` reader (host, reference binary) | rc 8, sandbox violation |
| `mod` import: path / fd (rev1 spike, `entry_dir None`) | `IMPORT-OK 0` / E0001 (drives §8.2) |
| [REVISED: GFA-1] `examples/modular/agent.ax`, rev2 fd + EACCES + empty scopes (host) | rc 2, E0901 `module scorelib ... Permission denied`; path mode rc 0 |
| `syscall_hint` filter (`hintfilter.py`) applied pre-exec | 159 at `execve`; with execve 159 at `poll`; with substrate killed at `open` |
| corpus differential rev1 | 698 programs, 29 s (superseded, §2.3) |
| [REVISED: GFA-2] corpus differential rev2 (`corpus_diff2.sh`) | 698 programs, 44 s. Rev1 selector: 538 → 278 rc0-identical, 253 both-fail, 4 nondet, **3 diff** (`modular/agent.ax`, `kernel_orchestrator.ax` ×2). GF-3.3 selector: 535 → 278 / 253 / 4 / **0** |
| [REVISED: GFA-3..6, GFD-1/3/5/7] cited lines | read and confirmed: `fc_linux_profile.sh:209,354-357,375-376`; guest-init `:82`, `:260`, `:383`; `guest-init.sh:176-181`; `psv_guest_boot_test.sh:46-51`; `gate.sh:1069,1404-1416`; `build-guest-image.sh:344`; no `gate.sh`/`governance` caller of `axon_kernel_gate.sh`; `git grep -l syscall_hint` list; `mkfilter.py` ALLOW holds 56 and 59, FS holds 79 and 217 |
| `cargo test -p axon-guest-init` (spike) | 3 passed, rc 0 |
| `cargo fmt --all -- --check` (spike rev1) | rc 0 |

**Not run:**
- clippy on the spike commits;
- `psv_guest_boot_test.sh` and `b263_qualify.sh`: they need a `dist/guest-linux` image, which this host refuses to build (§0,
  NH-F5);
- the jailer path (`fc_linux_profile.sh`): the spike booted Firecracker directly with the same cmdline policy word;
- any `asi-runtime` build.

Claims that depend on these are marked [INFERENCE].

**Reproduce:** `tasks/guest-fs-spike/` holds:
- `mkfilter.py`: the spike filter;
- `hostgate.py`: host mirror of the child's open → seccomp → exec;
- `vmrun.sh`: one guest boot; the rootfs is assembled as described in its header;
- `hintfilter.py`;
- `corpus_diff.sh` + `corpus_results.tsv` (rev1);
- `corpus_diff2.sh` + `corpus_results2.tsv` + `corpus_bothfail2.txt` (rev2);
- `axon_vm_run1.sh`;
- `guest-init.spike.diff`;
- `reader_init.ax`;
- `build-linux-image.log`;
- `guest_matrix_1.txt` and `guest_matrix_2.txt`.
