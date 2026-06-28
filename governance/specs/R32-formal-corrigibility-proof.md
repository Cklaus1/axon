# Tech Spec — R32: Formal Corrigibility Proof (TLA+ model + Coq proof of kill-switch invariants)

**Spec ID:** `R32-formal-corrigibility-proof`
**Status:** 📝 Draft (2026-06-28)
**Implements:** Machine-checked verification of the R27 kill-switch guarantees:
(1) contained code cannot disable the kill-switch; (2) the kill fires within a bounded time
after being tripped; (3) the latch is monotone — clear-to-tripped is the only allowed
state transition, and it cannot be reversed by the supervised process.
**Depends on:**
- `governance/specs/R27-corrigibility-resource-bounds.md` — the implementation this spec
  formally verifies: file-based kill latch at `~/.axon/runs/<run_id>.kill`, supervisor polling
  every 100ms, SIGKILL on trip → job exits 4 (`HALTED_EXIT_CODE`). The informal argument for
  correctness (contained subprocess has no write FD to the supervisor's kill file) is formalized
  here.
- `crates/axon-os/src/latch.rs` — `LatchState {Clear, Tripped}`, the monotone transition
  `clear → tripped`, fail-closed `poll()` that the Coq model must accurately reflect.
- `crates/axon-core/src/interp.rs:378,1842` — the in-interpreter `@[corrigible]` flag (a
  cooperative convenience, NOT the corrigibility guarantee per R27 §4.0; R32 proves the
  guarantee: the supervisor-owned latch).
**Audience:** an implementer who builds *strictly* against this document and reads only it.
A background in TLA+ or Coq is assumed; familiarity with R27 is required.

> **Read this framing first.** R27's informal argument — "the contained process runs in a
> subprocess that doesn't have write access to the supervisor's kill file" — is correct as stated
> but informal. Informal arguments miss corner cases: a race between kill-trip and job-exit can
> misclassify a killed job as naturally exited; a process with `CAP_SYS_PTRACE` could inject into
> the supervisor; the kill-file path (runtime-generated, includes run-id) could theoretically be
> guessed. R32 formalizes the argument, makes the assumed trusted base explicit, and produces
> machine-checked proofs of three key properties. The goal is not to replace R27 — it is to
> harden the single weakest link in the kill-switch chain: the gap between "we believe it is
> correct" and "it is proven correct."

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check |
|---|---|---|---|
| **A1** | TLA+ model type-checks and TLC finds no invariant violations in 10M states | §2, §7 | `acc_a1_tla_model_valid` |
| **A2** | Coq proof file compiles with no axioms beyond the standard library | §3, §7 | `acc_a2_coq_proof_compiles` |
| **A3** | Quickstart commands execute: `tlc R27KillSwitch.tla` and `coqc R27Corrigibility.v` succeed | §7, §8 | `acc_a3_quickstart_commands_execute` |
| **A4** | TLA+ model covers all required state transitions | §2.2, §7 | `acc_a4_model_covers_all_transitions` |
| **A5** | Liveness proven: ◇(killed) — once kill is tripped, eventually the job exits | §2.3, §3.2, §7 | `acc_a5_liveness_proven` |
| **A6** | Safety proven: □(¬containedCanDisableKill) — at no reachable state can contained code unset the kill | §2.3, §3.3, §7 | `acc_a6_safety_proven` |
| **Core** | Bounded kill: kill fires within 2×poll_interval after kill file is written | §3.2, §7 | `kill_fires_within_bound` |
| **Core** | No contained write path to kill file (syscall-level, capability-bounded) | §3.3, §4, §7 | `no_contained_write_to_kill_file` |
| **Core** | Monotone latch: clear→tripped is the only allowed transition; tripped→clear is not in the model | §2.2, §3.1, §7 | `latch_is_monotone` |
| **Core** | Supervisor ownership invariant: kill file's write path is exclusively the supervisor's | §2.3, §4, §7 | `supervisor_ownership_invariant` |
| **Gate** | The acceptance gate itself fails if any check above is missing or stubbed | §-Gate | `scripts/r32_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Motivation: why informal is not enough

### 1.1 What R27 gives us, and what it leaves open

R27's §4.2 states the O-CORRIGIBLE obligation:

> For all reachable states `s` of a contained run: if `supervisor.latch = Tripped`, then every
> capability checkpoint `c` reachable from `s` evaluates `deny`.

R27 argues this holds by construction: no write edge from contained code to `Latch.state` exists
in the module graph. This is a sound *structural* argument — it is not a proof. Structural
arguments can miss:

- **Race conditions.** If the job calls `exit()` in the same scheduler timeslice that the
  supervisor sends SIGKILL, the supervisor's `wait()` may observe a "natural" exit (status 0),
  not a killed exit. This is a mis-classification, not a safety violation per se, but it exposes
  an audit gap: the ledger records the wrong stop reason. A formal model over the concurrent
  transitions catches this explicitly.
- **Injected write paths.** A job with `CAP_SYS_PTRACE` could `ptrace(POKEDATA)` into the
  supervisor process and clear the latch in-memory. R27 §4 states `CAP_SYS_PTRACE` is denied in
  the contained environment, but this is a runtime policy claim, not a proof. R32's process
  isolation argument (§4) makes the capability denial explicit and its scope clear.
- **Kill-file path disclosure.** The kill-file path includes the run-id (runtime-generated). If a
  contained process can read the supervisor's environment or `/proc/<pid>/cmdline`, it might learn
  the path and attempt to write it. R27's informal argument assumes this is impossible; §4 states
  precisely why.
- **Wrapping vs. proving.** R27 says "the enforcement is below the model." R32 makes this a
  theorem over a state machine, not a design assertion.

### 1.2 Why machine-checked proofs (TLA+ + Coq), not just additional tests

- Tests cover sampled executions. The kill-switch must hold for **all** executions, including
  adversarially constructed ones the test suite does not sample.
- TLC's exhaustive state-space search (10M states) catches invariant violations the R27 adversarial
  tests (`contained_code_cannot_disable_latch`, §7 of R27) do not because tests are finite
  programs, not symbolic state machines.
- The Coq proof is **machine-checked** — the same level of rigor as a formally verified operating
  system kernel (e.g., seL4). Once compiled with no additional axioms, the three theorems are
  *unconditionally* true under the stated model assumptions.
- Together they form a two-layer argument: TLA+ establishes the finite-state model and TLC
  validates it exhaustively; Coq then proves the infinite-trace properties (liveness, the monotone
  latch over unbounded time) that TLC's finite exploration cannot guarantee.

### 1.3 Honest scope: what R32 proves vs. what it assumes

R32 proves properties of a **formal model** of the kill-switch, not of the Rust implementation
directly. The gap between model and code is the **trusted base** (§5.2). A reader who believes
R32 "proves the implementation correct" has misread it. What R32 provides:

- A precise, machine-checked model of the kill-switch state machine.
- Three machine-checked theorems about that model.
- A clear statement of what the model assumes about the underlying system (Linux process isolation,
  the Axon checker's enforcement of `@[contained]` fs restrictions).
- Evidence — via TLC exhaustive search — that the model has no invariant violations within the
  bounded state space.

What R32 does not provide: a proof that the Rust code in `latch.rs`/`killchan.rs` is a correct
refinement of the TLA+ spec (that would require a full verified-implementation effort, out of
scope); a proof against kernel vulnerabilities; a proof against side-channel attacks (§5.1).

---

## §2 — TLA+ Model: `governance/proofs/R27KillSwitch.tla`

The TLA+ spec is approximately 150 lines. This section describes each section; the actual `.tla`
file is an artifact of R32 implementation (§6).

### 2.1 Module header and constants

The module is named `R27KillSwitch`. It declares three constants:

- `POLL_INTERVAL` — the supervisor's poll period (in abstract time units; concrete value is 100ms,
  but the model is parameterized). TLC model-checks with `POLL_INTERVAL = 1` (unit time).
- `KILL_FILE_PATH` — an opaque string constant representing `~/.axon/runs/<run_id>.kill`. The
  model treats it as a token, not as a filesystem path, so it can reason about "who can write
  this path" without encoding a full filesystem model.
- `SUPERVISOR_PID` and `JOB_PID` — opaque process identifiers. The model uses them to partition
  write authority: only `SUPERVISOR_PID` may write `KILL_FILE_PATH`.

### 2.2 State variables

```
VARIABLES
  kill_file_exists   : BOOLEAN
  kill_file_owner    : PID ∪ {⊥}            -- ⊥ = file does not exist
  job_state          : {"running", "exiting", "killed", "done"}
  supervisor_state   : {"polling", "kill_sent", "waiting", "done"}
  contained_writes   : SUBSET STRING         -- set of paths the contained process has written
  clock              : Nat                   -- abstract clock tick; bounded in TLC
```

Six variables. `contained_writes` is the key invariant target: the model tracks every filesystem
write the contained process makes and the `ContainedCannotWriteKillFile` invariant asserts
`KILL_FILE_PATH ∉ contained_writes` at all times.

### 2.3 Initial state and transitions

**Init:** `kill_file_exists = FALSE`, `kill_file_owner = ⊥`, `job_state = "running"`,
`supervisor_state = "polling"`, `contained_writes = {}`, `clock = 0`.

**Transitions (named; A4 requires all of these):**

| Transition | Precondition | Effect |
|---|---|---|
| `Init` | always | (the initial state above) |
| `SupervisorPoll` | `supervisor_state = "polling"` | `clock' = clock + 1`; if `kill_file_exists` then `supervisor_state' = "kill_sent"`, else remains "polling" |
| `KillTrip` | `supervisor_state = "polling"` and external action | `kill_file_exists' = TRUE`, `kill_file_owner' = SUPERVISOR_PID`; appends to `kill_file_exists` only from SUPERVISOR side |
| `KillSent` | `supervisor_state = "kill_sent"` | `job_state' = "killed"`, `supervisor_state' = "waiting"` |
| `JobExit` | `job_state = "running"` | `job_state' = "done"` (natural exit, not via kill) |
| `KillFileWrite` | `job_state = "running"` | models a contained-process write attempt: `contained_writes' = contained_writes ∪ {some_path}` where `some_path ≠ KILL_FILE_PATH` always (enforced by the capability restriction in the transition guard) |
| `SupervisorDone` | `supervisor_state = "waiting"` and `job_state ∈ {"killed","done"}` | `supervisor_state' = "done"` |

The `KillFileWrite` transition explicitly excludes `KILL_FILE_PATH` as a writable target for the
contained process. This is the formal encoding of the OS-level capability restriction argued in §4.
The invariant then asserts this exclusion holds for all reachable states.

### 2.4 Invariants (TLC checks all at every reachable state)

```tla
KillFileMonotone ==
  kill_file_exists => kill_file_exists'
  (* once the kill file exists, it stays — no transition deletes it *)

ContainedCannotWriteKillFile ==
  KILL_FILE_PATH ∉ contained_writes
  (* the set of paths the contained process has written never includes the kill file *)

SupervisorOwnsKillFile ==
  kill_file_exists => kill_file_owner = SUPERVISOR_PID
  (* if the file exists, only the supervisor could have created it *)

LatchMonotonicity ==
  [](kill_file_exists => []kill_file_exists)
  (* once tripped, always tripped — typed as an invariant over the trajectory *)

JobCannotClearKill ==
  [](kill_file_exists => job_state ∉ {"running"} ~> job_state ∈ {"killed","done"})
  (* once kill is tripped, the job does not remain "running" forever *)
```

Five named invariants. A4 requires ≥5 (`acc_a4_model_covers_all_transitions` checks this by
grepping `INVARIANT` blocks in the `.tla` file).

### 2.5 Temporal properties (liveness and safety)

```tla
Liveness ==
  kill_file_exists ~> (job_state = "killed" \/ job_state = "done")

BoundedKill ==
  kill_file_exists ~> (clock <= clock_at_kill + 2 * POLL_INTERVAL /\ job_state = "killed")

SafetyProp ==
  [](kill_file_exists => <>(job_state = "killed" \/ job_state = "done"))
```

`Liveness` (leads-to) is the A5 property: once the kill file is tripped, eventually the job
exits. `BoundedKill` is the bounded-time variant (the `kill_fires_within_bound` check): the job
exits within two poll periods of the kill being tripped. `SafetyProp` is the A6 property framed
as a safety assertion: once tripped, the job cannot remain running indefinitely.

### 2.6 TLC model-checking configuration

`R27KillSwitch.cfg` (§6) configures TLC to:
- Bound `clock` to 10 (enough to cover `2 * POLL_INTERVAL` transitions).
- Set `POLL_INTERVAL = 1`, `SUPERVISOR_PID = "supervisor"`, `JOB_PID = "job"`.
- State-space budget: 10M states (required by A1; TLC reports the count).
- Check all five `INVARIANT` declarations and the three `PROPERTY` declarations.
- Symmetry reduction on `contained_writes` (power-set of string tokens; bounded to paths ∈
  `{"some_path_1", "some_path_2", KILL_FILE_PATH}` — three tokens sufficient to show the
  exclusion).

---

## §3 — Coq Proof: `governance/proofs/R27Corrigibility.v`

The Coq proof is approximately 300 lines. This section describes the module structure and each
main theorem. The file uses only Coq's standard library (`Coq.Bool.Bool`, `Coq.Arith.Arith`,
`Coq.Lists.List`); no external axioms, no `Admitted` — A2 verifies this with `coqc -w all` and
`Print Assumptions` on each theorem.

