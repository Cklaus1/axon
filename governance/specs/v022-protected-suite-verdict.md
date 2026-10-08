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
  guest). What the guest interpreter ENFORCES by that name (amendment 102, a runtime taint under the
  static analyses of amendments 53-100, which stay): a value that sealed (candidate) code produced
  carries a taint that every operation of the evaluator propagates, and an operator frame REFUSES to use
  a tainted value as a SELECTOR: the NAME given to a name-resolving builtin (`sandbox_run`,
  `scheduler_spawn`, the `goal_*` family), an operator CLOSURE the candidate picked out of a table by a
  key, an index or a branch and then called, the IMPL a method call dispatches to (the receiver's runtime
  type), and the WIDTH of fixed-width arithmetic. That is the whole claim: candidate bytes cannot choose
  WHICH operator code runs or WHICH operator impl or width answers. It is NOT the claim that candidate
  output cannot influence the verdict: it does (the suite compares the candidate's answer with an
  expected one), and an operator that branches on candidate data (`if cand_ok() { a() } else { b() }`)
  has written a rubric the candidate chooses a branch of. That includes two shapes the taint cannot see
  (amendment 106, executed by the round-11 reviewer): OMISSION (a candidate that does not call an operator
  callback, or withholds a `send` on an operator channel, selects what the operator's branch or index does on
  absence) and a table of precomputed operator VERDICTS indexed by a candidate value (a table of closures indexed
  the same way is refused; a table of verdicts is not). A suite that lets candidate data, or candidate silence,
  select between a strict and a lenient operator check has let the candidate choose the rubric; no sound rule
  closes that. Honest suites compare the candidate's output to an expected value and never branch or index the
  CHECK by candidate data or by whether the candidate acted. Not covered: integer HANDLES of kernel
  objects and authority values (effect lists, budgets) a tainted value supplies, a path, URL or
  `ai_complete` prompt a tainted value supplies, native codegen (`axon build`) and the native
  `gfx`/`axon-domain` registries; the taint is an over-approximation (coarse per binding, per dict and
  per channel), and its cost to an honest suite is listed in amendments 102 and 106 (among them: folding or
  mapping a candidate's `[u8]` is refused unless the operator casts `as i64`, running a candidate-nominated
  entry point by name is refused, an unpinned `let v = work(0)` then `v.ok()` in an arm is refused where
  the pinned form passes).

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
  plus amendments 53/60/72/78/83/88/94/96/100/102/106 (the delta, listed from git in `governance/notes/v022-pci-delta.md`).
  origin/main's 13 commits under `crates/axon-core/src` were MERGED WITHOUT PCI REVIEW; only 5 of the 13
  change the interpreter (Rc arrays and cheaper calls, shared strings with lent closure captures,
  `&mut` write-through, first-class fns, the `arr_sort_by` rewrite) and the other 8 change no interpreter
  EVALUATION: six are native codegen, build/cache or CLI-help changes; `378da246` touches `interp.rs` and
  `interp/builtins.rs` only by a `cfg` re-export and a visibility change (native `agent_action`
  attribution); `edfe3e2d` changes checker concat typing and resolver capture analysis, so `axon check`
  accepts different programs (it is not an evaluation change, and it is not a sealing-neutral one either:
  what the checker admits is what the seal then judges). The 5 are neither narrowing nor known neutral, and the sealing claims
  for them are only what the gate rows `am94 ...` show: a `&mut`
  value a candidate leaves in the operator's binding is cast and judged at the edge back (dispatch and
  width arms, including an annotated operator array and an aborted call), and a `&mut` operand is
  undetermined to the dispatch rule; a sealed frame cannot take an operator fn as a first-class value; and a
  candidate's write to its by-value array parameter does not reach the operator's copy (observed, not
  guarded). Lent closure captures, shared strings and `arr_sort_by` have no row of their own beyond
  the routes listed in amendment 94.
  Amendment 96 (round 9) closes four more routes of the one class "a value or a name crossed the seal
  without its edge": the result of `sandbox_run` is cast to its declared `i64` at the crossing; every
  read of an operator global goes through ONE lookup (drift-tested); a candidate's own fn values are
  marked and an operator closure is not replaceable by one; and the dispatch rule's width arm covers
  `-x` / `~x`. Its sweep lists what it examined, what was open and what it did not examine, and claims
  no completeness; every builtin that runs user code is classified by a drift test.
  Amendment 102 (round 11) moves the class "a value or name sealed code chose selects operator code" from a static
  sweep, which five rounds in a row found one instance short, to a runtime taint at the selection primitives (PSV-1
  above); the earlier edges stay underneath it and are not claimed redundant.
  Amendment 106 (round 11) closes the channel's STATE (`len`, a sealed drain, a skipped `select` arm), a kernel or
  world write made under a branch or loop the candidate sized, a callback run a candidate-chosen number of times,
  the text of a value that shows shared state, and the existence oracle on every path (one text for a sealed
  caller, 120 pairs tested); it words the PSV-1 residual to include omission and precomputed verdict tables.
  Each amendment's PRINCIPAL arms are exercised by named gate rows in `scripts/v022_pci_gates.sh`
  (rows named `am53` ... `am106`; the row count is not quoted here, it is derived and drift-tested
  by `scripts/pci_delta.py --check`). Arms verified to fail a gate row when their code is removed:
  the declared-return cast, the dict edges, the `()` coercion of an absent return type, channel
  stamping at creation, strict closure arguments at a crossing, the am83 arithmetic arm, and the arms of
  am100 and am102 (the name sinks, the existence-oracle text, the closure pick, the taint of a binding, a
  shared object, the kernel and the world: each removal is a mutation row KILLED by its own attack, and the test that
  row fails is itself a test a gate row runs: checked for every unit row of am100 (M2600-M2629), am102
  (M2700-M2763) and am106 (M2910-M2939), by reading both registries). The runner rows of am100 (`axon-psv`, M2603-M2607) are CORROBORATION of
  the unit rows, as am96's are: with the taint on, the production route refuses those attacks by the taint
  first, so four of them were withdrawn (amendment 102) and the static guards are evidenced at unit level
  only. The production pair (the static layer plus the taint) is covered only by the `Both` columns of
  amendment 102's tests, the runner leg and the gate's sweep step; a static guard removed from the production
  route ALONE is not observable, because the unit-test switch `TAINT_FORCE_ON` is `cfg(test)` and no shipped
  binary has the taint off. Of the taint's own rules only the closure-table and name rules have runner rows
  (amendment 106); dispatch and width have none, because the static layer refuses every such attack first. The
  closure arm of `replaced_ok` (am78) fails only the am72 dict-snapshot row, not an am78 row, and the
  remaining arms are covered by mutation rows only. That those rows are re-run and killed at the
  frozen head is a FREEZE OBLIGATION, not yet shown. A non-empty guest effect ceiling is OUTSIDE the PCI certification.
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
  - Fabric revision, and Firecracker, launcher and host-config digests;
  - guest image, kernel, verifier, suite-registry and policy digests;
  - the intended launch manifest digest.
