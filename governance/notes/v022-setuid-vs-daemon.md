# Decision memo: setuid-root helper or root daemon for the protected launch

Status: **DECIDED, operator decision F (2026-10-04): keep the setuid-root helper (S) for C9,
under conditions C1-C3 below.** The per-connection service (D2) remains the planned
replacement. This memo answers amendment 45's open review item ("review setuid-root helper vs
daemon"). It changes no code. Since it was written, amendments 65 and 66 have met C1 in code
and added C2's checks, and amendment 65 decided C3. The sections below are updated where
those facts changed. Elsewhere, line references are to the dev head it was written against.

Sources: `governance/specs/v022-psv-protocol.md` amendments 45, 50 and 54;
`crates/axon-fabric/src/privileged_launcher.rs`, `bin/axon-protected-launcher.rs`,
`sealed_exec.rs`, `custodian.rs`; `crates/axon-fabric/tests/privileged_launcher.rs`;
`scripts/trust_root_preflight.sh`; mutation rows M520-M549 and M620-M639 in
`scripts/v022_g01_mutations.py`. Line references are to the dev head this was written
against (afe64ef9).

## The question

Fabric runs as its own non-root uid (operator decision A). Only the launch needs root:
the jailer, cgroups, a network namespace. Two ways to give Fabric that one root action:

- **S, setuid-root helper (what is built).** `axon-protected-launcher` is installed
  `04750 root:<fabric group>`. Fabric executes it once per launch and writes one request on
  its stdin. The kernel reports the caller's real uid, and the helper admits only the
  configured Fabric uid.
- **D, root daemon on an authenticated socket.** A root process listens on a Unix socket
  that only the Fabric group can connect to. It authenticates each caller with
  `SO_PEERCRED` and serves the same request. D has two forms:
  - **D1, long-running** (`Accept=no`): one server process, as amendment 45 assumed.
  - **D2, per connection** (`Accept=yes`, `StandardInput=socket`): systemd starts a fresh
    root instance for each connection and hands it the connection as stdin/stdout. The
    instance serves one launch and exits. Amendment 45 did not consider this form.

The request schema, the operator config, the snapshot, the observation check, the custodian
spend and the hand-over are the same in every design. They live in `serve_as` and below.
Only the entry differs: how the process becomes root, what it inherits, and how it learns
who the caller is.

## Comparison

| | S: setuid helper | D1: long-running daemon | D2: per-connection service |
|---|---|---|---|
| Root process exists | only during a launch | always | only during a launch |
| Caller authentication | kernel real uid (`getuid`) | `SO_PEERCRED` | `SO_PEERCRED` |
| Who may reach it | Fabric group (file mode 04750) | Fabric group (socket 0660) | Fabric group (socket 0660) |
| State inherited from the caller | a lot (next section) | nothing | nothing |
| Fabric may run with `NoNewPrivileges` | **no**: the setuid bit is then ignored and the helper refuses (fail-closed) | yes | yes |
| Fabric's cgroup limits apply to the root side | yes: a Fabric OOM kill takes the helper with it | no | no |
| Concurrency | one process per launch (kernel) | the server must serialise or thread | one process per launch (systemd) |
| Filesystem constraint | not on a `nosuid` mount | none | none |
| Extra TCB | kernel setuid exec rules; glibc/ld.so secure mode (`AT_SECURE`) | systemd socket activation; a long-lived root event loop | systemd socket activation |
| Code in this repo | built and evidenced | none | the activation and `SO_PEERCRED` code already exists for the custodian (`custodian.rs`: `activated_listener`, the peer-credential checks, M626-M628) |
| Deployment | install one file 04750; group membership | unit + socket; a service lifecycle | unit + socket (template instance); no lifecycle beyond systemd's |
| Audit | one process per launch; no unit, no journal entry unless the helper logs | a journald unit; per-request logging is up to the code | a journald unit instance per launch (`axon-launch@N`), with start, exit status and resource accounting for free |

### Attack surface