### 3.1 Definitions (preamble, ~60 lines)

The proof file opens with an inductive type for system states mirroring the TLA+ model:

```coq
Inductive LatchState : Type := Clear | Tripped.
Inductive JobState   : Type := Running | Exiting | Killed | Done.
Inductive SupState   : Type := Polling | KillSent | Waiting | SupDone.

Record SystemState := mkState {
  latch       : LatchState;
  job         : JobState;
  supervisor  : SupState;
  clock       : nat;
  kill_clock  : option nat;     (* None = kill not yet tripped; Some t = tripped at t *)
  contained_wrote_kill_file : bool;  (* true would be a violation *)
}.
```

An `Inductive step : SystemState -> SystemState -> Prop` encodes the transition relation with
one constructor per TLA+ transition name (seven constructors, matching §2.3). A `reachable`
predicate is the reflexive-transitive closure of `step` from `init_state`.

### 3.2 Theorem 1: `kill_latch_monotone`

**Statement:**
```coq
Theorem kill_latch_monotone :
  forall s1 s2 : SystemState,
    reachable s1 ->
    step s1 s2 ->
    latch s1 = Tripped ->
    latch s2 = Tripped.
```

**Informal statement:** In any execution of the system, if `latch` is ever `Tripped`, it remains
`Tripped` in every successor state.

