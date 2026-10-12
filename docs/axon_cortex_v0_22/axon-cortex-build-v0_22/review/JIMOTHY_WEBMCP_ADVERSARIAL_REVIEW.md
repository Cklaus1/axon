# Schema / lightweight Reflex adversarial review — folded-in findings

**Scope:** single-session source/contract inspection and synthetic falsification, not an independent security audit or live model evaluation. Findings below were used to change the documents/reference/tests before release. Product claims remain unexecuted.

## Failure cases made explicit

1. **Absent output impersonates a confident negative.** Delete the active route/Boolean/presence/value answer. The reference raises an explicit refusal; it never derives false, omission or confidence one from a default.
2. **Schema feature smuggling.** Add a remote reference, union, nested object/array, duplicate/unsafe property, unknown constraint, unbounded number or excessive manifest/domain. The supported-subset compiler refuses. Arrays are not truncated to their first item.
3. **Exact-copy provenance is only implicit.** During review, the span input boundary was tightened to require source digest, source version and explicit code-point units. UTF-16 or stale/unstated units cannot silently flow through; source offsets and exact value remain checked. Semantic selection quality still needs model evaluation.
4. **Changed task has the same number of outputs.** Mutate task, labels, projection, encoder, tokenizer, weights, precision, runtime, device, batch profile or calibration identity. Fixed-head compatibility refuses until requalified; no same-count shortcut.
5. **Correlated examples manufacture support.** Hundreds of rows with one group count as one independent representative. Representatives are selected by stable ID before examining correctness/confidence; 30 all-correct examples do not automatically establish the 95% target under 13-way correction.
6. **Protected labels escape via a teacher cache.** Change partition, principal, project, teacher version or other job scope and verify a different key. Reject duplicate/mismatched committed records and ignore only a torn uncommitted tail. Actual networking, filesystem locks and provider retry guarantees remain live requirements, not reference-test claims.
7. **Hints or confidence create authority.** MiCode's reference separately checks local effect policy, exact input/target/snapshot/principal/project/grant, confirmation, expiry and revocation. It still returns a synthetic unexecuted proposal, not a real grant or completed task.
8. **Old or ambiguous work executes again.** Canceled/unreconciled effects refuse before local dispatch preparation. Runtime generation and current registry/schema/grant changes invalidate the prior proposal; real cancellation/reconciliation needs the live owner.
9. **Qualification and OOD are imaginary.** The reference tests matching identity and handling of externally supplied qualified/unknown/OOD states. It does not implement or validate an OOD detector, real calibration population, deployment signer or admission authority. Gates explicitly retain these as independent product evidence.
10. **Documentation success is laundered into product PASS.** Package mutation tests reject changed requirement statuses, missing mappings, stale owner exports, optional research forced onto new critical paths and fake draft activation. Existing package integrity tests continue checking altered/deleted/untracked files and generated-view drift.

## Limits deliberately not hidden

Full schema standards, live browser APIs, protected dataset custody, operational teacher locking/retry, numerical encoder parity, train-time optimizer behavior, memory-isolation and cancellation races, signed revocation, actual provider outputs, and end-to-end verified utility were not exercised. No claimed performance improvement, Jev reproduction or readiness for unrestricted automatic actions follows from this update.
