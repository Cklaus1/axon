# Operator runbook: deploying the v0.22 protected host (Candidate C9)

This is the ordered procedure that takes the protected host from nothing to a
`protected_verifier_ready.py` verdict. Steps marked **OPERATOR** hold keys or make decisions,
and no script does them. Everything else is done by `scripts/operator_deploy_protected_host.sh`
(called "the kit" below), which is dry-run by default.

Ground truth: `governance/specs/v022-psv-protocol.md` amendments 44, 45, 50, 54, 56, 57 and
61-63 ("Operator deployment"); `governance/status/v022-psv-protocol.json`
(`candidate_9.operator_items_before_protected_deployment`); the code named in each step. The
setuid-vs-daemon decision is in `governance/notes/v022-setuid-vs-daemon.md`.

## What the kit does and does not do

It installs, from one standalone clone at one commit:

- the provenance allowlist;
- the Fabric, custodian, verifier and launch-profile users (`axonb263`);
- the directories;
- `axon-fabric`, the setuid-root `axon-protected-launcher` (04750 root:axon-fabric),
  `axon-custodian`, `fc_linux_profile.sh` and the operator's observer program, under
  `/usr/local/libexec/axon/`;
- the guest image under `/usr/local/lib/axon/guest-linux/`;
- the suite and grant registries;
- the signed B263 record and waivers, once the operator hands them over;
- `/etc/axon/custodian.json`, `/etc/axon/protected-launcher.json` and
  `/etc/axon/protected-host.json`, every pin computed from the INSTALLED bytes;
- `/etc/axon/trust/verifier.json`;
- the custodian's systemd units, with the socket enabled;
- `/etc/axon/host-toolchain-pin.json`.

Then it runs the production `ProtectedHost::operator()` as the Fabric uid against the result,
and finally `trust_root_preflight.sh` in protected mode.

It **never** generates a key, reads a private key, or signs anything. A missing key, trust
root or signed record is reported as `BLOCKED` or `PENDING`, with the command that fixes it.
The kit is idempotent: re-run it after each operator step. It exits 3 until nothing is left.

Example configs matching what it writes are in `profiles/protected-host/*.example`. The test
is `scripts/test_operator_deploy.sh`. It covers the dry run on the real host and a full
`--apply` inside a private mount namespace with a shadow `/etc`. Its namespace run passes the
production loader and the protected-mode preflight.

## 0. Before you start (OPERATOR decisions)

- **Setuid helper or daemon** (memo). The kit deploys what the code implements: the setuid
  helper. If you keep it, meet the memo's conditions C1-C3. In particular, the process that
  runs Fabric must not have `NoNewPrivileges`. With it, the helper refuses every launch.
- **The observer program and its key custody.** No production observer ships in this
  repository. Fabric runs the observer **as the Fabric uid**, so an observer that holds its
  signing key where the Fabric uid can read it lets Fabric mint observations. This is a
  recorded follow-up. Decide the observer's boundary: a setuid observer helper or an
  observer service.
