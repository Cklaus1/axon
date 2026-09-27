# Protected Check Isolation (PCI): a prerequisite of protected verification

Status: **PARTIAL.** Not certified. This document defines the property and its threat surface,
and records the current state of each surface. Certification follows the same discipline as
G01:
1. freeze an Axon and MiCode pair plus this document;
2. run the named tests and the mutation run;
3. run one final independent review, with no edits.

## Why this is its own property (decided 2026-09-26)

Four consecutive G01 final reviews (candidates 1-4) each found a claim-level blocker. None
concerned authentication. Each concerned candidate code changing the meaning or control flow of
the operator's own test:
- a symlink escape (FG-062);
- `break` escaping a candidate function (FG-063, FG-064), and escaping through its predicates
  (FG-065);
- a candidate redefining a suite helper's definitions (FG-066).

That is a different property from "the trusted verifier really produced this evidence". Folding
it into G01 would turn one acceptance gate into an unbounded proof of the whole language and
module system. A verifier verdict is trustworthy for protected use only when three distinct
properties hold:

| # | Property | Question | Gate |
|---|---|---|---|
| 1 | **Authenticity** | Did the trusted verifier really produce and sign this evidence? | G01-r22-independent-issuer |
| 2 | **Isolation / semantic integrity** | Did it run the operator's check without candidate influence? | **PCI (this document)** |
| 3 | **Correct binding** | Is that authentic, isolated verdict about THIS task, trial, output tree and policy? | G01 clauses 4-5 (pins, joins); admission |

## The property

> **Candidate-controlled bytes cannot alter the identity, dependencies, namespace, control
> flow, or pass/fail semantics of an operator-owned verification check, except through the
> explicitly defined candidate-under-test interface.**

The candidate-under-test interface is the set of names the operator's suite imports from the
candidate's modules (`mod f` / `use f.{double}`). Candidate code runs only when the suite calls
through that interface. Whatever it returns is judged by the suite's own assertions. Nothing
else the candidate supplies may change which code the check runs, or what "passed" means.

## Classification rule for reviews

- A finding that falsifies the authenticated issuer or provenance is a **G01 BLOCKER**.
- A finding that lets candidate code alter what the protected check executes, or what PASS
  means, is a **PCI BLOCKER**.
- Anything else is an adjacent finding, recorded in the sweep.

Existing findings are reclassified explicitly below, never hidden or downgraded.

## Threat surface and current state