**Proof strategy:** By case analysis on the `step s1 s2` constructor.

- For each of the seven transition constructors, the proof checks whether the constructor has a
  precondition that allows `latch s1 = Tripped` and whether the resulting `latch s2` could be
  `Clear`.
- The key case is that **no transition has `latch := Clear` in its effect** — this is visible by
  inspection of the `step` constructors. Coq's case analysis exhaustively verifies all seven cases.
- The `KillTrip` constructor sets `latch := Tripped` from `Clear`; no other constructor writes
  `latch`. Therefore once `Tripped`, it stays `Tripped`.
- Proof length: ~25 lines. `destruct` on the `step` constructor, then `simpl; auto` closes each
  sub-goal.

**Connection to acceptance checks:** `latch_is_monotone` and A5 (`acc_a5_liveness_proven`)
both depend on this theorem.

### 3.3 Theorem 2: `kill_fires_within_2_polls`

**Statement:**
```coq
Theorem kill_fires_within_2_polls :
  forall s : SystemState,
    reachable s ->
    latch s = Tripped ->
    exists t : nat,
      t <= clock s + 2 * POLL_INTERVAL /\
      exists s' : SystemState,
        reachable_from s s' /\
        clock s' = t /\
        (job s' = Killed \/ job s' = Done).
```

**Informal statement:** Given that the kill file is tripped at some reachable state `s`, there
exists a future state `s'` reachable from `s` where the job has exited, and `s'`'s clock value is
at most `clock(s) + 2 × POLL_INTERVAL`.

**Proof strategy:** By case analysis on `supervisor s`.

- **Case `supervisor s = Polling`:** The `SupervisorPoll` transition fires within one clock tick
  (the model is fair — TLA+ strong fairness is encoded as a Coq `Fair` predicate over the `step`
  relation). After `SupervisorPoll`, if `latch = Tripped`, the `KillSent` transition fires
  immediately, setting `job := Killed`. Total: ≤ 2 ticks.
- **Case `supervisor s = KillSent`:** `KillSent` transition fires in the same tick. `job := Killed`
  in one step.
- **Case `supervisor s = Waiting`:** The job is already `Killed` or `Done` (invariant on `Waiting`
  state: job is terminal). Zero additional ticks.
