# PCI delta since the certified interpreter (31413ca7)

Status: MUTABLE note (amendment 82). It does not amend `governance/proofs/v022-pci/CERTIFICATION.md`,
which stays as certified and says what it says: axon `31413ca7`, the LOCAL backend, the EMPTY
effect ceiling, 94/94 mutations, with surfaces 18 (non-empty grant) and 19 (microVM) scoped out.
This note says what the guest interpreter is NOW, relative to that, and how much of the difference
is covered. Base of this note: `c9r4c/claims` at `d92f798b` (= `v022/veto`).

## (a) Interpreter changes since 31413ca7

`git diff --stat 31413ca7..d92f798b` over `interp.rs`, `interp/*.rs`, `main.rs`, `checker.rs`,
`resolver.rs` and `builtins.rs` (26 commits touch `crates/axon-core/src`):

| file | change |
|---|---|
| `interp.rs` | +2822 lines (net) |
| `interp/conform.rs` | +1566, new file (the cast at every declared boundary) |
| `interp/eval.rs` | +106 |
| `interp/builtins.rs` | +58 |
| `interp/proptest.rs` | +42 |
| `interp/goal.rs` | +29 |
| `main.rs` | +131 |
| `resolver.rs` | +187 |
| `checker.rs` | +27 |
| total | 4658 insertions, 310 deletions (9 files) |

`lib.rs` and the `BUILTINS` table's file are outside that list. The commits that carry the
changes, by theme (full list: `git log 31413ca7..d92f798b -- crates/axon-core/src`):

| theme | commits (short) | what changed in pass/fail semantics |
|---|---|---|
| Sealed modules, module path | 08903fdf, 38273ef1, 8f8e9755, 32fdea5c | A sealed module's `use` resolves only in the sealed dirs; one file per module name, suite first; an unreadable suite module never falls through to the candidate; the sealed rule holds whoever imports. More refusals. |
| RNG | 8e9858f4, b8bbdb74, bce231ad, 8a7962fb | A sealed frame may not reseed or DRAW from the process RNG; per-kernel RNG isolation. More refusals. |
| Key and completion | a4a05f28, 91c2f676, f2a12aab, e9a13f72 | K is unreachable from candidate code; completion is the test body's end; sealed handlers act only on sealed operations; the raw key read is unix-only (wasm32 compiles). |
| Amendment 53 (e8537726) | | A value is cast at every declared boundary (`conform.rs`, `Interp::cast`: fn/method args at entry and result at return, closure args and result, `let`, fields); an operator method name is the operator's. Type-confusion panics at every return. A candidate can no longer choose the code under the operator's judging method. |
| Amendment 60 (3cf23eef, 8a5419de) | | The cast's notion of "the same type" is the key a method call dispatches on (integer widths, `Value::type_name`); a call through a local goes through its value; every refusal arm of the cast has its own row. |
| Amendment 72 (fa0643ac, 68176b53, 6b793da8) | | At a seal crossing every type position is determined from the operator side or the crossing is refused (generic `Chan<T>`, and so on); the operator's dicts are SNAPSHOTTED when handed to sealed code and verified at every edge back; `()` coercion and strict closure args. More refusals, and a crossing that used to pass leniently now refuses. |
| Amendment 74 (48a077ee) | | Predicate primitives are refusal sites; `keyed_outcome` and exposed guards rowed (mostly harness and rows). |
| Amendment 78 | NOT IN THIS BASE | On `c9r4c/psv1b` (`3970:78. A position the operator held is judged by what it held, deeply and strictly`, rows M1840-M1848, A127, A130 as named there). The wording of the claims spec names it because the claim is about the frozen head; if psv1b does not merge, the wording must drop 78. |

All of these narrow what a candidate can do (more refusals, never fewer), except the one behaviour
change to the test runner: a completion token now means "the test body returned normally", which
`91c2f676`/`f2a12aab` fixed to be the body's own end, and which is NOT evidence that every
assertion ran (an assertion inside a closure the candidate never calls does not run; the
reviewer executed it to a keyed PASS). Suites must assert after the call.

## (b) PCI gate rows rerun at this head, and the mutation rows that cover the delta

`scripts/v022_pci_gates.sh` (18 rows), run on `d92f798b` + this branch's comment-only source edits,
interpreter build (`cargo build --locked -p axon-core --no-default-features --bin axon`), exit 0,
`v022_pci_gates: PASS — 18 rows`. Log `/var/tmp/c9-cl-tmp/pci.log`.

