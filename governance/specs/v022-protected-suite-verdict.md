# v0.22 Protected suite verdict (microVM) — acceptance surface

Status: **DRAFT, frozen before implementation** (operator direction, 2026-09-27). This document
defines what the microVM suite-verdict path must do and how it is judged. It does not certify
anything.

## The rule this workstream lives under

> **Development-host protocol tests may prove the mechanism. They must never satisfy any of the
> three remaining protected-readiness components.**

The development host (this WSL machine, ADR-001 D1) builds the protocol and tests it
adversarially. Only the PROTECTED host certifies it.

The rule is enforced by `scripts/protected_verifier_ready.py`, not by convention.

### The trust root is the operator's, outside any repository

Neither WHICH keys are trusted, nor WHAT verifies a signature, nor WHAT DECIDES readiness may come
from this repository. Otherwise an agent with repository write access could plant a key, sign a
fake qualification, or simply edit the readiness script to print PASS.

```
operator root trust
      │
      └── /etc/axon/trust/              root/custodian-owned, never agent-writable
             ├── qualification/         B263 records and protected-host certifications
             ├── observer/              ADR-002 preflight observer keys
             ├── verifier/              Fabric verifier keys
             ├── admission/             admission / transition authority
             └── verifier.json          axon-verifier-manifest/1: pins the installed axon-fabric

repository  →  evidence only (records, proofs, gate rows)
            →  governance/status/trust-expectations.json may NARROW accepted key ids, never add one
```

One trust source per authority. A key trusted for one purpose never becomes valid for another,
and this is enforced in the SIGNATURE, not only by the directory the key sits in:
`axon-evidence-signature/2` signs `"axon-evidence-signature/2\n<authority>\n" + bytes` and names
its `domain`. A verifier expecting one authority refuses a signature for another
(`RULE:authority-domain`), and relabelling the domain breaks the signature. `/1` (bytes alone) is
refused; nothing operator-signed under it exists.

**Protected runtime (Fabric).** The protected profile's B263 evidence is trusted only under
`/etc/axon/trust/qualification/` (`QualificationTrust::operator()`, `TrustAuthority`):
- a fixed, absolute host path;
- every path component, from `/` down to each key, root-owned, not group- or other-writable, and
  not a symlink;
- the record's `issuer_key_id` equal to the key that verified it (`RULE:issuer-claimed`;
  `b263_qualify.sh --issuer-key-id`);
- a missing or unreadable root is NOT QUALIFIED, never a fallback;
- `axon-fabric submit` refuses `--linux-trusted-issuers`;
- no repository path, CLI argument, environment variable, workspace file or candidate config can
  replace or extend the set.

Test roots exist only behind the Cargo feature `test-trust-root`: `for_manifest`,
`check_operator_owned_below` and `ReadinessTrust::test`. A production build (`cargo build`, no
dev-dependencies) does not contain them.

**Readiness, the authority.**
- `protected_backend`, `g01_on_protected_backend` and `pci_on_protected_backend` are decided by
  `axon-fabric verify-readiness --repo DIR` (`axon_fabric::readiness`), run from the
  **operator-installed** binary pinned in `/etc/axon/trust/verifier.json`.
- That binary reads the repository as evidence only, and takes authority solely from
  `/etc/axon/trust/qualification/`, which must also be unwritable by the process running it.
- It reports its own identity with every verdict (`verifier`): the sha256 of its executable,
  `build` (`production` | `test-trust`), `fabric_revision`, `source_dirty`, `rustc`, `profile`
  and `target` (build.rs provenance).
- A certification binds the verifier that made it (`readiness_verifier_sha256`); a different
  binary deciding later refuses it. A production verifier built from a dirty tree refuses every
  certification.
- The relay requires the self-report to equal `verifier.json` field for field, and a clean
  `production` `release` build over `/etc/axon/trust/qualification` (see the runbook).
- `scripts/protected_verifier_ready.py` merely relays that verdict. Editing it, or any repository
  file, cannot turn the operator verifier's verdict to PASS.
