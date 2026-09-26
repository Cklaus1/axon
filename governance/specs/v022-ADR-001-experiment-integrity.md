# v0.22 ADR-001 — Experiment-integrity architecture: authority, isolation, admission, producer fidelity

Status: **PROPOSED — operator decisions required (§6).** Nothing here unblocks CX-21 /
CODING-STRATEGY-001, which stays **PREREGISTERED / NOT_RUN** until every prerequisite in §7
is proven by executable, discriminating evidence.

Date: 2026-09-26. Inputs (preserved, read-only runs):
- the 80-gate audit `wf_f213aa49`: `.axon-v022/sweep/audit_wf_f213aa49.json` (raw findings; counts
  are NOT deduplicated bugs until reproduced);
- the design panel `wf_6c790b05`: `.axon-v022/sweep/arch_wf_6c790b05.json` (three independent designs
  per theme plus an adversarial critic).

## 1. Why this ADR exists

The audit found the same shape of defect again and again: **authority is established by something
the caller supplies.** It shows up as caller-supplied registry, store and journal paths; claimed
principal, grant, admitter and issuer strings; and documents presented inline instead of read
from an operator-owned record. FG-050 fixed one instance (verifier identity) with signatures.
Signatures alone do not help while the signing key, the registries and the store are readable and
writable by the same uid that runs the subject.

## 2. Measured host facts (these bound every design)

| fact | consequence |
|---|---|
| This agent and MiCode run as **uid 0** | no uid boundary exists today |
| WSL interop **enabled** (`WSLInterop`), `/mnt/c` mounted rw | any Linux uid can start `wsl.exe -u root`: a host escape for any code run on the host, candidate `build.rs` included |
| `cklaus` is in **`docker`** (root:docker 0660 socket) and **`sudo`** | the developer uid is root-equivalent |
| `kernel.yama.ptrace_scope=1` | blocks cross-process ptrace/pidfd_getfd of non-descendants (helps fd-based capabilities) |
| microVM launcher needs root (`fc_linux_profile.sh`), jailer drops to `axonb263` (uid 999) | a protected path needs one pinned, privileged launch step |

**None of the separate-uid remedies can be qualified until §6 D1 is done.**

## 3. Theme 1 — Authority root

Threat model (all three options were assessed against each attacker): a malicious workspace or
repository; a malicious or compromised MiCode process (same uid today); a forged principal or
grant; an alternate registry/store/journal path; a sibling trial.

| option | custody (who holds keys, registries, journal, store) | caller paths / claims | sibling trial | verdict |
|---|---|---|---|---|
| A. Operator-owned Fabric service under a separate uid (socket-activated, `SO_PEERCRED`) | service uid | eliminated: the service uses fixed operator config | not separated if both arms share a uid | ranked 2: right custody, but peer-uid-to-principal is only as good as the uid split |
| B. Operator-root-signed capabilities verified in-process, no daemon | still the caller's uid (store and ledger key are writable by the caller) | capabilities replace claims | a bearer file on a shared uid is readable by the sibling | ranked 3: right FORMAT, wrong LOCATION |
| C. Hybrid: signed operator root manifest + custodian uid + connection-bound capabilities | custodian uid | eliminated | a per-trial capability handed over the connection, never as a file | **ranked 1** (conditional) |

**Decision (proposed): the Option-C hybrid.**
1. **Custody.** One socket-activated custodian uid alone owns the registries, the Fabric journal and
   state, the loop store, the ledger key and the launcher configuration. The **verifier signing
   key moves to its own uid** (`axon-verifier`) beside the microVM launcher, and signs only
   receipts produced from a microVM result.
2. **Authority format.** Attenuable capabilities (actions ⊆ parent, narrower scope, budget ≤ parent),
   delivered as **connection-bound handles**, never bearer files.
3. **Attestation v2.** A new signing domain, `acf-receipt-attestation/2`, that also binds store_id,
   journal_id, authority_epoch, suite@version, backend profile, qualification-evidence digest,
   experiment/arm/trial, and a single-use nonce consumed at intake. v1 stays verifiable only for
   historical records, so the Stage-5 exact-pair receipt remains historical evidence: not
   re-certified, not invalidated.
4. **Promotion.** Admit, transition, revoke, baseline and plan approval carry detached operator
   Ed25519 signatures from keys the custodian never holds. Disjointness (admitter ≠ verifier ≠
   proposer) is compared by key fingerprint, not by name. `plan.operator_approved: bool` becomes a
   signed `approval_ref`.
5. **Reads never mutate.** Status, cancel and inspect use a non-reconciling read. Reconcile becomes an
   authenticated verb; today `Journal::open` reconciles before `authorize()` runs.
6. **Evaluation reads records, not requests.** `evl::evaluate` evaluates only episodes that have an
   `EpisodeIntake` ledger entry. Assignments and attempt ids are issued, and stored in the
   operator-owned store, BEFORE execution.
