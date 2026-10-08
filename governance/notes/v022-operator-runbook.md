# Operator runbook: deploying the v0.22 protected host (Candidate C9)

This is the ordered procedure that takes the protected host from nothing to a
`protected_verifier_ready.py` verdict. Steps marked **OPERATOR** hold keys or make decisions,
and no script does them. Everything else is done by `scripts/operator_deploy_protected_host.sh`
(called "the kit" below), which is dry-run by default.

Ground truth: `governance/specs/v022-psv-protocol.md` amendments 44, 45, 50, 54, 56, 57 and
61-66 ("Operator deployment"); `governance/status/v022-psv-protocol.json`
(`candidate_9.operator_items_before_protected_deployment`); the code named in each step. The
setuid-vs-daemon decision is in `governance/notes/v022-setuid-vs-daemon.md`.

## What the kit does and does not do

It installs, from one standalone clone at one commit:

- the provenance allowlist;
- the Fabric, custodian, observer, verifier and launch-profile users (`axonb263`), five
  different uids (the observer is its own principal, amendment 68);
- the directories, including the observer's record store and key directory
  (`/var/lib/axon-observer/{observed,key}`, its own, 0700);
- `axon-fabric`, the setuid-root `axon-protected-launcher` (04750 root:axon-fabric),
  `axon-custodian`, `axon-observer` (the observer SERVICE) and `fc_linux_profile.sh`, under
  `/usr/local/libexec/axon/`;
- the guest image under `/usr/local/lib/axon/guest-linux/`;
- the suite and grant registries;
- the signed B263 record and waivers, once the operator hands them over;
- `/etc/axon/custodian.json`, `/etc/axon/observer.json`, `/etc/axon/protected-launcher.json`
  and `/etc/axon/protected-host.json`, every pin computed from the INSTALLED bytes. That
  includes the helper config's `custodian.sha256`, the sha256 of the installed
  `axon-custodian` (amendment 65), and its `observer.service {socket, uid, sha256}`, the
  installed `axon-observer` (amendment 68). The helper refuses a production config without
  the first, relays an observation only from the second, and on every reply it checks that
  the sending process executes exactly that program. It also writes the helper config's
  `fabric {path, sha256, revision}` (amendment 79): the installed `axon-fabric`, by path, sha256
  and the build revision THAT FILE states when it describes itself (`verifier-manifest`), never a
  value you type. The helper serves a launch or an observation only for a caller running that
  program, and the observer names it (and that revision) as the verifier. `custodian.json` gains
  `observer_uid`, the one uid the custodian tells whether a nonce was issued. The host config's
  `observer` section names NO `command`: a production Fabric refuses one;
- `/etc/axon/trust/verifier.json`;
- the custodian's and the observer's systemd units, with both sockets enabled (the custodian's
  socket unit carries `ExecStartPost=setfacl -m u:axon-observer:rw`, which needs the `acl`
  package: the observer service asks the custodian whether a nonce was issued, as its own uid);
- `/etc/axon/host-toolchain-pin.json`, which the freeze reads (amendment 65).

Then it runs the production `ProtectedHost::operator()` as the Fabric uid against the result.
It judges the operator's Fabric unit (`--fabric-unit`) for `NoNewPrivileges` and for anything
that implies it. It verifies that the helper config's custodian pin is the installed
`axon-custodian`, which is also the program the custodian unit starts; a mismatch FAILS (exit
1). It does the same for the observer: `observer.service` must pin the installed
`axon-observer`, name the socket and uid of `observer.json` and of the installed units, the
unit must start that program as the observer user, and the observer's socket must be root's
alone (`SocketMode=0600`); a host config naming `observer.command`, an observer config naming
the Fabric as the observer or a caller other than root FAIL. It BLOCKS an observer key that
is absent, not owned by the observer uid, readable by another uid, or in a directory that is
not the observer's 0700. It checks that the kernel has `SO_PASSPIDFD` (Linux 6.5 or later).
Finally it runs `trust_root_preflight.sh` in protected mode with `--fabric-pid` and
`--observer`.

It **never** generates a key, reads a private key, or signs anything. A missing key, trust
root or signed record is reported as `BLOCKED` or `PENDING`, with the command that fixes it.
The kit is idempotent: re-run it after each operator step. It exits 3 until nothing is left.