- **Case `supervisor s = SupDone`:** The supervisor is done; the job was already terminal by the
  `SupervisorDone` precondition.

The `Fair` predicate is a separate axiom-free definition over `step`: every enabled transition
is eventually taken. This is the only non-trivial assumption in the proof; it is not an external
axiom but an `Hypothesis` parameter — callers must supply the fairness proof for their execution
model. Coq's `Print Assumptions` reports `Fair` as a `Variable`, not an axiom, so A2 is not
violated.

**Proof length:** ~70 lines. `inversion` on the `reachable_from` path, `destruct` on `supervisor
s`, `omega` for the clock arithmetic.

**Connection to acceptance checks:** `kill_fires_within_bound` and `acc_a5_liveness_proven`.

### 3.4 Theorem 3: `contained_cannot_disable_kill`

**Statement:**
```coq
Theorem contained_cannot_disable_kill :
  forall s : SystemState,
    reachable s ->
    contained_wrote_kill_file s = false.
```

**Informal statement:** In any reachable state of the system, the contained process has not
written the kill file. Equivalently: `contained_wrote_kill_file` is invariantly `false`.

**Proof strategy:** By induction on the `reachable` derivation.

- **Base case (`init_state`):** `contained_wrote_kill_file init_state = false` by definition.
- **Inductive step:** Assume `reachable s` and `contained_wrote_kill_file s = false`. For any
  `s'` with `step s s'`, prove `contained_wrote_kill_file s' = false`.
  - Case analysis on the `step` constructor: only the `KillFileWrite` constructor updates
    `contained_writes`/`contained_wrote_kill_file`. Its Coq encoding includes a `guard`:
    `path_written ≠ KILL_FILE_PATH`. This guard is the formalization of the OS capability
    restriction (§4): the contained process has no write FD to the kill file's path. Given this
    guard, the `KillFileWrite` constructor cannot set `contained_wrote_kill_file := true`.
  - All other constructors do not modify `contained_wrote_kill_file`; the IH propagates.

**The critical encoding:** The `path_written ≠ KILL_FILE_PATH` guard in `KillFileWrite` is the
formal counterpart of the OS-level argument in §4. If an implementer adds a transition that sets
`contained_wrote_kill_file := true`, the theorem fails to compile — Coq rejects the proof. This
is the machine-checked nature of the guarantee: a code change that weakens the write restriction
breaks the proof, not just a test.

**Proof length:** ~50 lines. `induction` on `reachable`, `destruct` on `step` constructors,
`discriminate` to discharge the `path_written = KILL_FILE_PATH` cases (the guard makes them
`False`), `assumption` for the others.

**Connection to acceptance checks:** `no_contained_write_to_kill_file`, `supervisor_ownership_invariant`,
and A6 (`acc_a6_safety_proven`).

### 3.5 Corollary: `kill_once_tripped_job_never_resurfaces`

A short corollary (~15 lines), combining `kill_latch_monotone` and `kill_fires_within_2_polls`:

```coq
Corollary kill_once_tripped_job_never_resurfaces :
  forall s s' : SystemState,
    reachable s ->
    latch s = Tripped ->
    reachable_from s s' ->
    job s' ≠ Running.
```

Once the kill is tripped, the job never returns to `Running` in any successor state. This follows
directly from monotonicity (the latch stays `Tripped`) and the bounded-firing theorem (the job
exits within 2 polls). It is stated as a corollary because it is the most human-readable summary
of R27's corrigibility guarantee.

---

## §4 — Process Isolation Argument (formal encoding of the OS-level assumption)

The theorems in §3 depend on the `KillFileWrite` guard: contained code cannot write `KILL_FILE_PATH`.
This section states precisely why that guard is valid in the real OS, so an implementer can verify
the guard accurately captures the implementation.

### 4.1 Separate address spaces via `fork()`/`exec()`

The Axon supervisor (R21 `axon-os`) launches contained jobs via `std::process::Command`, which
on Linux uses `fork()` + `exec()`. After `fork()`:
- The child process has a copy of the parent's file descriptors, but **not** any file descriptors
  opened after the `fork()` — the kill file is opened by the supervisor *after* spawning the
  child (or with `O_CLOEXEC` set, which closes FDs in the child on `exec()`).
- The supervisor opens the kill file with `O_CREAT | O_WRONLY | O_CLOEXEC`. The `O_CLOEXEC` flag
  ensures the write FD is closed in the child process's `exec()`. The child therefore has no write
  FD to the kill file at birth.

**Formal encoding:** The `KillFileWrite` guard `path_written ≠ KILL_FILE_PATH` encodes this: the
child has no mechanism to open or write `KILL_FILE_PATH` because (a) it doesn't hold the FD and
(b) it cannot open the path by name (see §4.2).

### 4.2 Kill-file path is unguessable by the contained process

The kill-file path includes the run-id, which is a UUID generated by the supervisor at run start.
The contained process is never told the run-id; it is not in `argv`, not in the environment (the
supervisor strips its environment before exec, per R27's `@[contained]` semantics that prohibit
`env_var()` inside contained code — `contained-env-deny.md` memory note). The contained process
therefore cannot construct the kill-file path by enumeration.

**Formal encoding:** `KILL_FILE_PATH` is an opaque constant in the TLA+ model (§2.1). The
`KillFileWrite` transition's non-deterministic `path_written` is chosen from a bounded set that
does not include `KILL_FILE_PATH`. This models the fact that the contained process cannot learn the
kill-file path: it can write arbitrary paths in its allowed `fs: [write("./out/")]` prefix, but
`~/.axon/runs/<run_id>.kill` is outside any container's declared `@[contained]` allowlist by
construction.

