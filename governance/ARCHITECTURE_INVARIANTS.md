# Architecture Invariants — The Rules No Change May Break

These are the load-bearing properties of Axon. A change that violates one is **wrong by definition**,
even if its tests pass — because these invariants are what the tests, the spec, and the safety story all
rest on. The autonomous builder treats a proposed change that breaks an invariant as a design error to
escalate (write it up, propose the invariant change explicitly), never as a thing to quietly route around.

Each invariant has an **ID** so commits and reviews can cite it (`preserves I-7`, `violates I-3`).

---

## Pipeline & semantics

- **I-1 — Pipeline order is fixed.** `Lexer → Parser → Resolver → Infer(HM) → Checker → Borrow →
  Capabilities → Verify → (Codegen | Interp)`. A phase may not depend on a later phase's results. New
  analysis slots in at a named point, never reorders the existing ones.
- **I-2 — The interpreter is the reference semantics.** Where interpreter and codegen disagree, the
  *interpreter* defines correct behavior and codegen is the bug (until a spec says otherwise). Every
  dual-path feature carries a parity test (`TESTING_STANDARD.md` seam rule).
- **I-3 — No null, ever. No exceptions, ever.** Absence is `Option<T>`; failure is `Result<T,E>`. No
  language construct may introduce a null, a hidden default-on-missing, or an unwinding exception as
  control flow. (User-reachable Rust `panic!` is the *graceful-failure* mechanism, caught at the CLI
  boundary, not an exception primitive.)
- **I-4 — User-reachable code never aborts the host.** Any input a user can supply must fail as a
  catchable panic or a clean diagnostic — never SIGABRT, never stack-overflow, never hang. Guards
  (`RECURSION_LIMIT`, `MAX_EXPR_DEPTH`, bounded worker stack) exist for this; new recursive/parsing
  surface extends them.
- **I-5 — Ownership is two-mode (`own`/`ref`), enforced by the borrow checker.** No GC. No
  silent deep-copy where a move/borrow was written. `&[T]` borrows; `for-in` borrows; `ref` is
  binding-only.

## Data layout & ABI (codegen)

- **I-6 — Canonical IR layouts are frozen:** `str = {i64 len, ptr data}` (null-terminated);
  `Result<T,E> = {i1 tag, [max(size T,E) x i8]}` (0=Err,1=Ok); `Option<T> = {i1 tag, T}` (0=None,1=Some);
  slice/array `= {i64 len, ptr data}`. Changing a layout is an ABI break — requires a spec + a version bump.
- **I-7 — Integers default to `i64`; `i32` exists for interop.** A literal's default type does not change.

## The success signal (added 2026-05-31 after the bug hunt)

- **I-8 — Failure exits non-zero; success exits zero (or the program's `i64` return).** Every panic,
  type error, failed verify/deploy-gate, invalid input, and missing-resource path exits non-zero. An
  autonomous loop / CI relies on this absolutely. Diagnostics → **stderr**; program output → **stdout**.
  Distinct failure *classes* get distinct codes so a pipeline can branch: **2** = static check/parse
  error, **3** = `@[verify]`/deploy-gate *policy* rejection (the artifact didn't meet its bound),
  **101** = runtime panic / bug-crash. Interpreter and native runtime agree (BUG_HUNT #26).
- **I-9 — No silent success on degenerate input.** Overflow, undefined-name lookups, inverted/empty
  arguments must produce an error or a *documented, intentional* sentinel — never a plausible-looking
  wrong value that masquerades as success. (This invariant was *violated* at audit time — see
  `BUG_HUNT_2026-05-31.md` #6, #19, #27 — and is the top fix queue.)
- **I-10 — Determinism is available.** `test` and `goal` runs are reproducible given a fixed seed; RNG
  is seedable (`AXON_SEED`). Non-determinism is opt-in, never the silent default for graded/recorded runs.

## Trusted Computing Base (TCB) & capabilities

- **I-11 — The capability boundary is real and total.** Code cannot reach net/fs/exec except through the
  declared-capability surface (`@[contained]` / policy). Any new path to those resources is a TCB change:
  it updates this file, the `ROADMAP.md` TCB section, and ships paired allow + deny tests.
- **I-12 — Self-modification cannot weaken the TCB.** Adaptive/agent/goal machinery that rewrites code or
  config may never grant itself a capability it didn't already hold. (Forward-looking for R4/R10.)
- **I-13 — Provenance for `@[adaptive]`/`@[goal]`/`@[agent]` is not opt-out-able.** Where these zones
  execute, their logging is injected by the compiler/interpreter, not by user cooperation. (Partially
  true today; codegen-side enforcement is R4 work.)

## Surface & docs

- **I-14 — Diagnostics are keyed by stable error codes** (E/W-codes). Code = contract; message = mutable.
  Tests assert on codes. A new diagnostic invents its code at spec time.
- **I-15 — The spec and the behavior do not drift.** Shipping behavior updates its `spec/*.md` /
  `stdlib.md` entry and `REQUIREMENTS.md` in the same change. An undocumented behavior is a latent lie to
  the next builder.

---

## How to change an invariant

Invariants are not immutable — they are *expensive* to change, deliberately. To change one:
1. Write a short proposal (use `SPEC_TEMPLATE.md`) stating the invariant, why it must change, and the blast radius.
2. Enumerate every test, spec, and example that depends on it.
3. Land the invariant edit *here* in the same commit that lands the change, with the proposal linked.
4. Never change an invariant implicitly by merging code that happens to break it — that's how an
   unsupervised system silently rots its own foundations.