- **S.** Any process running as a member of the Fabric group can exec the helper. Only the
  Fabric uid gets past the uid check (`serve_as`; M531, M548). A setuid program's input
  surface is larger than its request: everything the kernel lets a caller set before
  `execve` and does not reset for a set-id exec. The helper resets it first, in `harden()`
  (`privileged_launcher.rs` around line 1168): signal dispositions and mask, umask, working
  directory, every descriptor above stderr (`close_range`), the environment, `PR_SET_DUMPABLE`
  0, and resource limits (core 0; CPU, FSIZE, DATA, AS, NPROC infinite; NOFILE 65536).
  `become_root()` then makes every id 0 (M549). The kernel adds `AT_SECURE`, so ld.so ignores
  `LD_*`. The kernel also drops the dumpable flag at a set-id exec, so the caller cannot
  ptrace the helper.

  What `harden()` does NOT reset, and the kernel does not either:
  - the caller's cgroup (memory, pids, CPU limits apply to the root launch);
  - its mount, network, IPC and UTS namespaces (an unprivileged Fabric cannot create them
    outside a user namespace, and inside one the set-id bit does not elevate);
  - its scheduling class and nice value, and its I/O priority;
  - `RLIMIT_STACK`, `RLIMIT_MEMLOCK`, `RLIMIT_MSGQUEUE`, `RLIMIT_RTPRIO` and the other
    limits not in the list;
  - its controlling terminal and session;
  - the audit login uid.

  Between `execve` and `become_root()` the helper's real uid is still Fabric's, so Fabric
  can signal it. That is only a denial of service of its own launch. Nothing has been
  acquired by then.
- **D1 and D2.** The surface is the request, and that surface is the same for every design.
  systemd builds the process environment, so nothing is inherited from the caller. D1 adds a
  long-lived root event loop: one malformed request that wedges or corrupts it affects every
  later launch. D2 does not have that problem.

### TCB size

The helper logic (`serve_as` and below: the Rust std, serde_json, sha2, ring and libc) is the
same in every design. S adds the kernel's set-id exec semantics and glibc's secure mode,
plus `harden()`'s list, which must be complete. D1 and D2 add systemd's socket activation,
which the custodian already relies on, so it is in the TCB anyway. D1 also adds a
concurrency model. None of the three changes the TCB amendment 45 lists below the helper:
the pinned bash and launcher, the unpinned root-owned system tools the launcher runs, the
pinned firecracker and jailer, the kernel and rootfs. Those dominate. The compiled-launcher
follow-up shrinks them; switching the entry design does not.

### Environment and descriptor inheritance

This is S's inherent cost, and the one place the designs really differ. `harden()` handles
the classic vectors: environment (`BASH_ENV`, `PATH` and `LD_*` reaching the root bash),
inherited descriptors, umask, cwd, signals and core dumps. Two facts matter here:

1. **`harden()` is now evidenced (amendments 65 and 66).** When this memo was written,
   nothing tested it: deleting the environment clear or the `close_range` left the suite
   green. Now `a_callers_process_state_never_reaches_the_root_helper_or_its_launcher` runs the
   real setuid-root helper from a hostile Fabric-uid caller, with one row per reset:
   - ignored signals (M1474) and the signal mask (M1475);
   - umask (M1476) and cwd (M1477);
   - inherited descriptors (M1478);
   - `RLIMIT_CORE` (M1479), the lowered CPU/FSIZE/DATA/AS/NPROC limits (M1480) and NOFILE
     (M1482);
   - SIGPIPE ignored (M1481).

   The environment clear was first exempted as dominated by `sealed_exec`'s explicit envp.
   Amendment 66 found that false. A caller can make the helper panic after `harden()`, by
   running `--probe` on a closed pipe. With the clear removed, the helper's own Rust runtime
   then read `RUST_BACKTRACE` and printed the root process's stack, with every frame's
   address, to the caller's stderr. The clear is the only guard on that route, and it is now
   an active row (M1487, attack
   `the_root_helpers_address_layout_never_reaches_its_caller`). `PR_SET_DUMPABLE 0` remains
   a measured exemption, not counted as killed: the kernel already makes a set-id exec
   non-dumpable at `fs.suid_dumpable` 0.
2. **`harden()`'s list is an enumeration of hazards, not a primitive.** The
   fix-at-the-source rule this project applies elsewhere argues against relying on such a
   list. D2 removes the class rather than enumerating it.

### Auditability