7. **MiCode is two principals.** The harness/producer holds the trial capability and writes the
   export. The model-controlled tool domain (bash, cargo, the candidate's tests) can reach neither
   the capability nor the export.

## 4. Theme 4 — Isolation classification

Options assessed: (1) the local interpreter is development-only; (2) a security-enforcing local
jail (per-trial uid, namespaces, Landlock); (3) the local backend is detect-only.

**Decision (proposed): (1).**
- `process_scoped/local-interpreter` is **DEVELOPMENT-ONLY**. It is ineligible for protected
  evaluation, admission, activation and qualification.
- `post_run` diffing is a **detect-only tripwire**. A detection is an incident and an admission
  veto; its absence proves nothing.
- `linux-microvm-protected` is the **only protected-eligible profile**, and the CX-21 protected
  verifier must run on it.
- Eligibility is enforced by the **consumer** (axon-loop) from a compiled `PROTECTED_PROFILES`
  constant plus the frozen plan's evaluation class and qualification-evidence ref, never from
  request fields, Fabric flags or `config.json`.
- The caller `--linux-*` flags can no longer produce a receipt labelled linux-microvm-protected.
  That profile is configured only from operator-owned config.
- The single-checkout `GitWorkspace` / `run_two_policy_comparison` harness refuses r22 and paired
  trials. Arms run serially, from Fabric-materialized base digests, in fresh per-arm trees.
- (2) is held back as the stage-2 answer to **arm** confinement. It is built only if CX-21 must
  include task families (e.g. Rust/cargo) that the qualified guest cannot execute.
- Protected RegisteredCheck: the suite is delivered on an operator-materialized read-only drive,
  and the verdict is read on a supervisor-owned channel, bound to the operator-registered EXPECTED
  test-id set.

Consequence: CX-21's first protected run is restricted to task families the qualified guest can
execute, unless a toolchain guest is qualified. That is a preregistration amendment (§6 D4).

## 5. Admission and producer fidelity

**Admission** (merge of the revalidate and safety-veto designs):
- a **veto stage before utility** in `decide()`: any attempt's safety violation vetoes, and
  aggregate utility cannot offset it;
- per-ATTEMPT `TrialSafety` with typed reason codes and asymmetric trust: a clearance needs an
  authenticated non-subject monitor; any party may incriminate its own arm, but a subject may
  never pause the ACTIVE policy;
- Unknown blocks ACCEPT, and the hypothesis history gets a distinct `Vetoed` verdict;
- activation re-derives the admission against the as-of snapshot and then **revalidates current
  state** (epochs, verifier keys and pins, revocations, admitter trust) inside the one
  transition append; `ActivationBasis` is recorded;
- rollback re-derives and revalidates the predecessor (admission, baseline issuer, candidate view,
  profile qualification, strength floor). If that fails, it pauses atomically rather than falling
  back unsafely;
- `rollback_policy_ref` is consumed, or refused unless it is the fixed digest of the hard-coded
  semantics;
- laptop mode keeps working with no operator key; its records are labelled ineligible for
  protected promotion.

**Producer fidelity:**
- typed `Fact`/`CostFact`/`LiabilityFact` with a closed `UnknownKind` (timed_out, cancelled,
  unmatched, missing_evidence, unverifiable, not_run, unbound), and per-kind EVL counters with the
  invariant `assigned = pass + fail + Σunknown + missing`;
- a `MeteredAdapter` at MiCode's provider boundary: an intent line is fsynced before each call,
  every retry, dead reissue and failure is recorded, and there are no clamps and no `zero_usage`;
- protected economics are `final` only from operator-signed metering receipts. A producer-asserted
  `final` is downgraded to `estimated`;
- liability ceilings come from the frozen plan or a Fabric reservation, never from producer fields;
- first, land only the local fixes that move facts TOWARD unknown (remove clamps, replace the
  literal liability 0, keep a null `matched_checks` distinct from 0). None of them can make Axon
  more willing to ACCEPT. Today's economics are fail-safe by accident (both arms must be Known,
  and MiCode emits `estimated`).

## 6. Operator decisions required

| id | decision | recommended |
|---|---|---|
| D1 | Host hardening for protected mode: disable WSL interop (`/etc/wsl.conf [interop] enabled=false`, restart WSL); run agents and MiCode as a non-root uid outside `docker`/`sudo`/`adm`; create custodian (`axon-fabric`), verifier (`axon-verifier`) and per-arm trial uids | yes: without it, no protected claim holds on this host |
| D2 | Adopt the Option-C authority hybrid (§3) | yes |
| D3 | The local backend is development-only; only the microVM is protected-eligible (§4) | yes |
| D4 | CX-21 preregistration amendment: restrict the first protected run to guest-executable task families; economics `report_only` until metered receipts exist | yes |
| D5 | The operator generates and holds the root, admitter and qualification keys (agents never create or hold them), and signs the B263 evidence (S3-6) | required |
| D6 | Deployment shape for the custodian: a systemd socket-activated unit vs a manual launch | socket-activated |

## 7. Prerequisites before any protected experiment (user-set, 2026-09-26)

1. Verifier authenticity and protected evaluation: G01 attestation + pins (landed, 355ebd4f; under
   independent re-audit), then attestation v2, the microVM-only protected signer, and the verdict
   channel bound to the expected test-id set.
2. Enforced trial and workspace isolation: §4 on the microVM, per-arm trees, the retired
   single-checkout harness.
3. A real operator-controlled authority root: §3 plus D1/D2/D5.
4. Safety-veto semantics in admission: §5.
5. Complete per-attempt economic and resource accounting: §5 producer fidelity.

## 8. Order of work

1. **Now (local, executable, discriminable; no operator input):**
   - the G01 residuals (the signer requires a `check:<id>` suite; the local-effectless branch must
     never be a protected issuer);
   - the axon-loop protected-class constant and the development-only refusal;
   - `evaluate()` reads intake records only;
   - the veto stage in `decide()`;
   - the activation and rollback revalidation skeleton;
   - the toward-unknown producer fixes;
   - typed `UnknownKind`.
2. **After D1/D2/D5:** the custodian service, capabilities, attestation v2, signed promotion,
   operator-owned Linux profile configuration.
3. **After S3-6 and D4:** the qualified protected verifier path, then CX-21 readiness review.
