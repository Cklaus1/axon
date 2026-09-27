# Protected Check Isolation: certification record

**CERTIFIED 2026-09-27**, within the spec's "Scope of the claim".

## What was certified

| Item | Value |
|---|---|
| Spec | `governance/specs/v022-protected-check-isolation.md` **as of 31413ca7**, sha256 `bd5c6d326d04d12a7b3ab82b838d3e7aaa3e1bf90a9575d06f7d70f6992067ca` |
| Axon | `31413ca7abb6ff730e1b63718d4304c7a8402675` (candidate 5) |
| MiCode | `b72cf4c4380335e5f5590181012e707cea259fab` |
| Freeze manifest | `freeze-31413ca7.json` |

The property:

> Candidate-controlled bytes cannot alter the identity, dependencies, namespace, control flow, or
> pass/fail semantics of an operator-owned verification check, except through the explicitly
> defined candidate-under-test interface.

The claim covers the language-level rules. It also covers the LOCAL interpreter backend running a
`registered_check` under the EMPTY effect ceiling, the only locally signable profile. Surface 18
(a local run under a non-empty grant) is scoped out, per ADR-001 D1/D3. Surface 19 (the microVM
backend) is **open**. The spec file keeps its frozen bytes, including its "PARTIAL" status line.
This record is what certifies it.

## Certification rule, each condition

| Condition | Evidence |
|---|---|
| `scripts/v022_pci_gates.sh` passes | `pci-runner-31413ca7.txt`: 18 rows |
| Every `--scope=pci` mutation is killed at the frozen SHA, and every baseline passes | `mutations-31413ca7.json`: `--scope=all`, 94/94 killed, all baselines pass. That includes all 45 PCI rows and every G01 row |
| Axon and MiCode suites plus clippy are green, and the interop and MiCode G01 gates pass | `proof-run-31413ca7.txt`: Axon 473 + 1573, MiCode 5306, 0 failures; clippy clean. `interop-31413ca7.txt` 250/250; `micode-gates-31413ca7.txt` 19 rows; trees clean after |
| ONE final independent review, with zero PCI BLOCKERs | `final-review-31413ca7.json` (wf_d51ff5e0): all five roles return **CERTIFY** |

## How it got here

Candidates 1-4 were each frozen, run and reviewed once, and none was certified. Every executed
blocker is recorded as a false green, FG-067..082, in the spec's History:
- frame escapes;
- a single global namespace;
- routes past a static sealing walk;
- shared handle-addressed kernel state;
- struct-`where` provenance.

A self-audit also found FG-082.

The certified design rests on three structural mechanisms, not on lists of escapes:
- **Affirmative completion evidence.** A test passes only with a per-run HMAC token, issued when
  its body returned normally.
- **Frame edges as allowlists (`contain_frame`).**
- **Runtime provenance.** Sealing is enforced at the call, global-read and refinement edges.
  Definition-owned predicates run under their definition, and there is one kernel per provenance
  for every handle-addressed and builtin-read piece of state.

## Readiness

| Item | State |
|---|---|
| G01-r22-independent-issuer | REGISTERED (9ae4c605) |
| Protected Check Isolation | **CERTIFIED** (31413ca7), within its stated scope |
| **Overall protected-verifier readiness** | **NOT READY**. The protected (microVM) backend (PCI surface 19; G01-r22-registered-check, G01-r22-verifier-separation) emits no suite verdict yet and carries none of these guarantees. |

## Final-review findings: disposition (none blocking)

Each finding is recorded, not hidden, and tracked in the sweep.

**MINOR**
- The prob-predicate pseudo-builtins `P`/`E`/`Var` are not reserved names. A non-distribution
  `P(…)` falls through to a user fn named `P`, and the W0003 text is wrong for it. This is
  reachable only when the operator's own suite depends on a name only the candidate defines.
- `eval_prob_pred` evaluates its left operand twice and drops the first flow.
- Process-global statics outside `Kernel` are shared across provenance: the principal token
  stream, `RNG_STATE` (Random is refused) and the regex cache. The kernels are per provenance, so
  a token from one never resolves in the other.
- The clock-reading `temporal_now`/`temporal_new`/`temporal_is_valid` builtins are classified
  pure. They should carry `Time`.
- Mutation-proof gaps:
  - M89 kills on a message;
  - the handler-arm `contain_loop_control` is unmutated;
  - M52/M60/M71 kill on side effects;
  - some fixed sub-claims are tested but not mutated.
- The interop gate exercises only the positive completion path.
- MiCode's `.env` closure is gated by unit-test rows. The real binary holds.

**FUTURE**
- Candidate `@[test]`s that match the substring filter still run in the verifier process (surface
  14; the named test's verdict is unaffected).
- Cortex's local, non-Fabric adjudicator requires no completion evidence.
- One principal token stream per Kernel.