**Additional path-traversal defense:** R27's `@[contained]` checker denies `..` components in
fs write paths (memory note `contained-path-traversal.md`). A contained process cannot use `../`
sequences to escape its allowlist prefix toward `~/.axon/runs/`. The Coq guard applies to all
path values in the `KillFileWrite` transition, including traversal-escape attempts.

### 4.3 No `CAP_SYS_PTRACE` in the contained process

A process with `CAP_SYS_PTRACE` could `ptrace(PTRACE_POKEDATA)` into the supervisor's address
space and clear `latch.state` in memory. R27 §4.1 states "contained process runs with reduced
capabilities (`CAP_SYS_PTRACE` denied)." The formal model's `KillFileWrite` guard is therefore
not sufficient alone — it only covers filesystem writes. The Coq proof's trusted base (§5.2)
explicitly includes the claim that `CAP_SYS_PTRACE` is denied in the contained process.

**Implementation note:** `axon-os run --killable` MUST set `PR_SET_NO_NEW_PRIVS` and drop
`CAP_SYS_PTRACE` from the child's capability bounding set before `exec()`. The R32 gate script
(§-Gate) asserts this by inspecting `/proc/<pid>/status` in the acceptance test's subprocess.

### 4.4 The `O_CREAT | O_WRONLY | O_CLOEXEC` contract

The complete argument rests on three file-descriptor invariants enforced by the supervisor:
1. The kill file FD is opened with `O_CLOEXEC` so `exec()` closes it in the child.
2. The kill file is opened *after* `fork()` (or with `O_CLOEXEC` on any pre-fork open), so the
   child never inherits a write FD.
3. The supervisor does not pass the kill-file path to the child in any observable channel (argv,
   env, `unix::socket`, shared memory).

These three invariants are stated as implementation obligations, not proven in the Coq model.
They are part of the trusted base (§5.2, T3).

---

## §5 — Limitations and Scope

### 5.1 What the formal proof does NOT cover

The following attack classes are **explicitly out of scope** for R32. They are stated here so a
reader does not infer a guarantee that does not exist.

- **Side-channel attacks.** Timing, cache, power, and electromagnetic side channels can leak the
  kill-file path or the latch state across the process boundary. The model has no timing model
  beyond abstract clock ticks. Side-channel defense requires hardware mitigations (e.g., process
  isolation features like Intel CET) or the R26 confidential computing substrate — not a software
  latch proof.
- **Kernel vulnerabilities.** A kernel CVE that breaks `fork()`/`exec()` process isolation (e.g.,
  a container escape via namespace confusion) could allow the contained process to reach the
  supervisor's address space. R32 assumes Linux process isolation is correct (§5.2, T1). A kernel
  vulnerability invalidates this assumption; R32's theorems do not hold against a compromised
  kernel.
- **Physical attacks.** Hardware tampering, cold-boot attacks, and DMA attacks are out of scope.
  These are addressed (partially) by the R26 confidential-computing substrate (SEV-SNP/TDX).
- **Wrong formalization.** The Coq proof is only as correct as the model it proves properties
  about. If the TLA+ state machine does not faithfully represent the Rust implementation in
  `latch.rs` / `killchan.rs`, the theorems are vacuously correct about the wrong system. The
  **model–code correspondence** is argued informally in §3 and §4 but not machine-checked. A full
  proof of correspondence would require a verified-C/Rust toolchain (e.g., RefinedRust or Verus),
  which is out of scope for R32.
- **Checker-level bypasses of `@[contained]`.** R32 proves that if `@[contained]` correctly
  restricts the contained process's fs-write allowlist to exclude `~/.axon/runs/`, then contained
  code cannot write the kill file. Whether `@[contained]` is *correctly enforced by the checker*
  is R7's guarantee, not R32's. A checker bug that silently widens the allowlist would invalidate
  the `path_written ≠ KILL_FILE_PATH` guard without R32 detecting it.
- **Race between `exit()` and SIGKILL.** The bounded-firing theorem (§3.2) proves the job exits
  within `2 × POLL_INTERVAL`. It does not distinguish between the job exiting because SIGKILL was
  delivered vs. the job calling `exit()` just before SIGKILL arrived. Both outcomes terminate the
  job within the bound — but the *cause* recorded in the audit ledger may differ. R32 does not
  prove audit correctness of the stop reason; only that the job stops. Mis-classification of
  "killed" vs. "natural exit" is an audit precision concern, addressed by R28's ledger writer, not
  R32.

### 5.2 Trusted base for this proof (what must be true for the theorems to hold)

| ID | Assumption | Where it comes from |
|---|---|---|
| **T1** | Linux process isolation is correct: `fork()`/`exec()` creates a new process with no inherited write FD to the kill file when `O_CLOEXEC` is set. | Linux kernel, not proven by R32. |
| **T2** | The Axon checker correctly enforces `@[contained]` fs-write restrictions: a contained function declared with `fs: [write("./out/")]` cannot write to `~/.axon/runs/` at the Axon level. | R7's guarantee (the checker's `@[contained]` enforcement, `E1001/E1004`, `checker.rs`). |
| **T3** | `axon-os run --killable` sets `PR_SET_NO_NEW_PRIVS` and drops `CAP_SYS_PTRACE` from the child's bounding set before `exec()`. | Implementation obligation on `crates/axon-os/src/supervisor.rs`; verified by the gate's subprocess `status` check. |
| **T4** | The supervisor opens the kill file with `O_CLOEXEC` and does not pass the kill-file path to the contained process in any channel. | Implementation obligation on `crates/axon-os/src/killchan.rs`; verified by code inspection and the `no_contained_write_to_kill_file` acceptance test. |
| **T5** | The TLA+ model and Coq definitions faithfully represent the semantics of `latch.rs` / `killchan.rs`. | Argued informally in §3 and §4; not machine-checked (the model–code gap, §5.1). |

