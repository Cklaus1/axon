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
| 2b | Trait DEFINITIONS, and methods added by a second impl of a different trait for the same type | **open**, not yet probed | — |
| 3 | Symlink or path escape from the candidate or suite tree | fixed: any link is refused before launch (candidate side mutated; suite side untested) | FG-062; `a_candidate_holding_a_symlink_is_refused` (M57) |
| 4 | Ambient module-path fallthrough (trial-cache `~/.axon/lib`, interpreter bindir) | fixed: `AXON_PATH_EXCLUSIVE` | FG-060; M53. Fabric does not probe that the pinned interpreter honours it (MINOR) |
| 5 | Path-list injection into the module path (`:` in the state dir) | fixed | FG-059; M52 |
| 6 | Candidate-controlled verifier environment | fixed: clean environment and applied empty ceiling | FG-057; M44, M55 |
| 7 | Cross-frame `break` / `continue` | fixed: contained at every frame edge (functions, closures, predicates, handler arms) | FG-063/064/065; M58, M59 |
| 8 | Cross-frame `return` | believed contained: `call_fn` ends a `Return` at the callee | not yet independently probed |
| 9 | Refinement / predicate / attribute execution escaping into verifier control flow | fixed for loop control (7). Other effects of predicates are not yet assessed | FG-065 |
| 10 | Effect-handler or closure escape | loop control contained (7). A candidate-defined NAMED handler used by the suite is not probed | — |
| 11 | **`exit(0)` or equivalent early successful termination** | **open** (reclassified from G01 MAJOR-ADJACENT). A candidate's `exit(0)` counts as a clean test pass. Unsignable today, because every locally signable check has an empty ceiling and `exit` needs IO. Live once the microVM signs suite verdicts under an IO grant | candidate-2 review (wf_2f0bc4c3) |
| 12 | **A test returning `Err` treated as success** | **open** (reclassified from G01 MAJOR-ADJACENT). An operator test that returns `Err`, for example via `?` over the candidate's output, counts as passed | candidate-2 review (wf_2f0bc4c3) |
| 13 | A pass signed although the required assertion or completion point was not reached | open as a general property. 7, 11 and 12 are instances. There is no positive "completed" marker yet; completion is inferred from the absence of a failure | — |
| 14 | Candidate `@[test]` functions joining the run through the substring filter | partial: only the exact named test decides the verdict. A candidate test still runs in the verifier's process and changes the exit code | FUTURE finding (candidate-4 review) |
| 15 | A candidate suppressing the failing test's own result line (Failed → NotRun) | open as MINOR: it fails closed | candidate-4 review |
| 16 | The admission scan does not see `use a::b` | mitigated: resolution is confined (4) and the ceiling is empty | G01 claim, known gaps |

## Readiness state model

| Item | State |
|---|---|
| G01-r22-independent-issuer (authenticity / provenance) | pending: frozen candidate 5 under final review |
| Protected Check Isolation | **PARTIAL**: surfaces 2b, 8, 10, 11, 12 and 13 are open or unprobed |
| **Overall protected-verifier readiness** | **NOT READY** |

G01 may register on zero authenticity or provenance blockers while PCI stays open. That holds
only on these conditions:
1. The registry row and the G01 claim record PCI as a prerequisite of any protected-verifier or
   promotion claim.
2. Overall protected-verifier readiness stays NOT READY until PCI passes.
3. No release or admission logic treats G01 alone as sufficient protected verification. Today
   no admission path reads a gate registration, and a protected scope also requires protected
   (microVM) evidence, which no real producer emits yet (G01 known gaps).

## Next steps (after candidate 5's G01 decision)

1. Decide the semantics of 11 and 12. A check passes only if the operator's test body returns
   normally, which is the positive completion point in 13. An `exit(…)` from below the test,
   or an `Err` return, is a failure.
2. Probe 2b, 8 and 10. Fix whatever is live.
3. Build a PCI regression suite: one test per surface, run through the real submit path, plus a
   mutation driver like G01's.
4. Freeze, run the proof sequence, then run one final independent review against this
   document.