| # | Surface | State | Evidence / reference |
|---|---|---|---|
| 1 | Module or import shadowing of operator-suite modules | fixed: the suite's directory resolves first | FG-052; `a_candidate_cannot_shadow_a_module_of_the_suite` (M04) |
| 2 | Candidate redefinition of operator-suite fns, types, enums, modules, `let`s, named refinements and trait impls | fixed: E0002 across the merged program | FG-066; resolver `duplicate_let_refinement_or_impl_produces_e0002` (M60-M62); `a_candidate_cannot_redefine_a_suite_helpers_impl_or_constant` |
| 2b | Trait DEFINITIONS, and methods added by a second impl of a different trait for the same type | **fixed** (was live: a signed-off pass). Methods are unique by their DISPATCH key (`type_name_of`, name) across the merged program; trait names are unique (a duplicate trait was not live, since trait methods carry no bodies) | FG-068; resolver `duplicate_let_refinement_or_impl_produces_e0002` (M69, M70); `a_candidate_cannot_redefine_a_suite_helpers_impl_or_constant` (second-trait method) |
| 3 | Symlink or path escape from the candidate or suite tree | fixed: any link is refused before launch (candidate side mutated; suite side untested) | FG-062; `a_candidate_holding_a_symlink_is_refused` (M57) |
| 4 | Ambient module-path fallthrough (trial-cache `~/.axon/lib`, interpreter bindir) | fixed: `AXON_PATH_EXCLUSIVE` | FG-060; M53. Fabric does not probe that the pinned interpreter honours it (MINOR) |
| 5 | Path-list injection into the module path (`:` in the state dir) | fixed | FG-059; M52 |
| 6 | Candidate-controlled verifier environment | fixed: clean environment and applied empty ceiling | FG-057; M44, M55 |
| 7 | Cross-frame `break` / `continue` | fixed: contained at every frame edge (functions, closures, predicates, handler arms) | FG-063/064/065; M59 (M58 retired as equivalent) |
| 8 | Cross-frame `return` | contained, probed: `call_fn` ends a `Return` at the callee, so a candidate function's or closure's `return` never ends the test (the assertion after it runs) | interp `a_test_completes_only_when_its_body_returns_normally` (`t_return`, `t_closure_return`) |
| 9 | Refinement / predicate / attribute execution escaping into verifier control flow | fixed for loop control (7). Other effects of predicates are not yet assessed | FG-065 |
| 10 | Effect-handler or closure escape | loop control contained (7). Named handlers: not live, probed — a named handler is desugared per FILE at parse time, so a candidate's same-named handler cannot rebind the suite's, in either merge order. Interface note: a closure the SUITE hands to the candidate runs under whatever handler the candidate installs; that is the candidate-under-test interface, by the suite's own choice | `tests/pci_isolation.rs` `a_candidate_cannot_rebind_a_suite_named_handler` |
| 11 | **`exit(0)` or equivalent early successful termination** | **fixed by 13** (reclassified from G01 MAJOR-ADJACENT). `axon test` still reports the test ok, but it issues no completion token, so Fabric records Unknown | FG-067; `a_pass_needs_evidence_that_the_test_completed` (M63, M67, M68) |
| 12 | **A test returning `Err` treated as success** | **fixed by 13** (reclassified from G01 MAJOR-ADJACENT). A test whose body yields `Err` (tail value, `return` or `?`) is EndedEarly: no token | FG-067; interp `a_test_completes_only_when_its_body_returns_normally` (M65); `a_pass_needs_evidence_that_the_test_completed` |
| 13 | A pass signed although the required assertion or completion point was not reached | **fixed: affirmative completion evidence.** Fabric generates a fresh 32-byte secret per run and hands it to `axon test --completion-key-stdin`, which reads it BEFORE any program code runs. Only a test whose body returned normally (not `Err`) gets `HMAC-SHA256(key, "axon-test-completion/1\0" + name)` on its ok line. A Passed verdict requires a verifying token for the named test (or every passed test), else Unknown. A forged ok line printed by the candidate does not verify. This replaces enumerating escapes: any route that ends the body early yields no token. Scope: plain `@[test]`s; property tests issue no token, so a verdict resting on one is Unknown | FG-067; M63-M65, M67, M68; `tests/test_completion.rs` (independent HMAC vector) |
| 14 | Candidate `@[test]` functions joining the run through the substring filter | partial: only the exact named test decides the verdict. A candidate test still runs in the verifier's process and changes the exit code | FUTURE finding (candidate-4 review) |
| 15 | A candidate suppressing the failing test's own result line (Failed → NotRun) | open as MINOR: it fails closed | candidate-4 review |
| 16 | The admission scan does not see `use a::b` | mitigated: resolution is confined (4) and the ceiling is empty | G01 claim, known gaps |
| 17 | The check's working directory (relative runtime reads of the suite's own fixtures) | **fixed** (was live under an IO grant: a pass for a wrong candidate, and the honest one failed). The check inherited the LAUNCHER's cwd, so a relative read resolved outside the pinned, digested tree and a candidate planted the answer there. The check now runs in `<run>/check`, which is materialized read-only | FG-069; `a_suites_runtime_fixture_is_the_pinned_one` (M71) |
| 18 | An effectful grant reaching the VERIFIER's own state as the same uid (the Fabric state dir, workspace store, journal, registry file, other trials) | **open, by construction of the local backend.** A local check runs as the launcher's uid. Under a grant with `fs_write` beyond the empty ceiling it can write whatever that uid can. Under `Exec` it can do anything the uid can, including `chmod`-ing the read-only suite tree. PCI cannot hold for a local run under such a grant. **Scoped out of the local claim (see "Scope of the claim")**, consistent with ADR-001 D3, under which the local backend is dev-only. Every locally SIGNABLE check runs under an empty ceiling (G01 clause 2) | ADR-001 D3; G01 clause 2 |
| 19 | The microVM (protected) backend carries the same guarantees: cwd in the check tree, affirmative completion evidence, confined module path | **open, not claimed.** The completion key is wired only into the LOCAL executor, and no real protected producer emits evidence yet (G01 known gaps). A prerequisite of any protected-verifier claim | — |

