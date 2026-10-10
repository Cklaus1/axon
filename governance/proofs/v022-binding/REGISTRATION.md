# v0.22 binding batch, candidate 1 — registration record

Frozen pair: Axon `363b783416f450fe733784e827e9fc1900fe944b` (v022/stage3) + MiCode
`1baa2f815765b303505ef6e854dc285eef3a4391`. One independent final review, `wf_d788c05a-be2`:
one reviewer per gate, judging the gate's wording literally
(`final-review-363b7834.json`, brief `review-brief-363b7834.md`).

## Proof run on the frozen pair (`proof-run-363b7834.txt`)

| Step | Result |
|---|---|
| Clippy | clean |
| Axon loop/contracts/fabric/cortex | 481 passed |
| axon-core | 1573 passed |
| PCI runner | 18 rows |
| Stage-5 binding rows | `stage5-rows-363b7834.txt` |
| MiCode | 5306 passed, 19 gate rows |
| Mutations (`--scope=all`) | 97/97 killed |
| Real-binary interop | 250 assertions |
| Trees | clean before and after |

## Outcome

| Gate | Verdict | Status |
|---|---|---|
| G32-r22-sidecar-bindings | REGISTER, zero blockers | **REGISTERED** |
| G33-r22-decision-rule-freeze | REGISTER, zero blockers | **REGISTERED** |
| G11-r22-independent-admission | DO_NOT_REGISTER | PARTIAL → candidate 2 |
| G11-r22-admission-disposition | DO_NOT_REGISTER | PARTIAL → candidate 2 |
| G11-r22-rollback-revalidate | DO_NOT_REGISTER | PARTIAL → candidate 2 |

G11 blockers (all executed by the reviewers):
- a transition issuer is not separated from the proposer, evaluator or subject of the policy it
  activates;
- activation does not recheck withdrawn safety clearances, contexts or verifier pins;
- economics report the always-unknown Fabric execution cost as known;
- rollback does not recheck profile qualification.

## Recorded findings (not hidden, not blocking the registered claims)

- **MAJOR-ADJACENT, found by both the G32 and G33 reviewers:** the attempt that counts for a
  trial can be chosen after outcomes exist. A later passing attempt replaces a recorded failing
  one (best-of-k). This is fixed in candidate 2 with the G11 work.
- **MAJOR-ADJACENT (G32):** EVO propose accepts a never-intaken episode naming the trusted
  verifier. This is already a known gap in the G01 record ("learning inputs are outside G01"):
  a proposal must still pass evaluation and admission.
- **MAJOR-ADJACENT (G32):** in a protected trial the policy link is the producer's claim.
  ADR-001 D12; the protected execution path (microVM) carries it.
- **MINOR:** G33's evidence rows do not name the repeated-task test, and `analysis_method_ref` is
  not bound to the executed method.
- **FUTURE:** attestation /2 binds no scope or nonce (ADR-002 preflight observation), and task
  identity is by name.

This record is immutable. Current state lives in `governance/status/`; readiness is derived by
`scripts/protected_verifier_ready.py`.
