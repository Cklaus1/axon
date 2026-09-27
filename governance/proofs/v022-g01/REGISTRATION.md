# G01-r22-independent-issuer: registration record

**REGISTERED 2026-09-26.** The gate certifies authenticity, provenance and binding only.

## What was certified

| Item | Value |
|---|---|
| Claim | `governance/specs/v022-G01-claim.md` **as of 9ae4c605**, sha256 `12ce46ecd701fd06f54f5190d2f1a6d9e7fa296e8d9abc086971967f1098aadf` |
| Axon | `9ae4c605db3a44ba650909df69896383accc1afb` (scoped candidate 6) |
| MiCode | `dd4ea0a97a31486678b6c9f715c94c8ea38500b2` |
| Freeze manifest | `freeze-9ae4c605.json` |

The claim file keeps its frozen bytes, including its "NOT REGISTERED" header line. The header
describes the document's status when it was frozen. This record is what registers it. Code
committed after 9ae4c605 is not covered by this registration. That includes the Protected Check
Isolation fixes.

## Registration rule, each condition

| Condition | Evidence |
|---|---|
| Every clause has named evidence | The claim's evidence table. The final review's clause auditor traced clauses 1-7 and found every one HOLDS (`final-review-9ae4c605.json`, role `clause`) |
| Every `--scope g01` mutation is killed at the frozen SHA | `mutations-9ae4c605.json`: 50/50 killed, all baselines pass, commit 9ae4c605 |
| The real-binary interop and named MiCode G01 gates pass against the frozen pair | `interop-9ae4c605.txt` (250/250); `micode-gates-9ae4c605.txt` (19 rows) |
| Fresh Axon and MiCode suites plus clippy on the frozen pair | `proof-run-9ae4c605.txt`: Axon 467 + 1565, MiCode 5306, 0 failures; both trees clean after |
| ONE final independent review with zero claim-level blockers | `final-review-9ae4c605.json` (wf_6e49a7c7): all five roles REGISTER, zero BLOCKERs |

## Prerequisite: Protected Check Isolation, and readiness

G01 alone is **never** sufficient for protected verification. Any protected-verifier or
promotion claim also requires **Protected Check Isolation**
(`governance/specs/v022-protected-check-isolation.md`). PCI is **PARTIAL** and not certified.
**Overall protected-verifier readiness is NOT READY.** Today no admission or release path reads
this registration. A protected scope still needs protected (microVM) evidence, and no real
producer emits that yet.

## Final-review findings and their disposition

None was a blocker. The claim is judged literally, and each item below is recorded, not hidden.

**MAJOR-ADJACENT**
- **The verdict carries no freshness binding** (verdict adversary, executed). A verdict minted
  before the plan froze counted after the freeze, because the freeze ordering rests on the
  unsigned episode context. The frozen claim quotes the gate statement "stale … evidence is
  rejected". The only staleness its clauses define is "a verifier the operator has since
  untrusted or re-keyed", and no clause binds time, epoch or plan freeze. So no clause is
  falsified.
  **This is a disclosed limitation of what "stale" means in this registration.** Tracked for
  attestation v2: bind the frozen plan ref, the epoch or a Fabric run nonce, and have EVL refuse
  a pre-freeze verdict.
- **PCI / MiCode: a committed `.env` can set `MICODE_EXTENSIONS_TRUST_PROJECT=1`** (cross-system,
  executed). That re-enables project hooks, giving the judged repository same-uid code execution
  on the MiCode host. Clause 4 (`axon.*` closed to repository sources) holds. The attestation
  still refuses a repo-chosen verifier. Tracked as PCI-adjacent: close `extensions.*` to the
  `.env` fold.

**MINOR / FUTURE** (tracked in the sweep; none falsifies a clause):
- One signed verdict can be intaken under two arm ids in one scope. Only EVL's trial-id
  uniqueness stops a double count, and that uniqueness is not pinned by a mutation.
- The signed `policy_digest` is joined to nothing.
- The replayed-transition pointer is still returned after revocation.
- The cross-scope rule keys on receipt bytes.
- Admission compares a 64-bit key fingerprint, and re-derivation does not re-verify the
  signature.
- The microVM signing branch trusts the caller-named launcher and trust dir. It is not
  load-bearing: no microVM verdict counts.
- The interpreter is pinned by path.
- Mutation-coverage gaps: EVL's door gate, M03's target, re-key/TOFU/rollback guards, the
  isolated "issuer trusted" conjunct, equivalent guards, and the unmutated MiCode-side guards.

## Follow-up (after registration; not part of what was certified)

- The PCI-adjacent MiCode `.env` findings recorded above have since been FIXED in MiCode
  fc18bbc6 (`MICODE_EXTENSIONS_TRUST_PROJECT` and the other authority keys) and b72cf4c4 (the
  provider endpoint and routing keys: the `.env` closure is derived from the project-layer
  denylist, credentials stay open). Tracked on PCI surface 20.