- On a protected host, the custodian's launcher (operator-owned, ADR-001 D6) runs the pinned
  verifier before any protected Stage 7 / CX-21 run. The repository entry points are advisory.
- Gate registration rows and unsigned proof documents are necessary but NOT sufficient.

**Adversarial proof** (`crates/axon-fabric/tests/readiness.rs`). From a genuinely certified
repository, none of these keeps `protected_backend` PASS, each for its stated reason:
- the agent signs with its own key, plants it and lists it as expected;
- narrowing the expected issuers;
- changing source, scripts, manifests or local keys, committed or not;
- changing the spec, the evidence or the record;
- re-labelling the record for another component;
- a certification of another history;
- a trust root that is world-writable, not root-owned or symlinked;
- the production root being absent on this host.

A governance-only change keeps the certification.

### Two layers, never merged

| Layer | Made of | Readiness credit |
|---|---|---|
| `PSV_PROTOCOL_PROVEN` | dev-host tests, Firecracker/KVM dev execution, the negative matrix, mutations | **none** |
| `PROTECTED_BACKEND_CERTIFIED` | operator-trusted issuer, a qualified protected host, the observer deployed, signed evidence over exact revisions and digests | the three components |

Even when every PSV dev test passes, the three components stay NOT_RUN until the protected-host
certification exists.

Operator qualification steps are separate from coding, and are never reported as protected
success by the implementation:
- the B263 signature (S3-6);
- installing the trusted issuer key;
- deploying the preflight observer (ADR-002).

## Architecture

```
operator suite + candidate
        ↓
sealed guest inputs            (PSV-2)
        ↓
preflight observer attests the launch   (PSV-6, ADR-002)
        ↓
Firecracker guest              (PSV-7)
        ↓
trusted guest runner           (PSV-1, PSV-3)
        ↓
PCI completion proof           (PSV-3)
        ↓
guest verifier verdict         (PSV-4)
        ↓
Fabric protected receipt       (PSV-4, PSV-5)
        ↓
G01 authentication             (PSV-5)
        ↓
binding / intake               (the registered verification-binding gates)
```

## Acceptance surface

Each clause names what must hold. The negative matrix below names how each one fails closed.

**PSV-1 — The operator's suite, not the candidate's checks.**
- The guest executes the operator-registered suite (`check_registry`, pinned by WorkspaceVersion
  and entry) and exactly the task's registered acceptance test (`task_acceptance`).
- Candidate bytes can never define, add or select the rubric (G01-r22-verifier-separation, in the
  guest).