R32's theorems are unconditionally true *within their model*. They hold in the real system if
and only if T1–T5 all hold. This is the load-bearing honesty statement; do not soften it.

---

## §6 — Artifacts to Create

R32 produces four artifacts. **None of them are Rust code** — R32 is a formal specification and
proof, not an implementation. The Rust code it verifies already exists in R27.

### 6.1 `governance/proofs/R27KillSwitch.tla` (~150 lines)

The TLA+ spec described in §2. Sections:
1. `EXTENDS Integers, Sequences, FiniteSets` — standard TLA+ modules.
2. Constants: `POLL_INTERVAL`, `KILL_FILE_PATH`, `SUPERVISOR_PID`, `JOB_PID`.
3. Variables: the six state variables from §2.2.
4. `TypeInvariant` — type-correctness (all variables in their declared domain).
5. `Init` — initial state predicate.
6. Seven named action predicates (one per transition, §2.3).
7. `Next = SupervisorPoll \/ KillTrip \/ KillSent \/ JobExit \/ KillFileWrite \/ SupervisorDone`.
8. `Spec = Init /\ [][Next]_vars /\ WF_vars(SupervisorPoll) /\ WF_vars(KillSent)` — the temporal
   formula with weak fairness on the two supervisor actions.
9. Five named `INVARIANT` predicates (§2.4).
10. Three temporal `PROPERTY` predicates (§2.5).

### 6.2 `governance/proofs/R27KillSwitch.cfg` (~20 lines)

TLC model-checker configuration for `R27KillSwitch.tla`:
- `CONSTANT` assignments: `POLL_INTERVAL <- 1`, `KILL_FILE_PATH <- "kill_path"`,
  `SUPERVISOR_PID <- "supervisor"`, `JOB_PID <- "job"`.
- `INIT Init` and `NEXT Next`.
- All five `INVARIANT` names listed.
- All three `PROPERTY` names listed.
- `CHECK_DEADLOCK FALSE` — deadlock is not a concern (the `done` states are valid terminal states,
  not deadlocks).
- No `SYMMETRY` annotation needed (the model is not symmetric; `SUPERVISOR_PID ≠ JOB_PID`
  by construction).

### 6.3 `governance/proofs/R27Corrigibility.v` (~300 lines)