S leaves no record beyond what the helper writes to Fabric (its report) and what the
launcher writes. D2 gives every launch a systemd unit instance: start time, exit status,
the resources used, and the journal. The record is written by a component that is outside
Fabric's uid and outside the helper. D1 gives one unit for all launches.

### Deployment complexity

S has the fewest moving parts: one file 04750, one group. But it needs a non-`nosuid`
filesystem, and Fabric's service manager must not set `NoNewPrivileges`. Most systemd
hardening presets set it. With it set, the helper's euid is not 0 and it refuses every launch.
That failure is safe. Since amendment 65 it is no longer easy to misdiagnose:
- the helper's refusal names NoNewPrivileges (`PR_GET_NO_NEW_PRIVS`), or else nosuid or a user
  namespace (`setuid_honoured`);
- `--probe` reports `no_new_privs`;
- the trust preflight takes `--fabric-pid` (required in protected mode) and FAILS unless the
  running Fabric has `NoNewPrivs: 0`;
- the deployment kit refuses a Fabric unit that sets `NoNewPrivileges`, or anything that
  implies it on some systemd version. D1 and D2 need a socket unit and a service unit (D2: a
template). That is the custodian's shape, which the operator deploys anyway.

### What the current code and tests already enforce (S)

- The caller is the configured Fabric uid, by real uid (M531, M548). Root is never the
  Fabric uid (M536, M546).
- The helper reads its config only from the fixed path, ownership-walked from `/`. A
  production build refuses `--test-config` (`a_production_helper_never_takes_its_config_from_a_path_its_caller_names`).
- After authentication the helper is root in every id (M549). A production helper whose
  euid is not 0 launches nothing (`a_production_helper_that_is_not_root_launches_nothing`).
- The request schema is fixed, with unknown fields denied (M535). Paths are plain direct
  children of the out root (M532, M534).
- Inputs come only from the Fabric uid's files, bounded, and are snapshotted into a
  root-private staging dir (M537-M539).
- Every authority program is executed same-byte from the descriptor that was hashed
  (M520-M530).
- One verified observation authorizes one launch, spent at the root boundary through the
  custodian (M620-M639). Since amendment 65 the custodian is also the PROGRAM the operator
  pinned. The helper config's `custodian.sha256` is required in production (M1485). On every
  reply message the helper takes the kernel's `SCM_PIDFD` for the sender. It hashes the
  sender's `/proc/<pid>/exe` through an open descriptor, re-checking the pidfd after the open,
  and refuses an impostor (M1483) or an executable another uid can rewrite (M1484).
  Amendment 66 (M1489) adds the verification as a whole to M602's guard set. A helper whose
  euid is not 0 cannot open another uid's `/proc/<pid>/exe`, so it is refused there before
  the spend rule (M628). This needs Linux 6.5 or later (`SO_PASSPIDFD`); an older kernel
  refuses every pinned call.
- `harden()`'s resets (M1474-M1482, M1487; see above). A setuid-root helper whose euid is not
  0 refuses in every build, naming the cause (amendment 65).
- The guest runs the policy the manifest names (amendment 54, M670-M672).
- Special files are removed and set-id bits cleared at the hand-over (M540).
- The trust preflight checks the installed helper: root-owned, setuid, group = Fabric's, no
  access for other; `--probe` as the Fabric uid reports euid 0 and a production build; every
  other actor's exec is refused by the kernel.
- The real guest boot test (`psv_guest_boot_test.sh`, cases `helper`, `helper-policy`) runs
  a non-root uid through a setuid-root test-trust helper with the real launcher and image.

Not enforced, or not evidenced:

- ~~`harden()`'s resets have no test and no row.~~ Done (amendments 65 and 66).
- ~~The `NoNewPrivileges` interaction is not covered by the preflight.~~ Done (`--fabric-pid`,
  amendment 65).
- The production helper build has never been exercised in a real guest boot. The boot test
  uses a test-trust helper, and the `trust-probe` case runs the launcher's plain mode.
  Amendment 45 lists this as a deployment item.
- `PR_SET_DUMPABLE 0` has no row (measured exemption, amendment 65).

## A related item the same decision touches: the observer