- **Out root and staging root placement** (amendment 45). The kit places them at
  `/var/lib/axon-fabric/runs` (Fabric's, 0700) and `/var/lib/axon-protected-launcher`
  (root, 0700). Decide whether they may share a filesystem with anything else.
- **The compiled-launcher follow-up** (amendment 45): whether the launcher script's
  unpinned host tools may stay in the root TCB.
- **The root-owned listener** (status item): the custodian's clients accept a listener bound
  by the custodian uid or by root (PID 1, socket activation). Review that.
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

## 3. Release binaries from the same clone

```bash
cd /srv/axon-freeze
env -u RUSTC_WRAPPER CARGO_TARGET_DIR=/var/lib/axon-build/target \
  cargo build --release --locked -p axon-fabric --bins       # production: no test-trust-root feature
/var/lib/axon-build/target/release/axon-fabric verifier-manifest   # build production, profile release,
                                                                    # source_dirty false, fabric_revision = HEAD
```

A target dir outside the tree is the simplest choice. The kit checks all of the following
through the verifier's own report:

- `axon-fabric` is a clean production release build of the clone's HEAD;
- `axon-protected-launcher --probe` reports `build: production`;
- `axon-custodian` refuses `--test-config`, as a production build does.

## 4. Dry run, review, apply

```bash
K=/srv/axon-freeze/scripts/operator_deploy_protected_host.sh
ARGS=(--from /srv/axon-freeze --expect-commit <FREEZE_SHA> --bin-dir /var/lib/axon-build/target/release
      --observer-bin /path/to/operator-observer        # [--observer-interpreter FILE] if it is a script
      --suite-registry /path/to/suites/registry.json    # cortex-check-registry/1; relative paths come along
      --grant-registry /path/to/grants/grants.json      # axon-fabric-grant-registry/1 + its grant files
      --signer-public-key <64 hex>                       # from step 5; omit on the first pass
      --agent <micode uid> --agent <claude uid> --guest-cmd '<in-guest probe, see step 6>')
bash "$K" "${ARGS[@]}"            # DRY RUN: every action, every file's exact bytes, owner and mode
sudo bash "$K" "${ARGS[@]}" --apply
```

Read the dry run, and check these in particular:

- the three configs;
- the helper's line `install /usr/local/libexec/axon/axon-protected-launcher root:axon-fabric 4750`;
- the units (`SocketMode=0660`, `SocketGroup=axon-fabric`);
- `PENDING` and `BLOCKED` at the end.

The first `--apply` creates the users, directories, binaries, image, data, units and
toolchain pin. It does **not** write the host and helper configs until the signer key exists:
those configs would name a key that is not there.

## 5. Provision trust roots and keys (OPERATOR custody; the kit only checks them)

Generate every private key on the machine where it will live. No private key ever enters
the repository, the clone, or an agent-writable path.

| What | Where | Owner / mode | Private half |
|---|---|---|---|
| Fabric attestation key (host signer) | `/etc/axon/keys/fabric-attest.pk8` | `axon-fabric:axon-fabric 0400`; `/etc/axon/keys` root 0755 | on the host, readable by the Fabric uid only (A20) |
| its public half | `/etc/axon/trust/verifier/host-signer.pub` | root 0644, dir root 0755 | — |
| B263/certification issuer (qualification) | `/etc/axon/trust/qualification/operator.pub` | root 0644, dir root 0755 | **offline**, operator only |
| observer key | `/etc/axon/trust/observer/observer.pub` | root 0644, dir root 0755 | the observer's own custody (see step 0) |
| admission, monitor | `/etc/axon/trust/{admission,monitor}/` | optional here | loop-side |

```bash
sudo /usr/local/libexec/axon/axon-fabric keygen --out /etc/axon/keys/fabric-attest.pk8   # prints public_key, fingerprint
sudo chown axon-fabric:axon-fabric /etc/axon/keys/fabric-attest.pk8                      # keygen writes it 0400
sudo install -d -o root -g root -m 0755 /etc/axon/trust/{qualification,observer,verifier}
echo <public_key> | sudo tee /etc/axon/trust/verifier/host-signer.pub >/dev/null
# On the OFFLINE operator machine:  axon-fabric keygen --out op-qualification.pk8
# then copy ONLY its public_key here:
echo <operator public_key> | sudo tee /etc/axon/trust/qualification/operator.pub >/dev/null
echo <observer public_key> | sudo tee /etc/axon/trust/observer/observer.pub >/dev/null
sudo chmod 0644 /etc/axon/trust/*/*.pub
```

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
**inside a candidate guest** and prints its one JSON line.

**There is no ready-made harness for this in the repository.** The guest runs Axon programs
(`fc_linux_profile.sh --program`), not shell scripts.

**OPERATOR**: supply a command that boots the pinned image with the probe and prints its last
line. One possible shape, unverified: an Exec-granted Axon program put on the workspace drive
with `--put` that runs `/bin/sh probe.sh /etc/axon/trust`, launched through
`fc_linux_profile.sh` with `profiles/linux-microvm/fixtures/policy-io-exec.json`. The
preflight requires `"addressable": false`.

Until a command exists, the kit reports the preflight as `PENDING`. A preflight that has not
run is not a pass, and readiness needs its protected-mode PASS report as certified evidence.

## 7. B263 re-qualification and the operator's signature

The image changed, so qualification is PROTECTED_ONLY-open (amendments 54 and 63).

```bash
cd /srv/axon-freeze            # clean: the allowlist excuses dist/ and target/
sudo env -u RUSTC_WRAPPER CARGO_TARGET_DIR=/var/lib/axon-build/target \
  scripts/b263_qualify.sh --issuer-key-id ed25519:<16 hex fingerprint of the OPERATOR qualification key>
# exit 3 = PASS_WITH_BLOCKED (expected: x3_l0_hypervisor_boundary and x4_trusted_evidence_issuer
#          are recorded BLOCKED unconditionally on this host); 0 = PASS; 1 = FAIL; 4 = SKIP (not a result)
# evidence: /var/lib/axon-build/target/b263-evidence/<UTC>.json
```

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
- `b263_qualify.sh` hard-codes `"host": "WSL2-nested"` and a Hyper-V caveat. On any other
  host the record would misstate where it ran. Fabric accepts any non-empty host, so nothing
  refuses it. Fix the script before qualifying a non-WSL2 host.
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

The kit runs it as its last step, once nothing is `BLOCKED` and `--agent` and `--guest-cmd`
are given:

```bash
bash /srv/axon-freeze/scripts/trust_root_preflight.sh --verifier axon-verifier --custodian axon-custodian \
  --fabric axon-fabric --agent <uid> [--agent <uid> …] --guest-cmd '<step 6>' \
  --out /var/lib/axon-deploy/trust-preflight-<UTC>.json
```

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

## Follow-ups this deployment surfaced (not done by the kit)

1. **No reader for the host-toolchain pin.** The kit writes `/etc/axon/host-toolchain-pin.json`
   (`axon-host-toolchain-pin/1`, marked PROPOSED). It records rustc, cargo, make, gcc, cc1,
   as, ld, mksquashfs, the other recorded kernel tools, and the cargo host linker, as the
   image's build records state them, plus any drift on this host now. Nothing reads it.
   Proposed reader: `v022_freeze_manifest.py` (and `guest_build_env.py`'s `image_problems`)
   walk the pin from `/` operator-owned, like the allowlist, and refuse a manifest whose
   `source.build_environment.toolchain`, `host_tools`, `rootfs.tool` or
   `kernel.build_environment.tools` digests differ from it. A missing pin refuses in protected
   mode. This is protected code with its own rows. Busybox is guest-side and already pinned by
   `kernel.pin`.
2. **Nothing pins `axon-custodian`'s bytes.** The unit starts it by path. `custodian.json`
   cannot carry a digest (unknown fields are denied). Only root ownership protects it. The kit
   records its sha256 in `/var/lib/axon-deploy/deploy-*.json`. A reader would need a code
   change, for example the helper verifying the custodian's executable through
   `/proc/<peer pid>/exe` against a pin in its own config.
3. **The observer runs as the Fabric uid** (recorded follow-up; step 0). A production observer
   with a separate key boundary is needed.
4. **No in-guest harness for `--guest-cmd`** (step 6).
5. **`b263_qualify.sh` hard-codes the host string** (step 7).
6. **`harden()` has no tests or rows, and the preflight does not see `NoNewPrivileges` on the
   Fabric service** (memo C1, C2).
7. **Gate rows and proof documents** for the three protected components are not registered or
   written (step 9).
