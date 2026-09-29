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
  "suite_store":    "/var/lib/axon/suites",
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
read-only by the launcher. The guest mounts it `ro,nodev,nosuid,noexec,noacl` (amendment 37).

| Device | Content | Mounted at |
|---|---|---|
| `input-0` | candidate tree (the exact WorkspaceVersion from Fabric's store) | `/in/candidate` |
| `input-1` | operator suite tree (from the registry) | `/in/suite` |
| `job` | `launch-manifest.json`, `completion-secret` (mode 0400, root), guest policy | `/in/job` (read-only) |
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
K = HMAC-SHA256(S, "axon-guest-completion/1\n" || sha256(B))
token(test) = completion_token(K, test)    -- the existing PCI interpreter derivation
```

- The runner derives `K` from the manifest it VERIFIED. It hands `K` to
  `axon test --completion-key-stdin --seal /in/candidate` over stdin, and never exposes `S` or `K`
  to candidate code.
- Candidate code runs unprivileged in the guest (`AXON_PATH` = `/in/candidate`, exclusive). The
  secret file is readable only by root.
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
- Fabric verifies and consumes an observation. It holds no observer key and cannot mint one.

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
- the guest digests equal the qualification's;
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
    kills its refactored guard).
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
37. **An input's extended attributes are refused at every layer (PSV-2, C9 round-1).** A POSIX ACL
    entry `user:65534:---` on a 0644 file leaves the mode normalised and the tree digest unchanged,
    yet denies the test uid the file: a correct candidate was driven to a genuine keyed Failed.
    The rule is now about the child's effective view, not a list of shapes:
    - the guest input check refuses ANY entry, the input root included, that carries ANY extended
      attribute, and names it;
    - the launcher stages inputs with a plain `cp -R` (no ACL, no xattr; the exec bit kept) and
      builds each image with `mkfs.ext4 -E no_copy_xattrs`;
    - the guest mounts all three input drives `noacl` (§4 now reads `ro,nodev,nosuid,noexec,noacl`).

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
      (PSV-5). Negative-matrix A65.
    - ADR-002's key separation is checked at the operator roots, not only in the store config.
      `operator_trust::rooted` and `rooted_keys` refuse a key that another operator root also
      holds, so an observer key that is also a verifier (or qualification, admission, monitor) key
      authenticates nothing, in `check_bundle` or in any rooted lookup (PSV-6). The store's
      `check_separation` stays as a second check. The Fabric side is a separate workstream.
      Negative-matrix A66.
    - `check_bundle` refuses a launch manifest in which any `*sha256` field is all zeros. That is
      the placeholder Fabric's `psv::prepare` writes when there is no operator host config
      (`host_config_sha256`, `suite.registry_sha256`). `LaunchManifest::verify` in `axon-psv` is
      unchanged, because the guest runner also calls it for development launches (PSV-7).
      Negative-matrix A67.
    - **Accepted, open (class d; not fixed and not claimed):** `protected_scopes`,
      `task_acceptance` and `verifier_pins.check_suites` are still store config. A store writer can
      declassify a scope, or point a protected evaluation at another operator-registered suite or
      test. No development verdict is labelled protected by this, because the evaluation record
      then says development. This is ADR-001's accepted limitation (`v022-G01-claim.md`, "Accepted
      limitations"), recorded as a follow-up after amendments 10 and 12. Review rounds should treat
      it as accepted, not as a new finding. The negative matrix lists it under "Accepted, open".