The protocol's recorded follow-up says: "The observer runs as Fabric's UID
(protected-only)." `observer::observe` executes the operator's observer program through
`sealed_exec` **as the Fabric uid**. An observer that signs with its own key can therefore
only use a key the Fabric uid can read. That would let Fabric mint observations, which is
the separation ADR-002 exists to prevent. The observer needs the same kind of boundary as
the launch: a setuid observer helper (S) or an observer service reached over a socket
(D2-shaped, like the custodian). Whatever the operator decides here should be decided for
the observer at the same time. **No production observer ships in this repository**, so the
deployment kit takes the observer program from the operator.

**Decided (operator decision G):** the observer will be a separate, socket-activated observer
service (D2-shaped, like the custodian). It is being built on another branch, and the kit's
support for it lands with that branch.

## Recommendation (adopted as operator decision F)

**Keep the setuid-root helper (S) for the C9 protected deployment, under three conditions.
Plan the per-connection socket-activated service (D2) as the replacement, landing with the
compiled-launcher follow-up.**

Reasons to keep S now:

1. It is built, reviewed and evidenced through the production route: 29 helper rows (M520-M549; M533 is unused), 20
   custodian/observation rows (M620-M639), and the real guest boot through a setuid helper. Switching
   would rewrite the entry and the preflight, invalidate those rows' kill evidence, and
   reopen review just as the candidate approaches freeze. No open attack forces that.
2. Its inherited-state hazards are bounded. The caller is the Fabric uid, which is already
   authenticated and already holds everything those hazards could reach. The kernel's
   set-id rules (`AT_SECURE`, the dumpable flag, user-namespace non-elevation) cover the
   vectors that would cross a uid. The ones left (cgroup, limits, scheduling) are denial of
   service of Fabric's own launch.
3. D's main advantages are hardening of Fabric itself (`NoNewPrivileges`) and removing a
   hazard class. Both are worth having. Neither closes a hole in the current counting rules.

Conditions for keeping S:

- **C1: evidence for `harden()`. MET** (amendments 65 and 66; see above). As written: before the protected deployment, add attack tests with
  rows that run the real helper with a planted environment (`BASH_ENV`, `PATH`,
  `LD_PRELOAD`), an inherited descriptor, an ignored `SIGTERM`/`SIGCHLD`, a lowered
  `RLIMIT_NOFILE`/`RLIMIT_FSIZE` and a hostile cwd and umask. Each must show that the
  launcher it starts sees none of it. This is protected code and evidence work for the
  candidate's own process; the kit does not do it.
- **C2: no `NoNewPrivileges` on Fabric. Checked; the operator's to keep.** The Fabric
  service (however MiCode runs it) must not run under `NoNewPrivileges=yes`,
  `setpriv --no-new-privs`, or a container's `no-new-privileges`. Two checks now exist. The
  preflight's `--fabric-pid` reads the running process's `NoNewPrivs` (amendment 65). The
  kit's `fabric-unit` step judges the unit's text, including the settings that imply it on
  some systemd versions (runbook step 4a).
- **C3: give the root side its own cgroup. DECIDED in amendment 65:** the helper, the
  launcher and its host tools stay in Fabric's cgroup by design. The VMM is in the jailer's
  own cgroup, with limits set per launch. A launch that Fabric's limits kill is a failure or
  unknown attempt, never a verdict. The operator sizes Fabric's `MemoryMax=`/`TasksMax=`.
  Moving the root side out is the D2 follow-up. Whatever a killed launch leaves behind is
  cleaned up by `fc_linux_profile.sh --reap ID`.

Why D2 rather than D1 as the target: D2 keeps S's best property (the root side exists only
for one launch), removes S's inherited-state class wholesale, lets Fabric run
`NoNewPrivileges`, gives every launch a systemd audit record, and reuses code the custodian
already has (activation, `SO_PEERCRED`, a mode stated in every reply). D1's always-on root
loop buys nothing over D2 here.

If the operator prefers to switch now rather than later, the change is: a `--socket` entry in
`axon-protected-launcher` that takes the request from the activated connection and
authenticates `SO_PEERCRED` instead of `getuid`; a template unit pair; Fabric connecting
instead of executing; and preflight checks for socket mode and group instead of setuid. Each
needs new rows and review, and the boot test's `helper` cases must be redone. Estimate: one
workstream, plus a re-run of the helper rows.