- Fabric consumes it and cannot mint it: it holds no observer key, so it cannot forge the
  SIGNATURE. What the signature attests is stated per field (amendment 79, which replaced the
  claim that every field was measured):
  - MEASURED by the observer from the operator's installed files: host config, launcher,
    Firecracker, guest kernel and rootfs, and suite registry digests. The qualification record and
    profile manifest digests are also measured and checked, but they are launch-manifest fields,
    bound to the observation through `intended_launch_manifest_sha256`; the observation itself has
    no such fields;
  - PINNED by the operator (the helper config's `fabric {path, sha256, revision}`): the verifier
    digest, which the root helper holds the caller's executable to on every launch and relay;
  - TOLD by the operator's kit (amendment 85): the Fabric revision. The kit reads it from the
    installed binary and writes it into the helper config; the helper compares only the digest,
    and the observer signs the configured value. The digest is pinned and measured; the revision
    is the kit's word about that file;
  - NAMED by the measured profile manifest: the guest init and axon digests;
  - the nonce: issued for that epoch and unspent (custodian), observed once (observer);
  - the principal's word, NOT measured: the guest policy digest (the root helper binds the policy
    it boots to it) and the authority epoch (the loop's scope pointer, joined at intake).
  The verifier digest is the digest of the executable file the Fabric-uid process had when the
  helper opened `/proc/<ppid>/exe`. The plain exec-after-spawn route (spawn the helper, then exec the
  pinned file) was EXECUTED by the round-6 reviewer (18 of 20 attempts got an observer-signed
  observation naming the pinned verifier) and is REFUSED since amendment 85 (0 of 20 relayed, control
  served): `reply_channel_private` requires that no process other
  than the helper and its parent holds the pipe the reply is written to (and a production helper
  refuses a stdout that is not a pipe). That NARROWS the route; it does not close it. A same-uid
  process can still reopen the pipe's other end through `/proc/<pid>/fd/N` of the parent, receive
  the descriptor in flight (`SCM_RIGHTS`), `pidfd_getfd` it, `ptrace` the genuine program, or
  `LD_PRELOAD` it. So the Fabric pin is a configuration and mistake guard, not a defence against
  malicious same-uid code, and the helper serves a caller whose executable at that instant is the
  pinned file and whose reply pipe has one reader. The verdict's integrity does not rest on it.
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

## What the refusal-site gate claims (C9 round 9, amendment 98)

`scripts/v022_refusal_coverage.py` derives the guard sites from the code of every in-scope file. The claim is
exactly this: **every GATE-VISIBLE refusal site (what is NOT visible is listed under "What the gate still cannot see") has a mutation row, a CHECKABLE exemption (a fact a reviewer can re-execute:
dominated, unreachable, not on the protected route), a recorded OBSERVED measurement (a survey saw a named test fail; re-measured
over a sample at the freeze, amendment 107), or an entry on a counted, greppable REMAINDER list of guards no
test observes alone.** REMAINDER is **not claimed covered**: it is the list of guards for which no test is known to
fail when the guard is removed. The gate prints the counts by category on every run and
`python3 scripts/v022_refusal_coverage.py --remainder` lists each site (`grep REMAINDER`). An earlier wording
("a row or a reasoned exemption", amendments 48 and 95) let a REMAINDER entry read as an exemption; it is not one.

### What the gate still cannot see (C9 round 11, amendment 103)

"Every refusal site" is true of the sites the gate can SEE, and the REMAINDER count is a count of those. Round 10
(EQUIVALENCE) executed two forms the gate did not see: a production VALUE handed to a primitive that no test observes
(`exec_axon_test`'s `.env("AXON_PATH_EXCLUSIVE", "1")` -> `"0"`, the PATH prefixed with `/in/candidate`) and an
OWNER argument (`Some(h.owner)` -> `None` at the two sites that open the root helper). Both are visible now: a literal
or constant handed to a spawn builder (`.env` value, `.arg`, each `.args([..])` element, `.current_dir`, `Stdio::..`),
an owner/uid/gid argument, a permission mode literal, and a literal field of a Config/Cfg/Authority/Policy/Manifest/
Trust struct literal are each a site, credited only by a row whose edit changes THAT value, by an `OBSERVED` entry
naming the test a survey saw fail, by a checkable exemption, or by a counted `val_*` REMAINDER entry. What the gate
STILL cannot see is printed in its last lines (`STILL BLIND:`) and is, concretely:

- a value built by computation, or handed through a local binding (`let m = 0o700; mkdir(m)`: seen at the literal, not
  at the use), a `format!` of variables, a path joined at run time;
- a spawn through a wrapper fn or a builder not named `.env/.arg/.args/.current_dir/.stdin/.stdout/.stderr/.uid/.gid`;
- a struct literal of a type not named Config/Cfg/Authority/Policy/Manifest/Trust, and a literal inside a nested literal;
- a default read as a value (`unwrap_or`, `map_or`, `Default::default`);
- a uid or mode that is the operand of a comparison (only the per-term and constant rules see those);
- whether a REMAINDER or OBSERVED entry is TRUE: nothing re-runs the survey that wrote it;
- a row that deletes a REDUNDANT PAIR (git's `GIT_NO_LAZY_FETCH` + `protocol.allow=never`) credits both members, though
  only the pair is shown to be observed (each alone survives, because the other still holds);
- Python other than `scripts/guest_build_env.py`, the shell scripts, and any decision that is not Rust or that file.

A reader must therefore not take "every refusal site" as "every guard".

### Amendment 107 (C9 round 11, eqgate7): values are now followed to their sinks, and what remains

The list above was extended by instance three rounds running. The gate now follows a literal, a const, a collection of
them and a one-level local to a SINK: the arguments of an exec wrapper (`sealed_exec::command`, an explicit table checked
against every `Command::new` of the scope in both directions), every call of a fn with an `Option<u32>` expected-owner
parameter (a field, a local, `None` all count), a field named `owner`, and a const named at one (its definition is a site).
Counts: value sites of the older forms 228; flow sites 59 (16 rowed, 43 OBSERVED); 46 sink arguments COMPUTED and counted but
not sites; 2 bare const uses not followed. The claim has a fourth disposition, **a recorded OBSERVED measurement**, which the
freeze re-measures over a sample (`scripts/v022_resurvey.py`, record `governance/status/v022-resurvey.json`) instead of every
run re-checking it. What is still blind is the list printed by the gate (`STILL BLIND:`) and in amendment 107.

**Withdrawn runner rows, stated plainly.** M2603, M2604, M2605 and M2607 (amendment 100) survive only because the production
route refuses those attacks by TAINT; their unit twins M2600, M2601, M2602 and M2606 kill only with the taint rules OFF
(`TAINT_FORCE_ON=false`, a `#[cfg(test)]` switch in `interp.rs` and `taint.rs`): they judge a layer in a mode the shipped
binary never runs. This is defence in depth, not a gap in the claim as written, but no production-route test guards the static
layer alone for the `stricter(..)` programs of `taint_tests.rs`.

## Mutation record for the readiness authority

Mutants of `readiness.rs` and `backend.rs` are killed by `tests/readiness.rs`, `tests/trust_root.rs`
and `tests/qualification.rs`, with ONE exception. That exception is classified **equivalent** and is
**not counted as killed**:

- `no-diff-fail` — the git-diff failure path does not fail closed. It is unreachable: the ancestor
  check before it has already established that `axon_sha` is an ancestor of HEAD, and for such a
  pair the diff cannot fail. The code still fails closed there (defence in depth), but no test can
  distinguish the mutant.