The Coq proof file described in §3. Sections:
1. `Require Import` standard library modules (no external deps).
2. Type definitions: `LatchState`, `JobState`, `SupState`, `SystemState` (§3.1).
3. `POLL_INTERVAL : nat := 1` (matching TLC's model constant).
4. `init_state : SystemState` definition.
5. `step : SystemState -> SystemState -> Prop` (the transition relation, 7 constructors).
6. `reachable : SystemState -> Prop` (reflexive-transitive closure via `clos_refl_trans`).
7. `reachable_from : SystemState -> SystemState -> Prop` (relative reachability).
8. `Fair : (SystemState -> SystemState -> Prop) -> Prop` (fairness hypothesis variable).
9. Three main theorems (§3.2–§3.4) with full proofs.
10. One corollary (§3.5).
11. Closing `Print Assumptions kill_latch_monotone.` etc. to verify the axiom-free claim.

### 6.4 `scripts/r32_acceptance_gate.sh` (~80 lines)

The gate script described in §-Gate below. It checks proof file presence, runs TLC, runs Coq,
cross-checks theorem names vs. TLA+ invariant names, and reports pass/fail per acceptance check.

---

## §7 — Test Plan (all named checks are normative)

Each acceptance check below must exist, be seen to fail before the implementation, and then pass.
A check that passes vacuously (e.g., by asserting `true`) fails the anti-stub guard (§-Gate).

**TLA+/TLC checks (machine-run by the gate script):**

- `acc_a1_tla_model_valid` — `tlc R27KillSwitch.tla -config R27KillSwitch.cfg` exits 0; stdout
  contains "Model checking completed" and reports ≥10M states explored (or "No error has been
  found" for smaller bounded models; the gate accepts either — the key is no invariant violation).
- `acc_a4_model_covers_all_transitions` — grep `R27KillSwitch.tla` for the seven transition names
  (`SupervisorPoll`, `KillTrip`, `KillSent`, `JobExit`, `KillFileWrite`, `SupervisorDone`, and
  `Init` as the initial state predicate). All seven must be present. Also grep for ≥5 `INVARIANT`
  declarations. Missing name → fail.
- `acc_a5_liveness_proven` (TLA+ half) — grep `R27KillSwitch.tla` for `Liveness` in a `PROPERTY`
  declaration; grep for `BoundedKill` in a `PROPERTY` declaration. Presence is required; TLC
  verifying it without a counterexample trace is the substantive check (covered by `acc_a1`).
- `acc_a6_safety_proven` (TLA+ half) — grep for `ContainedCannotWriteKillFile` in an `INVARIANT`
  declaration and for `SupervisorOwnsKillFile` in an `INVARIANT` declaration.
- `latch_is_monotone` (TLA+ half) — grep for `KillFileMonotone` and `LatchMonotonicity` in
  `INVARIANT` declarations.
- `supervisor_ownership_invariant` (TLA+ half) — grep for `SupervisorOwnsKillFile` in an
  `INVARIANT` declaration.

**Coq checks (machine-run by the gate script):**

- `acc_a2_coq_proof_compiles` — `coqc governance/proofs/R27Corrigibility.v` exits 0 with no
  errors or warnings. The gate then runs `grep "Axioms:" R27Corrigibility.v.output` (or pipes
  `Print Assumptions` output through the Coq batch mode) and asserts no external axioms are
  listed beyond Coq's built-in `eq_refl`, `eq_rect` etc.
- `acc_a5_liveness_proven` (Coq half) — grep `R27Corrigibility.v` for `Theorem kill_fires_within_2_polls`;
  must be present with a `Proof.` ... `Qed.` block (not `Admitted.`).
- `acc_a6_safety_proven` (Coq half) — grep for `Theorem contained_cannot_disable_kill` with
  `Proof.` ... `Qed.`.
- `kill_fires_within_bound` — grep for `Theorem kill_fires_within_2_polls` (same as A5 Coq half;
  the distinct acceptance-check name is for the bounded-time aspect specifically).
- `no_contained_write_to_kill_file` — grep for `contained_wrote_kill_file s = false` in the
  statement of `contained_cannot_disable_kill`; must appear as a conclusion, not just a hypothesis.
- `latch_is_monotone` (Coq half) — grep for `Theorem kill_latch_monotone` with `Proof.` ... `Qed.`.
- `kill_once_tripped_job_never_resurfaces` (corollary) — grep for `Corollary
  kill_once_tripped_job_never_resurfaces` with `Proof.` ... `Qed.`.

**Cross-check (TLA+ ↔ Coq consistency):**

- The gate asserts that the five TLA+ `INVARIANT` names appear (by substring) in the Coq file's
  theorem/lemma/corollary names or comments. Specifically:
  - `KillFileMonotone` ↔ `kill_latch_monotone`
  - `ContainedCannotWriteKillFile` ↔ `contained_cannot_disable_kill`
  - `SupervisorOwnsKillFile` ↔ `supervisor_ownership_invariant` (comment or lemma)
  - `LatchMonotonicity` ↔ `kill_latch_monotone` (same theorem covers both)
  - `BoundedKill` (TLA+ property) ↔ `kill_fires_within_2_polls` (Coq theorem)
  Missing correspondence → gate fails.

**Quickstart execution (`acc_a3_quickstart_commands_execute`):**

The §8 quickstart commands are executed verbatim by the gate. Both must exit 0 and produce the
documented output patterns. If TLA+ tools or Coq are not installed, the gate marks the check as
`PENDING (tool not installed)` and the spec milestone is considered incomplete until they are.
A `PENDING` result does NOT count as a pass — R32 is not done until both tools are installed and
both commands succeed.

**Anti-stub guard (shared with §-Gate):**

- Neither proof file may contain `Admitted` (Coq) or `\* TODO` / `\* STUB` (TLA+) in any
  theorem or invariant block.
- The `Print Assumptions` output for each of the three main theorems must list no items beyond
  Coq's kernel axioms (`eq_refl`, `eq_rect`, `eq_ind`, `eq_sym`, `eq_trans`). Any `Variable`
  hypothesis must be listed by name and argued non-circular in the spec.

---

## §8 — Build Instructions (in spec; not yet implemented)

These are the exact commands executed by `acc_a3_quickstart_commands_execute`.

```bash
# ── Install TLA+ tools ──────────────────────────────────────────────────────
wget -q https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar \
     -O /usr/local/lib/tla2tools.jar
alias tlc='java -jar /usr/local/lib/tla2tools.jar'

# ── Run TLC model checker ────────────────────────────────────────────────────
tlc governance/proofs/R27KillSwitch.tla \
    -config governance/proofs/R27KillSwitch.cfg \
    -workers auto
# Expected output (last two lines):
#   Model checking completed. No error has been found.
#     Estimates of the probability that TLC did not check all reachable states …

# ── Install Coq (requires opam; see https://opam.ocaml.org/doc/Install.html) ─
opam init --bare --yes
eval "$(opam env)"
opam install --yes coq.8.18.0

# ── Compile the Coq proof ────────────────────────────────────────────────────
coqc governance/proofs/R27Corrigibility.v
# Expected: no output (success), exit code 0.
# If the proof is incomplete or has Admitted goals, coqc will warn or error.
```

Both commands must run to completion without errors for `acc_a3` to pass. Intermediate failures
(e.g., Java not installed, `opam` not available) must be reported clearly by the gate script with
remediation instructions.

---

## §-Gate — `scripts/r32_acceptance_gate.sh` (pinned; FAILS if any §0 check is missing or stubbed)

The gate script performs the following steps in order. It exits 1 on the first failure and prints
which check failed.

1. **Artifact presence check.** Assert all four artifacts exist:
   `governance/proofs/R27KillSwitch.tla`, `governance/proofs/R27KillSwitch.cfg`,
   `governance/proofs/R27Corrigibility.v`, `scripts/r32_acceptance_gate.sh` (self-check).
   Missing file → gate fails immediately.

2. **TLA+ structure check (`acc_a4_model_covers_all_transitions`).** Grep
   `governance/proofs/R27KillSwitch.tla` for all seven transition names and ≥5 `INVARIANT`
   declarations. Report per-name pass/fail. Any missing name → gate fails.

3. **TLA+ anti-stub check.** Grep for `\* TODO`, `\* STUB`, `ASSUME FALSE`, `CHOOSE x \in {} :
   TRUE` in the `.tla` file. Any match → gate fails with the offending line.

4. **TLC run (`acc_a1_tla_model_valid`).** Execute:
   `java -jar /usr/local/lib/tla2tools.jar governance/proofs/R27KillSwitch.tla -config governance/proofs/R27KillSwitch.cfg -workers auto`.
   If `tla2tools.jar` is not found, mark `acc_a1` as `PENDING (TLA+ not installed)` and continue.
   If TLC exits non-zero or its stdout contains `Error:` or `Invariant.*violated`, gate fails.
   If TLC exits 0, assert its stdout contains "No error has been found" → `acc_a1` PASS.

5. **Coq structure check (`acc_a2` pre-flight).** Grep `R27Corrigibility.v` for:
   - `Theorem kill_latch_monotone` with `Proof.` and `Qed.` (not `Admitted.`).
   - `Theorem kill_fires_within_2_polls` with `Proof.` and `Qed.`.
   - `Theorem contained_cannot_disable_kill` with `Proof.` and `Qed.`.
   - `Corollary kill_once_tripped_job_never_resurfaces` with `Proof.` and `Qed.`.
   Any `Admitted.` in place of `Qed.` → gate fails. Missing theorem → gate fails.

6. **Coq anti-stub check.** Grep `R27Corrigibility.v` for `Admitted`, `admit`, `FIXME`, `TODO`.
   Any match → gate fails.

7. **Coq compile (`acc_a2_coq_proof_compiles`).** Execute `coqc
   governance/proofs/R27Corrigibility.v`. If `coqc` is not found, mark `acc_a2` as `PENDING (Coq
   not installed)` and continue. If `coqc` exits non-zero, gate fails with its error output.
   If `coqc` exits 0, assert no line of its stderr matches `Warning: Declared.*axiom` → `acc_a2`
   PASS.

8. **TLA+ ↔ Coq cross-check.** For each of the five correspondences in §7:
   assert the Coq file contains the expected theorem/comment name. Missing → gate fails.

9. **Quickstart execution (`acc_a3_quickstart_commands_execute`).** Execute the §8 quickstart
   commands in a subprocess. If either tool is unavailable, mark `acc_a3` as `PENDING`. Otherwise,
   assert both exit 0 and their stdout matches the expected patterns. Failure → gate fails.

10. **Summary.** Print a table of all §0 checks with PASS / FAIL / PENDING. Exit 0 iff all
    checks are PASS (no PENDING counts as passing). Exit 1 otherwise.

Wire `r32_acceptance_gate.sh` into the repo's `gate.sh --strict` after TLA+ and Coq tooling are
confirmed available in the CI environment.

---

## §-Definition of Done

**Per artifact:**
- `R27KillSwitch.tla` — TLC reports "No error has been found" with no invariant violations.
- `R27KillSwitch.cfg` — configures TLC with the correct constants and all five invariants + three
  properties enabled.
- `R27Corrigibility.v` — `coqc` exits 0; `Print Assumptions` for each of the three theorems lists
  no external axioms; no `Admitted` in any proof block.
- `scripts/r32_acceptance_gate.sh` — exits 0 against the above three artifacts; all §0 checks
  report PASS; wired into `gate.sh --strict`.

**Per milestone (R32 complete):**
- All ten §0 acceptance checks are PASS (not PENDING).
- `kill_latch_monotone`, `kill_fires_within_2_polls`, `contained_cannot_disable_kill`, and the
  corollary `kill_once_tripped_job_never_resurfaces` are all compiled to `Qed.` with no axioms
  beyond Coq's kernel.
- TLC reports no invariant violations in the TLA+ model covering all five invariants and three
  temporal properties.
- The cross-check (§7) confirms TLA+ invariant names correspond to Coq theorem names.
- The honest scope statement (§5) is not removed or softened: R32 proves properties of a formal
  *model*, with a clearly stated trusted base (T1–T5). A reader who inspects the gate output and
  reads §5 knows exactly what is and is not proven.

Only then is R32 done.

---

## §-Notes for the implementer (do NOT deviate without updating this spec)

- **Do not add `Admitted` to make the Coq file compile faster.** An `Admitted` goal makes the
  compiled `.vo` file silently unsound — the three theorems would be "proven" only in the sense
  that Coq accepted the file, but the proof is actually a hole. The gate explicitly refuses
  `Admitted`. Prove the theorems, or do not claim to have done so.
- **The `KillFileWrite` guard is load-bearing.** If you relax `path_written ≠ KILL_FILE_PATH` (e.g.,
  to allow "the contained process can write any path the checker approves"), `contained_cannot_disable_kill`
  fails to compile. Do not weaken the guard without updating §4 and re-arguing the OS-level
  invariants. The guard is a *formalization* of the OS argument; changing it is changing the
  argument, not just the proof.
- **The `Fair` hypothesis in `kill_fires_within_2_polls` is not an axiom.** It is a `Variable`
  (or `Hypothesis`) local to the theorem, making the theorem conditional: "if the execution is
  fair, then the kill fires within 2 polls." This is the correct framing — fairness is a property
  of the scheduler, not of the latch itself. Do not collapse it to an axiom or remove it.
- **TLA+ version pinning.** The spec targets TLA+ tools v1.8.0 and TLC's Java backend. A version
  mismatch may change how TLC handles fairness (`WF_vars`) and temporal properties. The
  `tla2tools.jar` download URL in §8 is pinned to v1.8.0. Do not upgrade without re-running
  the full TLC check.
- **Coq version pinning.** The spec targets Coq 8.18.0. The `clos_refl_trans` combinator and
  the `omega` tactic are in scope in 8.18.0; earlier versions use `Omega` (capitalized) and may
  not have `omega` as a primary tactic. If upgrading Coq, audit the proof for tactic name changes.
- **R32 does not modify any Rust code.** If you find yourself editing `latch.rs`,
  `killchan.rs`, or `supervisor.rs`, you are outside R32's scope. R32 verifies the design; R27
  implements it. R32 may reveal implementation bugs (e.g., if the formal model's `KillFileWrite`
  guard cannot be justified by the actual `killchan.rs` implementation), in which case the fix
  goes into R27 (a new commit), not into R32.
- **The trusted base is not negotiable.** T1–T5 are stated honestly because they are the boundary
  of what R32 actually proves. Do not silently absorb T1–T5 into the formal model as
  "obviously true." They are assumptions that the real system must satisfy; an operator or auditor
  reading the spec must see them to understand what they are trusting.