**PSV-2 — Separately sealed, digest-bound inputs.**
- Candidate tree and suite tree are delivered to the guest as two separately sealed inputs, each
  bound by digest (candidate WorkspaceVersion; suite id@version#entry).
- The guest runner refuses to start if either digest differs from what the launch manifest names.
- The candidate reaches the suite only as a module path, and is sealed per PCI (E0004).

**PSV-3 — PCI inside the guest.**
- The affirmative completion token (a per-run key and a token per completed test) is derived
  from a host-generated per-run secret; tokens are issued by the guest interpreter and checked
  by both the guest runner and Fabric. "Completed" means the test body returned normally. It is
  not evidence that every assertion ran: an assertion inside a closure handed to the candidate
  runs only if the candidate calls it, so a suite must assert after the call.
- Sealing, containment and per-provenance kernels run in the guest interpreter as certified at
  `31413ca7` (`governance/proofs/v022-pci/CERTIFICATION.md`, local backend, EMPTY effect ceiling)
  plus amendments 53/60/72/78 (the delta, listed from git in `governance/notes/v022-pci-delta.md`),
  each exercised by named PCI gate rows (`scripts/v022_pci_gates.sh`, 28 rows) and covered by
  named mutation rows; that those rows are re-run and killed at the frozen head is a FREEZE
  OBLIGATION, not yet shown. A non-empty guest effect ceiling is OUTSIDE the PCI certification.
- A pass without completion evidence is Unknown. A candidate that prints a second line naming a
  test turns that test's genuine verdict into Unknown (fail-safe: never into a pass).

**PSV-4 — Fabric signs only a guest-path verdict.**
- Fabric attests (`acf-receipt-attestation/2`) a verdict as PROTECTED only when it came from the
  protected guest path, and signs other classes under their own label (`development`,
  `guest-unobserved`).
- Backend `linux-microvm-protected`, under a current B263 qualification. Currency is judged at
  dispatch and just before launch, not again at signing (a stated limit).
- A protected receipt under a non-empty guest effect ceiling is outside the PCI certification
  (PSV-3); the signed receipt names only the policy digest, not the ceiling
  (`governance/notes/v022-pci-delta.md`, section c).
- The verdict carries the guest runner's completion evidence.
- A verdict from any other backend, or reconstructed outside the guest, is never attested as
  protected.

**PSV-5 — The receipt binds everything that makes the verdict mean something.**
- The signed receipt (or the attestation's signed binding) names:
  - guest image, kernel and runtime (guest `axon`) digests;
  - suite id, version, entry and test;
  - candidate tree (WorkspaceVersion);
  - task, trial, attempt and operation;
  - verifier identity and key;
  - the preflight observation digest (PSV-6);
  - the launch manifest digest.
- Intake and EVL verify each join (G01 authentication, then the registered binding gates).

**PSV-6 — Fresh observer evidence joins the exact launch (ADR-002).**
- A `PreflightObservation`, signed by the observer's own key (authorised by the operator root key,
  distinct from every other role), is bound to a custodian nonce and epoch, and names:
  - host profile;
  - Fabric revision and Firecracker digest;
  - guest image, kernel, verifier, suite and policy digests;
  - the intended launch manifest digest.
- Fabric consumes it and cannot mint it: it holds no observer key, so it cannot forge the
  SIGNATURE. What the signature attests is stated per field (amendment 79, which replaced the
  claim that every field was measured):
  - MEASURED by the observer from the operator's installed files: host config, launcher,
    Firecracker, guest kernel and rootfs, suite registry, qualification record and profile
    manifest digests;
  - PINNED by the operator (the helper config's `fabric {path, sha256, revision}`), which the root
    helper holds the caller's executable to on every launch and relay: the verifier digest and
    the Fabric revision;
  - NAMED by the measured profile manifest: the guest init and axon digests;
  - the nonce: issued for that epoch and unspent (custodian), observed once (observer);
  - the principal's word, NOT measured: the guest policy digest (the root helper binds the policy
    it boots to it) and the authority epoch (the loop's scope pointer, joined at intake).
  The verifier digest is the digest of the executable file the Fabric-uid process had when the
  helper opened `/proc/<ppid>/exe`. Any Fabric-uid code can arrange that this is the pinned file:
  exec it after spawning the helper (executed: 18 of 20 attempts got an observer-signed
  observation naming the pinned verifier; no `LD_PRELOAD` or ptrace needed, and Yama does not
  touch the exec route), `LD_PRELOAD`, or ptrace. So the helper serves a caller whose executable
  at that instant is the pinned file, not necessarily the pinned program's instructions (amendment
  84; see amendment 85 for any later change to when the helper measures it).
- Freshness and one-use are enforced where the launch happens, not at intake: Fabric's early
  check and the root helper verify the observation's age and epoch and the custodian spends its
  nonce once. Intake rejects a receipt whose observation digest does not join, or whose launch
  (manifest digest, trial/attempt/operation, candidate) or epoch is not the trial's; it
  deliberately does not re-check age and keeps no nonce registry, so a genuine verdict is not
  expired by the passing of time (protocol §9, C9 correction).

**PSV-7 — Development cannot pass as protected.**
- The local interpreter backend, a development-class evaluation, and a receipt without a current
  operator-installed B263 qualification cannot satisfy the protected profile (ADR-001 D3), in code
  and in the readiness derivation. Host identity is checked at install by the operator kit
  (`scripts/operator_deploy_protected_host.sh`), not by Fabric or readiness, which require only a
  non-empty `host` in the record (`accept_b263`); post-C9 item N makes it a code rule. The Stage 7
  `b263_qualification` readiness component is a bare sha pin in a mutable status file (redundant
  weak check; it cannot make Stage 7 READY alone, because the operator verifier component judges
  B263 currency).

## Negative matrix (each must fail closed, and be tested)

| Case | Expected refusal point |
|---|---|
| Replayed verdict or receipt (same operation, fresh journal) | Fabric withholds (replay is never signed), or launch refuses: nonce/observation not fresh or already spent (PSV-6). Intake does not re-check age or nonce |
| Wrong guest image or kernel | Observer/launch-manifest join fails (PSV-5/6); Fabric refuses an unqualified profile |
| Wrong suite, version or test | Guest runner refuses (PSV-2); intake refuses (registered acceptance, PSV-1) |
| Candidate changed after sealing | Guest runner digest check (PSV-2); receipt input-tree join (PSV-5) |
| Stale or replayed observer evidence | Launch: age and one-use (Fabric early check, root helper, custodian spend). Intake: digest, launch and epoch joins only, no age or nonce check (PSV-6) |
| Wrong verifier key | G01 authentication refuses (PSV-5) |
| Missing completion evidence | Verdict Unknown (PSV-3) |
| Mismatched trial/attempt/operation | Intake identity joins (PSV-5) |
| Local/dev backend presented as protected | Protected class counts nothing (PSV-7, D3) |
| Unsigned or self-signed protected-host certification | Readiness stays NOT_RUN (the rule above) |

## How the three readiness components are earned

| Where | What | Earns |
|---|---|---|
| Dev host | The protocol and the negative matrix above, adversarially | Nothing in readiness: *protocol proven* |
| Protected host | B263-qualified `linux-microvm-protected` run; the operator signs the certification record | `protected_backend` PASS |
| Protected host | G01 authenticity through the guest path; operator-signed record | `g01_on_protected_backend` PASS |
| Protected host | PCI surface through the guest path; operator-signed record | `pci_on_protected_backend` PASS |

When all seven components are PASS, PROTECTED_VERIFIER_READY = READY. Stage 7 and CX-21 then
still need their own frozen documents.

## The certification record the readiness script checks

`governance/proofs/v022-protected/<component>.json`, plus
`governance/proofs/v022-protected/<component>.json.sig`: a QUALIFICATION-domain
`axon-evidence-signature/2` over the exact bytes, from an operator key in
`/etc/axon/trust/qualification/` (`axon-fabric sign-evidence --authority qualification`, run by
the operator where the key lives).

```json
{
  "schema": "axon-v022-protected-certification/2",
  "component": "protected_backend | g01_on_protected_backend | pci_on_protected_backend",
  "host_profile": "linux-microvm-protected",
  "qualification_profile": "linux-microvm-protected",
  "psv_spec_sha256": "<sha256 of THIS document>",
  "axon_sha": "<40 hex>", "micode_sha": "<40 hex>", "fabric_revision": "<40 hex>",
  "guest_image_sha256": "<64 hex>", "guest_kernel_sha256": "<64 hex>", "guest_runtime_sha256": "<64 hex>",
  "suite": {"id": "…", "version": "…", "entry": "…", "test": "…", "digest": "…"},
  "candidate_tree_ref": "acf1:…",
  "observer_key_id": "ed25519:…", "observation_sha256": "<64 hex>",
  "verifier_key_id": "ed25519:…",
  "b263_qualification_sha256": "<64 hex>",
  "evidence": ["<proof files in governance/proofs/>"],
  "evidence_bundle_sha256": "<sha256 over the concatenated sha256 of each evidence file, in order>",
  "readiness_verifier_sha256": "<sha256 of the installed axon-fabric that decides readiness>",
  "trust_preflight_sha256": "<sha256 of the protected-mode trust_root_preflight.sh report, one of evidence>",
  "certified_at": "YYYY-MM-DDTHH:MM:SSZ"
}
```

The readiness script requires every field, with well-formed digests and commit ids, and:
- `psv_spec_sha256` equal to this document's current hash;
- `axon_sha` an ancestor of the judged tree, with **no file outside `governance/` changed since**;
- every evidence file present, and the recomputed bundle digest equal;
- `trust_preflight_sha256` naming one of those evidence files, which is an
  `axon-trust-preflight/1` report with `mode: protected` and `verdict: PASS`;
- `readiness_verifier_sha256` equal to the sha256 of the verifier deciding now;
- `observer_key_id` and `verifier_key_id` to be keys in `/etc/axon/trust/observer/` and
  `/etc/axon/trust/verifier/` now;
- `observation_sha256` to name one of the evidence files: an `axon-preflight-observation/1` whose
  detached observer-domain signature (`<file>.sig`) verifies under the observer root, signed by
  `observer_key_id`, observing this profile, this `fabric_revision` and this guest;
- `b263_qualification_sha256` to name one of the evidence files: an `axon-b263-evidence/1` record of
  this profile, qualification-signed (`<file>.sig`) under the operator root, whose
  `profile.artifacts` are this guest (`vmlinux` = `guest_kernel_sha256`, `rootfs.sqfs` =
  `guest_image_sha256`, `axon` = `guest_runtime_sha256`);
- the B263 record to have been current when the run was observed (`end` no later than
  `observed_at`, within the maximum age of it), as well as at decision time;
- the run itself among the evidence (amendment 57): exactly one `axon-fabric-submit/1` (Fabric's
  submit output: the receipt, its `acf-receipt-attestation/2` and the `axon-psv-evidence/2`
  bundle) and exactly one `acf-compute-request/1`. The attestation must verify under the
  verifier-root key `verifier_key_id` names. The receipt must be protected evidence naming the
  bundle's launch manifest, this observation, the guest verdict and the manifest's qualification.
  The observation must join that manifest field for field. The manifest's `qualification_sha256`
  must be `b263_qualification_sha256`, and its suite and candidate must be the record's `suite`
  (`digest` = the suite tree digest) and `candidate_tree_ref`. `micode_sha` is operator-attested:
  no run document carries it;
- the operator-installed `verify-readiness` to verify the signature under the operator root;
- if `trust-expectations.json` lists expected issuers, the signer to be among them.

Every file is read ONCE, as a regular file and never through a symlink, and each check above,
including the signature, is made on that one buffer.

Any later change to the spec, the code or the evidence therefore invalidates the certification
rather than inheriting it.

## The trust-root preflight (executable, not mode bits)

`scripts/trust_root_preflight.sh` makes REAL attempts under each intended service UID (`setpriv`,
no supplementary groups) and requires the kernel's answer:

| Actor | Must | Attempted as |
|---|---|---|
| verifier | read every file in `qualification/` and `verifier.json` | `cat`, `ls` |
| verifier, custodian, every agent UID (MiCode, Claude) | NOT create/rename in any directory, from `/` down | `mkdir` of a probe (removed if it ever succeeds) |
| same | NOT open any file for writing | `open(O_RDWR)`, no byte written |
| same | NOT change any file's mode | `chmod(2)` re-applying its own mode |
| candidate guest | NOT address the root at all | `trust_root_guest_probe.sh` INSIDE the guest (`--guest-cmd`) |
| fabric | NOT modify any of the above; alone READ the attestation signing key | `open(O_RDONLY)` of the key by every actor (A20) |
| all actors | the same refusals over every path the protected-host config pins (O1) | as above, over the config, launcher, manifest, registry, record and the key's directory |

`chmod(1)` is deliberately not used: GNU chmod skips the syscall on an unchanged mode and reports
success, so it attempts nothing (measured). Root is never accepted as an actor, and a non-root
invocation is NOT_RUN (exit 2), never a pass.

Without `--root` the mode is `protected` (the fixed `/etc/axon/trust`, every ancestor probed). With
`--root` it is `dev` and certifies nothing. `scripts/test_trust_root_preflight.sh` proves the
mechanism on a fixture: a clean root passes, and each of these FAILs on its own check:
- a world-writable key;
- an agent-owned directory;
- a group-writable manifest;
- a custodian-owned key;
- a verifier that cannot read;
- a guest that can see the root;
- a POSIX ACL write grant.

## Operator runbook — installing the verifier

Five distinct things. Keep them apart; each has one source.

| Item | Where it lives | Who sets it | How it is checked |
|---|---|---|---|
| Installed verifier **binary** | an operator-chosen absolute path, e.g. `/usr/local/libexec/axon/axon-fabric`, root-owned, not group/other-writable | operator, from a clean `cargo build --release -p axon-fabric` at a reviewed revision | every path component operator-owned; its sha256 recomputed on every use |
| Binary **sha256** | `verifier.json` → `sha256` | printed by the installed binary itself | relay recomputes the file's digest; the binary self-reports the same; the certification's `readiness_verifier_sha256` must equal it |
| **Build / profile metadata** | `verifier.json` → `build`, `profile`, `fabric_revision`, `source_dirty`, `rustc`, `target` | printed by the installed binary itself (build.rs) | must equal the running binary's self-report; must be `production`, `release`, `source_dirty: false` |
| **Trust-root paths** | fixed in code: `/etc/axon/trust/{qualification,observer,verifier,admission}/`; echoed in `verifier.json` → `trust_roots` | not configurable (no CLI, env, repo or workspace override) | must equal the compiled-in paths; the qualification root must equal the one the verifier reports deciding over |
| **Verifier manifest** | `/etc/axon/trust/verifier.json` (fixed path), root-owned | operator | operator-owned walk; schema `axon-verifier-manifest/1`; every field present |

The manifest DESCRIBES and PINS the binary. It is not the binary, and the binary is not configured
by it. To install:

1. Build `axon-fabric` in release mode from a clean checkout of the reviewed revision. It must
   be a STANDALONE CLONE, not a linked worktree. Either `CARGO_TARGET_DIR` points outside the
   tree, or `/etc/axon/provenance-allowlist` (root-owned, 0644) excuses `target/`. Every object in
   the tree counts, and `.gitignore` excuses nothing (protocol amendment 44).
2. Copy it to the chosen path as root, with mode 0755.
3. Run THAT installed path: `/usr/local/libexec/axon/axon-fabric verifier-manifest`. Never run the
   build tree's copy for this step: the manifest names the executable that produced it, so running
   another copy would pin the wrong executable.
4. Review the output. Its `path` must be the installed path, `fabric_revision` the reviewed
   revision, `source_dirty` false, `profile` release, and `build` production. A binary produced
   by `cargo test` reads `test-trust`, because the dev-dependency feature unifies into it
   (observed); only a plain `cargo build --release` yields production.
5. Write the output to `/etc/axon/trust/verifier.json` as root, with mode 0644.
6. Run the trust-root preflight in protected mode with the real service UIDs and the real guest
   command. Commit its report as certification evidence.
7. Only then sign certification records (`sign-evidence --authority qualification`). Each record
   names `readiness_verifier_sha256` = the manifest's `sha256`.

Replacing the binary without re-certifying fails closed at two independent points:
- the relay refuses, because the digest differs from the manifest;
- the new binary refuses every existing certification, because `readiness_verifier_sha256` differs.

## Mutation record for the readiness authority

Mutants of `readiness.rs` and `backend.rs` are killed by `tests/readiness.rs`, `tests/trust_root.rs`
and `tests/qualification.rs`, with ONE exception. That exception is classified **equivalent** and is
**not counted as killed**:

- `no-diff-fail` — the git-diff failure path does not fail closed. It is unreachable: the ancestor
  check before it has already established that `axon_sha` is an ancestor of HEAD, and for such a
  pair the diff cannot fail. The code still fails closed there (defence in depth), but no test can
  distinguish the mutant.