Example configs matching what it writes are in `profiles/protected-host/*.example`.
`crates/axon-fabric/tests/operator_examples.rs` loads the launcher and custodian examples
through the production loaders, so the examples cannot drift from the code. The kit's test is
`scripts/test_operator_deploy.sh`. It covers the dry run on the real host and a full `--apply`
inside a private mount namespace with a shadow `/etc` (`scripts/lib/opkit_ns.sh`, `ns_run`: the boundary that matters is that namespace and the helper's proof, not a text check). `scripts/opkit_ns_drift.py` is a BEST-EFFORT SECOND LAYER that flags the 145 shapes its `--selftest` lists (and the mentions it derives from them) when a test script runs the kit outside `ns_run`; it is not a deny-by-default guarantee: a test script that wants to evade a textual gate can, and the gate catches mistakes, not intent. Its namespace run passes the production
loader, the freeze's toolchain-pin reader and the protected-mode preflight. It also shows that
a Fabric process under `NoNewPrivileges` fails the preflight, and that a wrong custodian pin
fails the kit's check. For the observer service (amendment 68) it holds a control (the full
deployment passes, every actor but the observer FAILS to open the key and to connect to the
socket by a real attempt) against attacks that each break one fact: `--observer-bin`, an
observer user that is the Fabric's or the custodian's, a key another uid can read or the
Fabric owns, a key directory the group can enter, an absent key, a wrong or missing program
pin, another socket or uid in the helper config, `observer.command` in the host config
(refused by the production loader as well), an observer config naming the Fabric as the
observer, a caller that is not root, or another store, a socket unit the Fabric group can
connect to, and, in the preflight itself, an observer key or socket every uid can reach and a
record store the Fabric owns.

## 0. Before you start (OPERATOR decisions)

- **Setuid helper or daemon: DECIDED (operator decision F, 2026-10-04).** Keep the setuid
  helper for C9, under the memo's conditions C1-C3. Where they stand:
  - C1 (`harden()` evidenced) is met in code: amendment 65 rows M1474-M1482, and amendment 66
    row M1487 (the environment clear).
  - C2 (no `NoNewPrivileges` on Fabric) is the operator's, checked twice. The kit judges the
    Fabric unit's text (step 4a). The preflight judges the running process through
    `--fabric-pid` (step 8).
  - C3 (cgroup) is decided in amendment 65: the root launch stays in Fabric's cgroup, and the
    VMM gets its own jailer cgroup. **OPERATOR**: size the Fabric unit's `MemoryMax=` and
    `TasksMax=` for the helper, the launcher and the host tools it runs. A launch killed by
    those limits is a failure or unknown attempt, never a verdict.
