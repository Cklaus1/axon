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
    - **What the service MEASURES vs what it is TOLD.** It refuses to sign unless every measured
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
      present must cast (non-strictly: undetermined parts stay free) to its recorded type, and a
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
      non-claim (1), unchanged for that position.
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
      **Exposed: 32 flag lines in 27 sites.** Dispositions: 14 ROWED, each killed by its own attack (a
      symlink or an existing file at the shape the flag defeats): the helper's ownership-walk base
      (M1907), input snapshot symlink (M1908) and create_new (M1909), policy snapshot (M1910), operator
      file leaf (M1911), the hand-over's `fstatat` (M1912) and `fchownat` (M1913), the observer's key
      (M1914) and artifact measurement (M1915), the workspace store's no-clobber rename (M1916), the loop
      store's temporary create (M1917), `psv::prepare`'s policy (M1918) and `keygen` (M1919); 6
      carry a checkable exemption: the destination `O_NOFOLLOW` is dominated by the create_new on the line
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
      clocks, not statuses, and are not a form). **Exposed: 33 sites.** ROWED with their own attacks: the three
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
