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
    - **Language follow-up (not fixed here).** `dict_get`, `dict_get_or` and `host_await_val`
      return a free type variable, so a stored value of any type unifies with any use. A closure
      has no declared return type at run time, so the boundary check does not cover its return
      (it still catches the value at the next declared `fn`, including a test declared
      `-> Result`). An `Option`-returning test whose `?` meets a well-typed `None` is the route
      where the completion rule is the only guard; a test pins it
      (`a_test_ended_by_question_mark_is_never_completed`, `t_find`). The fix belongs in the type
      system: typed dicts, or a runtime tag check against the inferred type.
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