| row | tests | result |
|---|---|---|
| 1 shadowing | axon-fabric/check_effects 1 | PASS 1/1 |
| 2 redefinition | axon-core lib 2 | PASS 2/2 |
| 2b second-trait method | axon-fabric/check_effects 1 | PASS 1/1 |
| 3 symlink escape | axon-fabric/check_effects 1 | PASS 1/1 |
| 4 ambient module path | axon-fabric/check_effects 2 | PASS 2/2 |
| 4 ambient module path | axon-core/pci_isolation 1 | PASS 1/1 |
| 5 path-list injection | axon-fabric/check_effects 1 | PASS 1/1 |
| 6 verifier environment | axon-fabric/attestation 2 | PASS 2/2 |
| 7 loop control | axon-core lib 3 | PASS 3/3 |
| 7 loop control | axon-fabric/check_effects 2 | PASS 2/2 |
| 8/11/12 return, exit, Err | axon-core lib 1 | PASS 1/1 |
| 10 named handlers | axon-core/pci_isolation 1 | PASS 1/1 |
| 13 completion evidence | axon-core/test_completion 2 | PASS 2/2 |
| 13 completion evidence | axon-fabric/check_effects 3 | PASS 3/3 |
| 14 exit code | axon-fabric/check_effects 1 | PASS 1/1 |
| 17 working directory | axon-fabric/check_effects 1 | PASS 1/1 |
| 21 sealed candidate | axon-core lib 2 | PASS 2/2 |
| 21 sealed candidate | axon-fabric/check_effects 2 | PASS 2/2 |

No row failed. These rows are regression evidence for the surfaces the gate script lists, and
for nothing else.

Mutation rows whose target is `crates/axon-core/src` (`scripts/v022_g01_mutations.py`, table
`MUTATIONS`, 134 such rows): M58-M62, M65, M67-M70, M72-M79, M82-M83, M85-M97, M219, M243, M246-M248,
M250-M251, M260, M270-M272, M436, M560-M566, M651-M667, M920-M939, M1140-M1157, M1660-M1681,
M1730-M1731, M1734, M1764-M1765. Per delta:

| delta | rows (named in the amendment's own text) |
|---|---|
| sealed modules, RNG, key, completion | M5xx: M560-M566 (sealed handlers, replay feed, completion); the PCI-scope rows M52-M97 |
| amendment 53 | M651-M667 (+ M566), matrix A86 |
| amendment 60 | M86-M89, M562-M563, M651-M652, M920-M939, M1140-M1157, matrix A86 |
| amendment 72 | M1660-M1681 (named M660, M662, M1145, M1148, M1152, M1153 too), matrix A96-A109 |
| amendment 74 | M1730-M1731, M1734, M1764-M1765 (core-target rows) |
| amendment 78 | M1840-M1848: not present in this base (on `c9r4c/psv1b`) |

THIS TASK DID NOT RE-RUN THE MUTATION ROWS. The claims-spec wording says the rows are "re-run at the
frozen head"; that is the freeze procedure's obligation, not something this note evidences. What
the rows' own recorded runs at their amendments say is in each amendment's text. Do not read the
table above as a fresh kill count.

## (c) Surfaces 18 and 19, and what the protected profile does with a non-empty ceiling

Outside the certification, unchanged: surface 18 (a non-empty effect grant) and surface 19 (the
microVM). PSV-3's guest-interpreter claim covers the EMPTY ceiling only.

What the code does today (read, not executed in this task; no code was changed):

- `submit.rs` derives the ceiling from the ADMITTED grant (`grants::effect_ceiling(grant.grant())`),
  never from an operator flag, and hands the same value to the host executor and to
  `GuestPolicy::for_grant(&req, &ceiling)`. For `for_grant`, `""` is `allowed_effects: []` (deny
  every effect); a non-empty ceiling becomes that list in the guest policy. Nothing in
  `for_grant` refuses a non-empty ceiling (only a policy too long for the cmdline).
- `axon_psv::protected_policy_ceiling` (root helper before the nonce is spent, Fabric where it
  binds the policy, and the guest runner) requires the policy to be the manifest's `policy_sha256`
  and to state `allowed_effects` explicitly (absent is refused: "I did not say" is not "I said
  none"). It returns the ceiling, empty or not. It does not refuse a non-empty one.
- `signing::attestation_decision`: on the microVM backend (`LINUX_MICROVM_PROTECTED`) the receipt
  is signed for evidence class `protected` OR `guest-unobserved` REGARDLESS of the effect ceiling
  (`r.effect_ceiling` is consulted only on the other arm). On any other backend a non-empty
  ceiling is refused (`KEY_REACHABLE`), and an empty one is signed only as `development`.

So, plainly: **under a non-empty ceiling a receipt CAN be signed `protected`**, provided the
microVM path ran and was observed. The effect ceiling is applied inside the guest, the policy
digest is joined to the observation, and the PCI surfaces still run, but the PCI certification
does not cover that combination. This is an OPERATOR-VISIBLE SCOPE LIMIT, recorded here and in
amendment 82, not a defect fixed: the operator chose "reword + delta note, non-empty guest ceiling
OUT of scope" (2026-10-05). An operator who wants protected verdicts only under an empty ceiling
must pin that through the grant registry; the code does not enforce it.
