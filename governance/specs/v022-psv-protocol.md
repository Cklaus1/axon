# v0.22 PSV protocol — authority contract and frozen schemas (step 0)

Status: **DRAFT, frozen before implementation** (operator direction, 2026-09-28). This fixes, before
any M1 code:
- which trust domain produces every load-bearing protected field;
- the O1 authority contract;
- the formats M1–M4 exchange.

Companion to:
- `v022-protected-suite-verdict.md`, the acceptance surface;
- `v022-psv-gap-map.md`, the gaps and the adversarial matrix A1–A19.

It certifies nothing, and it moves no readiness component.

## Build order

```
0. freeze these schemas + the O1 contract            (this document)
1. O1  operator-pinned launcher, profile, suite registry, signing key
2. M1  guest verdict runner + sealed input transport
3. M2  protected verdict + Fabric dispatch/attestation   ─┐ M3 may proceed in parallel
4. M3  observer / preflight join                          ─┘ once §3 is frozen
5. O2  loop verifier/observer keys from the operator root  (parallel with M1/M2)
6. M4  intake / EVL protected joins                        (O2 is a HARD prerequisite)
7. A1–A21 negative matrix complete  →  PSV_PROTOCOL_PROVEN
```

Throughout:

```
PSV_PROTOCOL_PROVEN        eventually PASS (dev host; no readiness credit)
protected_backend          NOT_RUN
g01_on_protected_backend   NOT_RUN
pci_on_protected_backend   NOT_RUN
PROTECTED_VERIFIER_READY   NOT_READY
```

## 1. Trusted producer of every protected field

No load-bearing protected field may originate from MiCode, the candidate, the repository, or a
caller-supplied path.

| Field | Produced by | Enforced by |
|---|---|---|
| candidate tree digest | Fabric, over its workspace store | M1 (guest re-digest), M4 (join) |
| suite id / version / entry / tree digest | the operator suite registry (O1) | M1, M4 |
| test id | the experiment's `task_acceptance`; the request NAMES it | M1 (runs only it), M4 |
| per-attempt completion secret | Fabric's CSPRNG, per attempt | §4 |
| completion proof | the trusted guest runner | M2 (host recomputation) |
| guest kernel / rootfs / runtime / init digests | the operator profile manifest (O1), pinned by the B263 qualification | M2, M3, M4 |
| launcher digest | the operator host config (O1) | M3 (observed), M4 |
| launch manifest | Fabric | M3 (observed), M4 |
| preflight observation | the observer (`/etc/axon/trust/observer/`) | M3, M4 |
| B263 qualification | `/etc/axon/trust/qualification/` | Fabric (exists), M4 |
| attestation signing key | the operator host config (O1); readable ONLY by the Fabric service UID | O1, preflight |
| verifier / observer public keys, for the loop | `/etc/axon/trust/{verifier,observer}/` | O2 |
| trial / attempt / operation | the experiment, bound by Fabric | exists |

A thirteenth origin, found while writing this: Fabric's attestation signing key is named INSIDE the
caller-supplied check registry (`signer.key_path`, resolved relative to it,
`bin/axon-fabric.rs:151-181`). O1 removes it with the registry.

## 2. O1 — the authority contract

**Protected mode takes no configuration from the caller.** A request with
`hardware_isolation=true, os=linux` resolves EVERYTHING that defines the protected run from one
operator-owned file:

```
/etc/axon/protected-host.json            root-owned walk from /, no g/o write, no symlink
{
  "schema": "axon-protected-host/1",
  "launcher":       {"path": "/usr/local/libexec/axon/fc_linux_profile.sh", "sha256": "…"},
  "profile_manifest": {"path": "/usr/local/share/axon/linux-microvm/manifest.json", "sha256": "…"},
  "artifacts_dir":  "/usr/local/share/axon/linux-microvm/dist",
  "qualification":  {"record": "/var/lib/axon/b263/record.json", "signature": "….sig", "waivers": null},
  "suite_registry": {"path": "/usr/local/share/axon/suites/registry.json", "sha256": "…"},
  "signer":         {"issuer_ref": "…", "public_key": "…", "key_path": "/var/lib/axon/fabric/attest.pk8"},
  "out_root":       "/var/lib/axon/fabric/runs",
  "observer":       {"nonce_store": "/var/lib/axon/custodian/nonces", "max_age_s": 300}
}
```

Rules:
1. **Every path is absolute and operator-owned.** `check_operator_owned` applies to:
   - the host config;
   - the launcher;
   - the profile manifest;
   - the artifacts;
   - the suite registry.

   Each file's sha256 must equal its pin. The signing key is owned by the Fabric service UID, mode
   0400, and every ancestor is operator-owned.
2. **No caller override.** In protected mode, `submit` REFUSES:
   - `--linux-launcher`, `--linux-manifest`, `--linux-artifacts`;
   - `--linux-evidence*`, `--linux-waivers`, `--linux-out-root`;
   - a `--check-registry` whose suite would be used.

   It refuses rather than ignores, as it already does for `--linux-trusted-issuers`. A missing or
   non-conforming host config makes the profile INELIGIBLE, with no fallback.
3. **The caller names, never points.**
   - The request names a registered suite by id (`check:<id>`) and the test by name.
   - Version, entry and tree come from the operator registry; the candidate tree comes from
     Fabric's store.
4. **One source per thing.** The attestation signer comes from the host config, never from a
   registry file. The development-class `--check-registry` path keeps its own signer for local
   runs, which can never produce protected evidence (§6).
5. **Test roots only behind `test-trust-root`.** `ProtectedHostConfig::for_test(root)` has no
   production constructor, and a test-trust build reports `build: "test-trust"`.
6. **The trust preflight grows** (`scripts/trust_root_preflight.sh`):
   - agent UIDs can neither READ the signing key nor write any O1 path;
   - the Fabric UID can read the key but cannot write it.

Negative cases O1 must discriminate (added to the matrix):

| # | Case | Refused by |
|---|---|---|
| A16 | a replaced launcher reporting chosen digests or exit | launcher sha256 ≠ host-config pin, so ineligible |
| A17 | a caller-supplied suite registry | protected mode reads only the host config's registry |
| A20 | Fabric signing key readable by an agent UID | trust preflight (real read attempt) |
| A21 | any caller `--linux-*` / registry flag in protected mode | `submit` refuses, naming the flag |

## 3. Launch manifest — `axon-launch-manifest/1`

Built by Fabric per attempt, before the observation is requested. Its digest is
`sha256(canonical JSON)`, using the `axon_loop_contracts` canonical form: sorted keys, no
whitespace.

```json
{
  "schema": "axon-launch-manifest/1",
  "operation_id": "…", "task_id": "…", "trial_id": "…", "attempt_id": "…",
  "backend_profile": "linux-microvm-protected",
  "fabric_revision": "<40 hex>",
  "qualification_sha256": "<B263 record>",
  "host_config_sha256": "<axon-protected-host/1 file>",
  "launcher_sha256": "…", "firecracker_sha256": "…",
  "profile_manifest_sha256": "…",
  "guest": {"kernel_sha256": "…", "rootfs_sha256": "…", "axon_sha256": "…", "init_sha256": "…"},
  "policy_sha256": "<guest policy bytes>",
  "suite": {"id": "…", "version": "<WorkspaceVersion>", "entry": "…", "test": "…",
            "tree_digest": "acf1:…", "registry_sha256": "…"},
  "candidate": {"workspace_version": "acf1:…", "tree_digest": "acf1:…"},
  "completion": {"scheme": "axon-guest-completion/1"},
  "observation_nonce": "<custodian nonce, hex>",
  "limits": {"wall_time_ms": 0, "output_bytes": 0}
}
```

It carries NO secret. The completion secret travels separately (§4).

## 4. Input transport and the completion proof (M1)

**Structurally distinct, read-only inputs.** Each is a separate virtio block device, attached
read-only by the launcher. The guest mounts it `ro,nodev,nosuid,noexec` (amendment 37: this guest
kernel's ext4 has no `noacl`).

| Device | Content | Mounted at |
|---|---|---|
| `input-0` | candidate tree (the exact WorkspaceVersion from Fabric's store) | `/in/candidate` |
| `input-1` | operator suite tree (from the registry) | `/in/suite` |
| `job` | `launch-manifest.json`, `completion-secret` (mode 0400, root) | `/in/job` (read-only) |
| `out` | the verdict and the run's output | `/out` (the only writable device) |

**Before executing anything**, the guest runner:
1. re-computes the acf1 tree digest of `/in/candidate` and `/in/suite`, using the same algorithm
   as Fabric's store (shared code, not a re-implementation);
2. compares each digest with the launch manifest, and the manifest with the digest Fabric named on
   the kernel command line;
3. on any mismatch, writes `status: refused`, and executes nothing.

**Completion proof.** Per attempt, Fabric generates a 32-byte secret `S` from its CSPRNG. `S` is
written only to the job device and kept in Fabric's memory; it never appears in the manifest, a
receipt, a log or the journal.

```
B = canonical JSON {
      scheme: "axon-guest-completion/1",
      operation_id, trial_id, attempt_id,
      suite_id, suite_version, entry, test,
      candidate_tree_digest, suite_tree_digest,
      launch_manifest_digest }
K = HMAC-SHA256(S, "axon-guest-completion/1\n" || hex(sha256(B)))   -- the lowercase HEX digest, as axon_psv::completion_key
token(test) = completion_token(K, test)    -- the existing PCI interpreter derivation
```

- The runner derives `K` from the manifest it VERIFIED. It hands `K` to
  `axon test --completion-key-stdin --seal /in/candidate` over stdin, and never exposes `S` or `K`
  to candidate code.
- Candidate code runs unprivileged in the guest (`AXON_PATH` = `/in/suite:/in/candidate`, i.e. the
  suite's directory then the candidate's, with `AXON_PATH_EXCLUSIVE=1`). The secret file is readable
  only by root.
- The guest POLICY is not on the job device: the launcher delivers it with `--policy FILE` on the
  kernel command line (`axon.policy=<base64>`), and its sha256 is bound in the launch manifest
  (`policy_sha256`).
- The host recomputes `K` from `S` and its OWN `B`, never from anything the guest reports.

A proof from another attempt, candidate, suite, test or manifest therefore does not verify (A11).

## 5. Guest verdict — `axon-guest-verdict/1`

Written by the runner to `/out/verdict.json`. Its sha256 is also emitted on the serial console
(`PSV-VERDICT sha256=<hex>`) and cross-checked by the launcher, as `B263-OUT` is today.

```json
{
  "schema": "axon-guest-verdict/1",
  "launch_manifest_sha256": "…",
  "inputs": {"candidate_tree_digest": "acf1:…", "suite_tree_digest": "acf1:…", "match": true},
  "test": "…",
  "status": "passed | failed | unknown | refused",
  "refusal": null,
  "exit_code": 0,
  "report": {"passed": ["…"], "failed": [], "completion": [["…", "<token>"]]},
  "runner": {"runner_sha256": "…", "axon_sha256": "…"},
  "stdout_sha256": "…"
}
```

The guest's `status` is a CLAIM. Fabric derives the verdict:
- `passed` only if the test named in the manifest is in `report.passed`, the completion token
  verifies under the host-derived `K`, the exit code is 0, `inputs.match` holds, and the digests
  equal the manifest's;
- otherwise `failed` (a named failure with a clean run) or `unknown`.

## 6. Protected attestation rule (M2)

The current rule, "protected backend OR empty effect ceiling → may sign"
(`signing.rs:47`), is replaced by two separate evidence classes:

```
PROTECTED evidence  ⇐  profile linux-microvm-protected
                     AND B263 qualification valid at dispatch
                     AND host config (O1) conforming, launcher and manifest at their pins
                     AND launch manifest built by THIS Fabric for THIS attempt
                     AND observation verified (§7): observer domain, nonce unused, fresh, joins
                     AND guest verdict cross-checked (drive = serial digest)
                     AND inputs.match, digests = manifest
                     AND completion verified under the host-derived K
                     AND the attestation signer is the host config's

DEVELOPMENT evidence ⇐ an effect-free local check (today's rule). Never protected.
```

The class is carried INSIDE what is signed. The receipt's `evidence_refs` gain these entries,
covered by the receipt digest the attestation binds:
- `evidence-class:protected` or `evidence-class:development`;
- `launch-manifest-sha256:`;
- `preflight-observation-sha256:`;
- `guest-verdict-sha256:`;
- `guest-kernel-sha256:`, `guest-rootfs-sha256:`, `guest-axon-sha256:`;
- `suite:<id>@<version>#<entry>/<test>`;
- `qualification-sha256:`.

`acf-receipt-attestation/2` stays; MiCode's check of it is unchanged. M2 must first confirm that
the attestation binding covers the receipt digest, including `evidence_refs`. If it does not, the
schema moves to `/3` and MiCode moves with it.

## 7. Preflight observation — `axon-preflight-observation/1` (M3)

Per ADR-002, extended with the O1 digests:

```json
{
  "schema": "axon-preflight-observation/1",
  "observer_key_id": "ed25519:…",
  "nonce": "<custodian-issued, hex>", "epoch": 0, "observed_at": "…Z",
  "host_profile": "linux-microvm-protected",
  "fabric_revision": "…", "firecracker_sha256": "…",
  "launcher_sha256": "…", "host_config_sha256": "…",
  "guest": {"kernel_sha256": "…", "rootfs_sha256": "…", "axon_sha256": "…", "init_sha256": "…"},
  "verifier_sha256": "<installed axon-fabric>", "suite_registry_sha256": "…",
  "policy_sha256": "…",
  "intended_launch_manifest_sha256": "…"
}
```

- It is signed with an OBSERVER-domain `axon-evidence-signature/2`, under
  `/etc/axon/trust/observer/`.
- The custodian issues the nonce, and it is consumed once: a second use is refused (A15). The
  observation is refused when older than `max_age_s` or from another epoch (A8).
- The sequence is custodian nonce, then the manifest (which contains the nonce), then the
  observation of the manifest digest, then the launch. It is not circular: the manifest names the
  nonce, and the observation names the manifest.
- Fabric verifies and consumes an observation. It holds no observer key and cannot mint one
  (cannot forge the signature; which fields the observer measured and which are the principal's
  word is amendment 79's table).

## 8. O2 — loop key authority

- In the PROTECTED class, intake, EVL and admission resolve a claimed verifier key ONLY from
  `/etc/axon/trust/verifier/*.pub`, and observer keys only from `/etc/axon/trust/observer/*.pub`.
  Both use the operator-owned walk, the same code as Fabric's `TrustAuthority`.
- The store's `verifier_keys` may NARROW (list a subset); it never adds (A18).
- The development class keeps store keys.
- Test roots exist only behind a test-only feature.
- O2 lands before M4. Otherwise M4's joins would verify against keys an agent can plant.

## 9. M4 — loop-side joins (protected class)

A result counts as protected only if all of these hold:
- the attestation verifies under an O2 key;
- the receipt names `evidence-class:protected`;
- the launch-manifest digest equals the observation's `intended_launch_manifest_sha256`;
- the observation verifies under an O2 observer key and is of the trial's epoch (the loop checks both);
  it was fresh and its nonce unused when Fabric consumed it at launch (§7). The loop does NOT re-check
  age and keeps no nonce registry: a re-derivation long after the launch must not expire a genuine
  verdict (C9 correction; the earlier wording implied a loop-side freshness check that never existed);
- the guest digests equal the qualification's (PRODUCER-side: Fabric's `psv::prepare` takes them from the profile manifest the qualification hashed; the loop does not join them to a B263 record, amendment 42, restated by amendment 82);
- suite, test, candidate, trial, attempt and operation all equal the experiment's.

Anything less is Unverifiable in the protected class (PSV-7). It is never silently development.

## 10. Adversarial matrix

These cases are written before the mechanisms. The gap map holds A1–A19; §2 adds A20 and A21. Each
mechanism's slice names the rows it must turn from "missing" to "refused for the stated reason",
and its tests assert the reason.

## Amendments made during implementation

These are recorded here rather than edited into the frozen text above.

1. **A third evidence class, `guest-unobserved` (M2).** §6 names two classes. The implementation
   has a third: a verdict produced on the guest path and fully checked by Fabric (§5), but without a
   verified preflight observation. It is signed as `guest-unobserved`, and is never protected; only
   M3's verified observation makes a verdict `protected`. The class is inside the signed receipt
   (`evidence-class:` ref), and the attestation rule signs each class only where its backend derives
   it (`signing::WRONG_CLASS`).
2. **The child's output is `/out/test-stdout` and `/out/test-stderr` (M1).** §5 said `stdout`.
   The first real boot showed that `/init` already redirects the runner's own stdout to
   `/out/stdout`, and sharing it corrupted the child's output.
3. **Launcher exit 27, `verdict-unbound` (M1).** It was not in §4. It means the verdict on the
   returned drive is not the one `/init` hashed on the serial console, or the guest named another
   launch manifest. It is not admissible.
4. **The observer is an operator-pinned PROGRAM (M3).** `PROGRAM --manifest FILE --out DIR` writes
   `observation.json` plus its `.sig`. It is named with its sha256 in the host config's optional
   `observer` section, together with the custodian's `nonce_store` and `max_age_s`. The nonce is
   issued before the manifest is built, and consumed only after every other check holds. A refused
   observation refuses the launch.


The next six come from the independent review of candidate 2 (wf_d725935a-7ed, verdict
DO_NOT_REGISTER): blockers B1–B3 and the majors. They are the negative-matrix rows A22–A24.

5. **Only the operator's test is collected (B1).** §4 said the runner runs "the named test". Two
   gaps remained. A `--filter` substring also ran suite siblings, and a candidate's own `@[test]`
   could be collected under the registered name. Now, under `--seal`, `axon test` never collects a
   `@[test]` whose span lies in a sealed module (the candidate), and `--exact` selects the one
   registered name and nothing else. The runner always passes both. `AXON_PATH` is `suite:candidate`,
   so the suite's modules win name resolution.
6. **Inputs come from the store, never from the run dir (B3).** §4's inputs were read from the run
   dir under the caller-supplied `--state`, which the caller owns and could swap before launch.
   Fabric now re-materializes the candidate (the request's WorkspaceVersion) and the suite (its
   registered version) from the content-addressed store, re-verifying every blob. They go into a
   new 0700 dir `<out_root>/<jail>.psv-inputs/{candidate,check}`, which is removed on every path.
   `prepare` requires each tree to BE its version. `LaunchManifest::verify` requires
   `tree_digest == workspace_version` for the candidate and `tree_digest == version` for the suite.
   The guest's `check_inputs` refuses a symlink, and any entry the digest omits (`.git`, `.micode`),
   so the bytes that run are exactly the bytes digested.
7. **`verifier_sha256` is in the manifest and the observation (major).** Both schemas gain it, taken
   from `readiness::verifier_identity()`. The observation joins it like every other field (13 join
   pairs). The observation is read ONCE and verified as bytes, so the document checked is the
   document joined.
8. **The secret dies with the launch; verify inherits nothing (majors).** The job drive's source,
   including `completion-secret`, is scrubbed the moment the launcher returns, before any further
   child runs. The `--verify-result` step runs with `env_clear()` and a fixed `PATH`, like the launch
   itself.
9. **A protected claim carries its documents: `axon-psv-evidence/1` (B2).** §9's joins were checked
   over DIGESTS the receipt names, and nothing held the documents those digests name. Fabric now
   returns a bundle `{schema, launch_manifest, observation, observation_signature}`: the manifest's
   canonical bytes and the observation's exact bytes, with its detached observer-domain signature.
   Intake requires the bundle for every protected claim, and refuses one offered for a receipt that
   does not claim protection. `protected_evidence::check_bundle` then verifies, in order:
   - `check`;
   - the manifest bytes against the receipt's `launch-manifest-sha256`, and
     `LaunchManifest::verify`;
   - the observation bytes against `preflight-observation-sha256`;
   - the observation signature under the OPERATOR's observer root (O2), and that it names that key;
   - the observation's join to the manifest;
   - the manifest's join to the request and receipt: operation, task, trial and attempt; the
     candidate (both the request's and the receipt's); the test (argv[1]); the guest kernel, rootfs,
     axon and init; and the qualification;
   - the receipt's `check-suite:` equals `{id}@{version}#{entry}`, and argv[0] equals
     `check:{id}`.

   The bundle is stored in CAS beside the intake record (`verification_psv_evidence_ref`). MiCode
   keeps it beside the receipt (`psv_evidence_ref`).
10. **A verdict names its one matched check (major).** For Passed or Failed, the PSV receipt sets
    `matched_checks = 1`, which the receipt contract requires for a pass.

Recorded follow-ups, not fixed in candidate 3:
- Safety-monitor keys and `protected_scopes` are still held in the store.
- Admission does not re-run `protected_evidence::check`.
- Candidate output can splice result lines (MINOR).
- A launch with no observer proceeds as `guest-unobserved` (MINOR).
- The ownership checks on `nonce_store` and `out_root` are missing (MINOR).
- Custodian UID separation is protected-only.

The next two come from the independent review of candidate 3b (wf_1bc28496-38e, verdict
DO_NOT_REGISTER). They are the negative-matrix rows A25 and A26.

11. **A failure is a verdict only with KEYED evidence (PSV-4).** §6 required completion evidence for
    a pass and none for a failure. As a result, a correct candidate could print a `failed` line for
    the registered test over its own genuine, keyed pass, and Fabric attested a protected Failed.
    Now:
    - Under `--completion-key-stdin`, the interpreter keys every failure IT decides, in its own
      domain: `HMAC(K, "axon-test-failed/1\0" + name)`. A pass stays
      `HMAC(K, "axon-test-completion/1\0" + name)`.
    - `axon_psv::keyed_outcome` decides the outcome only when exactly one line names the test and
      that line carries the token K issues for its status.
    - The guest runner reports `failed` only with that evidence and a failing exit.
    - Fabric's `derive` requires it for a Failed, and requires a non-zero exit. Anything else is
      Unknown and never protected. A failure in candidate code (a panic, or `exit(n)`) is still a
      failure the interpreter decides, so it is keyed.
    - `derive` also requires the verdict bytes it reads to be the ones the launcher bound
      (`result.json` `psv.verdict_sha256`), and not whatever the extracted file holds later.
12. **Monitor keys are operator-rooted in the protected class (PSV-7).** O2 (§8) rooted only the
    verifier and observer keys. A safety-monitor key that existed only in the mutable store cleared a
    protected trial, and the reviewer reached ACCEPT and activation on one.
    - There is a fifth trust authority, `monitor` (`/etc/axon/trust/monitor/`).
    - In a protected scope, `safety::report` authenticates a clearance only under a monitor key the
      operator root also holds.
    - Admission's protected re-derivation re-checks the key against the root at every derivation,
      so a key revoked at the root un-counts the clearance.
    - The store may name a monitor and narrow the set, but never add a key.

Recorded from that review as follow-ups, not fixed here (none was a BLOCKER):
- `task_acceptance` and `verifier_pins.check_suites` are store config.
- The loop has no anchor of its own for the guest kernel, rootfs, init or qualification digests.
- Intake and EVL do not re-check the observation's freshness or epoch.
- The observer runs as Fabric's UID (protected-only).
- `protected_scopes` is store config.
- In protected mode, the guest effect policy comes from the caller's `--grant-registry`.

The next one comes from the independent review of candidate 4 (wf_ecfcd666-6c9, verdict
DO_NOT_REGISTER, 7 of 8 claims registered). It is negative-matrix row A27.

13. **K is unreachable from candidate code, even under an Exec grant (PSV-3).** The derived key K
    lives in the memory of the `axon test` process, which also runs candidate code. Before this
    fix, that process was dumpable, the guest kernel has no Yama, and the effect ceiling came from
    the caller's grant.
    - **Reproduced in the real guest** on the candidate-4 image, under `policy-io-exec`: candidate
      code spawned a shell that scanned the process's own writable memory and recovered K. HMAC
      under the recovered value reproduced the interpreter's own pass token, so any pass or failure
      token could be forged.
    - **Now, under `--completion-key-stdin`, `axon test`:**
      - reads the key raw from fd 0 (no buffered copy) and scrubs the line;
      - makes itself NON-DUMPABLE (`PR_SET_DUMPABLE 0`) before any program code runs, so no
        unprivileged process, its own children and parent included, can open its memory;
      - removes `Exec` from the effect ceiling (and, with no ceiling, takes every effect except
        `Exec`), so it spawns nothing whatever the grant says.
    - **The guest runner independently** passes the policy's ceiling without `Exec`.
    - **Recorded, not done:** Yama in the guest kernel config needs a kernel rebuild and a B263
      re-qualification, so it is an operator-side follow-up.

The next one comes from the independent review of candidate 5 (wf_293dfdb6-9d8, verdict
DO_NOT_REGISTER, 7 of 8 claims registered, PSV-3 among them). It is negative-matrix row A28.

14. **The suite's modules win name resolution, with no fall-through (PSV-1).** Amendment 5 relied on
    `AXON_PATH=suite:candidate`. The module loader, however, tried the next directory when a module
    existed but could not be read.
    - **Executed by the reviewer to a keyed PASS**, through the real runner: a suite `helper.ax`
      with one Latin-1 byte, plus a candidate `helper.ax` with the wanted answer.
    - **Now:** only an ABSENT module moves the search on. One that exists but cannot be read
      (non-UTF-8 content, a directory of that name), or whose existence cannot be determined
      (`try_exists` error), is E0901, and the search stops.
    - **Not covered by a test:** the `try_exists` error arm (EACCES while traversing) cannot be
      exercised by the root-run tests.
    - **Recorded, not fixed:** under an IO grant, a sealed candidate can READ the suite's source
      through the filesystem. It learns the answers, but cannot change the rubric.

The next four come from dev review-loop round 1 (wf_336353cb-a2b, 16 agents, run on protocol
code identical to candidate 6, whose certifying review wf_0bc40b69-da7 had registered 8/8). The
round found five executed blockers, and those falsify candidate 6's PSV-1, PSV-6 and PSV-7. They
are negative-matrix rows A29–A32.

15. **A sealed module's imports stay sealed (PSV-1).** A `use` inside a sealed (candidate) module
    searched the suite dir first. It could therefore pull into the program an operator module the
    entry never imports, which then decided the verdict. Now a module loaded from a sealed dir
    resolves its own nested `use`s only in the sealed dirs.
16. **The observation's authority epoch is joined and re-checked (PSV-6).**
    - `check_bundle` takes the trial's epoch and refuses an observation made under another one. The
      loop joins that epoch to its OWN scope pointer, so an authority store chosen by the caller no
      longer yields accepted protected evidence.
    - Fabric re-reads the current epoch after the observer returns, and refuses the launch if it
      moved.
    - Recorded, not done: on a protected host, `--store` should also come from the operator config
      (defence in depth; the loop join is the enforcement point).
17. **A protected decision re-verifies its verdicts (PSV-7).** Admission trusted the stored
    evaluation record. Now, in the protected class, admission (and so rederive and the
    protected-scope gate) re-runs intake's own verification on every counted verdict, from its
    stored request, receipt, attestation and `axon-psv-evidence/1` bundle, under the operator
    roots. The receipt must claim protected evidence. The record, the ledger and the CAS are all
    writable by a store writer; the signatures are not.
18. **The execution leg is attested, not claimed (PSV-7).**
    - Fabric attests an EXECUTION it dispatched to the protected profile, in its own domain
      (`axon.fabric-execution/1`), over that execution's request and receipt.
    - EVL's protected class counts a trial's execution leg only with that attestation, verified
      under the operator's verifier root. Admission re-verifies it.
    - MiCode's D12 flow is unaffected: its executions were already ineligible for the protected
      class.

The next four come from dev review-loop round 2 (wf_7cb5856d-806, 16 agents), which found four
distinct blockers. The first is a regression of amendment 15. They are negative-matrix rows A33–A36.

19. **One file per module name, suite first (PSV-1; supersedes amendment 15's mechanism).** The
    sealed-only nested search let a candidate module's `use` load FIRST under the name of a suite
    module. The suite's own later `use` then found it "already loaded", and the candidate defined the
    rubric. Now, a sealed module's `use` of a name that any operator (unsealed) dir holds is E0901.
    Under that rule, a sealed-only search list is equivalent, so it is removed.
20. **A protected decision counts only the trials it re-verifies (PSV-7).** Admission decided on the
    stored arm counters. Now, in the protected class:
    - every arm's counters must equal its trials' outcomes;
    - the trials must be exactly the frozen plan's population;
    - every counted trial re-verifies against its OWN episode;
    - the evaluation's class must equal the frozen plan's (defence in depth).
21. **A protected clearance re-verifies its signature (PSV-7).** A clearance counted by its ledger
    `key_id` string alone. Now `safety::report` keeps each clearance's detached signature
    (`clearance-signatures/`) and records its ref on the event, and protected admission re-verifies
    it under the operator's monitor root.
22. **A protected host runs nothing outside the protected profile (PSV-3).** A local dispatch on a
    protected host ran workload code in the host's own privilege domain, beside the protected
    attempts' custody. Now `submit` refuses any selection other than the protected profile when a
    protected host is configured, and nothing runs.

The next four come from dev review-loop round 3 (wf_bf757240-925, 16 agents), which found five
blockers reducing to four fixes (two PSV-7 findings share one). They are negative-matrix rows
A37–A40.

23. **The launcher's inline Python runs isolated (FIELD-ORIGIN).** The pinned launcher runs as
    root, and its `python3 -c` / `python3 -` calls put the caller's working directory first on
    `sys.path`. Every call now runs with `-I` (isolated: no cwd on the path, no env-driven import).
24. **A protected record must agree with its re-verified documents (PSV-7).** Re-verification
    checked each counted verdict's documents but not every join back to the stored record. Now each
    counted trial's episode must have run its arm's policy; its recorded outcome must equal the
    verdict the verifier signed; and its preflight-context signature is stored at evaluation
    (`context-signatures/`) and re-verified at admission under the operator's observer root, not
    taken from the `context_signed_by` string.
25. **A sealed frame may not reseed the process RNG (PSV-1).** The interpreter's RNG is
    process-global, so sealed candidate code could `srand` it and predict every draw the operator's
    acceptance test makes. `srand` is now refused (SandboxViolation) from a sealed frame; the
    operator's test may still use randomness.
26. (Grouped with 24: the arm-policy, outcome and context-signature joins are one fix.)

The next six come from the two Candidate-8 certifying reviews (wf_bff9835f-4a0 and wf_ae3a5a74-41e,
both DO_NOT_REGISTER) and the C9 hardening wave. The second review's findings were present at 7b
too, so 7b's PSV_PROTOCOL_PROVEN is WITHDRAWN (governance/status/v022-psv-protocol.json).

27. **Every RNG draw comes from the running frame's kernel (PSV-1; supersedes amendment 25's
    mechanism).** Amendment 25 refused `srand` by name. The next fix refused draws by the `Random`
    effect row. Both were lists, and the review steered the operator's stream through
    `goal_run_random`, whose row is `{AI,Net,IO}`, to a keyed PASS for a wrong candidate. The RNG
    now lives in the interpreter's per-provenance `Kernel`. `rng_next`/`rng_reseed` select the
    running frame's kernel, and the process-global stream is gone. The sealed kernel's stream is
    seeded by a one-way SHA-256 derivation of the operator seed. So sealed code can neither move,
    reseed nor learn the operator's stream, whatever builtin it uses. Candidate code may still use
    randomness. Negative-matrix A40, A41, A42.
28. **Equivalence is not presumed (mutation evidence).** Of eight rows once retired as
    "equivalent", six were load-bearing (M254, M103, M255, M104, M209, M210). They had passed
    paired-disable only because each row's assigned test attacked a route another guard covers.
    All six are ACTIVE, each killed by a test of its named property. A row is retired only if all
    four cells hold for ONE named attack, with the attack's own failure message; the full package
    suite (in the row's feature configuration) stays green without it; and an all-paths dominance
    argument is recorded. Only M58 and M245 meet this. M176 was stale, not equivalent (M293 now
    kills its refactored guard). Amended by 39: a kill must be the row's own attack.
29. **A counted receipt must claim protected evidence, on every writer route (PSV-7).** A store
    writer repointed a genuine protected record at genuinely signed guest-unobserved or
    development verdicts. That is refused at admission (`does not claim protected evidence`).
    Negative-matrix A43.
30. **A protected record's attribution is checked, not only its documents (PSV-7).** Re-verification
    authenticates each document's own signer, but never compared it with the record's attribution.
    Admission's attribution checks are the only guard against a record that names an identity the
    operator root never held. Negative-matrix A44.
31. **The grant registry of a protected host is operator-pinned (FIELD-ORIGIN / Lane-A D1).** This
    closes the follow-up recorded after amendment 12 ("the guest effect policy comes from the
    caller's `--grant-registry`"). A caller registry is refused on every route of a protected host.
    status/cancel authorize before any journal write and reconcile only their own scope. In
    development, the authorizing registry's sha256 is recorded in the op's intent.
    Negative-matrix A45.
32. **Readiness certification is decided from the object store, not from git's porcelain view
    (FIELD-ORIGIN).** Git runs env-cleared from a fixed binary, with replace objects disabled.
    Replace refs, grafts, and skip-worktree or assume-unchanged entries are refused. The certified
    commit, the trees and the working-tree bytes are re-hashed. Negative-matrix A46. Still open, and
    it needs an operator decision: an untracked file hidden by `.git/info/exclude` or a self-ignoring
    `.gitignore` is invisible to the check, and refusing such files would change what counts as READY.
    CLOSED by amendment 44 (operator decision C): every object in the tree counts; only the
    operator's allowlist excuses generated material.
33. **The guest verdict itself is joined; the evidence bundle is `/2` (PSV-5; supersedes amendment 9's
    bundle shape).** `guest-verdict-sha256` was required but joined to nothing. The bundle now carries
    the exact verdict bytes. They must hash to the receipt's digest, be `axon-guest-verdict/1`, name
    the bundle's launch manifest and test, carry the manifest's inputs, and claim the outcome the loop
    counts. A `/1` bundle is refused. The five manifest joins that no test killed (operation, task,
    guest rootfs/axon/init) are each the only refusal of their forgery and are now tested.
    Negative-matrix A47, A48. The loop does not check an observation's age: see §9 and A8.
34. **Protected identity at Fabric and the runner (PSV-2/3/4/6).**
    - A protected host with no observer section launches nothing (A49).
    - A bundle travels only with a protected verdict (A50).
    - The pass side needs exactly one keyed result line, with its own discriminator (A51).
    - The runner refuses to read the completion secret if it cannot make itself non-dumpable (A52).
    - The guest input check refuses trees holding what the digest cannot see. The digest is a
      cross-peer contract with MiCode, so it is unchanged (A53).
35. **Fabric launches on the protected profile only through the observed path (C9 dev review
    round 1: PSV-4/5/6, FIELD-ORIGIN).**
    - The protected profile runs only an operator-suite `registered_check`. Every launch on it
      goes through the launch manifest, the custodian nonce and the preflight observation, and
      Fabric has no other way to start the launcher. An `interpreter_run` used to launch there
      with none of these and was attested as a protected execution (A54).
    - Fabric attests an execution only when its receipt carries the protected class, the
      launch-manifest digest and the preflight-observation digest. No execution can carry them
      today, so no protected execution leg exists, and EVL's protected class (amendment 18)
      cannot count a trial until an observed execution path is designed.
    - A bundle travels only with a receipt whose FINAL class is protected. The class is read
      after an inadmissible launch is downgraded, not from `derive` (A55).
    - `out_root` and `observer.nonce_store` must exist, be directories owned by the Fabric
      service UID, and be mode 0700. Only a missing host config means "not a protected host".
      The trust preflight probes the paths `axon-fabric protected-host-paths` lists, which is the
      list `ProtectedHost::load` walks (A56).
    - ADR-002 key-role separation is checked on the operator roots themselves: the observer root
      may hold neither the host signer's public key nor a key of another authority root. This is
      checked when the host config loads and again at every observation (A57).
36. **One read per authority decision; readiness attribution joined; build provenance hardened
    (PSV-7, FIELD-ORIGIN, C9 round 1).**
    - Readiness checked one read of the certification record and verified the operator signature
      over a second read, and hashed the trust preflight on one read and parsed another. A FIFO or a
      rename served different bytes to each, so one genuine signature certified any tree or any
      component (reproduced). Every file readiness decides on is now read once
      (`backend::read_regular`: `O_NOFOLLOW`, non-blocking, regular files only, size-capped), and
      the signature is verified over the buffer the fields are checked on. The same class was
      fixed in the B263 qualification (profile manifest hashed and parsed from one buffer, and
      `psv::prepare` joins its read to the qualified digest), the observer (the `.sig` is read once
      and that text goes into the bundle), `interpret_linux_result` (the recorded `result.json`
      digest is of the bytes interpreted), and the signer key (one `O_NOFOLLOW` open; the fd is
      checked and read) (A58, A61).
    - The record's attribution is joined, not only shape-checked: `observer_key_id` and
      `verifier_key_id` must be keys in the operator's observer and verifier roots at decision time;
      `observation_sha256` and `b263_qualification_sha256` must name certified evidence files; the
      observation must verify under the observer root, signed by `observer_key_id`, and match the
      record's profile, `fabric_revision` and guest; the B263 record must verify under the
      qualification root, be `axon-b263-evidence/1` of the protected profile, and qualify the
      certified guest (`vmlinux` = kernel, `rootfs.sqfs` = image, `axon` = runtime) (A59). The
      observation and B263 signatures are read from `<file>.sig` beside each evidence file.
    - `build.rs` derives `fabric_revision`/`source_dirty` from `/usr/bin/git` with the environment
      dropped and replace objects off. Skip-worktree/assume-unchanged entries, replace refs, grafts,
      any change, any untracked non-ignored file, and any file ignored by a rule outside a tracked
      `.gitignore` make the build dirty. It re-derives whenever the working tree changes, not only
      this crate's `src/` (A60). This is stricter than readiness's own untracked-file check
      (amendment 32's open item stays open there; both are superseded by amendment 44's one rule). A digest of the compiled sources is not
      recorded: a build script cannot see the compiled file set, and on a clean tree
      `fabric_revision` already names it.
    - The signing key must be mode 0400 (§2 rule 1); 0600 is refused and `keygen` writes 0400.
      `verify-evidence` names its trust root, the operator root and the build, and is
      `authoritative: false` unless the root is the operator's, passes the ownership walk, and the
      build is production (A61).
37. **An input's extended attributes are refused at every layer (PSV-2, C9 round-1).** A POSIX ACL
    entry `user:65534:---` on a 0644 file leaves the mode normalised and the tree digest unchanged,
    yet denies the test uid the file: a correct candidate was driven to a genuine keyed Failed.
    The rule is now about the child's effective view, not a list of shapes:
    - the guest input check refuses ANY entry, the input root included, that carries ANY extended
      attribute, and names it;
    - the launcher stages inputs with a plain `cp -R` (no ACL, no xattr; the exec bit kept) and
      builds each image with `mkfs.ext4 -E no_copy_xattrs`;
    - ~~the guest mounts all three input drives `noacl`~~ **Withdrawn in C9 round 1b.** The guest
      kernel's ext4 has no `noacl` option: with it, the input mount failed and the guest rebooted
      ("ext4: Unknown parameter 'noacl'"), measured by `psv_guest_boot_test.sh` after the image was
      rebuilt. The textual test of guest-init.sh had passed. In its place the runner refuses any job
      file (the job directory, the secret, the manifest) that carries an extended attribute, since an
      ACL on the job drive could GRANT the test uid the 0400 secret. The launcher layers are unchanged.

    The guest verdict's self-reported `runner.init_sha256` is renamed `runner.runner_sha256`: it is
    the digest of `axon-psv-runner` (`/proc/self/exe`), not the `axon-guest-init` that the
    manifest's `guest.init_sha256` pins. It is informational and never attribution: nothing compares
    it, and the runner is bound through the pinned rootfs. The verdict schema stays
    `axon-guest-verdict/1` (no verdict under the old field was ever certified; a host refuses the
    old field under `deny_unknown_fields`, which fails closed). Changing `guest-init.sh` and the
    runner makes the profile manifest's `guest_init`, `rootfs.sqfs` and `axon-psv-runner` pins stale
    until the image is rebuilt and re-pinned at freeze. Negative-matrix A53.
38. **Attribution is joined to the signer; one key, one authority root; a protected manifest
    names its host (C9 round 1, loop workstream).**
    - The recorded attribution of a counted protected trial must BE the signer that re-verified it:
      the verdict's issuer and key id, the context's `context_signed_by`, and its
      `context_observer_ref`. Amendment 30 only asked whether the named identity was trusted and
      rooted, so a record naming a second identity the operator root also holds was accepted
      (PSV-5). Negative-matrix A62.
    - ADR-002's key separation is checked at the operator roots, not only in the store config.
      `operator_trust::rooted` and `rooted_keys` refuse a key that another operator root also
      holds, so an observer key that is also a verifier (or qualification, admission, monitor) key
      authenticates nothing, in `check_bundle` or in any rooted lookup (PSV-6). The store's
      `check_separation` stays as a second check. The Fabric side is a separate workstream.
      Negative-matrix A63.
    - `check_bundle` refuses a launch manifest in which any `*sha256` field is all zeros. That is
      the placeholder Fabric's `psv::prepare` writes when there is no operator host config
      (`host_config_sha256`, `suite.registry_sha256`). `LaunchManifest::verify` in `axon-psv` is
      unchanged, because the guest runner also calls it for development launches (PSV-7).
      Negative-matrix A64.
    - **Accepted, open (class d; not fixed and not claimed):** `protected_scopes`,
      `task_acceptance` and `verifier_pins.check_suites` are still store config. A store writer can
      declassify a scope, or point a protected evaluation at another operator-registered suite or
      test. No development verdict is labelled protected by this, because the evaluation record
      then says development. This is ADR-001's accepted limitation (`v022-G01-claim.md`, "Accepted
      limitations"), recorded as a follow-up after amendments 10 and 12. Review rounds should treat
      it as accepted, not as a new finding. The negative matrix lists it under "Accepted, open".
39. **A kill is the row's own attack succeeding (mutation evidence; amends 28).** The mutation run
    counted any test failure as a kill. When a guard was removed and a DIFFERENT check still
    refused the attack, a reason-asserting test failed on the reason, and that was scored killed.
    That is the equivalent shape, not a kill. The C9 dev review measured it on M140, M201, M262,
    M263 and M285, and by evidence shape on M24, M27-M29, M205 and M214-M217, inside a reported
    "308/308 killed". The audit of all 308 recorded kills found 45 such rows.
    - Every active row has an attack marker (`scripts/v022_attack_markers.py`), checked both ways:
      no active row without one, no marker without a row. The marker states the attack getting
      through (accepted, counted, signed, launched, passed), never a refusal reason.
    - A row is KILLED only when the panic that FAILS its test matches its marker. That panic is the
      last one on the test's thread, whole message. It is not the first: M279 and M280 recorded a
      caught setup panic.
    - A failure on any other panic is REFUSED_ELSEWHERE. It is reported on its own line, is never
      counted killed, and fails the run. The report separates KILLED, REFUSED_ELSEWHERE,
      EQUIVALENT, STALE, survivors and unapplied rows.
    - A weak row is fixed in one of two ways. Either a test attacks the route where the row's guard
      is the only guard, and panics `ATTACK: ...` if it gets through (M24, M28, M140, M201, M205,
      M215, M217, M262, M263). Or the guard is dominated on every path and is retired under the
      four-cell rule with its all-paths argument recorded (M27, M29, M214, M216, M285, M287, M288,
      and the new M377, M378, M384, M385; M384 is ACTIVE again, amendment 42). Tests on those routes accept any refusal, because
      which independent layer refuses is not the property.
    - STALE ("the old text is absent") no longer shows that a guard is gone. M204's guard lived on,
      refactored, with no row. A stale row must name an ACTIVE replacement row, and both harnesses
      check that the replacement is killed by its own attack. M204 is ACTIVE again, re-anchored on
      the current observe seam. M176 is stale, with replacement M293.
    - The check_bundle joins that had no row now have one (M375-M384), and the bundle schema check
      has an only-guard attack (M383).
    Status: the evidence model and harness are implemented. Rows the round-1 audit found weak and
    that are not fixed yet are listed in the round's report. They show as REFUSED_ELSEWHERE, so a
    run cannot pass while they remain.
40. **How Axon learns whether a tree is its commit: one hardened git (FIELD-ORIGIN / PSV-7, C9
    round 2).** Amendment 36 hardened build provenance against the caller's environment, but the
    repository's own `.git/config` still steered git: `core.worktree` pointed `status` at a clean
    mirror, and `core.checkStat=minimal` with `core.trustCtime=false` hid a same-size edit. The
    guest manifest's `axon_tree_dirty_at_build` still came from PATH git, skipped untracked files
    and read "cannot tell" as clean. Readiness's git honoured a promisor remote, so a missing object
    was lazily fetched through the repository's `core.sshCommand`, as the verifier.
    - One implementation, `crates/axon-fabric/src/git_data.rs` (std only), serves readiness,
      `build.rs` and the `axon-provenance` helper. It never fetches (`GIT_NO_LAZY_FETCH=1`,
      `protocol.allow=never`), forces `--work-tree`, and overrides the stat-cache, excludes and
      attributes settings.
    - The repository's config is refused unless every key is on an inert allowlist (core layout
      keys, keys git_cmd overrides, remote url/fetch, branch tracking, user, lfs). It is located
      and parsed without running git inside the repository. An unknown key is refused, never
      interpreted. Readiness refuses such a repository before it asks git anything else.
    - Build provenance hashes every file of HEAD's tree (each object verified by hash) from the
      working-tree bytes. Git's stat cache is not trusted, since the index is a repository file:
      a forged stat entry hid an edit from `status`. A gitfile or symlinked `.git` is refused, so a
      verifier is built from a plain clone.
    - `scripts/linux_profile_manifest.py` compiles the helper with `rustc` and takes its answer.
      Cannot tell is dirty. `build-guest-image.sh` snapshots the tree before it builds. The
      manifest is clean only if the snapshot and the tree at manifest time are both clean and name
      the same revision. The reasons are recorded in `axon_tree_dirty_reasons`. The guest image
      contents are unchanged, so no boot test is needed. Fabric already refuses anything but a
      boolean `false`.
    - The build script's PCI lineage check (`merge-base --is-ancestor 31413ca7`) used PATH git
      too. It now asks the helper (`--descends`), which uses the same git and refuses
      `info/grafts`. History does not depend on which clone asks, so the lineage check accepts a
      linked worktree; the provenance of a linked worktree is always dirty.
    - Fabric's qualification also requires the B263 evidence record's `source.tree_dirty` to be
      false. `b263_qualify.sh` computed it from PATH git, untracked files excluded, and a failed
      git read as clean. It now takes the same provenance. That script needs KVM to run end to
      end, so only its wiring is tested; it has no mutation row.
    - Rows A65 and A66, mutations M450-M459. M346 (the skip-worktree refusal in provenance) is now
      dominated by the byte comparison (M451). It is a four-cell candidate, not retired here.
41. **One key, one authority root, at Fabric and readiness too (C9 round 2, keys workstream;
    PSV-6 + FIELD-ORIGIN).** Amendment 38 put key-role separation at the loop's roots. Fabric
    checked it only on the observer route, and read another root with `keys_in`, which answered
    an empty list on ANY `read_dir` error.
    - `operator_trust::keys_in` treats only NotFound as an absent root. Any other failure to list
      a root (EACCES for the non-root Fabric UID, ENOTDIR, ELOOP) refuses. Measured before the fix:
      as uid 65534, an observer key shared with a mode-000 verifier root passed separation.
    - Every Fabric and readiness trust-root read goes through one function,
      `backend::exclusive_root_keys`: the qualification trust (`qualification()`), the
      `verify_operator_evidence*` routes, the observer's `check_separation`, and readiness's key
      ids, observer keys, B263 keys and certification signature. A key another authority root
      also holds refuses. A present peer root is walked for operator ownership where the root is
      the operator's, as the loop's `exclusive` walks it. The peers are the root's siblings under
      the trust directory, derived from `TrustAuthority::ALL`, which for an operator root are
      exactly the roots the loop reads.
    - The host signer's public key is refused in every authority root but the verifier's: at
      `ProtectedHost::load` for the qualification, admission and monitor roots (and the observer
      root when no observer is configured; a configured observer's root stays
      `check_separation`'s), and at every `qualification()`, because the loaded qualification
      trust carries the signer's key. Readiness has no host config; there the signer's key is the
      verifier root's, so exclusivity refuses it in the qualification root.
    - Measured before the fix: Fabric signed a B263 record with its host key planted in the
      qualification root and `qualification()` accepted it; the same key signed a readiness
      certification and readiness said PASS.
    - Operational consequence: every authority root under `/etc/axon/trust` must be readable by
      the Fabric service UID, or protected launches and qualification refuse.
    Negative-matrix A67. Mutation rows M460-M466.

42. **The observation signer is recorded and joined; protected digests are sha256s; one suite-id
    parser; the qualification join is producer-side (C9 round 2, PSV-5, loop workstream).**
    - `check_bundle` verified the preflight observation under ANY key of the operator observer root
      and dropped the signer. It now also requires the signer to be the key the store registers for
      exactly one TRUSTED observer (the store narrows the root, as for the verifier) and returns it.
      Intake (`verification_observation_signed_by`) and EVL (`verification.observation_signed_by`)
      record it, and admission's `reverify_protected` requires the record to BE the signer the bundle
      re-verifies under, as for `context_signed_by` (A62). A missing or different attribution does
      not count. Negative-matrix A68.
    - A64 refused only the all-zero placeholder. Every `*sha256` field of a protected launch manifest
      must now be 64 lowercase hex and not all zero (`protected_evidence::is_sha256_hex`, over the
      same field walk). Fabric's `psv::prepare` refuses to build a manifest with any `*sha256` that
      is not 64 lowercase hex, so `verifier_identity()`'s `unknown` fallback never reaches a launch.
      The zero placeholder is still built for a hostless (never protected) launch. Receipt digest
      refs and the observation's digest fields are each joined to a manifest field or computed
      digest, so they inherit the format. Negative-matrix A69.
    - M384's retirement was false. `check_bundle` runs BEFORE `check_pins`, not after, and
      `check_pins` read the suite id with `split('@')`, so a manifest suite id holding `@` was
      refused only by M384. M384 is ACTIVE again; its killer is that attack at `check_bundle`'s own
      boundary. The source is fixed too: one parser (`axon_cortex::runner::check_suite_id`,
      `parse_check_suite_ref`, `check_suite_ref`) for Fabric's check registry (every registration,
      file or library), the receipt and manifest suite references, the loop's store pins (the config
      is refused when written and when read) and `check_pins`. On the intake, EVL and admission
      routes the store's config check now refuses an ambiguous pin before M384 is reached; M384
      stays the only guard at `check_bundle`'s boundary. Negative-matrix A69.
    - §9 says "the guest digests equal the qualification's", and the gap map said intake and EVL
      verify the qualification. They do not. The loop joins the receipt's `qualification-sha256`
      and guest digests to the manifest and the observation, and pins only the guest interpreter
      (`executable_digest`). The join of kernel, rootfs, init and interpreter to the B263
      qualification is PRODUCER-side: Fabric's dispatch takes them from the profile manifest whose
      bytes the qualification hashed (`psv::prepare`). Carrying the signed B263 record and the
      profile manifest in the bundle, and verifying both under the Qualification root, is a larger
      change than this round's scope, so the text is amended rather than the join added. A
      loop-side qualification join remains a FUTURE item.
    - Not done here (MINOR, not in this workstream): the manifest's `limits` are not joined to the
      request's limits.
43. **An only-guard route, or a retirement, for every weak or missing row (mutation evidence;
    C9 round 2, harness).** The round-2 EQUIVALENCE review found M177 scored KILLED on M436's
    E0901 refusal, markers that matched any non-pass or any refusal, and load-bearing guards with
    no row.
    - **The module order (M177 guest, M04 Fabric).** The security attack, a planted `helper.ax`
      steering a broken candidate to PASS, needs both the order and the first-match rule (M436)
      removed. The order alone guards another property: an HONEST candidate holding a module named
      like a suite module is judged by the suite's module. With the order reversed, M436 turns that
      into no verdict (E0901) and, without M436, a wrong one. Both rows are ACTIVE on that
      property. Their tests fire only on evidence that the candidate's file was resolved first
      (its E0901, the planted value failing the test, or, in Fabric, a verdict that differs from a
      control submission only by that file). M04 was retired in round 1b against M436. It is
      reinstated, because its full-suite cell now fails on that test. The joint security attack
      stays as a test on both routes.
    - **Markers.** Every marker that matched a bare non-pass verdict, a generic status or an
      `is_err_and(contains(reason))` shape now requires the attack's own evidence: the sibling's
      result line (M220), the attack's value 107 (M74), a control run (M44), the run reading its
      arguments (M140), a launched verdict rather than any non-`NotRun` (M192-M204, M227), an
      accepted forgery (M239, M241), a run status (M265, M310), a printed `Ok(())` (M324-M331).
      An `unwrap_err()` on an `Ok` marker is acceptance by construction: an `Ok` cannot come from
      a refusal, and a setup failure panics elsewhere.
    - **New ACTIVE rows (M480-M499).** `read_regular`'s O_NOFOLLOW and O_NONBLOCK, each arm of
      `execution_attestation_decision`, a service leaf being a directory, the signer key's
      O_NOFOLLOW, `no_xattr` failing closed, readiness's unwritable-root check (its enforcement,
      `writable_by_me`, and the production flag). For guest-init.sh: the input drives' `ro` and
      `noexec` options, the one-manifest-word rule (the script's own block, run against fake
      cmdlines), and `env -i` for the PSV runner and for the non-PSV workload.
    - **Retired (four-cell).** `read_regular`'s regular-file check (M482) against the non-blocking
      open and the one-read rule (M481, M335). With every decision made on one read, a FIFO serves
      nothing a regular file could not. `service_leaf`'s symlink check (M487) against `is_dir`
      (M486) and the mode check (M327). The tests on those routes accept any refusal, so neither
      retired check's reason text is asserted.
    - **guest-init.sh.** A row pins only the script's text. That the kernel applied it is the boot
      test's job: `psv_guest_boot_test.sh` gains a `mounts` case, in which the test child reads
      `/proc/mounts`. The `/out` bind mount and the serial digests are covered by the `pass` and
      `tampered` cases. The cgroup ceilings are B263 evidence, not a verdict property, and have no
      row. A boot SKIP (77) proves nothing.
    - **Full-suite cell.** For a retired row, the cell also runs every consumer of the guard's
      crate: its workspace reverse dependencies and, for axon-core, every package that execs the
      interpreter binary (it names `AXON_BIN`). The list is derived from the tree. Each consumer
      runs in its own configuration with `AXON_BIN` set. A consumer suite that fails on the clean
      tree makes the cell CONSUMER_BASELINE_BROKEN, never a pass.
44. **Operator decisions B, C and E (2026-09-29): strict protected counting; git-ignore has no
    security authority; protected answers come from a standalone clone (C9 round 2, bce
    workstream).** The operator adopted these as written.
    - **B, strict counting.** A trial counts as protected only with the whole chain. Each link and
      the check that enforces it:
      1. Qualified profile: Fabric `qualification()` at dispatch.
      2. Fresh observer evidence: Fabric `observer::observe` (pinned observer, operator observer
         root, role separation, epoch, age, one-use nonce); the loop's `check_bundle` (A68).
      3. Pinned privileged launcher: `launcher_pinned`; `launcher_sha256` joined from the manifest
         to the observation.
      4. Exact manifest: `psv::prepare`; `check_bundle` (A64, A69).
      5. Protected guest execution: a protected host runs only the protected profile, and only an
         operator suite (amendments 22, 35).
      6. Affirmative completion: `psv::derive` (keyed completion token, amendment 11).
      7. Protected verdict: `derive` and `check` (exactly the `protected` class).
      8. Verifier authentication: `verify_check_evidence` under the operator verifier root, then
         admission's `reverify_protected` (amendments 17, 20).
      9. Exact joins: `check_bundle`, `check_pins`, and the attribution joins (A62, A68).
      10. The attested execution leg (amendment 18): `evl::verify_execution` with
          `observed_protected_execution`.
      No code relaxes any link. Fabric cannot produce link 10 today, since no job can be both an
      execution and protected (amendment 35). So **zero protected trials count until an OBSERVED
      EXECUTION path exists**. The operator has accepted that consequence. Incomplete chains
      still count as attempt, cost, failure or unknown. Follow-up: an observed execution path,
      which needs a cross-peer manifest shape with MiCode. A read-only audit of every link found no
      route by which an incomplete chain counts as protected, so it added no row (the launcher
      workstream's rows were renumbered into A72-A75 at integration). It recorded two
      latent items for that follow-up:
      * A test-trust Fabric build is not refused at a protected launch, and nothing on the loop
        side pins the manifest's `verifier_sha256` to the installed verifier.
      * `observed_protected_execution` checks that its manifest and observation references are
        well-formed, and joins them to nothing. Its hex check also accepts upper case.
      Both are harmless while link 10 cannot be produced. Both must be closed when it can.
    - **C, git-ignore has no authority.** Readiness certification, build provenance (`build.rs`)
      and the guest manifest (`axon-provenance` helper) decide "is this tree its commit?" through
      ONE rule, `git_data::tree_differs`. It walks the working tree as a filesystem and compares it
      with HEAD's tree (for readiness, the certified tree, outside `governance/`):
      * a tracked path whose kind, mode or bytes differ is a change;
      * ANY other object is a change: an untracked file, a `.gitignore`d one, one hidden by
        `info/exclude` or `core.excludesFile`, a special file, or a directory that holds no
        tracked path (reported whole). `.git` at the top is not walked, and symlinks are not
        followed.
      The only exceptions are paths on the operator's allowlist, `/etc/axon/provenance-allowlist`.
      * It is walked from `/` like the trust roots: every component root-owned, not group- or
        other-writable, not a symlink. It is then read once as the regular file that was walked.
      * Format: the first line is `axon-provenance-allowlist/1`; then one entry per line (`#`
        comments). An entry is an exact relative path, or a directory prefix ending in `/` (that
        real directory and everything below it). There are no globs, no `.`, `..` or `.git`
        components, no leading `/`, and no whitespace. Anything else refuses the whole list.
      * An entry that names or holds a tracked path is refused, so the list can never excuse a
        source.
      * A missing allowlist excuses nothing. A present one that fails the ownership walk is
        refused (the answer is dirty), never read as empty.
      * Default contents for a normal build, installed by the operator as root, mode 0644:
        `axon-provenance-allowlist/1`, `target/`, `dist/`. `target/` is cargo's in-tree output,
        and `dist/` is the guest build's output. A certified build may instead set
        `CARGO_TARGET_DIR` outside the tree. Excusing `target/` trusts the build to start from an
        empty target: cargo reuses what it finds there.
      * Tests use `AllowlistSource::test` (test-trust builds only). `b263_qualify.sh` no longer
        writes `scripts/__pycache__/`.
      * Git's own views (status, the ignored-file check, readiness's untracked listing) are kept,
        and may only ADD reasons. The walk dominates M347 and M414 now. Both are four-cell
        candidates, not retired here.
      * Row A70; mutations M500-M503. M290 and M451 were re-anchored to the rule's call sites.
    - **E, a protected answer comes from a standalone clone.** Build provenance already refused a
      gitfile or symlinked `.git` (A65).
      * Readiness now refuses them before it asks git anything (`refuse_git_spoofing`), and
        `repo` must be the top of the clone.
      * The guest manifest binds the PCI lineage itself. It asks `axon-provenance --lineage`,
        which answers through `descends_from_protected` (standalone clone only). A tree that does
        not descend is dirty.
      * `build-guest-image.sh`'s early `--descends` check stays a development check: it accepts a
        linked worktree and can make nothing clean.
      * Row A71; mutations M504-M506. `descends_from_protected`'s own gitfile refusal is not given
        a row: in a worktree, the manifest is already dirty by A65's refusal (M453).
    - **Operational consequence.** A freeze, a certified verifier build and an evidence guest
      image run from a STANDALONE CLONE, never a linked worktree, with the allowlist installed or
      nothing generated in the tree. A guest image built in a developer worktree is always dirty.
      Fabric's qualification refuses it (RULE:manifest-clean), and that is intended.
      * The committed `profiles/linux-microvm/manifest.json` says clean at `3f81dc67`. That value
        was produced under the old rule, so the evidence image must be rebuilt under this one.
      * `scripts/v022_freeze_manifest.py` records `git rev-parse HEAD` and the profile manifest's
        revision and digests, but not whether its root is a standalone clone. It also does not
        record whether the guest manifest is clean.
      * Done at integration: the freeze now refuses a root whose `.git` is not a real
        directory (checked: it refuses the dev worktree), and refuses a guest manifest that is
        not `axon_tree_dirty_at_build: false` with `axon_tree_dirty_reasons: []` (checked: the
        committed manifest, built at 3f81dc67 under the old rule, has no reasons field and is
        refused). It binds both values.
      * The long-run target is unchanged: the input tree is digest X, and X is what was reviewed,
        launched and evaluated.

45. **Fabric runs non-root and reaches root only through `axon-protected-launcher`; authority
    programs are executed from the descriptor that was hashed (operator decisions A and D,
    2026-09-29; C9 round 2, launcher workstream).** Negative-matrix A72-A75; mutation rows
    M520-M549.
    - **Mechanism: a setuid-root helper, not a root daemon.** `axon-protected-launcher` (a bin of
      `axon-fabric`) is installed root-owned, mode 04750, group = the Fabric service's group, on a
      filesystem without `nosuid`. Fabric (non-root) executes it per launch and writes ONE request
      on its stdin. A daemon on a `SO_PEERCRED` socket authenticates the caller no better than the
      kernel's real uid does here, but it is a root process that is always running and listening,
      with a lifecycle, concurrency and a socket to protect. The setuid helper exists only for the
      duration of one launch. What a setuid program inherits from its caller is reset first,
      before anything is read: signal dispositions and mask, umask, working directory, every
      descriptor above stderr, the environment, resource limits (core 0; the rest raised). After
      the caller is authenticated the helper becomes root in every id (`setgroups(0)`,
      `setresgid(0,0,0)`, `setresuid(0,0,0)`), so the caller can no longer signal or trace it, and
      bash does not drop a setuid euid back to the caller's uid.
    - **Request (`axon-protected-launch-request/1`), per-launch data only, unknown fields denied:**
      jail id, out dir, the three PSV input dirs, the launch manifest digest, the guest policy
      bytes, the timeout. Nothing in it names an executable, a manifest, an artifact or a config,
      and no field is evaluated by a shell (each is one argv word). Every path must be a plainly
      spelled direct child of the operator's out root in Fabric's fixed shape (`<out>/<op>`,
      `<out>/<inputs>/{candidate,check,job}`); anything else is refused (exit 30, nothing
      launched).
    - **Operator config only** (`axon-protected-launcher/1` at the fixed path
      `/etc/axon/protected-launcher.json`, read through an `O_NOFOLLOW` walk from `/`, every
      directory and the file operator-owned and not group/other-writable): the Fabric uid (never
      0), the pinned interpreter (bash), the pinned launcher, the pinned profile manifest, the
      artifacts dir, firecracker and jailer paths, the out root, a root-private staging root, the
      timeout and input-size limits. The helper itself re-verifies the manifest at its pin and the
      kernel, rootfs, firecracker and jailer at the manifest's pins (each operator-owned, opened
      without following a symlink) before anything is acquired. The launcher script still copies
      and re-hashes them itself.
    - **What root touches of Fabric's.** The out root and the input dirs are opened from `/` one
      `O_NOFOLLOW` component at a time, ownership re-checked on each descriptor. The inputs are
      copied into a root-private snapshot: Fabric-owned regular files and directories only (a
      root-owned hard link, a symlink or a FIFO is refused), bounded by `max_input_bytes`, modes
      set to what the tree digest records. Fabric's job files (the per-attempt secret) are consumed
      at the snapshot, and the snapshot is removed when the launcher returns, before
      `--verify-result`. The out dir is created by the helper (root 0700) and given to the
      launcher as `/dev/fd/N`, so a rename in Fabric's out root cannot redirect a root write. The
      helper runs `--verify-result` itself, then hands the tree to the Fabric uid: directories,
      regular files and symlinks (never followed) are chowned, set-id bits cleared, and any other
      file type (a device node, a FIFO, a socket) is removed, whatever put it there.
    - **Fabric side.** The protected-host config must pin `privileged_launcher` (path + sha256).
      `ProtectedHost::operator` refuses a root Fabric and a helper config that admits another uid,
      writes under another out root, or runs another launcher or profile manifest than the host
      pins. Fabric executes the helper from its verified descriptor (D) and requires the report's
      `launcher_sha256` to be the launcher the launch manifest and observation name. A launch that
      did not go through the privileged helper is never protected: `psv_receipt` downgrades it to
      `guest-unobserved`, so no bundle is emitted (operator decision B's "pinned privileged
      launcher"). The development route (no helper configured: tests and dev hosts) still runs the
      launcher directly, same-byte, and cannot produce protected evidence. A test-trust helper
      (it accepts `--test-config`, a caller-chosen config) counts only inside a test-trust Fabric.
    - **Trust preflight.** Root is refused as every actor, including Fabric. The helper and its
      config, and every path that config pins, are operator files (probed like the others). New
      checks: the helper is root-owned, setuid, not group/other-writable, no access for other,
      group = the Fabric's; it admits exactly the `--fabric` uid; `--probe` run as the Fabric uid
      reports effective uid 0 (the setuid bit is honoured) and, in protected mode, a production
      build; for every other actor the kernel refuses the exec. MEASURED while writing this:
      `setpriv`'s own exec still holds root's DAC override (it ran a 0700 root-owned file as uid
      40004), so the exec is made by a shell already running as the actor. The existing probes
      were not affected: each runs a program that then makes the attempt.
    - **D: same-byte hash-and-exec** (`sealed_exec`, the one exec path for the launcher, the
      helper and the observer). The object is opened once with `O_NOFOLLOW`; on the descriptor it
      must be a regular file, owned by the operator (or root), not group/other-writable; a read
      lease is taken (the kernel refuses one while any process holds the file open for writing,
      and any later open for writing breaks it; the lease is set with no signal owner, since a
      break otherwise delivers SIGIO, which terminated the test process); the descriptor's bytes
      are hashed and compared with the pin; the command executes THAT descriptor with
      `execveat(fd, "", AT_EMPTY_PATH)`. A script is never executed through its `#!` line: its
      interpreter must be pinned too, is executed from its own verified descriptor, and reads the
      script as `/dev/fd/N`, a reopen of the same inode. In the child, immediately before
      `execveat`, each descriptor's identity (device, inode, size, mtime, ctime, mode, owner) is
      re-read and the lease must still be held; otherwise the exec is refused (ETXTBSY). The helper
      re-checks both after `--verify-result` and reports `unchanged`.
    - **Why this is "bytes authenticated == bytes executed" for an immutable inode.** The hash is
      over the descriptor, and the descriptor is what is executed, so a rename, a symlink swap or a
      directory swap changes only what the PATH names, never what runs (tested with a directory
      rename: the verified bytes run). The inode is immutable to every non-root uid: root-owned and
      not group/other-writable, so only root (the operator, inside the TCB) can open it for
      writing. A writer that already held it open (from before a chown or chmod) prevents the
      lease, so the open is refused. A root writer after the hash breaks the lease and the exec is
      refused, or the run is reported changed. The interpreter reads the same inode through
      `/dev/fd/N` from offset 0, so the bytes it reads are the bytes hashed while the inode is
      unchanged, and `unchanged` covers the whole run through `--verify-result`. Fabric, running
      non-root, cannot lease the root-owned helper or observer, so on its side immutability rests
      on the owner/mode check plus the identity re-check. That is the stated operator model:
      only root writes root-owned files.
    - **What would still need a compiled launcher (FOLLOW-UP, not done here).** The launcher is a
      738-line bash script. Its own bytes and bash's are pinned and executed same-byte, but it runs
      unpinned root-owned host tools from a fixed PATH (python3, coreutils, e2fsprogs `mkfs.ext4`
      and `debugfs`, iproute2, util-linux); the out-dir and manifest descriptors it inherits are
      closed only by the pinned jailer (`close_range`, seen in the pinned binary) before the VMM
      runs. A compiled launcher would pin or remove those tools, close every inherited descriptor
      itself, and let the helper execute one pinned binary with no interpreter.
    - **TCB of the root side:** the kernel; the helper binary (its Rust std, serde_json, sha2, ring,
      libc); the operator config file; the pinned bash and launcher; the root-owned system tools
      above; the pinned firecracker and jailer; the pinned kernel and rootfs. The dynamic loader
      and shared libraries of bash and the helper are root-owned system files, unpinned.
    - **Candidates for four-cell retirement, not rowed (each dominated on every path, argument
      given; none retired here):** the parent-side `unchanged()` in `sealed_exec::command` and the
      post-hash `unchanged()` in `open_verified` (the same `check_unchanged` runs in the child
      before every exec: M524/M527); the leaf-name check in `validate_request` and the
      `parent.join(leaf)` equality beside it (each implies the other; M533 left unused); the
      helper's `O_NOFOLLOW` on the input and out-root walk (every object reached is then required
      to be Fabric-owned or operator-owned on its descriptor, so a followed link reaches nothing
      Fabric could not already read); the set-id clear in the hand-over (the kernel's chown already
      clears S_ISUID/S_ISGID); the report's `unchanged` flag and Fabric's check of it (a changed or
      lease-broken inode already refuses the `--verify-result` exec, and the outcome is unknown).
      Paths that only a PRODUCTION build takes cannot be exercised by the test-trust harness: the
      helper's refusal when its effective uid is not 0, its refusal of `--test-config`, and
      Fabric's refusal of a `privileged_launcher.test_config` key.
    - **Real guest, through the helper.** `scripts/psv_guest_boot_test.sh` gains a `helper` case:
      the pass case launched by a non-root uid through a setuid-root (test-trust) helper, with the
      real launcher, image and engine, the launcher and bash executed from their verified
      descriptors and the out dir and manifest handed over as `/dev/fd/N`. Running it found a
      launcher defect: the cleanup's leftover-process check (`pgrep -f -- "--id $ID"`) matched
      the launcher's OWN command line whenever the caller names the id, which Fabric always did
      and the helper does, so every such launch ended cleanup-incomplete (24, unknown). The
      check now excludes the launcher's own pid, and runs `pgrep` as a simple command (a
      command-substitution subshell carries the same command line and matched too). The guest
      image is unchanged. Result on the current image (built at 3f81dc67): 12/12 PASS, the
      previous 11 plus `helper`.
    - **Before a protected host can be deployed:** install the production helper and its config
      (operator; this change writes nothing under /etc); run the Fabric service as its own non-root
      uid with the helper's group; run `trust_root_preflight.sh` in protected mode on the host;
      re-pin the launcher (its bytes changed here) in the host config and the helper config;
      exercise the PRODUCTION helper build on the host (the boot test uses a test-trust build,
      which takes `--test-config`); decide whether the out root and the staging root may share a filesystem with anything
      else; and the compiled-launcher follow-up above if the operator wants the script's tools out
      of the TCB.
46. **Sealed code acts only on its own operations; a test completes only when its body ran to its
    end (C9 round 3, core workstream; PSV-1 and PSV-3 blockers).**
    - **PSV-1, handlers.** A candidate's `with handler` answered builtins that the OPERATOR's code
      performed: a closure the suite handed the candidate, called inside the candidate's `with`.
      The arm aborted the operator's listener at its `println` (the assert never ran), or resumed
      its `read_file` with bytes the candidate chose. Both gave a keyed pass for a wrong answer
      through the real runner. The rule now lives in one predicate, `Interp::handler_may_answer`:
      a SEALED handler frame may answer or abort an operation only if no OPERATOR frame has been
      entered since the frame was installed. Otherwise the search goes on outward to an operator
      frame, or the real builtin runs. `with_frame` is the only place provenance changes, and it
      counts active frames of each provenance, so the check is exact. It covers a third route the
      review did not execute: the operator's listener calls a CANDIDATE function, and the
      candidate's outer handler aborts at that function's `println`. The operation is sealed, but
      the abort would unwind through operator code. A check on "is the operation's frame sealed"
      misses that route (row M562 is killed by exactly it). The same predicate gates the
      multi-shot replay feed. A sealed arm's `resume(v)` is never the result of an operator
      operation during the replay: that operation cannot be re-fired, so it is E1314. An operator
      handler still answers anything, including an operation candidate code performs inside it.
      The candidate's handler still answers the candidate's own operations. `run_handler_arm` is
      the one frame-selection point; `host_await*` and every effectful builtin reach it through
      the same interception in `call_builtin`. No other code selects a frame.
    - **PSV-1, RNG.** Operator code that runs ABOVE a sealed frame is refused the operator RNG
      (`Interp::rng_next` and `rng_reseed`, the only draw and reseed paths). Examples are a closure
      the suite handed the candidate, or an operator handler arm answering a candidate operation.
      The run fails with a clear error. The candidate decides how many times such code runs, so a
      draw there would advance the operator's stream by a count the candidate chose, and steer
      the operator's next challenge (review run3.log: 652 after one call, 125 after five). We
      REFUSE rather than give that code a third stream. A third stream is still advanced by
      call count, so what the closure draws stays candidate-scheduled. A refusal is fail-closed
      and visible to the suite author, who can draw before handing code to the candidate. The
      candidate's own draws (sealed kernel) and the operator's draws with no sealed frame below
      them are unchanged. The superseded note is PCI spec row 10's "a closure the suite hands to
      the candidate runs under the candidate's handlers by the suite's own choice". A closure the
      suite hands over still runs where the candidate calls it, but candidate handlers no longer
      reach it and it cannot draw the operator RNG. The PCI spec is not edited here.
    - **PSV-3.** `run_test_fn_inner` decided Completed from the returned value's tag. `?` on a
      type-confused `None` returns `None`, which is not `Err`, so it read as a completion and a
      genuine token was minted. The single source is now whether the TEST frame's body evaluated
      to its end. `call_fn_frame` records it for the test's depth. Any `Flow::Return` there, from
      `?` or `return`, whatever it carries, is EndedEarly and gets no token. This is stricter
      than before for an explicit `return` in a test body: `axon test` still reports it as
      passing, but it no longer carries completion evidence. The confusion itself is closed where
      it is cheap and sound. A `fn` declared `-> Result` that returns `Some`/`None`, or one
      declared `-> Option` that returns `Ok`/`Err`, panics at its return boundary. The review's
      exact candidate now FAILS on that confusion.
    - **Language follow-up (not fixed here; this note corrected by amendment 53).** `dict_get`,
      `dict_get_or` and `host_await_val` return a free type variable, so a stored value of any type
      unifies with any use. The return-boundary check above compares ONE thing: the constructor
      family of a declared `Result` or `Option` return (`Some`/`None` from a `-> Result` fn,
      `Ok`/`Err` from a `-> Option` fn). It checks no scalar kind, no struct or enum name, no
      payload and no element, no parameter, and no closure return (a closure has no declared return
      type at run time). This note used to say a confused value is still caught "at the next
      declared `fn`". That held only for a `Result`/`Option` mismatch: a struct, scalar or `str`
      returned from a fn declared `-> i64` crossed silently, and the round-4 review turned that into
      a keyed PASS (the operator's `r.ok()` dispatched on the runtime type to the candidate's
      method; amendment 53, matrix A86), which is a rubric substitution, not only a completion
      issue. An `Option`-returning test whose `?` meets a well-typed `None` is the route where the
      completion rule is the only guard; a test pins it
      (`a_test_ended_by_question_mark_is_never_completed`, `t_find`).
    - **Rows.** M560 (the frame-selection filter), M561 (the replay feed), M562 (counting operator
      frames rather than reading the current provenance), M563 (the RNG refusal), M564/M565 (the
      completion decision and the flag it reads), M566 (the return-boundary check). Negative matrix
      A76 and A77. The guest image must be REBUILT to carry the new interpreter; its scripts and
      runner are unchanged.

47. **Readiness applies Fabric's B263 rules; decision E by what git acts on; lineage from
    hash-checked objects (C9 round 3, readiness workstream).**
    - **One set of B263 rules (PSV-7).** Readiness checked only the signature, schema, profile and
      guest digests of the B263 record a certification names. An operator-signed record that
      FAILED, was stale, came from a dirty tree or named no host certified PASS, and readiness
      stayed PASS after the host's qualification lapsed, while Fabric refused every protected
      launch. The record's rules (issuer-claimed, fail-zero, pass-count, blocked-count, result,
      waivers, end not future and within the maximum age, engine digests, tree-clean, host, caveat)
      are now one function, `backend::accept_b263`, called by Fabric's `qualification()` and by
      readiness. Readiness judges currency at DECISION time (system clock, Fabric's default 30
      days), takes waivers only from certified, qualification-signed waiver files bound to the
      record, joins the observation's `firecracker_sha256` to the record's engine, and requires
      `certified_at` to parse, to be no earlier than the observation's `observed_at`, and not to be
      in the future. The manifest rules (engine pins, manifest-clean, manifest identity) stay
      Fabric's, since they are about the host's installed manifest. The readiness fixture's B263
      record is now a genuine qualifying record. Negative-matrix A78.
    - **Decision E on the repository git acts on.** `git_data::discover` accepted any real `.git`
      directory. A linked worktree's admin dir copied in as `.git` (its `commondir` naming another
      clone) got a clean, descending answer from readiness, build and guest provenance and the
      freeze. `discover` now also requires the hardened git's `--git-common-dir` to be `top/.git`.
      `v022_freeze_manifest.py` applies the same rule through `/usr/bin/git` with the caller's
      environment dropped, which also stops a caller's `GIT_DIR` from choosing the bound
      `axon_sha`. Negative-matrix A79.
    - **Lineage from verified objects.** Readiness and `provenance::lineage` used `git merge-base
      --is-ancestor`, whose commit walk never checks an object's hash. `git_data::descends` walks
      from HEAD through `Objects` (every commit hashed against its name, parents parsed from those
      bytes) and stops at the revision or a root. The review judged the readiness route immaterial
      because of its tree comparison; executed, it was not: the forged ancestor certified PASS,
      since the orphan carried the certified tree. Negative-matrix A80.
    - **Rows M570-M584**, each killed by its own attack. Tested but not rowed (the allocation is
      spent): the `certified_at` parse, the moved RULE:issuer-claimed on the readiness route, and
      the freeze script's caller-environment drop.

48. **Every refusal on the helper path has a row or a stated reason; guest mounts and the runner
    environment are judged by behaviour; kept four-cell records must be current (C9 round 3,
    harness workstream; rows M585-M609).**
    - **Rows derived from the code.** `scripts/v022_refusal_coverage.py` (run by `gate.sh`) lists
      every refusal site in `privileged_launcher.rs`, `sealed_exec.rs` and
      `bin/axon-protected-launcher.rs` (`return Err(`, `Err(format!`, `refuse(`, `Err(bad(`, and
      each read of `TEST_TRUST_BUILD`). Each must be overlapped by a registry row or sit on its
      exemption list with a reason (an OS error that fails closed, an operator-authored field of
      the operator-owned config, or a named dominating check). It fails on an unrowed site, a
      stale or redundant exemption, and a row whose text no longer applies. `--without=M…`
      shows it naming a site once its row is dropped.
    - **New ACTIVE rows, each attacking the route where it is the only guard:** the config file's
      mode (M585) and owner (M586); the ancestor walk (M587) and `operator_dir`'s owner and mode
      (M588, M589); the helper's own re-verification of kernel, rootfs, firecracker and jailer
      (M590: a firecracker with the pinned bytes owned by another uid, which the launcher's
      sha256 check would pass); `Lease::Required` (M591); the post-hash `unchanged()` (M592: for
      the kernel, rootfs and engine, which are verified but not executed, nothing re-checks them
      later, so amendment 45's listing of it as dominated was wrong for them); `MAX_BYTES`
      (M593); the staging root being operator-owned (M598: a Fabric-owned root lets the Fabric
      swap the snapshot the launcher then reads); the inputs dir's owner (M599); the jail id
      (M600: `../x` staged the inputs outside the staging root); `build_name` (M603).
    - **Production-build paths are now exercised.** Every `cargo test` build is test-trust, so
      `tests/privileged_launcher.rs` builds the helper and Fabric a second time, without the
      feature, into a target dir of its own. Against that build: a setuid helper offered a
      root-owned config through `--test-config` must refuse it (M601); a helper running as the
      Fabric uid with lease, DAC-override, chown and fowner capabilities, its config at
      `/etc/axon` on a tmpfs in a private mount namespace (the host's `/etc` is never written),
      must refuse on its euid (M602); and the verifier manifest must report that a test-trust
      helper's route never attests protected (M605).
    - **PSV-4.** The report-to-route derivation is `LaunchRoute::of_report` (M604, unit tested with
      a test-trust report), and `psv_receipt` calls `backend::attests_protected` (M606), the one
      reader of `TEST_TRUST_BUILD` for this decision. One mutant stays structurally untestable:
      rewriting the call in `psv_receipt` to an inline `may_attest_protected(true)` equals the
      real code in every test build. It is named here rather than counted.
    - **Retired as mutual pairs (four cells executed):** the interpreter-is-a-script refusal
      (M594) and the verified descriptor being close-on-exec (M595): the kernel refuses to run a
      `#!` script executed from a close-on-exec descriptor (ENOENT, measured), and no caller
      inherits the interpreter's descriptor. The staging root's 0700 (M596) and the per-launch
      staging dir's 0700 (M597): either one keeps another uid out of the snapshot. The same
      reasoning makes M529 (a script program with no interpreter) equivalent-shaped: its recorded
      kill is the refusal message, while the exec itself would fail with ENOENT.
    - **Guest mounts by behaviour.** M493-M496 were killed by a text test that passed with `,rw`
      appended, which both util-linux and busybox mount apply as read-write. They, and new rows
      M607-M609 (a later `rw`, `exec`, `dev,suid`), are now killed by running the script's own
      mount block on loop-device ext4 images, as root in a private mount namespace, and reading
      `/proc/mounts`. M498/M499 run the workload block with a polluted PID-1 environment and read
      the exec'd child's environ. The boot test's `mounts` case still checks the same property
      inside a real guest; this one runs in `cargo test` without a VM. The text test remains for
      a non-root lane and now reads the effective option set.
    - **Paired-disable `--only` keeps only current records.** A kept record needs the git blob of
      its row's file, its test files (the test module, or the integration test and its
      `tests/common`) and every sibling's file to be unchanged since the record's commit. Stale
      records refuse the run, or are re-executed with `--reexecute-stale`. `--check-stale` lists
      them: 24 of the 33 records in the status file at bf964212 are stale by this rule.
49. **The suite join has one reading; the launch names its authority (C9 round 3, loop
    workstream; PSV-5 and PSV-6; matrix A81, A82; rows M610-M618).**
    - **Suite join (A81).** `check_bundle` formatted the manifest's suite as
      `{id}@{version}#{entry}` and compared that string with the receipt's `check-suite:` ref. A
      manifest naming version `V#x`, entry `accept.ax` therefore joined the pin
      `check-suite:acceptance@V#x#accept.ax`, which the one parser reads as version `V`, entry
      `x#accept.ax` (reproduced on bf964212: the reviewer's case was recorded as a protected
      verdict). The receipt's ref is now read by `parse_check_suite_ref` and compared with the
      manifest field by field; the request's `check:<id>` is compared with the parsed id. The one
      writer, `runner::check_suite_ref`, now returns an error unless the reference it writes reads
      back as exactly its (id, version, entry); Fabric's `register_check` (every registration, file
      or library) and `psv::prepare` go through it, and Fabric writes a suite's reference once,
      where the suite is resolved. `check_pins` compared the argv id as a string prefix of the
      recorded ref; it now compares the parsed id. The parser already refused `@` and `#` in a
      version and a separator in an id. An entry may still hold `#` or `@`: it is the tail after
      the version's first `#`, so with the id and version separator-free every reference has one
      reading, and the second readings were all consumers that did not use the parser.
    - **Launch authority (A82).** The launch manifest is now `axon-launch-manifest/2`: it carries
      `authority` = {`epoch`, `tenant_id`, `task_family`}, the scope's authority epoch the launch
      was made under and the scope. The observer receives the manifest, and its signed
      `intended_launch_manifest_sha256` covers the authority. `check_bundle` takes the trial's
      scope beside its epoch (intake passes the episode's, which it binds to the scope pointer) and
      refuses a manifest naming another epoch, tenant or family. On a protected host the
      launch-time epoch is read from the store the host config pins (`authority_store`, optional
      key; its parent directory is operator-owned and the trust preflight probes it as
      `authority-store`). A caller `--store` that names another store is refused; one naming the
      same store is accepted, as MiCode passes it; a host config that pins none launches nothing.
      The preflight observation's own shape is unchanged: its `epoch` is still joined to the
      trial's epoch, and now also transitively to the manifest's.
    - **Consumers.** MiCode (`micode-persist/src/fabric_check.rs`) keeps the `psv_evidence` bundle
      verbatim and parses no launch manifest, so nothing there breaks. On a protected host its
      `loop_store` must be the host config's `authority_store`. The guest runner verifies the
      manifest (`LaunchManifest::verify`, `deny_unknown_fields`), so an image built before this
      change refuses every `/2` manifest: **the guest image must be rebuilt** and
      `psv_guest_boot_test.sh` re-run on it before this counts as proven in a guest. An operator
      observer program must read the manifest's `authority.epoch` rather than any fixed value.

50. **The observation nonce belongs to a CUSTODIAN, and the root helper launches only on its one
    observation (operator decision D6; C9 round 3, custodian workstream; PSV-6 findings "there is
    no separate custodian" and "the setuid-root helper launches without any observation").**
    Negative-matrix A83-A84; mutation rows M620-M639. Supersedes the nonce parts of §7, of
    amendment 35 (A56's `nonce_store` leaf) and of amendment 45's helper description; §2's
    example `observer.nonce_store` is superseded by `observer.custodian`.
    - **Before.** Fabric issued the nonce (its own RNG), stored it (a Fabric-owned 0700
      `nonce_store`, which a protected host REQUIRED to be Fabric's: `service_leaf`) and spent it
      (`observe`). "Used once" was enforced by the principal the observation constrains. The
      setuid-root helper's request carried no observation, so the Fabric uid could launch any
      number of times for one observed manifest, or with none.
    - **The custodian (`axon-custodian`, a bin of `axon-fabric`; `custodian.rs`).** Its own uid,
      socket-activated by systemd (example units in `profiles/protected-host/systemd/`, config
      example `profiles/protected-host/custodian.json.example`; nothing is installed). It reads
      only `/etc/axon/custodian.json` (`axon-custodian/1`: `custodian_uid`, `fabric_uid`,
      `launcher_uid`, `socket`, `store`, `max_age_s`), walked from `/` operator-owned by the same
      reader as the helper's config. A protected start refuses: a custodian uid equal to the
      Fabric's or 0, a Fabric uid 0, a spender (`launcher_uid`) other than 0; running as another
      uid than `custodian_uid`; a store that is not its own 0700 directory, or whose parent chain
      is not the operator's; no systemd activation (`LISTEN_PID`/`LISTEN_FDS`), or an activated
      socket at another path. Callers are authenticated by `SO_PEERCRED`: `issue` (a fresh
      128-bit nonce bound to an epoch) only for `fabric_uid`; `spend` only for `launcher_uid`,
      and a nonce spends once (the existing atomic rename, now inside the custodian's store; the
      `.used` record then names the manifest it was spent for). Clients authenticate the custodian
      the same way: the socket's listener must be the custodian uid, or root (systemd binds an
      activated listener, so `SO_PEERCRED` on the client side reports PID 1). Every reply states
      the custodian's mode: `protected`, `test` (a test-trust build's `--test-config`) or `dev`.
    - **DEV.** `axon-custodian --dev --socket P --store D` is a manual launch that binds its own
      socket and answers `mode: dev`; Fabric may also keep an in-process store in development
      (`Custodian::InProcess`). Neither ever yields a protected launch: the helper refuses a dev
      custodian's spend, and it spends only through the custodian ITS operator config names,
      which never issued an in-process nonce.
    - **Fabric.** The host config's `observer` section names `custodian: {socket, uid}`; a
      `nonce_store` there is refused. `ProtectedHost::operator` refuses a custodian uid equal to
      Fabric's euid or 0; the socket's directory is ownership-walked like `out_root`'s parent.
      Fabric holds only a client: it is ISSUED the nonce and puts it in the manifest; the
      observation is verified as before (an early check, so the root helper is never asked to
      launch on an observation Fabric can already refuse), but Fabric spends NOTHING. The
      decision: the spend moves entirely to the root boundary. A Fabric spend first would leave
      the helper nothing to spend unless the record had a second state, and a check performed by
      the constrained principal proves nothing to the boundary that must hold against it; the
      custodian refuses a Fabric spend outright (`launcher_uid`). The direct (development) route
      has no root boundary, never counts (amendment 45), and spends nothing.
    - **The helper (replaces amendment 45's request and config bullets).** Request
      `axon-protected-launch-request/2` adds `observation` (the exact bytes Fabric verified) and
      `observation_signature`; config `axon-protected-launcher/2` adds `observer: {root,
      max_age_s, host_signer_public_key}` and `custodian: {socket, uid}` (a uid equal to the
      Fabric's or 0 is refused). After the inputs are snapshotted into the root-private staging
      dir, and before the out dir exists, the helper: requires the snapshot's
      `job/launch-manifest.json` to have the request's `psv_manifest_sha256`; verifies the
      observation with `observer::verify_observation`, the ONE function Fabric's `observe` also
      uses (observer root operator-walked, key-role separation including the host signer,
      observer-domain signature, signer = claimed key, field-for-field join to that manifest and
      digest, freshness); then spends the manifest's nonce through the configured custodian with
      the observation's epoch, as root. Only a protected custodian's spend (a test custodian's
      inside a test-trust helper) authorizes the launch. Any failure: exit 30, nothing launched.
      `helper_agrees` holds the helper's custodian and host signer to the host config's.
    - **Where this meets the epoch workstream (amendment 49).** The launch manifest is
      `axon-launch-manifest/2` and names `authority {epoch, tenant_id, task_family}`. At the root
      boundary the observation's epoch must equal the snapshot manifest's `authority.epoch`, and
      the custodian holds the same epoch to the one the nonce was issued for. So the epoch the
      root launch is authorized under is the one the manifest (and the digest the verdict binds)
      names, not a claim beside it (mutation row M639).
    - **Trust preflight.** `axon-fabric protected-host-paths` lists the custodian's config
      (operator file), its store and socket, and its three uids; `trust_root_preflight.sh`
      requires the custodian actor to be the config's `custodian_uid`, separate from the Fabric's
      and not root, issuing to the `--fabric` uid and spending for root; the store to be the
      custodian's own 0700 directory that the custodian can create in and the Fabric, the verifier
      and every agent cannot (create or chmod); and the socket directory to be the operator's.
    - **Tests (each an ATTACK and a control).** One observation replayed for a second helper
      launch (and in the real guest boot test); a helper launch with no observation, a minted
      one, or one signed with the host signer; an observation of another manifest, and a snapshot
      other than the named manifest; a Fabric-served custodian socket (root); Fabric writing the
      custodian's store, and a store the Fabric owns (root); a non-Fabric uid issued a nonce and
      a Fabric spend (root and unit); a dev custodian and an in-process Fabric nonce never
      launching; config and host rules. Existing rows M196 (the custodian's spend), M292 (the
      socket directory is the operator's) and M325 (the store is private) are re-pointed to the
      guards that replace theirs; M291/M324/M326/M327 follow their renamed tests.
    - **Operator deployment this adds (PROTECTED_ONLY).** Create a custodian system user (its own
      uid and group, no login); install the production `axon-custodian` (root-owned, pinned like
      every authority program), `/etc/axon/custodian.json` (root-owned 0644), and the two systemd
      units; enable the socket; add the `observer.custodian` section to
      `/etc/axon/protected-host.json` and the `observer` and `custodian` sections to
      `/etc/axon/protected-launcher.json` (the helper's config schema is now `/2`); run
      `trust_root_preflight.sh` in protected mode with `--custodian` set to the custodian user.

51. **Every row is killed by its own attack, or retired against a named sibling; two defects the
    rows wave found (C9 round 3).** The full mutation run at `1084ed1c` left 15 rows that were not
    killed by their own attack (9 refused elsewhere, 2 survivors, 4 unapplied). None was counted.
    - **Re-attacked (the guard is still the only guard).** M72 (a `return` escaping a candidate
      predicate through an operator helper); M194/M201/M204 (the direct route, where no privileged
      helper re-verifies the observation); M310 (a library-composed protected host with a launcher
      Fabric runs itself); M284 (the development lineage under the caller's `GIT_DIR`).
    - **Re-anchored on the guard's current form.** M218/M230, M270 and M576.
    - **Retired EQUIVALENT_DID** (four-cell records, all-paths argument in `EQUIV_RECORD`; never
      counted as killed): M186 (vs M606 + M620), M286 and M459 (vs the hashed ancestry walk M581),
      M453 (vs the common-dir rule M580), and M602 (vs the custodian spend rule M628). The
      `launcher_uid == 0` half of M602's guard became its own row, M640.
    - **Defect: a protected custodian could never start.** Its store-parent check listed the
      parent's entries, and the custodian-owned store is one of them. The rule is now the parent
      CHAIN only (`backend::check_operator_chain`). Row M641; matrix A83. M602's test now drives the
      production helper against the production, socket-activated custodian.
    - **Defect: a ref answered the lineage** (matrix A85, row M642). `descends` resolved the
      revision with `git rev-parse`, which prefers a ref to an abbreviated hash. A branch named like
      the certified abbreviation made an orphan HEAD descend. `git_data::object_named` now resolves
      by object name only, and refuses a ref name, an ambiguous prefix and a prefix naming nothing.
      This is the same class as A80: the repository under review must not choose which object a
      certified name means.

52. **Evidence runs build and run the same bytes; paired-disable shards; development-only compiler
    cache (C9 round 4, harness).** Nothing here relaxes a counting rule. Equivalents are still never
    counted as killed, and all four cells plus the consumer cells stay.
    - **A harness script runs what it builds.** `cli_run.rs` spawned 51 harness scripts under the
      caller's `CARGO_TARGET_DIR`. The scripts `cargo build` into that directory and then exec
      `target/debug/<bin>` by path (76 scripts hard-code it). Every integration and paired-disable
      run sets the variable, so their native and wasm parity legs judged whatever binary was already
      in the tree's `target/`: at 05d78061 that was an axon built at 2caab1b2. Fixed at the one
      spawn: `harness_bash()` removes the variable, and
      `every_harness_script_is_spawned_with_its_own_target_dir` refuses a bare spawn. Integration
      at 291f6065 is the first run whose parity legs built and ran the compiler under test.
    - **Tests whose attack no longer reached its guard.** Amendment 50 gave `submit_with` an
      automatic observer but left `only_an_operator_suite_runs_on_the_protected_profile`'s
      hand-built config unobserved. With M187 and M400 both removed, the helper refused for want of
      an observation, and the test still passed. It is now observed; its four cells hold. The
      launcher test's request write treated a broken pipe (the kernel refusing the exec, 126, before
      stdin is read) as a panic; that flake is fixed.
    - **Sharded paired-disable.** `--shard=K/N` runs one slice in its own standalone clone and
      target dir. `--join` writes the status file only when the shards are exactly 0..N-1 of one N,
      all at HEAD, each holding exactly its slice, and together covering every record once
      (`scripts/test_v022_paired_disable_join.py`, 8 refusals and 2 controls). The harness builds
      every binary the suites exec before any cell runs, and refuses to run without them. A failing
      full-suite cell keeps its output.
    - **Compiler cache for development only.** sccache runs only in the development evidence
      runners (`RUSTC_WRAPPER` in the runner, never global). A cache is an input the source tree
      does not contain (decision E), so `v022_freeze_manifest.py` refuses any rustc wrapper (row
      M650, test `a_compiler_wrapper_does_not_freeze`). `build-guest-image.sh` refuses one as its
      first statement; it has a stated reason instead of a row, because removing that refusal makes
      a mutation run perform a full image build. The refusal was exercised by hand (exit 2).
    - **Known limits carried into round 4.** The integration's `axon-core` leg runs
      `--no-default-features`, so its in-process codegen tests record SKIP ("axon build (no codegen
      feature)"); the parity scripts build their own codegen axon and do run. Android legs SKIP (no
      NDK or emulator). These are skips, not passes.

53. **The candidate never chooses the code that runs under the operator's judging method: every
    value is cast at every declared boundary, and an operator method name is the operator's (C9
    round 4, core workstream, PSV-1; matrix A86).**
    - **Before.** A method call selects its method by the receiver's RUNTIME type
      (`self.methods.get(&(argv[0].type_name(), method))`), and nothing tied that type to the one
      the operator's code saw. Two routes reached a keyed PASS for a wrong candidate through
      `axon_psv::runner::run` (review logs `/var/tmp/c9r4-psv1/`): (a) a TYPE CONFUSION: `dict_get`'s
      value type is free (`Deferred`, which unifies with anything), so a candidate's
      `pub fn solve(n: i64) -> i64` returned its own `Fake`, and the operator's `r.ok()`, meant for
      `impl Judge for i64`, ran the candidate's `impl Mine for Fake`; the same with the operator's
      impl on the candidate's interface type `Point`. Amendment 46's return check compared only
      `Result` against `Option`. (b) NO confusion at all (found while fixing (a)): the candidate
      DECLARES `-> Fake`; the checker resolves a method call by name only, so `r.ok()` still type
      checks, and ran `Fake::ok`. And `check_sealed` walked only expressions, so a candidate could
      name the operator's types and traits in signatures, fields, bounds and impl headers.
    - **After (the value boundary, `interp/conform.rs`).** One cast, `Interp::cast`, applied at
      every point where a value meets a DECLARED type: a named fn's or method's arguments at entry
      and its result at return (`call_fn_frame`, generalizing amendment 46's check, which is now
      the `confused` decision of this cast), a closure's arguments and result under every
      `fn(..) -> ..` type the reference crossed (a per-reference contract chain, gradual typing's
      function proxy, so one closure used at two types is not confused), a lambda's own parameter
      annotations, every value sent on a channel under the element types the channel object
      crossed, a `let x: T` annotation, and each field of a struct or enum literal. What is cast:
      scalar kind (every integer width is one kind), `str`, `bool`, `()`, `Decimal`; a struct or
      enum by name and then its fields against the declared field types; `Option`/`Result` by
      constructor and then the payload; arrays and tuples element by element; a refinement by its
      base; `dyn Trait` and a type parameter's bounds by an `impl` of that trait. A type parameter
      is BOUND from the first value that meets it (the arguments in order, or a closure's first
      result) and every later value must agree. At a seal crossing (a sealed fn or closure
      returning to operator code) a value at a type parameter nothing determined is refused: by
      parametricity no honest body produces one (`fn solve<T>(n: i64) -> T`). A declared type the
      interpreter cannot read (an unknown name, `Dict`, `Uncertain<T>`, a handle) is accepted: the
      cast refuses only what it can SHOW is another type, so no honest program is rejected (the
      full axon-core suite and every example stay green).
    - **After (the dispatch edge, `Interp::seal_method`).** In operator code, a method name the
      operator's code defines (in its impls or traits) is the operator's: a call that would
      dispatch it to a SEALED method is refused. The candidate's own method names (its API) stay
      callable from the suite. Route (b) has no confusion, so the cast cannot see it; this edge is
      its only guard, and it also stops (a) on its own.
    - **After (static, E0004).** `check_sealed` walks type positions too: parameter and return
      types, struct and variant fields, refinement bases, trait method signatures, impl headers
      (trait and `for` type), generic bounds, and `let`/lambda annotations inside bodies. The
      item's own type parameters are local names and are not counted.
    - **What is NOT claimed.** (1) A value that meets NO declared type on its way to operator code
      is dynamically typed by the language: a `Dict` value (the type system gives `dict_get` no
      element type) and an unannotated module-level `let` of the candidate. The dispatch edge
      still keeps the candidate's methods from running under an operator method name there; a
      suite that relies on such a value's TYPE pins it with an annotation (`let r: i64 = X`),
      which is cast. **Narrowed by amendment 72 part 2:** a `Dict` KEY the operator held is no longer
      open — sealed code cannot retype it; what stays open is a value at a key the operator
      never held. (2) The candidate chooses its own declared types. If the suite dispatches a
      method on a value whose type is the candidate's declaration, the operator's impl FOR THAT
      TYPE runs (e.g. a lenient `impl Judge for bool` when the candidate declares `-> bool`): that
      is the operator's own rubric, and a suite pins the type by annotation. (3) A closure that
      crosses the host boundary (`host_await_val`) comes back without its contract: the host is
      operator code. (4) Native codegen is unchanged: the PSV guest runs the interpreter.
    - **Rows.** M651 (the dispatch edge), M652 (integer kind), M653 (struct name), M654 (array
      elements), M655 (`Option` payload), M656 (tuple elements), M657 (struct fields at a return),
      M658 (struct-literal fields), M659 (parameters), M660 (the parametricity refusal at a seal
      crossing), M661/M662 (closure result and arguments), M663 (channel sends), M664 (`let`
      annotations), M665 (E0004 over type positions), M666 (trait bounds), M667 (lambda
      annotations). Each attack carries a confused `true` that selects the operator's OWN lenient
      `impl Judge for bool`, so no candidate method is involved and the dispatch edge cannot stand
      in for the cast; each marker is the attack's test COMPLETING. M566 (amendment 46) now anchors
      the whole return cast and stays killed by its own test. Negative matrix A86. Real-runner
      test: `crates/axon-psv/tests/sealed_frames.rs::the_candidate_never_chooses_the_operators_judging_method`
      (the review's two candidates, the declared-`Fake` candidate, and the confused `true`; GOOD
      is a keyed pass, WRONG a keyed failure, every attack a keyed failure). It fails against an
      interpreter built at 1b687d95 (keyed pass for the review's Fake).
    - **Operator deployment.** The guest image must be REBUILT to carry the new interpreter; its
      scripts and runner are unchanged.

54. **The guest runs the policy its launch manifest names, and the receipt binds it (C9 round 4,
    PSV-6 BLOCKER; matrix A87; rows M670-M679).**
    - **Before.** §4 says the policy's sha256 "is bound in the launch manifest (`policy_sha256`)",
      and the observation joins that field. But the policy that ran was the helper request's
      `policy_json`, which Fabric supplied per request and nothing compared with the manifest:
      not the helper (`validate_request` checked only its size), not `fc_linux_profile.sh` (it
      compares the guest-reported policy with the one it embedded, both from the request), not
      `axon-guest-init` or `axon-psv-runner`. `axon-guest-verdict/1` named no policy. So a genuine
      observation of a manifest naming P1 (`allowed_effects: []`) launched P2 (IO, Net, Time),
      and a policy with no `allowed_effects` ran the candidate with no effect ceiling (the runner
      set `AXON_ALLOWED_EFFECTS` only when one was present). Reproduced by the reviewer through the
      real test-trust helper, a test custodian and a genuine observer signature, and re-run here
      as an attack test at `1b687d95` (launched, exit 0). The helper tests' manifest fixture named
      `policy_sha256` `bbbb…`, the digest of nothing, and nothing noticed.
    - **After.** One rule, `axon_psv::protected_policy_ceiling(policy, manifest)`: the policy's
      sha256 IS the manifest's `policy_sha256`, and it states `allowed_effects`. It returns the
      effect ceiling. It is applied at every point the policy crosses:
      - *Root helper* (`axon-protected-launcher`, request schema `/3`). The request carries no
        policy. Fabric writes the policy `psv::prepare` bound to `<inputs>/policy.json`, beside
        the job dir. The helper snapshots it into its root-private staging (a regular file of the
        Fabric uid, never followed, at most 64 KiB), holds it to the snapshot manifest (whose
        digest is the request's and which the observation then joins field for field) BEFORE the
        observation is verified and the nonce spent, and hands the launcher exactly that snapshot.
        A refused policy therefore spends nothing (the tests launch the same observation with the
        right policy afterwards).
      - *Launcher* (`fc_linux_profile.sh`, PSV mode, both routes). `--policy` is required; its
        sha256 must be the job manifest's `policy_sha256` and it must state `allowed_effects`,
        refused with exit 22 before anything is acquired.
      - *Guest runner* (`axon-psv-runner`, the in-guest guard). It reads the ONE `axon.policy=`
        cmdline word (the one `axon-guest-init` enforces), decodes it, and applies the rule after
        the manifest check and before the inputs are read; any failure is a refusal and nothing
        runs. The test then ALWAYS runs under that policy's ceiling (minus `Exec`), taken from the
        verified policy rather than the environment; `[]` denies every effect.
      - *Verdict and joins.* `axon-guest-verdict/2` adds `policy_sha256` (the digest of the policy
        the runner was given, `""` for none). Fabric's `psv::derive` and the loop's `check_bundle`
        require it to equal the manifest's, so the receipt binds the policy that ran.
      - *Fabric.* `backend::run_linux_profile` takes no policy parameter: both the helper route and
        the direct route launch the `Launch`'s policy (the bytes whose digest the manifest names),
        so Fabric cannot hand the launcher a second policy.
    - **Decision: a missing ceiling on the protected profile.** Following CLAUDE.md's rule that "I
      did not say" and "I said none" are different statements: an explicitly empty
      `allowed_effects: []` is a ceiling that denies every effect; an ABSENT one (omitted or
      `null`) is REFUSED on the protected profile by the helper, the launcher and the runner. It
      never falls back to "no ceiling", and the protected profile has no permissive default to
      fall back to. Fabric's own policy (`GuestPolicy::for_grant`) always states one. The generic
      `axon-guest-init` behaviour (warn, run unrestricted on that axis) is unchanged for non-PSV
      program runs, which never yield protected evidence.
    - **Rows.** M670 (helper call), M671/M672 (the rule's digest and ceiling halves, helper route),
      M673 (runner call), M674 (ceiling half, runner route), M675 (the ceiling is always set),
      M676 (Fabric's verdict join), M677 (the loop's verdict join), M678/M679 (the launcher's two
      checks). Matrix A87. `psv_dev make-job` now takes `--policy FILE` and `check-verdict`
      reports `policy_joins`.
    - **Tests.** `privileged_launcher.rs::a_genuine_observation_of_one_policy_never_launches_another`
      and `::a_manifest_policy_naming_no_ceiling_launches_nothing` (real test-trust helper, test
      custodian, genuine observer signature); `launcher_isolation.rs::the_launcher_boots_only_the_policy_the_manifest_names`
      (the real launcher script); `runner.rs::the_guest_runs_only_the_policy_the_manifest_names`,
      `::a_policy_naming_no_ceiling_never_runs_unrestricted`, `::the_runner_reads_the_one_cmdline_policy_word`;
      `psv_dispatch.rs::a_guest_under_a_policy_the_manifest_does_not_name_yields_no_verdict` (both
      routes, the real runner); `intake.rs::a_guest_verdict_that_ran_another_policy_is_refused`.
      `psv_guest_boot_test.sh` adds `policy-launcher`, `policy-guest` (the in-guest guard in a real
      Firecracker guest, through the launcher's embed test hook) and `helper-policy`.
    - **Operator deployment.** Rebuild the guest image (`axon-psv-runner` changed) and re-pin
      `profiles/linux-microvm/manifest.json`; a B263 re-qualification of the image is needed
      before protected use (already PROTECTED_ONLY-open). Install the new helper (request `/3`) and
      launcher together with the Fabric that writes `<inputs>/policy.json`: an older Fabric's `/2`
      request is refused (unknown schema), never launched.

55. **A protected rule is evidenced where it decides, not where it is defined (C9 round 4,
    EQUIVALENCE; rows workstream).** Before: four protected rules were enforced in production
    through calls that had no row, and the whole axon-fabric suite stayed green with each call
    removed (readiness's `trust.check()`, the custodian's `load_config` → `check(true)`,
    `ProtectedHost::operator()`'s four calls, the helper's `Authority::lease()`), as did the
    custodian's socket-activation checks. Eight ACTIVE rows (M490-M492, M629, M640, M546, M634,
    M591) were killed only by a unit test calling the rule function directly; M547/M548/M636/M637
    only by a test calling `helper_agrees` directly. After: every one is attacked through the
    PRODUCTION entry, with `/etc/axon` a tmpfs in a private mount namespace (`unshare -m`; the
    host's `/etc` is never written) and the binaries started as their units start them.
    - **Readiness** (`--test readiness`). The installed `verify-readiness`, deciding with
      `ReadinessTrust::operator()`, run as root (which can write every trust root) certifies
      nothing; the same decision by a uid that cannot write the roots certifies (control). M690
      (the `trust.check()` call); M490-M492 re-anchored. The record's unrowed bindings each get an
      attack on the route where they alone refuse: bundle (M691), component (M692), host profile
      (M693), qualification profile (M694), PSV spec (M695), the `descends` call (M696), a change
      outside governance/ (M697), schema (M698), the `attribution` call (M699).
    - **The production custodian** (`--test privileged_launcher`). `axon-custodian` with no
      arguments, socket-activated as its uid under a config breaking one A83 rule, serves nothing:
      custodian = Fabric (M629), a spender other than 0 (M640), root as the Fabric (M701, a rule
      that had no row), and the call that applies them (M700). Started without its unit's
      `LISTEN_PID`/`LISTEN_FDS` (M702), or activated on another socket (M703), it serves nothing.
    - **`ProtectedHost::operator()`**, run by a production `axon-fabric submit`. The helper config
      is read under the production rules (M706) and must describe the host's launch path (M707,
      the `helper_agrees` call; M547, M548, M636, M637 re-anchored). `fabric_is_not_root` and
      `custodian_is_separate` are DOMINATED on their one production route: a root Fabric is refused
      by `helper_agrees` (M548; a helper admitting root is refused by M536), and a custodian that is
      the Fabric uid by the helper config's own rule (M633). So M704/M546 (vs M548) and M705/M634
      (vs M633) are retired EQUIVALENT_DID with four-cell records on the production route, never
      counted as killed; their unit tests are now controls only (a direct attack assertion there
      would fail the retired guard's full-suite cell while proving only the helper refuses).
    - **Decision D, the lease.** A root helper is always granted a lease on a root-owned file, so the
      production refusal (`Lease::Required`) can only be reached by changing the host
      (`fs.leases-enable`). A TEST-TRUST-ONLY switch (`/etc/axon/TEST-no-read-lease`, in
      `sealed_exec::take_read_lease`) makes every lease unavailable; the test-trust helper in
      production mode (`Authority::production()`, setuid-root) then launches nothing (M708, the
      selection; M591 re-anchored). M709 rows the switch's gate: the production-build helper, with
      the switch present, launches, and its binary does not contain the switch's path.
    - **Not done here.** `scripts/v022_refusal_coverage.py` still scans the three helper files only;
      extended to custodian.rs, bin/axon-custodian.rs, readiness.rs and protected_host.rs it names
      38 refusal sites with neither a row nor an exemption (measured at this commit).
    - Operator deployment: none. No matrix row: no production behaviour changed (the lease switch
      is absent from the production build, M709).
    - Evidence at 06ad62bc: `v022_g01_mutations.py --only=` the 28 new or re-anchored ACTIVE rows,
      28/28 KILLED by their own attack; `v022_paired_disable.py --only=M704,M546,M705,M634`, all
      four cells and the full-suite cell hold for each.

56. **A check runs the binary built from the tree under test; the harnesses judge only a clean
    commit and say which registry they ran; the guest image is built in an environment the build
    constructs (C9 round 4, harness2: EQUIVALENCE (6) blocker, EQUIVALENCE (5) and FIELD-ORIGIN /
    PSV-2 majors, currency and skip-accounting minors).** Nothing here relaxes a counting rule.
    - **Before.** Amendment 52 removed `CARGO_TARGET_DIR` at `cli_run.rs`'s one spawn, but the
      scripts still chose their binary by guessing: `clock_parity.sh` built nothing and fell back
      to `./target/debug/axon` (a planted one was run and reported on); the R17 IR/QEMU gates,
      spawned by `integration_fixtures.rs` with a bare `bash`, took `target/{debug,release}/axon`
      or `command -v axon` (a planted `axon` on PATH ran and the checks PASSED); the drift test
      counted literal spawns in one file; and a paired-disable cell left binaries built from the
      MUTATED tree in the workspace `target/` for later cells.
    - **After: one primitive per side.** A script picks a binary only through
      `scripts/lib/axon_bin.sh`: `named_bin VAR` (a harness that builds nothing runs only what its
      caller names, and REFUSES with none named, exit 2, a failure and never a skip), or
      `use_built VAR bin` after its own `cargo build` (the path cargo built to, from
      `cargo metadata`'s `target_directory`, never a guessed `target/`), or a target dir it
      assigned itself. All 90 scripts (and the example demos) that hard-coded a path were moved
      to it; `clock_parity`, `chan_parity`, `utf8_boundary_parity` and `unsigned_parity` now build
      their own codegen axon like their peers; `replay_host_gate`, `diagnostic_location_gate`,
      `gate_verdict_is_read`, `wasi_env_control_gate`, `reference_gate` and the R17/eBPF/Zephyr
      gates require a named binary, and `gate.sh` / CI name the one they built. A test runs a
      script only through `crates/axon-core/tests/script_spawn` (`Bins::Named` / `BuildsItsOwn` /
      `NoWorkspaceBinary`), which strips every binary-naming variable from the inherited
      environment and refuses to run a script whose text guesses. `harness_binaries.rs` fails the
      build if any test in ANY crate spawns a script another way (bash, sh, python3, env, direct
      exec, a script path handed to `.arg`), if any script under `scripts/`, `examples/`,
      `profiles/` or the root picks a binary another way, or if a script accepts a binary through
      a variable the helper does not strip. The same rule covers Rust tests that EXEC a workspace
      binary their own cargo run does not build (the `axon` interpreter from axon-psv, axon-fabric,
      axon-cortex, axon-os and axon-intent tests; `cortex`; `axon-os`): integration measured the
      PSV-1 attack test PASSING THE ATTACK on a `debug/axon` that predated the fix, because
      `cargo test -p axon-psv` never rebuilds axon-core. They now go through
      `script_spawn::workspace_bin`, which BUILDS the binary with cargo into
      `<target>/workspace-bins` (cargo rebuilds whatever changed), or, when a harness names it
      (`AXON_BIN`), refuses it unless it is at least as new as every source file of its package,
      its workspace path dependencies and `Cargo.lock`; the drift gate refuses a test that reads a
      binary variable or joins a `<profile>/axon*`/`cortex*` path itself. The axon-os/axon-intent
      suites that used to SKIP without a built interpreter now build it and run.
      `axon-vm`'s live boot takes only `AXON_GUEST_KERNEL`
      (it fell back to a kernel no test built). Skips stay skips: the codegen probes are unchanged.
    - **Harness integrity.** Both harnesses refuse ANY uncommitted change (`git status
      --porcelain --untracked-files=all`, the whole tree: 24 rows guard files in `scripts/` and
      `profiles/`, and the registry and markers are in `scripts/`). Every run, shard and record
      carries the registry/marker git blobs, each row's old/new sha256 (a record: its edits and
      marker digest), the uid, `/etc/axon` presence and the unset variables; `--merge` and
      `--join` refuse a shard from another registry or a dirty tree, or a row/record executed with
      edits that are not this registry's. Cells run with `AXON`/`AXON_BIN`/`CORTEX_BIN` unset
      (consumers are handed this run's interpreter). After every cell that ran a mutated crate and
      could have run scripts, every executable in the workspace target dir is removed and the
      prerequisites rebuilt. A full-suite cell runs with `--show-output` and records the tests
      that printed `skipped:` (root-only or host-dependent) as skips, per package.
    - **Currency.** A kept paired-disable record is stale when ANY file of its owner package or
      of any consumer package changed since its commit (`git diff --name-only`), or when its
      edits/marker digest is not this registry's (a record without one is stale). This commit
      changes tests in `axon-core` and `axon-fabric`, so every kept record is stale and the
      paired-disable status must be re-executed (`--reexecute-stale` or a fresh sharded run).
    - **Guest image build environment.** `scripts/guest_build_env.py` replaces the list of four
      wrapper variables and four config files. Every cargo run of `build-guest-image.sh` goes
      through it: the caller's environment is dropped (cargo sees exactly `CARGO_HOME`,
      `CARGO_TARGET_DIR`, `HOME`, `LC_ALL`, `PATH`, `RUSTC`, plus fetch proxies), the toolchain is
      the one `rust-toolchain.toml` pins, resolved by rustup under a cleared environment (so
      `RUSTUP_TOOLCHAIN`, `RUSTUP_HOME`, PATH choose nothing), `RUSTC` is that toolchain's rustc,
      `RUSTFLAGS` only what the step passes explicitly, and `CARGO_HOME` and the target dir are
      fresh directories it creates. Before anything builds it refuses if cargo's EFFECTIVE
      configuration from the build directory (`cargo -Zunstable-options config get
      --show-origin`, every ancestor config) holds any key that could reach the build, from any
      origin (only `target.<triple>` settings for triples the build never compiles, i.e. the
      tree's wasm rustflags, pass). Its record (`axon-guest-build-env/1`: `rustc -vV`, `cargo -V`,
      both binaries' sha256, the exact environment, the effective config, each built artifact's
      sha256) is embedded in the guest manifest as `source.build_environment`, and
      `source.rustc` is now the pinned rustc's identity rather than PATH's. The freeze refuses a
      manifest with no such record, one whose environment is not exactly the constructed one,
      whose target dir or `CARGO_HOME` was not fresh, whose effective config held a build
      setting, or whose `axon` / `axon-guest-init` / `axon-psv-runner` digests are not the ones the
      controlled build produced, and binds the record's digest and `rustc -vV`.
      `linux_profile_manifest.py` compiles its provenance helper with the pinned rustc, never
      `$RUSTC`, and writes no bytecode cache into the tree it describes. M650 stays on the
      freeze's own-environment refusal, renamed to say that is all it checks.
    - **Rows** (all PSV scope, `PSV_IDS` extended by M720-M739): M720-M725 (axon_bin.sh
      no-fallback and cargo-resolved path; the spawn helper's variable stripping and script
      refusal; the two drift scanners), M726-M729 (whole-tree dirty refusals; merge/join registry
      blobs), M730-M733 (effective-config key rule; constructed environment; toolchain under a
      cleared environment; pinned rustc for the provenance helper), M734-M738 (the freeze's
      build-environment record, environment equality, fresh dirs, effective config, artifact
      binding), M739 (the package-wide currency rule).
    - **Tests.** `crates/axon-core/tests/harness_binaries.rs` (planted stale `target/` binary and
      PATH binary against the real `replay_host_gate.sh`; a config-moved target dir against the
      real `clock_parity.sh`; ambient variables; the helper's refusal; both drift gates),
      `crates/axon-core/tests/harness_integrity.rs` (the real harnesses in scratch repositories:
      dirty `profiles/`, edited marker registry, untracked `scripts/` file; merge and join from
      another registry; a record going stale on an owner-package file no list named),
      `crates/axon-fabric/tests/guest_build_env.rs` (through `build-guest-image.sh
      --build-env-only`: an ancestor config, the dotted key, a guest-target linker, rustflags, a
      `build.rustc` in the tree's own config; a caller's `RUSTC_WRAPPER`, `RUSTC`, `RUSTFLAGS`
      through a real controlled build; a planted `RUSTUP_HOME` toolchain), `freeze_manifest.rs`
      (seven uncontrolled builds and three unbound artifacts), `guest_provenance.rs` (a caller's
      `RUSTC` that compiles a helper reporting a dirty tree clean), and
      `scripts/test_v022_paired_disable_join.py` (four more refusals).
    - **Operator deployment.** A guest image must be rebuilt with this `build-guest-image.sh` (it
      needs network for the fresh `CARGO_HOME`, and rebuilds everything in a fresh target dir);
      an image whose manifest lacks `source.build_environment` no longer freezes. No change to
      `/etc/axon`.
    - **Also corrected.** `psv_dispatch.rs` named M402 for the protected arm (it is M400);
      `EQUIV_RECORD["M347"].all_paths` now states the allowlist's role under decision C.
    - **Second wave (C9 round 4, harness2 continued; rows M860-M873, `PSV_IDS` extended by
      M860-M879).** Rows for the guards the first wave left unrowed, each killed by its own
      attack through the production route: `workspace_bin`'s stale-named-binary refusal (M860);
      the test-side resolution drift rule (M861, attack fixtures in
      `a_test_that_resolves_its_own_binary_is_flagged`); `--merge` / `--join` refusing a
      dirty-tree shard and a row or record run with edits that are not this registry's
      (M862-M865, the real harnesses in scratch clones); the scrub of workspace binaries after a
      mutated cell, in both harnesses (M866, M868), the removal of the caller's
      `AXON`/`AXON_BIN`/`CORTEX_BIN` from every cell (M867), and full-suite skip accounting
      (M869). M866-M869 run the REAL harnesses on a miniature committed workspace whose tests
      plant an executable in the workspace target dir, log the `AXON_BIN` they were given, and
      skip as a root-only test does.
    - **The guest's serial record was cut by its own reboot (defect, reproduced).** One boot of
      the pass case ended `PSV-VERDICT-INIT[    0.256427] reboot: Restarting system`; looped
      through the boot test's exact path it reproduced 1 in 40. Mechanism: guest-init's
      `echo` writes go into the tty and the 8250's buffer and leave as the UART drains them;
      `reboot -f` does not wait, so everything after the first 16 bytes (one UART FIFO) of the
      final burst was lost -- the verdict digest, `B263-OUT` and `B263-DONE` -- and the kernel's
      own reboot message, which printk writes synchronously, landed on the same line. The host
      refused that run (`B263-DONE` absent, rc 21), so no wrong verdict resulted; but the host
      read every serial field with substring patterns (`.*sha256=<64 hex>.*`,
      `grep B263-DONE`, sed on an unterminated last line), so a record spliced into another
      line or cut before its newline was read as the record. Fixed at both ends: every reboot
      of the guest goes through `halt_guest`, which runs `sync`, then `stty -F /dev/console`
      (busybox applies it with TCSETSW, which returns only after all console output has left
      the UART), then `reboot -f` (M872, and M873 for a reboot that bypasses it); and the
      launcher reads every serial field -- on the launch path and under `--verify-result` --
      through one primitive, `serial_record`, which takes a line only when it matches in full
      and a newline ends it (M870 unterminated, M871 spliced). Tests:
      `b263_profile_wiring::every_guest_reboot_first_drains_the_serial_console` (the real
      functions with logging stand-ins) and `axon-fabric/tests/serial_records.rs` (the real
      launcher's `--verify-result`: a verdict spliced with the kernel's reboot message, an
      unterminated `B263-OUT`, a policy record inside another line; control: whole records
      bind). Evidence on a real guest: with the fixed image the same loop gave 0 truncated in
      120 boots and 0 in a further 200 (0 of 320; 1 of 40 before), and `psv_guest_boot_test.sh` 17/17 PASS
      through the new parser. Image digests tested (not re-pinned here; the integrator builds
      the final image from a standalone clone): rootfs.sqfs
      455d2ddd61cd4b2eaf040d2a93b58ca453ee922a7a67551576d963b55bf83909, vmlinux
      4ec3ba40a4024467d959fd3812af000ad9f7f9900579cc9f0fd01a465dcd22b7. Operator deployment:
      rebuild the guest image (guest-init.sh changed).

57. **Readiness joins the certification record's run attribution to the run itself, and the B263
    record to the qualification the observed launch ran under (C9 round 4, readiness workstream;
    rows M740-M750).**
    - **Before.** `attribution()` joined `observer_key_id` to the observation's signer, but
      `verifier_key_id` was checked only for membership in the verifier root, `suite` only for
      non-empty fields, and `candidate_tree_ref` and `micode_sha` only for format. The observation's
      `suite_registry_sha256`, `verifier_sha256`, `host_config_sha256` and
      `intended_launch_manifest_sha256` were compared with nothing, because the launch manifest was
      not in the evidence. So `b263_qualification_sha256` was never joined to the manifest's
      `qualification_sha256`, and `accept_b263` judged currency only at decision time: any
      operator-signed, currently fresh B263 record with the same guest and engine certified a run
      launched under a different record, for example one issued after the run or for another
      host. The operator's signature was the only thing behind these fields (class c).
    - **After: the run is certified evidence.** The record's `evidence` must hold exactly one
      `axon-fabric-submit/1` (Fabric's own submit output: the receipt, its
      `acf-receipt-attestation/2`, and the `axon-psv-evidence/2` bundle with the exact launch
      manifest and guest verdict) and exactly one `acf-compute-request/1` (the request it
      answered). None, or two, is refused. Readiness (`launched`, called at the end of
      `attribution` on the production decision) then requires:
      - the attestation verifies (`attestation::verify`) over that request and receipt under the
        verifier-root key whose id is `verifier_key_id`, so the key id is the key that signed;
      - the receipt is protected evidence (`protected_evidence::check`) and names the bundle's
        manifest, the certified observation (`observation_sha256`), the bundle's guest verdict and
        the manifest's qualification, and the manifest's operation, task, trial, attempt, candidate
        and test are the request's and receipt's;
      - the certified observation joins the manifest field for field
        (`PreflightObservation::joins`: intended manifest digest, nonce, verifier, host config,
        suite registry, policy, launcher, engine, guest);
      - the record's `b263_qualification_sha256` is the manifest's `qualification_sha256`, the
        digest Fabric computed from the record its operator host config pins;
      - the record's `suite` (id, version, entry, test, and `digest` = the suite tree digest) and
        `candidate_tree_ref` are the manifest's.
    - **Currency at `observed_at`.** `accept_b263` is also applied with `observed_at` as the
      time, so the record's `end` must be no later than the observation and within the maximum age
      of it. The decision-time check stays. Of the two, only "end after the run" can differ in
      practice: a record stale at `observed_at` is stale now too.
    - **Not joined, and why.** `micode_sha` stays operator-attested: no document a protected run
      produces carries a MiCode revision. The B263 `host` string has no observed counterpart
      either: no observation or manifest names a host. The host is joined through the digest
      instead. The observed host's operator config pins the record path, Fabric computes
      `qualification_sha256` from it, and the observation covers the manifest. A record for
      another host is therefore another digest and is refused. Pinning a host identity in the
      host config remains the separate MINOR finding.
    - **Operator deployment.** When assembling a certification record on the protected host, the
      operator adds the run's `axon-fabric submit` stdout (with `receipt_attestation` and
      `psv_evidence` present) and the request file to `evidence`, beside the observation and the
      B263 record. No new key or root is needed.
    - Negative-matrix A88 (run attribution) and A89 (B263 joined to the launch), tests in
      `crates/axon-fabric/tests/readiness_launch.rs`. The readiness fixture now builds a genuine
      run: a manifest, an observer-signed observation of it, and a receipt attested by the
      verifier-root key. Its B263 attacks relaunch under the attacked record, so each one still
      reaches the rule it was written for.

58. **The refusal-site gate covers the custodian, readiness, the protected host, the observer and
    the guest protocol (C9 round 4 fix wave, EQUIVALENCE (4); rows2 workstream, rows
    M760-M774).**
    - **Before.** `scripts/v022_refusal_coverage.py` scanned the three helper files only
      (amendment 48). Amendment 55 measured 38 unrowed, unexempted refusal sites in the next four
      files. At 866474ea the count was 41 in those four and 44 with `observer.rs` and axon-psv
      `lib.rs`. The gate already failed on the helper at that base: the policy change
      (amendment 54) moved the policy size bound into `snapshot_policy` (a stale exemption) and
      added an owner check with no row.
    - **After.** The gate scans `custodian.rs`, `bin/axon-custodian.rs`, `readiness.rs`,
      `protected_host.rs`, `observer.rs` and `crates/axon-psv/src/lib.rs` as well. A `use` line
      that names `TEST_TRUST_BUILD` is no longer a site, because it reads nothing. Count per file
      (uncovered before, then rows and exemptions after): custodian 10, then 13 rowed and 7
      exempt; axon-custodian 1, then 1 rowed; readiness 24, then 30 rowed and 17 exempt;
      protected_host 6, then 17 rowed and 2 exempt; observer 3, then 6 rowed and 3 exempt;
      axon-psv lib 6, then 18 rowed and 3 exempt.
    - **New ACTIVE rows, each attacked through the production route where it alone refuses:**
      - The production `axon-custodian`, socket-activated under `/etc/axon` (a tmpfs in a
        private mount namespace), refuses a config of another schema (M760) or with
        `max_age_s` 0 (M761).
      - The helper reads a custodian's refused spend as a refusal (M762, `CustodianRef::call`'s
        `!r.ok`, attacked by the one-observation replay).
      - The production custodian takes no `--test-config` (M763).
      - A production `axon-fabric submit` refuses a host config of another schema (M764), one
        naming the helper's test-trust `test_config` (M765), and a host config it cannot stat
        (M766: EACCES as the Fabric uid). It refuses these rather than running as a
        development host.
      - A production readiness verifier built from a dirty tree certifies nothing (M767). Clean
        and dirty production builds are made from one copy of these sources, and the clean one
        certifies (the control).
      - A narrowing list the verifier cannot stat is not read as absent (M768, EACCES as uid 4242
        on the production decision).
      - The repository may narrow the issuers but never add one (M769).
      - A test-trust verifier names itself so, both in its identity (M770, which
        `protected_verifier_ready.py` requires to be `production`) and in its report (M771).
      - The helper reads `policy.json` only as a regular file of the Fabric uid (M772).
      - The observation joins the manifest pair by pair (M773) and on `guest.init_sha256`
        (M774, a new helper-route attack whose only difference is the init digest).
    - **Exemptions.** Each is in the script with a reason a reviewer can check. The categories
      are:
      - an OS or tool error that fails closed;
      - an operator-authored or operator-signed field;
      - a site whose condition a named row already mutates at another line (M490/M491, M338/M418,
        M749, M350-M352);
      - an arm with no value to admit with;
      - a check that only re-reports what the next statement refuses on the same input
        (`found != top` before `refuse_config`; git failures before the hash-checked tree
        comparison, M290/M697).
      None of these is an EQUIVALENT_DID retirement, and none is counted as killed.
    - **Not scanned yet** (listed by the gate on every run, with counts measured at 866474ea):
      `psv.rs` 2 (the tree re-reads need a workspace version whose materialisation does not read
      back as itself), axon-psv `runner.rs` 4 (guest), `protected_evidence.rs` 9,
      axon-loop `admission.rs` 18 and `intake.rs` 27. Scanning the loop files as they are would
      also miss their tail-expression refusals (`Err(refused(…))` without `return`), which the
      site pattern does not match. That pattern would have to be extended first.
    - **Base repairs in tests only.** The readiness fixture now fills `GuestVerdict.policy_sha256`;
      five readiness test binaries did not compile at the base. The production readiness test
      (M490-M492, M690) re-launches the run under its re-dated B263 record. Its control was
      PARTIAL at the base ("the evidence bundle changed").
    - **Found, not fixed here (readiness workstream's area).** M338 (the verifier_key_id
      membership, ACTIVE) is REFUSED_ELSEWHERE at 866474ea. Since amendment 57, `launched()` also
      looks `verifier_key_id` up in the verifier root and verifies the receipt attestation under
      it (M740), so its test's attack is refused there too. M338 needs a four-cell retirement
      against M740, or an attack that reaches it alone.
    - Evidence at c2c80ebd: `v022_g01_mutations.py --scope=all --only=M760-M774` plus the cited
      M325, M490, M491, M703 and M749, all KILLED by their own attack; M338 REFUSED_ELSEWHERE, as
      above.
    - No production behaviour changed: no matrix row, no operator deployment.
    - **Wave 2 (integrator decisions on the merged rows2; rows M775-M857).**
      - **Every protected decision file is scanned; `NOT_YET_SCANNED` is empty.** The site
        pattern now also counts a TAIL refusal: an `Err(…)` built as a value without `return`
        (an `Err(…)` that only *matches*, such as `Err(e) =>`, `if let Err(e) =`, or `Ok(_) | Err(_)`,
        is not counted). It also counts each call of a refusal constructor: `refused(`,
        `fail(`, `shape(`, the runner's `refused(`, and psv.rs's `unknown(`. Their definitions
        are not sites. `psv.rs`, axon-psv `runner.rs`, `protected_evidence.rs`, and axon-loop
        `admission.rs` and `intake.rs` are scanned. A retired STALE row's text is absent by
        definition, so the gate no longer reports it. Per file (sites: rowed / exempt):
        - privileged_launcher 49: 30 / 19;
        - sealed_exec 17: 11 / 6;
        - axon-protected-launcher 3: 2 / 1;
        - custodian 21: 15 / 6;
        - axon-custodian 1: 1 / 0;
        - readiness 44: 32 / 12;
        - protected_host 19: 19 / 0;
        - observer 9: 7 / 2;
        - axon-psv lib 23: 19 / 4;
        - psv.rs 20: 14 / 6;
        - runner 14: 5 / 9;
        - protected_evidence 30: 25 / 5;
        - admission 48: 34 / 14;
        - intake 63: 43 / 20.

        "Rowed" includes the EQUIVALENT_DID retirements, each with a four-cell record and none
        counted as killed.
      - **New ACTIVE rows on the newly scanned sites** (rows workstream):
        - M775: a check that produced no verdict is never receipted, through Fabric's submit.
        - M776, M777, M778: a protected receipt states exactly one evidence class and every
          required digest ref. Each is attacked through readiness with a re-attested receipt.
        - M779: the receipt counts the outcome the guest verdict claims, attacked through the
          loop's `intake_episode`.
        - M780: the runner runs only a suite entry inside the suite tree.
        - M781: a run over its output limit yields no verdict.
      - **The strict reading of "dominated."** Every exemption that only repeated another check
        is now an ACTIVE row or an EQUIVALENT_DID retirement:
        - ACTIVE rows:
          - M795: helper request schema.
          - M797: custodian spend manifest digest.
          - M798: timeout bound.
          - M799: policy size.
          - M800: exact input spelling.
          - M802: the out dir must be NEW.
          - M805: root-owned candidate dir.
          - M806: custodian request schema.
          - M807 and M808: `protected-host-paths` refuses what `load` refuses.
          - M810: hash-checked change set.
          - M811: observer exit status.
          - M812: `launched()`'s `verifier_key_id` lookup.
          - M814: the top of a standalone clone.
        - Retired, four-cell, never counted: M796 (against M623 and M797), M801 (against M800),
          M803 and M804 (against M802), M809 (against M810), M813 (against M325), and M338
          (against M812). This resolves the open M338 item above.
        - Source changes, both strengthening:
          - readiness's `git()` primitive refuses a failed git once, for every question it
            answers; six per-call checks are replaced.
          - `refuse_config` is applied to the repository git discovers.
      - **Loop (admission.rs, intake.rs).**
        - ACTIVE rows M815-M829, M831, M832, M834-M851 and M857, each attacked through
          `admission::admit`, `pointer::transition` or `intake::intake_episode`.
        - Retired, four-cell: M830 (against M857), M852 (against M122 and M104), M853 with M854,
          and M855 with M856.
      - **Exemption kinds left.** Each names the fact a reviewer can check:
        - NOTHING TO ADMIT: the arm holds an error and no value to continue with.
        - OS/TOOL ERROR: the operation does not happen.
        - OPERATOR-AUTHORED/SIGNED field.
        - NAMED ROW: a registry row mutates this refusal's condition at another line.
        - NON-LINUX cfg.
        - UNREACHABLE BY CONSTRUCTION. Three sites are left for the integrator's ruling:
          - psv.rs's candidate and suite tree re-reads (219, 225). The dir is created new and
            0700 and written from a store tree that re-derives its ref; nothing of the Fabric
            uid runs between the write and the re-read.
          - intake.rs's `cl22` policy-scheme check (290). Every policy record the store can name
            is filed under its own `cl22` digest (`cas_path`, `check_name`), so the check is
            reached by no input even with those disabled. No four-cell record can be built.
      - **Heuristic overlap.** Three loop sites (admission 543 and 825, intake 513) are covered
        only because the scanner's guard-block heuristic overlaps a neighbouring row (M819, M826,
        M838). Each is in substance a NOTHING-TO-ADMIT `ok_or`.
      - No production behaviour of a guard changed: no matrix row and no operator deployment.
59. **Every row's attack reaches its own guard after amendments 54 and 57; a cell's build is judged by
    its own cargo invocation; every cell keeps its output; every row ends on the run's interpreter
    (C9 round 4 fix wave, rows3 workstream, rows M880-M894; M868 re-anchored).**
    - **Before.** The full mutation run at 49eb3765 found ACTIVE rows that their own attack did
      not kill:
      - **M410, M474, M612** (`one_read.rs`, `psv::prepare`). Since amendment 54, `prepare`
        writes the guest policy exclusively BESIDE the job dir. These tests called `prepare`
        twice with job dirs in ONE parent. With the guard removed, the attack died on
        `guest policy: File exists (os error 17)` before it reached the guard (class e). The
        sweep found no other caller: the submit route's `private_inputs` creates a NEW
        `<op>.psv-inputs` dir per operation, and the policy is the only exclusive producer
        added in round 4.
      - **M341-M345, M349** (`readiness_attribution.rs`). Each attack edited one document
        (the observation, the record's digest, or the B263 record) and left the run documents
        alone, so amendment 57's run joins refused it first. These are the attested receipt's
        `preflight-observation-sha256`, and the launch's `qualification_sha256` against the
        record.
      - **M278 / M58 (harness).** M278's baseline read `compile_error` in a sharded run, and
        M58's consumer baseline read `CONSUMER_BASELINE_BROKEN`. In each case the per-row record
        kept only the label. The rule was "`could not compile` or `error[E` anywhere in the
        output", and `full_suite_ok` returned on it BEFORE its keep-the-output branch.
      - **Interpreter (harness).** Shard 1 ended with `BAD interpreter binary changed during the
        run`. Its `target/debug/axon` reported `49eb3765-dirty`.
      - **Cost (harness).** The 2-shard paired-disable run at 49eb3765 spent about 45 min per
        record. `after_cell` scrubbed the workspace target dir and rebuilt every prerequisite
        (axon, the cortex bins, psv_dev) after EVERY cell with an edit under crates/: three cells
        per record, and cold under `CARGO_INCREMENTAL=0`.
    - **After (tests only for the rows; no production source changed).**
      - Each `prepare` gets its own inputs dir, laid out as on the submit route
        (`<job>.psv-inputs/job`). The control asserts that the policy lands beside its own job
        dir.
      - Each readiness attack is relaunched (`relaunch`, `rebundle`), so every amendment-57 join
        holds and the attack differs only in the guarded property:
        - M341: the run launched (and was honestly observed) with another kernel, or by
          another fabric revision, than the record certifies.
        - M349: the receipt and the record agree on an observation digest that names no
          certified file.
        - M342: the launch ran under a qualification digest that no certified file has. The
          genuine B263 record is listed last, so a lookup that settles for some file finds a
          valid one.
        - M343, M344, M345: the launch ran under the agent-signed record, the
          other-guest record, or the other-profile record.
      - All six take the first route, an honest attack that reaches the row's own guard. None
        needed an EQUIVALENT_DID retirement.
    - **Harness.**
      - **Build first.** `cargo test --no-run` is run alone first. A compile error is THAT
        invocation's failure; a killed build is `build_failed`. Compiler text in a test's
        output is then judged as the test's own result.
      - **Cause of M58, reproduced.** Run alone at 49eb3765 with the shard env (sccache,
        `CARGO_INCREMENTAL=0`, alongside the integrator's four shards), the axon-cortex consumer
        suite exits 0. Under `--show-output` (added in round 4 to count skips), its
        `compile_fail` doctests print their expected `error[E0559]`/`error[E0599]`, and the old
        rule read that as COMPILE_ERROR (`/var/tmp/c9r4-rows3-repro-pd/axon-cortex.log`). This is
        deterministic, not environmental: every axon-core-owned retirement has axon-cortex among
        its consumers, so each would read CONSUMER_BASELINE_BROKEN.
      - **M278, not reproduced.** It passed alone and under sccache under concurrent load. It
        also passed through the UNCHANGED harness on the shard's own sequence (M274, M276,
        M278, M280, M282 under the shard env), with every output saved. No OOM kill was logged.
        A recurrence is now classified by the build invocation and keeps its output.
      - **Kept output.** Every non-passing baseline, mutated cell, paired-disable cell, build
        and full-suite or consumer cell writes its whole output under `/var/tmp/v022-cells`. The
        harness prints the path, and the mutation record names it (`baseline_output`,
        `cell_output`).
      - **Cause of the interpreter change, reproduced.** axon-core's `build.rs` embeds `-dirty`
        and is re-run only when `.git/HEAD`, `.git/index` or `src/` change. Its own `git status`
        refreshes `.git/index`, so the NEXT build re-runs it. In shard 1 that was M860's mutated
        cell, after the `--lib` rows M651-M667 restored `src/`. M860's edit is in
        `tests/script_spawn`, so `-dirty` was baked in. The restoring `cargo build` keeps it,
        because nothing the build script watches changed again.

        Measured in a 49eb3765 clone with the shard env:
        1. Clean build: `5d9aff1e…`, `(49eb3765)`.
        2. After an edit outside `src/`: `a460653e…`, `(49eb3765-dirty)`.
        3. Restored and rebuilt: still `a460653e…`, `-dirty`.
        4. After touching `build.rs`: `5d9aff1e…` again, byte-identical.

        Every row after M860 in shard 1 ran an interpreter with the clean tree's code and a
        `-dirty` identity. Only `axon --version` reads that identity, and no row's verdict
        depends on it.
      - **The interpreter fix.** After EVERY row, `restore_interpreter` rebuilds the
        interpreter from the restored tree and byte-compares it to the run's. If it differs, it
        re-runs the build script (touching `build.rs`, which changes no content) and compares
        again. A row whose interpreter cannot be restored is BAD by name
        (`interpreter_not_restored`), and the run stops there. The start builds from the clean
        tree, and refuses an interpreter that names itself dirty. Paired-disable restores and
        compares after every cell, and a failure there is the record's. The mutation harness's
        separate post-row interpreter rebuild is removed; the per-row restore does it.
      - **The cost fix (paired-disable).** `after_cell` keeps the guarantee and pays only for
        what can be stale:
        - It scrubs only when the cell ran a script that builds: the row's test target calls
          the spawn helper (`spawns_scripts`), or, for a full-suite cell, any test of any suite
          the cell ran does (`package_spawns_scripts`). M724's workspace drift test makes the
          helper's call the whole fact.
        - It rebuilds a prerequisite only when the file is missing, or its sha256 is not the
          clean build's. The digests are recorded after `build_prereqs`, and the files are
          named from cargo's metadata.
        - The interpreter is always rebuilt and byte-compared. When nothing changed, that is a
          no-op build.
        - A prerequisite that cannot be restored is the record's failure.
      - **What the first evaluated M58 full-suite cell showed.** With the consumer baselines no
        longer misread, paired-disable `--only=M58` reached M58's full-suite cell for the first
        time. Its kept outputs named two defects in test code, both fixed:
        - At 9a823637, this workstream's own harness test executed
          `<tgt>/debug/axon`, which the workspace binary-resolution lint
          (`harness_binaries`) refuses. Fixed at 00c1509d: the test reads the identity from
          the binary's bytes.
        - At 00c1509d, every consumer that ran after axon-core's own suite in one cell refused
          `AXON_BIN` with "older than `crates/axon-core/out/copy_dst.txt`". The
          `sandbox_scope_copy` fixture of that suite writes the file, and `out/` is
          git-ignored. `script_spawn::stale_against_sources` counted every file under the
          crate, outputs included. It now counts the files git calls the working tree (tracked,
          or untracked and not ignored), and falls back to the whole directory without git.
          Attack: `an_ignored_output_written_into_a_crate_is_not_a_source` (M894). The control
          is M860's test: a newer source still refuses.
    - **Rows** (each killed by its own attack, through the REAL harness on a miniature
      workspace, `crates/axon-core/tests/harness_integrity.rs`):
      - M880: a test printing compiler text is not a mutation cell's compile error.
      - M881, M882: the same for paired-disable's own cell and its full-suite/consumer cell.
      - M883, M884: a mutation baseline / mutated cell that did not pass keeps its output.
      - M885, M886: a paired-disable cell / full-suite cell that did not build keeps the
        build's output.
      - M887: a paired-disable cell whose test failed keeps its output.
      - M888: every row ends on the run's interpreter.
      - M889: restoring re-runs the build script.
      - M890: the restored interpreter is compared, never assumed.
      - M891: a cell's changed prerequisite is rebuilt and byte-compared. In the attack, every
        cell replaces the run's `cortex`.
      - M892, M893: a cell that ran a building script is scrubbed after. M893 is the full-suite
        case: the miniature's row test runs no scripts, and another test of its package does.
      - M894: a git-ignored output a test wrote into a crate is not a source that a named
        binary is stale against.
      - M868 is re-anchored on the scrub in its new place, with the same attack (`_PC`). The
        miniature's tests now plant a script-built binary only where the file runs scripts
        (`RUNS_SCRIPTS`).

      The M888 attack reproduces shard 1 in the miniature: a real `build.rs`, and an index
      refresh before the mutated cell. It fails on the unchanged harness with a final
      `-dirty` interpreter.
    - **Reasoned exemptions (evidence plumbing, no verdict decided):**
      - paired-disable's failing full-suite keep (`pd-suite-`). It existed before; only its
        writer moved into `keep_output`.
      - paired-disable's per-cell restore call and both harnesses' clean start. No current
        retirement edits an axon-core file outside `src/`, so no record's cell can bake `-dirty`
        in. An attack seeded into the target dir self-heals at the start, because the harness's
        own `git status` refreshes the index (measured). Both call the primitive that M888-M890
        kill.
    - **Evidence.**
      - At 8862d1b5: `v022_g01_mutations.py --scope=all --only=<43 rows>`. These are every row
        whose test is in `one_read.rs` or `readiness_attribution.rs`, or that guards `psv.rs`.
        All 43 were KILLED by their own attack, including M341-M345, M349, M410, M474 and M612.
      - At 9a823637: `--only=M880-M890`, all KILLED.
      - At 00c1509d: `--only=M866-M869,M880-M893`, all 18 KILLED.
      - At 9940675a: `--only=M722-M725,M860,M861,M894`, all KILLED.
      - At b4a298a6, through the new harness: the 43 rows above plus M278, 44/44 KILLED. M278's
        baseline passed, and no row's interpreter needed restoring.
      - At 9940675a: `v022_paired_disable.py --only=M58` HOLDS, for the first time with its
        full-suite cell evaluated. All four cells hold, the retired guard's full suite is
        SUITE_OK, every consumer (axon-cortex, -fabric, -intent, -os, -psv, -wasm, -web) is
        SUITE_OK, and no interpreter fault was recorded (record at
        `/var/tmp/c9r4-rows3-pd-M58-final2.json`). The run exits 1 only because a `--only` run
        covers one record. Its earlier records read BASELINE_BROKEN, then (at 49eb3765)
        CONSUMER_BASELINE_BROKEN, so the full-suite cell had never been evaluated.
      - Refusal coverage, the matrix check and the paired-disable join test PASS.
    - No production behaviour changed: no matrix row, no operator deployment.

60. **The cast's notion of "the same type" is the key a method call dispatches on; a call through a
    local goes through its value; every refusal arm of the cast has its own row (C9 round 4b,
    core2 workstream, PSV-1 and EQUIVALENCE; matrix A86).**
    - **Before.** Review round 4b (wf_72ccc216-e8d, logs `/var/tmp/c9r4-psv1b/`) executed two new
      members of the A86 class to a keyed PASS for a wrong candidate through
      `axon_psv::runner::run`. (a) INTEGER WIDTHS: `cast_named` accepted any `Int` or `SizedInt`
      at every integer name, without converting, while a method call dispatches on
      `Value::type_name`, which for a `SizedInt` is its width. A `u8` laundered through `dict_get`
      into a fn declared `-> i64`, and kept by the suite's own `let r: i64` (the pin amendment 53's
      non-claim (2) recommended), ran the operator's lenient `impl Judge for u8`. The same merge held
      at every boundary the cast applies (closure results, channel elements, struct fields,
      `Option` payloads, type-parameter bindings), and in the other direction an `i64` at a
      declared `u8` kept its `i64` dispatch. The parameter, `let` and field coercions also
      RE-TAGGED a `SizedInt` of another width to the declared one without converting its value.
      (b) A NON-CLOSURE AT A FN TYPE: `cast_at` at `T::Fn` accepted anything that was not a closure
      ("a named fn is referred to by name"), although no named fn is ever a value (E0306); and
      `eval_call`, for a local bound to a non-closure, fell through to a builtin and then a user fn
      of that NAME. With the suite's reference `fn square` and `let square = make_square()`, a
      candidate returning a confused `0` had the operator's own solution answer for it. The
      round-4b EQUIVALENCE review added: the enum-name check (`if enum_name != n`) had no row
      (with it removed, an `Other::A` at a declared `-> Grade` ran the operator's lenient impl for
      `Other`), and `conform.rs` and the interpreter's seal edges were outside the refusal-coverage
      scan.
    - **After (the cast, `interp/conform.rs`).** Each declared type admits exactly the values whose
      dispatch key it is. An integer by WIDTH: `i64`/`isize`/`usize` admit only `Value::Int`; a
      fixed width admits only a `SizedInt` of that width and CONVERTS an `Int` in place (what a
      parameter, `let` or field of that width always did); another width is refused, as the
      checker refuses it (E0307). The parameter, `let` and field coercions convert only an `Int`
      and no longer re-tag another width, which the cast then refuses. `Dict` admits only a dict.
      A soft wrapper (`Uncertain`/`Temporal`) at a declared type that fixes the runtime type (a
      scalar, struct, enum, refinement, container, `dyn`, union, `fn`, `Chan`; also a type
      parameter whose binding is one of these) is REPLACED by its inner value, so it dispatches as
      the declared type; at `?`, an unbound type parameter or an unknown name it is kept (honest
      generic code passes it through). A declared `Uncertain<T>`/`Temporal<T>` admits the wrapper
      of THAT name with its inner value cast to `T`, or a plain value cast to `T` (soft typing);
      the bare name admits only its wrapper. At a `fn(..) -> ..` type only a closure crosses.
    - **After (the call, `Interp::eval_call`).** A name bound in the local environment is called
      THROUGH ITS VALUE: a closure is called, anything else panics "value of type … is not
      callable". It never falls through to a builtin or fn of the same name. The checker already
      refuses a call through a local of a non-fn type (E0306), so no checked program changes
      meaning; the route existed only for a value whose type is `Deferred`.
    - **The remaining equivalence classes (audited).** `f32` and `f64` share one runtime
      representation (`Value::Float`, key `f64`); `str`/`String` one (`Value::Str`); `isize`/`usize`
      are `Value::Int` (key `i64`). In each case every value at the declared type, honest or not,
      has the same key, so the candidate's choice of value selects nothing. A tuple of another
      arity: a longer one's extra elements are unreachable (`t.2` on a pair is a checker error), a
      shorter one fails the read; neither changes the key (`tuple`). A closure of another arity
      panics at its first call and dispatches on `fn`. Declared names the interpreter does not
      know (the `Goal…` deferred prefix, a handle, `RawPtr`) still accept any value: no operator
      impl can be keyed to them at runtime (an impl's key is the rendered declared name, which no
      value's `type_name` produces), so the dispatch they lead to is the value's own key — that is
      the residual, recorded here.
    - **Restated non-claim (2) of amendment 53.** An annotation now pins the EXACT dispatch type:
      after `let r: T = X`, a method call on `r` runs the operator's impl for `T` (for a soft or
      unknown `T`, as stated above), whatever `X` was. What remains by design: the candidate
      chooses its OWN declared types, so a suite that dispatches on a value typed only by the
      candidate's signature runs the operator's impl for that declaration (e.g. a candidate
      declaring `-> bool` selects the operator's `impl Judge for bool`). And at a declared
      `Uncertain<T>`, soft typing lets the candidate hand either the wrapper or a plain `T`, so the
      operator's impl for `Uncertain` or for `T` runs accordingly; a suite that cares annotates the
      plain `T` (which unwraps). Non-claims (1), (3) and (4) of amendment 53 stand.
    - **Rows.** Blockers: M920 (an integer width at `i64`), M921 (another fixed width), M922 (an
      `i64` converted at a fixed width), M923 (only a closure at a fn type), M924 (a local is called
      through its value), M925 (`Dict`), M926 (a soft wrapper unwrapped at a plain type), M927/M928
      (a declared soft type's wrapper name and inner value), M929/M930 (a plain value at a declared
      soft type; the bare name), M931 (the enum name — the EQUIVALENCE blocker). Audit of every
      refusal arm, each reached ALONE by its own attack: M932 (array), M933 (tuple), M934 (union),
      M935 (`Chan`), M936 (`Option`), M937 (`Result`), M938 (`f64`), M939 (`bool`), M1140 (`str`),
      M1141 (`()`), M1142 (`Decimal`), M1143 (a struct), M1144 (an enum), M1145 (a type parameter's
      binding), M1146 (`dyn Trait`), M1147 (a generic enum's variant fields), M1148 (a channel's
      queued values), M1149 (a refinement's base), M1150 (`check_impl`), M1151 (`kind_ok`), M1152
      (a channel send's cast), M1153/M1154 (a closure's argument and result casts), M1155 (the
      return site), M1156 (the call edge's own condition, `seal_call`), M1157 (a non-integer at a
      fixed width). M652 is RE-ANCHORED ACTIVE on the `i64` arm (its old text was the merged
      integer arm; its attack, a confused `bool` at `-> i64`, is unchanged). The seal edges'
      other rows already exist: M651 (`seal_method`), M87 (`seal_global`), M88 (`seal_refine`),
      M562 (`handler_may_answer`), M563 (`rng_guard`), M86/M89 (the `seal_call` call sites). The
      generic-route attacks hand an `i64` to the operator's `judge<T: Judge>`, whose lenient `i64`
      impl is the keyed pass, so each declared type's own arm is the only check in the way; each
      marker is the attack's test COMPLETING.
    - **Coverage scan.** `scripts/v022_refusal_coverage.py` scans `conform.rs` and the seal-edge
      region of `interp.rs` (between two anchors; the rest of that file is not a protected
      decision), and counts the interpreter's `return panic(` as a refusal. Exempt, with the
      reason: the depth bound (no input reaches it: a value 1,000,000 deep cannot be built in a
      run; measured 2,000/4,000/8,000 levels at 1.7s/7.8s/78s) and the closure-arity check (no
      consequence, above). The unknown-variant refusal lies in M931's block; no input reaches it
      either (one enum per name, E0002; an undeclared variant is E0404). `shape` was renamed
      `type_of_value` (the scanner reads `shape(` as the loop's refusal constructor).
    - **Tests.** Real runner,
      `crates/axon-psv/tests/sealed_frames.rs::the_candidate_never_selects_the_operators_code_by_width_or_by_name`:
      the review's width candidate (pinned and unpinned), its fn-reference candidate, and a
      `Dict`-table route that reaches the call with no declared fn type in the way; GOOD is a keyed
      pass and WRONG a keyed failure on every suite, every attack a keyed failure. Against the
      interpreter at 6d6517a1 it fails (keyed pass for the pinned width candidate). Interpreter
      (`crates/axon-core/src/interp.rs`):
      `a_value_of_another_integer_width_never_crosses_a_declared_integer` (seven boundaries),
      `a_value_of_another_fixed_width_never_crosses_a_declared_fixed_width`,
      `an_i64_at_a_declared_fixed_width_takes_the_width`, `width_correct_values_cross_unchanged`
      (generic code, width-correct values, literal conversion), `a_non_closure_never_crosses_a_declared_fn_type`,
      `a_call_through_a_local_never_resolves_the_name_elsewhere` (controls: a local closure
      shadowing the fn is called; with no local the fn is called), `a_soft_wrapper_takes_the_declared_plain_type`,
      `a_declared_soft_type_casts_its_wrapper_and_its_inner_value`,
      `a_confused_enum_never_crosses_as_another_enum`,
      `every_declared_type_refuses_a_value_of_another_type`,
      `the_remaining_cast_arms_refuse_a_value_of_another_type`. The whole axon-core suite and the
      examples stay green.
    - **Evidence.** `v022_g01_mutations.py --scope=all --only=M652,M920-M939,M1140-M1157`: at
      3cf23eef 36/39 KILLED (M939/M1140 REFUSED_ELSEWHERE, the union case ran first; M1155's
      mutation did not compile); at 8a5419de, after reordering the cases and fixing M1155's
      mutation, the 15 rows sharing that test plus M1155 all KILLED, so all 39 are KILLED by their
      own attack. Refusal coverage (`--without=M923,M931,M1156` names their sites) and the
      matrix check PASS. The guest image rebuilt from a standalone clone at 3cf23eef (rootfs
      `0a45005c…935b`, axon `b29e04f0…f936`, runner `9853986c…513a`) passes every case of
      `scripts/psv_guest_boot_test.sh` (root, KVM); the re-pin is left to the integrator.
    - **Operator deployment.** The guest image must be REBUILT to carry the new interpreter; its
      scripts and runner are unchanged.

61. **The refusal-site gate's file set is a rule, not a list; the loop side's refusal sites are
    rowed or exempt; an execution is attested only by a verifier the operator qualified for its
    profile (C9 round 4b fix wave, rows4a workstream; rows M940-M1019; matrix A92).**
    - **Before.** Round 4b (EQUIVALENCE) found the refusal-site gate's `PROTECTED` list omitting
      whole protected decision files (`evl.rs`, `store.rs`, `attestation.rs`, `operator_trust.rs`,
      the Fabric backend, submit and binary) while `NOT_YET_SCANNED` was `{}`: a hand-kept list
      standing in for the real set (class a). Two guards had no row: `evl::verify_execution`'s
      check that the execution attestation's signer is a trusted verifier (with it removed, a
      revoked verifier's attestation counted and the owner and consumer suites stayed green), and
      `operator_trust::verify_evidence_signature`'s RULE:issuer-trusted.
    - **Found and fixed (A92).** `verify_execution` checked that the signer is trusted and that its
      key is operator-rooted, but not that the operator QUALIFIED it for the profile it attests.
      The operator's `verifier_pins[...].backend_profiles` bounded only verdicts
      (`intake::check_pins`). A verifier the operator trusts but pinned only for development
      backends attested a protected execution, and the trial counted in EVL and re-verified at
      admission (failing reproduction: `an_execution_attested_by_a_verifier_not_qualified_for_its_profile_counts_nothing`,
      `verified_pass` 2). `verify_execution`, the one primitive both doors call, now also
      requires the signer's operator pin to name the receipt's backend profile, read as it is now.
      The execution attester is deliberately NOT joined to the verdict's verifier: each is
      operator-trusted and qualified for the profile, and each signs its own documents, which the
      episode binds by digest; requiring one identity would add no authority.
    - **The rule (`scripts/v022_refusal_coverage.py`).** In scope: every `.rs` file under
      `crates/axon-fabric/src`, `crates/axon-loop/src`, `crates/axon-loop-contracts/src` and
      `crates/axon-psv/src` (recursively, bins included), `crates/axon-core/src/interp/conform.rs`,
      and in `interp.rs` and `interp/eval.rs` the non-test functions whose name matches
      `seal|conform|cast` (the seal edges; the interpreter's `panic(` refusal constructor is now
      a site). Each in-scope file is exactly one of: SCANNED (every site rowed or exempt with a
      reason, else BAD), `OUT_OF_SCOPE` (named with a checkable reason: `evo.rs`, `profile.rs`,
      `bin/axon-loop.rs`), or `NOT_YET_SCANNED` (named with its measured count of uncovered
      sites, which the gate re-measures and refuses when it differs). An in-scope file in none of
      the tables is SCANNED, so a new file is judged the day it appears; a table entry naming a
      file outside the rule is refused. `--freeze` fails while `NOT_YET_SCANNED` is non-empty, and
      `v022_freeze_manifest.py` runs the gate that way and refuses to bind a freeze otherwise
      (test: `freeze_manifest.rs::a_freeze_is_refused_while_a_protected_file_is_not_yet_scanned`);
      the gate's digest and file count are bound into the freeze manifest.
    - **Loop side, per file (sites: rowed / exempt).** `evl.rs` 64: 49 / 15 (RE-REPORTED 2,
      NOTHING TO ADMIT 6, UNREACHABLE 5, NAMED ROW 2); `store.rs` 17: 6 / 11 (OPERATOR-AUTHORED 2,
      UNREACHABLE 1, OS ERROR 3, SELECTS NOTHING 4, NOTHING TO ADMIT 1); `attestation.rs` 19:
      2 / 17 (NOTHING TO ADMIT 7, SELECTS NOTHING 9, NAMED ROW 1); `operator_trust.rs` 13: 8 / 5;
      `safety.rs` 8: 5 / 3; `tasks.rs` 5: 5 / 0; `candidates.rs` 5: 5 / 0; `rules.rs` 4: 3 / 1;
      `epoch.rs` 1: 0 / 1; loop `lib.rs` 1: 0 / 1. `plan.rs` 26: 18 / 4, with 4 left NOT YET
      SCANNED (below). Admission's execution leg is `verify_execution` (M941, M943).
    - **Rows.** ACTIVE, each killed by its own attack on the production route: M940-M947,
      M949-M953, M958-M961, M963, M965-M969, M975-M978, M982-M997, M999-M1019. M965 (the
      policy half of `bind_episode`'s byte binding) is the library primitive's own contract,
      killed by its crate's test of it (`fixtures.rs::bind_episode_refuses_mismatches`): every
      production caller decides the predicate first (EVL's M964; intake fetches the policy by the
      episode's own ref), and the four-cell run showed the primitive's suite needs it, so it is not
      retired. Retired EQUIVALENT_DID with four-cell records (never counted): M948 (vs M950: a
      symlink's lstat mode is 0777 on Linux), M954/M955, M956/M957, M962 (vs M15 + M963), M964
      (vs M965), M970/M971, M972/M973, M974 (vs M972 + M973), M979/M980/M998 (the three lstat
      checks of the store path), M981 (vs M819). Tests that pinned WHICH of two independent
      refusals answered were widened to accept either (the precedent of M487):
      `trust_root.rs`, `readiness.rs`, `protected_host.rs` (symlink or mode) and `redteam.rs`'s
      AB9 cherry-pick (one evaluation per experiment, or the population join). The
      RULE:issuer-trusted row (M944) is killed through readiness (an agent-signed B263 record),
      the production decision.
    - **Not done here.** `NOT_YET_SCANNED` lists, with counts: the Fabric files (rows4b:
      `backend.rs`, `submit.rs`, `git_data.rs`, `provenance.rs`, `bin/axon-fabric.rs`; unassigned:
      `branches.rs`, `grants.rs`, `journal.rs`, `signing.rs`, `workspace.rs`, `axon-psv-runner`),
      the interpreter's (core2: `conform.rs`, `interp.rs`'s seal edges), and on the loop side
      `ledger.rs`, `pointer.rs`, `price.rs`, `tel.rs`, the contract layer (`canonical.rs`,
      `checks.rs`, `compute.rs`, `episode.rs`, `ids.rs`, contracts `lib.rs`, `policy.rs`,
      `receipt.rs`, `schema.rs`) and `plan.rs`'s four dominated sites (incumbent equals candidate;
      the candidate's scope, candidate view, and an added tool), whose four-cell attacks are written
      (`tests/plan_sites.rs`) but whose rows need ids past M1019. A freeze refuses until the list is
      empty. The two new script guards (the freeze's coverage refusal; the gate's re-measured
      count) have tests but no mutation row: the id range is exhausted.
    - **Evidence.**
      - `v022_g01_mutations.py --scope=all --only=<the 63 ACTIVE rows above but M965>` at
        698d4912: 63/63 KILLED by their own attack, none refused elsewhere, no survivor
        (`/var/tmp/c9r4b-rows4a-mut-final3.json`); M965 (with M951, M963) at 9ad610fb: KILLED
        (`/var/tmp/c9r4b-rows4a-mut-M965.json`).
      - `v022_paired_disable.py --only=...`: all four cells and the full-suite condition hold for
        M954-M957, M962, M964, M970-M974, M979-M981, M998 at 698d4912
        (`/var/tmp/c9r4b-rows4a-pd{A,B,C}.json`; pdB also holds the superseded M965 attempt,
        whose full suite was red, which is why M965 is ACTIVE) and for M948 at 9ad610fb
        (`/var/tmp/c9r4b-rows4a-pdD.json`).
      - Full suites (rc 0): axon-loop and axon-loop-contracts (381), axon-reflex and
        cortex-policy-adapter (38), axon-fabric single-threaded (553); `cargo build -p
        axon-fabric --bins`; fmt; clippy -D warnings on axon-loop, axon-loop-contracts and
        axon-fabric. `v022_refusal_coverage.py` passes, `--freeze` refuses (27 files NOT YET
        SCANNED); the paired-disable join self-test passes. `psv_matrix_check.py` reports only
        A91 missing, a row another workstream holds.
    - **Operator deployment.** A verifier whose execution attestations a protected evaluation is
      to count must be pinned for `linux-microvm-protected` in the loop store's `verifier_pins`.
      The Fabric verifier that already signs the protected verdicts is pinned so; nothing else
      changes.

62. **The refusal-site gate covers the Fabric decision files; the root helper's report, the protected
    executable and every B263 qualification rule have rows; the protected PCI lineage names the
    certified revision by its whole hash (C9 round 4b, EQUIVALENCE (4), Fabric side; rows4b
    workstream, rows M1020-M1100, matrix A91).**
    - **Before.** The gate scanned none of `backend.rs`, `submit.rs`, `git_data.rs`,
      `provenance.rs`, `bin/axon-fabric.rs`, `signing.rs`, `workspace.rs`, `journal.rs`,
      `branches.rs`, `grants.rs` (`bin/axon-provenance.rs` has no site). Measured at 6d6517a1 with
      the gate's own site rule, 240 refusal sites there had neither a row nor an exemption
      (backend 45, submit 37, git_data 23, provenance 5, bin/axon-fabric 54, signing 2,
      grants 6, branches 22, workspace 20, journal 26). With the helper report's schema check, its
      exit/error check, its same-byte check (`!report.unchanged || helper.unchanged()`) and the
      protected executable's id and digest checks all removed at once, the whole axon-fabric suite
      stayed green (549). accept_b263's issuer-claimed, pass-count, blocked-count, blocked-unwaived,
      waiver-reason, waiver-expiry, end-not-future, engine-digests and caveat rules, the waiver
      binding, the qualification's schema, profile, engine-pin, manifest-clean and manifest-binding
      rules, and RULE:issuer-trusted had no row. The guest manifest's PCI lineage resolved the
      certified revision `31413ca7` (32 bits) by abbreviation.
    - **After.**
      - The gate scans those eleven files (one marked block in
        `scripts/v022_refusal_coverage.py`). Every site is rowed or exempt with a stated reason
        (kinds: NOT A SITE, NON-UNIX/cfg, RESOURCE BOUND, RE-REPORT, NAMED ROW, NOTHING TO ADMIT,
        OS ERROR, OPERATOR-AUTHORED, UNREACHABLE, USAGE, DEVELOPMENT ROUTE, NOT ON THE PROTECTED
        ROUTE, NOT A VERDICT PROPERTY; FLAGGED marks a reason the integrator must accept or
        replace).
      - `provenance::descends_from_protected` (the `--lineage` answer the guest manifest binds)
        refuses a certified revision that is not a full 40-hex commit id, and
        `scripts/linux_profile_manifest.py` names it as
        `31413ca7abb6ff730e1b63718d4304c7a8402675` (A91). The development check (`--descends`,
        build-guest-image.sh's early check) still resolves an abbreviation, by hash only.
    - **New ACTIVE rows, each killed by its own attack through a production route:**
      - Helper report, through `submit` on the helper route with a stand-in helper (an ELF
        trampoline that runs the REAL test-trust helper and passes its report through `sed`):
        another schema (M1020), a clean report with exit EXIT_UNKNOWN (M1021), an error after the
        launch with exit EXIT_LAUNCHED (M1022), a launcher that changes its own inode during
        `--verify-result` so the REAL helper reports `unchanged: false` (M1023), a helper that
        changes its own inode during the launch (M1024).
      - Protected executable, through `submit`: another executable id with that id's digest
        (M1056), the guest id with another interpreter digest (M1057).
      - accept_b263, on BOTH production routes in one test (Fabric's `submit`, which qualifies
        before launching, and readiness's `protected_components`): issuer-claimed (M1025),
        pass-count (M1026), blocked-count (M1027), blocked-unwaived (M1028), waiver-reason (M1029),
        waiver-expiry (M1030), engine-digests (M1032, readiness joins only the firecracker digest,
        so readiness alone is reached there), caveat (M1033), waiver-bound (M1034),
        RULE:issuer-trusted (M1035: a record signed by a key no root holds, NAMING that key, so
        issuer-claimed agrees).
      - RULE:end-not-future (M1031) on the input where it alone refuses: under no practical
        maximum age (`max_age_s = u64::MAX`), a future record's negative age read as unsigned
        passes RULE:end-fresh.
      - The qualification's own rules, through `select` and `submit`: schema (M1036), profile
        (M1037), engine-pin equality (M1039), manifest-clean (M1040), the record qualifies THIS
        manifest (M1041), a waiver file of another schema (M1042).
      - Backend selection, through `submit`: architecture (M1044), checkpoint kind (M1045),
        reproducible grant (M1046), brokered network (M1047), engine (M1048), x1 guest policy
        channel (M1049), x2 path scope on the protected profile (M1050), os=linux without
        isolation (M1051), hardware isolation with os=none (M1052), a path-scoped grant on the
        host (M1053), interpreter_run on the host (M1054), a policy the guest cmdline would
        truncate (M1055).
      - Submit: an absent authority store is not epoch 0 (M1058), argv with an element the run
        ignores (M1059), refuse_links' body, every call (M1060), a suite edited after
        registration (M1061), the axon-os supervisor's refusal (M1062), one op id one input
        (M1063), an orphan under a superseded epoch (M1064), a stale-epoch request is not
        journalled (M1065), a cancelled branch never runs again (M1066), the epoch re-read before
        the launch (M1067), the grant's cost budget (M1068), the placeholder policy (M1069).
      - Lineage and provenance, through the built `axon-provenance`: the allowlist chain's owner
        (M1070) and write bits (M1071), a certified revision naming a
        blob (M1073), a tag chain past the peel limit (M1074), an ambiguous abbreviation (M1075),
        build provenance calling the config refusal (M1076), the full-id rule (M1084, A91).
      - The axon-fabric binary: status/cancel's principal|grant binding (M1077); the protected
        signer key derives its pin (M1078), is readable by no one else (M1079) and is owned by
        the Fabric uid (M1080). These are `bad(` uses the gate's site rule does not see (flagged).
    - **Retired EQUIVALENT_DID (four-cell records):** M1038 (the engine pin is required) against
      M1039; M1043 (an empty qualification root) against M1035; M1081/M1082 (blob re-verified;
      tree re-derived) as a mutual pair; M1083 (the manifest hashes to its reference) against
      M1082; M1072 (discover's symlinked `.git`) and M1085 (refuse_config's git-dir location) as a
      mutual pair (the first run read M1072 REFUSED_ELSEWHERE: refuse_config refused the same
      symlink, which had been exempted as nothing-to-admit; it is now rowed). Their tests accept either refusal; three existing tests that named one message
      (`no_trusted_issuer_configured_refuses_even_a_signed_record`,
      `the_committed_profile_has_no_trusted_issuer_so_protected_dispatch_is_refused`,
      `a_manifest_that_pins_no_engine_is_refused`) now accept either.
    - **Flagged for the integrator.** `bin/axon-fabric.rs`'s `ProtectedHost::operator()` refusal
      (a config that exists but does not load must not read as no protected host) is reached by no
      test: operator() reads only `/etc/axon`, which tests may not write. Exemptions marked
      FLAGGED (git_data `run`'s status check, `config.worktree`, the unreadable config, provenance's
      check-ignore count, workspace's duplicate and blob/manifest rewrite checks, the status/cancel
      grant resolution, grants' principal binding) state a domination or reachability argument
      that has no four-cell record.
    - **Evidence.**
      - At 5aeffbf1, `v022_g01_mutations.py --scope=all --only=<60 ACTIVE rows>` in two shards
        (`/var/tmp/c9r4b-rows4b-mutA.json`, `-mutB.json`): 57 KILLED by their own attack;
        M1068 and M1072 REFUSED_ELSEWHERE and M1079 SURVIVED (each fixed at 8d0ad933, above).
      - At 8d0ad933, `--only=M1068,M1079` (`-mutC.json`): both KILLED.
      - At 4b527e5f, `v022_paired_disable.py --only=M1038,M1043,M1081,M1082,M1083,M1072,M1085`
        (`/var/tmp/c9r4b-rows4b-pd.json`): all seven HOLD with full_suite SUITE_OK (exit 1 only
        because a new `--only` file holds no other record).
      - Refusal coverage PASSES; the matrix check fails only on A91/A92 (other workstreams'
        rows, reconciled by the integrator).
    - **Wave 2: the strict reading (integrator ruling).** A site exempted because another check
      refuses the same input needs an ACTIVE row whose attack reaches it alone, or an
      EQUIVALENT_DID retirement with an all-paths record and an executed four-cell run.
      - **New ACTIVE rows, each killed by its own attack:**
        - a stored version whose manifest names one path twice (M1090);
        - status/cancel resolving the grant at decision time, attacked by a grant the operator
          revoked after submission on a protected host (M1092);
        - the grant's principal binding (M1093) and its pinned bytes (M1098);
        - a protected-host config that exists but does not load (M1094). The production lookup
          runs under `unshare -m --propagation private` with a tmpfs on /etc/axon, and a
          development call naming its own grant registry must be refused;
        - signing's local-class arm (M1099) and its effectful-local arm (M1100).
      - **Retired EQUIVALENT_DID, four cells executed:**
        - run()'s status check vs M451, attacked by a corrupt index (M1086);
        - config.worktree vs M450 (M1087);
        - the check-ignore count vs M500, attacked by an ignored name that is not UTF-8 (M1089);
        - publication's byte comparison vs M1081+M1082, attacked by a blob planted before
          publication (M1091);
        - check_target's two job-kind arms vs M1054 (M1095, M1096);
        - the allowlist chain's symlink rule vs M1071 (M1097).
      - **The unreadable-config refusal is not rowed.** Its four-cell run read
        set_off=ATTACK_REFUSED. discover's own git call (rev-parse on the same config) refuses
        first on every caller, and with that check also removed an empty path fails to
        canonicalize. It is exempt as UNREACHABLE, with the measurement, and M1088 is
        unallocated.
      - **Source fixes:**
        - `publish_file` compares an existing file in ONE place, after the no-clobber rename. A
          second, earlier comparison could be reached only by a race.
        - `RunDir::new` creates the run dir non-recursively, so a leftover dir is refused, never
          reused. This was the development-route bug flagged above.
      - **Other dominated-kind exemptions restated:**
        - NOTHING TO ADMIT for the missing signature file and the missing manifest;
        - NO OUTCOME for a malformed parent, which never equals the 40-hex target;
        - UNREACHABLE for the relative allowlist path (test-only constructor) and for
          `DestinationExists` (every caller materializes into a dir it created new);
        - NAMED ROW only where the row's mutation makes the same removal at the same site
          (M321, M253).
      - **Checkable facts now in the reasons:** journal.rs names the three places the journal
        is read back, and that a replay is never signed (signing.rs `if replayed`, M01/M402).
        branches.rs names the grep showing that no production code calls open_experiment,
        publish or cancel.
      - **Evidence (wave 2):**
        - At c5c21986, `--only=M1090,M1092,M1093,M1094,M1098,M1099,M1100`: all KILLED
          (`/var/tmp/c9r4b-rows4b-mutD.json`).
        - `v022_paired_disable.py --only=M1086,M1087,M1089,M1091,M1095,M1096,M1097`: all HOLD
          with full_suite SUITE_OK. M1088 did not hold and is withdrawn
          (`/var/tmp/c9r4b-rows4b-pd2.json`).
        - axon-fabric suite: 630 passed. axon-loop-contracts: 70 passed. Refusal coverage and
          the matrix check (A91, FLOOR 91) pass.
    - **Operator deployment.** The guest image built after this amendment carries
      `pci_lineage.certified_revision` as the full id; `profiles/linux-microvm/manifest.json` is
      regenerated by the next image build (harness), not edited here.

63. **Every byte of the guest image is made in the controlled build environment, and the freeze
    judges the whole image (C9 round 4b, harness3: three FIELD-ORIGIN major-adjacent findings on
    the guest image's build provenance).** No counting rule is relaxed.
    - **Before.** Amendment 56 built the three guest binaries through
      `scripts/guest_build_env.py`, but (1) cargo's effective configuration was checked ONCE, at
      `begin`, while cargo re-reads it on every invocation from its working directory upward; the
      build ran in the clone, under `/var/tmp` (drwxrwxrwt), so a `.cargo/config.toml` planted in
      an ancestor after `begin` wrapped all 34 rustc invocations of the next build, and the record
      still read `foreign=[]` and passed the freeze (executed by the reviewer); (2) the freeze's
      judge (`shape_problems`) never read the recorded `builds`, so a record showing
      `--config build.rustc-wrapper=...` or `-C linker=...` in RUSTFLAGS was accepted, and
      `run_cargo` passed any argv to cargo; (3) `vmlinux` was built with the caller's whole
      environment (the gcc on PATH, KCFLAGS/KCPPFLAGS/CROSS_COMPILE/LLVM/CC, MAKEFLAGS) and
      `rootfs.sqfs` with the caller's `mksquashfs`, and the freeze bound only the three Rust
      artifacts to a build record; the manifest's `kernel.cc` was `gcc --version` from whatever
      PATH ran the manifest step.
    - **After.**
      - *Immutable for the build.* `begin` copies the tree's TRACKED files (working-tree content,
        hardened `/usr/bin/git ls-files`) into `<base>/src`, where `<base>` is a fresh directory
        under a BUILD PARENT (`<builder home>/.cache/axon-guest-build`, or
        `AXON_GUEST_BUILD_PARENT`) whose every ancestor must be owned by root or the builder and
        not group/other-writable (a sticky `/var/tmp` is refused). Cargo runs there, never in the
        clone, so no other uid can place a config anywhere cargo looks. The provenance snapshot
        is now taken BEFORE the copy, so the copy lies between two clean observations of the tree.
        (A consequence: the guest `axon --version` reads `(unknown)`; the manifest binds the
        revision.)
      - *Checked per invocation.* `cargo` re-reads the effective configuration before AND after
        every invocation; either differing from begin's refuses (before: cargo never starts;
        after: the step fails). Both checks are recorded in the invocation's entry.
      - *Exact invocations.* `cargo` runs only an entry of `INVOCATIONS` (args and RUSTFLAGS
        exactly; the protected three plus the development backends' three) and refuses anything
        else before cargo starts.
      - *Kernel.* `guest_build_env.py kernel` copies the pinned tarball, config and overlay into
        a private directory, verifies each COPY against `kernel.pin`, extracts with `/usr/bin/tar`
        and runs `/usr/bin/make ARCH=x86_64 olddefconfig` and `... -jN vmlinux` under a
        constructed environment (`HOME`, `LC_ALL=C`, `PATH=/usr/bin:/bin`, the four
        `KBUILD_BUILD_*`; nothing of the caller's). It records path, realpath, sha256 and version
        of make, gcc, cc1, as, ld (required) and of the other host tools present, plus the
        effective config and vmlinux digests, in `kernel-build.json`, which the manifest carries
        as `kernel.build_environment` (and `kernel.cc` is now read from it).
      - *Rootfs.* `guest_build_env.py rootfs` assembles the root filesystem from the record's own
        artifacts (each COPY re-hashed against the recorded digest), the pinned busybox (its COPY
        verified), the tree copy's `guest-init.sh`, with `/usr/bin/mksquashfs` and its exact
        flags under a constructed environment; inputs, tool identity, argv and output digest go
        into the record (`rootfs`).
      - *The judge.* `shape_problems` additionally requires every recorded invocation to be its
        table entry with both config checks equal to begin's, the toolchain to be the pinned
        channel's (channel, toolchain directory, and the host linker's identity), and cargo to
        have run on `<base>/src` under a recorded builder-only parent. The freeze then applies
        `image_problems` to the WHOLE manifest: the binaries are exactly the protected builds in
        order; `rootfs.sqfs` is the controlled assembly's output from the record's artifacts and
        the manifest's busybox and guest-init digests, made by `/usr/bin/mksquashfs`, exact flags,
        constructed environment; `vmlinux` and the effective config are the controlled kernel
        build's, from exactly the manifest's pins, with make in the constructed environment, the
        required tools recorded and a builder-only parent. The freeze binds the kernel record's
        digest and its gcc. The record schemas are `axon-guest-build-env/2` and
        `axon-guest-kernel-build/1`; a manifest built before this amendment does not freeze.
    - **What remains recorded, not verified.** Tool identities (rustc, cargo, gcc, cc1, as, ld,
      make, mksquashfs) are their sha256 at build time; nothing independent pins the expected
      digests, and shared libraries they load are not hashed. This is the round-4b FUTURE item
      for rustc, now covering the C toolchain too: **operator item** — pin the expected host
      toolchain digests in operator trust (or a toolchain manifest) for the qualified build host.
      The build user and root remain trusted: they can write the build parent.
    - **Rows** (M1180-M1198, all PSV; M1199 unused):
      - M1180: `begin` refuses a build parent with an ancestor another uid can write.
      - M1181: cargo runs on the private copy (the reviewer's attack: an ancestor config of the
        clone written after `begin`).
      - M1182 / M1183: the effective-config check before / after every invocation.
      - M1184: `cargo` runs only a table invocation (attack: `--config` naming a wrapper).
      - M1185 / M1186: the judge holds each recorded invocation to its table entry / to both
        config checks (attack: a record with a linker in RUSTFLAGS / a config that appeared
        during the runner's build).
      - M1187: the binaries are exactly the protected builds in order.
      - M1188: the toolchain is the pinned channel's.
      - M1189: the rootfs installs only the recorded bytes.
      - M1190: the judge requires the private copy under a builder-only parent.
      - M1191: the rootfs is made by `/usr/bin/mksquashfs`, never the caller's PATH's.
      - M1192: the kernel's make runs in the constructed environment (attack: KCFLAGS, CC,
        CROSS_COMPILE and a planted PATH).
      - M1193: each pinned input is verified as the copy used (attack: another well-formed
        tarball; also config, overlay, busybox).
      - M1194: the freeze applies the whole-image judge.
      - M1195 / M1196: vmlinux is the controlled kernel build's from the manifest's pins / its
        make ran privately in the constructed environment with the toolchain recorded.
      - M1197 / M1198: rootfs.sqfs is the controlled assembly's from the controlled artifacts and
        pins / made by `/usr/bin/mksquashfs`, exact flags, constructed environment.
    - **Tests.** `crates/axon-fabric/tests/guest_build_env.rs` (production route: the
      `--build-env-only` begin of `build-guest-image.sh`, then the `cargo`, `finish`, `rootfs` and
      `kernel` steps the image build calls, in a scratch git checkout): the three existing tests
      (the ancestor-config cases now planted above the build parent) and nine new ones, each an
      ATTACK with an honest control. `crates/axon-fabric/tests/freeze_manifest.rs`: the fixture
      is now a whole controlled image (cargo + rootfs + kernel records); six new tests, 28 attack
      cases. M738's test keeps the record self-consistent (its rootfs inputs follow its
      artifacts) so it still reaches its own guard, and M737's case records the same foreign config in every invocation's checks (a self-consistent record only the begin-time judge refuses; the first run at 9f506848 had M737 survive, refused by M1186 instead); `image_problems` does not repeat
      `shape_problems`, so M734-M737 keep a single owner. `b263_profile_wiring.rs` now reads the
      rootfs install from the controlled step.
    - **Evidence** (pre-squash commits on c9r4b/harness3; the final commit carries the same
      scripts and tests).
      - `v022_g01_mutations.py --scope=all --only=<54 rows>`: every active row guarding
        `guest_build_env.py`, `v022_freeze_manifest.py`, `linux_profile_manifest.py` or
        `build-guest-image.sh`, or killed by a test in `guest_build_env.rs`, `freeze_manifest.rs`,
        `guest_provenance.rs` or `b263_profile_wiring.rs` (M284, M454-M458, M493-M499, M505,
        M506, M581-M584, M607-M609, M642, M650, M730-M738, M872, M873, M1180-M1198): 53/54
        KILLED by their own attack, M737 SURVIVED (refused by M1186: see Tests). After the M737
        fixture fix, M734-M737 4/4 KILLED; after a test-only path fix in `guest_build_env.rs`,
        its twelve rows (M730-M732, M1180-M1184, M1189, M1191-M1193) 12/12 KILLED. No
        REFUSED_ELSEWHERE.
      - A FULL image build (`AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh`, kernel
        included) from a standalone clone succeeded; `shape_problems` and `image_problems` of its
        manifest are both empty. The controlled kernel build reproduced the pinned vmlinux byte
        for byte (`4ec3ba40...22b7`). `scripts/psv_guest_boot_test.sh` (root, KVM) on that image:
        17 PASS, 0 failures, no SKIP. The manifest reads dirty only because this host has no
        operator `/etc/axon/provenance-allowlist` (`dist/` unexcused); no re-pin is committed.
    - **Operator deployment.** None for the scripts. The integrator rebuilds the final image with
      the FULL build (`AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh`, kernel included:
      a `--rootfs-only` build needs a `kernel-build.json` from a controlled kernel build) from a
      standalone clone and re-pins; the operator item above stands.

64. **Every in-scope refusal site is rowed, retired four-cell, or exempt by a checkable fact; the
    refusal-site gate holds at a freeze; a guard killed only by its library test is a separate,
    never-killed class; the final paired-disable's eleven full-suite failures are triaged (C9 round
    4b, integration of integrate-A..E, integrate-2 and integrate-3; rows M1200-M1403, M1450; no
    matrix row).**
    - **Before.** Amendment 61 left `NOT_YET_SCANNED` non-empty, so `v022_refusal_coverage.py
      --freeze` (and with it every freeze) refused: the whole contract layer (`canonical.rs`,
      `checks.rs`, `compute.rs`, `episode.rs`, `ids.rs`, contracts `lib.rs`, `policy.rs`,
      `receipt.rs`, `schema.rs`), `ledger.rs`, `pointer.rs`, `price.rs`, `tel.rs`, `plan.rs`'s four
      dominated sites and the psv runner; and several exemptions on `evl.rs`, `store.rs`,
      `intake.rs`, `operator_trust.rs` rested only on "another check refuses first". The gate's
      own guards (the re-measured count, the freeze reading, the interp region union, anchor
      uniqueness, the freeze's consultation) had no rows. Rows whose only kill was a direct test of
      a public library function had no honest class: neither ACTIVE (no production attack reaches
      them alone) nor EQUIVALENT_DID (the full-suite condition fails on the library test).
    - **After (the gate rule).** Unchanged from amendment 61: every `.rs` under the protected
      crates' `src` plus the interpreter's seal regions is SCANNED, OUT_OF_SCOPE with a checkable
      reason, or NOT_YET_SCANNED with a re-measured count, and `--freeze` fails while the last is
      non-empty. `NOT_YET_SCANNED` is now EMPTY: `v022_refusal_coverage.py` and `--freeze` both
      pass (rc 0), so the freeze manifest's coverage refusal (M1396/M1397) no longer fires.
    - **Per file (rowed / exempt; 0 uncovered).** canonical.rs 9/3; checks.rs 37/0; compute.rs
      2/0; episode.rs 8/0; ids.rs 8/1; contracts lib.rs 4/0; operator_trust.rs 8/5; policy.rs
      4/3; receipt.rs 9/0; schema.rs 15/13; ledger.rs 18/0; pointer.rs 41/1; plan.rs 24/2;
      price.rs 11/1; tel.rs 6/4; rules.rs 4/0; evl.rs 51/13; store.rs 10/7; intake.rs 44/19;
      axon-psv-runner.rs 1/2.
    - **Rows (by workstream).** integrate (gate guards): M1392-M1397 ACTIVE. integrate-A
      (contract crate, `tests/contract_sites.rs`): ACTIVE M1200, M1201, M1204-M1206, M1210-M1213,
      M1215, M1216, M1218, M1221, M1222, M1228-M1231, M1234, M1235; ACTIVE again (integrate-3, below) M1232; retired
      EQUIVALENT_DID M1202, M1203, M1207, M1219, M1220, M1225, M1226, M1237-M1240, M1242, M1243,
      M1245-M1252, M1254-M1262, M1265-M1267; LIBRARY_PRIMITIVE (integrate-3, below) M1214, M1217,
      M1223, M1224, M1233. integrate-B (`checks.rs`,
      `tests/checks_sites.rs`): ACTIVE M1270-M1293; retired M1294 (vs M1295), M1296 (vs
      M34+M1291). integrate-C (`ledger.rs`, `pointer.rs`): ACTIVE M1300-M1308, M1311-M1313,
      M1315-M1328, M1331-M1333, M1335-M1344; retired M1309, M1310, M1314, M1329, M1330, M1334.
      integrate-D (price, tel, plan, rules, runner): ACTIVE M1345, M1347-M1351, M1353-M1359,
      M1362; retired M1346, M1360, M1361, M1365-M1368, M1370, M1371, M1364 (below).
      integrate-E (strict rework): ACTIVE M1376; retired M1375, M1377-M1380; the four evl
      null/absent-value arms are NOTHING TO ADMIT with their executed cells named in the reasons.
      integrate-2: `pointer.rs` check_activate's admission route (`tests/activation_sites.rs`,
      genuine admissions through `pointer::transition`): ACTIVE M1386 (target), M1388 (H1), M1389
      (K1), M1390 (mechanism-test label), M1391 (deployment_enabled); retired M1387 (the
      admission's scope, set vs M1388+M1334: a cross-scope admission; each alone refuses, all
      three removed scope B serves scope A's candidate). `checks.rs`
      (`tests/checks_rest_sites.rs`): retired M1400 (empty shortlist, vs M1220+M1238), M1402 (role
      upgrade, vs intake's M45: a check request doubling as the execution request counts with
      both removed). `evl.rs` `matched_checks == 0`: retired M1372 (below). Harness rows M1373,
      M1374 (below).
    - **De-duplication.** One row per guard: integrate-A's M1268 duplicated C's M1330
      (`pointer.rs` next_epoch) and A's M1264 duplicated D's M1369 (`policy.rs`
      authority_expansion); M1268 and M1264 are removed (row, marker, record), M1266/M1267's sets
      now name M1330. D's and A's parse-layer rows are different sites (M1362/M1363 are parse's
      calls; A's are the walker's and the typed rules' own checks), so none was dropped.
    - **Ruling R1: LIBRARY_PRIMITIVE.** A guard of a public primitive that is dominated on every
      production route (every production input it refuses is refused on that route by another
      named guard, earlier or later to the same outcome) and whose retirement fails only because a
      direct library test needs it. `LIB_RECORD` in the registry names, per production route, the
      dominating guard and the library test; the mutation harness RUNS the row (its library-test
      kill is required) but reports it in its own class and NEVER counts it killed; the freeze
      manifest binds `LIB_RECORD` in its equivalence digest and counts the class separately from
      the active rows. Members: M965, M1295, M1297, M1298, M1299, M1352, M1363, M1369 (ruling R1's
      list, each verified against the code), and M1401 (`check_shortlist`'s scope/view,
      dominated by `bind_episode`'s M963/M1294 at intake and identity arguments elsewhere) and
      M1403 (`bind_acf`'s verified-bytes rule, dominated by M1296+M1291 on its one production
      caller), which meet the same definition; integrate-3 adds M1214, M1217, M1223, M1224 and
      M1233 (below). The class is now FIFTEEN rows: M965, M1214, M1217, M1223, M1224, M1233,
      M1295, M1297, M1298, M1299, M1352, M1363, M1369, M1401, M1403. Verified, and recorded honestly in the routes: for
      M1297, M1298 (evl), M1299, M1352 and M1363 the dominating guard runs LATER on the same route,
      not earlier (M857, M1281, M10, M1351, the pointer's fence / profile negotiation); M1299's
      Unknown kind and M1363's transition exit code (Conflict vs Refused) differ, the outcome
      (not counted / not applied) does not. Harness row M1373 (the class is never counted killed;
      `harness_integrity.rs::a_library_primitive_row_is_never_reported_as_killed`).
    - **Integrator ruling on E's flag (`evl.rs` `matched_checks == 0`).** Retired the EVL site,
      M1372, vs the contract code (episode.rs M1249, receipt.rs M1257) and the schema walker's
      minimum check (schema.rs M1218), which is the code that enforces the schemas' `minimum: 1`:
      no schema byte is a set member. The full-suite condition applies to the retired guard alone.
    - **SIBLING_ONLY (the same ruling, applied to integrate-A).** A's nine schema-JSON rows
      (M1208, M1209, M1227, M1236, M1241, M1244, M1253, M1263, M1269) were retired mutually with
      their code rows, and their own full-suite cells could only fail on
      `fixtures.rs::checked_in_schemas_are_the_package_bytes` (the MiCode package bytes are
      digest-pinned). They are now SIBLING_ONLY edits: members of exactly the code rows' sets
      `SIBLING_RECORD` names (M1247, M1248, M1255, M1256, M1258, M1261, M1262, M1265, M1250 are
      the retired rows), never active, never retired, never counted killed; paired-disable refuses
      a sibling-only edit used by a set its record does not name. Harness row M1374
      (`harness_integrity.rs::a_sibling_only_edit_is_never_an_active_row`).
    - **Ruling R3.** `plan_evo_tel.rs::freeze_refuses_candidate_equal_incumbent` requires that the
      freeze is refused (NotReady from `inc == cand`, or Refused from the candidate-provenance
      check M1011), not which answers (precedent M487); M1364 is then retired vs M1012.
      integrate-3 applies it once more: `axon-loop/tests/cli.rs::
      cli_pointer_baseline_resolve_transition_show_revoke` pinned exit 4 for a transition whose
      fence skips an epoch; on that CLI route (`parse` then `pointer::transition`, nothing written
      between) the two refusals are parse's typed rule M1266 (Refused, 4; library primitive) and the
      pointer's `next_epoch == current + 1`, M1330 (Conflict, 5). The test now requires a refusal
      (4 or 5) with the store unchanged; M1266 is LIBRARY_PRIMITIVE (round 4c triage: dominated on the one production route by M1329+M1330, and its removal alone fails fixtures.rs and parse_validate_sites; the earlier retirement claimed a green suite).
    - **Ruling R2 (FLAGGED exemptions).** integrate-A's four (ids.rs authority epoch, policy.rs
      target/admission refs, schema.rs non-integer number) state the experiment in their reasons.
      integrate-2 adds one, `operator_trust.rs` `!dir.is_absolute()`: NO RELATIVE INPUT (every
      production base is `/` and every dir a compiled-in constant or a loader-checked absolute
      config path); measured: with the check removed, `check_owned_chain("/", d)` for `etc`,
      `etc/axon/trust`, `./etc` and `` still refuses (`is not below /`).
    - **Also fixed.** `crates/axon-core/tests/refusal_coverage_gate.rs` (from the integrate
      commit) spawned the gate outside the script helper, which
      `harness_binaries::every_script_spawn_in_the_workspace_goes_through_the_helper` refuses; it
      now uses `script_spawn::script(.., Bins::NoWorkspaceBinary)`. PSV_IDS covers M1400-M1469 (integrate-3: M1450-M1469).
    - **Triage of the final paired-disable (integrate-3).** The 6-shard run at 1cf94ffc held for
      144 of 155 records. Every one of the eleven failures had its four cells and failed ONLY the
      full-suite cell (the retired guard removed alone, the owner's and consumers' whole suites).
      Each was reproduced through its exact path; none was called a flake without a measurement.
      - *M1214, M1217, M1223, M1224, M1233 -> LIBRARY_PRIMITIVE.* The failing tests were the
        primitives' own unit tests (`schema::tests::structural_rules` for the oneOf / required /
        additionalProperties rules, `schema::tests::profile_id_pattern_is_exact` for the pattern
        rule, `ids::tests::deserialize_validates_too` for the acf1 scheme). The all-paths claim was
        re-verified against the code: `validate_against`'s only callers are `canonical::parse`
        (every Contract document) and `PilotPlan::from_value`; on the first, typed serde refuses
        the same values later in the same parse (non-defaulted fields, deny_unknown_fields, the
        id/ref/acf1/currency newtypes, `ProfileId`/`PeerId` calling `pattern_matches` directly,
        `PinAck`); on the second, `strict_record` with its canonical round-trip and the
        experiment id's `TaskId::new`. The checked-in oneOfs never overlap, so `n == 2` occurs on
        no production document. Two record texts were wrong and are corrected in `LIB_RECORD`:
        M1214's said every oneOf member was a bounded string or null (the policy pin's `ack` is two
        closed objects; `PinAck` refuses them, redteam a4/s07 passed with M1214 removed), and
        M1233's said Acf1Ref is never read outside contract documents (evl's protected-evidence
        join reads one with plain serde, dominated LATER by `verify_check_evidence`'s schema
        parse; fabric holds only digests it computed or journalled). M1217 and M1233 dominate
        each other on the contract route (their four-cell pair held at 1cf94ffc); neither is
        counted killed. Each library test now prints an ATTACK marker.
      - *M1232 -> ACTIVE (a false retirement).* Its record said every Ref outside a contract
        document is compared with a computed digest or names a CAS file. `axon-loop pointer
        revoke --policy` does neither: the flag becomes a `Ref` (`Ref::new`, i.e. `check_hex64`)
        and `pointer::revoke` appends it to the authority ledger unread; no schema walk sees a CLI
        flag. With the hex rule removed a revocation naming `cl22:<HEAD's digest in uppercase>`
        is recorded (exit 0). M1232 is ACTIVE, killed on that route by
        `tests/cli.rs::a_revocation_naming_a_reference_that_is_not_a_digest_is_never_recorded`
        (control: the real digest is recorded).
      - *M1266 -> LIBRARY_PRIMITIVE*: its guard is dominated on the one production route by M1329+M1330, and its removal alone fails fixtures.rs and parse_validate_sites.
      - *M346 -> stays retired; the control is made deterministic.* 150 runs clean and 150 with
        M346 removed: all pass. M346's edit reaches the test only as the bytes of the copied
        `provenance.rs` blob, i.e. different random object ids. The control ("a unique
        abbreviation of HEAD") used HEAD's first 4 hex digits in a fixture of 19 objects, so it
        held by chance (each other object shares the prefix with p = 1/65536: about one fixture in
        3600; over the 154 fabric full-suite cells of the 1cf94ffc run, about 4%). The test now
        re-rolls HEAD (an empty commit) until `git rev-parse --disambiguate` names one object;
        the attack (a colliding blob) is unchanged. 40/40 and the whole file pass.
      - *M1095, M1043 -> stay retired; the custodian tests' waits are fixed at the source.* The
        mutations change nothing: the four tests pass clean, with M1095 and with M1043 removed,
        alone, three-way concurrent, and under 40 CPU spinners. The cell signatures ("No such
        file or directory" for the custodian socket; "Listening ... Terminated" with no
        request served, or no refusal written) are reproduced EXACTLY on a clean tree by delaying
        the socket activation 12 s: the namespace helpers waited 200 x 50 ms for the socket and
        the Python clients 20 s, which the 6-shard host exceeded. The bounds are now >= 180 s
        (`ACTIVATION_POLLS`, `CLIENT_TIMEOUT_S`), and a socket that never appears exits the
        namespace script non-zero -- a SETUP failure, never a verdict; before, the attack
        assertions (`!reply.contains("ok":true)`) could pass on a custodian that never ran. With
        the 12 s delay all four now pass; `privileged_launcher.rs` passes whole (58).
      - *M58 -> stays retired; a real axon-os defect fixed.* `acc_a1_smoke_kill_journey`: 10/10
        exit 4 clean, with M58 removed and with M58+M59 removed. The 8 was the job's TIMEOUT
        (Denied, axis time): `axon-os run` reset `<run>.kill` to `{"latch":"clear"}`
        unconditionally as it started, so a kill landing before the loaded `run` reached that
        line -- which `axon-os kill` accepts as a pre-arm and promises "a run starting with this
        id will pick it up" -- was discarded and the job ran out its 10 s. Reproduced by arming
        first: exit 8 after 10 s. The latch is now created clear only when absent
        (`create_new`), so an armed kill stops the run at its first poll (exit 4, 0 s). New row
        M1450 (`r27_acceptance.rs::a_kill_armed_before_its_run_starts_stops_the_run`; control: an
        un-armed run is stopped only by its timeout). acc_a1 stays strict (exit 4). No other
        interpreter retirement is on this route: the killable agent has no loop control, and the
        exit code is decided by `run_bounded`'s latch poll versus the timeout, not by the
        interpreter.
    - **Evidence.** Mutation: integrate-2's new rows 121/121 KILLED at 8640d94f and 6/6 at
      b1c0a3a3; integrate-3's changed and new rows (M1214, M1217, M1223, M1224, M1232, M1233,
      M1450) each KILLED by its own attack at the integrate-3 fix commit. Paired-disable: one
      joined record set at the commit carrying this amendment, every record re-executed under the
      currency rule (the five LIBRARY_PRIMITIVE rows and M1232 leave the retired set: 149
      records), all holding. Refusal coverage plain and `--freeze`: rc 0.
    - **Matrix.** No row. No PSV production hole was found: M1232's guard was present and
      refusing (its EVIDENCE claim was false, not the code), and the kill-latch defect is in the
      axon-os supervisor's R27 kill switch, outside the PSV negative matrix (it is rowed, M1450).
    - **Operator deployment.** None.

65. **The setuid helper's inherited-state resets are evidenced; the custodian is the program the
    operator pinned; the B263 record names the host it ran on; the host-toolchain pin has a
    reader; NoNewPrivileges is named and checked (C9 round 4b, gaps: the operator deployment kit,
    c9r4b/opkit a24e170a, found these code gaps).** No counting rule is relaxed.
    - **Before.** (1) `privileged_launcher::harden()` (the setuid helper's reset of signal
      dispositions and mask, umask, working directory, inherited descriptors, environment,
      dumpable flag and resource limits) had no test and no row: each reset could be deleted with
      the whole suite green. (2) The custodian was authenticated only by the uid that bound its
      socket (or root, systemd's activation); its PROGRAM was pinned nowhere, so any program
      serving the socket as the custodian uid could answer every spend `ok` (negative matrix A93).
      (3) `scripts/b263_qualify.sh` wrote `"host": "WSL2-nested"` and a fixed Hyper-V caveat
      whatever host it ran on; every protected receipt carries `qualification-host:<host>` and
      `qualification-caveat:` verbatim. (4) The kit's `/etc/axon/host-toolchain-pin.json`
      (`axon-host-toolchain-pin/1`) had no reader. (5) No harness ran
      `scripts/trust_root_guest_probe.sh` in a real guest (the preflight runs whatever
      `--guest-cmd` the operator supplies; its own test uses a mount-namespace stand-in). (6) A
      Fabric under NoNewPrivileges makes the kernel ignore the helper's set-id bit: measured, the
      production helper then runs as the Fabric uid and refuses every launch (euid rule, M602),
      with a reason naming a missing setuid bit or a nosuid mount; the preflight probes through
      `setpriv` without NoNewPrivileges and cannot see how the service is really started.
    - **After.**
      - (1) `a_callers_process_state_never_reaches_the_root_helper_or_its_launcher`: the
        setuid-root (test-trust) helper is executed by a hostile Fabric-uid caller (python3 as
        the Fabric uid) that ignores SIGTERM/SIGHUP/SIGINT, blocks SIGUSR1/SIGALRM/SIGTERM, sets
        umask 0, lowers FSIZE/CPU/NOFILE, raises CORE, leaves fd 9 open on its own file, works in
        its own directory and sets BASH_ENV/ENV/LD_PRELOAD/PATH. The stand-in launcher records,
        from /proc as root, its own state and its parent's (the helper's). One assertion per
        property, each its own row. Two resets have NO row, by a stated dominance:
        the environment clear (the launcher's environment is built from nothing by
        `sealed_exec::command`'s explicit envp, and no code on the helper's path reads a variable;
        the test still asserts the end-to-end property) and `PR_SET_DUMPABLE 0` (the kernel
        already makes a set-id exec non-dumpable at `fs.suid_dumpable` 0, measured on this host,
        and at 2 the difference is only a root-owned core file; not observable without crashing
        the helper). Neither is counted killed.
      - (2) `CustodianRef` gains `sha256` (the program pin). The helper config requires it in
        production (`load_config`; a test config may omit it); every test fixture pins the built
        `axon-custodian`. With a pin, `CustodianRef::call` sets `SO_PASSPIDFD` before sending the
        request, reads the reply with `recvmsg`, and for EVERY message takes the kernel's
        `SCM_PIDFD` of its sender; `check_sender_program` reads the pid from the pidfd's fdinfo,
        opens `/proc/<pid>/exe`, asks the pidfd again (alive then means alive at the open, so the
        pid was not recycled), refuses an executable not owned by root (or this uid) or writable
        by group/other, and hashes the OPEN descriptor against the pin. This is the source: the
        one call every issue and spend goes through. It is checked where the nonce is SPENT (the
        helper's config); Fabric's own issue call carries no pin (`protected_host.rs`), and
        `helper_agrees` now compares the custodian's socket and uid only (M636 re-anchored, same
        guard). A kernel without `SO_PASSPIDFD` (Linux < 6.5) refuses every pinned call.
      - (3) `scripts/b263_host.py` measures the identity (hostname, `/etc/machine-id`,
        `systemd-detect-virt`, kernel), prefixed by the operator's `--host-label`; the caveat is
        the operator's `--caveat`, else derived from the measured virtualization.
        `b263_qualify.sh` takes both from it and records `host_facts.{hostname, machine_id, virt,
        wsl, host_label}` and `source.host_identity_sha256`. Readers: `accept_b263` requires a
        non-empty host and carries it into the qualification; `submit.rs` writes it into every
        protected receipt as evidence. Nothing compares it to the running host (A89: the host is
        joined through the qualification digest the host config pins); a host-binding rule
        (the record's machine-id against the running host's) is a possible follow-up, not a
        current counting rule.
      - (4) `guest_build_env.toolchain_pin_problems`: the pin must be an operator file (the file
        and every directory above it real, root-owned, not group/other-writable); every host tool
        the image's build records name (kernel tools, cargo's linker, mksquashfs, rustc, cargo:
        `recorded_host_tools`, the kit's own extraction) must be pinned at the same path and
        digest, and every pinned tool recorded. `image_problems` applies it last. ABSENT pin:
        the FREEZE refuses (`image_problems(img, pin_required=True)`: a freeze binds evidence for
        certification, and without the operator's pin the toolchain is attested only by the
        build's own record); in development (`guest_build_env.py toolchain-pin MANIFEST`) it is a
        WARNING. A pin that exists is always judged. The freeze tests now run the freeze in a
        private mount namespace whose `/etc/axon` holds only the test's pin (root required; the
        host's /etc is never read or written).
      - (5) `psv_guest_boot_test.sh` case `trust-probe`: `trust_root_guest_probe.sh` runs inside
        the REAL guest (the pinned image, kernel and launcher) in the launcher's plain mode under
        an Exec grant, put on the workspace drive and run by busybox sh: the operator trust root
        is unaddressable; control: `/work` is addressable (a mount). Not through the PSV runner,
        which strips Exec from every test child (A27: measured, a suite test's `exec` is refused
        `requires effect Exec`, so case `reach` is refused by that ceiling, not by provenance). No
        guest change.
      - (6) The helper refuses, in EVERY build, when its own executable is setuid-root but its euid
        is not 0, naming NoNewPrivileges (`PR_GET_NO_NEW_PRIVS`) or else nosuid/user namespace
        (`setuid_honoured`). `--probe` reports `no_new_privs` and `cgroup`. The preflight takes
        `--fabric-pid PID` (REQUIRED in protected mode) and FAILS unless `/proc/PID/status` shows
        the Fabric uid and `NoNewPrivs: 0`. No row: on the production route the euid rule (M602)
        refuses the same launch, and on a test-trust route the operator-file owner rule; this is a
        diagnostic (refusal-site gate exemption).
      - **Cgroup decision (memo condition C3): the root launch stays in Fabric's cgroup, by
        design, and is recorded.** The VMM is NOT in it: the jailer places firecracker in its own
        cgroup (`/sys/fs/cgroup/<parent>/<id>`, with memory.max, pids.max and cpu.max set per
        launch by `fc_linux_profile.sh`). What inherits Fabric's limits is the helper, the pinned
        bash launcher and the host tools it runs before the jail. A Fabric OOM, TasksMax or stop
        that kills them mid-launch ends the launch without a report (or with exit 31): it is a
        failure/unknown attempt under the strict counting rules, never a protected verdict (the
        out dir stays root's until the hand-over, and the verdict needs the helper's report);
        `fc_linux_profile.sh --reap ID` cleans what it left. The operator sizes Fabric's
        MemoryMax/TasksMax for helper + launcher + tools; the probe's `cgroup` field shows where
        the helper runs. Moving the root side to its own cgroup is the D2 (per-connection socket
        service) follow-up of the setuid-vs-daemon memo, not done here.
    - **Rows** (all PSV; M1470-M1486; M1487-M1499 unused; the assigned M1450-M1469 are
      integrate-3's, amendment 64):
      - M1474-M1482 (`harden()`): ignored signals reset (M1474), mask cleared (M1475), umask
        (M1476), cwd (M1477), inherited descriptors (M1478), RLIMIT_CORE 0 (M1479), lowered
        CPU/FSIZE/DATA/AS/NPROC reset (M1480), SIGPIPE ignored (M1481), NOFILE (M1482).
      - M1483: the reply's sender executes the pinned program (attack: an impostor on the socket,
        test-trust route). M1484: an executable another uid can rewrite is refused. M1485: a
        production helper config must pin the custodian (production route, the setuid production
        helper in a private /etc/axon).
      - M1486: the B263 identity is measured (attack: the constant).
      - M1470-M1473: the freeze binds only the operator-pinned tools (another rustc; an unpinned
        recorded tool; no pin; a pin matching a tampered build but owned by another uid or
        writable).
      - M636 re-anchored to the socket/uid comparison (same guard, same test).
    - **Matrix.** A93 (the custodian program).
    - **Tests.** `privileged_launcher.rs`: five new tests (harden; impostor custodian; rewritable
      custodian executable; production pin required + production impostor + control; production
      helper under `--no-new-privs`). `qualification.rs`:
      `the_b263_record_states_the_host_it_ran_on`. `freeze_manifest.rs`:
      `a_guest_image_not_built_with_the_operators_pinned_tools_does_not_freeze`.
      `test_trust_root_preflight.sh`: a Fabric process under NoNewPrivileges FAILS the new check;
      control passes. `psv_guest_boot_test.sh`: case `trust-probe`.
    - **Operator deployment (kit changes, c9r4b/opkit).** `protected-launcher.json` gains
      `custodian.sha256` = the sha256 of the INSTALLED `axon-custodian` (`gen_configs`;
      `protected-launcher.json.example`), and the helper refuses a production config without it;
      run `trust_root_preflight.sh` with `--fabric-pid $(systemctl show -p MainPID --value
      <fabric unit>)`; run `b263_qualify.sh --host-label <operator name> [--caveat <text>]`;
      the toolchain pin's status line ("no reader") is now false: the freeze reads it, and refuses
      without it. The Fabric unit must not set `NoNewPrivileges=yes` (or a preset that implies it).

66. **harden()'s environment clear is an ACTIVE row on the route where it is the only guard (C9
    round 4b, final; supersedes amendment 65's "dominated" exemption for the environment clear).**
    No counting rule is relaxed.
    - **Before.** Amendment 65 gave `privileged_launcher::harden()`'s environment clear no row,
      as dominated: the launcher's environment is built from nothing by `sealed_exec::command`'s
      explicit envp (M228) and "no code on the helper's path reads a variable". The second half
      is false. The helper's OWN Rust runtime reads `RUST_BACKTRACE` when it panics, and the
      caller can make it panic after `harden()`: `--probe` writes its report with `println!`,
      SIGPIPE is ignored (M1481), so a report pipe whose read end the caller has already closed
      fails EPIPE and the write panics. Measured with the clear removed: the setuid-root helper,
      executed by the Fabric uid with `RUST_BACKTRACE=full`, printed its full stack to the
      caller's stderr, every frame with its address (the helper binary's and libc's): the root
      process's address layout handed to its unprivileged caller. `sealed_exec` is not on that
      route, so the clear is the only guard there and is not dominated.
    - **After.** Row M1487 (`std::env::remove_var(k)` in `harden()` removed): attack
      `the_root_helpers_address_layout_never_reaches_its_caller` (`privileged_launcher.rs`, root
      only): the installed setuid-root (test-trust) helper is executed by python3 as the Fabric
      uid with `RUST_BACKTRACE=full` and `--probe` on a closed pipe. Setup asserts the panic
      happened ("failed printing to stdout"); ATTACK: no `stack backtrace`, no address in the
      caller's stderr. Control: the same caller on an open pipe reads the probe's JSON, exit 0.
      Killed by its own attack. The end-to-end environment assertion in
      `a_callers_process_state_never_reaches_the_root_helper_or_its_launcher` stays (M228 is its
      row). `PR_SET_DUMPABLE 0` remains amendment 65's measured exemption, not counted killed.
    - **Setup race found by the final paired-disable (fixed).** The first version of the attack's
      caller closed the report pipe's read end in the parent AFTER the fork; under load the
      helper's write could land in the pipe first and the probe exited 0 (the setup assertion
      failed: 11 of 40 runs under 40 CPU spinners; the clean axon-fabric baseline of one shard read
      BASELINE_BROKEN). The read end is now closed before the fork and a one-byte write confirms
      no reader exists before the helper starts: 0 of 60 under the same load, 0 of 40 idle; with
      the clear removed the attack succeeded 15 of 15 under load.
    - **Harness defect found by the final paired-disable (fixed at the source).** At d39ab3ad the
      CLEAN axon-core baseline failed `harness_binaries.rs::every_script_spawn_in_the_workspace_goes_through_the_helper`:
      amendment 65's freeze tests ran `v022_freeze_manifest.py` in a private mount namespace by
      taking the spawn helper's command apart (`inner.get_program()` / `get_args()` handed to
      `unshare`), and `the_b263_record_states_the_host_it_ran_on` ran `b263_host.py` with a bare
      `python3`. The gaps workstream ran only axon-fabric's suite. Every paired-disable record whose
      full-suite set includes axon-core would read BASELINE_BROKEN. Fix: the helper owns the
      wrapped form, `script_spawn::script_under(wrapper, interpreter, script, bins)` (the checked,
      stripped command of `script()` with the wrapper's argv in front; a wrapper naming a
      repository script is refused); both sites go through it / `script()`. The drift gate is
      unchanged (no call site is whitelisted). New test
      `a_wrapped_script_is_checked_and_stripped_like_any_other` (ATTACK: a guessing script under a
      wrapper is refused and never runs; every binary-naming variable is removed; control: an
      honest wrapped script runs with exactly the named binary), row M1488.
    - **M602's guard set (found by the final paired-disable at 78832d1b, BAD M602).** Amendment 65
      made every production helper verify the custodian PROGRAM on each reply (M1485 requires the
      pin; M1483 is the comparison). A helper whose euid is not 0 cannot open the custodian's
      `/proc/<pid>/exe` (another uid's process; ptrace read access), so with M602 removed the
      launch is refused by the pin verification before the custodian's spend rule (M628) is
      reached: the retired_off and set_off cells read OTHER_FAILURE and the full-suite cell
      failed `a_production_helper_that_is_not_root_launches_nothing` (its reason assertion named
      only the euid and M628 refusals). Not a false retirement: no route reaches M602 alone. The
      set is now {M628, M1489}; M1489 (new, ACTIVE) removes the verification as a whole
      (`check_sender_program` returns the pid unchecked) and is killed by the impostor attack;
      the test accepts the pin verification's refusal as the third reason. Executed by hand at
      the fix: base / M602 off / M628+M1489 off refused, M602+M628+M1489 off ATTACK_SUCCEEDS;
      the whole axon-fabric suite with M602 removed rc 0.
    - **Paired-disable suite bound (found by the same run, BAD M214 and M89).** Their four cells
      held; the full-suite cell read SUITE_BROKEN with no failure printed: the whole
      `cargo test -p axon-fabric` was cut by `bounded_run 12G 2400` (completed binaries summed to
      ~2040-2060 s, psv_dispatch 925-1319 s under the 6-shard load against 466 s idle; the cut
      landed 33-38 tests into a 40-test readiness binary still making progress). Not a hang. The
      bound is now 7200 s (`SUITE_BOUND_S`), and a suite cut by its time or memory bound is
      recorded as `<TIMEOUT|RESOURCE_EXHAUSTED: ...>` in the failures, never as an unnamed break.
    - **Rows.** M1487, M1488, M1489 (PSV; from the gaps assignment M1450-M1499, unused
      M1490-M1499 remain).
    - **Matrix.** None (an existing A of amendment 65, harden(); the harness's EQUIVALENCE (6)).
    - **Operator deployment.** None.
67. **A paired-disable record's consumers are selected from the build graph and the tree's text,
    and the record names every consumer it ran and skipped (C9 round 4b, pdfast).** No counting
    rule is relaxed; the four cells, the owner's and row package's full suites, kept outputs, skip
    accounting, ambient-binary scrubbing, interpreter restore, registry/edit digests, the currency
    rule and --shard/--join are unchanged.
    - **Before.** The full-suite cell ran, besides the owner's and row package's suites, the
      owner's DIRECT reverse dependencies and (for axon-core only) every crate whose sources name
      `AXON_BIN` -- a hand-shaped rule, and an incomplete one. Measured against the build graph and
      the tree at 78832d1b it missed: transitive linkers (axon-loop, axon-loop-contracts,
      axon-reflex and cortex-policy-adapter link axon-psv, whose runner execs the interpreter, so
      they observe every axon-core edit); test targets that read another crate's source tree as
      data (axon-core `refusal_coverage_gate` reads crates/axon-{fabric,loop,loop-contracts,psv}/src;
      axon-loop-contracts `redteam` reads crates/axon-loop/schemas); and test targets that list
      the workspace root or crates/ (axon-core `harness_binaries`, `cli_run`, axon-ledger
      `authority_reachability`, cortex `skills`, the `script_spawn` freshness walk included by
      core/cortex/intent/os/psv tests, the guest-image script run by guest-init `b263`).
    - **After.** `scripts/v022_pd_consumers.py` (rule `build-graph/1`, its docstring holds the
      rule and the proof): a consumer unit -- a package's crate-wide unit, or one `tests/*.rs`
      target -- is relevant to the retired guard's file F (in package X) by L (X in its
      link/dev/build closure), B (a build script it builds re-runs on F, a computed re-run path
      counting as the whole tree), D (its text, with its linked libraries' sources and the scripts
      it names, names a path in X or F, or lists the root or crates/), or E (it execs a binary
      built from the tree -- CARGO_BIN_EXE, a target-dir path, a `_BIN` variable, a cargo
      command line, or a bare name in a function that locates programs -- of a package relevant
      by L/B/D/E, a least fixed point). Relevant crate-wide unit: whole suite; else exactly the
      relevant targets; else SKIPPED with the reason. Proof: a skipped unit's code, the programs
      it spawns, the bytes it reads and the build that made them are byte-identical with and
      without edit A; the only input every edit changes for every unit is the tree-state bit, the
      same for every record, which cannot witness THIS guard. Doubt includes.
    - **Record.** Each EQUIVALENT_DID record carries `consumer_selection` (rule, mutated file and
      crate, `run` {package: scope, reason}, `skipped` {package: reason}),
      `matrix.consumer_suites` {package: state, scope}, `matrix.clean_baselines` (computed or
      reused, from which cache file) and `timing_seconds`; a STALE record carries
      `consumer_selection.not_applicable`. --join and --check-stale refuse a record without it,
      a skipped consumer without a reason, a selection that is not the rule's at this commit (a
      consumer the graph reaches skipped), and a passing full-suite cell that did not run exactly
      its selected consumers.
    - **Speed.** The retired-guard cell and the full-suite cell judge the same edit A, which is now
      applied and built once. Clean baselines are computed once per (commit, package, flags,
      environment, interpreter digest) and persisted (`<target>/v022-pd-baselines/<commit>.json`,
      or `--baseline-cache=DIR` shared by the shard clones of one commit); a reused verdict is
      used only after this target dir builds the suite from the clean tree; never across commits.
    - **Measured consequence (before the narrowing below).** The sound selection removes NO consumer the previous rule ran (148
      records); it adds about 20 test targets to every non-core record and ten whole suites to the
      three axon-core records. Clean-tree run time of the added targets (this host, under the
      concurrent final run): axon-core six targets 1094 s, axon-psv three 713 s, axon-cortex four
      246 s, axon-os four 13 s, intent/ledger/guest-init/loop-contracts under 10 s each. A full
      run is therefore SLOWER than before, not faster: the previous consumer set under-tested.
    - **Defect found end to end (fixed).** M400's first run (f6c20335) failed CLOSED on the
      interpreter check: axon-core's consumer targets, built in their default (codegen)
      configuration, wrote `debug/axon`, and cargo does not re-copy the fresh
      `--no-default-features` build over it. A package whose binary the run pins (the
      prerequisite commands, one list) now runs as a consumer in that pinned configuration; each
      consumer's cargo command is recorded in `matrix.consumer_suites`.
    - **Base derivation (narrowing, provable).** The walker clause made `script_spawn`'s
      freshness walk a whole-tree listing, selecting every unit that includes it. A computed
      listing base is now resolved by how it is derived (scripts/v022_pd_consumers.py,
      `listing_base`): a function whose only base is crates/ joined with a PARAMETER, whose
      worklist grows only from the `path = "..."` entries of the Cargo.toml files it reads, and
      whose root value is joined only to crates/ or files or handed to a same-file function that
      only anchors a listing at it (`git -C root ls-files -- dir`, rebasing that output), lists
      exactly the manifest closure (every dependency edge) of the package passed in. Proof: its
      worklist starts at one package directory and grows only by that package's manifest path
      dependencies; nothing else is listed. The package passed in is one the unit names as a
      literal cargo `-p` (the only place a unit takes a package from: a listing that chose one
      would itself be a whole-tree listing, an environment value is doubt); the unit is relevant
      iff X is in the manifest closure of such a package. A `-p` followed by an expression, no
      `-p` at all, crates/ or the root listed, a relative root literal: whole tree (selection-suite
      controls for each). Second narrowing, same discipline: a `<NAME>_BIN` in a `const`/`static`
      `&[&str]` list never used in a function that reads the environment is a variable NAME (the
      list `script_spawn` strips from a child), not a binary the unit locates; a list read from
      the environment stays an exec (control). Effect: axon-psv's three targets, axon-os's four,
      axon-intent's and axon-cortex's `check_executor`/`cli` leave every non-core record (they
      reach none of loop, loop-contracts, fabric); nothing a record ran under the previous
      harness is removed (148 records).
    - **Measured, M400 end to end (fabric-owned), host load 2-8.** Cells identical in every run to
      the previous harness's record (baseline / retired-off / sibling-off REFUSED, set-off
      SUCCEEDS, full suite SUITE_OK), interpreter restored. Per-step seconds:

      | step | 1466b89a cold | 1466b89a warm cache | narrowed cold |
      |---|---|---|---|
      | clean baselines (all suites) | 1313 | 4 (reused) | 975 |
      | own fabric suite, A applied | 231 | 209 | 207 |
      | axon-core 6 targets | 785 | 761 | 680 |
      | axon-psv 3 targets | 203 | 185 | -- |
      | axon-cortex targets | 110 (4) | 97 (4) | 33 (2) |
      | os / intent / ledger / guest-init | 15 | 14 | 1 |
      | cells (baseline, set-off, sibling-off) | 24 | 23 | 24 |
      | record | 2691 | 1301 | 1928 |

      Steady state (baselines reused) for a fabric record: about 960 s under the narrowed rule,
      against about 230 s for the same steps under the previous selection.
    - **Estimate.** Per record the sound selection adds about 12 min (axon-core's targets
      dominate: `cli_run`, `harness_integrity`) over the previous harness, after the narrowing
      (about 18.5 min before it). For 148 records on 6 shards sharing `--baseline-cache`: about
      +5 h over the previous run's ~8.5 h, i.e. about 13-14 h wall at pd6's load. The remaining
      whole-tree units are genuine readers (refusal gate, harness_binaries scanning every spawn
      site, ledger authority_reachability, cli_run's corpus and crate-list walks, the guest-image
      script) or doubt, and stay.
    - **Hosts (amendment 67, second part).** Every record's `environment.host` names hostname,
      kernel, cores, memory and the toolchain (`rustc -vV`, `cargo -V`, system LLVM). --join
      refuses a record without it and records from different toolchains (row M1505); shards from
      several hosts with one toolchain join, and the joined file lists the hosts.
    - **Rows.** M1500 (--join's selection check), M1501 (a skipped consumer needs its reason),
      M1502 (a reachable consumer is never skipped), M1503 (a passing cell ran its selected
      consumers), M1504 (currency: a selection no longer the rule's is stale), M1505 (--join
      refuses two toolchains; attack `a_join_refuses_records_from_two_toolchains`); attacks
      `a_join_refuses_a_record_without_a_consumer_selection`,
      `a_join_refuses_a_skipped_consumer_without_a_reason`,
      `a_join_refuses_a_record_that_skips_a_reachable_consumer` (control: the rule's selection
      joins), `a_join_refuses_a_passing_cell_that_ran_none_of_its_consumers`,
      `a_kept_record_is_stale_once_a_new_consumer_reaches_it` (harness_integrity). All six
      KILLED by their own attack. The rule's clauses are judged by
      `scripts/test_v022_paired_disable_selection.py` (synthetic workspaces, one ATTACK or
      CONTROL per inclusion case and a 200-graph property); each of its 18 clauses removed fails
      that suite, as does each base-derivation and names-only clause. Unused M1506-M1519.
    - **Matrix.** None. **Operator deployment.** None. Every existing paired-disable record is
      stale under this amendment (it carries no selection) and is re-executed.

68. **The observer is a SERVICE with its own uid and key, reached only through the root helper's
    relay; it signs only what it measured, once per nonce (operator decisions G and G1 = A,
    2026-10-04; C9 round 4b, observer workstream; matrix A94).** (Amendment 67 is reserved by the
    pdfast branch.) No counting rule is relaxed.
    - **Before.** `observer::observe` executed the operator-pinned observer PROGRAM (`--manifest
      FILE --out DIR`) through `sealed_exec`, AS THE FABRIC UID, and verified the
      `observation.json` + `.sig` it wrote. Whatever that program measured, its signing key had to
      be readable by the uid that ran it, i.e. by Fabric: Fabric could sign any observation itself.
      ADR-002's "Fabric holds no observer key" held only for the key ROOT (the public halves), not
      for the private key (runbook step 0, "recorded follow-up"; §5's follow-up "the observer runs
      as Fabric's UID"). The repository shipped no measuring observer: the stand-in copied the
      manifest's facts.
    - **Decision G1 = A, and the departure from G's wording.** Decision G asked for an observer
      service with "an SO_PEERCRED caller check (only the Fabric uid)" and a client-side program
      pin mirroring M1489. The pin cannot be checked by Fabric: Fabric is non-root (decision A) and
      the kernel denies `/proc/<pid>/exe` across uids without `CAP_SYS_PTRACE` (measured on this
      host: uid 4302 opening uid 4301's `/proc/<pid>/exe` → EACCES), so a fail-closed check would
      refuse every launch; `CAP_SYS_PTRACE` for Fabric was rejected (it would let Fabric read the
      observer's memory, i.e. its key). G1 = A: the setuid-root helper relays. The observer's
      caller rule is therefore **uid 0 (the helper), not the Fabric uid**. That is the realisation
      of G's "own boundary", not a weakening: the helper admits only the Fabric uid (M531, the same
      gate as a launch), and between Fabric and the observer it adds what Fabric could not do —
      the per-reply program check (M1489's function) and a measurement of the RUNNING Fabric.
      Fabric cannot connect to the observer at all (socket `0600 root:root`).
    - **The service (`axon-observer`, a bin of `axon-fabric`; `observer_service.rs`).** Custodian
      shape (amendment 50): its own system uid, socket-activated (`custodian::activated_listener`:
      an activation at another path or none is refused), config only at `/etc/axon/observer.json`
      (`axon-observer/1`: `observer_uid`, `fabric_uid`, `caller_uid`, `socket`, `store`,
      `key_path`), read by `privileged_launcher::read_operator_file` from `/`. A protected config
      must name three principals (`observer_uid` ≠ `fabric_uid`, neither 0; M1520) and
      `caller_uid` 0 (M1521); `test_paths` is a test-config key. It runs only as `observer_uid`
      (M1526). Modes as for the custodian, stated in every reply: `protected`; `test`
      (`--test-config`, test-trust builds only, M1548); `dev` (`--dev --test-config`, test-trust
      only), which the helper never relays.
      - **The key** (`load_key`, ONE `O_NOFOLLOW` open): a regular file owned by the observer's
        euid (M1523), mode 0400 (`& 0o277 == 0`, M1522). So no other non-root uid can read it,
        the Fabric's included. A POSIX ACL granting another uid read needs a mask granting it, and
        the mask IS the group bits of `st_mode` (POSIX.1e), so an ACL grant is refused by the same
        check (tested on ext4 with an `system.posix_acl_access` naming the Fabric uid); no separate
        xattr rule is added (it would be an equivalent mutant). Its public half must be in the
        operator observer root and in no other root (`key_in_root`, `exclusive_root_keys`; M1524).
      - **The store**: its own 0700 directory (`custodian::check_store`, M1525), whose parent
        chain is the operator's. One record per nonce, created `create_new` BEFORE signing
        (M1540); the nonce must be 32 lowercase hex, or it would name a path (M1530).
      - **The request** (`axon-observer-request/1`: `manifest`, `caller_sha256`; fixed schema,
        M1528) is answered only for `caller_uid` by `SO_PEERCRED` (M1527). The manifest must pass
        `LaunchManifest::verify` against its own digest (canonical, `/2`, protected profile,
        completion scheme; M1529); the service computes `intended_launch_manifest_sha256` itself.
    - **What the service MEASURES vs what it is TOLD (the table below is AMENDED by amendment 79:
      the Fabric revision, the verifier digest and the init and axon digests are no longer told).**
      It refuses to sign unless every measured
      value EQUALS the manifest's claim (M1549; one row per measured field, M1531-M1539, each
      replacing that measurement by the claim). Measured from operator files (the host config
      `/etc/axon/protected-host.json` and the helper config `/etc/axon/protected-launcher.json`,
      both read through the operator walk, kit-installed root 0644), never from the request:

      | observation / manifest field | source | class |
      |---|---|---|
      | `host_config_sha256` | sha256 of `/etc/axon/protected-host.json` | MEASURED (M1531) |
      | `launcher_sha256` | the host config's `launcher.path` | MEASURED (M1532) |
      | `firecracker_sha256` | the helper config's `firecracker` | MEASURED (M1533) |
      | `guest.kernel_sha256`, `guest.rootfs_sha256` | `vmlinux`, `rootfs.sqfs` in the host config's `artifacts_dir` (streamed) | MEASURED (M1534, M1535) |
      | `suite_registry_sha256` | the host config's `suite_registry.path` | MEASURED (M1536) |
      | `qualification_sha256` (manifest only) | the host config's `qualification.record` | MEASURED (M1537) |
      | `profile_manifest_sha256` (manifest only) | the host config's `profile_manifest.path` | MEASURED (M1538) |
      | `verifier_sha256` | the RUNNING Fabric's executable, hashed by the root helper from its parent's pidfd | MEASURED by root (M1539, M1544) |
      | `observed_at` | the service's clock | MEASURED |
      | `intended_launch_manifest_sha256` | sha256 of the bytes received | DERIVED over told bytes |
      | `host_profile` | the manifest's, required protected (M1529) | CONSTANT |
      | `observer_key_id` | the service's own key | OWN |
      | `guest.init_sha256`, `guest.axon_sha256` | the manifest's (both live INSIDE the measured `rootfs.sqfs`) | TOLD, covered by the measured rootfs bytes |
      | `fabric_revision` | the manifest's (`env!` in the Fabric binary) | TOLD, determined by the measured running binary |
      | `policy_sha256` | the manifest's (built in Fabric's private 0700 job dir) | TOLD |
      | `nonce`, `epoch` | the manifest's | TOLD; nonce bound once here and spent once at the root (custodian); epoch joined at the spend and by the loop |

    - **Strength against today.** Against the Fabric uid: today ZERO (Fabric could hold the key);
      after, STRONGER for every MEASURED row (a principal Fabric cannot impersonate read those
      installed bytes and found the manifest's digests), and for `verifier_sha256` stronger than
      the design draft (the RUNNING Fabric, not the installed file). EQUAL to today for every TOLD
      row (Fabric's word), except that an observation is made at most once per nonce. An
      observation is a measurement at one instant; the window to the launch is bounded by
      `max_age_s` and by the helper's own pins of launcher, firecracker and profile manifest.
    - **The helper's `--observe` (setuid-root; `privileged_launcher::serve_observe`).** The same
      entry: `harden()` first, the same `setuid_honoured`/euid rules, the same bounded stdin read.
      `--observe` leads and leaves the rest of the command line to the launch's own parse (so the
      `--test-config` rule, M601, is the same code). Then: `authenticated()` — the ONE caller gate
      now shared by a launch and a relay (load the operator config; the caller's REAL uid must be
      `fabric_uid`, M531; become root in every id); `axon-protected-observe-request/1` (`schema`,
      `manifest`; unknown fields denied; fixed schema, M1545); `running_caller`: `pidfd_open` of
      the parent, which must still be the parent (`getppid`) and alive (the pidfd's own pid) both
      before and after `/proc/<ppid>/status` and `/proc/<ppid>/exe` are read, with every uid
      (real, effective, saved, fs) the Fabric uid (M1544), then the open executable hashed; the
      observer named by the helper config's `observer.service {socket, uid, sha256}` (a production
      config: absolute plain socket, a sha256 pin, a uid neither the Fabric's nor 0, M1546);
      `ObserverRef::observe`: the listener's `SO_PEERCRED` uid must be the observer uid or root
      (M1541), `SO_PASSPIDFD` is set and every reply message's sender is checked by
      `custodian::check_sender_program` against the pin (`read_from_pinned`, now shared by the
      custodian and observer clients; M1542, and M1483/M1489 for its comparison and verification
      as a whole); finally the observer's mode must be protected (a test observer's only in a
      test-trust helper; `custodian_mode_launches`, M1543). Report
      `axon-protected-observe-report/1`; exit 0 with an observation, 30 without. Nothing is
      launched; the launch path is unchanged and still verifies the observation itself and spends
      the nonce (amendment 50).
    - **Fabric's client.** `ObserverConfig` gains `relay`. `ProtectedHost::load`: an `observer`
      section WITHOUT `command` is the relay route (through the host's pinned privileged helper);
      WITH `command`, a production build refuses the host config (M1547) and only a test-trust
      build runs the in-uid stand-in. `observer::observe` on the relay route executes the helper
      `--observe` from its verified descriptor, and verifies the relayed bytes with the unchanged
      `verify_observation` and epoch rule. `pinned_paths` lists `observer.command` only when
      present. DEV (ADR-001 D1): unchanged; a development Fabric has no host config and so no
      observer.
    - **Failure modes (all fail closed).** No socket, a refused connect, a timeout, an over-bound
      or malformed reply, `ok:false`, a dev observer, a sender that is not the pinned program, a
      listener of another uid, a parent that is not the Fabric uid or has exited (`pidfd_open`
      failing, or the pidfd/getppid re-check), an unreadable or unequal measurement, an existing
      nonce record, a failing signature/join/freshness check at Fabric: no observation, so nothing
      reaches the launch. Service side: a config, key, store, uid or activation check failing →
      exit 2, nothing served.
    - **Rows** (PSV; M1520-M1549, all ACTIVE, each killed by its own attack; M1550-M1559 unused).
      Tests: `crates/axon-fabric/tests/observer_service.rs` (17, three root-only), two production-
      build tests in `privileged_launcher.rs`, and unit tests in `observer_service.rs` and
      `privileged_launcher.rs`. M531 (the shared caller gate) also has an observe-route attack
      (`an_observe_relay_for_a_caller_that_is_not_the_fabric_relays_nothing`); it stays one row.
      `read_from_pinned` gained the service name and a bound (the custodian passes "custodian" and
      `MAX_MESSAGE`: its messages are unchanged); the refusal-site gate's exemption anchor for the
      bound moved with it. New exemptions (operator-authored fields, OS errors, races that fail
      closed, re-reported refusals, named rows) are listed in `v022_refusal_coverage.py`.
    - **Matrix.** A94.
    - **Operator deployment (PROTECTED_ONLY; kit delta in
      `governance/notes/v022-observer-service-kit-delta.md`).** A system user `axon-observer`; the
      key generated by the operator AS that user (`/var/lib/axon-observer/key/observer.pk8`, 0400,
      directory 0700) and its public half at `/etc/axon/trust/observer/observer.pub`;
      `/etc/axon/observer.json` (root 0644; example `profiles/protected-host/observer.json.example`);
      the production `axon-observer` (root-owned, pinned); `axon-observer.socket` +
      `axon-observer.service` (examples in `profiles/protected-host/systemd/`); the helper
      config's `observer.service {socket, uid, sha256}`; the host config's `observer` section
      WITHOUT `command` (the observer program the runbook installed is no longer used); the
      preflight probes that the Fabric uid (and every agent) cannot read the key or connect to the
      observer socket.

69. **The axon-fabric suite's cost is its digests and its serial runner, not its waits; setup waits
    end on readiness and only fail (C9 round 4b, fabricfast; governance/notes/v022-fabric-suite-time.md).**
    No counting rule is relaxed; no bound that is the property under test is shortened.
    - **Before.** The axon-fabric suite, run as the harness runs it (`cargo test -q -p axon-fabric
      -- --test-threads=1 --show-output`), took 1440-1472 s idle at 16500980 (psv_dispatch 495-529 s)
      and passed 2400 s under the 6-shard paired-disable (amendment 66). Per-test timing shows no
      test above 40 s and no healthy-path timeout; the time is real work, most of it SHA-256 of
      ~35-80 MB debug binaries at opt-level 0 (1.03 s per 80 MB; the helper re-verifies every
      pinned artifact, the custodian client verifies the custodian program per reply, the fixtures
      pin every binary). Setup waits were short fixed polls: the observer hold point (20 s) failed
      the CLEAN baseline at load ~100 (gpumaster), the custodian start (10 s) returned the same
      `Err` a refusing custodian returns, the FIFO feeder (5 s) was paid by every honest run, and
      the copied-custodian test proceeded after 5 s without asserting the socket existed.
    - **After.** (ii) Every setup wait ends on its readiness signal OR on the fixture process
      exiting (reported at once with its reason), bounded by `SETUP_BOUND` (600 s), which only
      fails; a custodian that neither listens nor exits is a setup panic, never a refusal; the
      observer in `wait` mode makes no observation unless told to go; the FIFO feeder serves until
      the decision under test has returned. Sites and old/new bounds: the note, §3 (12 sites in
      axon-fabric, axon-cortex `cli.rs`, axon-core `cli_run.rs`); the timing-SLA bounds of axon-os,
      axon-vm and axon-intent are the properties and are unchanged. (ii-b) `[profile.dev.package.sha2]
      opt-level = 3`: the same digests ~30x faster; no workspace crate's codegen changes. (iii)
      `scripts/cargo_test_shards.py` runs one package's suite as concurrent shard PROCESSES (each
      `cargo test -p P <target> -- <the cell's libtest args> --exact <its tests>`, --test-threads=1
      kept inside), prints every shard's whole output in cargo's order, fails on any test lost (a
      shard must report `running <its count> tests`), runs every shard after a failure, and kills
      and names every unfinished shard on its bound's SIGTERM. `full_suite_ok` uses it for
      `SHARDED_PACKAGES` = {axon-fabric}, the package audited to share no state across processes
      (note §2). Two cross-process collisions were found and fixed at their primitive: readiness's
      fixed production-verifier build dir (an exclusive flock, per-process copies), and
      `script_spawn::workspace_bin`, whose shared target dir cargo relinks on every build of the
      interpreter in a git worktree (axon-core's build script watches `.git/HEAD`/`.git/index`
      paths that do not exist where `.git` is a file), so one process's build removed the binary
      another had just been handed (M482's sharded own-package baseline, "No such file or
      directory"); it now builds under an exclusive lock and hands each process its own copy.
      That race predates sharding wherever two harness cells share a target dir. Measured: serial 218 s
      idle; sharded 38 s idle, 61 s under 32 busy loops.
    - **With amendment 67 (integration, c9r4c/integrate).** The two changes compose at
      `full_suite_ok`, the one function every whole-suite cell and every clean baseline
      (`cached_baseline`) runs through. Amendment 67 decides WHICH suites a record's full-suite
      cell runs: the owner's and the row package's, plus each consumer the build graph and the
      tree's text select (`consumer_runs`: a whole suite, or exactly the `--test` targets the rule
      found relevant, `-- --test-threads=1` kept). This amendment decides HOW each of those runs:
      a cell whose package is in `SHARDED_PACKAGES` (axon-fabric), as the row's own package or as
      a selected consumer, whole-suite or target-scoped, runs through
      `scripts/cargo_test_shards.py` with the same cargo selectors and libtest args; every other
      package, and a combined owner+row cell (`-p A -p B`, which the shard runner refuses), runs
      under `cargo test` as before. Sharding never changes the selection and the selection never
      bypasses the runner: the record's `consumer_selection` and `consumer_suites` name exactly
      what ran, whichever runner ran it.
    - **Rows.** None new (M1560-M1579 unused). The rows whose attack lives in a changed test were
      re-run and each is KILLED by its own attack: M253, M482, M631, M632, M763, M767, M770, M771,
      M1483, M1484, M1489.
    - **Tests.** axon-core `harness_integrity`
      `a_sharded_suite_run_reports_every_test_as_a_serial_run_does` (ATTACK: a sharded run hides a
      failing test in a later binary, or loses a skipped test's output; control: without the
      failure every test passes once).
    - **Matrix.** None.
    - **Operator deployment.** None.

70. **A signed qualification states only measured host facts; a store key has one canonical form
    (C9 round 4b, smallfix; operator-kit review).** No counting rule is relaxed.
    - **Before.** (1) `b263_qualify.sh` recorded `x3_l0_hypervisor_boundary` BLOCKED with a
      constant reason, "Host is WSL2 with nested KVM under Hyper-V (operator decision D2) ...".
      That reason is inside the record the operator SIGNS, so a qualification run on bare metal or
      under another hypervisor asserted, under the operator's signature, a false fact about its
      host (amendment 65 derived `host` and `caveat` from the measurement but left this row's
      text constant). (2) A public key in the loop store's `verifier_keys` / `monitor_keys` /
      `observer_keys` was read two ways: `operator_trust::rooted` / `exclusive` lowercase the key
      they are given, while `attestation::verify` / `verify_document` / `key_id_of_hex` accept
      only lowercase hex (`unhex`). An uppercase store key therefore PASSED the operator-root
      lookup and then failed every verification. (3) `axon-fabric keygen`'s doc comment said the
      key is written 0600; it is written 0400, the only mode the signing-key loader accepts.
      (4) The `--authority` usage refusal listed four authorities and omitted `monitor`.
    - **After.** (1) `b263_host.py` separates measurement (`measure()`: hostname, machine-id,
      `systemd-detect-virt`, kernel, WSL) from derivation; `x3_reason(facts)` names WSL2/Hyper-V
      (and D2) only for a host measured as WSL, and otherwise states what was measured (bare
      metal: the host kernel's KVM; another hypervisor: nested under it; unmeasurable: unknown).
      `b263_qualify.sh` takes the row's reason from `b263_host.py --x3-reason` at pre-flight,
      before anything launches; a host that cannot be stated exits 2 (host-state error). The row
      stays BLOCKED on every host. (2) The canonical form is 64 LOWERCASE hex, as every other
      reader enforces: `attestation::unhex` (verify, verify_document, key_id_of_hex),
      `privileged_launcher::is_hex64` (`observer.host_signer_public_key`), the `Ref` digest
      (`ids.rs`, "digest must be 64 lowercase hex"). `Config::check_separation` — the one place the
      store reads every registered key, on `write_config` and on every `config()` read — now
      refuses a key not in that form (`attestation::is_canonical_public_key_hex`), naming the role
      and identity; it never normalises. (3) Comment corrected. (4) The usage text is derived from
      `TrustAuthority::ALL`.
    - **Rows.** M1580 (b263_qualify.sh records the constant again) and M1581 (`x3_reason`'s WSL
      branch taken for every host), attack `the_x3_reason_states_only_what_the_host_measured`
      (`qualification.rs`): the reason for bare-metal, kvm, vmware and unmeasured fact sets makes
      no WSL/Hyper-V/D2 claim and states its measurement; control: the WSL fact set names WSL2 and
      Hyper-V, and this host's reason through the production CLI agrees with its kernel. M1580 is
      judged by the harness's text (b263_qualify.sh needs root and KVM end to end); the reason it
      reads is the script's own output. M1582 (the canonical-form refusal disabled), attack
      `an_uppercase_public_key_is_refused_where_the_store_reads_it` (`evl_admission.rs`): for each
      role an uppercase key written behind the store's back is refused by `config()` (the attack
      reports that the operator-root lookup accepts it), and `write_config` refuses it; control:
      the lowercase key is written, read and rooted. The new refusal site is covered by M1582 in
      `v022_refusal_coverage.py`. The `--authority` usage refusal stays exempt (USAGE); its anchor
      moved with the text. Test `the_authority_usage_names_every_authority` (`verify_evidence.rs`).
    - **Matrix.** None (no A id assigned to this workstream).
    - **Operator deployment.** A store config holding an uppercase key now fails to load with a
      reason naming the role; re-register the key in lowercase (`axon-fabric keygen` prints
      lowercase). Nothing else.

71. **A run dir name never meets a crashed same-pid predecessor's leftover; the custodian program
    pin is stated at the strength it has; the SENTINEL harden() finding is recorded OPEN; and
    decision code on the protected path is in the refusal-site gate's scope by RULE, not by a
    directory list (C9 round 4c: r4c-fixes, parts 1 and 2, integrated on c9r4c/integrate).** No
    counting rule is relaxed. Part 1 (c9r4c/fixes) is the first three items; part 2
    (c9r4c/fixes-rc) is the refusal-site gate items that follow them.
    - **RunDir retry (SENTINEL MINOR).** *Before:* `submit.rs` `RunDir::new` created the run's
      dir NEW (`create_dir`, C9 round 4b) at `<op-hash16>-<pid>-<seq>`. After a crash, a
      restarted Fabric with the same pid (PID 1 in a container) met its predecessor's leftover
      at the same name and refused every retry of that operation until `seq` passed the
      leftovers (fails closed; availability only). *After:* the name carries 64 bits from the OS
      RNG (`ring::SystemRandom`): `<op-hash16>-<pid>-<seq>-<16 hex>`. `create_dir` is kept
      unchanged: it is the guard that nothing materialized into the run dir meets stale files,
      and any existing dir at the name (leftover or planted) is still refused, never reused.
      The random component makes that refusal a 2^-64 collision rather than a retry's fate, and
      no other uid can aim a planted dir at a name it cannot predict. No reaping was added: it
      would delete under `<state>/runs` on startup, and the leftovers are inert. The exemption
      that cites this property (refusal coverage, `StoreError::DestinationExists`, "a leftover
      run dir is refused, never reused") stays true. *Test:*
      `crates/axon-fabric/tests/check_effects.rs::a_leftover_run_dir_of_a_crashed_process_with_the_same_pid_does_not_refuse_the_retry`
      plants every name the crashed process could have left for this pid (seq 0..4096, each
      holding a stale file; setup asserts all 4096 exist), submits a retry through `submit`
      (ATTACK: the retry is refused; control: it runs and passes), and asserts no leftover was
      reused (each still holds exactly its stale file). *Row:* M1655 (removes the random
      component). *Matrix:* none (an availability property, not a negative verdict case).
    - **The custodian program pin, at its real strength (SENTINEL MAJOR-ADJACENT 2; amendment
      65(2), A93).** Amendment 65 stated the custodian as "the PROGRAM the operator pinned", read
      as "every byte of a reply comes from a process executing exactly that program". The
      mechanism (`custodian::check_sender_program`) establishes less, and this is the property
      it has: *for each message of a reply, the kernel names the sending process (`SCM_PIDFD`);
      the helper opens the file behind that process's `/proc/<pid>/exe` at the time it reads the
      message, re-checks the pidfd after the open, refuses a file another uid can write, and
      hashes that open file against the operator's pin.* It identifies the executable the sender
      was started from, as of the read. It does NOT cover the code mapped into that process
      (a library, an `LD_*`-loaded object, memory written through `/proc/<pid>/mem` or ptrace by
      a process with access to it), nor what the process executed when it sent. An actor able to
      exploit the difference must already run as the custodian uid (or root), which A93 treats as
      in scope; so against that actor the pin is a check on the custodian's started program, not
      on the author of the reply bytes. Not demonstrated in this round (it needs operator
      authorization). *Follow-up (recorded, not implemented):* reply signing: the custodian
      signs each reply with a key only the operator-installed custodian holds (readable by the
      custodian uid alone, provisioned by the operator kit), and the helper verifies the
      signature under an operator-pinned public key, binding the reply bytes to the key holder
      rather than to an executable path. A93's row should carry this residual.
    - **harden() inherited state (SENTINEL MAJOR-ADJACENT 1): OPEN, parked pending the user's
      decision; not yet implemented.** The finding stands as filed: `harden()` resets neither
      interval timers nor the caller's terminal session. Nothing below is implemented, and no row
      exists for it. The analysis done before the item was parked, recorded so the work can
      resume:
      - Fabric spawns the helper via `sealed_exec::command` in Fabric's own process group
        (`backend.rs`), reads its stdout to EOF and then waits; `setsid()` in `harden()` is
        compatible with that. `setsid()` fails when the caller made the helper a process-group or
        session leader; that case must refuse the launch (a flag set by `harden()`, checked
        beside `setuid_honoured` in the binary, keeps `harden()`'s signature for the
        observer-branch merge).
      - Not reset today, each to be reset to a stated value with its own row and attack: the
        interval timers ITIMER_REAL, ITIMER_VIRTUAL, ITIMER_PROF (POSIX timers are deleted by
        execve and need nothing); the controlling terminal / session (`setsid`); rlimits STACK,
        RSS, MEMLOCK, LOCKS, SIGPENDING, MSGQUEUE, NICE, RTPRIO, RTTIME (CORE, CPU, FSIZE, DATA,
        AS, NPROC, NOFILE are reset, M1479/M1480/M1482); nice; ioprio; oom_score_adj; scheduling
        policy; timer slack (`PR_SET_TIMERSLACK` 0 restores an inherited default, so an explicit
        value is needed); personality; `PR_SET_TSC` (x86).
      - Candidates to record as harmless or already covered, each with its checkable reason when
        the item resumes: pending signals (delivered at `harden()`'s unblock, before anything is
        read or launched); seccomp, Landlock, user/pid namespaces and ptrace (each needs
        NoNewPrivileges or makes the kernel ignore the set-id bit, so `setuid_honoured`
        refuses); cgroup (amendment 65); SIGIO via `F_SETOWN` (not permitted against the helper
        once `become_root` has run); keyrings (nothing on the launch path calls `request_key`);
        THP disable; MDWE; CPU affinity; root directory; securebits and the capability bounding
        set (each can only make a launch fail).
      - The existing harden() rows (M1474-M1482, M1487) are unchanged.
    - **Refusal-site gate: before.** `scripts/v022_refusal_coverage.py` scanned four hand-named
      `SCOPE_DIRS` (axon-fabric, axon-loop, axon-loop-contracts, axon-psv), conform.rs, and the
      interpreter's seal functions in two named files. The round-4c SENTINEL review found protected
      decision code outside it: axon-guest-init (PID 1 of the protected guest, 18 sites, 0 rows),
      axon-workspace-recipe (the one walker and path rule host and guest digest trees with, 11
      sites), the `axon test` entry (`cmd_test`, PSV-3) and the resolver's sealed-module refusal
      (E0004). evo.rs was OUT_OF_SCOPE with a reason the code contradicts: verified,
      `discovery_evidence_refs` is read nowhere outside evo.rs and its type (policy.rs only bounds
      its length; axon-reflex/shortlist.rs only builds one), `plan::check_candidate` re-judges the
      shortlist, parent and scope, never the evidence roles, and admission (admission.rs, the `want
      = Confirmation` loop) counts Confirmation trials for the same candidate. So evo::propose's
      role refusal is the ONLY B281 holdout guard on that path, and a proposal it should have
      refused WIDENS what counts. The SITE pattern also missed `Diagnostic::error(` (resolver),
      `process::exit(<non-zero>)` (guest PID 1, `axon test`, every CLI) and calls of file-local
      refusal constructors. (The reviewer's note that SITE misses conform.rs's `Err(mismatch(..))`
      arms is true of the SITE regex alone; `is_site`'s tail-`Err(` path does see them — measured,
      23 conform.rs sites, all rowed or exempt — but it did not see the `kind_ok(..)` calls of
      conform.rs's local refusal closure; it does now.)
    - **Refusal-site gate: after (the rule).** PROTECTED CRATES = `ROOT_CRATES` (axon-fabric,
      axon-loop, axon-loop-contracts, axon-psv) + every package `scripts/build-guest-image.sh`
      builds (`-p NAME`: axon-guest-init, axon-psv, axon-core, axon-guest-kernel) + the transitive
      closure of their NORMAL workspace dependencies, read from each Cargo.toml (`[dependencies]`
      and `[target.*.dependencies]` entries with a `path`; an OPTIONAL one only when a feature the
      build enables turns it on — default features for the host crates; dev- and build-dependencies
      are not linked into a shipped binary). Every non-test .rs under a protected crate's src/ is in
      scope, except (i) a dependency's own BINARY sources (src/main.rs, src/bin/**, `[[bin]]` paths,
      and modules only a binary root declares — axon-vm's chain/quorum): a dependency is linked as
      its library; (ii) modules lib.rs gates behind a feature the build leaves off (axon-cortex
      `ai`). The LANGUAGE crate (axon-core) and what it alone links (axon-certcheck, axon-domain,
      axon-gfx-mock, axon-surface) are scoped by function region: `SEAL_FN` =
      `(?<!un)seal|conform|cast` over every source compiled into the guest's `--no-default-features`
      build (`unseal` is the TEE builtin, not Protected Check Isolation), plus `CORE_ENTRY_FN` in
      main.rs (`cmd_test`, `read_completion_key`, `completion_token`, `failure_token`), plus the
      existing REGIONS and conform.rs. A new dependency of a protected crate, or a new package the
      guest builds, is in scope the day it appears. Measured at this commit: 12 whole crates
      (axon-attest, axon-audit, axon-cortex, axon-fabric, axon-guest-init, axon-guest-kernel,
      axon-loop, axon-loop-contracts, axon-os, axon-psv, axon-vm, axon-workspace-recipe), 5 language
      crates.
    - **Refusal-site gate: after (the forms).** A site is also a `Diagnostic::error(` (DIAG), a
      `process::exit(` whose argument is not the literal `0` (EXIT), a call of a file-local refusal
      closure (`let NAME = |..| .. Err(..)` on one line), and a call of a file-local DIVERGING
      refusal constructor (`fn NAME(..) -> !` or `let NAME = |..| -> !` whose first 8 lines exit
      with a non-zero literal: axon-custodian's `die`, the key reader's `fail`). The definitions
      themselves are not sites.
    - **Refusal-site gate: rows (PSV, M1630-M1651; each killed by its OWN attack on the production
      route).** evo::propose (the `evo propose` verb's function),
      `crates/axon-loop/tests/evo_b281.rs`: M1630 a Confirmation/Reporting-role episode never
      reaches the proposer; M1631 an episode of another scope never feeds it; M1632 no proposal from
      no eligible evidence. The REAL gate over a scratch tree (`refusal_coverage_gate.rs`, which now
      extracts HEAD's `crates/*/Cargo.toml`, `crates/*/src/**` and the guest build script with `git
      archive`): M1633 a crate a protected crate links is scanned; M1634 a package the guest builds
      is scanned; M1635 a non-zero exit and a compile refusal are sites; M1636 a call of a local
      refusal constructor is a site (each attack asserts the gate NAMES the planted site, so a
      mutant that stops scanning a form — and thereby orphans that form's exemptions — cannot pass
      for naming it; each control runs after its attack). The REAL axon-guest-init binary as root in
      a private mount namespace whose /proc/cmdline the test wrote
      (`crates/axon-guest-init/tests/policy_refusals.rs`): M1637 a refused policy starts no workload
      (the Refuse arm acts), M1638 possibly-truncated cmdline, M1639 repeated policy word, M1640
      constrains nothing, M1641 wrong schema, M1642 duplicate key (no last-wins), M1643 a seccomp
      filter that failed to apply is never followed by the workload, M1644 a program that is not
      whole instructions is never installed in part (nine bytes: an ALLOW instruction and a stray
      byte), M1645 a filter the kernel rejects is a refusal, not a skipped filter. The production
      workspace store (`crates/axon-fabric/tests/recipe_refusals.rs`, `WorkspaceStore::import_dir` /
      `materialize`): M1646 a stored version naming `../escape` never materializes (a version
      well-formed in every other way, planted in the store), M1647 a name with a control character
      (the manifest is line-based), M1648 an absolute symlink target, M1649 a non-UTF-8 name refuses
      the tree and is never silently dropped, M1650 likewise a special file, M1651 an import root
      that is a symlink is never followed.
    - **Refusal-site gate: exemptions (checkable facts, per site).** evo.rs: SELECTS NOTHING
      (duplicate eligible id; the caller's eligible set vs the registered list — the candidate is a
      remove/swap of the incumbent, which require_shortlist checks against the registered list),
      NAMED CHECK (>256 evidence: candidate.validate's check_array bound), NOTHING TO ADMIT
      (mutation space exhausted). Guest PID 1: OS ERROR (fork, execvp, PR_SET_NO_NEW_PRIVS), NOTHING
      TO ADMIT (empty value: zero bytes are refused as BadJson; the cmdline Err arm; MMDS returned
      nothing / failed), NAMED ROW (empty BPF: the kernel refuses len 0, M1645), SELECTS NOTHING (a
      non-object payload is still held to M1640/M1641), UNREACHABLE (a NUL in an argv string), RELAY
      (the supervisor's exit code is its child's), NOT ON THE PROTECTED ROUTE (MMDS: the protected
      VM has `"network-interfaces": []` and an empty network namespace). Recipe: NAMED ROW (an
      absolute path's first component is empty: the Traversal arm, M1646), RESOURCE BOUND (depth,
      byte and entry quotas). axon-core main.rs: NOT A SITE (`fail`'s body), NOTHING TO ADMIT (no
      key read; sources that do not parse), UNREACHABLE on the protected route (a short key: the
      runner writes a 32-byte HMAC as 64 hex), USAGE. Resolver: NAMED ROW (check_sealed's only emit;
      M79 removes the call). axon-custodian: NOT A SITE (`die`'s body), OS ERROR (bind), NOTHING TO
      ADMIT (load_config refused; no listener), DEVELOPMENT ROUTE (`--dev`). axon-fabric.rs: NOT A
      SITE (`refuse`'s body), NON-PRODUCTION (psv_host_guest is `#[cfg(feature =
      "test-trust-root")]`). Launcher: USAGE, RELAY (the report writer). axon-provenance:
      DEVELOPMENT ROUTE (`--descends` is the build's early check; the protected lineage is the
      snapshot's descends_from_protected, M505).
    - **NOT YET SCANNED (honest count; a freeze REFUSES while any is listed).** 118 sites in 18
      files: axon-cortex runner.rs 30 (incl. parse_axon_test_json, the verdict parser Fabric
      reads protected results with), lib.rs 7, generate.rs 6; axon-attest lib.rs 18; axon-audit
      lib.rs 10 (the interpreter's effect ledger); axon-os manifest.rs 9, coalition.rs 5,
      record.rs 5, approval.rs 4, ledger.rs 3, runtime.rs 3, profile.rs 1, replay.rs 1; axon-vm
      admit.rs 6, firecracker.rs 5; axon-core main.rs 3 (cmd_test's merge-error and type-error
      aborts and its pass/fail exit code, which psv.rs cross-checks against the verdict);
      axon-psv-runner 1 (`exit(3)` after start() refuses); axon-custodian 1 (the usage `die`,
      whose only possible anchor also lies in the guard block of a site M631 covers). With 25
      ids (M1630-M1654) the rows above went to the guards that carry PSV properties; these were
      not exempted in bulk. `python3 scripts/v022_refusal_coverage.py` holds (rc 0) and
      `--freeze` FAILS (rc 1) on exactly these 18 files.
    - **Integration with amendment 68 (c9r4c/integrate).** Part 2's EXIT and diverging-constructor
      forms reach `crates/axon-fabric/src/bin/axon-observer.rs`, the observer service binary
      amendment 68 added after part 2 was measured: 16 sites, 11 of them `die` calls no row or
      exemption named. Each now has one. Rowed (6): the uid check (M1526), the key and store
      checks (M1522-M1525), the `--test-config` gate (M1548), and the protected store's PARENT
      chain, which carried no row (the custodian's twin is M641) and is now **M1550** (id from
      the observer's unused M1550-M1559), killed by
      `privileged_launcher.rs::a_protected_observer_serves_only_from_a_store_the_operator_placed`:
      the production `axon-observer`, socket-activated as its own uid under a private
      `/etc/axon` with its 0400 key in the operator observer root and its own 0700 store, asked
      by root (ATTACK: it answers with the store under a Fabric-owned parent; control: it answers,
      in protected mode, with the store under an operator-owned parent). To give the load_config
      refusal and the chain check separate guard blocks, the chain check moved out of the
      protected match arm into `if mode == Mode::Protected { .. }` after the uid check: the same
      check in the same mode, before the key, the store and the listener. Exempt (10): `die`'s
      body (NOT A SITE), the five `test_config` closure refusals and the test/dev bind
      (NON-PRODUCTION: reached only through the `TEST_TRUST_BUILD`-gated arm, M1548),
      load_config refused and no activated listener (NOTHING TO ADMIT), and the usage arm
      (USAGE). The launcher's usage exemption anchor followed amendment 68's new usage text
      (`[--observe] [--probe]`).
    - **Rows.** M1655 (part 1; RunDir retry), M1630-M1651 (part 2; refusal-site gate) and M1550
      (integration; the observer's store-parent chain), all PSV. From the assignment
      M1600-M1659: M1600-M1629 were reserved for the parked harden() item and are unused;
      M1652-M1654 and M1656-M1659 are unused.
    - **Matrix.** None.
    - **Operator deployment.** None.

72. **At a seal crossing every position of the type a value is cast to is determined from the
    operator side, or the crossing is refused (C9 round 4c, workstream r4c-psv1, PSV-1; matrix
    A96-A105).**
    - **Before.** Review round 4c (findings `/var/tmp/c9r4c-findings-PSV-1.json`, cases
      `/var/tmp/c9r4c-psv1-logs/w/`) executed three BLOCKERS of one class to a keyed PASS for a
      wrong candidate, through the exact runner invocation and through `axon_psv::runner::run`:
      at a seal crossing a type position could be left UNDETERMINED, and an undetermined position
      admitted any value non-strictly, so the candidate chose the runtime type there and with it
      the operator's impl (a lenient `impl Judge for u8`). (B1) A generic `Chan<T>` parameter
      never bound `T`: `chan<i64>()` created an unstamped channel (its turbofish was no cast
      point), `type_of_value(Chan)` was `Chan<?>`, and the candidate's first send bound `T := u8`.
      (B2) A fn with no declared return type had no return cast (`return_type: None` → nothing),
      though the checker types its call `()`; the checker also accepted `.ok()` on that `()`.
      (B3) `field_cx` substituted an unbound outer type parameter with erase=true, so a
      `Wrap<T>` field was never checked, and a `Wrap<T>` argument never bound `T`. The
      MAJOR-ADJACENT: a sealed frame calling an operator closure cast its arguments
      non-strictly, so `apply<T>(f: fn(T) -> bool)` handed the operator's unannotated
      `|x| x.ok()` a `u8`. The MINOR: an impl for `f32`/`isize`/`usize` is keyed by the declared
      name but such values dispatch as `f64`/`i64`, so it never ran.
    - **The invariant.** At every value crossing of a seal edge — fn and method arguments and
      results (including an ABSENT return type), closure arguments and results, channel sends,
      struct and enum fields at any depth, collection elements — the value is cast against a
      type that is fully determined from the operator side, or, where a position is a type
      parameter (or a part of one) that no operator-side value determined, the crossing is
      refused. "Determined" has one representation: a binding records the parts the binding
      value did not show (an empty array's element, `None`'s payload, a generic struct's
      argument no field shows, a channel with no closed stamp, a closure's signature) as
      UNDETERMINED (`?undetermined`), distinct from `?` (a position whose type is not stated,
      e.g. an unannotated lambda parameter), and a strict cast refuses a value there exactly as
      it refuses one at an unbound type parameter (`cast_tparam`, M660).
    - **After (`interp/conform.rs`).** (1) `Interp::value_type` replaces `type_of_value`: what a
      type parameter binds to, with undetermined parts marked; a generic struct's or enum's
      type arguments are read off its fields (`generic_type`, `bind_from`); a channel yields its
      stamp; a handle yields `handle:<kind>`, which then admits only that kind of handle (it was
      `?`, admitting anything). A binding with undetermined parts is FILLED by a later non-strict
      (operator-side) value at the same parameter (`merge`), so `pick(None, Some(9))` binds
      `Option<i64>`. (2) `field_cx` binds each definition parameter to its argument AS WRITTEN,
      read in the caller's environment through a `parent` link (never erased); a missing
      argument is undetermined. Closure and channel element types are resolved through that
      chain (`resolve`) to the activation's own parameters, so a field binds the caller's `T`,
      or is refused at a strict crossing. (3) A bound parameter's binding is cast in
      `Cx::binding_cx()`, which carries the crossing's strictness (it was `Cx::default()`,
      dropping it). (4) Channels: `Interp::chan_created` stamps a channel at creation with the
      element type its `chan<T>()` states when that type is closed over known names
      (`parser::parse_type_text` reads the lowered `chan::<T>` name back), and records whether
      operator code created it. `stamp_chan` binds `T` at `Chan<T>` from the existing stamp
      (non-strict) and REFUSES an element type with a parameter nothing determined at a strict
      crossing (a candidate returning its own `Chan<u8>` at `Chan<T>`). `chan_send_check`: a
      sealed frame's send is a seal crossing; on an operator-created channel it needs an element
      type something on the operator side determined (`Chan::new(4)` or an operator generic
      `chan<T>()` handed to the candidate's free `U` is refused); determined element types are
      cast first, the rest may be bound by the (already pinned) value. A stale channel-table
      entry (address reuse) is replaced, never inherited. (5) `closure_args_check` takes
      `entering` (a sealed frame calling an OPERATOR closure): each argument must meet a position
      some contract in the chain determined, and is cast against those first, strictly; layers
      whose parameter is still free (e.g. the operator's own generic struct literal's
      `fn(T)`) are cast after, non-strictly — so an honest program whose operator annotation
      `|r: i64|` determines the argument still runs (`a_lambdas_annotated_parameter_is_cast`).
    - **After (`interp.rs`).** `call_fn_frame`: at a seal crossing a fn (or impl method) with no
      declared return type hands operator code `()` — the type the checker gave the call. Not a
      refusal: an honest unit fn whose body ends on a value keeps running; only what reaches the
      operator changes. Inside operator code, and between candidate fns, the behaviour is
      unchanged (an operator `@[test]` fn returning `Err` still does not complete, PSV-3).
    - **After (the checker, defence in depth).** `method_lookup_key` keys `Type::Unit` as `()`,
      so a method call on `()` with no impl for `()` is E0403. A new diagnostic E0505 refuses an
      impl for `f32`, `isize` or `usize` (the MINOR; no `.ax` in the repository had one).
      `AXON_REFERENCE.md` regenerated (143 codes, 130 live).
    - **Native codegen.** Unchanged, and this is stated rather than implied: the seal is
      interpreter-only. `--seal` is an option of `axon test` (the PSV runner's
      `exec_axon_test` path, which runs the tree-walking interpreter); `axon build` has no
      `--seal` (measured: `axon build b.ax --seal DIR` is a usage error), so no native binary
      ever runs sealed code and there is no native crossing to cast. Non-claim (4) of amendment
      53 stands.
    - **What is NOT claimed.** Non-claims (1)-(3) of amendment 53 as restated by amendment 60
      stand, narrowed (and non-claim (1) further by part 2): a closure that reaches the candidate with no contract (through a `Dict`)
      can no longer be CALLED by sealed code with arguments (A102). Still open by design: the
      candidate's own declared types (non-claim (2)); a value that meets no declared type on its
      way to operator code (non-claim (1)), e.g. an operator `dict_get` on a `Dict` the
      candidate filled at a key the operator never held (part 2: the rule for new keys);
      `host_await_val` (non-claim (3)). A generic impl whose methods name the
      impl's type parameter is refused by the checker (E0308 'unknown type T'), so that variant
      is unreachable.
    - **Variants hunted (each executed through the exact runner invocation; baseline at
      2a66eb0b → after).** Keyed PASS → refused: generic enum variant field, nested
      `Wrap<Wrap<T>>`, a generic struct through a plain `T`, `T` bound from an empty `[i64]`,
      a candidate `Chan<T>` return, a generic struct's `fn` field, an operator generic
      `chan<T>()`, `Chan::new(4)`, a closure in a dict, an impl method with no return type,
      `pick(None, ·)`. Already refused at baseline and still refused: tuple, array,
      `Option<T>` and `Result<T, E>` of a parameter. Honest controls (identity and relay
      programs over each shape) pass before and after.
      Pinned together in `a_type_parameter_inside_any_shape_is_never_filled_by_the_candidate`
      (tuple, array, `Option`, `Result`, each of those around a `Wrap<T>`, `Wrap<T> -> T`, two
      parameters, a returned closure, a generic impl's method).
      `host_await_val` crossings: unreachable inside a seal — a sealed `axon test` has no host
      driver, so the call returns its refusal (`a_host_await_crossing_is_unavailable_…`).
      Trait-bounded `T: Judge` is unreachable for a candidate (a sealed module cannot name an
      operator trait). **Was open, now closed by part 2 (below)**: a `Dict` the
      operator hands a sealed frame — the candidate overwrote an existing key with a `u8` and
      the operator's `x.ok()` on its untyped `dict_get` ran the `u8` impl (keyed pass). It was
      pinned as RECORDED by `an_untyped_dict_value_the_candidate_filled_is_a_recorded_open_position`,
      which part 2 replaces with refusal tests.
    - **Rows.** M1660 (creation stamp; attack: the candidate re-declares the operator's
      `chan<i64>()` as `Chan<u8>` through a dict hop, which the send-side rule alone accepts),
      M1661 (sealed send on an operator channel), M1662 (channel at a strict crossing), M1663
      (field type arguments not erased), M1664 (undetermined position at a strict crossing),
      M1665 (strictness through a binding), M1666 (no declared return type at a crossing), M1667
      (operator closure arguments from sealed code), M1668 (handle binding), M1669 (E0403 on
      `()`), M1670 (E0505). Re-anchored in place (same guard, same test, new text): M662,
      M1145, M1148, M1152, M1153. Every new refusal site in `conform.rs` has its row (refusal
      coverage).
    - **Tests.** Interpreter (`crates/axon-core/src/interp.rs`):
      `a_channel_carries_the_element_type_its_creation_states`,
      `a_sealed_send_on_an_operator_channel_needs_a_determined_element_type`,
      `a_channel_returned_at_an_undetermined_element_type_is_refused`,
      `a_generic_struct_or_enum_argument_binds_its_type_parameter`,
      `a_value_at_an_undetermined_position_never_crosses_a_seal`,
      `a_fn_with_no_declared_return_type_hands_the_operator_unit`,
      `an_operator_closure_called_from_sealed_code_takes_only_determined_arguments`,
      `a_handle_binding_admits_only_that_handle`,
      `a_type_parameter_inside_any_shape_is_never_filled_by_the_candidate`,
      `a_host_await_crossing_is_unavailable_inside_a_sealed_test_run`,
      `an_untyped_dict_value_the_candidate_filled_is_a_recorded_open_position` (REPLACED by part 2's
      tests). M1148's test now reaches the queued-value cast through a `Chan::new` channel
      (a stamped `chan<i64>()` refuses the send first). CLI (`crates/axon-core/tests/cli_run.rs`):
      `a_method_call_on_unit_without_an_impl_is_e0403`,
      `an_impl_for_a_type_the_runtime_represents_as_another_is_e0505`. Real runner
      (`crates/axon-psv/tests/sealed_frames.rs`):
      `an_undetermined_type_position_never_selects_the_operators_impl` (the review's four
      candidates: GOOD a keyed pass, WRONG a keyed failure, every attack refused unkeyed).
    - **Operator deployment.** The guest image must be REBUILT to carry the new interpreter;
      its scripts and runner are unchanged.

    - **Part 2 (same workstream, operator decision): the operator's dicts.** `Dict` has no element
      types, so for a dict the operator hands sealed code, the position of each value is
      determined by what the OPERATOR PUT THERE, and by nothing else.
    - **Rule.** (1) *Snapshot.* At every edge where operator code hands a value to sealed code —
      a candidate fn's arguments, a candidate closure's arguments, an operator closure's result
      returning into sealed code, a value the operator sends on a channel, an effect-handler arm's
      payload — `Interp::dict_edge_in` finds every dict reachable from the value (array and tuple
      elements, struct and enum fields at any depth, `Option`/`Result` payloads, channel queues,
      dict values; each dict once, cycles safe) and records the type (`value_type`, with
      `?undetermined` where the value does not show it) of each value it holds, and which keys
      hold an OPERATOR closure. (2) *Dirty.* `dict_set`, `dict_remove` and `dict_inc` by SEALED
      code mark the dict's snapshot dirty (the mutation primitives are those three; the dict is
      an `Rc`, so every alias — an array, a struct field, a candidate closure's capture — shares
      the one snapshot, which is how aliasing is covered). A mutation by OPERATOR code instead
      bumps `dict_epoch`: every snapshot from an older epoch is retaken at the next hand-over, so
      an operator that retypes its own dict between two calls is not blamed. (3) *Verify.* At
      every edge back to operator code — a candidate fn returns, a candidate closure returns,
      sealed code calls an operator closure, an effect-handler arm of operator provenance runs —
      `Interp::dict_edge_out` checks every dirty dict: each key the operator held that is still
      present must cast (round 5, amendment 78: STRICTLY, against the HELD VALUE, deeply — see 78) to what it held, and a
      key that held an operator closure may not now hold a candidate closure. A removed key is
      not checked (the operator then reads `None`); remove-then-re-add with another type is
      refused (the check is on the final state).
    - **Keys the candidate ADDS, and dicts the candidate BUILDS (decided, justified).** They are
      NOT constrained. Nothing operator-side determined those positions: the operator reads them
      as it reads any candidate output with no declared type, and the repository's own GOOD
      control (`SUITE_FNTABLE`: the candidate builds and returns a dict of closures that the
      operator reads) requires it. Refusing additions or non-empty candidate dicts would reject
      honest suites; the alternative of refusing the operator's untyped READ of an un-snapshotted
      key would reject every `dict_get` of candidate-built data. The invariant's wording for this
      position is therefore: a Dict value is determined from the operator side exactly where the
      operator put it; a value at a key the operator never held is candidate output, which a suite
      that dispatches on its type pins with an annotation (`let r: i64 = X`, cast). Stated, not
      implied: a candidate-built dict whose value the operator calls a method on UNPINNED still
      selects the operator's impl by the candidate's chosen type (the `f64`/`str`/width choice) —
      non-claim (1), unchanged for that position. **Superseded by amendment 83:** an unpinned operator
      dispatch on such a value is refused; the non-claim is output only.
    - **Cost bound.** One type per entry, taken ONCE per dict per epoch: O(entries) at the first
      hand-over and after an operator-side dict mutation, O(1) per dict at a later hand-over of
      an unchanged dict (the walk does not descend into a still-fresh dict). Verification costs
      O(entries) only for a dict sealed code mutated since the last verification. A dict of more
      than `DICT_SNAP_MAX` = 1,000,000 entries is REFUSED at the crossing, never skipped (a
      skipped dict is one the candidate could retype). The honest-program sweeps (352 `.ax` files'
      diagnostics, 55 `@[test]` files, the examples' exit codes) are unchanged by it, and no
      example comes within three orders of magnitude of the bound.
    - **Tests.** `sealed_code_cannot_retype_a_dict_entry_the_operator_held` (the review's
      overwrite, remove-then-re-add, an alias in an array, a nested dict, `Wrap<Dict>`,
      `Option<Dict>`, `[Dict]`, a generic `T` bound to the dict — each with its GOOD and WRONG
      controls), `a_dict_the_candidate_mutated_is_verified_at_every_edge_back` (an operator
      closure the candidate calls, a channel, a replaced operator closure),
      `a_dict_the_candidate_adds_to_or_builds_still_crosses` (new keys, a candidate-built dict,
      the operator retyping its own dict), `a_dict_over_the_snapshot_bound_is_refused_not_skipped`,
      and through the real runner `a_dict_entry_the_operator_held_is_never_retyped_by_the_candidate`.
      The effect-handler arm edge is hooked (`handler_edge_into`/`handler_edge_back`) and covered
      only structurally by the verification primitive (no handler-specific attack test; no row).
    - **Rows.** M1671 (bound), M1672 (retype check), M1673 (operator closure replaced), M1674
      (dirty marking), M1675 (snapshot at a candidate fn's arguments), M1676 (verify at a candidate
      fn's return), M1677-M1680 (the walk descends into a nested dict, `Option`/`Result`, struct
      and enum fields, array and tuple elements), M1681 (the verdict is acted on). Matrix A106-A109. The call_closure and channel
      hooks have no row: the return edge of the enclosing candidate fn re-checks the same dirty
      dict, so removing one of them alone changes no outcome (an attack through it is refused
      elsewhere, and a row there would score REFUSED_ELSEWHERE, never a kill).
    - **Native codegen.** Unchanged: the seal is interpreter-only (part 1).
    - **Operator deployment.** The guest image must be rebuilt (part 1 already requires it).
    - **Matrix check.** `psv_matrix_check.py` FAILs on this branch only because A94 and A95 live
      on other branches; FLOOR is 109 and it will pass once integrated.

73. **`harden()` resets the process attributes a set-id exec preserves, derived from the full
    enumeration (C9 round 4c, workstream HARDEN; matrix A110-A117; rows M1600-M1620, M1622;
    M1621 and M1623-M1629 unused).** No counting rule is relaxed.
    - **Before.** Round 4c's SENTINEL review (MAJOR-ADJACENT): `harden()` reset signals, mask,
      umask, cwd, descriptors, environment and seven limits, and nothing else. A caller that armed
      `ITIMER_REAL` before exec made the setuid helper die of SIGALRM (exit 142); ^C written to a
      pty the caller owns killed it with SIGINT (the helper sat in the caller's foreground process
      group). Both reproduced; the enumeration below is the single source for what else a caller
      hands down (execve(2) "preserved across execve", credentials(7), prctl(2)).
    - **After.** `harden()`: disarms ITIMER_REAL/VIRTUAL/PROF (M1600-M1602; POSIX timers are
      deleted by execve and need nothing) FIRST, so a timer cannot fire between the disposition
      reset and the disarm; `setsid()` (M1603), whose failure (EPERM: the helper is a process
      group leader) sets a flag that `session_left()` reports and the helper binary refuses the
      launch on (exit 30, `could not leave its caller's session`), checked beside
      `setuid_honoured` so `harden()`'s signature is unchanged (M1604; the check is not on
      `--probe`, which launches nothing). Fabric spawns the helper as an ordinary child and
      reads stdout to EOF then `wait()`s (backend.rs), so `setsid` is compatible; an operator who
      runs the helper from an interactive shell's job control gets the refusal. Limits: STACK
      8 MiB soft/unlimited hard, RSS and LOCKS unlimited, MEMLOCK 8 MiB, SIGPENDING unlimited,
      MSGQUEUE 819200, NICE 0, RTPRIO 0, RTTIME unlimited (M1605-M1613); with the earlier seven
      that is all sixteen, and `every_resource_limit_the_kernel_lists_is_one_harden_resets` (A116)
      fails if `/proc/self/limits` lists another. Scheduling: nice 0 (M1614), I/O class none
      (M1615), SCHED_OTHER (M1616, which also clears reset-on-fork), every CPU (M1617). Kernel
      state: oom_score_adj 0 (M1618), timer slack 50000 ns (M1619; an explicit value, because
      `PR_SET_TIMERSLACK 0` restores the INHERITED default, i.e. the caller's), personality
      PER_LINUX (M1620), child-subreaper cleared (M1622). Every reset is best effort for a
      test-trust helper that is not root (limits fall back to what the hard limit allows).
    - **Operator-visible change.** The helper and the launcher it runs now run at nice 0, I/O
      class none, SCHED_OTHER, oom_score_adj 0 and every CPU whatever the Fabric unit set
      (`Nice=`, `OOMScoreAdjust=`, `CPUAffinity=`, `IOSchedulingClass=`): those are caller state,
      not the helper's. A unit that wants a different policy for the root side needs the D2
      follow-up (the helper in its own unit).
    - **Disposition of every preserved attribute.** R = reset in `harden()` (row); H = harmless
      because; C = covered by a named guard.
      | Attribute | Disposition |
      |---|---|
      | real/saved uid and gid, supplementary groups | C: `become_root` (`setgroups(0)`, `setresgid`, `setresuid`) before any act as root; M-row: the become_root attack |
      | capabilities (inheritable, bounding set), securebits | H: only a caller holding CAP_SETPCAP can change them, and a lowered bounding set or NOROOT leaves a euid-0 helper WITHOUT the capability, which `become_root` refuses (fail closed) |
      | NoNewPrivileges, nosuid, user namespace, ptrace-tracing (set-id ignored) | C: `setuid_honoured` / euid rule (amendment 65, M602) |
      | seccomp filters, Landlock domains | C: both need NoNewPrivileges for an unprivileged caller, which makes the kernel ignore the set-id bit (as above) |
      | dumpable | R before: `PR_SET_DUMPABLE 0` (amendment 65 exemption); set-id exec also resets it |
      | core limit, CPU, FSIZE, DATA, AS, NPROC, NOFILE | R: M1479, M1480, M1482 |
      | STACK, RSS, MEMLOCK, LOCKS, SIGPENDING, MSGQUEUE, NICE, RTPRIO, RTTIME | R: M1605-M1613 |
      | signal dispositions, mask | R: M1474, M1475, M1481 |
      | pending signals | H: the caller can `kill` the helper at any moment before `become_root` makes the real uid 0, so a pending one adds nothing; after it only root can signal |
      | umask, cwd, root directory | R: M1476, M1477; chroot needs CAP_SYS_CHROOT (the caller has none) |
      | open descriptors above stderr, file locks on them | R: M1478 (`close_range`) |
      | descriptors 0-2 closed or hostile | H: Rust's std reopens a closed 0-2 to /dev/null before `main` (measured: `--probe` with all three closed exits 0); what the caller put there is the caller's own file |
      | environment | R: M1487 (and `sealed_exec::command` for children, M228) |
      | process group, session, controlling terminal | R: M1603, M1604 |
      | interval timers | R: M1600-M1602 (POSIX timers: deleted by execve) |
      | nice value, I/O priority, scheduling policy, CPU affinity | R: M1614-M1617 |
      | oom_score_adj, timer slack, personality, child subreaper | R: M1618, M1619, M1620, M1622 |
      | PR_SET_TSC | H, measured: a faulting TSC kills every program of this libc in the loader, before `main` (even `true`), so the helper dies at its first instruction having acted on nothing and no reset in `harden()` could run; none was written (it would be a guard no attack distinguishes). `a_callers_timestamp_counter_trap_launches_nothing_it_cannot_finish` pins the fail-closed shape (no row) |
      | PR_SET_PDEATHSIG | H: prctl(2): cleared by the kernel on a set-id exec |
      | PR_SET_KEEPCAPS, signal alt stack, mlockall, robust list, io_uring/AIO | H: cleared by execve |
      | ADDR_NO_RANDOMIZE and the other PER_CLEAR_ON_SETID personality bits | H: cleared by the kernel on a set-id exec; the rest of the personality is M1620 |
      | PR_SET_PTRACER (Yama) | C: dumpable 0 and the real uid 0 after `become_root` |
      | coredump_filter | C: RLIMIT_CORE 0 (M1479) and dumpable 0 |
      | THP-disable, speculation-control, MDWE, mempolicy, MCE-kill, name, CPU time counters, uclamp | H: performance, tightening-only (unclearable) or hardware-error behaviour, none grant or withhold authority |
      | session keyring | H: nothing in the helper or its launcher tree calls keyctl |
      | audit loginuid | H: it attributes the CALLER, which is the correct attribution |
      | cgroup | amendment 65 (the helper is in the caller's cgroup, documented) |
    - **Rows.** M1600-M1602 timers, M1603 setsid, M1604 the refusal, M1605-M1613 limits,
      M1614-M1617 scheduling, M1618/M1619/M1622 kernel state, M1620 personality. Each is killed by
      its own attack: the caller (root, then dropped to a non-root uid; the NICE/RTPRIO ceilings
      are raised before the drop, as a service manager's `LimitNICE=` does) arms the state, a
      WITNESS (an ordinary exec of python3) shows the state survived, a CONTROL launch with
      nothing armed reads the reset values through the same observation, and the setuid helper
      run under the armed state must report the control's values (or, for timers, live). Tests:
      `crates/axon-fabric/tests/privileged_launcher.rs` (8 root-only) and one unit test in
      `privileged_launcher.rs`. Refusal coverage: the new refusal site in `session_left` is covered
      by M1604; its caller in the binary is exempted as a diagnostic. M1474-M1482, M1487 and the
      observer rows are unchanged.
    - **Matrix.** A110-A117 (assigned A120-A127; renumbered at integration so the matrix is one
      contiguous run, A94 observer, A95 custodian reply, A96-A109 PSV-1, A110-A117 harden,
      A118-A122 gate).
    - **Operator deployment.** None required. The helper must not be started by a process that
      made it a process-group leader (interactive job control); Fabric does not.


## Amendment 74: predicate primitives are refusal sites, a row covers only what its edit changes, and the evidence harness stops trusting labels (C9 round 4c, gate)

- **Before.** Round 4c (EQUIVALENCE) found (B1) `axon_psv::keyed_outcome`, the one function that decides whether a result line carries K's token, with no row and invisible to the refusal gate (it refuses by `return None` / `.then_some`); (B2) a site counted covered by a row whose edit never touches it (`judge_file` spanned `old.count('\n')` lines, so an `old` ending in a newline covered the next line: evl.rs `!issuer_ok` and custodian.rs `seen != pid`); (B4) `psv_guest_boot_test.sh` exec'd its helper, custodian and fabric from a guessed `${CARGO_TARGET_DIR:-$REPO/target}/debug`, which the harness_binaries scan did not flag; and join/merge took a shard's `holds`/`all_killed` label on trust.
- **After (gate).** A function declared `-> bool` or `-> Option<..>` is ONE site (its body is the guard block; exempt by an anchor on its HEAD line); `.then(`/`.then_some(` is a line site; `let .. else {` opens its block; a guard block stops at its function head; a row covers a site only when a line its edit CHANGES (difflib over old/new, leading/trailing newlines not lines) lies in the block. `.ok_or(` is not a site (it converts an absence a scanned decision already made). bin/axon-loop.rs is scanned (its OUT_OF_SCOPE reason was false: it decides G10 and the request schema itself, M1736, M1737). Rows M1720-M1745: keyed_outcome's token comparison (M1720 guest, M1721 Fabric) and one-line rule (M1722); EVL `!issuer_ok` (M1726, retired EQUIVALENT_DID: four cells against M10 + M1299, argument in EQUIV_RECORD); the custodian one-process rule (M1727); the PCI provenance predicates (M1730, M1731, M1734), the reference scheme rule (M1732), the guest's production no-bypass constant (M1733), a freeze's class read back from the ledger (M1735); the gate's own logic (M1738-M1742); join/merge recompute (M1743, M1744); the journal lock window (M1745). Every newly visible site in the current in-scope files has a row or a reasoned exemption (PREDICATE OF NAMED ROWS, LOOKUP, RECORDED FACT and the existing kinds); dependency-crate sites stay NOT YET SCANNED (the sites branch).
- **After (harness).** `script_spawn::binary_choice_violations` flags a slash-less `.../debug` assignment and `$VAR/<bin>` uses of it (the boot test's exact line is an attack case); the boot test resolves with `use_built`. `join_shards` recomputes HOLDS from the recorded cells with the run's own predicate and refuses a mismatch; `merge` recomputes all_killed from the rows. 15 generic markers became ATTACK texts.
- **Triage items.** M1266 is LIBRARY_PRIMITIVE (its retirement claimed a green suite; fixtures.rs and parse_validate_sites go red), its GUARD_SETS entry removed. `claims_gate.sh`: `echo | grep -q` under pipefail gave false "missing verb" failures under load (13 misses in 300 loops, 0 with a here-string); fixed, with a drift check. Journal `lock_exclusive`: the retry window widened from 500 ms to 5 s (the fd is already O_CLOEXEC; the flock lingers in a fork's child until its exec); M813 is neither retired nor reclassified.
- **Matrix.** A95 and A118-A122. The branch numbered its rows A94-A99 (after A93, so the checker's contiguity rule held on its own branch); at integration A94 (observer) and A96-A109 (PSV-1) were taken, so its rows became A95 (custodian reply) and A118-A122 (the keyed verdict line, the EVL independence arm, the guessed helper path, the refusal-site gate, the edited shard). The matrix is one contiguous run A1-A122; `psv_matrix_check.py` has no reserved gaps.
- **Amendment 74 addendum: the test-module rule hid production code (audit F1).** `code_lines` returned `lines[:i]` at the FIRST `#[cfg(test)]` followed by a line starting `mod tests`, dropping everything after it to the end of the file. Real protected-route miss: axon-os `approval.rs` has its test module at 128-187 and production code after it, including `authorize` (reached from submit.rs `supervise_requiring` -> supervisor.rs -> `approval::authorize`); its `approval required but missing` refusal was invisible, so the file has 5 sites, not 4. Executed bypass: an empty `#[cfg(test)] mod tests {}` above a production `Err(...)` made the gate skip the `Err` (the same for `mod tests;`). Fix at the source: `code_lines` now BLANKS (line numbers kept) only each `#[cfg(test)]` item's own extent, by a brace matcher that skips strings (incl. raw and byte), char literals, lifetimes and nested comments, or its `;` for `mod tests;`; a module file declared `#[cfg(test)] mod X;` is out of scope as test-only, nothing else. M1747 (gate test `production_code_after_a_test_module_is_scanned`: an empty stub module and a real one with braces in strings/chars/comments, each above/before a production `Err`). `approval.rs:248` is rowed: M1746 (`else if manifest.require_approval` -> `false &&`), killed by `grant_authority::the_grants_require_approval_policy_is_enforced` through the Fabric submit route (ATTACK text on its required-and-absent step). Count correction: NOT_YET_SCANNED approval.rs 5 sites with M1746 covering one (4 left). Re-derivation of the axon-cortex lib.rs sites: under the corrected rule 6 of its 9 sites lie inside `#[cfg(test)]` modules (`contract_tests` and the like), so they are non-sites by the same rule that hides unit tests everywhere; 3 remain (counted in NOT_YET_SCANNED). I scanned every `crates/*/src/**/*.rs` for a site that the corrected rule shows and the old one hid: approval.rs only.

- **Amendment 74 x 75 integration (c9r4c/integrate).** Merged on top of amendment 75's empty NOT_YET_SCANNED, amendment 74's predicate-primitive and whole-file test-module rules expose 54 sites in 22 files: the dependency crates (axon-attest, axon-audit, axon-cortex, axon-os, axon-vm) plus `observer_service.rs` (3) and `attestation.rs` (1). Judged by callers: 49 now carry an exemption stating a checkable fact (no non-test caller; not reached by Fabric's `axon_os`/`axon_cortex`/`axon_vm` names; operator-authored `parse_manifest` scalars; `CheckRegistry::check` is a LOOKUP whose one protected consumer's refusal is its own site; `is_canonical_public_key_hex` is a PREDICATE OF NAMED ROWS, its one caller's refusal disabled whole by M1582; the observer's `plain_absolute`, `is_hex` and `key_in_root` base). FIVE are NOT exempted and are listed in NOT_YET_SCANNED (2 files), because they DECIDE on the protected route and no row attacks them: the axon-os admission chain reached from `supervise_requiring` (`is_ancestor`, `host_allows`, `host_matches` through `Grant::intersect`'s effective grant, `IsolationRequirement::satisfied_by`, and `scan_effects`'s `calls_name`). `v022_refusal_coverage.py` holds (rc 0); `--freeze` FAILS on exactly those 5 sites until each has a route row through `submit` (or a stated, checkable reason it cannot matter). The matrix rows A94-A99 became A95 and A118-A122 (A94 is the observer's, A96-A109 PSV-1's, A110-A117 harden's).

## Amendment 75: the 118 dependency refusal sites are judged by their protected-route callers (C9 round 4c, sites)

75. **NOT_YET_SCANNED is empty: `v022_refusal_coverage.py --freeze` holds.**
    - **Before.** Amendment 71 brought the closure of the protected roots' normal workspace
      dependencies into the refusal gate and listed 118 sites in 18 files as NOT YET SCANNED
      (a freeze refused while any was listed).
    - **After.** Every site was judged by reading its function and its callers on the protected
      route (Fabric, root helper, launcher script, guest, runner, `psv::derive`; loop intake and
      readiness), never by file. The ledger of every disposition, with its call-graph fact, is
      `governance/notes/v022-dependency-sites.md`: 10 ROWS (M1760-M1769), 102 exemptions each
      stating a checkable fact (81 not on the protected route, 15 operator-authored on it, 3 the
      journal budget already exempt as not a verdict property, 1 named row, 1 nothing to admit,
      1 usage) and 6 non-sites (test assertions of axon-cortex `lib.rs`, whose test module is
      renamed `contract_tests` to `tests` so the gate's test-module rule applies). No bulk
      exemption, no retirement record (nothing was retired).
    - **Rows (each KILLED by its own attack on the production route).** M1760 the certified
      verdict parser's no-summary refusal, through `submit` on the helper route
      (`psv_dispatch an_output_with_no_summary_is_no_verdict`: a genuine keyed pass whose output
      lost only its summary line, digest consistent; new stand-in tamper `no-summary`; control:
      the same launch with its summary passes). M1761 the one suite-id rule, through the registry
      FILE the `axon-fabric` binary loads (`CheckRegistry::load`); M1762 the suite-reference
      parser's version and entry rule (the loop config writer); M1763 the strict parser's
      duplicate-key refusal (intake of an episode holding `corpus_role` twice). M1764/M1765
      `axon test` in the runner's exact invocation: the type-check abort (an ill-typed candidate
      that would run to the right value earns no pass) and the failing exit (`3`, which `derive`
      cross-checks against the verdict). M1766-M1769 the approval token's four bindings (decision,
      program, grant, its own digest) through `submit`'s axon-os admission, each alone.
    - **Custodian usage-die anchor overlap.** The usage arm could not be exempted: M631's guarded
      statement (`cu::check_store(..).unwrap_or_else(die)`) sat four lines below, so the gate's
      guard block for it reached up to the `_ =>` arm and every anchor for the usage arm was
      "exempt yet covered by M631". The store check is now its own `if let Err(e) = .. { die(&e) }`
      (behaviour unchanged), M631 is re-anchored on it and re-run, and the usage arm carries its
      own USAGE exemption.
    - **Audit follow-ups (applied at integration).** (F2) The ledger's claim that axon-audit is
      "reached only via axon-os cli.rs" was textually wrong: axon-core, the `axon` binary the guest
      execs, links it (preflight.rs; main.rs `set_ledger_path`, `flush_ledger`;
      interp/builtins.rs `append_global`, `append_ai_call`). The ten axon-audit exemptions now
      carry the reason that holds: the psv runner execs `axon test` with `env_clear()` and sets no
      `AXON_AUDIT_LEDGER`, and every audit failure in axon-core is only printed or discarded
      (`let _ =`), never an exit code or a verdict (category NOT A VERDICT PROPERTY). (F3)
      `Coalition::new` is `pub`; the checkable fact is that it has no non-test CALLER (only
      `axon-os/tests/r27_acceptance.rs`), and the five coalition exemptions say so. `ledger.rs:74`
      is the journal's real spend check (the admission budget guard); its exemption says so and
      stays "not a verdict property" because its refusal is the journal's `BudgetExceeded`.
    - **Matrix.** None. **Operator deployment.** None.

## Amendment 76: a verdict is a refusal site (C9 round 4c, admit)

76. **The refusal-site gate reads refusals that are RETURNED VERDICTS; the axon-os admission chain and
    the verdicts the loop and Fabric produce are rowed or exempted by their protected-route callers.**
    - **Before.** Integration round 2 passed everything except `v022_refusal_coverage.py --freeze`, which
      failed on five axon-os sites that decide on the protected route and had no row (`is_ancestor`,
      `host_allows`, `host_matches` behind `Grant::intersect`; `IsolationRequirement::satisfied_by`;
      `calls_name` behind `scan_effects`). The class under them: `gate::admit` and `supervisor.rs`
      refuse by RETURNING an enum value (`Admission::Deny {..}`, `Verdict::Denied {..}`), a shape none
      of the gate's site forms (`Err(`, a bool/Option return, the named constructors) matched, so the
      whole admission chain was invisible, not only the five.
    - **After (gate).** The gate DERIVES the verdict types from the in-scope code: every enum that is
      not an `*Error` (whose refusals travel inside `Err(` and are sites already) and has a refusal-named
      variant (`STRONG_NEG`: Deny/Refus/Reject/Veto/Blocked/*Violation/Invalid/Unauthorized/Unverified/
      *Mismatch/Tamper/Forbid/Halted/Unsupported/FailClosed/Fail/Malformed/*Exhausted/*Bound/Corrupt/
      Tripped/Flagged) or a deciding name (`Verdict|Decision|Outcome|Admission`), its NEGATIVE variants
      being those plus the "no verdict" ones (`WEAK_NEG`: Unknown, TimedOut, Cancelled, ...). Forms:
      (a) a built negative variant (`Enum::Variant`, `Self::Variant` in the enum's own impl) is a line
      site, a pattern (arm left side incl. tuple patterns, `let`/`if let`, `matches!`, `==`) is not;
      (b) a function whose return type is a verdict (also through Result/Option/Vec/Box, a tuple, a type
      alias) is a block site, exempt on its head line like a predicate primitive; (c) a call of a HELPER
      CONSTRUCTOR (a short fn whose every construction is negative, or whose body is one `Err(..)`, or a
      local closure building a refusal) is a site and its definition is not (a name defined twice in one
      file is a platform variant and no helper); (d) a closure predicate refused through `ok_or`
      (`.filter(|k| k.len() == 32).ok_or(..)?`) is a site: with the predicate inline nothing else scans
      it; (e) `#[cfg(all(test, ..))]` is test code (the psv xattr module read as production). `?`,
      bare `ok_or` and `matches!` guards stay what they were: a `matches!` condition is in its
      refusal's guard block already (`a_matches_guard_opens_its_refusal`). Over the in-scope files the
      forms see 274 sites in 35 files; 152 had neither a row nor an exemption. `sites()` now returns the
      site's kind (line / predicate / verdict) and one `_guard_start` serves every form. Rows M1770-M1781
      are the gate's own guards, each killed by a planted production-shaped refusal it must name
      (`refusal_coverage_gate.rs`, 8 new tests); the gate tests also require `evl.rs` and
      `axon-guest-init` to be fully scanned (they plant sites there), which is why those two are rowed
      first.
    - **After (the five, and the chain).** Through `submit` -> `supervise_requiring` (tests in
      `axon-fabric/tests/admit_route.rs`): M1785-M1788 each effect axis of `gate::admit`'s table, M1789
      its denial, M1790 the confidentiality ceiling, M1791 the supervisor honouring `admit`, M1792 honouring
      `approval::authorize`, M1793-M1795 `calls_name` (whitespace before the parenthesis, the call test,
      a bare `mod`). `is_ancestor`, `host_allows`, `host_matches` are EXEMPT with a checkable fact and a
      drift test: on the route the supervisor grant and the manifest's grant are the same grant
      (`admission_intersects_the_resolved_grant_with_itself`), `intersect` emits only clones of its inputs,
      so no edit of those predicates widens anything and the narrowing direction is a denial. The
      isolation requirement is checked twice, by `backend::select` (M1052) and by the supervisor (M1796
      guard, M1797 predicate arm), and each dominates the other: M1052's recorded kill was the
      SUPERVISOR's refusal (`assert_never_runs` read any receipt that was not `Unsupported` as "it ran",
      so a `Denied` receipt of a request that never spawned counted as the attack getting through). The
      helper now judges by effect, so M1052 as then written became REFUSED_ELSEWHERE. It stays ACTIVE on what only
      selection answers, the receipt contract (`Unsupported`, never another status;
      `hardware_isolation_linux_is_refused_without_a_qualified_profile`, which also keeps the full suite red with
      M1052 off, so a retirement of M1052 would be false); M1796 (the supervisor's guard) and M1797 (`satisfied_by`'s
      ProcessScoped arm) are LIBRARY_PRIMITIVE, killed by a direct `supervise_requiring` test
      (`axon-os/tests/admit_isolation.rs`). A four-cell retirement of them against M1052 was executed and
      REFUSED: the four cells held, but axon-os's own suite pins both (`hardware_isolation.rs`,
      `runtime_tests::only_the_protected_linux_profile_satisfies_a_microvm_requirement`), and a retirement
      needs the full suite green.
    - **After (the verdicts the loop and Fabric produce).** EVL (M1783 a verifier-reported failure, M1784
      an undelivered trial, each counted as a pass), admission (M1798-M1800 the Vetoed/Reject/Inconclusive
      constructions, M1801-M1803 `Verdict::from`), intake's receipt-to-verification map (M1804, M1805), `tel`'s
      bounded add (M1806), safety's violation state (M1807), `project_receipt_status` (M1808, M1810-M1812;
      M1809, the Canceled arm, is LIBRARY_PRIMITIVE: EVL's `run_end` M131 dominates it on every route),
      the guest runner (M1813 the keyed fallback, M1814 a refusal reported as a pass; the guest's no-policy
      decision M1782), `interpret_linux_result`'s guards (M1815-M1820) and `linux_receipt`'s outcome map
      (M1821-M1823), submit's verdict over unjudged bytes (M1826). Two rows written for submit's
      `Failed`/`Passed` relabel (M1824, M1825) were REFUSED_ELSEWHERE by the completion check after them
      (M63, M64) and are not kept: their lines are exempt as dominated, with the measurement in the reason.
    - **Dispositions.** 55 rows (M1770-M1826 less M1824/M1825; M1827-M1829 unused), 0 retired, 3 LIBRARY_PRIMITIVE (M1796, M1797, M1809), and 123 exemptions each stating a checkable fact
      (`governance/notes/v022-dependency-sites.md`, "Amendment 76 addendum"; no bulk exemption).
      NOT_YET_SCANNED is empty and `--freeze` holds.
    - **Matrix.** A123-A126 (the matrix is one contiguous run A1-A126, `psv_matrix_check.py` FLOOR 126).
      **Operator deployment.** None.

## Amendment 77: the verifier's identity is the image it runs, and a sharded suite's relink cannot unmake it (C9 round 4c, shardflake)

77. **`readiness_verifier_sha256 is not a sha256` under `scripts/cargo_test_shards.py`.**
    - **Before.** `verifier_identity()` hashed the file at `std::env::current_exe()`'s PATH. A
      sharded suite runs several `cargo test` processes at once on one target dir; any of them that
      finds the tree changed (a touched `.git/index` or any tracked file: `build.rs` watches the
      whole tree) RELINKS the test binary, replacing the file under the siblings still running it.
      A running sibling's `current_exe()` then reads `.../readiness-HASH (deleted)`, the digest
      read fails, the identity becomes `"unknown"`, and `readiness_fixture`'s positive control
      failed with "readiness_verifier_sha256 is not a sha256" (gpumaster, integration round 2:
      a sharded run failed, serial passed). Reproduced by touching `Cargo.toml` every two seconds
      during a sharded `--test readiness` run (same message); a clean sharded suite passed 10 of
      10 on the old code, so the trigger is a tree change during the run, not load.
    - **After.** The identity is the digest of the image the process RUNS (`/proc/self/exe`,
      `readiness::running_image()`), which survives replacement of its file. The test binaries'
      re-executions of themselves (`journal`, `trust_root`, `restart_matrix`) and the observer
      test's own digest use the same path. `privileged_launcher`'s production build, shared by
      every shard, is built under an exclusive lock and each process runs private copies
      (the `workspace_bin` pattern).
    - **Row (killed by its own attack).** M1830 puts `current_exe()`'s path back in
      `running_image_sha256`; `verifier_identity_replaced the_identity_survives_replacement_of_the_executable_file`
      starts a copy of its own binary, unlinks the copy (what a relink does) and asks the child for
      its identity: old code reports `"unknown"`.
    - **Part 2: the class is removed at the runner.** `scripts/cargo_test_shards.py` no longer
      runs `cargo test` per shard. It builds ONCE (`--no-run`), then runs the same `cargo test`
      once more with a stub runner that records, per test binary, the environment and working
      directory cargo itself gives a test process (nothing is guessed: `CARGO_MANIFEST_DIR`,
      `CARGO_PKG_*`, `CARGO`, library paths, cwd = the package root), and every shard execs the
      already-built binary with that environment, `-q` forwarded as cargo forwards it, and
      `--exact <its tests>`. No shard can relink anything, so `env!("CARGO_BIN_EXE_*")` paths and
      every `current_exe()` stay valid for the whole run. Measured parity: a test's environment
      under `cargo test` and under a shard differs only in the invoking shell's own `_`/`OLDPWD`.
      Counts, per-binary order, `test result:` lines, the lost-test rule, the SIGTERM cut and
      build failure (cargo's output, its status) are unchanged; the doc-test unit still goes
      through cargo (it builds no test binary). A binary whose environment was not captured fails
      the run. Test: `harness_integrity a_sharded_run_survives_a_source_change_made_while_it_runs`
      plants the relink (one shard rewrites the package source while another is mid-test and a
      third starts afterwards): the previous runner fails it (`... (deleted)`), this one passes.

78. **A position the operator held is judged by what it held — deeply, and strictly (C9 round 5,
    workstream r4c-psv1b, PSV-1; matrix A127-A130; M1840-M1845 and M1847-M1848 (there is no M1846); amends 72 part 2).**
    - **Findings** (`/var/tmp/c9r5-findings-PSV-1.json`, cases `/var/tmp/c9r5-psv1-logs/w/c0..c6`).
      (1) BLOCKER, executed (c1): the candidate REPLACED a dict key the operator held with a
      candidate-built dict carrying a `u8`. Part 2 cast each held key against its recorded
      `value_type`, which for a held dict is only `Dict`; the replacing dict is another `Rc` with
      no snapshot, so its values were unconstrained (the in-place `dict_set(inner, "x", u8)` was
      refused; the replacement was a keyed pass). (2) MAJOR-ADJACENT, executed (c4): the operator
      held `None` at "best" (recorded `Option<?undetermined>`); the candidate stored
      `Some(4 as u8)`, and part 2's non-strict cast let it through — against amendment 72's
      invariant ("a position nothing on the operator side determined is refused"). (3) FUTURE:
      `walk_fresh` returned silently past `MAX_CAST_DEPTH`, leaving the dicts below unrecorded.
    - **After.** The snapshot keeps the HELD VALUE at each key (`DictSnap::held`, a clone: a
      nested dict is the same `Rc`, with its own snapshot) instead of a type. `Interp::replaced_ok`
      judges the value now at a held key against the held one: (a) a dict that is NOT the same
      `Rc` is judged against the held dict's entries (its own snapshot) — a key present in both
      keeps its type, recursively; keys only the replacement has are free, exactly as part 2's new
      keys (stated there, unchanged); (b) arrays and tuples element by element, structs and enum
      variants field by field, `Option`/`Result` through the payload — so a container carrying a
      dict is never judged by its bare type; (c) the LEAF rule: the replacement casts STRICTLY to
      the type the held value showed, so an undetermined position (a `None`'s payload, an empty
      array's element, a `Result`'s other arm) REFUSES a value instead of staying free; a held
      closure's signature is not shown, so it is exempt from the strict cast, but an operator
      closure may not be replaced by a candidate's closure or by a non-closure; (d) cycles are
      handled by a visited set of (held, replacing) pairs, and a depth past the cast's bound is
      refused. `dict_edge_out` calls it for every held key of a dirty dict.
    - **Placeholder-then-fill (item 2) — adopted, measured.** Honest-program impact: the sweeps
      are identical (352 `.ax` files' diagnostics, 55 `@[test]` files green, example run exit
      codes) and no `.ax` in the repository stores a `None` or `[]` placeholder in a dict or runs
      under `--seal` at all (seals are made only by the PSV runner), so no repository program
      breaks. The cost is real and stated: an operator that hands the candidate a `None`/`[]`
      slot to FILL, then reads it untyped, now gets a refusal — it holds a TYPED placeholder
      instead (`Some(0)`, `[0]`), which the candidate may then replace with a value of that
      type (`a_placeholder_the_operator_held_is_not_filled_by_the_candidate`: GOOD `Some(0)` ->
      `Some(9)` passes, WRONG `Some(4)` fails). Leaving a placeholder untouched, or emptying a
      typed slot (`Some(3)` -> `None`), is unaffected.
    - **Walk bound (item 3).** `walk_fresh` returns `Result` and calls `walk_depth_ok`, which
      REFUSES a value nested deeper than `MAX_CAST_DEPTH` (the same bound the cast uses) —
      consistent with the entry-count rule. A real value that deep overflows the thread stack
      first, so it is tested directly (`walk_bound_tests::a_value_nested_past_the_bound_is_refused_not_left_unvisited`).
    - **Replacement family, hunted.** Replaced at a held key and refused: a candidate-built dict,
      a dict two levels down, `[inner]`, `Some(inner)`, `(inner, 1)`, `Wrap { v: inner }`, a dict
      received on the operator's channel; scalars in containers (`[3]`, `Wrap { v: 3 }`, `(3, 1)`,
      `Some(3)`); a closure replaced by a non-closure (and by the candidate's). Accepted by design
      (a position the operator never determined): keys only the replacement has, and a
      candidate-built dict RETURNED to the operator (e.g. `fn f(inner: Dict) -> Dict { fresh }`,
      `Wrap<Dict>` returns) — the operator's argument is not at that position; the return is
      candidate output, part 2's non-claim. Not covered, stated: a generic enum variant's fields
      are judged by the same rule but have no dedicated attack test.
    - **SUPERSEDED by amendment 83 (round 6):** the list below is the round-5 state; the Dict
      non-claims now reduce to output only, never to selection of an operator impl.
    - **Non-claim text updated** (amendment 53 (1), amendment 72 part 2): what stays open for a
      `Dict` is exactly (i) a key the operator never held, (ii) a dict the candidate builds and
      returns, (iii) an operator-held dict whose snapshot an operator-side mutation made stale
      WHILE sealed code ran (the epoch retakes it at the next hand-over only). Everything the
      operator put in a dict, at any depth, is judged.
    - **Rows.** M1840 (a replacing dict judged against the held entries), M1841 (array/tuple),
      M1842 (struct), M1843 (`Option`/`Result`), M1844 (a store at an undetermined position is
      strict), M1845 (the leaf rule refuses), M1847 (the walk bound), M1848 (a held closure is not
      replaced by a non-closure). Re-anchored in place (same guard, same test, new text): M1672,
      M1673 (the closure check moved into `replaced_ok`), M1677-M1680 (the walk now returns
      `Result`). Matrix A127-A130; the matrix check passes (130 rows).
    - **Tests.** `a_position_the_operator_held_is_judged_by_what_it_held_when_replaced`,
      `a_placeholder_the_operator_held_is_not_filled_by_the_candidate`, the closure-to-non-closure
      case in `a_dict_the_candidate_mutated_is_verified_at_every_edge_back`, the bound test, and
      through the real runner `a_replaced_or_filled_position_is_judged_by_what_the_operator_held`
      (c1 and c4, with GOOD and WRONG controls).
    - **Native codegen.** Unchanged: the seal is interpreter-only (amendment 72).
    - **Operator deployment.** The guest image must be rebuilt.

## Amendment 79: the observation's fields are measured, pinned or the principal's word, said field by field (C9 round 5, obsbind)

79. **The observer no longer countersigns Fabric-authored values, the helper serves only the Fabric
    program the operator pinned, and the observer observes only a nonce the custodian issued.**
    - **Before (reviewer findings PSV-6 and SENTINEL, round 5).** (Wording corrected by amendment 84: the observation has a Firecracker DIGEST, `firecracker_sha256`, not a revision; and `observed once` is the observer's `.observed` record, while the custodian's state is issued/unspent/expired/spent.) The PSV-6 claim said the
      observation "names ... Fabric and Firecracker revisions; guest image, kernel, verifier, suite
      and policy digests" and that "Fabric consumes it and cannot mint it". EXECUTED
      (`probe_told_fields`, through the real helper `--observe` relay and the real observer): a
      manifest with `policy_sha256`, `guest.init_sha256` and `guest.axon_sha256` of nine digits
      each, `fabric_revision` `not-the-running-revision` and `authority.epoch` 987654321 was
      relayed and SIGNED; `measure()` compared nine fields and copied the rest from the manifest.
      And `verifier_sha256` was the digest of `/proc/<ppid>/exe` of whichever Fabric-uid program
      called `--observe`: nothing operator-held pinned it (the existing control for
      `an_observe_relay_whose_parent_is_not_the_fabric_relays_nothing` was a python3 process that got
      an observer-signed observation naming python3 as the verifier). The observer's store and the
      custodian's issue store grew without bound under Fabric-controlled requests, and the
      observer never asked whether a nonce had been issued.
    - **(a) The Fabric program is pinned.** The helper config gains `fabric {path, sha256,
      revision}` (required; loaded with the strictness of the other pins: a lowercase sha256, an
      absolute plain path, and a build revision that is 40 lowercase hex on a production host).
      `authenticated()`, the ONE caller gate a launch and an `--observe` relay share, now also
      requires that the digest of the caller's executable (hashed from its pidfd by
      `running_caller`, after the helper is root in every id) equals `fabric.sha256`; otherwise
      exit 30 and nothing is launched or relayed (M1857). The observer reads the same pin from the
      helper config it already reads and measures: `verifier_sha256` and `fabric_revision` are the
      PIN's, and the manifest's claims must equal them (M1539, M1850); the observer also refuses a
      caller digest the root helper measured that is not the pin (M1853).
      **How the revision is derived (the choice).** It is the operator's word in the helper
      config, READ FROM THE INSTALLED BINARY by the operator's kit (`axon-fabric verifier-manifest`,
      whose self-reported `sha256` must be the installed bytes') and checked again by the kit
      (`check` step) and the trust preflight (`helper-fabric-pin-path`, `helper-fabric-pin-sha256`).
      Neither the root helper nor the observer EXECUTES the pinned binary to learn it: running a
      Fabric-uid-installed program as root, or as the observer, to ask it its own name would trust
      the answer of the thing being measured. The alternative considered, embedding the revision
      in the binary and having the observer scan the pinned file, was rejected: it adds a parser
      over attacker-influenced bytes in the signing service to learn what the operator can state.
    - **(b) The guest's init and axon digests are cross-checked.** The observer reads the profile
      manifest ONCE (its digest is the measurement of `profile_manifest_sha256`) and requires the
      manifest's `guest.init_sha256` and `guest.axon_sha256` to equal `artifacts.axon-guest-init.sha256`
      and `artifacts.axon.sha256` there (M1851, M1852): the same two fields `psv::prepare` reads when
      it builds the manifest.
    - **(c) What the observer measures, what the pinned program supplies, what stays the principal's
      word.**

      | observation / manifest field | source after amendment 79 | class |
      |---|---|---|
      | `host_config_sha256`, `launcher_sha256`, `firecracker_sha256`, `guest.kernel_sha256`, `guest.rootfs_sha256`, `suite_registry_sha256`, `qualification_sha256`, `profile_manifest_sha256` | operator files, hashed by the observer | MEASURED (M1531-M1538) |
      | `verifier_sha256` | helper config `fabric.sha256`; the helper holds the caller to it | PINNED, and the caller MEASURED to be it (M1539, M1853, M1857) |
      | `fabric_revision` | helper config `fabric.revision`, read from the installed binary by the kit | TOLD by the operator's kit (M1850; reclassified from PINNED by amendment 85: the helper compares only `fabric.sha256`) |
      | `guest.axon_sha256`, `guest.init_sha256` | the measured profile manifest's `artifacts` | NAMED by a measured file (M1851, M1852) |
      | `nonce` | issued for that epoch and unspent, unexpired (custodian); observed once (observer) | CUSTODIAN's (M1858, M1859, M1540) |
      | `policy_sha256` | the manifest's: built in Fabric's 0700 job dir | **THE PRINCIPAL'S WORD.** Not listed as measured. The root helper holds the policy it BOOTS to this digest (`policy_at_root`, A87), so a launch cannot run another policy than the observation names, but the observer cannot know what policy Fabric should have chosen |
      | `epoch` | the manifest's `authority.epoch`; the custodian binds the nonce to the epoch FABRIC asked it to issue for | **THE PRINCIPAL'S WORD.** The epoch is the loop's scope pointer, which the observer cannot read; it is joined at INTAKE (`check_bundle`: observation epoch, manifest epoch and the loop's own epoch). The custodian holds no authoritative epoch either (it records the one in the issue request and compares the one the helper passes at the spend), so the launch-time epoch check is not independent evidence; intake's join is |
      | `observed_at`, `intended_launch_manifest_sha256`, `host_profile`, `observer_key_id` | the observer's clock; derived over the bytes received; constant; its own key | as in amendment 68 |

      `the_observer_signs_the_operators_values_for_fabric_revision_and_guest_digests` pins the
      stated non-claim: a manifest whose policy digest and epoch are told is still signed as told.
    - **(d) Stated non-claim: an executable-digest measurement does not bind the code that runs, nor
      that the process was started from the pinned file (reworded by amendment 84).**
      `verifier_sha256` is the digest of the executable file the Fabric-uid process HAD when the
      helper opened `/proc/<ppid>/exe` (hashed by descriptor, with the pidfd re-asked for "still the
      same pid and my parent", which does not detect an `execve` of that process). The plain
      exec-after-spawn route (exec the pinned file after spawning the helper) was EXECUTED by the
      round-6 reviewer (18 of 20 attempts returned an observer-signed observation naming the pinned
      verifier; the other 2 lost the race and were refused before any nonce record; no `LD_PRELOAD`
      and no ptrace) and is REFUSED since amendment 85: no process except the helper and its parent
      may hold the pipe the reply is written to, and a production helper refuses a stdout that is
      not a pipe (0 of 20 relayed; M2050, M2051, M2053, M2054). That narrows the route; it does not
      close it. Remaining, per amendment 85: reopening the pipe through `/proc/<pid>/fd/N` of the
      parent, `SCM_RIGHTS`, `pidfd_getfd`, `ptrace`, `LD_PRELOAD`/`LD_AUDIT`, and
      `process_vm_writev` of a same-uid process running the genuine binary. Attacker code then runs
      under the genuine digest (reviewer demo `ldpreload_demo.c`: the helper-side `readlink` of its
      parent's exe reports `/usr/bin/true` while the constructor is attacker code). Amendment 85's
      reply-pipe rule narrows only the plain exec route. The helper's `harden()` clears ITS OWN environment (and its children's), not its
      caller's. Fabric cannot run under `NoNewPrivileges` (the helper is setuid-root; amendment 65
      refuses it), so that is no lever. `kernel.yama.ptrace_scope` >= 2 stops `pidfd_getfd` and a
      ptrace attach, and nothing else here (a host setting the operator makes; the kit does not
      verify it). The Fabric pin is a configuration and mistake guard, not a defence against
      malicious same-uid code. So what the observation attests about the verifier is "at that instant the Fabric-uid
      caller's executable was the pinned file", not "the pinned program's instructions made this
      manifest". Amendment 85 changes who may READ the reply (one reader), not when the
      helper measures the executable, and declines to re-measure after the reply. The guest verdict's integrity does not rest on it:
      it rests on the hidden check, the signature chain and the loop's joins.
    - **Major-adjacent: the observer accepts only a nonce the custodian issued.** The observer asks
      the custodian (the helper config's `custodian` section, which it already reads; the custodian
      gains a `check` op, answered ONLY to its configured `observer_uid`, M1860, which a protected
      custodian config must name and which is neither root, the Fabric's nor the custodian's, M1861)
      whether the nonce is outstanding for the manifest's epoch, BEFORE it hashes or records anything
      (M1858, M1859). The observer authenticates the custodian by the uid of its socket's listener,
      not by the program pin: it runs as its own uid and cannot open another uid's `/proc/<pid>/exe`,
      which is how a pinned program is measured; the PROGRAM is authenticated where the nonce is
      spent, by the root helper (amendment 65). A custodian impostor of the custodian uid could say
      yes to a nonce nobody issued and then fail every spend: an observation nothing launches on.
      A protected observer takes only a protected custodian's nonce (M1862, M1863). The kit grants the
      observer's uid the custodian socket by ONE named ACL entry (`ExecStartPost=setfacl -m
      u:<observer>:rw`), so the socket stays 0660 for the Fabric group.
      **Bounds.** The custodian drops records past their max age at every issue (M1865) and holds at
      most `MAX_OUTSTANDING` = 1024 issued-and-unspent nonces; the next issue is refused (M1866). The
      observer's record carries the nonce's expiry (from the custodian's `check` reply) and every
      request drops the records whose nonce the custodian no longer honours (M1864); an unparsable
      record (a crash between its creation and its write) is dropped a day after its mtime. The
      observer store is thereby bounded by what the custodian holds outstanding and spent records
      live no longer than the custodian honours the nonce. Spent (`.used`) records are bounded the
      same way (pruned a max age after the spend); the number of spends is the number of real
      launches.
    - **Minors.** A production helper config refuses `observer.service.uid == custodian.uid` (M1868;
      the kit created five user NAMES, the code did not re-check the uids). One observer connection
      has an ABSOLUTE deadline for its request (`REQUEST_DEADLINE`, 30 s; each read waits for what is
      left of it, M1867), where each read had its own 30 s. The helper's reply timeout for the
      observer was a fixed 30 s while the observer streams the guest kernel and rootfs through
      SHA-256 (and creates the nonce's record before signing, so a timeout burned the nonce): it is
      now 30 s plus the artifacts' size at a floor of 25 MiB/s (M1869, M1870), derived rather than
      cached; a cache keyed by inode, mtime and size would be a measurement the observer did not
      take. The call site passing the derived value is not a row (a source-level pass-through whose
      kill would need a rootfs that really takes minutes to hash). The custodian's epoch is whatever
      the caller says (above): authority is the loop's scope pointer joined at intake.
    - **Rows (PSV; M1850-M1870, all ACTIVE, each killed by its own attack; M1871-M1879 unused).**
      M1531, M1532, M1533, M1536, M1537, M1538 and M1539 keep their meaning; their `old` text moved
      (the observer reads the profile manifest once and the pin) and was updated in the registry.
      Tests: `crates/axon-fabric/tests/observer_service.rs`, `tests/privileged_launcher.rs`, and unit
      tests in `custodian.rs`, `observer_service.rs` and `privileged_launcher.rs`. The fixtures that
      run the helper now give it a parent that IS the pinned Fabric program (a shell or python3
      running as the Fabric uid that forks the helper, or the test process itself): the old
      `exec`-from-root launches had no Fabric parent at all, which the pin refuses.
    - **Matrix.** A131-A134 (numbered after A126 here; A127-A130 are held by another branch, so
      `psv_matrix_check.py` on this branch alone reports exactly those four missing).
    - **Operator-visible change (PROTECTED_ONLY).** The helper config gains `fabric` (the kit
      computes it from the installed `axon-fabric` and refuses a mismatch); the custodian config
      gains `observer_uid`; the custodian socket unit gains `ExecStartPost=setfacl` (needs the `acl`
      package); the preflight records `helper-fabric-pin-path`, `helper-fabric-pin-sha256` and
      `custodian-observer-uid`. See `governance/notes/v022-observer-service-kit-delta.md`.

## Amendment 80: the build's own configuration is judged by structure, the host binaries record what built them, and the build record is its runner's (C9 round 5, buildenv)

80. **FIELD-ORIGIN BLOCKER (executed by the reviewer).** `scripts/guest_build_env.py` classified cargo's
    effective config by splitting the text cargo PRINTS. Cargo prints a `cfg(...)` target whose expression holds
    double quotes with SINGLE quotes (`target.'cfg(all(target_os="linux", target_env="musl"))'.linker`); the
    `strip('"')` left a stray quote, the table read as a harmless triple, and a committed `linker` under it
    linked the guest binaries (a probe linker ran) while the record said `foreign=[]`, `begin` accepted it and
    `shape_problems` returned `''`.
    - **After.** Classification is on the STRUCTURED config (`cargo config get --format json`, a nested object
      keyed by the exact key names; no dotted-string splitting anywhere). `COMMITTED_KEYS` is an exact allowlist
      (`target.wasm32-wasip1.rustflags`, `target.wasm32-unknown-unknown.rustflags`, the tree's own); every other
      key, in any table (`[build]`, `[env]`, `[source]`, `[patch]`, `[registries]`, `[net]`, `[http]`, `[profile]`,
      `[unstable]`, any `cfg(...)` or plain-triple target) and from any file, is refused. A tolerated key's triple
      must also not be a `cfg(...)` table or a triple the build compiles for; the host triple is `rustc -vV`'s,
      not a constant. A cargo that cannot print its config refuses the build. Any variable in the environment that
      is not the constructed one is refused (cargo's own "environment variables that may affect" note does not
      name `RUSTFLAGS` or `RUSTC_WRAPPER`). The `--show-origin` text is read only to record origins.
    - **Rows (killed by their own attack).** M730 (retargeted: the classification call), M1880 (the exact
      allowlist), M1881 (the cfg/compiled-triple guard), M1882 (environment refusal), M1883 (host triple),
      M1885 (cargo that cannot print its config). The attack is the committed-config form driven through
      `begin` (`a_committed_cargo_config_cannot_name_a_program_in_any_spelling`: the single-quoted and
      double-quoted cfg forms, plain host and musl triples, `[env]`, `[build]`, `[source]`, `[patch]`,
      `[registries]`, `[net]`, `[http]`, `[profile]`, `[unstable]`, a non-allowlisted wasm key, and an
      ancestor's cfg / `[env]` / `[source]`; control: the tree's own config begins with `foreign == []`).
    - **MAJOR-ADJACENT 1: host binaries (done).** The setuid `axon-protected-launcher`, the verifier, the
      custodian and the observer had no controlled build environment. `crates/axon-fabric/src/build_state.rs`
      (included by `build.rs`) reads what cargo hands the build script (`RUSTC_WRAPPER`,
      `RUSTC_WORKSPACE_WRAPPER`, `CARGO_ENCODED_RUSTFLAGS`, `RUSTC_LINKER`): any non-empty is recorded in the
      verifier identity (`build_state`, pinned field for field by `protected_verifier_ready.py`, which also
      refuses a non-empty one) and REFUSES a production build (release profile, no `test-trust-root` feature).
      The kit refuses a verifier whose `build_state` is non-empty and calls the new
      `guest_build_env.py check-host-build CLONE` (SUPERSEDED by amendment 86: judged a list of ambient
      variables, which the controlled host build now drops; it judges a constructed environment) -- the
      guest's classifier over the kit's OWN environment (no
      `CARGO_*` but `CARGO_HOME`/`CARGO_TARGET_DIR`, no `RUSTC*`/`RUSTFLAGS`/`RUSTUP_TOOLCHAIN`, no C-toolchain
      variable, and an effective config from the clone, its ancestors and `CARGO_HOME` that is only the committed
      one). Rows M1884 (CARGO_* refusal), M1897-M1899 (`refusal`, the workspace wrapper, `build.rs` stopping
      the build; tested through the REAL `build.rs` in a scratch package). **Operator-trusted, unchanged:** the
      `cargo`/`rustc` binaries themselves (the `rust-toolchain.toml` pin), `--locked`'s check of each crate
      against `Cargo.lock`, and a reused `CARGO_HOME`'s cached crates. `check-host-build` is made at DEPLOY time
      and cannot see an ancestor config that existed only during the build; `build_state` covers only the four
      variables above. The other three binaries are covered by the same `build.rs` run (one crate), but only
      the verifier reports it.
    - **MAJOR-ADJACENT 2: the build record is its runner's (done).** `begin` and `kernel` create a per-build
      random key (0400) in `<build parent>/keys` (the builder-private parent, 0700, never removed with the
      build directory), and every write of the record signs it: `proof = HMAC-SHA256(key, canonical record
      without the hmac)`, covering every field including artifact digests. `dist RECORD DIST` records the digest
      of every dist artifact and refuses a copy that is not the controlled step's output;
      `linux_profile_manifest.py` takes dist digests FROM THE RECORD and refuses a differing dist file or a
      record without the proof; `image_problems` (the freeze, and the kit before it installs anything) ends
      with the proof of the build and kernel records, after every structural judge, so each structural row is
      still killed by its own attack (the fixtures re-sign after an edit). The key must be a regular file
      (no symlink) owned by the builder, closed to others, in a directory likewise, under a parent only root or
      the builder can write. Rows M1886-M1896. **If the freeze runs in another process or host,** the
      `<parent>/keys/<id>.key` files must travel (or the freeze runs as root/the builder on the build host);
      otherwise it refuses ("cannot be checked"). **Residual trust: the builder account** (anyone who can
      read the key can sign any record), and the toolchain pin for the binaries' own bytes.
      **CORRECTED by amendment 86:** that statement was wider than stated and the proof, as built here,
      authenticated nothing. The judge took the builder's uid and key directory FROM THE RECORD it was
      judging, so anyone who could write a private key into ANY directory they owned (executed: uid 65534)
      signed a record the freeze accepted. After amendment 86 the residual is: whoever can act as the
      account the OPERATOR pinned (`/etc/axon/builder-pin.json`), or write into its private parent.
    - **Kit.** `operator_deploy_protected_host.sh` runs `shape_problems` and `image_problems` on the guest
      records before installing. `test_operator_deploy.sh` builds its synthetic image from properly signed
      records and refuses (by real kit runs) a verifier with a build state, `RUSTC_WRAPPER`/`RUSTFLAGS`/
      `CARGO_BUILD_RUSTC_WRAPPER` in the kit's environment, and a hand-written, edited or kernel-edited record.
      These three kit guards are not mutation rows (the harness drives cargo tests only). Each was removed
      ALONE by hand and `test_operator_deploy.sh` run (tree restored after each): without the `build_state`
      check, `ATTACK: a verifier built under a compiler wrapper` fails ("expected REFUSED (2), got 3"); without
      the `check-host-build` call, `ATTACK: a host build under RUSTC_WRAPPER` fails likewise; without the
      install-time record judge, `ATTACK: a guest image whose build record is a hand-written one (no builder
      proof) was accepted by the kit` fails. Each is a manual check on 2026-10-06, not a registered row.
    - **Tests never touch the real builder-private parent.** `guest_build_env.rs` runs every build under a
      per-process directory (`AXON_GUEST_BUILD_PARENT`, removed at exit) and `freeze_manifest.rs` keeps its
      fixture keys and build parent in a per-process directory; both fail (`ATTACK: a test ... real
      builder-private parent`) if a record or fixture path lies under `~/.cache/axon-guest-build`. Before this
      fix the tests left one proof key per build there (and the freeze fixture wrote fixed keys and set the
      directory's mode).
    - **FUTURE (not done).** `TrustAuthority::Admission` has no consumer (loop admitters come from store-config
      identities); `helper_agrees` does not join `artifacts_dir`, `firecracker`, `jailer`, `observer.root`
      and `observer.max_age_s` to the host config.
    - **Matrix.** A135-A137 (the integrator renumbers). **Operator deployment.** The installed verifier.json must be
      re-pinned (the verifier identity gained `build_state`); the guest image must be rebuilt (records now carry
      a proof and `dist`).

## Amendment 81: the evidence harness compares toolchains everywhere, the gate sees guards that are flags, and the freeze judges the status file (C9 round 4c, eqgate)

81. **Source: the round-5 EQUIVALENCE review (`DO_NOT_REGISTER`).** Two BLOCKERS and four MAJOR-ADJACENT
    findings, all executed. Mutation ids M1900-M1959 (60 of 60 used), matrix rows A138-A144 (the integrator
    renumbers; A127-A137 belong to other workstreams, so this branch alone fails `psv_matrix_check.py`'s
    contiguity rule, which was checked with placeholders for that gap: 144 rows, 538 citations, all resolve).
    - **BLOCKER 1: `--merge` never compared the toolchain across shards.** Only `v022_paired_disable.py
      --join` did. Two synthetic shards at a clean HEAD with different rustc and different interpreter
      digests merged to `all_killed: true`. Now ONE helper (`v022_g01_mutations.shard_toolchain_problem`)
      is the refusal, and `--merge`, `--join` and the partial `--only` write all call it (a drift test
      reads the three call sites). `--merge` compares rustc, cargo, LLVM, the interpreter's sha256 and
      the uid and `/etc/axon` presence the shard ran under; a mutation shard now records the host it ran
      on (`host_identity`, the same function as the paired-disable record's) and a shard that records
      none is refused, as a record was. Rows M1900-M1904; `a_merge_refuses_*` in `harness_integrity.rs`.
      The interpreter digest is compared as the review asked; a build that is not byte-reproducible
      across hosts will refuse a cross-host merge, which is the safe direction (state it, do not relax it).
    - **MINOR: the paired-disable `--only` write.** It kept earlier records on their stored `holds` label
      and never compared their toolchain. It now recomputes `holds` from each kept record's cells (the
      join's `record_derivable_holds`) and applies the same toolchain refusal to kept and new records
      together (`kept_records_problem`; M1905, M1906). A kept record from an older toolchain, or one
      with no host, is refused with "re-execute it": exactly what the old `--only` quietly allowed.
    - **BLOCKER 2: guards expressed as an OPEN FLAG had no row, no exemption and no test.** A refusal done
      by the kernel (`O_NOFOLLOW`, `O_EXCL`/`create_new(true)`, `RENAME_NOREPLACE`, `AT_SYMLINK_NOFOLLOW`)
      builds no `Err`, so no gate form saw it, and each could be removed alone with the root-run suite
      green (the review ran the helper's input and policy snapshot, the observer's key and artifact
      reads). **The class is fixed at the gate:** every use of such a flag in non-test in-scope code is a
      site (`OPEN_FLAG`: `O_NOFOLLOW`, `O_EXCL`, `create_new(true)`, `RENAME_NOREPLACE`,
      `AT_SYMLINK_NOFOLLOW`, `O_DIRECTORY`, `MS_NOSUID|NODEV|NOEXEC|RDONLY|BIND|PRIVATE|SLAVE|REC`),
      one alternative per source line so each is its own row (M1943-M1949). The sweep that derived the
      list covered every in-scope file for every `O_*`, `AT_*`, `MS_*`, `RENAME_*` and `create_new`
      token: no `MS_*` flag is used in in-scope Rust today (the form is there for the day one is).
      **Exposed: 19 uncovered flag sites** (of 32 flag lines swept; the others were already covered by an existing row; 56 new sites in all with forms (b)). Dispositions: 13 ROWED, each killed by its own attack (a
      symlink or an existing file at the shape the flag defeats): the helper's ownership-walk base
      (M1907), input snapshot symlink (M1908) and create_new (M1909), policy snapshot (M1910), operator
      file leaf (M1911), the hand-over's `fstatat` (M1912) and `fchownat` (M1913), the observer's key
      (M1914) and artifact measurement (M1915), the workspace store's no-clobber rename (M1916), the loop
      store's temporary create (M1917), `psv::prepare`'s policy (M1918) and `keygen` (M1919); 6
      carry a checkable exemption (counting the sites the dropped-or-covered lines share): the destination `O_NOFOLLOW` is dominated by the create_new on the line
      above (and `a_symlink_at_a_snapshot_destination_is_refused_by_create_new_alone` pins that open(2)
      fact), the hand-over's regular-file open is race-only in a root-private dir, a CSPRNG nonce
      (2^-128), a completion secret created in a directory created three lines up, the loop store's
      `nofollow()` (dominated by the `guard()` lstat walk M979/M980/M998; **classified explicitly, as the
      brief asked: its removal alone leaves axon-loop green because it only closes the guard-to-open
      window**), and two axon-os sites off the protected route (kill latch, staging dir); the other
      sites were already covered by existing rows. One existing exemption was DROPPED, not kept: the
      hand-over's `fstatat(.., AT_SYMLINK_NOFOLLOW)` had been exempted as "OS error", which hid the flag.
    - **MAJOR-ADJACENT (a): learning eligibility.** `learning_eligible` returns `Result<bool, _>` and
      `evo::propose` excludes by a `Some("reason")` chain, so neither was a site. Rows and attack tests:
      the corpus-role term for Confirmation/Reporting (M1926) and for mechanism_test (M1927), the
      verification-passed (M1928) and final-usage (M1929) conjuncts, and each of the four exclusion arms
      (M1930-M1933). **Stated, not hidden:** the `status == Completed` conjunct is redundant while
      `LoopEpisode::validate` refuses `verification passed` over another status; it is pinned by
      `a_passed_verification_requires_a_completed_episode`, not rowed (removing it changes no
      behaviour). The mechanism-test arm is dominated for the exclusion itself (the `!learning_eligible`
      arm excludes a mechanism-test episode too); its row's attack is the wrong recorded REASON, and it is
      counted as no more than that.
    - **MAJOR-ADJACENT (b): the gate's site forms.** Derived by sweeping the in-scope crates: a `Some("..")`
      or `Some(format!(..))` used as a value (not `== Some(..)`, `matches!`, an arm or `|`) is a site
      (M1950, M1951); a function returning `Result<bool, _>`, an `i32` or an `ExitCode` is a site of its
      own (M1952-M1954; `u8`, `u32` and `i64` returns found in scope are discriminants, a port read, uids and
      clocks, not statuses, and are not a form). **Exposed: 37 sites** (19 `Some(reason)` value lines, 18 functions). ROWED with their own attacks: the three
      `other_loop_role` arms (M1920-M1922) and the evaluator and subject arms of
      `issuer_independent` (M1923, M1924), the safety veto (M1925), the post-run suite check
      (changed, unreadable: M1934, M1935: no test touched them), `axon test`'s three failure arms
      (M1936-M1938), `create_once` (M1939), `reaches` (M1940), `type_matches` and `pattern_matches` (M1941,
      M1942). EXEMPT with a checkable fact: nine `axon-os` CLI functions returning `ExitCode` and its
      `Verdict::exit_code` (no crate outside axon-os names `axon_os::cli`, or calls `exit_code()` on its
      Verdict), `axon-vm`'s `exit_code`, the two `exit_code` mappings on the Err path (every arm a
      literal non-zero), the proposer arm of `issuer_independent` (dominated: EVL adds every arm proposer to
      `subject_issuers`, so the subject arm, M1924, refuses it first on any record the loop wrote), the
      assume-unchanged arm of readiness (dominated by the byte comparison, M451), the `Profile:` text.
      `schema.rs` `type_matches`/`pattern_matches` are covered at FUNCTION level by the rows on their
      integer and scheme arms; the other arms (`object`, `array`, `string`, `boolean`, `null`; the
      identifier, `acf1` and currency patterns) are exercised by the typed layer and are not individually
      rowed (the typed layer refuses first, M1217/M1233).
    - **MAJOR-ADJACENT (c): the freeze did not read the paired-disable status file.** It bound
      `paired_disable_digest` and checked nothing about its content, and the in-tree file has 59 of
      the retirement records, 49 at `aad46c05` and 10 at `c9647b35`, `all_hold` true: a freeze would have
      bound it. `paired_disable.status_problems` is now what the freeze asks, with one reason per defect:
      the file is the `--join`ed one (`hosts`, `toolchain`, no `shard`), schema, a clean tree, this tree's
      registry blobs, every retirement record present exactly once (the universe is GUARD_SETS plus
      STALE_REFACTORED), every record at the file's commit, `edits_sha256` the registry's, `holds`
      recomputed from the four cells and the full-suite cell (never the label), host and toolchain recorded
      and consistent, `consumer_selection` present and equal to what the rule selects now, `all_hold`.
      **The "freeze commit" is read as the evidence commit**: a status file is committed AFTER the run
      that made it, so equality with HEAD would refuse every real file; the file's commit must be HEAD, or an
      ancestor of it with only `governance/status/` changed since (anything else is stale evidence). The
      current in-tree file is refused: `the_partial_in_tree_status_file_is_refused` runs the validator
      on a snapshot of it. **No status file was written or faked.** Rows M1955-M1959 (the freeze's call, the
      missing-record, source-change, cells and not-joined arms); the other arms are tested over synthetic
      joined files (`a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit`, twelve
      defects and the toolchain mix) but not individually rowed, because the id range ran out.
      **What the freeze binds as mutation evidence today:** the registry's digest and counts only; it binds
      NO mutation-run status file, so a merged run's `all_killed` is not judged by it. The run's
      `all_killed` (recomputed by `--merge`, amendment 74) has to be checked by the integrator's own gate.
    - **MINOR: why 143 of 148 retirements have only a joint cell as evidence in the tree.** At this head only
      M1726 and M1267 have records of their own. That is expected until the final joined run, and it is
      the reason the freeze check above exists: a freeze made before that run is now refused rather than
      binding a partial file.
    - **MINOR: binary freshness is judged by file modification time** (`script_spawn::stale_against_sources`
      compares a named binary's mtime with the newest source cargo would rebuild it from): a stale binary
      touched newer passes. A size-and-digest comparison is NOT cheap here: it needs an expected value, and
      the only authority for "the binary this tree builds" is building it. The guard that does hold a
      digest is the harness's own: every mutation or paired-disable row ends on the interpreter the run
      started with, rebuilt and byte-compared (M888-M890). Not changed; stated.

## Amendment 82: claim text states what the code does (C9 round 5, claims)

82. **Wording and evidence accuracy; no code guard changes (operator decision 2026-10-05, PSV-3
    option "reword + delta note, non-empty guest ceiling OUT of scope"). Findings
    `/var/tmp/c9r5-findings-PSV-{2,3,4,5,6,7}.json`.** Every reworded sentence, before and after:
    - **PSV-3 (BLOCKER), certification scope.** Before (verdict spec): "Sealing, containment and
      per-provenance kernels run in the guest interpreter exactly as certified for the local path
      (`governance/proofs/v022-pci/CERTIFICATION.md`)." After: "... as certified at `31413ca7`
      (local backend, EMPTY effect ceiling) plus amendments 53/60/72/78 (the delta), each covered
      by named PCI gate rows and mutation rows re-run at the frozen head. A non-empty guest effect
      ceiling is OUTSIDE the PCI certification." (Amendment 84 replaced this "After": the gate
      rows did not exist for the four amendments, and "re-run at the frozen head" is a freeze
      obligation, not a present fact.) The delta, the gate-row results and the exact
      protected-profile behaviour under a non-empty ceiling are in
      `governance/notes/v022-pci-delta.md` (mutable; CERTIFICATION.md is untouched).
    - **PSV-3, completion.** Before: "a token per completed test", and the `interp::TestEnd` doc
      "Completed ... its every assertion ran". After: "completed" means the test body returned
      normally. An assertion inside a closure handed to the candidate runs only if the candidate
      calls it: executed by the reviewer, a suite closure the candidate never calls still yields a
      keyed PASS. Suites must assert after the call. The `TestEnd` comment says the same (comment
      only).
    - **PSV-3, key.** Before: "a fresh per-run key and a token per completed test ... produced and
      checked by the trusted runner INSIDE the guest." After: derived from a host-generated per-run
      secret (`submit.rs` draws it from the OS RNG; the guest enforces length, not freshness);
      tokens are issued by the guest interpreter and checked by both the guest runner and Fabric.
    - **PSV-3, fail-safe (not an A-row: nothing refuses it).** A candidate-printed second line
      naming a test makes `keyed_outcome` return `None`, so Failed or Passed becomes Unknown. It
      never produces a pass. Recorded under "Accepted, open" in the negative matrix.
    - **PSV-6 (BLOCKER 1).** Before: "Intake rejects a receipt whose observation digest or launch
      manifest digest does not join, or whose observation is stale, replayed or from another
      launch", and the matrix row "Stale or replayed observer evidence | Intake: observation
      nonce/epoch/age". After: freshness (`max_age`) and one-use are enforced at LAUNCH (Fabric's
      early check, the root helper, the custodian spend). Intake enforces the digest, launch
      (manifest digest, trial/attempt/operation, candidate) and epoch joins and deliberately does
      not expire a genuine verdict (section 9 already said so; the claim and the row never did).
      The "Replayed verdict or receipt" row, which also named intake, now says launch refuses and
      intake does not re-check. The "Fabric cannot mint it" and measured-vs-told wording belongs to
      `c9r4c/obsbind` and is not touched here.
    - **PSV-4.** Before: "Fabric attests a verdict only when it came from the protected guest
      path." After: Fabric attests a verdict as PROTECTED only when it came from the protected
      guest path, and signs other classes under their own label (`development` for a local,
      effect-free check; `guest-unobserved` for an unobserved guest verdict) (`attestation_decision`).
      The class is inside the signed receipt, so a signature never upgrades it. B263 currency is
      judged at dispatch (`qualification()` at selection) and again just before launch, NOT at
      signing: a record that lapses during a long run still yields a receipt naming the older
      record's digest. Stated as a limit; no code change. Also recorded: `derive` reads
      `result.json` a second time (`fs::read`) beside `interpret_linux_result`'s `read_regular`;
      `out_root` is operator-owned and root-written, so this is a defence-in-depth gap only.
    - **PSV-5, section 9.** Before: "the guest digests equal the qualification's". After: the
      producer-side wording of amendment 42 (above, section 9). Recorded precisely, no code change:
      (1) the EXECUTION leg of a protected trial rests on the Fabric execution attestation
      (`execution_document` signs the request and receipt digests; `execution_attestation_decision`)
      plus SHAPE checks: `evl::observed_protected_execution` requires the protected class and that
      `launch-manifest-sha256` and `preflight-observation-sha256` each appear once as hex, and no
      bundle is delivered for that leg, so those two digests are not joined loop-side to a manifest
      or observation; (2) `names_one_sha256` accepts uppercase hex while the verdict path
      (`is_sha256_hex`) requires lowercase. Accepted MINORs: the manifest's `backend_profile` is not
      joined to the receipt's `backend_profile_ref` (unreachable while `PROTECTED_PROFILES` has one
      element; drift test
      `protected_profiles_is_the_one_profile_the_launch_manifest_pins` added, no guard so no row);
      the manifest's `limits` and the receipt's `matched_checks` are not joined by the loop
      (`matched_checks` derives from test stdout the loop never sees; covered by the verifier
      signature only).
    - **PSV-2.** `axon-workspace-recipe`'s header named `axon-guest-init` as the digesting side; it
      is `axon-psv-runner` (`axon-guest-init` does not link the recipe). Header comment only.
      Recorded as availability-only: Fabric's `tree_version_ref` (used by `psv::prepare`) silently
      omits top-level `.git`/`.micode` and ignores empty directories, while the guest's
      `check_inputs` refuses both, so such a tree gets a manifest and then a guest refusal. The
      direction is safe (fails closed).
    - **PSV-7.** Fabric and readiness do NOT compare the B263 record's host to the running host:
      `accept_b263` (shared by `LinuxProfileConfig::qualification` and readiness) requires only a
      non-empty `host`; nothing compares `host_facts.machine_id` to `/etc/machine-id`. Only the
      operator deploy kit does, once, at install. So a current operator-signed record from host A
      installed by the operator on host B passes in code and readiness; no agent-reachable route to
      that install was found. The "possible follow-up" of amendment 56 is promoted to item N of
      `governance/specs/post-c9-keys-and-signatures.md`, together with two FUTURE items: the
      Stage 7 `b263_qualification` readiness component is a bare sha pin (redundant weak check,
      it cannot make Stage 7 READY alone), and the readiness relay script runs the operator
      verifier with an unscrubbed environment.
    - **Rows.** None added: no code guard changed. The one added test is a drift test over two
      constants. M1960-M1969 are unused.

83. **The dispatch rule: operator code never selects an operator impl by a type nothing on the
    operator side determined (C9 round 6, workstream r4c-psv1d, PSV-1; matrix A145-A148;
    M1990-M1997; supersedes the Dict non-claims of 53 (1), 72 part 2 and 78).**
    - **Why a rule, not another path.** Round 6 (`/var/tmp/c9r6-findings-PSV-1.json`, cases
      `/var/tmp/c9r6-psv1-logs/w/d1..e2`) found the third Dict bypass in three rounds: a candidate
      closure that captured the operator's dict retyped an entry against a snapshot taken at the
      hand-over (the operator wrote the key afterwards); and, as the stated non-claim, a suite that
      reads a candidate-written output dict or returned dict (`let out = dict_new(); solve(out);
      dict_get(out, "result").ok()`) lets the candidate choose the receiver's runtime type, so the
      operator's lenient `impl Judge for u8` runs. Every fix so far tried to make a CONTAINER
      remember what the operator put in it. The class is one level lower: an operator impl is
      selected by the runtime type of a receiver, and an untyped read gives the candidate that type.
    - **The rule (`Interp::seal_dispatch`, called from the one place the impl key is computed —
      `Expr::MethodCall` in `eval.rs`, beside `seal_method` of amendment 53).** In a sealed run
      (`seal.active`), an OPERATOR frame may not dispatch a method that two or more operator impl
      types define on a receiver the analysis did not determine. Refusal: `operator code
      dispatched `ok` on a value whose type nothing on the operator side determined (here `u8`) —
      the candidate would choose the impl; pin it with `let x: T = ...``. Outside a sealed run
      nothing changes (`seal.active` is false; the analysis is not even built). A method with ONE
      operator impl type has nothing to select between and dispatches (a wrong-typed receiver fails
      with "no method", it cannot pick another impl). The rule's arithmetic arm (`seal_width`)
      applies the same determination to `+ - * / % & | ^ << >>` on a fixed-width integer: a `u8`
      the candidate chose truncates where the operator's `i64` does not (`(v << 1) == 254` with
      255; `+`/`*` on a `u8` PANIC on overflow, so the shift is the result that completes).
    - **How "determined" is tracked (decision; rebuilt by amendment 88 — read 88 for the audit).**
      Not a runtime flag (a `Value` has no slot, and one on every scalar would cost every value
      copy) and not the checker's inferred types (unification can make a read of an untyped dict
      `i64` because a later use compares it with a literal — the runtime value is still the
      candidate's `u8`). A STATIC, name-keyed dataflow per OPERATOR fn (`interp/pin.rs`), built
      once for a sealed run, in which every "determined" rule is justified as **the operator chose
      this type**: a literal; a binding with a closed `let`/parameter annotation (the value was
      CAST to it, amendment 53); a binding initialised from a determined expression; the result of
      an OPERATOR fn with a closed declared return (cast at its return) — never a candidate fn,
      whatever it declares; a builtin whose declared return names no type variable (`dict_get`
      returns one, `len` does not); `x as T`; arithmetic on determined operands; a struct, array,
      tuple, `Option` or `Result` of determined parts; a field, element or match-binding of a
      determined value; a call of an operator method name every definition of which is the
      operator's and closed. Not determined: `dict_get`/`dict_values`/`recv`, an unannotated lambda
      parameter or result, a call of a local binding, a call of a candidate fn, a channel method,
      anything open. A name is determined only if EVERY binding and every assignment of it is (a
      greatest fixpoint). The result is the set of determined sites, keyed by (the fn that owns the
      site, a structural hash of the site), and the lookup is fail-closed: a site not in the set is
      undetermined. A receiver that is an operator-defined struct or enum at RUNTIME dispatches
      whatever the analysis says (sealed code cannot build one, E0004).
    - **Coverage** (each executed through the unit tests and, for the first two, the real runner):
      an output dict, a returned dict, an empty accumulator, a captured-closure dict, a channel
      receive, an `Option`/struct/tuple/generic-enum payload, an unannotated lambda parameter and
      result, a `match` on the untyped value before the dispatch, new keys, nested containers. Not a
      dispatch, left alone and tested: interpolation, `to_str` and comparison of an untyped read
      (the language has no operator overloading and no trait default methods; a `Trait::method(x)`
      path call is not syntax it accepts).
    - **The Dict snapshot is kept (defence in depth), with one fix.** It still guards what the rule
      does not: a held key retyped before the operator reads it through a builtin that behaves by
      runtime type, and the operator's PINNED reads (the refusal arrives at the edge, not at the
      read). The staleness the review executed is fixed at the source: the first sealed mutation
      since the last verification retakes the snapshot from what the dict holds NOW (before that
      mutation), so an operator write between the hand-over and the candidate closure's call is
      part of what the operator held. The rows M1660-M1681 and M1840-M1848 stand unchanged.
    - **What a suite author must now annotate.** Every untyped read the suite then dispatches on:
      `Some(x) => assert(x.ok())` becomes `Some(x) => { let y: i64 = x  assert(y.ok()) }`;
      `assert(c.recv().ok())` becomes `let r: i64 = c.recv()  assert(r.ok())`; a lambda
      `|x| x.ok()` becomes `|x: i64| x.ok()`; a helper `fn check<T: Judge>(x: T)` dispatching on
      `x` needs a closed parameter type. Calls of a fn with a closed declared return, literals,
      casts and annotated bindings need nothing. The annotation is a CAST (amendment 53): a
      candidate's `u8` at an `i64` pin is refused as a runtime type confusion, so the pinned suite
      keeps GOOD a keyed pass and WRONG a keyed failure.
    - **Honest-program impact, measured (this branch).** Sweeps (352 `.ax` diagnostics, 55
      `@[test]` files, example run exit codes): identical — no repository `.ax` runs sealed.
      Suites that DO run sealed and needed annotations: three real-runner tests in
      `crates/axon-psv/tests/sealed_frames.rs` (`a_dict_entry_the_operator_held_is_never_retyped_…`,
      `a_replaced_or_filled_position_is_judged_…`, `an_undetermined_type_position_never_selects_…`:
      dict reads, a channel receive, an unannotated `|x| x.ok()`), each annotated; the 19 unit
      tests of the OTHER seal layers observe their attacks through a dispatch on an untyped read —
      exactly what the rule now refuses first — and run with the rule OFF (a test-only switch,
      `DISPATCH_RULE_OFF`, set under `SEALED_DIRS_TEST_LOCK`, the paired-disable cell for the
      rule), so each layer is judged by its own attack; the rule's tests run with it ON. The axon-core
      suite, the axon-psv suite and (see report) the Fabric/Cortex/OS suites pass.
    - **Residual non-claim (smaller).** A value the operator reads untyped and uses ONLY through
      builtins that do not select an operator impl (printing, comparing, `to_str`) is the
      candidate's choice of value, which the suite compares or prints: that is output, not
      selection of the rubric. The analysis is conservative in one direction only: it can refuse an
      honest untyped dispatch (annotate it); it cannot admit an undetermined one except through a
      closed annotation, which is a cast.
    - **Rows.** M1990 (the refusal), M1991 (the two-impl scope), M1992-M1995 (the analysis: a
      receiver the analysis cannot determine, a type-variable builtin, an unannotated lambda
      parameter, a match binding), M1996 (arithmetic), M1997 (the snapshot retake). Matrix A145-A148
      (A149 and A150 were unused here; at integration obsbind2's rows took them, so the matrix is contiguous A1-A161 and FLOOR is 161). Tests: `operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict`,
      `operator_code_never_dispatches_on_a_value_from_any_untyped_position`,
      `a_determined_receiver_dispatches_and_so_does_an_unambiguous_method`,
      `operator_arithmetic_never_runs_at_a_width_the_candidate_chose`,
      `a_closure_that_captured_the_operators_dict_cannot_retype_after_an_operator_write`,
      `interpolation_and_comparison_of_an_untyped_read_select_no_operator_impl`, and through the
      real runner `operator_code_never_dispatches_on_an_untyped_read_and_a_pinned_suite_passes`.
    - **Native codegen.** Unchanged: the seal is interpreter-only (amendment 72).
    - **Operator deployment.** The guest image must be rebuilt.

## Amendment 84: the PCI delta is generated and gated, and four wordings that said more than the code (C9 round 6, claims2)

84. **Wording and evidence accuracy; no code guard changes (round-6 findings
    `/var/tmp/c9r6-findings-PSV-{3,4,5,6,7}.json`).** Every sentence changed, before and after:
    - **PSV-3 BLOCKER, "each covered by named PCI gate rows".** Before: amendments 53/60/72/78 "each
      covered by named PCI gate rows". False: `scripts/v022_pci_gates.sh` had 18 rows, all on surfaces
      1-21, none naming a test of any of the four. After: ten rows added (28 in all), one group per
      amendment (53, 60, 72 with its dict snapshot, 78), each with unit tests and a real-runner test
      (`axon_psv::runner::run`); the gate fails on an absent, renamed, filtered or `#[ignore]`d test
      (verified by renaming one). The dict rows are written against the current tests and labelled
      "psv1d may replace"; amendment 83's replacement of the snapshot extends them.
    - **PSV-3, the note was stale and hand-kept.** `governance/notes/v022-pci-delta.md` said amendment
      78 was "NOT IN THIS BASE" and named M1846 (which does not exist: M1840-M1845 and M1847-M1848),
      with totals measured at the old base and files and commits omitted (`lib.rs`, `parser.rs`,
      `ast.rs`, `error.rs`/E0505, `bae904b8`'s keyed failure token, `abf72e0b`, `73834357`). It is now
      GENERATED by `scripts/pci_delta.py` from `git log`/`git diff --numstat 31413ca7..<head>` over
      `crates/axon-core/src`, and `crates/axon-core/tests/pci_delta_note.rs` fails if the block drifts,
      if a commit is not classified, or if a later commit touches the interpreter. The phrase "not in
      this base" is removed from the note and from amendment 82.
    - **PSV-3, "re-run at the frozen head".** Now a FREEZE OBLIGATION: the freeze procedure must show a
      joined paired-disable run at the frozen head in which each named mutation row is killed by its own
      attack. It was never a present fact.
    - **PSV-4 (non-empty ceiling).** Added to PSV-4: a protected receipt under a non-empty guest effect
      ceiling is outside the PCI certification; the signed receipt names only the policy digest. The delta
      note adds the `backend::select` precondition: any restricting grant needs the signed B263 to show
      `x1_guest_policy_channel` PASS; a fully unrestricting grant needs no x1.
    - **PSV-7.** Before: "a receipt from any host without a current B263 qualification cannot satisfy
      the protected profile". After: "a receipt without a current operator-installed B263
      qualification", with host identity checked at install by the operator kit and not by Fabric or
      readiness (post-C9 item N). The Stage 7 `b263_qualification` bare-sha-pin note is in the claim.
    - **PSV-6 (a), the exe digest.** Before (verdict spec, amendment 79 (d), runbook): the digest of "the
      executable file the Fabric process was started from"; non-claim "`LD_PRELOAD`, ptrace";
      "`ptrace_scope` >= 2 closes the ptrace half"; "the helper serves only the Fabric program the
      operator pinned". After: the digest of the executable file the Fabric-uid process HAD when the
      helper opened `/proc/<ppid>/exe`; any Fabric-uid code can arrange that this is the pinned file (exec
      it after spawning the helper, `LD_PRELOAD`, ptrace). Round-6 reviewer, executed: a python3 Fabric-uid
      script that spawned `helper --observe` and then `execv`'d the pinned file got an observer-signed
      observation naming the pinned verifier in 18 of 20 attempts (the other 2 were refused before any
      nonce record), with no `LD_PRELOAD` or ptrace; Yama does not touch the exec route (the comfort
      sentence is dropped); the helper serves "a caller whose executable at that instant is the pinned
      file". This describes the code at this head; amendment 85 describes any later change to when the
      helper measures it, and the custodian's request deadline (the round-6 reviewer executed a drip
      client holding the single-threaded custodian for 131 s; that is availability, fail-closed).
    - **PSV-6 (b), (c).** "Fabric and Firecracker revisions" is "Fabric revision and Firecracker digest"
      (`PreflightObservation` has `firecracker_sha256`). "The custodian's: the nonce, issued for that
      epoch, observed once" is "issued for that epoch and unspent (custodian), observed once (observer)":
      the one-observation record is the observer's `.observed` file. Also in amendment 79's table.
    - **PSV-5 gap-map.** Rows 63 and 65 of `v022-psv-gap-map.md` are marked HISTORICAL (superseded by M4
      row 96 and amendments 42 and 82).
    - **PSV-4 minor, not done.** Passing the interpreted `result.json` bytes into `derive` (instead of a
      second read) was not trivial and is left; the second read is defence in depth over an
      operator-owned root-written directory.
    - **Runbook.** Fabric's `<state>/runs/` leftovers after a crash are documented as operator cleanup
      (no sweep exists).
    - **Rows.** None added; no code guard changed. M2040-M2049 unused.

## Amendment 85: the helper's Fabric pin is a guard against mistakes, and the reply pipe has one reader (C9 round 6, obsbind2)

85. **Round-6 PSV-6 (DO_NOT_REGISTER): the exec race, the custodian's deadline, and what the
    revision is.**
    - **Before: the executed exec race.** Amendment 79 said the helper "serves only the Fabric
      program the operator pinned". `running_caller()` hashes `/proc/<ppid>/exe` at the moment it is
      opened and re-asks the pidfd only whether the parent is still the parent and alive, which an
      `execve` does not change. EXECUTED by the round-6 reviewer (real setuid helper, real observer
      and custodian, pin = `/usr/bin/sleep`): a python3 running as the Fabric uid forks a worker
      that holds the read end of a pipe, spawns `helper --observe` with that pipe as its stdout and
      an attacker-authored manifest on stdin, and `execv`s the pinned file; 18 of 20 attempts got
      an observer-signed observation naming the pinned verifier, which the worker read. No
      `LD_PRELOAD` and no ptrace: amendment 79's non-claim named the wrong mechanisms, and its
      remark that `ptrace_scope` >= 2 narrows the problem does not touch this route. Launches pass
      the same gate, so the race applied there too.
    - **After: the reply pipe has one reader.** `reply_channel_private` runs inside `authenticated()`
      (the one gate; a production helper always, a test helper when its config says
      `private_reply_channel`, because most fixtures put a harness between the helper and the
      reader). It requires that no process except the helper and its parent holds the pipe the
      reply is written to, found by walking every process's `/proc/<pid>/fd` for the pipe's
      `(dev, ino)`; a production helper also refuses a stdout that is not a pipe (a file or
      terminal is readable by every process of the uid, so there is no holder to count). The
      attack's worker holds the read end and is refused: 20 of 20. Control in the same test: the
      pinned program itself (python3), running the helper and reading its own pipe, is served.
      A holder that is another child of the parent mid-spawn (a pipe end its `exec` has not yet
      closed) is waited out for 300 ms; the attack's worker does not go away. Fabric's own spawns
      (the launch and the relay) use pipes and are both the parent and the reader.
    - **Rows (killed by their own attacks):** M2050 (the rule), M2051 (a production stdout must be a
      pipe), M2053 (the scan finds every holder), M2054 (the gate applies the rule to every request).
      Matrix A149, A151.
    - **What this does NOT close, and why that is inherent.** A same-uid process can always obtain
      what a same-uid process holds: it can reopen the pipe's other end through
      `/proc/<pid>/fd/N` of the parent (a pipe can be reopened that way), receive the descriptor in
      flight (`SCM_RIGHTS`, which no descriptor table shows), `pidfd_getfd` it (Yama >= 2 or
      `CAP_SYS_PTRACE` stops that one), `ptrace` the genuine program, or `LD_PRELOAD` it. So **the
      Fabric pin is a configuration and mistake guard (the wrong binary, a stale install, an
      agent run by hand as the Fabric uid, the demonstrated exec race), not a defence against
      malicious same-uid code**, and `verifier_sha256` names the executable file the Fabric-uid
      process had when the helper opened `/proc/<ppid>/exe`. The guest verdict's integrity does not
      rest on it (the hidden check, the signature chain and the loop's joins).
      **Considered and not done.** Re-measuring the parent's executable after the reply: it detects
      an exec that happens between request and reply and nothing else, and the attack execs before
      the first measurement; no deterministic attack exists to kill a row with, so it is not added.
      `/proc/<pid>/stat` `starttime` does not change on exec. An `AF_UNIX` socketpair with
      `SO_PASSPIDFD` identifies the SENDER of each message, not the holder of the other end, so the
      forked worker would hold an end all the same; a socket cannot be reopened from `/proc`, which
      would close one of the routes above, at the cost of changing both Fabric spawn sites; left
      to a design pass if the operator wants the pin to mean more.
    - **Major-adjacent: the custodian's absolute deadline.** EXECUTED: a Fabric-uid peer dripping one
      byte every 20 s held the single-threaded custodian for 131 s (bound 4096 x 29 s), stalling every
      issue, check and spend. `Server::serve_one` now reads under the observer's absolute-deadline
      loop (`REQUEST_DEADLINE`, 30 s, `serve_one_within`; M2052; matrix A150).
    - **Minor: what the revision is.** `fabric_revision` was classed PINNED. It is the build revision
      the installed binary states about itself (`axon-fabric verifier-manifest`), run as root by the
      kit from a caller-supplied `--bin-dir` and written into the helper config: the operator's
      kit's word about the file, not a measurement of it (FIELD-ORIGIN: it is told). It is
      reclassified here as **TOLD by the operator's kit** (the digest it sits beside is pinned and
      measured; the revision is not independent of it). Deriving it without executing the binary
      (a revision file written by the controlled build beside the binary and covered by the pin) is
      the buildenv change in amendment 80's lane and is not made here.
    - **Also corrected.** The observation names a Firecracker SHA-256, not a revision; the one-
      observation-per-nonce record is the observer's, while the custodian's state is issued,
      unspent, expired, spent.
    - **Operator-visible change.** None in configs (the new `private_reply_channel` key is test-trust
      only; a production helper is always strict). Anything that runs a PRODUCTION helper must hand
      it a pipe as stdout and read it itself (Fabric does).

## Amendment 86: the proof is the OPERATOR's pinned builder's, and the host binaries come from a controlled build (C9 round 6, buildenv2)

86. **FIELD-ORIGIN, two executed BLOCKERS and a major-adjacent.** (The round-5 `cfg(...)` fix holds: the
    reviewer's battery refused every spelling.)
    - **Blocker 1: the build-record proof authenticated nothing.** `proof_problems` found the key under
      `rec["build_parent"]`, owned by `rec["builder_uid"]`, both read from the record being judged; the
      ancestor and ownership checks accepted whichever uid the record named. Executed as root: a record naming
      uid 65534, a private directory owned by 65534, attacker-chosen binary digests and host tools copied from
      the real toolchain, signed under a key in that directory, passed `shape_problems` and `image_problems`
      (the freeze's and the kit's judge). **Design.** Who the builder is, and where its private parent lives,
      is the OPERATOR's word: `/etc/axon/builder-pin.json` (`axon-builder-pin/1`: `{uid, parent}`), read
      through the same root-owned-chain walk as the toolchain pin (`operator_file_problem`), installed by the
      kit's `toolchain` step from `--builder-uid N --builder-parent DIR`, and REQUIRED by the freeze
      (`image_problems(builder=None)` reads it; absent or not the operator's means refused). The key is
      looked up under THE PIN'S parent, must be owned by THE PIN'S uid, in a directory likewise, under
      ancestors only root or that uid can write; the record's own `builder_uid` and `build_parent` are never
      used to find it (so a forged record finds no key). The runner-side judge (`dist_problems`, run by the
      account that built) uses the judging process's own euid and `AXON_GUEST_BUILD_PARENT`. A pin file, not
      the judge's euid, because freeze, kit and build need not be one account (the freeze runs as root in a
      private mount namespace in the tests, and the operator's builder is an unprivileged account).
      **CORRECTED by amendment 90:** the proof's key was readable by every process the build ran (any build script could mint a proof), and "a planted `cc` never exists" (below) did not hold: the planted `cc` was written into the builder-owned toolchain directory that was first on the build's `PATH`. **Residual trust, stated correctly (as of amendment 86; see 90):** the proof is exactly as strong as the pin: whoever can act as the
      pinned account, or write into its private parent, can sign any record; and root on the judging host.
    - **Blocker 2: the host-binary refusal was defeated by a retry in the same target dir.** `build_state`
      is read only by axon-fabric's `build.rs`, which exits 1 under `RUSTC_WRAPPER` after cargo has compiled
      its dependencies under the wrapper (91 wrapped rustc calls, incl. `axon_loop_contracts`, `axon_psv`,
      `ring`, `sha2`); the same command without the wrapper compiled only axon-fabric, axon-vm, axon-loop
      and linked the wrapped rlibs; `verifier-manifest` then said `build_state=''`. **Design.** The host
      binaries are built by a CONTROLLED step, `python3 scripts/guest_build_env.py host-build OUTDIR`: a fresh
      STANDALONE clone of the tree's HEAD (so the build's own provenance is measured on a real clone), a
      fresh EMPTY target dir and `CARGO_HOME` it creates (`mkdir`, which fails on an existing one), the
      constructed environment (cleared; `PATH` = the pinned toolchain's bin dir and `/usr/bin:/bin`, the
      pinned `rustc`; nothing else inherited, so a caller's wrapper, flags, `AR_*`, `RANLIB_*`, `CARGO`,
      `RUSTUP_HOME` and a planted `cc` never exist), cargo's effective config checked before and after, ONE
      fixed invocation `cargo build --release --locked -p axon-fabric --bins --quiet`. It copies the four
      binaries (`axon-fabric`, `axon-protected-launcher`, `axon-custodian`, `axon-observer`) and writes
      `OUTDIR/host-build.json`, SIGNED by the builder (blocker 1's mechanism). The kit REFUSES a `--bin-dir`
      without such a record, a binary that is not what the record names, a record the pinned builder did not
      sign, extra files, another revision, a missing `--builder-uid/--builder-parent`; it copies the directory
      aside first so what is judged is what is installed. A retry shares nothing with an earlier build.
      **Record format** (`axon-host-build/1`, for the revision plumbing of obsbind2): `{schema, controlled,
      source_revision: <40 hex, HEAD of the fresh clone>, artifacts: {<binary>: <sha256>} for exactly the four
      binaries, toolchain: {channel, cargo, cargo_sha256, rustc, rustc_sha256, rustc_vV, host_tools: {cc, ld:
      {path, realpath, sha256, version}}}, env, builder_uid, build_parent, build_parent_ancestors, src_dir,
      cargo_home, target_dir (both `*_created_empty`), effective_config, builds: [{name:
      "axon-fabric-host", args, rustflags: null, config_before, config_after}], proof: {schema, id, hmac}}`.
      The judge is `guest_build_env.host_record_problems(dir, (uid, parent), commit)`; the revision to pin
      into the helper config can be read from `source_revision` of a record that judge accepts, instead of
      from the installed binary's self-report. The linker (`cc`, `ld`) the host build recorded is compared by
      the kit to the guest build's recorded tools and, when installed, the operator's toolchain pin.
    - **Major-adjacent: `check-host-build` judged a LIST of ambient variables** (it accepted `AR_*`,
      `RANLIB_*`, `RUSTUP_HOME`, `CARGO`, `RUST_TARGET_PATH`, `LIBRARY_PATH`, `CRATE_CC_NO_DEFAULTS` and an
      attacker directory first on `PATH`; a planted `cc` ran during an axon-fabric rebuild). It now judges
      cargo's effective config in a CONSTRUCTED environment (fixed PATH, fresh empty `CARGO_HOME`) from the
      clone, ancestors included; the ambient environment is not consulted, because the controlled host build
      drops it. Defence in depth, not the barrier. `build_state` stays in the verifier identity and in the
      kit's self-report check; a controlled build never has any, so that kit check is DOMINATED by the record
      check and has no test of its own (stated, not claimed).
    - **Field origins (tables of amendments 79/80, corrected).** The proof key's parent and the builder uid:
      THE OPERATOR'S (pin). The host binaries' digests and `source_revision`: the controlled host build's
      signed record. The helper config's `fabric.revision`: still the installed binary's own report read as
      root by the kit, so TOLD, not PINNED, until obsbind2 takes it from the signed record's
      `source_revision` (the amendment 79 row classing it PINNED (M1850) should read: told by the kit,
      cross-checked against the signed record by `check-host-record --commit`). The host-toolchain pin's
      `cc`/`ld` are now also compared to the host build's recorded linker (kit, `binaries` step).
    - **Rows (killed by their own attack).** M2080 (key under the pinned parent/uid, never the record's),
      M2081 (the pin is an operator file), M2082-M2088 (the host record: one invocation, revision, exact
      binary names, each digest, no symlink, no extra file, the builder's proof), M2089 (host build env
      constructed, not the caller's), M2090 (fixed PATH: the planted `cc`), M2091-M2092 (key and its
      directory owned by the pinned uid), M1884 retargeted (the host config check applies the classifier),
      M1184/M1185/M1893-M1895 re-pointed at the changed text. Not rows (dominated by a sibling that also
      refuses, so a mutation is REFUSED_ELSEWHERE): the pin's schema/shape check, a fresh target dir (`mkdir`
      refuses an existing one), an existing OUTDIR (`makedirs` refuses it too). Kit guards (not rows: the
      harness drives cargo tests), each removed ALONE by hand and `test_operator_deploy.sh` run
      (2026-10-06, tree restored after): the `check-host-record` call fails `ATTACK: a host build record
      edited after its builder signed it` (expected REFUSED (2), got 3); the linker comparison fails `ATTACK:
      a host build whose linker is not the one the guest build recorded` (got 0); the config-check
      refusal fails `ATTACK: an ancestor cargo config naming a compiler wrapper` (got 3). The kit's
      required `--builder-uid` is DOMINATED (without it the next step still refuses an empty uid): stated,
      not claimed as a kill.
    - **Matrix.** A152-A155 (assigned A154-A157; renumbered at integration). **Operator deployment.** Install the builder pin (the
      kit's `toolchain` step does, from `--builder-uid/--builder-parent`); build the host binaries with
      `host-build` as the builder and pass THAT directory as `--bin-dir` (a plain `cargo build` output is
      refused); re-pin `verifier.json`; the guest image records are unchanged in format (the builder pin is
      what they are judged against).

## Amendment 87: guards expressed as a permission mode or a process flag are sites, and the freeze judges the mutation run (C9 round 6, eqgate2)

87. **Source: the round-6 EQUIVALENCE review (`DO_NOT_REGISTER`).** One BLOCKER and one MAJOR-ADJACENT,
    executed. Mutation ids M2120-M2169 (M2151 withdrawn, see below), matrix rows A156-A161 (assigned A158-A163; renumbered at
    integration).
    - **BLOCKER: a guard expressed as a permission MODE or a prctl FLAG was invisible to the gate.** The
      review weakened four, one at a time, as root, with the full suites green: the completion secret's
      `0o400` -> `0o644` (743 passed), the trial-cache root's `0o700` -> `0o755` (743), the child's
      `PR_SET_NO_NEW_PRIVS` under `if false` (790), and `PR_SET_PDEATHSIG` deleted (790). The only exemptions
      near the prctl calls covered the `!= 0` error branch, not the call.
    - **The class is fixed at the gate (`PRIV_FORM`).** Derived by sweeping every in-scope file for the
      permission and syscall vocabulary: mode literals (`.mode(0o..)`), `set_permissions`/`from_mode`/`set_mode`,
      the chmod and chown families, `mkdirat`, `umask`, `prctl`, `setrlimit`, `setsid`/`setpgid`/`process_group`, the
      setuid family and `setgroups`, `signal`/`sigaction`/`sigprocmask`/`kill`/`killpg`, `fcntl`, `pre_exec`, and
      namespace/mount/capability/seccomp calls. A use in non-test code is a site unless a row's edit changes its
      block. One source line per group, each its own gate row (M2120-M2131), with one planted-form test
      naming a member of every group and refusing to name a mode READ, a definition or a comment. The sweep found
      no `unshare`, `clone`, `mount`, `capset` or `chroot` call in scope today; those alternatives exist for the
      day one appears.
    - **Exposed: 41 uncovered sites, plus 7 that only a weak "OS error" exemption covered. ROWED, each killed by a test whose attack is the weaker mode or the missing
      call, observed through `stat` or `/proc`: the completion secret 0400 (M2144), the Fabric-private inputs
      dir 0700 (M2145), the keygen key 0400 (M2146), the trial-cache root 0700 (M2147), a read-only
      materialization's directory and file modes (M2148-M2150), the check child's pre_exec hook,
      `no_new_privs`, the parent-death signal, and the setgroups/setgid/setuid drop (M2152-M2157, observed in the
      child's own `/proc/self/status`), the root helper's snapshot modes and out-tree hand-over (M2158-M2166),
      the launcher's rlimits (M2167) and the helper's `setgroups`/`setresgid` (M2168, M2169).
      One existing exemption was DROPPED, not kept: the runner's `prctl`/`setgroups` exemptions said "OS error"
      and covered the calls. The gate rule "exempt yet covered" was narrowed to an exemption pinned on the
      site's OWN line, so the hand-over's regular-file `openat` (race-only, exempt on its own line) can sit
      beside the rowed `fchmod`/`fchown` of the same arm.
    - **Remainder, stated (do not read as covered).** Every exemption that begins `REMAINDER` is a guard no
      test observes yet, not a claim of domination: `grep -n REMAINDER scripts/v022_refusal_coverage.py`.
      They are: the guest PID 1's `PR_SET_NO_NEW_PRIVS`, seccomp, signal forwarding (they need a real boot,
      `psv_guest_boot_test.sh`, which this workstream did not run); the root helper's and the PSV runner's
      `PR_SET_DUMPABLE` (not observable from outside on a default host: a setuid exec leaves the process
      non-dumpable under `fs.suid_dumpable=0` and the next exec resets the bit; core dumps are also stopped by the
      rowed `RLIMIT_CORE=0`); the unprivileged fallbacks and the kernel-default limits of `lim2`; `F_SETOWN`;
      the read-only-tree unlock in `remove_tree` (row M2151 survived as root, which bypasses modes, and was
      withdrawn rather than weakened); the `PR_GET_NO_NEW_PRIVS` query. Three more are dominated or off the
      route and say so (the lease take, whose absence the `F_GETLEASE` re-read refuses; `pre_exec(exec.run())`,
      whose closure is M527/M528; the axon-os and Cortex local-executor paths, `--dev` custodian).
    - **MAJOR-ADJACENT: the freeze bound no mutation-run status file.** It bound the registry's digest and
      counts, so a freeze could be cut with no merged run at HEAD and `all_killed` was never judged.
      `v022_mutation_status.problems` is now what the freeze asks of
      `governance/status/v022-psv-mutation-run.json` (the file `--merge` writes over `--scope=all`), one reason per
      defect: absent; not `--merge`'s (`merged_from`, no `shard` or `only`); another scope; a commit that is neither
      the freeze commit nor an ancestor with only `governance/status/` changed since (ONE rule, shared with
      paired-disable's validator); a dirty tree; other registry blobs; shards that disagree on toolchain,
      interpreter digest or uid, or record no host; any active row missing, duplicated or run with another edit;
      any row not GOOD by `row_good` recomputed from its recorded baseline and result, named by class
      (`REFUSED_ELSEWHERE`, `SURVIVED`, stale, baseline, interpreter); a LIBRARY_PRIMITIVE row not flagged as the
      registry classifies it; and an `all_killed` label that is false or true over rows that are not. LIBRARY_PRIMITIVE
      rows are counted apart (`counts()`; the freeze manifest records `killed_library_primitive_rows_not_counted`
      beside `killed_active_rows`). `--check-status PATH` is the same function on a command line. Rows
      M2132-M2143. **No status file was written or faked**; neither in-tree status file is accepted (the
      paired-disable file is partial; the mutation run does not exist).
    - **What the status files are, honestly (SENTINEL).** `status_problems` and `problems` are SELF-CONSISTENCY
      checks. A forged joined file with every record fabricated passes with 0 defects, because nothing binds a file
      to a run: the harness is run by the operator's own (freeze) account and its output is operator-attested. The
      freeze check detects staleness, partial or mixed shards, a label contradicting its cells, and internal
      inconsistency; it does NOT detect forgery by the account that runs the freeze. No cheap SOUND strengthening
      exists without a key: a per-row digest or a nonce chain over fields the same account writes proves
      nothing the account cannot also forge. A strengthening that is sound needs a signature by a key the
      freeze account does not hold (the operator qualification key, which no agent signs with); it is left
      stated, not implemented (matrix row A161).
    - **MINOR: no joined paired-disable record at this head.** Unchanged and expected until the final evidence
      run; the paired-disable validator (amendment 81) refuses a freeze until it exists.

88. **The dispatch analysis trusts only what the operator chose (C9 round 7, workstream r4c-psv1e,
    PSV-1 + SENTINEL; matrix A162-A167; M2171-M2178; amends 83).**
    - **Findings.** Three executed BLOCKERS, all soundness bugs of `interp/pin.rs` in which
      "determined" over-approximated: (B1) a CANDIDATE-declared type counted — every fn of the
      program, the candidate's included, was in the closed-return table, so `solve(3).ok()` with the
      candidate's `-> u8`, `let p: P = solve(); p.x.ok()` with a candidate `type P = { x: u8 }`, and
      `(solve(3) << 1) == 254` were determined; (B2) a local binding named like an operator fn was
      judged by the global's closed return (`Some(f) => f().ok()` with the candidate's `|| 4 as
      u8`); (B3, SENTINEL) `closed()` accepted any `Named` type, a TRAIT name included
      (`let y: Judge = v; y.ok()`). Adjacent: verdict keys were receiver TEXT across the whole
      program; the candidate's methods steered the operator's method table; the pin cache was keyed
      by a heap address; honest operator-only `dyn`/generic helpers had no annotation that fixed
      them; an operator method named `recv` made a channel read look determined.
    - **Audit of every rule (the question for each: did the OPERATOR choose this type?).**
      Literal/`None`/`Some`/`Ok`/`Err`/containers of determined parts: yes. A closed annotation: the
      value is cast to it, and "closed" now means only scalars, `Dict`, operator structs, enums and
      refinements WITH CLOSED FIELDS (recursively, generics substituted by their closed arguments),
      and `Option`/`Result`/`[T]`/tuple/`Chan` of closed — NOT a trait name, `dyn`, a type
      parameter, `Self`, a type a sealed module defines, or any unknown name (a trait or a
      candidate type does not constrain the concrete runtime type to one the operator chose).
      A call of a FREE fn: only an operator fn, only when it is not shadowed by a local binding of
      the same name, and only when its declared return is closed. A method call: only a method
      name every definition of which is the OPERATOR's (`impls` already was) and closed, and never
      a channel method name. Module-level lets: operator lets only (a sealed `let` is never admitted, so a
      candidate global is undetermined). A `match`/`while let` binding and an assignment target: as their
      source. A lambda parameter: only a closed annotation. A `for` variable: `i64`.
      Cast (`as T`) and builtin returns: the operator named the type.
    - **Fixes at the source.** `Pins::build` takes the sealed-span predicate and builds from
      OPERATOR items only: a sealed fn is in no table, a sealed type/enum/refinement is OPEN, a
      sealed `let` is no determined global (M2171, M2172, M2178). A call whose callee name is bound
      anywhere in the fn is undetermined and the global tables are consulted for unbound names
      only (M2173). `Tys::closed` as above (M2174 trait names, M2175 `dyn`). Keys carry the owner:
      `(FnDef address, site hash)`; within one fn equal text means equal names and so an equal
      verdict, across fns the keys never meet, so a refusal cannot depend on an unrelated fn nor a
      sibling's pin admit another's site (M2176); the owner at run time is `Interp::pin_fn`, set
      by `call_fn_frame`, and a lambda body is owned by its CREATOR (the closure carries the
      creator's address under `PIN_FN_MARK` in its capture cell, the same mechanism as the sealed
      mark). The address-keyed `pin_cache` is REMOVED (lookup is a hash-set probe on the
      structural key: deterministic, nothing to go stale). The lookup is FAIL-CLOSED (a site the
      analysis did not record as determined is undetermined, so a global initialiser or anything
      unanalysed refuses). Channel method names are never determined (M2177).
    - **Operator-only polymorphism (adjacent d), design.** Two options were weighed: an
      interprocedural "every call site's argument is determined" analysis (large, and a lambda or
      a dict-reachable call site defeats it), or admitting dispatch when the receiver's RUNTIME
      value is an operator-defined struct or enum. The second is sound because the candidate
      cannot create such a value: a sealed module naming an operator type is refused (E0004; the
      `sametype` case of the round-7 logs is E0002/E0004), and an operator struct a candidate
      merely PASSES BACK is the operator's own value, whose impl the operator chose. It refuses
      exactly what matters: a receiver whose runtime type is a builtin scalar or sized integer,
      which the candidate can fabricate. Adopted (`Interp::seal_dispatch`, `Pins::is_operator_type`):
      `fn total(a: dyn Shape, b: dyn Shape)` and `fn total<T: Shape, U: Shape>(a: T, b: U)` over the
      operator's own structs pass; the same helper given a candidate-stored `u8` is refused
      (`operator_only_polymorphism_dispatches_and_a_candidate_influenced_one_does_not`).
    - **Honest-program impact.** A call of a CANDIDATE fn is never determined, whatever it
      declares (round-7 case `honest_i64`: `assert(solve(3).ok())` with a candidate `-> i64` is now
      refused). The annotation the suite author adds is the operator's pin, and it keeps working:
      `let r: i64 = solve(3)  assert(r.ok())` (GOOD passes, WRONG fails, a candidate `-> u8` is
      refused by the cast). Sweeps (352 `.ax` diagnostics, 55 `@[test]` files, example exit codes)
      are identical; no repository `.ax` runs sealed.
    - **Round-5/6/7 cases.** Every case of `/var/tmp/c9r{5,6,7}-psv1-logs/w` through the exact
      `exec_axon_test` invocation: attacks refused, `dyn_honest`/`generic_honest`/`honest_annot` pass.
    - **Non-claims, now.** The Dict non-claims of 53 (1), 72 part 2 and 78 stay reduced to OUTPUT
      only (amendment 83). The rule is conservative in one direction (it can refuse an honest
      untyped dispatch; annotate it) and admits an undetermined receiver only through an operator
      annotation (a cast), an operator-defined runtime value, or a determined builtin. A
      comparison/`to_str`/interpolation selects no impl and is outside the rule. Open by design: a
      method the operator calls that exists once (nothing to select between); a trait DEFAULT
      method does not parse in this language (`traitdefault` of the round-7 logs is a parse error).
    - **Rows.** M2171 (a candidate fn in the closed-return table), M2172 (a candidate type closed),
      M2173 (local-name shadowing), M2174 (a trait name closed), M2175 (`dyn` closed), M2176
      (verdicts shared across fns), M2177 (channel method names), M2178 (a candidate global).
      The rules of `pin.rs` whose buggy form is only a REDUNDANT second guard (unknown names open,
      a type parameter open) have no row: no attack separates them. M1990-M1997 re-anchored (the
      key and the lookup changed). Matrix A162-A167. Tests: `a_type_the_candidate_declared_does_not_determine_the_receiver`,
      `a_local_binding_named_like_an_operator_fn_is_not_judged_by_it`,
      `a_trait_annotation_does_not_pin_the_runtime_type`,
      `a_verdict_belongs_to_its_own_fn_and_the_candidate_steers_none`,
      `operator_only_polymorphism_dispatches_and_a_candidate_influenced_one_does_not`,
      `an_operator_method_named_like_a_channel_method_does_not_determine_a_recv`,
      `a_candidates_global_and_a_sibling_fns_site_determine_nothing`, and through the real runner
      `operator_code_never_dispatches_on_a_type_the_candidate_declared`.
    - **Native codegen.** Unchanged: the seal is interpreter-only (amendment 72).
    - **Operator deployment.** The guest image must be rebuilt.

## Amendment 89: gate rows for every verified arm, the PSV-6 paragraph states what amendment 85 did, and the row count is derived (C9 round 7, claims3)

89. **Wording and evidence accuracy; no interpreter or guard code changed (two comment-only row-id
    fixes below). Findings `/var/tmp/c9r7-findings-PSV-3.json`, `-SENTINEL.json`, `-PSV-6.json`.**
    - **PSV-3 BLOCKER, the row count.** The claim said "28 rows"; the script had 30. The number is
      dropped from the claim, and `scripts/pci_delta.py --check` (run by
      `pci_delta_note::the_pci_delta_note_is_what_git_says`) now fails if the delta note quotes a gate
      row count other than the script's, or if the verdict spec hand-types one.
    - **PSV-3 MAJOR-ADJACENT, arms the gate did not discriminate.** Four rows added (34 in all), each
      naming an existing test, each VERIFIED by mutating the arm in the tree and running the named
      test (the test FAILED; the tree was restored): (i) the `()` coercion of an absent return type at
      a crossing, `a_fn_with_no_declared_return_type_hands_the_operator_unit`; (ii) channel stamping
      at creation (`chan_created`'s closed-type filter), `a_channel_carries_the_element_type_its_creation_states`;
      (iii) strict closure arguments at a crossing (`closure_args_check`),
      `an_operator_closure_called_from_sealed_code_takes_only_determined_arguments`; (iv) the am83
      arithmetic arm (`undetermined_arith`), `operator_arithmetic_never_runs_at_a_width_the_candidate_chose`.
      (v) The closure arm of `replaced_ok` (am78): removing it fails exactly one test, the am72
      dict-snapshot row's `a_dict_the_candidate_mutated_is_verified_at_every_edge_back`, and no am78 row;
      no test of the am78 group isolates it, and adding one would change interpreter tests that psv1e is
      editing. Stated as it is: that arm's gate coverage is the dict-snapshot row; its mutation row is
      M1673. The PSV-3 claim now says principal arms by gate rows, lists the verified arms, and names
      this exception, instead of "each amendment ... exercised".
    - **PSV-6 (SENTINEL and the PSV-6 reviewer).** The paragraph, amendment 79 (d) and the runbook
      presented the exec race as open. Now: the plain exec-after-spawn route was executed (18 of 20)
      and is refused since amendment 85 (no process but the helper and its parent may hold the reply
      pipe; production also refuses a non-pipe stdout; 0 of 20 relayed; M2050, M2051, M2053, M2054); the
      pin is a configuration and mistake guard, not a defence against malicious same-uid code; the
      remaining routes are those amendment 85 lists (`/proc/<pid>/fd/N` reopen, `SCM_RIGHTS`,
      `pidfd_getfd`, `ptrace`, `LD_PRELOAD`). Deleted: "Yama does not touch the exec route" and
      "Nothing here narrows that". The pointer is corrected: amendment 85 changes who may read the
      reply and declines to re-measure.
    - **`fabric_revision`** is TOLD by the operator's kit, not PINNED (amendment 85): fixed in the
      paragraph, amendment 79's table and the runbook.
    - **PSV-6 lists.** The "names" list is now the observation's fields (host profile, Fabric
      revision, Firecracker, launcher and host-config digests, guest image, kernel, verifier,
      suite-registry and policy digests, the intended launch manifest digest); the qualification
      record and profile manifest digests are measured and checked but are launch-manifest fields,
      bound through `intended_launch_manifest_sha256`.
    - **Comments.** Wrong mutation ids in comments corrected: `observer_service.rs` (src) M1864 to
      M1862/M1863 and M1869 to M1867; the test file's M1865 to M1864.
    - **Recorded, not done (liveness only, fail closed).** Nothing observes the 300 ms mid-spawn wait in
      `reply_channel_private` (changing 30 retries to 1 is undetected); nothing tests the observer prune
      and custodian expiry at the boundary second (`expires < now` against `age <= max_age`; a `<=`
      mutation would drop a still-honoured record and allow a second observation of one nonce in that
      second; spend remains single-use, so no second launch). A non-NotFound `/proc` error in
      `reply_channel_private` aborts the launch without a retry.
    - **Round-7 SENTINEL items for amendment 88 / psv1e, not this branch:** a trait-name annotation
      treated as determining, function-global pin keys, candidate-steered method closedness, the
      address-keyed pin cache, and builtin channel method names. They are interpreter code.
    - **Operational friction** (host build needs crates access; keys owned by the pinned builder uid; a
      non-status commit after the mutation run forces a rerun; a later interpreter commit needs a
      `THEMES` line) is in the operator runbook ("Operational friction").
    - **Rows.** None added. M2260-M2269 unused.

## Amendment 90: what the build processes can and cannot touch (C9 round 7, buildenv3)

90. **FIELD-ORIGIN, one executed BLOCKER and majors.** (Round 6's blockers 1 and 2 are closed, verified.)
    - **Blocker (executed at 1204a925).** `constructed_env` put the BUILDER-OWNED toolchain bin directory first
      on `PATH`; `host_tools()` recorded `cc`/`ld` by searching `/usr/bin:/bin` before the build, a lookup the build
      does not perform. A `build.rs` (repository code or any locked dependency's) wrote an executable `cc` into
      that directory; all four host binaries were linked through it (5 planted-cc link lines); the signed record
      still named `/usr/bin/cc`, `host_record_problems` returned `''`, the kit's cc/ld comparison passed and
      `verifier-manifest` said production, `build_state ''`. The same code READ the 0400 proof key, so any build
      script or proc macro could mint a valid proof for any record body. The guest build had the same shape.
      Amendment 86's "a planted `cc` never exists" and row M2090 did not hold (the planted `cc` was in the
      toolchain directory, not the caller's PATH).
    - **The model: three principals, said once.**
      1. The OPERATOR: root-owned tools (`/usr/bin`, `/bin`), the pins in `/etc/axon`.
      2. The RUNNER: root, which is the pinned builder. It creates the build directories, holds the proof keys
         (`<parent>/keys`, root's 0700), re-measures and signs. It is the only reader of the keys.
      3. The BUILD PROCESSES: cargo and everything it spawns (build scripts, proc macros, `make`, the kernel
         tree's own scripts). Each is started by the runner through `setpriv --reuid=U --regid=U --clear-groups
         --no-new-privs` as an UNPRIVILEGED uid (`AXON_GUEST_BUILD_UID`, default 65534; the operator's pin names
         the one the freeze expects). They write ONLY their own source copy, `CARGO_HOME` and target dir (the only
         directories the runner chowns to U), and `/tmp`. They cannot read the keys directory, write the
         toolchain, the system tool directories, the base directory or the record.
    - **Why a different uid and not "create the key after the last step".** A key that exists only after the
      last cargo step still persists afterwards, readable by the NEXT build's code if it runs as the same
      account (a later build's dependency `build.rs` reads the earlier build's key and forges a record under
      its id); and a build running as root can ptrace, remount or `nsenter` its way to anything. A uid
      boundary is the only thing that holds for both. The cost: the controlled build runs as root (the builder
      pin's uid is root in practice) and the build parent must be traversable by the build uid
      (`/var/lib/axon-guest-build` by default, never under root's 0700 home). Stated residuals: a build that
      starts a background process outliving its step still runs as the build uid and can touch the build uid's
      own directories (not the keys, the toolchain or the record); the artifacts are digested after the step;
      a build uid shared with another service on the host shares that service's files, so give the build
      its own uid.
    - **(1) PATH and the toolchain.** `PATH` is the FIXED SYSTEM directories only (`/usr/bin:/bin`); `cargo` and
      `rustc` are invoked by absolute path and `RUSTC` is set. `cc` and `ld` are recorded by the SAME lookup the
      build performs on that PATH (absolute path, realpath, digest). The pinned toolchain is a private,
      ROOT-OWNED copy under the build's base (`<base>/toolchains/<channel>-<triple>`, hard links where the
      filesystem allows, a copy otherwise): the build uid cannot traverse root's home, so it needs the copy,
      and the copy is something it cannot write. The SOURCE tree must itself be root-owned and closed to
      group/other writes entry by entry, else the build is refused (`install the pinned toolchain as root`).
      The record names the copy's paths and the stable source paths (`toolchain.source`); the toolchain pin
      compares rustc and cargo by their STABLE source path and digest (the copy's path changes with every build).
    - **(2) Re-measure.** `measured` = digests of cargo and rustc, a listing-with-digests of the toolchain bin
      directory, and `cc`/`ld` as the PATH resolves them, taken at `begin`. `write` (the one place a record is
      signed) re-measures and REFUSES to sign over any difference (`nothing is signed`); `cargo_step`
      re-measures before it starts a step too (dominated by the write after the step's first record entry).
      The judge requires `measured` to equal the record's toolchain digests and host tools.
    - **(3) The key.** Not reachable by build processes (see the model). Proved by the reviewer's attack as a
      regression test, on BOTH builds: a probe build script that writes an executable `cc` next to `RUSTC`,
      lists and reads every file in the keys directory, and writes into the base directory. Result: it ran as
      65534, `own OUT_DIR: writable` (the control: the probe works), every attempt denied; the record names
      `/usr/bin/cc` and the fixed PATH. A second, read-only probe judges the uid alone (so a build that wrongly
      runs as root still completes and the uid is what fails).
    - **(4) The kit compared only `cc` and `ld`.** It now compares the host build's `cc`, `ld`, `rustc`, `cargo`
      (path and digest) and `rustc -vV` to BOTH the guest build and the operator's pin; a pin with rustc/cargo
      digests `00..00`/`11..11` is refused (tested in the namespace phase of `test_operator_deploy.sh`).
    - **(5) The kit judged in the builder-owned clone.** After the clean-clone check and the kit-equals-clone
      `cmp`, the committed tree is exported from git's object database into a root-owned private copy
      (`$WORK/tree`) and the three guest files are copied (`$WORK/guest-stage`); EVERY later read, root-run
      script (the judge, `trust_root_preflight.sh`), install source (`fc_linux_profile.sh`, the units, the
      guest files) and pin computation comes from the copy. A drift test on the kit's own text fails if any
      `$CLONE/scripts|profiles|dist` read remains besides the copy-aside itself.
    - **(6) Minor.** `GIT_CEILING_DIRECTORIES=<base>` is part of the constructed environment (the guest
      `axon-core/build.rs` ran git and stamped the HEAD of whatever repository enclosed the builder's
      parent); the judge's environment equality includes it. `check-host-record` takes `--builder-uid`,
      `--builder-parent`, `--build-uid` together as plain ASCII decimal uids and an absolute parent, or none
      (then the pin): a partial or malformed set is refused (it silently fell back to the pin).
    - **(7) Origins the tables omitted, and `--expect-commit`.** `--apply` now REQUIRES `--expect-commit SHA`:
      the judge, the launcher script, the units and the host-build script all come from the commit being
      deployed, so the operator must name it (a commit that names itself vouches for itself). Origin table
      (trust statement per row):

      | field | origin | class |
      |---|---|---|
      | the builder uid, parent, build uid | `/etc/axon/builder-pin.json` (operator, installed by the kit from the flags) | the OPERATOR's word |
      | proof keys | the root runner, `<parent>/keys`, root 0700 | the builder's (root) |
      | host binaries' digests, `source_revision` | the signed `host-build.json` of a controlled build | SIGNED by the pinned builder |
      | helper config `fabric.revision` | the installed binary's own `verifier-manifest`, run as root by the kit, cross-checked against the signed record's `source_revision` through `check-host-record --commit` | TOLD by the binary; the commit is the operator's (`--expect-commit`) |
      | guest kernel tarball, config, overlay and busybox digests | the COMMITTED `profiles/linux-microvm/kernel.pin`, checked only against the pin file of the same commit | TRUSTS the commit the operator named; NOT independently pinned. The kit prints these digests in the dry run (`NOTE[guest]`) so the operator can compare them to the upstream release (kernel.org's `sha256sums.asc`) and to its own records |
      | the root judge, launcher script, units, host-build script | the commit named by `--expect-commit`, exported from git to a root-owned copy | the OPERATOR's commit |
      | the Fabric's authority-store epoch ledger key | the caller's `AXON_ATTEST_KEY` environment (`axon_loop::Store` `LedgerKey::from_env`; unset means unkeyed) | the caller's (accepted store-config limitation, ADR-001) |

    - **Rows (killed by their own attack).** M2220 (PATH is the fixed system dirs), M2221 (build processes run
      as the unprivileged uid), M2222-M2224 (the toolchain tree check and its call), M2225-M2227 (re-measure:
      sign refusal, bin listing, live linker), M2228 (git ceiling), M2229 (the proof is for the pinned build
      uid), M2231-M2233 (the judge: private toolchain copy, measured compiler, measured linker), M2235-M2236 (builder flags), M2237 (the kernel's make as the build
      uid), M2238 (the judge's environment equality). Retargeted: M1181, M1196, M2090; M1188 now kills (its test
      keeps the retooled toolchain a private-copy path, so only the channel judge can refuse it). NOT rows
      (dominated, stated): the judge's `build_uid` clauses (M2230 and M2234 as first written, root as the
      build uid, were REFUSED_ELSEWHERE by the pin-versus-record comparison and are NOT kept), measured cargo/rustc digests (the bin listing's file digests refuse the same swap
      first), the pin's `build_uid` shape clauses (the pin-versus-record comparison refuses the same inputs
      first), `bu <= 0` against `bu == builder_uid` (both fire for root), `cargo_step`'s pre-step measure (the
      write after the entry refuses it), `require_runner` (not testable without a non-root runner), the kit
      guards (not cargo tests; each removed alone by hand, see the report).
    - **Matrix.** A168-A171. **Operator deployment.** The controlled build now REQUIRES root and an unprivileged
      build uid; install the toolchain as root; use a build parent every uid can traverse; the kit takes
      `--build-uid N` and `--expect-commit SHA` (for `--apply`); the builder pin gains `build_uid`; a guest
      image and host build made before this amendment are refused (records lack `build_uid` and `measured`).

## Amendment 91: what a child is built with, what bounds a read, and the exemptions that were wrong (C9 round 7, eqgate3)

91. **Source: the round-7 EQUIVALENCE review (`DO_NOT_REGISTER`), two executed blockers and two minors.**
    Mutation ids M2270-M2339 (all 70 used; M2271, M2272, M2274, M2278, M2318 and M2338 were reused for the
    rows listed below once their first purpose was merged away), matrix rows A172-A177 (the integrator
    renumbers; this branch alone lacks A162-A171, which other branches hold, so its matrix check was run with
    placeholders for that gap).
    - **BLOCKER 1: the gate could not see environment construction, stdio redirection, git option lists,
      size caps, descriptor-inheritance flags, or a refusal made through a diverging closure that
      delegates.** Derived by sweeping every in-scope file for the builder and OS-boundary vocabulary
      (`BUILD_FORM`, one source line per group): `Command::{env_clear, env, env_remove, envs}`,
      `current_dir`, `stdin/stdout/stderr(Stdio::null|inherit)`, the git `-c` options and `GIT_*`
      variables, process-state libc calls (`setitimer`, `chdir`, `close_range`, `setpriority`, `sched_*`,
      `personality`, `flock`, `setsockopt`, `dup`, `pipe2`, `socket*`, `unlinkat`, `renameat2`, ...), the
      close-on-exec flags, and the cap forms (`.min(room)`, `.take(<bound>)`, `.take(MAX_*)`,
      `.truncate(MAX_*)`). Swept and NOT made forms, with the reason: `Stdio::piped()` (a pipe is the capture the
      caller then takes; removed, the take or the parse of empty output fails: it removes no guard; all 20
      sites in scope are followed by one), `x > MAX_*` (followed by its own refusal, which the `Err` forms
      see), and the uid/gid/pre_exec/process_group calls (amendment 87's `PRIV_FORM`). A diverging closure or
      fn is now a refusal constructor when its body calls `exit` with a literal non-zero or a COMPUTED code
      (`refuse`) or calls another constructor, to a fixed point; that makes the signer loader's 13 `bad(..)`
      calls visible (`DIVERGING_DEF` registered only a literal non-zero exit within eight lines).
      Gate rows M2270, M2273, M2275, M2277 (the form lines), M2279, M2280 (the transitive rule); planted-form
      tests `a_child_build_and_a_size_cap_are_sites` and `a_diverging_closure_that_delegates_to_a_refusal_is_a_constructor`.
    - **Exposed: 82 sites.** ROWED, each by a test whose attack is the weaker environment, redirection, cap or
      option, observed from inside the child, through a file, through memory growth or on a hostile repository:
      the signer key that is not a regular file (a FIFO an attacker feeds the genuine key through) and the signer
      naming extra fields (M2281, M2282); every `git_cmd` option and its empty environment, each against a
      repository whose own config names the thing to defeat (M2283-M2291: env, `GIT_OPTIONAL_LOCKS` as the
      index of the tree under test being written, fsmonitor, a hook from `core.hooksPath`, `core.excludesFile`,
      `core.attributesFile`, `core.checkStat`, `core.trustCtime`, `safe.directory`); the three stdio
      redirections of each of the root launcher (the `quiet` closure), the pinned launcher and its verify step,
      and the observer program, read from the child's own `/proc/self/fd` against pipes (M2292-M2300); the
      output caps of the PSV runner, the local check executor and the command generator's stdout and stderr, as
      peak-RSS growth past 120 MB for a 4 KiB bound, each test run alone in its own process so a neighbour's
      allocation is not read as the growth (M2301-M2304), the custodian's and observer's request-line bounds
      (answered at the bound, not at the 5 s deadline) and a measured file's bound (M2305-M2307); the helper's
      `openat` close-on-exec (the launcher's descriptor table against an allowlist, M2308); the PSV check
      child's and the local check child's and the interpreter child's environment, working directory, seed,
      ceiling and virtual clock (M2309-M2317, M2319), each against a parent whose own environment is hostile.
    - **Stated and not rowed (names, so they are greppable):** `grep -n REMAINDER scripts/v022_refusal_coverage.py`.
      The git reader's environment and `--no-includes` are DOMINATED (`git config --file` reads only the named
      file; `include.*` is refused by `allowed_key` either way); `GIT_CONFIG_NOSYSTEM` needs a writable
      `/etc/gitconfig` (never written); `core.untrackedCache=false` could not be attacked (a forged
      untracked-cache extension that hides a file was not constructible: git re-lists the directory); the
      observer's reply bound (`take(MAX_REPLY + 4096)`) needs a flooding stand-in helper; the legacy adapter's
      interpreter-child stdin, `PATH` and the psv check child's `PATH` have no row (no effect: the check child
      spawns nothing); `current_dir("/")` after `harden`'s `chdir("/")` is dominated; the custodian client's
      unpinned arm is the development route. Dispositions with rows were not exempted in bulk: 82 sites,
      41 exemptions with a call-graph or documentation fact each, the rest rowed.
    - **BLOCKER 2: exemptions that said a guard is unobservable or unreachable were wrong.**
      (a) `PR_SET_DUMPABLE` of the PSV runner is observed by a unit test in the binary (`PR_GET_DUMPABLE` is 1,
      the real `set_non_dumpable` is called, it is 0: M2320); the HELPER's `PR_SET_DUMPABLE` is observed by a
      FRESH PROCESS (this test binary re-run on one test) that runs `harden()` and reads the bit (M2321). The guest PID 1's hardening, which
      amendment 87 left as REMAINDER because it needs a real boot, is observed the same way, in a forked child of a
      host test: `apply_seccomp` leaves `no_new_privs` set and filter mode on (M2329, M2330), the supervisor
      forwards SIGTERM and SIGINT to its child and reports 128 + signal (M2331-M2333, a `sleep` grandchild). No
      guest boot was run. (b) `prepare`'s candidate and suite tree checks (exempted UNREACHABLE) are reached by
      handing the `pub` function another directory (M2322, M2323). (c) The journal's `line.seq != seq + 1`
      (NOT A VERDICT PROPERTY) survived while its sibling was killed: a journal with a gap is refused (M2324).
      (d) The `lim2` REMAINDER said "no row yet" for limits M1605-M1613 already row; the closure carrying them is
      now rowed too (M2276) and the stale text is gone.
    - **A real defect found by trying to reach an UNREACHABLE refusal, and fixed.** `Store::guard` (a `pub`
      function) stopped checking components at the first one that does not exist yet, so a `..` after a missing
      component was accepted (`root/a/../../outside`). The check is now made on EVERY component before any is
      looked up (M2325). Every internal caller builds paths from validated segments, so nothing was exploitable
      through the store's own API; the fix removes the dependence on that.
    - **The exemption audit (the reviewer sampled 36 of 861 plus the 14 REMAINDER and found 5 wrong in 50).**
      Done systematically, by kind: (i) EVERY REMAINDER (14), UNREACHABLE (19) and DOMINATED (8) was read; the
      wrong ones are the above and workspace's `DestinationExists` (M2274), the byte quota (M2272), the journal's
      overflow check (M2278) and duplicate-settlement check (M2318). (ii) A MECHANICAL check of every exemption
      that cites a row: 343 cite one; 41 cite a RETIRED row (a four-cell record stands behind each), three cited a
      row that does not exist (M1088, M1597, M2151: corrected), and the gate now refuses any exemption naming a row
      the registry does not hold (M2271). (iii) A mechanical call-graph check of the 264 "not on the protected
      route" exemptions with a unique anchor: 20 sit in generic functions, 54 name a function that also exists
      elsewhere under the same name (`validate`, `create`, `parse`...), and the ones with a real cross-crate caller
      are the documented executor and Cortex paths whose reasons already name the caller. (iv)
      `scripts/v022_exemption_survey.py` tries the cheapest kill for every exemption of a file set (its guard
      opener to `if false && (..)`, the crate's tests, restore) and reports the ones a test kills; a run that hits
      its time bound is INCONCLUSIVE, never a kill. **A first pass of this survey was wrong and is withdrawn:**
      `harden_makes_the_helper_non_dumpable` forked a multi-threaded test process, which deadlocked on an
      allocator lock, hung the lib suite, and the survey read the timeouts as kills (journal 541, 623 and others
      looked "wrong" and are not). The fork tests (this one and the guest's two) now run in a fresh single-test
      process; the survey was rerun clean. Swept: the journal, workspace, branches, axon-audit and axon-os
      ledger. **Result: 87 exemptions; 22 killed by a test (the exemption is wrong), 19 survived (the claim
      held), 1 inconclusive (the journal's lock deadline), 35 have a non-`if` opener (a match arm or a return:
      not tried), 10 have no line site.** The killed ones that were on the protected route and cheap are rows
      (above: M2272, M2274, M2278, M2318). **The remainder, not converted because no id was left in this
      range, each with the test that kills it: journal.rs 515 (`an_undeclared_scope_or_a_redeclared_ceiling_is_refused`),
      562 (`a_record_that_violates_the_state_machine_is_corruption`), 571
      (`sigkill_after_launch_reconciles_to_outcome_unknown_with_liability_kept`), 702
      (`g13_settlement_without_origin_is_refused_and_writes_nothing`), 722
      (`failed_and_cancelled_work_is_charged_or_held_never_dropped`), 1110
      (`same_operation_id_with_a_different_input_digest_is_a_conflict`); branches.rs 247, 252, 262
      (`an_experiment_needs_a_durable_base_two_arms_and_independent_approval`), 296
      (`branches_start_from_one_frozen_base_with_independent_run_identities`), 397
      (`cancelling_a_losing_branch_keeps_its_record_and_leaves_the_winner_alone`), 403, 409, 417, 439, 469
      (`publication_requires_base_epoch_writer_exact_verified_output_and_approval`), 537
      (`two_concurrent_publications_from_one_head_have_exactly_one_winner`); axon-audit lib.rs 392, 399, 535 and
      axon-os ledger.rs 81, 89 (`ledger::tests::budget_acquisition_blocked`, `weight_exfil_egress_denied_R25`):
      22 exemptions whose guards a test already kills and that need rows.** Their exemptions argue the route
      (the journal records the Fabric's own durability, branches and audit are off the protected route), which a
      killing test does not refute; they stay exemptions with this list as the debt, for the next round.
      A kind NOT surveyed: NOTHING TO ADMIT (114), OPERATOR-AUTHORED (60), OS ERROR (40): the first two are
      structural by type (no value to admit with; fields of an operator-owned file), the third fails closed.
    - **The gate's own staleness rule was too loose and is tightened.** Amendment 87 narrowed "exempt yet covered"
      to an exemption on the site's own line, so an exemption on a guard's OPENER line stopped being flagged when a
      row took the guard over (five survived: the journal's seq and duplicate checks, `prepare`'s two tree
      checks, the helper's `finish`). An exemption is now stale when EVERY site whose block holds it is covered by
      a row (the helper's regular-file `openat`, which lies in blocks of rowed `fchmod`s and is its own
      exempted site, is not). M2338; `an_exemption_inside_a_site_a_row_covers_is_stale`.
    - **MINOR: M2152 and M2153 shared one marker and one test.** M2152 (skip the whole `pre_exec` hook) now
      has its own test and marker (`the_check_childs_pre_exec_hook_runs`: neither bit set means the hook did
      not run), M2153 (only `no_new_privs`) keeps `the_check_child_cannot_gain_privilege_and_dies_with_the_runner`.
    - **MINOR: classified as non-guards, with the reason.** axon-os `(cap / 2).max(1)` and backend
      `wall_time_ms.div_ceil(1000).max(1)` (both sites): the helper refuses `timeout_s == 0` at
      `privileged_launcher.rs:588`, and a clamp to 1 only keeps a degenerate limit from becoming "no time at all"
      (availability, fails closed). The journal's `sync_all`/`fsync` and the `saturating_*` accounting are
      durability and arithmetic hygiene, not observable by a suite (a crash between write and sync is not
      reproducible in-process); not guards, not rowed.
    - **The cortex PDEATHSIG race closure** (`getpid()` after fork before `prctl`) is the same class as the
      runner's hook and is named here as unobserved: a race window of microseconds with no deterministic attack.

## Amendment 92: no test can reach the host through the kit, and what a build leaves running (C9 round 8, buildenv4)

92. **A kit refusal test must not become a real apply, and the build uid's leftovers must not be signed.**
    - **The incident (2026-10-06).** A by-hand removal of the `--expect-commit` guard made the test line
      `bash "$KIT" ... --apply` run as root on the dev host: a real deployment under `/etc/axon`, `/usr/local`,
      `/var/lib`, `/etc/systemd/system`, a setuid-root launcher, five system users and two enabled sockets (reverted
      with the user's approval). A refusal test that depends on the guard it tests becomes the thing it tests when
      that guard is removed or regresses.
    - **Why not `--root PREFIX`.** Not feasible without changing what is tested: the kit's destinations are absolute
      (57 hard-coded `/etc`, `/usr`, `/var`, `/run`, `/proc` and `/sys` references), users are made in the host user
      database (`useradd`, `getent`), the setuid launcher's ownership, the production loader (run as the Fabric uid)
      and `trust_root_preflight.sh` all resolve absolute paths. A prefix would make the tested kit another
      program from the deployed one, so it was not added and the kit's interface is unchanged. The prefix is the
      private mount namespace itself.
    - **`scripts/lib/opkit_ns.sh` (`ns_run`).** Runs a command under `unshare -m --propagation private` with a
      tmpfs over `/usr/local`, `/var/lib`, `/var/log`, `/var/spool`, `/var/mail`, `/run`, `/srv` and a tmpfs COPY of
      `/etc` (so `/etc/systemd/system`, `passwd`, `group` and `shadow` are the namespace's; `/run` hides the
      host's systemd and D-Bus sockets), THEN proves it before the command starts (`opkit_ns_assert`): the process
      is in a mount namespace other than PID 1's; each destination's mount in effect is a tmpfs; a canary file
      written under each is absent from `/proc/1/root`. Any failure: exit 97 and the command never ran. Order matters:
      the tmpfs check precedes the first write, so a failed proof never writes under a real destination.
      **CORRECTED by amendment 97: the "canary absent from `/proc/1/root`" check treated an UNREADABLE
      `/proc/1/root` as "not visible" and passed vacuously, and it was a negative lookup. The proof is now an
      identity comparison (device:inode of each destination through the host's view, which must differ) and an
      unreadable view refuses; the namespace is also PID, UTS, IPC and NET, not only mount.**
    - **Every kit call goes through it**, the guard-removed attack included. `test_operator_deploy.sh` runs
      every dry-run and attack line as `ns_run bash "$KIT" ...`; the namespace apply phase runs under `ns_run` and
      re-asserts before each kit call (`kit`). The `--apply` without `--expect-commit` attack now runs as ROOT
      (it used to run as uid 65534 to be safe): with the guard absent it SUCCEEDS, inside the namespace, and the
      test fails on the unexpected exit status. The test's own `host-build` setup runs in a private PID namespace. **CORRECTED by amendment 97: it still ran
      as root on the real host (a builder-private parent under the host's `/var/lib`, the synthetic-manifest Python,
      the host build, a forged-record directory), OUTSIDE `ns_run`; all of it now runs under `ns_run`.**
    - **Drift test** `scripts/opkit_ns_drift.py` (in `gate.sh` and as a cargo test): fails when a `test_*.sh`
      runs the kit (`$KIT`, the kit's name as a command) or carries `--apply` on a line that does not use `ns_run`
      or `kit`; `--selftest` plants an unwrapped `--apply` and requires it refused (M2265).
    - **Rows M2265-M2269**: the drift check; the helper's tmpfs, namespace and canary refusals (M2266-M2268, attacked
      with scratch directories and stand-ins for PID 1 via `OPKIT_*_FOR_TEST`, never a real destination, and
      `test_operator_deploy.sh` refuses to start if one is set); `ns_run` never starting its command (M2269).
    - **Guard removals by hand, inside the prefix only.** `--expect-commit` (`$APPLY = 0 || -n $EXPECT || refuse`
      -> `true`): the attack's `--apply` ran as root in the namespace (`operator_deploy: APPLY`), the test failed on
      it ("expected REFUSED (2), got 3"). The null-machine-id refusal -> `if False:`: the test failed
      ("a B263 record with a null machine-id was accepted ... (None == None)"). The host-toolchain comparison
      (`tools.get(n) != hb.get(n)`) -> `if False:`: the test failed ("expected REFUSED (2), got 0"). Each ran in a copy
      of the tree; host listings (`/etc/axon`, `/usr/local`, `/var/lib` names, `/etc/systemd/system`, users and groups,
      sums of `passwd`/`group`/`shadow`, setuid files) were identical before and after each experiment and after the
      unmodified full run. No kit invocation with `--apply` was run outside the helper, on this host or gpumaster.
    - **The kit's B263 machine-id comparison (M2260 is a kit guard, a hand removal above, not a cargo row).** It was
      `facts.get("machine_id") != here`, with `here = None` when `/etc/machine-id` is absent or empty, so a record
      with a null or empty machine-id compared equal on a host with none (None == None). A missing identity on EITHER
      side is now refused (`PENDING[data] B263: ... machine-id is missing or empty`). Tested in the namespace with
      `/etc/machine-id` removed inside it. Matrix A180.
    - **The build uid's processes (`guest_build_env.py`).** Amendment 90 stated, as a residual, that a background
      process outliving its step could still touch the build uid's directories. It could do more: the runner then
      hashed and signed whatever was there. After EVERY cargo step (and each kernel `make` step) the runner now
      (1) kills the build uid's whole PID set read from `/proc` (real, effective, saved or fs uid; never a name
      match) until a pass finds none, and refuses if any survives `SIGKILL` (`reap_build_processes`, M2261).
      **CORRECTED by amendment 97: this said "zombies excluded", and the filter was per PROCESS, so a
      multi-threaded process whose main thread had exited (State Z) while its other threads kept writing was not
      listed; the scan is now per THREAD and the step runs in its own PID namespace, see amendment 97.**
      (2) chowns the source copy, `CARGO_HOME` and target dir to root and removes group/other write
      (`lock_from_build`, M2262) so no later write BY PATH can change what is hashed. **CORRECTED by amendment 97:
      this said "so even a process the reaper could not see cannot change what is hashed"; that was false, because
      a chown does not revoke a descriptor the process already holds open (executed: a process wrote through its
      open fd after the lock-back). What makes it true is that no process of the step survives (own PID namespace
      plus the thread-aware verification), not the chown.** The trees are handed back to the build uid
      (`hand_to_build`) only at the start of the next step; (3) holds one
      advisory lock per build uid (`/run/lock/axon-guest-build-uid-U.lock`; **moved by amendment 97 to a root-owned
      directory, `/run/axon-guest-build-locks/`**) across hand-over, step and lock-back,
      because the reaper would otherwise kill another job's cargo: two controlled builds with one build uid now run
      one after the other. **The build uid must be dedicated to the build**: everything it runs is killed after each
      step (the default 65534 is `nobody`, so on a host that runs a service as `nobody` set `AXON_GUEST_BUILD_UID`).
    - **The rootfs's inputs.** `rootfs()` read `kernel.pin` and `guest-init.sh` from `rec["src_dir"]`, the copy the
      build uid owned. It now reads both from the committed tree (`git show HEAD:...` in the runner's own repository,
      `committed_file`; M2263, M2264). The kit judges a clean clone, so HEAD is the tree that was built.
    - **Tests, and what could and could not run.** Every `guest_build_env.rs` test now runs the script in a private
      PID namespace (`unshare --pid --fork --mount-proc --kill-child`), so the reaper can only ever see that
      namespace's processes; the host's `nobody` processes cannot be signalled, and the `host-build` of the kit
      test does the same. `a_detached_build_process_does_not_outlive_its_step_or_rewrite_what_is_signed` has a build
      script start a `setsid` writer that rewrites the built binary and a source-copy file every 50 ms: after the
      step the namespace holds no build-uid process and neither the artifact's nor the source copy's bytes move.
      `the_trees_a_step_leaves_are_root_owned_and_not_writable_by_the_build_uid` has the script leave 0666/0777
      entries. `the_rootfs_inputs_come_from_the_committed_tree_not_the_builders_copy` rewrites both inputs in the
      private copy. `setpriv` and `unshare` were available and everything above was executed as root on this host.
      Not executed: the real kernel `make` path (the existing kernel tests use a fixture tarball and its steps now
      pass through the same reap and lock, but no new kernel-specific attack exists), and a `nobody`-owned
      background service on the host (not present).
    - **Rows.** M2261-M2269 (M2260, the kit machine-id refusal, is by hand as above). **Matrix.** A178-A180.
      **Operator-visible interface.** Unchanged: no kit flag was added, so `v022-operator-changes.md` is not edited.
      Operators running the controlled build must give it a dedicated build uid and expect serialised builds.
## Amendment 93: the 22 exemptions amendment 91 left as debt (C9 round 8, exrows)

93. **Source: amendment 91's "Remainder", 22 exemptions whose guards a test already killed and that had no row
    because the eqgate3 id range ran out.** Mutation ids M2340-M2360 (M2361-M2369 unused), matrix row A181 (the
    integrator renumbers; A178-A183 are held by other branches, so this branch's matrix check was run with
    placeholders for that gap). **21 rowed, 1 exemption kept with a checkable reason, none retired.**
    - **The survey's kills were not all the guards' own attacks, and that decided the rows.** The survey rewrites
      a guard opener to `if false && (COND)`. For a guard whose opener is the SUCCESS branch (`if v.intent == intent {
      return AlreadyRecorded }` before the `Conflict` refusal; `if std::fs::read(&file)? == bytes { return Ok }`
      before `Exists`) that edit kills the identical-replay path, not the refusal, so each of those rows attacks
      the refusal itself (`if true || ..`, M2345, M2349). For a guard whose condition has a side effect
      (`if !create_once(..)? {`) the edit deletes the call, so the "kill" is the missing file. Every row
      below was run on gpumaster (clean clone of the commit) and is KILLED by its OWN attack: a marker
      that only an assertion naming the accepted attack carries, 0 REFUSED_ELSEWHERE.
    - **Journal (M2340-M2345).** Unknown scope (`an_undeclared_scope_or_a_redeclared_ceiling_is_refused`), a
      `launched` record for an op never reserved (corruption on open), completing an op whose outcome is unknown
      (`sigkill_after_launch_..`: the OutcomeUnknown op is NOT launched, so the `Completed` arm refuses), a
      settlement without origin, settling an op whose cost is known, and `begin` of the same id with another
      input. The tests' bare `matches!`/`unwrap_err` assertions gained messages that name the attack, because a
      bare `Ok` panic carries nothing for a marker.
    - **Branches (M2346-M2355).** The three `open_experiment` refusals (a base that is only a hash, one arm, a
      writer that is also an approver: the test's `unwrap_err().kind()` calls now name which was accepted), the
      existing-id reopen (M2349), and the publication refusals: wrong writer, self-approval/undeclared approver,
      stale epoch, verified on another branch, old head (each acceptance is a `panic!("ATTACK: {what} was
      accepted")`). **The cancelled-branch refusal (M2350) was NOT shown by the test the survey named:** that
      test's publication on the cancelled branch cites `loser-bad`, a FAILED check, so with the cancelled check
      removed the publication was still refused as `unverified` (REFUSED_ELSEWHERE shape). A new test,
      `a_cancelled_branch_refuses_a_publication_that_would_otherwise_succeed`, verifies a passing check on the
      branch first (control: the same publication on the other branch is a head).
    - **Audit and resource ledger (M2356-M2360).** `verify_against_file`'s truncation and extra-record
      refusals, the chain's entry-hash recomputation, and `ResourceLedger::carve`'s budget and persist-bytes
      refusals. **M2358's first attack was wrong and is replaced:** an unkeyed forgery handed to a keyed
      verifier is ALSO refused by the missing authenticated tip (`open_keyed`'s `Err(e)` arm), so removing the
      hash check left it refused (REFUSED_ELSEWHERE). The attack only the hash recomputation sees is an edited
      entry body behind intact `prev_hash` links and an intact tip (`ledger_tamper_fails_verification`).
    - **KEPT as an exemption, with a checkable reason: `branches.rs` `journal.fail(` (the lost head-CAS arm, the
      survey's "537").** It is DOMINATED: the only writers of `head-<n>.json` are `open_experiment` (`n = 0`) and `publish` (`n = head.seq + 1`),
      and `Journal::begin` has already refused a second intent under the operation id `pub.<exp>.<arm>.<n>`
      (Conflict, or AlreadyRecorded, both a `BranchError::Conflict`), while a planted head file moves
      `head.seq` and meets the expected-base refusal (M2355) first. The survey's kill of this site was the
      deleted `create_once` call. The reason text now states those facts rather than the route argument.
    - **FINDING (no test observes it, minimal test named, not added):** the `create_once`-false arm itself
      cannot be reached in-process by any caller; the smallest observation would be a `create_once` seam
      (a test-only closure) that returns false once, asserting `publish` returns `Conflict` and the journal op is
      `Failed` with a known zero bill. Adding the seam changes production code for an unreachable arm, so it is
      left.
    - **The route question.** The branches and audit sites are off the protected verdict route (amendment 91's
      `_BRN` grep still holds) and the ledger's budget and persist axes select nothing on it; rowing them is
      not a claim they matter to a verdict. It is the stronger form of the same exemption: a test that fails
      only when the guard is gone, instead of an argument that none does.
## Amendment 94: the features origin/main added move values into operator-held places and name operator fns (C9 round 8, PSV1F)

94. **Source: the round-8 PSV-1, PSV-3 and SENTINEL reviews (two executed blockers; one minor).**
    Mutation ids M2370-M2376, matrix rows A182-A184 (the integrator renumbers; this branch alone lacks
    A178-A189, so its matrix check fails ONLY on that gap). origin/main's 13 interpreter commits (Rc arrays
    and strings, `&mut` write-through, first-class fns, lent closure captures, `arr_sort_by`) were merged
    WITHOUT PCI review; this amendment closes the two routes the round-8 reviews executed and lists what was
    hunted.
    - **BLOCKER 1: a `&mut` write-through value reached the operator's binding with no seal edge.** The
      merged `call_fn_mut` casts the arguments on entry and the RETURN value on exit, then moves the `&mut`
      parameters' FINAL values back into the operator's binding. Nothing cast those values and no dict edge
      applied, and `interp/pin.rs` recorded a variable's initialisers only from `let`, assignment and
      `match`, so a variable lent as `&mut x` stayed 'determined' (or kept its annotation's pin). Executed:
      a candidate `fill(a: &mut [i64])` laundered `4 as u8` through a dict into `a[0]`; the operator's
      `a[0].ok()` ran the lenient `u8` impl (rc 0), and `255 as u8` made `(a[0] << 1) == 254` pass. With the
      operator's array ANNOTATED (`let a: [i64] = ...`) the dispatch rule cannot help, so the edge-back cast is
      REQUIRED, not only the pin change (the PSV-3 reproduction).
      Fix, both halves: (1) `call_fn_mut` treats a sealed callee's `&mut` outs as a seal crossing: each is
      cast STRICTLY against its declared parameter type and judged against what the operator held (a clone
      taken only for a sealed crossing), on EVERY outcome (an operator handler's abort leaves the binding
      `()` and the call a refusal), M2370/M2372/M2373 and M2371; (2) a variable passed as `&mut x` is an
      assignment from an undetermined source in the pin analysis, M2374; and a union annotation (`i64 | u8`)
      no longer counts as closed (it names more than one runtime type; the SENTINEL minor), M2375.
      Which half is sufficient alone: the cast (with the held-value judgement) alone refused every route I
      could execute, including the annotated array the dispatch rule cannot help; the pin half is defence in
      depth, as briefed, and its own attack is the analysis test
      (`a_mut_operand_is_never_determined_by_the_pin_analysis`), because every executable attack is also
      refused by the cast. The held-value judgement is deliberately NOT strict at this edge:
      a strict judgement refused the honest fill of an EMPTY output array (`[?undetermined]`), measured
      before it was relaxed; the declared parameter type judges what the held value did not show.
    - **BLOCKER 2: a first-class fn value bypassed the seal call edge.** `Expr::Ident` returned
      `fn_value(name, arity)` for any top-level fn named in value position; its forwarding body ran in an
      OPERATOR frame (no provenance mark), where `seal_call` allows the call. Executed: a candidate
      `let g = secret; g()` ran the operator's `secret` (direct `secret()` is refused), and
      `let g = peek; g()` read an operator global `SECRET`. Fix: the call edge applies at the CREATION of
      the value (a sealed frame naming an operator fn is refused there), M2376. A candidate's OWN fn value,
      and an operator's own handed to the candidate, are untouched (controls in the tests). A provenance mark
      on the value was tried and DROPPED: with the creation edge in place it refuses nothing the edge does
      not, so its row could not be killed by an attack of its own.
    - **Routes hunted and CLOSED** (each executed, rule on and, where the cast could mask it, off):
      `&mut [Dict]` with an element REPLACED by a dict whose key was retyped (rule on and off), and a dict
      retyped IN PLACE (the snapshot edge); nested `&mut [[i64]]`; an element appended to an empty or a
      non-empty array; a generic `&mut [T]`; `&mut` through a closure or a fn value (`call_fn` refuses a fn
      with `&mut` parameters outside a direct call, and the checker's E0605 refuses the call form); a lent
      closure whose captured binding the candidate assigns (an operator closure `|v| acc = v` called by the
      candidate: the closure-argument cast, annotated or not); `arr_sort_by` with a candidate comparator
      (it permutes the operator's own values and its comparator must return `i64`); the operator's
      by-value array passed to a candidate that writes its parameter (copy-on-write: the suite's copy and a
      lent closure's capture keep their values, A184); a fn value stored in an array or dict, passed to a
      candidate helper, returned to the operator (all refused at creation).
    - **NOT closed or not examined**: struct-field and nested-place `&mut` operands do not exist (the
      checker requires a whole local); `Rc::make_mut` writes other than through these routes were reviewed
      by reading the call sites touched here, not exhaustively; the `arr_sort_by` stable merge sort itself
      is still not PCI-reviewed beyond the comparator route above.
    - **Honest-program cost.** (a) An operator that reads, after `f(&mut a)`, a value of `a` and dispatches a
      method that two or more operator impls define, or does arithmetic on a fixed-width integer, must
      re-pin it: `let b: [i64] = a` then use `b` (the variable lent as `&mut` is undetermined for the whole
      function, by name). Measured: the un-re-pinned read after an honest write is refused. (b) A callee
      declared `&mut [T]` over a type parameter no argument determines is refused strictly at the edge back
      (as a return would be); a determined `T` is fine. (c) A sealed crossing keeps a clone of the lent array
      (the held value), so the callee's first write copies the array (copy-on-write); outside a seal nothing is cloned.
      Nothing else changed for honest suites: filling an empty output array, writing through a non-empty
      one, a candidate naming its own fns as values and an operator naming its own all pass (controls).
    - **Evidence.** Unit tests `interp::tests::a_mut_*`, `an_annotated_operator_array_lent_as_mut_*`,
      `a_sealed_frame_cannot_take_an_operator_fn_as_a_value`; runner tests (`axon_psv::runner::run`) in
      `crates/axon-psv/tests/sealed_frames.rs`; gate rows `am94 ...` in `scripts/v022_pci_gates.sh`; rows
      M2370-M2376 each killed by its own `ATTACK:` message on gpumaster. The rows M1672, M1841, M1843, M1844
      and M1845 name text in `replaced_ok` that gained a `strict` parameter; their old/new text follow it.
## Amendment 95: decisions expressed as a value, an atomic refusal, a term or a constant (C9 round 8, eqgate4)

95. **Source: the round-8 EQUIVALENCE review (`DO_NOT_REGISTER`, one BLOCKER): "a guard expressed as a VALUE, or as an
    atomic refusal, is still invisible to the refusal-site gate."** Mutation ids M2400-M2466 used (of M2400-M2469),
    matrix rows A185-A189 (other branches hold A178-A195; the integrator renumbers; this branch's matrix check was
    run with temporary placeholders for A178-A195, which are not committed). Base `c9r4c/integrate6` (89da403a); no
    file under `crates/axon-core/src` was touched.
    - **Survivors 1-6 are now killed by their own attack, each by a test that observes the production value or the
      guard itself.** (1) The PSV runner's check identity and digest pin are values in a new pure `guest_config`;
      a unit test reads `drop == Some((65534, 65534))` and the cmdline digest (M2400 TEST_UID, M2417 TEST_GID,
      M2401 `drop: None`, M2402 an empty pin). (2) The same test holds the digest. (3) The guest PID 1's environment:
      the real `axon-guest-init` binary, root only, runs a workload that prints the five variables; one row per
      `env::set_var` (M2403-M2407; AXON_BUDGET_TOKENS and AXON_PRINCIPAL had none). (4) `create_dir_all` for
      `create_dir`: a run dir (a function of its own, `RunDir::create_new`, M2408), `prepare`'s job dir (M2409), the
      helper's staging leaves and tree copy (M2410, M2411), the in-uid observer's work dir (M2412), and the
      Fabric-private inputs dir, whose `DirBuilder` gained no `recursive(true)` (M2418). Each test plants a
      directory with a stale file and requires the refusal and the file untouched. (5) Compound terms:
      custodian.rs's `!m.is_file()` is UNREACHABLE (execve refuses a non-regular file; replacing it with `false`
      left the whole suite green: an exemption stating that, not a row), its uid term is killed by an executable a
      stranger uid owns (M2413, root only), observer_service.rs's `!is_file()` by a FIFO that holds the key (M2414:
      the FIFO's bytes are kept alive by a second descriptor, so the key IS readable; a directory or device would be
      refused by the read and is not a kill). (6) `DEFAULT_OBSERVATION_MAX_AGE_S` (M2415, through the host config
      loader) and a `CERT_FIELDS` entry replaced by a duplicate (M2416: the test removes each of 22 fields, written
      out in the test, from a certified record and requires it named as missing).
    - **The gate now sees these FORMS (`scripts/v022_refusal_coverage.py`).** (a) `VALUE_FORM`: `create_dir`,
      `create_dir_all`, `DirBuilder`, `libc::mkdir`, a `drop:` field, an upper-case `*UID`/`*GID` name, a struct-literal
      `expected_*sha256|digest|hash` field (a field declaration or a parameter is not one), `env::set_var`,
      `env::remove_var` (M2420-M2427). (b) `OKOR_FORM`: a single-line `.ok_or(..)?` / `.ok_or_else(..)?` (M2428). THIS
      REVERSES amendment 76, which had judged a bare `ok_or` "an absence some other decision made": 71 refusals were
      unseen. `an_inline_predicate_refused_through_ok_or_is_a_site` was edited for that reason and says so. (c) `PRIV_FORM`
      gained `libc::syscall(` and `oom_score_adj` (M2432, M2433). (d) A form whose decision is its OWN LINE
      (flags, privilege and build calls, caps, the new `VALUE_FORM`/`OKOR_FORM`; not `Some(reason)`, whose decision is the
      condition above it) is credited only by a row whose edit changes THAT line (M2435): ten sites had been credited by a
      row on a neighbouring condition (the `create_dir` in the block of a uid check, `.take(MAX_*)` in the block of an owner
      test). A `DirBuilder` chain is judged at its `.create(..)` line. (e) A compound guard is judged PER TERM: each
      top-level `||`/`&&` term of the opening `if` of a row-covered line site must be reached by a row's changed
      characters, or the row must make the whole condition a constant (a small reducer evaluates `true`/`false` through
      `&&`/`||`: `false && (a || b)` is constant, `false && a || b` is not, M2434) or REMOVE the refusal (an edit that turns
      the `return Err` into a no-op credits every term), or the term is on `TERM_EXEMPT` with a fact; a term exemption that
      matches no uncredited term is stale (M2429, M2436). (f) A `const`/`static` read, outside strings, comments and the
      refusal's own message, by a function that contains a refusal site is itself a site; its definition needs a row that
      changes it or an exemption (M2430). (g) The exemption audit reads a row id of any number of digits (M2431).
      Each has a planted-form test in `refusal_coverage_gate.rs` and a gate row.
    - **Counts and dispositions.** The new rules reported about 260 sites and terms at their first run, over and above the
      original forms. After the rows above: **205 exemptions and 4 term exemptions were written; 65 rows (48 behaviour rows and 17 gate rows) kill; 5 exemptions went stale
      because a row now covers them and were dropped.**
      ok_or: 71 sites, 5 with a checkable fact (3 `Stdio::piped()` takes, 1 dominated by `plain_name` on the next line,
      1 unreachable behind `file_name() != Some(leaf)`), **66 are `REMAINDER`**. Constants: 13 rowed (M2415, M2416, M2452-M2457,
      M2462-M2466), 94 `REMAINDER` by kind (42 tags, 14 paths, 12 bounds, 10 tables, 9 texts, 4 structured, 3 exit codes) and 7
      Cortex ones exempt as not on the protected route. create_dir_all and friends: 14 `ENSURE-EXISTS`/`DOMINATED`/`UNREACHABLE`
      exemptions each stating what is created NEW below (the row) or why `create_dir` and `create_dir_all` agree there;
      axon-os (9) and the hidden `__psv-host-guest` stand-in (3) are not on the protected route (checkable).
    - **Surveys (executed on gpumaster, clean clones of a committed sha; a survey kill is a finding, not a row).** TERMS:
      21 uncredited terms, each replaced by `false` alone, the owning crate's suite run. 15 survived: 3 are equivalent and now
      TERM_EXEMPT with a fact (custodian `!is_file`, the launcher's `!is_dir` behind O_DIRECTORY, sealed_exec's `size < 0`
      behind the next term's `as u64`), 1 is dominated by a whole-guard row (checks.rs `starts_with('/')`), and **11 were real gaps,
      now killed by new tests** (the out root's owner M2438, the guest verdict's candidate tree, suite tree and test M2439-M2441, the
      trust preflight's schema M2442, a subject-issuer verifier M2445, the three evidence kinds M2447-M2449, a suite entry that is not a
      file M2450, an empty symlink target M2451). 6 were killed already; 4 got their own marker (M2437, M2443, M2444, M2446), one
      (evl.rs's `matches!(Passed|Failed)`) stays `REMAINDER`: 12+ tests fail but none on an assertion about it. TAGS: ten schema
      constants, each with `-eq4x` appended: 6 killed by fixtures (custodian, observer, launcher, grants, ACK, guest policy), **4
      survived** (readiness CERT_SCHEMA, journal, attestation, launch manifest): so "a literal pins it from outside" is NOT true of
      every tag, and the 42 tag entries say REMAINDER, not "equivalent". BOUNDS: killed and rowed: MAX_MESSAGE, MAX_POLICY, the evidence age,
      the guest policy word, the cmdline limit, the contract limits (a test that built its oversize document from `MAX_BYTES`
      itself refused a 1 GiB bound with a 1 GiB string: now literal pins), the recipe quota; **survived**: observer MAX_REPLY and
      MAX_PROFILE_MANIFEST, sealed_exec MAX_BYTES, backend MAX, git MAX_REASONS, certcheck MAX_DEPTH. SITES: `.take(MAX_REQUEST)` of the
      operator-file reader survived and is rowed with a new test (M2458); `.take(MAX_POLICY+1)` and `.take(MAX_BYTES+1)` survived and are
      outcome-equivalent (the comparison next to them refuses the same input; they bound memory only); `.mode(mode)` at creation
      survived because `set_mode(&shown, mode)` sets the final mode (a row was written, survived, and was withdrawn as an equivalent
      rather than counted: M2459 is unused); `custom_flags(O_NOFOLLOW)` is dominated by `create_new`.
    - **WHAT THE GATE STILL CANNOT SEE (stated, not covered; each is a greppable `REMAINDER`).**
      (1) The 66 single-line `.ok_or(..)?` refusals and the 94 constants above are LISTED, not observed: no claim is made that a test
      removes any of them alone, and for the ok_or the generic bypass needs a per-site default value that no mutation supplies.
      (2) A `.ok_or(..)` whose `?` is on the next line, a `?` on any other conversion (`.map_err(..)?`, `Option?`), `.expect(..)` and
      `.unwrap()` as refusals. (3) Terms of anything but an `if` / `else if` condition: `while`, a match guard, `let .. else`, a
      closure body, the inside of a parenthesised sub-condition or a `matches!`; and a negated group is one term. (4) A constant read
      only through a local that is later compared (`let m = MAX; if x > m`), a constant read by a function the rule does not see
      as refusing, a `static`, a constant in the interpreter outside conform.rs. (5) A uid or gid that reaches a primitive as
      a local, a computed value, a struct field not named `drop`, or a literal (`custodian_uid: me`); a digest pin that is not named
      `expected_*`. (6) A decision made in a shell script, a build script or a checked-in data file (the gate reads Rust).
      (7) `create_dir` reached through a wrapper that is not named `create_dir`, `mkdir` through `Command`. (8) A form credited by a
      row on its own line whose edit is not the form (a row that changes the comment or the message of that line).
      (9) Every equivalence or dominance claim in an exemption is the reviewer's to test; each cites a line or a survey, and a survey
      'survived' is a statement about this crate's suite, not a proof.
    - **Unfinished, stated.** The 66 + 94 REMAINDERs above. The survey of the fabric constants stopped at `MAX_REASONS`
      (it was cut when one candidate, `MAX_OUTSTANDING` raised to 2^30, ran for over an hour without finishing): the
      custodian's `MAX_OUTSTANDING` and IO timeouts, the observer service's IO timeout, protected_host's `KEYS` and
      `REFUSED_CALLER_FLAGS` are `REMAINDER` with no survey result. The 17 gate rows and 48 behaviour rows were each
      run by `v022_g01_mutations.py --only` on gpumaster from a clean clone and are KILLED by their own marker, except
      that M2459 (written for `.mode(mode)`) survived and was withdrawn; the full `axon-fabric` suite (serial and sharded,
      gpumaster) and the `axon-psv`, `axon-guest-init`, `axon-loop`, `axon-loop-contracts`, `axon-workspace-recipe` suites and
      the three axon-core gates were green at the rows' commit; matrix rows A185-A189 await renumbering at integration.

**Renumbering at integration (round 8, integrate7).** Four branches built in parallel numbered their matrix rows apart; the integration made the matrix contiguous, so the amendments above that cite the branch-local ids are read through this map: amendment 93 `A184` is now A181; amendment 94 `A190`-`A192` are now A182-A184; amendment 95 `A196`-`A200` are now A185-A189. Amendment 92's A178-A180 are unchanged. (The text of those amendments was rewritten to the new ids; where it says another branch holds a gap, that gap is closed by this integration.)

## Amendment 97: the reaper's blind spot, the dedicated build uid, and the namespace helper's remaining gaps (C9 round 9, buildenv5)

97. **Source: the round-9 FIELD-ORIGIN reviewer's findings on amendment 92**
    (`/var/tmp/c9r9-findings-FIELD-ORIGIN.json`; probes in `/var/tmp/c9r9-FO-logs/`). Mutation ids M2500-M2515
    (M2516-M2519 unused; M2265-M2269 re-targeted, below), matrix rows A195-A200 (A206-A209 unused; renumbered at
    integration, another branch holds A190-A199). `crates/axon-core/src` is untouched.
    - **Finding 1, executed: the reaper could be evaded and the lock-back did not close the gap.**
      `build_uid_pids` skipped any process whose `/proc/PID/status` read `State: Z`. A multi-threaded process whose
      main thread has exited reads `Z` while its other threads keep running; executed as uid 65534 under
      `setpriv --no-new-privs`, it was not listed, and after `lock_from_build`'s `chown -R root` + `chmod -R go-w` it
      kept writing `SUBSTITUTED` through a descriptor it had opened earlier. A chown does not revoke an open
      descriptor. So amendment 92's "zombies excluded ... until a pass finds none" and "even a process the reaper
      could not see cannot change what is hashed" were FALSE as written; both are corrected in place in amendment 92
      and restated here. What the code does now: (1) `build_uid_pids` enumerates `/proc/PID/task/*` and judges each
      THREAD, so a leader that has exited does not hide the live thread (M2500); (2) every build step runs in its
      OWN PID namespace (`as_build_uid`: `unshare --pid --fork --mount-proc --kill-child -- setpriv ...`): the
      step's first process is that namespace's init and the kernel SIGKILLs every other process in it when it
      exits, whatever it did (setsid, double fork, threads, an exited leader), and a missing `unshare` refuses the
      step (M2501); (3) `reap_build_processes` remains as the VERIFICATION that nothing of the build uid survives
      and the backstop that kills it. Hashing from a root-owned COPY was considered and not done: with every
      process of the step dead before anything is read there is no open descriptor left to defend against, and the
      namespace is the property that makes that true rather than the copy. `lock_from_build`'s docstring now says
      what it does and does not do.
      **Test, with a MULTI-THREADED writer whose main thread exits**
      (`a_threaded_writer_whose_main_thread_exited_does_not_outlive_its_step`): a build script starts a detached
      (`setsid`) Python process that opens a source-copy file and a target-dir file, starts a thread that rewrites
      both through those descriptors every 50 ms with a counter, and exits its main thread with a raw `exit(2)`
      syscall. After the step, in the runner's namespace, there is no live thread of the build uid and neither file
      moves. Run against the OLD `guest_build_env.py` with the new tests (branch `c9r9/be5-old`, gpumaster): 6
      failed, 1 passed, with the markers `ATTACK: a multi-threaded build-uid process whose main thread exited
      survived the step`, `ATTACK: the build step ran in the runner's own PID namespace`, `ATTACK: the reaper did
      not list a build-uid process whose main thread had exited`, `ATTACK: after begin the build uid still owns or
      can write a tree ...`, `ATTACK: a uid a deployed config names ... was accepted as the build uid`; the lock
      test fails on the old code too (it has no `LOCK_DIR`). On the new code: 38 `guest_build_env` tests and 3
      `operator_examples` tests pass (gpumaster, serial). **What a single row cannot show:** the PID namespace and
      the thread-aware scan are two layers on the same property, so removing EITHER alone leaves the end-to-end
      test green (the other layer still catches the writer). Each is therefore killed by its OWN test (M2500 by the
      unit test of the scan, which needs a process the scan alone must find; M2501 by a test that the step's PID
      namespace differs from the runner's), and the paired cell (both off) is the end-to-end attack succeeding,
      shown against the old code above.
    - **Finding 2: "the build uid must be dedicated" was prose.** `build_ids()` refused only uid 0 and the runner's
      uid, the kit only 0 and build uid == builder uid, and the default is `nobody`. A build uid equal to the Fabric
      uid would run repository build code as the owner of the host attestation key. `service_uids()` now collects
      (a) the five default account names that exist (axon-fabric, axon-custodian, axon-observer, axon-verifier,
      axonb263), (b) every `uid` / `*_uid` field of the deployed JSON configs under `/etc/axon` (`build_uid` of the
      builder pin excluded: it is what is being judged), (c) the `User=` of every installed `axon-*.service`; and
      `build_ids()` refuses a build uid that is any of them (M2502-M2505). The kit runs the same judgement through
      `guest_build_env.py check-build-uid UID BUILDER-UID [USER...]` with ITS configured account names (M2506),
      so `--build-uid 65534 --fabric-user nobody`, `--build-uid 1 --custodian-user daemon`, `--observer-user bin`,
      `--verifier-user daemon` and `--profile-user bin` are each `REFUSED ... DEDICATED` (five refusals in
      `test_operator_deploy.sh`, inside `ns_run`). **No cargo row exists for the kit's own refusal line** (a shell
      guard): hand removal of that line in a scratch worktree, run inside the namespace helper, failed the test at
      `ATTACK: a build uid that is the Fabric user's uid ...: expected REFUSED (2), got 1` and the host listing was
      identical before and after. `nobody` stays the default; it is refused as soon as a deployed config or unit
      names uid 65534.
    - **Finding 3: `begin()` was outside the lock.** `begin` now holds the per-uid lock for its whole body (M2509)
      and returns the trees to root before it returns (M2510). The lock file is created root-owned 0600 in
      `/run/axon-guest-build-locks`, a root-owned 0755 directory (not `/run/lock`, which is 1777): a directory
      another uid can write (M2507), a lock file that pre-exists with another owner or a looser mode, or a symlink
      is refused (M2508).
    - **Finding 4: the namespace helper.** (a) **Drift of the shadow list**: `OPKIT_DEFAULT_DESTS` in
      `scripts/lib/opkit_ns.sh` is now the ONE list (`opkit_ns_isolate` shadows exactly it and the proof asserts
      exactly it); `opkit_ns_drift.py` extracts every write target of the kit (`act_dir` / `act_install` operands
      with variables expanded and `for X in LIST` loops fanned out, `install_registry`'s destination parameter
      judged at its call sites, and the operands of its own mkdir/install/cp/mv/ln/chown/chmod/tee/useradd/groupadd
      and redirections) and fails if one is not under it; an unresolvable uppercase variable is itself a finding
      (M2514). `--selftest` plants seven outside writes (`/opt`, `/usr/lib/systemd`, `/usr/share`, `/boot`, a
      variable resolving to `/opt`, `tee` into `/usr/lib`) and two inside ones. (b) **PID/UTS/IPC/NET**: `ns_run`
      now unshares mount, PID (own `/proc`), UTS, IPC and NET (loopback only). NET stays the host's for exactly one
      step, the fixture's controlled host build (`OPKIT_NET=host`), because it downloads its crates; every kit call
      is network-private. The host's PID 1 is hidden by the PID namespace, so `ns_run` opens a descriptor on the
      host's root before it unshares and records the host's namespace ids; the proof uses those. (c) **Every root
      step under the helper**: the builder-private parent no longer exists under the host's `/var/lib` at all: the
      synthetic-manifest Python, the commit, the controlled host build and the verifier check run in
      `scripts/lib/opkit_fixture.sh` under `ns_run`, and hand the parent (keys, owners and modes intact) out through
      a 0700 stash outside `$WORK` (`chmod -R a+rX "$WORK"` would have made the keys world-readable, which the
      first run of this change showed); later namespaces restore it with `OPKIT_RESTORE=STASH=DEST`. The forged
      host-record is made the same way. (d) **The canary check** was a negative lookup that passed when
      `/proc/1/root` was unreadable; the proof now examines each destination through the host's view, refuses if it
      cannot (an unreadable view; M2511), and compares its device:inode with the namespace's own (M2268). The
      "not a tmpfs" refusal (M2266) is attacked with a shadow that is a DISK directory (a different object), because
      an unshadowed directory is also caught by the identity comparison. The
      canary-visibility test was dropped: whenever a canary could be seen through the view, the two objects are the
      same device:inode, so it could not be killed independently of the identity check. (e) **`OPKIT_*_FOR_TEST`**
      are honoured only when the outermost script of the shell is `scripts/test_opkit_ns.sh` itself (which now
      re-executes itself inside each namespace so the helper sees its caller); any other caller that sets one is
      REFUSED, never silently weakened (M2512). (f) **The drift regex** accepted `ns_run true; bash "$KIT"
      --apply`. It now splits each logical line into simple commands at `; && || | & $( ( ) { }` and backticks
      outside quotes, drops comments, skips `VAR=value` prefixes and the `refused LABEL PATTERN` test helper, and
      requires the kit / `--apply` / controlled-build command ITSELF to be an `ns_run`, `kit` or `inns` command
      (M2513, M2515). `--selftest` plants seventeen bypass shapes (including the reviewer's) and seven controls.
      M2265-M2269 were re-targeted to the new code text (M2268 is now the identity comparison).
    - **Rows with no cargo kill, said plainly.** (i) The kit's `--build-uid` refusal: shell guard, hand removal
      above. (ii) `ns_run`'s namespace FLAGS and the proof's per-namespace comparison are two layers on one
      property: removing the flags alone is refused by the proof (`ns_run refused ... the pid namespace is the
      host's`, exit 97, the command never ran), removing the comparison alone survives (equivalent: the flags
      still isolate), and removing both makes the attack succeed (`ATTACK: ns_run left a namespace shared with the
      host`). Run by hand on a scratch copy of `scripts/`; no row, and nothing here is counted killed.
    - **Evidence.** The full `test_operator_deploy.sh` passed on this host as root with the fixture, host build,
      forged record and every attack under `ns_run` (including `--fabric-pid 1`, which now names the namespace's
      own init: the unit judgement does not depend on it). Host listings (`/etc/axon`, `/usr/local`, `/var/lib`
      names, `/etc/systemd/system`, users, groups, setuid files, enabled units, sums of passwd/group/shadow) were
      identical before and after the full run and after the hand-mutated kit run (`/var/tmp/be5/host.before`,
      `host.after1`, `host.after2`). No kit `--apply` ran outside the helper, on any host.
    - **Not executed / not claimed.** The real kernel `make` path (its steps pass through the same PID namespace,
      reap and lock; no new kernel-specific attack); the `kernel` command's tree extraction and `chown` still run
      before it takes the lock (no build-uid process exists yet); a host whose `/etc/resolv.conf` points into
      `/run` would lose DNS for the fixture's download inside the namespace (this host's points into `/mnt`);
      the lock directory `/run/axon-guest-build-locks` is created on the host by the cargo tests (they run
      `begin` for real) and is left there, empty of anything but root-owned lock files.
## Amendment 96: four more routes of one class, and the sweep that enumerated the rest (C9 round 9, PSV1G)

96. **Source: the round-9 PSV-1 and SENTINEL reviews (two executed blockers; two minors) and the sweep the
    brief asked for.** Mutation ids M2470-M2480, matrix rows A190-A194 (another branch uses A195+; the
    integrator renumbers). The class, stated once so the sweep has a definition: a value that the
    CANDIDATE produced reaches operator code, or an operator-defined name is looked up from sealed code,
    on a path that does not pass the seal edge for it (`seal_call`, `seal_global`, `seal_method`,
    `seal_dispatch`, `seal_width`, `seal_refine`, the strict cast of a crossing). Four instances in two
    rounds (`&mut` write-through, first-class fn values, and now two more) is the evidence that fixing each
    route where it was found does not converge; this amendment fixes the primitive each time it can and
    adds a drift test where a source walk is natural.
    - **BLOCKER (PSV-1): `sandbox_run`'s result had no seal edge.** `sandbox_run(sb, "fn", arg)` calls
      `call_fn` on the fn named by the string and returned `ok!(v)` for any value; only the CANDIDATE's own
      declared return type was applied (`-> u8` is satisfied by `4 as u8`). `interp/pin.rs` treats a builtin
      whose declared return names no type variable as determined, and `sandbox_run` declares `-> i64`, so
      `let r = sandbox_run(..)` was determined: `r.ok()` dispatched the lenient `u8` impl, and
      `(r << 1) == 254` with `-> u8 { 255 as u8 }` wrapped at `u8` (both executed through the runner to a keyed
      pass; the pinned form `let r: i64 = ..` was already refused by the cast). Fix, at the builtin: when the
      callee is a SEALED fn and the caller is operator code, the result must be an `i64` (a unit result is the
      documented `0`), else the call is refused at the crossing (M2475). The brief offered a second route, marking
      `sandbox_run` open in the pin analysis (`builtin_ret_open`); it was NOT taken: with the cast in place every
      value the builtin returns for a candidate callee IS an `i64`, so open-marking would add a refusal on
      honest suites and, worse, would turn M2475's attack into a REFUSED_ELSEWHERE (the row could no longer be
      killed by its own attack).
    - **BLOCKER (SENTINEL): two fast paths read operator globals without `seal_global`.** `Expr::FieldAccess`
      and `Expr::Index` read `self.globals` directly for an identifier receiver (their comment said the lookup
      is "the Ident arm's, verbatim"; it was not). Executed (interpreter level, because the resolver's E0004
      refuses these names statically in a real run): `TABLE[0]`, `CFG.k`, `PAIR.0`, `N[0][0]` and
      `|| TABLE[1]` all completed; `let t = TABLE; t[0]` was refused. Fix: ONE lookup, `Interp::global_ref`,
      applies `seal_global` and every read goes through it: the identifier arm (M2470), the field fast path
      (M2471), the index fast path (M2472), and the module-level closure constant call, which had a THIRD
      pair of raw lookups of its own (M2473); the helper itself keeps the edge (M2474). `is_global` answers
      existence only (no value). DRIFT: `every_global_read_goes_through_global_ref` lists every non-comment
      use of the `globals` map under `crates/axon-core/src/interp*` with its reason (the definition, the
      session post-run report, the pin analysis' name set) and fails on any other.
    - **MINOR (SENTINEL): a candidate calling its OWN fn value with an argument was refused.** `let g = inc;
      g(n)` in candidate code read as sealed code calling an operator closure, because a fn value carried no
      mark. A fn value a SEALED frame takes now carries `SEALED_FNVAL_MARK`; `closure_args_check` treats a
      call of such a value as not entering operator code; the operator's own fn values stay UNmarked, so the
      strict argument refusal stands for them (M2476: marking them too lets sealed code call an operator fn
      value with an undetermined argument). The mark is deliberately NOT `SEALED_CLOSURE_MARK`: the value's
      forwarding body still runs in the operator frame and reaches the candidate fn through `call_fn`'s own
      crossing (strict return cast), exactly as before, so a candidate fn value handed to the operator is cast as it
      was. **Found by the sweep, same mark:** the held-value judgement (`replaced_ok`, amendment 72/78) knew only
      the lambda's mark, so a candidate FN VALUE written into a dict slot where the operator held a closure was not
      "the candidate's" and replaced it unrefused (the attack is a test; M2477 removes the second mark and it
      completes).
    - **MINOR (honest cost): the value of a `with handler` expression is undetermined.** Stated next to
      amendment 94's cost. The reviewer's wording was "dispatch inside `with handler` bodies and arms is always
      refused as undetermined"; MEASURED, that is narrower: an operator dispatch on a value that is the RESULT of a
      `with handler` expression (a handler arm may answer with any value) is refused until it is pinned
      (`let v: i64 = with handler ... { ... }`), while dispatch on a pinned or determined value INSIDE a handler
      body or arm completes (`the_value_of_a_with_handler_expression_is_undetermined_until_pinned`, three
      cases). It is a fail-closed over-refusal and is kept: `Expr::WithHandler` is `false` in `Ctx::det`.
    - **THE SWEEP (found by it: one more open route).** `-x` and `~x` on a fixed-width integer
      (`eval_unary`) wrap at the candidate's chosen width exactly as `x + y` does, and amendment 83's width arm
      (`seal_width`) was the BINARY arm only. Executed: `(-xs[0]) == 252` and `(~xs[0]) == 251` completed for
      `xs = arr_map([1], work)` with `-> u8 { narrow(4) }`. Fix: `seal_width_unary` and a unary site in the pin
      analysis (`unary_key`, `determined_unary`), M2478 (the guard), M2479 (the site is determined only when its
      operand is), M2480 (the refusal). A width the OPERATOR chose (`as_u8(5)`) is determined and wraps as
      written (control). Comparisons, `to_str`, interpolation, `wrapping_*`, a `match` literal and an `as_*`
      conversion were examined and stay un-refused ON PURPOSE: they observe the VALUE, which a width cast
      the candidate wrote has already fixed (`255 as i8` IS `-1`), so the candidate gains nothing it could not
      write as `-1`; wrap is what differs, and wrap only happens in the arithmetic arms.
    - **SWEEP LIST.** Method: (1) every call of `call_fn`, `call_closure`, `call_local_closure`, `call_fn_mut`,
      `run_named_fn*` and the goal/scheduler runners in `interp.rs`, `interp/{builtins,eval,goal,proptest,
      conform,value,provenance}.rs` was listed by grep and classified by what the result becomes; (2) every
      lookup of an operator-defined name (`globals`, `fns`, `methods`, `structs`, `enums`, `refines`, handlers,
      closures) and every cache or memo (`resolved_callees`, `fn_cx_cache`, `dict_snaps`, `chan_contracts`,
      the pin analysis' `determined` set) was listed by grep and judged; (3) the routes below were EXECUTED
      (a `judged_on` attack with the operator side `.ok()` or a shift, and an honest control). The ones that
      stay closed are pinned in `the_sweep_routes_stay_closed`.
      - **OPEN, FOUND AND FIXED:** `sandbox_run` result; `FieldAccess`, `Index`, closure-constant-call global
        reads; a candidate fn value replacing an operator closure; unary `-x`/`~x` width. (The fn-value call
        refusal is the over-refusal minor above.)
      - **CHECKED AND CLOSED, with the evidence:**
        `arr_map`, `arr_fold`, `arr_zip_with`, `arr_max_by`/`arr_min_by`, `arr_find`, `arr_filter`,
        `arr_partition`, `arr_take_while`/`arr_drop_while`: the result's declared type names a type variable
        (`[U]`, `T`), which `builtin_ret_open` treats as undetermined, and the pinned form is refused by the
        cast (executed). `dict_map_values`, `dict_filter`, `arr_group_by`: the result is a `Dict`, an untyped
        position (amendment 83). `arr_sort_by`, `arr_any`, `arr_all`, `arr_count_if`, `arr_sum_by`: the
        builtin inspects the closure's result for an exact `i64`/`bool`/number and refuses a `u8` (executed), and
        builds its own result. `scheduler_result` and `goal_best_input`: `numeric_score` reduces the result to a
        number the builtin builds; `.ok()` lands on the `i64` impl (executed). Every `goal_*`, `kernel_goal_run`,
        `scheduler_run`, `supervisor_run`: scalar results built by the builtin. `http_sse`/`http_sse_post`:
        the callback's result is dropped. A candidate lambda or fn value returned to the operator, a candidate
        channel, a forged `Uncertain`, an `Option` or an array element, a candidate global array, tuple or
        struct field, a candidate `impl` of an operator trait method (seal_method), a candidate `@[adaptive]`
        result: refused or typed by the return cast (executed, each with the guard's own message). Operator-side
        propagation of an undetermined value through `let`, a tuple pattern, a `match` binding, `Some`,
        a lambda parameter, an `if` branch, a block tail, reassignment, an array literal and a `for`
        variable stays undetermined (executed). `call_fn_frame` is reached only from `call_fn_sealed`
        (`call_fn`, `call_fn_mut`, so `eval_call_mut` too): every path to a fn body passes `seal_call` and the
        crossing cast (read, `grep call_fn_frame`). `resolved_callees` caches a `&FnDef` BEFORE `call_fn`, which
        applies `seal_call` on every call (read). `dict_snaps` and `chan_contracts` hold a `Weak` and check
        `ptr_eq`, so a reused address cannot inherit a snapshot (read). `fn_cx_cache` is keyed by a `FnDef` address
        of an immutable program (read). The pin analysis' site keys are `(owner fn, hash of the site text)` and
        its bindings are fn-wide by name, any unpinned fact removing the name (conservative; read). A candidate
        defining a test or helper name the suite defines is E0002 (executed through the runner with `--exact`).
        `assign_in_place` (`x = x + y` on a str/array local) falls back to `eval_binop_vals` only with an array
        or string slot, which has no width. A string-named fn (`sandbox_run`, `scheduler_spawn`, `goal_run`)
        naming an operator fn is refused by `call_fn`'s `seal_call` at the call (executed, `B7`/`B9`/`B10`).
      - **NOT EXAMINED, or known and left:** (a) an EXISTENCE ORACLE: a sealed `sandbox_run(sb, "name", 0)` or
        `goal_run("name", ..)` says "no function" for a name nothing defines and "cannot run" for an operator
        fn, so a candidate can probe which operator fn names exist (not their bodies). Not changed here; a fix is
        to answer both identically. (b) `Value::Handle` (native-module handles) crossing the seal was not
        examined. (c) `builtins.rs` (about 6,500 lines) was NOT read line by line: the user-code call sites
        were found by grep and the declared-return table was scanned for closed-return builtins that take a
        `Dict`, an array or a generic argument; no arm found returns an argument unchanged under a closed type,
        but that is a scan, not a proof. (d) handler arms and the multi-shot replay were not re-examined beyond
        the cost stated above. (e) Native codegen is out of scope: every claim here is the interpreter's.
      - **DRIFT.** `every_builtin_that_runs_user_code_is_classified` lists the 32 builtin arms that run user
        code (found by a source walk of `call_builtin`) with one of four dispositions (open by a type variable,
        a `Dict`, a scalar the builtin builds, or the seal crossing in the arm itself) and checks the declared
        return against it; a new arm, or a changed return type, fails until it is classified. goal.rs's
        `call_fn` sites must reduce their result to a score or a bool within the same function.
    - **Honest-program cost.** (a) `-x` / `~x` on a fixed-width integer the operator did not pin is refused (the
      binary form already was): `let w: u8 = ...` or `as_u8(..)` first. (b) The value of a `with handler`
      expression must be pinned before an operator dispatch on it (stated above, with amendment 94's cost (a)).
      (c) A call through a candidate fn value passes the strict return cast of a crossing, as an operator call of
      that fn did; a candidate generic fn cannot be taken as a value at all (the resolver). (d) A candidate callee
      of `sandbox_run` must return `i64` or `()`; one returning `bool`, `str` or a struct is refused (before, the
      value passed through typed as the callee declared). Honest suites using `sandbox_run` on their own fns
      are untouched. Nothing else changed for honest suites (controls in each test).
    - **PSV-3 text MINORs.** The verdict spec said "origin/main's 13 interpreter commits"; 13 commits under
      `crates/axon-core/src` were merged, of which 5 change the interpreter (shared Rc arrays, shared strings
      with lent closure captures, `&mut` write-through, first-class fns, the `arr_sort_by` rewrite) and 8 are
      native codegen, build/cache and CLI-help changes. The verdict spec and the delta note now say that;
      amendment 94's own sentence above keeps the loose phrase as history. The delta note re-ran "the 30 gate
      rows" and its per-delta mutation table stopped at amendment 83; both are brought up to date (amendments
      88, 94 and 96 rows, the gate-row count derived, the mutation list recomputed from `MUTATIONS`).
    - **Evidence.**
      Unit tests `interp::tests::{sandbox_run_results_are_cast_at_the_seal_crossing,
      a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path,
      a_candidates_own_fn_value_takes_arguments_and_an_operators_still_does_not,
      operator_unary_arithmetic_never_runs_at_a_width_the_candidate_chose, the_sweep_routes_stay_closed,
      every_global_read_goes_through_global_ref, every_builtin_that_runs_user_code_is_classified,
      the_value_of_a_with_handler_expression_is_undetermined_until_pinned}`; runner tests
      (`axon_psv::runner::run`) `sandbox_run_hands_the_operator_an_i64_or_nothing`,
      `operator_negation_never_runs_at_a_width_the_candidate_chose`,
      `a_candidate_calls_its_own_fn_value_and_never_reads_an_operator_global`; nine `am96` rows in
      `scripts/v022_pci_gates.sh`. Measured: rows M2470-M2480 and every other active axon-core row (178 in all) were run
      by `v022_g01_mutations.py --scope=all --only` on gpumaster from a clean clone of 29276036, three shards:
      178/178 KILLED by their own `ATTACK:` marker, 0 REFUSED_ELSEWHERE, 0 survivors, 0 stale (M1673 and M2376 old
      text re-anchored to the new forms). Local, exit codes: `cargo test -p axon-core --no-default-features
      --no-fail-fast` rc 0 (788 lib tests among 25 result lines); `cargo test -p axon-psv --no-fail-fast` rc 0 (63
      passed); `cargo clippy -p axon-core -p axon-psv --no-default-features --all-targets -- -D warnings` rc 0;
      `cargo fmt --check` rc 0; `v022_pci_gates.sh` rc 0 (49 rows); `psv_matrix_check.py` rc 0 (194 rows);
      `v022_refusal_coverage.py` rc 0 and `--freeze` rc 0 (a new `is_global` exemption and M2480 for the unary
      refusal); `pci_delta.py --check` rc 0. NOT run: the retired-row four-cell paired-disable, the full
      `--scope=all` run, the sibling crates (`axon-fabric` etc.: no file they build was changed).
## Amendment 98: what the refusal gate claims, the guest runner's paths, fail-open defaults and the build environment's Python guards (C9 round 9, eqgate5)

98. **Source: the round-9 EQUIVALENCE review (`DO_NOT_REGISTER`; one BLOCKER, three MAJOR-ADJACENT).** Mutation ids
    M2520-M2569 (all 50 used), matrix rows A201-A206 (other branches hold A190-A209; the integrator renumbers; this
    branch's matrix check was run with temporary placeholders for A190-A209, which are not committed). Base
    `c9r8/integrate7` (`ac03128f`); nothing under `crates/axon-core/src` was touched, and neither
    `scripts/guest_build_env.py` nor `scripts/lib/opkit_ns.sh` nor the kit (buildenv5's), whose guards this
    amendment only OBSERVES, through a new test file.
    - **The claim, reworded (finding 2).** "Every guard has a row or a checkable exemption" was not literally true:
      amendment 95 had called a REMAINDER entry "a guard no test observes". The claim is now exactly this: **every
      refusal site of every in-scope file has a mutation row, a CHECKABLE exemption (dominated, unreachable, not on the
      protected route: a fact a reviewer can re-execute), or an entry on a counted, greppable REMAINDER list of
      guards no test observes alone, and REMAINDER is NOT claimed covered.** The gate prints the counts by category on
      every run (`REMAINDER: N ...`), lists each site with `--remainder` (`grep REMAINDER`), and its last line says
      so. A second count, `OBSERVED-NOT-ROWED`, is a MEASUREMENT: a guard whose removal a named test fails in a survey,
      with no row of its own. It is not a row and not a claim of domination. Reworded in
      `v022-protected-suite-verdict.md` (new section), `gate.sh`, `v022-psv-protocol.json` and here.
      **REMAINDER before and after (gate-computed, Rust files; the Python file was not read before):**
      173 -> 149. Before: okor_field 68, const_tag 42, const_path 14, const_bound 12, const_table 10, const_text 9,
      other 9, const_other 5, const_exit 3, unlink_job 1. After: const_tag 42, okor_unjudged 16, const_path 14,
      okor_nodefault 12, const_bound 10, py_guard 10 (new: the Python file), const_text 9, other 9, const_table 7,
      okor_offroute 7, const_other 5, okor_closed 4, const_exit 3, unlink_job 1. `okor_field` is gone as a category:
      each of its 68 entries now says what a survey found.
    - **The runner's `guest_config` (finding 1, BLOCKER).** A new unit test asserts EVERY field literally: manifest,
      secret, candidate, suite, out, the interpreter that is handed the secret, the runner's own identity and the guest
      policy (M2520-M2527, each KILLED by its own `ATTACK: guest_config <field>`). Is a swapped SECRET path observed
      downstream? Measured only in part: `runner_exe`/`axon` digests reach the verdict and are compared host side
      (reviewer's reading, not re-executed here), and a candidate-chosen secret would be refused by the host's MAC check
      on a verdict it cannot forge; no test pinned the guest's own path, and none does now except this one. New gate
      FORM (`VALUE_FORM`): a struct-literal field whose value is an ABSOLUTE `PathBuf::from("/..")`/`Path::new("/..")`
      (M2554). It found four more: `AllowlistSource::operator()`'s walk base, `Authority::production()`'s walk base,
      `ReadinessTrust::operator()`'s walk base (all `/`, none observed: M2528-M2534, tests added; the allowlist PATH,
      the production authority's uid and `test: false`, and the readiness allowlist are rowed too) and the guest
      interpreter's registry LABEL in `submit.rs` (UNUSED, checkable: only `host_executor` reads `exe.path`, never
      for the protected profile).
    - **Fail-open defaults (finding 3).** `rules.rs`'s `independent_units.ok_or(..)?` -> `.unwrap_or(0)`: the suite stayed
      green and a rules document omitting it carried no minimum sample of distinct tasks (M2535); `candidate_budget`
      (M2536, fails closed at admission, rowed all the same) and the keyed rules (M2537). The ids.rs scheme checks
      (labelled REMAINDER while killed by `ref_parts`) are now rows (M2538, M2539) and the test also refuses a digest
      with NO scheme, which `unwrap_or(("cl22", s))` had accepted (M2538 survived before this test).
      **Survey table (68 `ok_or` sites, 56 of them left after the rows).** Method: replace the refusal by its permissive
      default and run the owning crate's suite (axon-fabric sharded on gpumaster, clean clones; the rest the same).
      Result column: S = suite stayed green, K = killed by an existing test. Rowed after a new test: S10 assertions
      list (M2560), S12 manifest names no guest interpreter digest (M2561), S14 custodian check without expiry reads
      as never expiring (M2562), S32 observed_at that is not a timestamp reads as now (M2563), S44 observer
      `max_age_s` not a number reads as no limit (M2564), S23/S24 tree entry id cut short / mode not octal read with a
      zero id / a regular file (M2566, M2567), plus the three in rules.rs and two in ids.rs above. Killed by an existing
      or new test, now OBSERVED-NOT-ROWED naming it: S11, S17, S20, S40, S41, S43 (its default 0 is also refused by
      `custodian_is_separate`), S57, S66, S67. Checkable DOMINATED (an outcome-equivalent later refusal, named in the
      entry): S34, S39, S45, S47, S48, S56, S65, S68. Survived and fail CLOSED (reason given): S08, S09, S13, S26. Survived,
      direction not judged: S15, S16, S18, S19, S21, S22, S33, S35, S36, S42, S50, S54, S55, S62, S63, S64. No neutral
      default exists (an absent lookup of an entity; not mutated, S25's default would loop): S25, S27-S31, S37, S38,
      S46, S49, S51, S61. Off the protected route: S01 (axon-attest; S, permissive) and the six Cortex sites (not surveyed).
      **Constants.** Tables and bounds whose test reads the constant itself survive any change of it. Pinned from outside
      and rowed: the nine `REFUSED_CALLER_FLAGS` (K01-K09 all killed by the new literal test; M2565), the nine
      ledger-dependent directories (`plans` could be dropped with the suite green; M2569), `sealed_exec`'s MAX_BYTES
      (the test sized its file from the constant; M2568). Killed by existing tests (OBSERVED): the observer's MAX_REQUEST,
      PROTECTED_PROFILES. Still S, REMAINDER: observer MAX_REPLY and MAX_PROFILE_MANIFEST, backend `read_regular`'s MAX,
      certcheck MAX_DEPTH. NOT surveyed: custodian MAX_OUTSTANDING (2^30 hangs the suite, as amendment 95 found) and the
      launcher's MAX_REQUEST. A no-op survey edit (adding `pattern` to ANNOTATIONS) was withdrawn and redone with `format`
      (killed). A row on one entry of a table credits the whole table site in the gate; the other entries are covered by
      the same test (the flag list: all nine killed), not by a row each.
    - **The build environment's Python guards (finding 4).** `crates/axon-fabric/tests/guest_build_env_guards.rs` (cases
      in `tests/guest_build_env_guards/*.py`) drives the REAL script's functions from a scratch copy inside a private PID
      namespace, as root and as uid 4242 (so that `AXON_GUEST_BUILD_UID` = root and = the builder's own uid are different
      cases): build_ids, require_runner, pinned_channel/rustup/toolchain, ancestors_of/_problem, build_parent,
      reach_problem, operator_file_problem, builder_pin, proof_key/proof_problems/write/measure_problem, shape_problems,
      entry_problems, the host record, dist and rootfs judges, kernel_problems, toolchain_pin_problems, copy_tracked_tree,
      git_clone, committed_file, private_toolchain, dist_record, pinned_copy, rootfs and kernel up to the real toolchain,
      cargo_step against a stand-in toolchain (config held at begin, before, during), and the command line. The gate now
      reads `scripts/guest_build_env.py` (`py_sites`, from the syntax tree: `fail`/`die`/`refuse`, `sys.exit(<message>)`,
      `raise <exc>`, a `return` of a non-empty message; the guard is the enclosing `if`): **109 sites, 45 rowed (40 by older
      rows, 5 more by M2540-M2553), 54 OBSERVED-NOT-ROWED by `scripts/v022_py_guard_survey.py` (each guard removed, a named case
      failing), 10 REMAINDER** (begin, cargo's host build, the reaper, `cp` failing, and the operator-/etc-dependent verbs:
      each needs the real toolchain, root with a kernel fault, or a pin under /etc). Exemptions are keyed by (function, n-th
      site, fragment) so a line shift keeps them and a moved site is refused. Rows M2540-M2553 are terms and values
      (`int(raw) == 0`, `== os.geteuid()`, the regex term, `require_runner`, the two ancestor terms, the recorded-mode term,
      the pin's `bu == 0`/`bu == uid`, the judging parent, `shape_problems`' build uid, the pinned build uid, the constructed
      PATH, the host-tool search path).
    - **Gate rows and planted forms.** M2554 (path field), M2555 (Rust REMAINDER counted), M2556 (Python `fail` is a site),
      M2557 (a Python exemption's fragment is checked), M2558/M2559 (Python REMAINDER / OBSERVED counted). The planted-form
      tests are `an_absolute_path_literal_handed_to_a_config_field_is_a_site`, `a_remainder_exemption_is_counted_and_never_
      claimed_covered`, `a_python_refusal_is_a_site_judged_by_its_own_guard`, `a_python_guard_exemption_is_counted_by_kind`.
      M2427 changed text (its line gained an alternative).
    - **WHAT THE GATE STILL CANNOT SEE.** (1) Everything amendment 95 listed (2)-(9) still stands, except (6) for the one Python
      file. (2) Python: a guard that is an expression VALUE (`return bool(..)`, `all(..)`), a refusal by a subprocess's exit
      status, a compound `if` is judged whole not per term (the terms of build_ids, builder_pin, ancestors and proof_problems
      have rows, the rest do not), the other scripts (`scripts/*.sh`, `linux_profile_manifest.py`, the kit). (3) Python
      survey kills are a measurement: they depend on the test file as committed, and `v022_py_guard_survey.py` must be re-run
      when either changes. (4) `okor_unjudged` is a list of guards for which only "the suite stayed green" is known.
      (5) A REMAINDER entry of any kind is not covered. (6) The path-field form sees `field: PathBuf::from("/..")` on one
      line, not a path built by `join` or a `const` path (the const rule sees that). (7) The cases of
      guest_build_env_guards.rs that depend on the host (a builder pin absent under /etc) are made only where it is absent.
    - **Unfinished, stated.** The 16 `okor_unjudged` and 10 `py_guard` REMAINDERs; the nonce-expiry and tree-parse sites'
      siblings S15/S16/S21/S22; MAX_REPLY, MAX_PROFILE_MANIFEST, backend MAX (their tests would need a stand-in observer that
      streams past the bound, or a multi-hundred-MB file); the K16/K17 constants were not run.


**Renumbering at integration (round 9, integrate8).** Amendment 97 (buildenv5) wrote its matrix rows as A200-A205 and amendment 98 (eqgate5) as A210-A215 while amendment 96 (psv1g) holds A190-A194; the integration made the matrix contiguous: amendment 97's rows are now A195-A200 and amendment 98's are A201-A206. The text of those amendments was rewritten to the new ids.

## Amendment 99: the reconciliation of the build environment's two rewrites (C9 round 9, integrate8)

Amendment 97 (buildenv5) rewrote `scripts/guest_build_env.py` (PID namespace, dedicated build uid, per-uid lock,
`begin` split into `begin` and `_begin`, the kit verb `check-build-uid`) while amendment 98 (eqgate5) taught the gate to
read that file with exemptions keyed by (function, n-th site, fragment). Merged, the gate refused: 17 BAD lines. This
amendment records what was re-anchored and re-judged. `crates/axon-core/src` and the interpreter are untouched; nothing
was weakened, no test or guard was relaxed.

- **Re-anchored rows.** M1473: its old text (`if st.st_uid != 0 or st.st_mode & 0o022:`) now occurs in three places
  (the toolchain copy, the lock directory, the operator file); it is extended with the next line (`return (f"{cur} (uid`) so
  it applies once, in `operator_file_problem`. M2221: `as_build_uid` now returns `unshare ... setpriv ...`; its old text was
  the pre-amendment-97 return, so it now replaces the whole new return with `return list(argv)` (M2501 keeps removing only
  the `unshare` prefix). Both still killed by their own attack.
- **New rows M2570-M2576** (INTEG8's range): M2570 root, M2571 the builder's own uid, M2572 a service uid (each in
  `build_uid_problem`, the kit verb's ONLY judge: `build_ids` also refuses root and the builder's uid before it asks, which
  is why the existing M2541/M2542 did not observe them); M2573 `as_build_uid` refuses a missing `unshare`; M2574 the verb's
  decimal check; M2575 the verb fails on the verdict; M2576 `build_ids` fails on the verdict (M2502 changes the line BEFORE
  the guard, so the guard line itself had no row). Attacks: `guest_build_env.rs::the_build_uid_may_not_be_a_service_uid`
  and new cases in `guest_build_env_guards/misc.py` and `ids_root.py` (the CLI verb with `0`, the builder's uid, a
  non-decimal, a dedicated uid as control; `build_uid_problem` directly; `as_build_uid` with `UNSHARE` absent). These are
  test additions, not relaxations.
- **Test fixed because the script changed.** `ids_root.py`'s expectation of `as_build_uid` was the pre-amendment-97 argv
  (no `unshare ... --kill-child`); it is now the new one. Found by reading the diff, not by a failing run: the edit was made
  before the first run. No guard regressed. The other eqgate5 cases, and all of buildenv5's, pass unchanged
  (`guest_build_env` 38, `guest_build_env_guards` 7, gpumaster).
- **Exemptions re-keyed.** `begin` 1/2 became `_begin` 1/2 (same REMAINDER, same sites; `begin` itself now holds the lock and
  calls `_begin`). `main` 7 and 8 became 9 and 10 (two `check-build-uid` sites were inserted before them; 9 is OBSERVED, 10
  REMAINDER as before). One new REMAINDER: `build_uid_lock` 2, the handler turning the OSError of the lock file's
  `O_NOFOLLOW` open into a refusal; the symlink case still refuses with it removed (the OSError propagates uncaught, no lock
  is taken), so it is a message and an exit path, and `LOCK_IS_SYMLINK` is observed by the flag, not by the handler.
  Counted, not claimed covered.
- **Counts for `scripts/guest_build_env.py`.** Before (eqgate5 alone, old file): 109 sites, 45 rowed, 54 OBSERVED-NOT-ROWED,
  10 REMAINDER. After: **119 sites, 54 rowed, 54 OBSERVED-NOT-ROWED, 11 REMAINDER** (+10 sites: `build_uid_problem` 3,
  `build_ids` 1, `as_build_uid` 1, `build_uid_lock` 3, `main` 2; +9 rowed; +1 REMAINDER). `scripts/v022_py_guard_survey.py`
  on the final script (gpumaster, `axon-fabric` guards test): 119 sites, 104 killed, 15 survived; the survivors are the 11
  REMAINDER sites plus `build_uid_problem` 3, `build_ids` 2, `build_uid_lock` 1 and 3, each rowed (M2572, M2576, M2507, M2508)
  and killed by `guest_build_env.rs`, a test the survey does not run. No OBSERVED exemption survived.
- **Four gate rows.** M1946 and M1949 SURVIVED at integrate7 for one reason: their planted probe line carried a SECOND form
  (`libc::renameat2(` is a BUILD_FORM; `MS_NOSUID` is a VALUE_FORM), so removing the flag alternative left the line a site.
  The probes in `refusal_coverage_gate.rs` (`FLAG_PROBES`) now isolate the flag (`let p = libc::RENAME_NOREPLACE;`,
  `libc::MS_NODEV`), and M1949's marker names MS_NODEV; the two rows are killed by their own attack. M1740 and M1950 were
  `not_applicable`: the text they replaced had been edited (`kind in ("line", "const", "form")`; `_some_reason_is_value(c)`
  inside `is_form`). Re-anchored to the current text; killed (see the run record below). M1394 (not in the coordinator's
  list) read REFUSED_ELSEWHERE once psv1g put an exemption (`is_global`) inside the anchored region: the control ran first on
  the mutated gate and failed on that exemption before the planted site was judged. The test now judges the planted site
  first (`names`) and runs the control on a second copy afterwards. No guard was weakened: the attack asserts what the row
  claims, that the planted site is reported.
- **Unfinished.** The 11 `py_guard` REMAINDERs stand, as the 150 REMAINDER sites overall (this amendment adds one).
  `test_opkit_ns.sh` and the drift selftest pass; the ns-dependent suites were run on gpumaster for the namespace-free
  parts only (amendment 92's calibration notes still hold).

## Amendment 100: a handler arm is judged by the fn that installed it, and a name the candidate chose never selects an operator fn (C9 round 10, PSV1H)

100. **Source: the round-10 PSV-1 review (two executed blockers; the round-9 `sandbox_run` blocker is closed), the SENTINEL
     reviewer's two minors, the PSV-3 reviewer's two text minors and an existence-oracle minor.** Mutation ids
     M2600-M2624, matrix rows A208-A212 (another branch may use A220+; the integrator renumbers). Base
     `c9r9/integrate8` (`d63416c4`), source commit `cc74956e`. `crates/axon-core/src/interp*` changed; classified in
     `scripts/pci_delta.py` as narrowing.
     - **BLOCKER A: handler arms and continuation replay ran under the WRONG PIN OWNER.** Pin sites are keyed
       (owning fn, hash of the site text) and every lookup uses `Interp::pin_fn`. `run_handler_arm` and
       `replay_continuation` never set it, so an arm body ran with `pin_fn` = the fn that PERFORMED the effect, not
       the fn that INSTALLED the handler. If the performer was another operator fn holding an identical, determined
       site text (`let v: i64 = 9 ... v.ok()`), the arm's undetermined `v.ok()` was accepted (executed through the
       runner: candidate `-> u8 { 4 as u8 }` completed; the controls, an i64 4, the honest 9 and a helper without the
       colliding site, behaved; the same through the general replay arm and the arithmetic arm `(v << 1) == 254`
       with `255 as u8`; a closure made inside an arm inherited the wrong owner too). Fix, at the source:
       `HandlerFrame` and `ResumeCtx` record `pin_owner` (the installer's `pin_fn` when the `with` was evaluated)
       and the arm, and the replay of its continuation, run under a `PinGuard` for that owner. The closure mark
       (`PIN_FN_MARK`) already takes the owner it is created under, so a closure made in an arm now carries the
       installer's. **Not done, and why.** The brief suggested also keying a pin site by its AST address. The AST is
       CLONED into the handler frame, the arm, the lambda body and the replay snapshot, so an address is not stable
       and keying by it would make every cloned site undetermined; the key stays (owner, text), made sound by giving
       every frame the right owner, and `identical_site_text_in_two_fns_gets_two_verdicts` pins that two fns with
       identical text get two verdicts. **The replay guard has no row of its own:** under the resolver's rule that
       `resume` is bound only inside an arm, the replay always runs under the arm's already-correct owner (a closure
       made in the arm carries it), so removing the replay guard changes nothing observable. It is kept as the
       second place that states the rule; it is an equivalent mutant, not claimed killed.
       DRIFT: `every_frame_that_runs_stored_operator_code_sets_its_owner`: the fns that construct a `PinGuard` are
       exactly `call_fn_frame`, `call_closure_owned_by`, `run_handler_arm` and `replay_continuation`; the last two
       must use the recorded `pin_owner`; and every other fn that evaluates a stored AST body (a fn body, a
       predicate, a global initialiser) is listed with the reason its owner is already right.
     - **BLOCKER B: a candidate-returned STRING selected which operator fn ran.** `sandbox_run`, `scheduler_spawn`,
       `goal_eval`, `goal_run` and the other `goal_*` / `kernel_goal_*` builtins resolve their NAME argument with
       `self.fns.get`. The `seal_call` edge fires only in a SEALED frame, and a `str` is a closed type, so a `str`
       the candidate returned counted as determined: the suite `sandbox_run(sb, entry(), 21)` asserting
       `== reference(21)` passed for a candidate `entry() -> "reference"` (and `scheduler_spawn`, `goal_eval`
       likewise). Fix: **the name argument of every name-resolving builtin is a SINK** (`pin::NAME_SINKS`: 20
       builtins, 21 argument positions, `goal_run_constrained`'s constraint included). In operator code the
       expression there must be NAME-PURE (`Ctx::npure`, a second analysis beside the type analysis, judging
       CONTENT instead of TYPE) or the call is refused at the call site (`Interp::seal_name_args`, called from
       `eval_call` before the builtin runs; an unrecorded site is refused). Name-pure means built only from
       operator literals, operator module constants, operator fns whose every return is name-pure
       (greatest fixpoint over the fns), `+`, interpolation, `to_str`/`axon_concat` of name-pure arguments,
       element reads of a literal array with a name-pure index, a branch between name-pure values, a loop
       variable whose both range bounds are name-pure, and locals every one of whose bindings is name-pure.
       Not name-pure: a call of a candidate fn (`sealed_names`), a parameter even when annotated `str` (an
       annotation pins nothing for a name), a dict/channel read, a method result, a loop variable over a range the
       candidate sized, anything the analysis did not see.
       **HONEST-PROGRAM COST (stated, fail-closed).** Under a sealed run, operator code can no longer pass a
       name-resolving builtin a name it received as a PARAMETER (`fn run_named(n: str) { sandbox_run(sb, n, 1) }`
       is refused), read from a dict/array it did not build from literals, or computed by a method; an honest suite
       names the candidate's fn by a LITERAL, by a constant, by a string it assembles from literals, or by a loop
       over a literal range of a literal table. A suite that asks the candidate WHICH fn to run is exactly what is
       refused. Outside a sealed run (`axon run`) nothing changes. The rule does not touch the
       type analysis: a name the operator chose is still not a type pin.
     - **Existence oracle (minor).** A refusal said "sealed code cannot run `secret`, which the operator defines" for
       an operator name and "unknown function" / "no function" for a nonexistent one, so sealed code could probe
       which names the operator defines. For a sealed caller the five paths that report a name that does not
       resolve (`seal_call`, the unknown-callee error, `sandbox_run`, `scheduler_spawn`, the unknown goal name)
       now say the same thing (`Interp::sealed_no_fn`), modulo the name itself (M2622, M2623). **Not covered:** a
       third observation, whether the call COMPLETES for an existing candidate fn versus a missing one, is
       the candidate's own business and not an oracle about the operator.
     - **SENTINEL minors.** (a) `every_global_read_goes_through_global_ref` saw only `self.globals` /
       `interp.globals`; it now fails on ANY mention of the word `globals` (any receiver, a binding, a struct
       pattern or initialiser) outside `interp/pin.rs`, which holds only a `HashSet<String>` of names and is asserted
       to name neither `Interp` nor `Value`. (b) `every_builtin_that_runs_user_code_is_classified` detected four
       literal call patterns; it now closes over the interpreter's call graph from the runner primitives
       (`call_fn`, `call_fn_mut`, `call_fn_frame`, `call_closure`, `call_local_closure`,
       `call_closure_owned_by`; the evaluator, the dispatcher and the host entry points are roots, not nodes; a call on
       another receiver, `ch.send(..)`, is not a call of an interpreter fn), so a NEW runner, direct or through
       a wrapper, is found. It found one the table had missed: `goal_eval` (a `f64` score, `Scalar`) is now classified.
     - **PSV-3 text minors.** `governance/specs/v022-protected-suite-verdict.md` and `governance/notes/v022-pci-delta.md`
       said "the other 8 are native codegen, build/cache and CLI-help changes". Reworded precisely: `378da246` touches
       `interp.rs` and `interp/builtins.rs` only by a `cfg` re-export and a visibility change, and `edfe3e2d` changes
       checker concat typing and resolver capture analysis, so `axon check` accepts different programs; neither
       changes interpreter evaluation (`scripts/pci_delta.py` themes updated to match). The gate row for
       amendment 96's `axon-psv` leg of the global-read edge is relabelled **corroboration only**: that candidate is
       refused statically (E0004) before it reaches the runtime edge, which is the interpreter unit test's with the
       static check bypassed. No runner attack that reaches the arm was added (the resolver refuses the names first).
     - **FUTURE (recorded, not changed).** The native `gfx` and `axon-domain` registries are per-`Interp`, not
       per-kernel: the operator's and the candidate's frames share them, and isolation rests on the unforgeability of
       the handle (a sealed frame cannot name a handle the operator holds), not on separate state. Anything that makes
       a handle forgeable or enumerable breaks the isolation without touching a seal edge.
     - **The sweep of the two new classes** (the first question: does operator code run under a frame other than the
       one that installed or owns it; the second: does a value sealed code chose become a name or a key that selects
       operator state). No completeness claimed.
       - **Found and fixed:** handler arm (bare tail resume and general), continuation replay, a closure made in an arm
         (class 1); `sandbox_run`, `scheduler_spawn`, `goal_run`, `goal_run_constrained` (metric and constraint),
         `goal_run_categorical`, `goal_run_random`, `goal_run_multistart`, `goal_continue`, `goal_eval`,
         `goal_best_input`, `goal_best_inputs`, `goal_best_inputs_f64`, `goal_best_score`, `goal_count`,
         `goal_history`, `goal_clear`, `agent_detect_loop`, `agent_uncertainty`, `agent_trace_len`,
         `kernel_goal_create` (class 2; `kernel_goal_run` and `scheduler_run` run a name already fixed at a sink).
       - **Found, NOT fixed (open):** an operator-built TABLE OF CLOSURES selected by a candidate-chosen key or index.
         `let h = dict_new(); dict_set(h, "double", |x| 0); dict_set(h, "reference", |x| x * 2)` then
         `match dict_get(h, entry()) { Some(f) => assert(f(21) == reference(21)) ... }` with a candidate
         `entry() -> "reference"` completes, and so does `let ops = [|x| 0, |x| x * 2]; let f = ops[idx()];
         assert(f(21) == reference(21))` with `idx() -> 1` (executed through the interpreter on gpumaster; the controls
         `"double"` and `0` fail). It is the same attack as B through a data structure instead of a builtin. NOT
         closed here: the name rule judges a call's argument position, and a selection is an element read, whose
         result is untyped. Design for the fix: a value-level check at the selection primitives (`Expr::Index`,
         `dict_get`, `arr_get` ...) in operator code of a sealed run, refusing a read whose key is not name-pure when
         the value read is an operator-created closure; a static taint on the binding would not cover a closure passed
         on as an argument.
       - **Checked and closed (read, not necessarily with a test of their own):** a replay's owner (redundant, above);
         closures stored and called later (`PIN_FN_MARK`, the creator's owner, now the installer's inside an arm);
         scheduler fibers (`call_fn` sets the fn's own owner; the name passed the `scheduler_spawn` sink, and
         `fn_by_name` applies `seal_call` at resolution); goal metrics and constraints (`call_fn`); the property runner
         (a host entry point through `call_fn`); session cells (one accumulated `main`, never sealed); the
         `return(v) => e` arm of a `with` and `spawn`/`select` bodies (the same fn, the same frame); refinement and
         `@[verify]` predicates (their text uses `_`, `value` or the fn's own parameter names and is evaluated inside the
         owning fn's frame, so no other fn's site shares its owner); global initialisers (owner 0, the owner the
         module-level lets are analysed under); `env_var(name)` (reads, selects no code).
       - **Not examined:** integer HANDLES a sealed value can choose (sandbox, principal, goal, fiber, supervisor ids)
         as keys of operator kernel state, beyond noting that `sandbox_run(sb_chosen_by_the_candidate, ...)` picks a
         ceiling the operator minted; paths and URLs a sealed value chooses and operator code then reads; prompt
         text a sealed value supplies to `ai_complete`.
     - **Evidence.** __EVIDENCE__