## Scope of the claim

PCI is certified for two things:
1. The **language-level** rules, which do not depend on the backend: resolution, uniqueness, frame containment and completion semantics (surfaces 1-2b, 7-13).
2. The **local interpreter backend running a `registered_check` under the empty ceiling**, which is the only locally signable profile (surfaces 3-6, 14-17).

A local run under a non-empty grant is a developer verdict (ADR-001 D1/D3), and PCI makes no claim for it (surface 18). The protected microVM backend is a separate prerequisite (surface 19).

## Certification rule

PCI is certified (its state becomes CERTIFIED, within "Scope of the claim") when a frozen
candidate meets every condition below. The candidate is the exact Axon and MiCode SHAs, clean
trees, and this document's hash.
- `scripts/v022_pci_gates.sh` passes;
- every `--scope=pci` mutation is killed at the frozen Axon SHA, with every baseline passing;
- the Axon and MiCode suites plus clippy are green, and the real-binary interop and MiCode G01
  gates still pass;
- ONE final independent review of the frozen pair finds zero PCI BLOCKERs. A PCI BLOCKER is
  candidate-controlled bytes altering the identity, dependencies, namespace, control flow or
  pass/fail semantics of the operator's check, within the scope above, other than through the
  candidate-under-test interface.

Nothing is edited during that review. Findings outside the scope, or about other properties, are
recorded as adjacent.

## Readiness state model

| Item | State |
|---|---|
| G01-r22-independent-issuer (authenticity / provenance) | **REGISTERED** 2026-09-26 at 9ae4c605 + micode dd4ea0a9 (`governance/proofs/v022-g01/REGISTRATION.md`), with this document as its prerequisite |
| Protected Check Isolation | **PARTIAL**: not yet certified. 14/15 are partial or MINOR, 18 is scoped out and 19 is open. The fixes for 2b, 11-13 and 17 are on v022/veto |
| **Overall protected-verifier readiness** | **NOT READY** |

G01 may register on zero authenticity or provenance blockers while PCI stays open. That holds
only on these conditions:
1. The registry row and the G01 claim record PCI as a prerequisite of any protected-verifier or
   promotion claim.
2. Overall protected-verifier readiness stays NOT READY until PCI passes.
3. No release or admission logic treats G01 alone as sufficient protected verification. Today
   no admission path reads a gate registration, and a protected scope also requires protected
   (microVM) evidence, which no real producer emits yet (G01 known gaps).

## Next steps

1. Done: 11-13 (affirmative completion evidence), and 2b, 8, 10 probed (2b was live, now fixed).
   17 was found and fixed.
2. 18 is bounded out of the local claim (see "Scope of the claim"). 19 stays a prerequisite of any
   protected-verifier claim.
3. Done: the regression suite is `scripts/v022_pci_gates.sh`, one row per surface with exact test
   names, invoked by `scripts/gate.sh`. The mutation proof is
   `v022_g01_mutations.py --scope=pci`.
4. Freeze, run the proof sequence, then run one final independent review against this
   document.