- **The observer: DECIDED (operator decisions G and G1 = A; amendment 68).** The observer is the
  `axon-observer` SERVICE: its own uid (default user `axon-observer`, never the Fabric's, the
  custodian's, the verifier's, an agent's or root; no login shell, no home), socket-activated by
  a root-only socket, reached only through the setuid-root helper's `--observe` relay. It
  signs only what it MEASURED (the installed launcher, firecracker, guest image, suite
  registry, B263 record, profile manifest and host config), what you PINNED (the Fabric program
  and its build revision, helper config `fabric`) and what the measured profile manifest names
  (the guest's init and axon digests), for a nonce the custodian issued, once per nonce. The
  guest policy digest and the authority epoch are the principal's word (amendment 79). Your decisions: the observer uid, the key
  (step 5: generated AS that uid on the host, never copied off, never in a group the Fabric is
  in) and the unit installation (the kit installs `axon-observer.socket` and `.service` from
  `profiles/protected-host/systemd/`, enabled). `--observer-bin` and `--observer-interpreter`
  no longer exist: the kit REFUSES them, because an observer PROGRAM runs as the Fabric uid
  and a production Fabric refuses a host config that names one.
- **Out root and staging root placement** (amendment 45). The kit places them at
  `/var/lib/axon-fabric/runs` (Fabric's, 0700) and `/var/lib/axon-protected-launcher`
  (root, 0700). Decide whether they may share a filesystem with anything else.
- **The compiled-launcher follow-up** (amendment 45): whether the launcher script's
  unpinned host tools may stay in the root TCB.
- **The root-owned listener** (status item): the custodian's clients accept a listener bound
  by the custodian uid or by root (PID 1, socket activation). Since amendment 65 the helper
  also checks the PROGRAM that sends each reply against `custodian.sha256`, so only the
  pinned `axon-custodian` can spend a nonce, whoever bound the socket. Fabric's own
  issue call is still authenticated by uid only, because the pin is checked where the nonce
  is spent. Review that.
- **Which uids are agents.** You need every uid MiCode or Claude runs as. The preflight
  probes them.

## 1. A standalone clone at the freeze commit, and the allowlist first

```bash
git clone <axon repo> /srv/axon-freeze && cd /srv/axon-freeze
git checkout --detach <FREEZE_SHA>          # a full 40-hex id
test -d .git && ! test -L .git              # a real .git: never a linked worktree (decision E)
sudo bash scripts/operator_deploy_protected_host.sh --from /srv/axon-freeze --only allowlist           # review
sudo bash scripts/operator_deploy_protected_host.sh --from /srv/axon-freeze --only allowlist --apply
```

The allowlist goes first because two later outputs land in the tree: the guest build's
`dist/`, and an in-tree `target/`. Without the allowlist both make the tree dirty under
decision C. Then the image manifest says `axon_tree_dirty_at_build: true` and Fabric refuses
it (RULE:manifest-clean), and the verifier reports `source_dirty: true` and readiness refuses
it. The kit refuses a clone that is dirty anywhere except `target/` and `dist/`, a linked
worktree, a symlinked `.git`, a clone that borrows objects, and a copy of itself that is not
the clone's own.

## 2. Full controlled guest image build (not `--rootfs-only`)

```bash
cd /srv/axon-freeze
AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh
```

- Run the **full** build. A `--rootfs-only` build has no `kernel-build.json`, and the freeze
  refuses it (amendment 63). The build needs network for its fresh `CARGO_HOME`. It runs
  cargo on a private copy of the tracked tree under `~/.cache/axon-guest-build` (the builder
  must own that parent, and it must not be group/other-writable). It records every tool's
  identity.
- Leave `RUSTC_WRAPPER` and sccache unset. The freeze refuses a build that used them
  (amendment 52).
- The build rewrites `profiles/linux-microvm/manifest.json`. Commit that re-pin through the
  integrator's freeze procedure. Deploy from the commit that carries it. The kit refuses
  unless `dist/guest-linux/manifest.json` is byte-identical to the committed copy, both
  artifacts match their pins, the manifest is clean, both build records are present, and the
  engine pins match `/usr/local/bin/firecracker` and `jailer`. **OPERATOR**: install the
  pinned Firecracker v1.10.1 `firecracker` and `jailer` there, root-owned. The kit does not
  install them.
- `scripts/v022_freeze_manifest.py` remains the judge of the whole image (`shape_problems`,
  `image_problems`). Run it as the freeze procedure says.
- **The freeze needs the host-toolchain pin (amendment 65).** The freeze refuses without
  `/etc/axon/host-toolchain-pin.json` on the host that runs it. It also refuses when the pin
  is not root-owned and closed to group/other writes along its whole path, and when any host
  tool the image's build records name is not pinned at the same path and digest. Once the
  re-pin is committed, write the pin with the kit before freezing:
  `sudo bash scripts/operator_deploy_protected_host.sh --from /srv/axon-freeze --only toolchain --apply`.
  The kit computes the pin with the freeze's own extraction
  (`guest_build_env.recorded_host_tools`). After installing it, the kit runs the freeze's
  reader (`guest_build_env.py toolchain-pin`), which must accept it. If the freeze runs on
  another host, install the same file there. **OPERATOR**: these digests are what the build
  recorded. Before you pin them, check them against your distribution's packages (the kit
  prints any `DRIFT` since the build).

## 3. Host binaries: the CONTROLLED host build (amendment 86)

```bash
cd /srv/axon-freeze
# as ROOT, the builder (the kit is told with --builder-uid 0 --build-uid N; AXON_GUEST_BUILD_PARENT must be
# the --builder-parent, the same private directory the guest build records live under)
sudo env AXON_GUEST_BUILD_PARENT=/var/lib/axon-guest-build \
  python3 scripts/guest_build_env.py host-build /var/lib/axon-build/host
```

`host-build` makes a fresh standalone clone of the HEAD, a fresh empty target dir and
`CARGO_HOME`, runs the one fixed `cargo build --release --locked -p axon-fabric --bins` in a
constructed environment (your `RUSTC_WRAPPER`, flags, `PATH` and the like are dropped, not
refused), and writes `host-build.json` beside the four binaries, signed by the builder. It
builds from nothing every time (minutes; the network for crates). The output directory must
not exist.

**Amendment 90 (round 7): the controlled builds run as ROOT and start every build process
as an unprivileged uid.** Run `host-build` and `build-guest-image.sh` with `sudo`. Install
the pinned toolchain as root (every file root-owned, none group/other-writable: the build
refuses otherwise). Use a build parent every uid can traverse
(`AXON_GUEST_BUILD_PARENT`, default `/var/lib/axon-guest-build`; never under root's
`0700` home) and give the build its own unprivileged uid (`AXON_GUEST_BUILD_UID`, default
65534; do not share it with another service). Tell the kit the same three facts with
`--builder-uid 0 --builder-parent DIR --build-uid N`, and name the commit to deploy with
`--expect-commit SHA` (required for `--apply`). Images and host builds made before this
amendment are refused: rebuild them.

**Migration.** A `--bin-dir` that is the output of a plain `cargo build` (no
`host-build.json`) is REFUSED. Pass `--builder-uid N --builder-parent DIR` to every kit run
that judges binaries or the guest: they are your word on who builds (the kit installs them as
`/etc/axon/builder-pin.json`, which the freeze reads and refuses without), and a build record
is judged against them, never against what it says. The kit checks:

- the record is signed by that builder, names exactly the four binaries and the clone's HEAD,
  and every binary is the bytes it names (the directory is copied aside first);
- its linker (`cc`, `ld`) is the one the guest build recorded and, once installed, your
  toolchain pin;
- `axon-fabric` is a clean production release build of the clone's HEAD;
- `axon-protected-launcher --probe` reports `build: production`;
- `axon-custodian` and `axon-observer` refuse `--test-config`, as production builds do.

## 4. Dry run, review, apply

```bash
K=/srv/axon-freeze/scripts/operator_deploy_protected_host.sh
ARGS=(--from /srv/axon-freeze --expect-commit <FREEZE_SHA> --bin-dir /var/lib/axon-build/host --builder-uid 0 --builder-parent /var/lib/axon-guest-build --build-uid 65534
      --suite-registry /path/to/suites/registry.json    # cortex-check-registry/1; relative paths come along
      --grant-registry /path/to/grants/grants.json      # axon-fabric-grant-registry/1 + its grant files
      --signer-public-key <64 hex>                       # from step 5; omit on the first pass
      --fabric-unit <the unit that runs Fabric>.service  # step 4a; its MainPID is the preflight's --fabric-pid
      --agent <micode uid> --agent <claude uid> --guest-cmd '<in-guest probe, see step 6>')
bash "$K" "${ARGS[@]}"            # DRY RUN: every action, every file's exact bytes, owner and mode
sudo bash "$K" "${ARGS[@]}" --apply
```

Read the dry run, and check these in particular:

- the four configs, including `custodian.sha256`, `observer.service.sha256` and `fabric.sha256`
  in `protected-launcher.json`, which must be the sha256 the `BINARIES` line prints for the
  custodian, the observer and `axon-fabric` (and `fabric.revision` the commit you deploy), and `observer.json` (`observer_uid` the observer user's,
  `fabric_uid` the Fabric's, `caller_uid` 0); `protected-host.json`'s `observer` section must
  have no `command`;
- `OK[fabric-unit]`, or the `BLOCKED[fabric-unit]` reasons;
- the helper's line `install /usr/local/libexec/axon/axon-protected-launcher root:axon-fabric 4750`;
- the units (the custodian's `SocketMode=0660`, `SocketGroup=axon-fabric`; the observer's
  `SocketMode=0600`, `SocketGroup=root`, `User=axon-observer`);
- `PENDING` and `BLOCKED` at the end.

The first `--apply` creates the users, directories, binaries, image, data, units and
toolchain pin. It does **not** write the host and helper configs until the signer key exists:
those configs would name a key that is not there.

### 4a. The Fabric service unit (amendment 65; memo condition C2)

Fabric runs as `axon-fabric` (decision A) under a unit you own, the one that runs
`axon-fabric submit` for MiCode. Under `NoNewPrivileges` the kernel ignores the helper's
set-id bit. The helper then runs as the Fabric uid, and it refuses every launch, naming
NoNewPrivileges. Give the kit the unit with `--fabric-unit NAME.service`, which it reads
through `systemctl cat`, drop-ins included. Before installing the unit, you can instead pass
`--fabric-unit /path/to/file`. The kit BLOCKS a unit that has any of the following in
`[Service]`:

- `NoNewPrivileges=yes`, or `DynamicUser=yes` (which implies it);
- any of `SystemCallFilter=`, `SystemCallLog=`, `SystemCallArchitectures=`,
  `RestrictAddressFamilies=`, `RestrictNamespaces=`, `PrivateDevices=`,
  `ProtectKernelTunables=`, `ProtectKernelModules=`, `ProtectKernelLogs=`, `ProtectClock=`,
  `ProtectHostname=`, `MemoryDenyWriteExecute=`, `RestrictRealtime=`, `RestrictSUIDSGID=`,
  `LockPersonality=`. The classic `systemd.exec` rule makes each of these imply
  `NoNewPrivileges` for a non-root `User=`. Measured on this host's systemd 259 with
  `User=nobody`: only `NoNewPrivileges=yes` and `DynamicUser=yes` set `NoNewPrivs: 1`, and none
  of these do. The kit cannot know which systemd the unit will run under, so it refuses them
  all. Leave them out of the Fabric unit; most hardening presets set them;
- `PrivateUsers=` (in a user namespace the set-id bit does not make the helper the host's
  root), `SecureBits=noroot` or `no-setuid-fixup`, and any `CapabilityBoundingSet=` (it also
  bounds the root launch);
- a `User=` other than the Fabric user.

The ground truth is the running process. The preflight (step 8) reads
`/proc/<MainPID>/status` and FAILS unless it shows the Fabric uid and `NoNewPrivs: 0`. The
kit takes that pid from `systemctl show -p MainPID --value <unit>`, or from `--fabric-pid`.
Start the unit before the last pass. `setpriv --no-new-privs` and a container's
`no-new-privileges` have the same effect as the systemd setting, and only the pid check sees
them.

## 5. Provision trust roots and keys (OPERATOR custody; the kit only checks them)

Generate every private key on the machine where it will live. No private key ever enters
the repository, the clone, or an agent-writable path.

| What | Where | Owner / mode | Private half |
|---|---|---|---|
| Fabric attestation key (host signer) | `/etc/axon/keys/fabric-attest.pk8` | `axon-fabric:axon-fabric 0400`; `/etc/axon/keys` root 0755 | on the host, readable by the Fabric uid only (A20) |
| its public half | `/etc/axon/trust/verifier/host-signer.pub` | root 0644, dir root 0755 | — |
| B263/certification issuer (qualification) | `/etc/axon/trust/qualification/operator.pub` | root 0644, dir root 0755 | **offline**, operator only |
| observer key (public) | `/etc/axon/trust/observer/observer.pub` | root 0644, dir root 0755 | see the next row |
| observer key (private) | `/var/lib/axon-observer/key/observer.pk8` | `axon-observer:axon-observer 0400`; the directory `axon-observer` 0700, `/var/lib/axon-observer` root 0755 | generated AS the observer uid on the host (`sudo -u axon-observer … axon-fabric keygen`), readable by that uid alone; in no other trust root |
| admission, monitor | `/etc/axon/trust/{admission,monitor}/` | optional here | loop-side |

```bash
sudo /usr/local/libexec/axon/axon-fabric keygen --out /etc/axon/keys/fabric-attest.pk8   # prints public_key, fingerprint
sudo chown axon-fabric:axon-fabric /etc/axon/keys/fabric-attest.pk8                      # keygen writes it 0400
sudo install -d -o root -g root -m 0755 /etc/axon/trust/{qualification,observer,verifier}
# The observer key, AS the observer uid (the kit creates the user and its 0700 key directory
# first; keygen writes the key 0400 and prints public_key):
sudo -u axon-observer /usr/local/libexec/axon/axon-fabric keygen --out /var/lib/axon-observer/key/observer.pk8
echo <public_key> | sudo tee /etc/axon/trust/verifier/host-signer.pub >/dev/null
# On the OFFLINE operator machine:  axon-fabric keygen --out op-qualification.pk8
# then copy ONLY its public_key here:
echo <operator public_key> | sudo tee /etc/axon/trust/qualification/operator.pub >/dev/null
echo <the observer key's public_key> | sudo tee /etc/axon/trust/observer/observer.pub >/dev/null
sudo chmod 0644 /etc/axon/trust/*/*.pub
```

Every key's algorithm, file format, encoding rule, signed bytes and accepting loader is in
`governance/notes/v022-key-formats.md`, derived from the code. In short: write each public
key as exactly 64 lowercase hex characters. The `ed25519:<16 hex>` fingerprint is a label,
never a key. Keep the signing key at mode `0400`; Fabric refuses `0600`.

Rules the code enforces (ADR-002):

- One key serves one authority. A key present in two roots refuses both.
- The host signer's key may appear only in `verifier/`. In any other root it refuses that
  root, because Fabric holds its private half.
- Each `*.pub` holds 64 hex characters. One malformed file refuses its whole root.

The kit checks all of these. `AXON_ATTEST_KEY` (axon-vm's attestation key) is not used by
the Fabric protected route.

Re-run the kit with `--signer-public-key <64 hex>` and `--apply`. It writes the configs and
`verifier.json`, enables the socket, and runs the loader check. That check reports
`PENDING` until the B263 record is installed (step 7).

## 6. The trust preflight's guest probe

`trust_root_preflight.sh` needs a `--guest-cmd` that runs `scripts/trust_root_guest_probe.sh`
**inside a candidate guest** and prints its one JSON line. The preflight requires
`"addressable": false`.

**Decided (operator decision H): the project builds and boot-tests this command.** The probe
already runs in the real guest in `psv_guest_boot_test.sh` case `trust-probe` (amendment 65).
That case uses the launcher's plain mode under an Exec grant, with the probe put on the
workspace drive and run by busybox sh. The deployable `--guest-cmd` wrapper is not in this
tree yet. Until it lands, the kit reports the preflight as `PENDING`. A preflight that has
not run is not a pass, and readiness needs its protected-mode PASS report as certified
evidence.

## 7. B263 re-qualification and the operator's signature

The image changed, so qualification is PROTECTED_ONLY-open (amendments 54 and 63).

Run it **on the protected host itself**:

```bash
cd /srv/axon-freeze            # clean: the allowlist excuses dist/ and target/
sudo env -u RUSTC_WRAPPER CARGO_TARGET_DIR=/var/lib/axon-build/target \
  scripts/b263_qualify.sh --issuer-key-id ed25519:<16 hex fingerprint of the OPERATOR qualification key> \
    --host-label <your name for this host> [--caveat '<what the boundary excludes>']
# exit 3 = PASS_WITH_BLOCKED (expected: x3_l0_hypervisor_boundary and x4_trusted_evidence_issuer
#          are recorded BLOCKED unconditionally); 0 = PASS; 1 = FAIL; 4 = SKIP (not a result)
# evidence: /var/lib/axon-build/target/b263-evidence/<UTC>.json
```

- **The host is measured (amendment 65).** `scripts/b263_host.py` writes the record's
  `host`: your `--host-label`, then the measured hostname, `/etc/machine-id`,
  `systemd-detect-virt` and kernel. It also writes `host_facts.{hostname, machine_id, virt,
  wsl, host_label}` and binds `source.host_identity_sha256`. The `caveat` is your
  `--caveat`, or else one derived from the measured virtualization. Both go verbatim into
  every protected receipt (`qualification-host:`, `qualification-caveat:`), so state them as
  you want them read. Fabric requires a non-empty host and compares it to nothing. The kit
  holds the record's `machine_id` to this host's `/etc/machine-id`, and it reports `PENDING`
  for a record without the measured host or without a host label.
- `x3_l0_hypervisor_boundary`'s **reason text** in `b263_qualify.sh` still describes WSL2
  with nested KVM under Hyper-V, whatever host it runs on. Amendment 65 measured `host` and
  `caveat`, but not that assertion's reason. The text is part of the bytes you sign, so read
  it, and write the waiver's `reason` for your real host. This is a code follow-up (below).

- **What is signed:** the EXACT bytes of that evidence file. Do not reformat it.
- **Which key:** the operator's qualification key, the one whose public half is in
  `/etc/axon/trust/qualification/` and whose fingerprint was passed as `--issuer-key-id`.
  Fabric requires the signer to be the key the record names (RULE:issuer-claimed).
- **How**, on the offline machine:
  ```bash
  axon-fabric sign-evidence --record <UTC>.json --key op-qualification.pk8 --authority qualification
  # writes <UTC>.json.sig: axon-evidence-signature/2, domain "qualification"
  ```
- A `PASS_WITH_BLOCKED` record needs a waiver for **every** BLOCKED assertion (template
  below), signed the same way: `--record b263-waivers.json`, which produces
  `b263-waivers.json.sig`.
- Install both with the kit:
  `sudo bash "$K" "${ARGS[@]}" --b263-record <UTC>.json --b263-waivers b263-waivers.json --apply`.
  The kit copies each file with its `.sig` to `/etc/axon/qualification/` and points
  `qualification.waivers` at the waivers. The loader check then reads `OK`.
- Freshness: the record's `end` must be within `qualification.max_age_s` (the kit
  materialises 2592000 s, Fabric's default) both at each launch and at readiness. For
  readiness the record must also predate the protected run's observation (amendment 57).
  Qualify first, run second.

### Waiver template: UNSIGNED TEMPLATE, fill in and sign; never commit it as if signed

Schema from `crates/axon-fabric/src/backend.rs` (`WAIVER_SCHEMA`, `parse_waivers`,
`accept_b263`). These are every field the code reads:

```json
{
  "schema": "axon-b263-waiver/1",
  "evidence_sha256": "<sha256 of the EXACT bytes of the B263 record being waived: sha256sum <UTC>.json>",
  "waivers": [
    {
      "assertion": "x3_l0_hypervisor_boundary",
      "reason": "<why qualifying without this assertion is acceptable; non-empty, or the waiver is refused>",
      "expires": "<YYYY-MM-DDTHH:MM:SSZ, in the future; absent or unparseable = expired>"
    },
    {
      "assertion": "x4_trusted_evidence_issuer",
      "reason": "<e.g. the harness records this row BLOCKED unconditionally; the record IS bound to a trusted issuer by the operator's detached qualification signature verified under /etc/axon/trust/qualification>",
      "expires": "<YYYY-MM-DDTHH:MM:SSZ>"
    }
  ]
}
```

Rules:

- A waiver is bound to one record (RULE:waiver-bound) and cannot be transferred.
- Each BLOCKED assertion needs a reasoned, unexpired entry (RULE:blocked-unwaived,
  RULE:waiver-reason, RULE:waiver-expiry).
- The file is verified under the qualification root, at `<waivers>.sig` by Fabric and at
  the evidence path's `.sig` by readiness.
- List exactly the record's BLOCKED assertions. The two above are what `b263_qualify.sh`
  records BLOCKED unconditionally. Check the record's `assertions` for others.

## 8. The trust preflight (protected mode)

The kit runs it as its last step, once nothing is `BLOCKED` and `--agent`, `--guest-cmd` and a
Fabric pid are available:

```bash
bash /srv/axon-freeze/scripts/trust_root_preflight.sh --verifier axon-verifier --custodian axon-custodian \
  --fabric axon-fabric --observer axon-observer --agent <uid> [--agent <uid> …] --guest-cmd '<step 6>' \
  --fabric-pid "$(systemctl show -p MainPID --value <fabric unit>)" \
  --out /var/lib/axon-deploy/trust-preflight-<UTC>.json
```

Since amendment 65, protected mode requires `--fabric-pid` and refuses to run without it. Its
`no-new-privs` check FAILS unless that process runs as the Fabric uid with `NoNewPrivs: 0`.
The Fabric unit must be running when the preflight runs.

Since amendment 68, protected mode also requires `--observer` (the observer uid) and judges the
observer service by real attempts under each actor: the observer opens its key and creates in
its record store; the Fabric, the verifier, the custodian and every agent FAIL to open the
key, FAIL to create in or chmod the key directory or the store, and FAIL to connect to the
observer's socket (mode 0600 root:root: the helper, as root, is its only client). The
observer's config must name exactly that uid, the Fabric's uid and caller 0, and the helper
config's `observer.service` the same socket and uid; the observer must not be the Fabric, the
custodian, the verifier, an agent or root.

The verdict line reads `PREFLIGHT verdict PASS mode protected report …`. Keep that report: it
is certified evidence (`trust_preflight_sha256`).

## 9. A protected run, and the certification records

Per component (`protected_backend`, `g01_on_protected_backend`, `pci_on_protected_backend`),
readiness wants an operator-signed `governance/proofs/v022-protected/<component>.json`.
Certified records are immutable once committed.

**Gate rows and proof documents first.** None of the seven gates readiness requires is
registered in `governance/cortex_gate_execution_registry.json` today:

- `G13-r22-profile-qualification`, `G13-r22-profile-eligibility`, `G13-r22-guest-truth`;
- `G01-r22-registered-check`, `G01-r22-verifier-separation`;
- `G03-r22-trial-isolation`, `G03-r22-physical-isolation`.

Neither proof document exists either: `governance/proofs/v022-g01-microvm/REGISTRATION.md`
and `governance/proofs/v022-pci-microvm/CERTIFICATION.md`. Without them a valid
certification makes a component `PARTIAL`, not `PASS`.

**The run.** Run one protected trial through Fabric as the Fabric uid, on the deployed host,
after B263 and the preflight. Keep:

- the request file (`acf-compute-request/1`);
- the exact stdout of `axon-fabric submit` (`axon-fabric-submit/1`, with
  `receipt_attestation` and `psv_evidence`).

From `psv_evidence`, write the `observation` bytes to `observation.json` and
`observation_signature` to `observation.json.sig`, byte for byte.

**Evidence files** go in the repository beside the record. Each signed file keeps its `.sig`
next to it:

- the preflight report;
- `observation.json` + `.sig`;
- the B263 record + `.sig`;
- the waiver file + `.sig`;
- the submit stdout;
- the request.

### Certification record template: UNSIGNED TEMPLATE

All 22 `CERT_FIELDS` from `crates/axon-fabric/src/readiness.rs`; schema
`axon-v022-protected-certification/2`:

```json
{
  "schema": "axon-v022-protected-certification/2",
  "component": "<protected_backend | g01_on_protected_backend | pci_on_protected_backend>",
  "host_profile": "linux-microvm-protected",
  "qualification_profile": "linux-microvm-protected",
  "psv_spec_sha256": "<sha256 of governance/specs/v022-protected-suite-verdict.md at the certified tree>",
  "axon_sha": "<40-hex FREEZE_SHA; HEAD must descend from it with nothing outside governance/ changed>",
  "micode_sha": "<40-hex MiCode revision (operator-attested; nothing joins it)>",
  "fabric_revision": "<40-hex: the installed verifier's fabric_revision; must equal the observation's>",
  "guest_image_sha256": "<rootfs.sqfs sha256 = the observation's and the B263 record's>",
  "guest_kernel_sha256": "<vmlinux sha256>",
  "guest_runtime_sha256": "<guest axon sha256>",
  "suite": {"id": "…", "version": "…", "entry": "…", "test": "…", "digest": "<suite tree digest = the launch manifest's>"},
  "candidate_tree_ref": "<= the launch manifest's>",
  "observer_key_id": "<ed25519:… of the key that signed observation.json; in /etc/axon/trust/observer>",
  "observation_sha256": "<sha256 of observation.json>",
  "verifier_key_id": "<ed25519:… of the key that signed the receipt attestation; in /etc/axon/trust/verifier>",
  "b263_qualification_sha256": "<sha256 of the B263 record = the launch manifest's qualification_sha256>",
  "evidence": ["<repo-relative path>", "…"],
  "evidence_bundle_sha256": "<sha256 of the CONCATENATION of each evidence file's lowercase hex sha256, in list order>",
  "readiness_verifier_sha256": "<sha256 of /usr/local/libexec/axon/axon-fabric (= verifier.json sha256)>",
  "trust_preflight_sha256": "<sha256 of the protected-mode PASS preflight report>",
  "certified_at": "<YYYY-MM-DDTHH:MM:SSZ, not before the observation, not in the future>"
}
```

Sign it with the qualification key, on the offline machine:

```bash
axon-fabric sign-evidence --record <component>.json --key op-qualification.pk8 --authority qualification
```

This writes `<component>.json.sig` beside the record. If
`governance/status/trust-expectations.json` lists `qualification_issuers`, the signer must be
on that list.

## 10. Readiness, and the expected transition

Run it from the clone, as a **non-root** uid. Readiness refuses a trust root that the
process running it can write, and root can write everything:

```bash
sudo -u axon-verifier python3 /srv/axon-freeze/scripts/protected_verifier_ready.py
```

| State | `protected_backend` / `g01_on_protected_backend` / `pci_on_protected_backend` |
|---|---|
| today (dev host) | `NOT_RUN`: "operator verifier is not installed on this host" |
| after steps 4-5 (verifier pinned, roots present) | the installed verifier decides: `NOT_RUN`, missing the seven gate rows, the two proof documents and "no protected-host certification record" |
| certification records signed, gates still unregistered | `PARTIAL` (`protected_host_certification_issuer` set) |
| gates registered, proof documents present, records valid | `PASS` |

Overall `PROTECTED_VERIFIER_READY` moves from `NOT_READY` to `READY` only when all three are
`PASS` and the four other components (g01_authenticity, pci_isolation,
verification_binding, no_open_blocking_false_greens; `PASS` today) stay `PASS`.

Operator decision B still holds. Zero protected trials COUNT in the loop until an observed
execution path exists (amendment 44). Readiness and counting are separate questions.

## Cleaning Fabric's run directories after a crash

Fabric makes one directory per run under `<state>/runs/`, named
`<16 hex of sha256(operation)>-<pid>-<seq>[suffix]`, created new (never reused) and removed when
the run ends normally (`RunDir::drop`). A crash, `SIGKILL` or power loss skips the removal, and
nothing sweeps: leftovers accumulate (they hold the run's materialised candidate and suite inputs
and its outputs, so they are as sensitive as the run). There is no automatic cleanup. To clear them:

1. Stop Fabric (or confirm no submit is in flight).
2. Remove the entries of `<state>/runs/` whose `<pid>` field is not a live process. When Fabric is
   stopped, that is every entry: `rm -rf <state>/runs/*`.
3. Leave `<state>/runs/` itself in place.

Only the leftover directory is affected: a new run never reuses a name, so a stale directory cannot
change a later result (amendment 71). This is an availability and disk-space matter, not a trust one.

## Operational friction (not weakenings; all fail closed)

- The host build (`guest_build_env.py`, a fresh empty `CARGO_HOME` and `cargo build --locked`) needs
  crates.io access, or a committed source replacement, on the deploy host.
- Record keys and their directory must be owned by the pinned builder uid. A key tree copied to the
  deploy host must be `chown`ed to an account that exists there.
- Any commit that changes a file outside `governance/status/` after the mutation run makes the run
  stale (docs included) and forces a full rerun. Sequence: make the last doc change first, then run.
- A later commit under `crates/axon-core/src` fails the PCI drift test until it has a `THEMES` line
  in `scripts/pci_delta.py` and the note is regenerated (`python3 scripts/pci_delta.py --emit HEAD`).

## Follow-ups this deployment surfaced

Closed by amendments 65 and 66 (the gaps workstream), with the kit updated to match:

- The host-toolchain pin has a reader: the freeze requires it (step 2).
- `axon-custodian`'s bytes are pinned: `custodian.sha256` in the helper config, verified per
  reply message. A kernel without `SO_PASSPIDFD` (older than Linux 6.5) refuses every pinned
  call, and the kit BLOCKS such a kernel.
- `b263_qualify.sh` measures the host (step 7).
- `harden()` has rows (M1474-M1482, M1487), and the preflight sees `NoNewPrivileges` on the
  Fabric service (`--fabric-pid`; step 4a).
- Setuid or daemon: decided (F). The observer's boundary: decided (G). The guest probe
  command: ours (H).

Still open:

1. **The observer service** is built and the kit deploys and checks it (amendments 68, 79). What
   the observer MEASURES is the installed files; the Fabric program is your pin and its revision is the kit's word
   (the helper holds every caller to it); the guest's init and axon digests are what the
   measured profile manifest names; the nonce is the custodian's. The guest policy digest and
   the authority epoch are the principal's word (amendment 79). The verifier digest names the
   executable file the Fabric-uid caller had when the helper opened `/proc/<ppid>/exe`, not the
   instructions it runs. The plain exec-after-spawn route (executed 18 of 20 by the round-6
   reviewer) is refused since amendment 85: no process but the helper and its parent may hold the
   reply pipe, and a production helper refuses a non-pipe stdout. That is a configuration and
   mistake guard, not a defence against malicious same-uid code: reopening the pipe via
   `/proc/<pid>/fd/N` of the parent, `SCM_RIGHTS`, `pidfd_getfd`, `ptrace` and `LD_PRELOAD` remain.
   The Fabric revision in the helper config is told by the kit, not pinned (amendment 85).
2. **The deployable `--guest-cmd`** (decision H, step 6). The preflight stays `PENDING` until it
   exists.
3. **`x3_l0_hypervisor_boundary`'s reason text** in `b263_qualify.sh` is still the WSL2/Hyper-V
   constant (step 7).
4. **Gate rows and proof documents** for the three protected components are not registered or
   written (step 9).
5. **Toolchain digests**: the pin attests what the build recorded. Nothing independent says
   those are the distribution's binaries (step 2).
6. **The root launch shares Fabric's cgroup** (C3, decided). Size `MemoryMax=`/`TasksMax=`
   accordingly. Moving it out is the memo's D2 follow-up.
