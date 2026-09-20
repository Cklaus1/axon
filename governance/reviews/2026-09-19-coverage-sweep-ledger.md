# Coverage sweep ledger — pinned baseline fccc4bc

Surfaces with NO test / example / harness that executes them (15, measured
against `BUILTINS` in `crates/axon-core/src/builtins.rs`, excluding mentions
in `builtins.rs` itself and in generated `AXON_REFERENCE.md`).

One domain per worktree. No two agents share a checkout.

| Agent | Branch | Assigned surfaces |
|---|---|---|
| numeric | `sweep/numeric` | `volatile_load_u16`, `volatile_load_u32`, `volatile_load_u64`, `bpf_get_smp_processor_id`, `bpf_ktime_get_ns` |
| kernel | `sweep/kernel` | `principal_spend`, `scheduler_done_count`, `supervisor_restarts` |
| json | `sweep/json` | `json_arr_f64`, `json_arr_from_f64`, `json_get_i64`, `json_path_json` |
| uncertain | `sweep/uncertain` | `uncertain_confidence`, `uncertain_deterministic`, `uncertain_dyn_f64` |
| audit | `sweep/audit` | none — audits the COVERAGE METRIC itself (see below) |

The audit agent exists because "named in a test file" is not "executed by a
test". The 15 above are the surfaces named NOWHERE executable; a builtin
named in a comment, a doc string, or a `BUILTINS`-adjacent table reads as
covered under the same query and may be exercised by nothing. That gap is
the same absent-vs-passed collapse this sweep is looking for, one level up.

## Outcome

All 15 assigned surfaces now have semantic coverage. Four of five agents
reported; the coverage-metric audit is still running.

| Surface | Verdict |
|---|---|
| `volatile_load_u16/u32/u64` | verified-sound — interp deliberately E0910-refuses (raw MMIO cannot be emulated), native lowers inline; no differential exists, so the native half checks width/endianness/zero-extension against itself at a real readable address |
| `bpf_get_smp_processor_id`, `bpf_ktime_get_ns` | verified-sound — the table's helper IDs were unit-tested, what the BACKEND emits was not; now asserted at the encoded-instruction level with a no-helper control |
| `principal_spend` | verified-sound — readout and both enforcers (`principal_authorize`, the LLM gateway) agree; that agreement was the named risk |
| `scheduler_done_count` | verified-sound — snapshot, not cumulative; `done + failed` accounts for every spawned fiber |
| `supervisor_restarts` | verified-sound — per-supervisor, counted on the production `supervisor_run` path, cross-checked against an independent counter |
| `json_arr_f64`, `json_arr_from_f64` | verified-sound — round trip is bit-exact over 8 values incl. -0.0, 1e300, 1e-300; non-finite emits `null`, never invalid JSON |
| `json_get_i64` | **DEFECT, fixed** — an integer too large for i64 was reported as "not an integer" |
| `json_path_json` | verified-sound — five distinct failure shapes; empty path is the empty KEY, leaf is re-serialised (both now pinned) |
| `uncertain_deterministic` | verified-sound — confidence 1.0, tag 0; does not survive combination (min-propagation), as documented |
| `uncertain_confidence` | verified-sound as code, doc corrected — it is a no-op, and claimed to record a confidence on a surrounding value that does not exist |
| `uncertain_dyn_f64` | **DEFECT, fixed** — interp/native divergence on `source_tag` (0 vs 2), an I-2 violation |

## Defects found, and what they have in common

Three, plus one the orchestrator took from an agent's adjacent-findings list:

1. `json_get_i64` — out-of-range integer mislabelled "not an integer"
2. `json_path_i64` — the SAME mechanism at a second site, collapsing three
   distinct failures into one message
3. `uncertain_dyn_f64.source_tag` — the interpreter reported a runtime-sourced
   value as user-constructed, while native reported it correctly. The fail-OPEN
   direction, on a provenance field, in the reference engine
4. `json_keys` doc claimed document order; the parser returns sorted

None is a crash. Every one is a WRONG STATEMENT ABOUT A RESULT — the same
class the last several sweeps have landed in, and the reason "it has no
coverage" was worth asking about at all.

Two were invisible to existing tests for a structural reason worth recording:
the `uncertain` bug is hidden by its own covered siblings (the non-`dyn`
constructors agree at 0 in both engines), and the `json_keys` claim is
untestable with alphabetical fixture keys, which is what the fixture had.

## Open, with an owner needed — NOT fixed

Found while probing, outside the finder's assigned domain, left alone
deliberately rather than folded into an unrelated commit:

* **A `@[bpf]` `if`-as-expression silently returns 0.** `fn f(ctx) -> i64 {
  if ctx > 0 { 7 } else { 3 } }` builds clean and emits `r0 = 0; exit` — not 7,
  not 3, no diagnostic. `codegen/bpf.rs` `lower_if` discards both branch values
  by design (a documented Slice-1 limitation) and the tail substitutes zero.
  A documented limitation that produces a silent wrong answer should be an
  E2301 refusal instead. This is the most serious open item here.
* **`ai_extract_uncertain_*` has the same `source_tag` class** as the fixed
  `uncertain_dyn_*`: codegen stamps 1, interp stamps 0. NOT demonstrable
  differentially — reading the tag out of `Result<Uncertain<i64>,_>` is
  E0910-refused natively — so it is asserted here as a code reading, not a
  measurement.

## Backend invariant established by this episode

> **No object file may be emitted from LLVM IR that has not passed LLVM module
> verification.**

Stated independently of the `if`-expression bug that exposed it. It is a
backend admission gate, not a regression workaround — the same shape as the
artifact admission gate: a transition (IR becomes a shipped artifact) that
nothing asserted was valid.

Measured state after the fix — every object-emitting entry point in
`codegen/output.rs` verifies first:

| entry point | verifies |
|---|---|
| `compile_to_binary` | yes, via `compile_to_binary_target` |
| `compile_to_binary_target` | yes |
| `compile_to_shared_lib` | yes |
| `compile_to_object_for_triple` | yes |
| `compile_to_freestanding_obj` | yes |
| `compile_to_freestanding_binary` | yes |
| `compile_to_wasm_object` | yes |
| `emit_bpf_object` | **yes — added here; it was the only hole** |

Still unverified, recorded not fixed: `write_ir`, `emit_bitcode`,
`emit_llvm_ir`. These emit IR/bitcode rather than objects. Bitcode is consumed
by other tools, so `emit_bitcode` is the one of the three that most deserves
the same gate.

## Reusable audit heuristic (the highest-yield thing this episode produced)

> **Search for documented "limitations" that COMPILE SUCCESSFULLY and
> substitute a default, zero, or null value. Prioritise those above explicit
> refusals.**

A refusal is visible: the build fails, the author learns, nothing ships. An
accepted program that returns a fabricated value is invisible by construction,
and the documentation makes it look considered. The BPF `if` had a spec
citation, a comment explaining the choice, and a clean build — and returned
neither branch.

The probe shape, in order of signal:

1. grep the backends for comments containing "Slice-1", "for now", "not yet",
   "yields no value", "unsupported" — near code that does NOT return an error
2. of those, keep the ones where the surrounding code still produces an
   artifact (an object, a value, an exit 0)
3. of those, keep the ones that substitute a literal — `const_zero()`,
   `unwrap_or_default()`, `Ok(None)` flowing into a default, `null`
4. write the smallest program that reaches it and check whether the answer is
   fabricated

Steps 1-3 are mechanical and could be a gate. Contrast with the inverse
question ("which features are unsupported?"), which surfaces the refusals —
the cases that are already safe.

### Second heuristic, from the mutation pass

> **A defensive branch that no input can reach is worse than no branch.** It
> tells every later reader — and every model trained on the file — that an edge
> case is handled, when the control flow makes it unreachable.

Found here as `then_open && else_open`: it looked like careful handling of a
terminating branch, and no program could produce that state, because
`Expr::Return` yields `None` and `Expr::Block` breaks on a terminator. A
mutation that deletes such a branch SURVIVES, which is the detection signal —
a survived mutation is a question about the code before it is a question about
the test.

Both are candidates for Cortex skill crystallisation: they are strategies about
where to look, not facts about Axon, so they should transfer to other compilers
and runtimes.

## The heuristic's first independent find — OPEN, not fixed

Running the probe above (steps 1-3, mechanically) over `codegen/`, `interp/`
and `axon-rt` produced 5 candidates. Four were placeholders on an
already-failing build, which is sound. The fifth is a silent wrong answer in
the MAIN native backend.

**`codegen/mod.rs`, the `None if !matches!(ret_sem, Type::Unit)` arm:**
"No value from body but function has non-void return type: emit a zero value
of the appropriate type to keep IR valid."

Reachability was MEASURED, not argued. Instrumented that branch and built all
328 `.ax` programs under `examples/` and `tests/fixtures/`:

| outcome | count |
|---|---|
| never reached | 295 |
| reached on a build that then ABORTED (sound placeholder) | 31 |
| **reached on a build that SUCCEEDED** | **2** |

The two are `examples/asi/search_rank.ax` (`score_clean`, `redteam`) and
`crates/axon-core/tests/fixtures/ai_extract_uncertain.ax` (`confident_count`,
whose `Result<i64,str>` zero is tag 0 = `Err` with an empty message).

Minimal repro, isolated to one construct:

```
fn pick(s: str) -> i64 {
    match ai_extract_uncertain_i64(s) {
        Ok(u) => { if u.value > 0 { 7 } else { 3 } }
        Err(_) => 5
    }
}
```
`AXON_AI_MOCK=1` — interp `7`, native `0`, build clean, no diagnostic.

Controls that make the mechanism exact:
* the same match with NO field read (`Ok(_) => 7`) agrees: interp 7, native 7
* `u.source_tag` instead of `u.value` also fabricates (interp 3, native 0)

So it is reading ANY field off an `Uncertain<T>` bound in a `Result` match arm
that makes the whole function return a fabricated value. **0 is outside
`pick`'s range** — its arms are 7, 3 and 5 — which is as clean a proof that the
value is invented as this class allows.

This also corrects a claim from the sweep: the `uncertain` agent reported that
reading `source_tag` out of a `Result<Uncertain<i64>,_>` is E0910-REFUSED
natively. It is not. It builds clean and fabricates. The agent inferred the
refusal rather than observing the built binary's output.

### Mechanism, isolated

`emit_field_access` takes the `Uncertain`/`Temporal` GEP path only when
`sem_type_of_expr(receiver)` says the receiver IS one. A binding introduced by
a match arm (`Ok(u) =>`) does not carry its payload type into codegen, so `u`
is not recognised, the specialised path is skipped, the generic struct path
fails, and the enclosing body lowers to nothing.

The binding's ORIGIN is the whole difference, measured:

| shape | interp | native |
|---|---|---|
| `let u = uncertain_new(5, 0.9)` then `u.value` | 7 | **7** |
| `match ai_extract_uncertain_i64(s) { Ok(u) => … u.value … }` | 7 | **0** |
| same match, no field read (`Ok(_) => 7`) | 7 | **7** |

So there are two separable fixes, and they are not alternatives:

* **the safety net** — a non-Unit body that lowers to nothing must RECORD an
  E0910 rather than fabricate. This closes the class for every future construct,
  not just this one, and is the small change.
* **the feature** — propagate a match-arm binding's payload type into codegen
  so the field read lowers correctly. This is the real repair and is larger.

Doing only the feature would leave the next unlowerable construct fabricating
a value silently, so the safety net is the one that must not be skipped.

Regression baseline for BOTH is captured (328 programs: 166 exit-0, 161 exit-1,
1 exit-2). After the safety net, exactly the 2 named programs may flip 0 -> 1,
and nothing else may move — diffed in both directions, since a program that
starts PASSING unexpectedly is as much a signal as one that starts failing.

Fix approach (deliberately NOT applied yet — the strict gate has not run on the
current batch): the zero-emission is legitimate ONLY as a placeholder after a
codegen error has been recorded. Record an E0910 naming the function when a
non-Unit body lowers to nothing, so the general case becomes a refusal instead
of an invented value. Regression check must confirm the 2 silent programs now
refuse AND that none of the 295 previously-clean programs starts failing.

## Second wave — four lanes, pinned at 1b56faa

| Lane | Branch | Owns | Must NOT touch |
|---|---|---|---|
| A | `sweep/audit` | the executed-coverage audit (running since wave 1) | the 15 assigned builtins |
| B | `sweep/safety-net` | the backend refusal invariant | `emit_field_access`, `sem_type_of_expr`, match-arm binding code |
| C | `sweep/typeprop` | match-arm payload-type propagation | the `None if !matches!(ret_sem, Type::Unit)` arm in `codegen/mod.rs` |
| D | `sweep/regression` | the measurement harness | anything under `crates/axon-core/src/` |

B and C are the two halves of one repair and are deliberately NOT one commit:
the safety property must be reviewable on its own, and it holds even if the
feature fix later misses another path.

### The invariant B implements

> **A successful native build may never invent a return value solely because
> codegen failed to produce one.**

Enforced independently of any specific type or construct, exactly as the BPF
module-verification gate is enforced independently of the `if` bug that
exposed it. Same shape both times: a transition nothing asserted was intended.

### Merge order (dependencies, not completion order)

    A (audit result) -> B (safety net) -> D (328-program diff)
      -> C (feature fix) -> differential validation -> strict gate

The two-stage evidence trail this produces is the point:

| stage | `pick()` native behaviour |
|---|---|
| before | silent wrong answer — prints 0, a value outside its range |
| after B | explicit refusal — build fails with a diagnostic naming the fn |
| after C | correct execution — prints 7, agreeing with the interpreter |

Collapsing B and C into one commit would destroy that trail: the "explicit
refusal" row would never exist in history, and the safety property could not
be reviewed apart from the feature that happens to make it unnecessary here.

### Two corrections to the finding as first reported

**1. The shipped example does NOT visibly diverge under `AXON_AI_MOCK`.**
I reported `examples/asi/search_rank.ax` as affected in a way that implied
observable wrongness. Both engines print `Best observed score: 0 / 100`. The
functions DO fabricate — that part stands — but the program's printed output
coincides, by arithmetic accident:

    expected_top()          = 0
    mock ai_extract value   = 1
    score_clean returns f64_to_i64(u.confidence*100) only when value == expected_top

so the interpreter also returns 0, for a legitimate reason. The agreement is a
coincidence of the mock's value, not evidence of soundness. A model whose
extraction matched `expected_top()` would make the interpreter return 90 and
the native binary still return 0.

This matters beyond pedantry: it is why the defect survived a corpus with a
parity suite. The one shipped program that exercises the broken path is also
the one whose output the mock happens to make agree.

The function-level divergence is proven separately and minimally:

    fn extracted() -> i64 {
        match ai_extract_uncertain_i64("rank the docs") {
            Ok(u) => u.value
            Err(_) => 0 - 1
        }
    }

    AXON_AI_MOCK=1 -> interp 1, native 0, build clean, no diagnostic.

**2. The backend is INCONSISTENT within this family, and that sharpens the
case for the safety net.** Not every shape fabricates — some already refuse:

| shape | native |
|---|---|
| `Ok(u) => u.value` | builds clean, returns a fabricated 0 |
| `Ok(u) => f64_to_i64(u.confidence * 100.0)` | **E0910 refusal**, build aborts |

So the refusal machinery exists, works, and is reached for one shape while the
neighbouring shape silently invents a value. That is not a missing feature with
a consistent boundary; it is a boundary with a hole in it. Agent B's invariant
is what makes the boundary total, independently of which shapes agent C later
teaches the backend to lower.

Method note: the probe that produced "native printed nothing" for one of these
was BROKEN — the build had aborted and the binary did not exist, so the run
was exit 127. Checking the exit code rather than trusting empty stdout caught
it. An empty result and a missing artifact look identical until you ask.

### Scope correction: this is NOT an Uncertain/AI defect

It is a core-language defect that happens to have been found through an AI
type. A plain user struct in a `Result`, field read in a match arm:

    type P = { x: i64, y: i64 }
    fn mk(b: bool) -> Result<P, str> { if b { Ok(P { x: 4, y: 9 }) } else { Err("no") } }
    fn f() -> i64 { match mk(true) { Ok(p) => p.y  Err(_) => 0 - 1 } }

`axon run` 9, native **0**, build clean, no diagnostic, no AI anywhere. Any
ordinary program matching a `Result<Struct, E>` and reading a field is exposed.

Boundary, measured — the mechanism is narrower and more specific than
"match-arm bindings are broken":

| shape | native | verdict |
|---|---|---|
| scalar payload, `Ok(n) => n * 2` | agrees | fine — scalars carry their type |
| struct payload passed WHOLE to a fn, `Ok(p) => take(p)` | agrees | fine — the binding IS a usable value |
| **struct payload, FIELD read, `Ok(p) => p.y`** | **0** | **fabricates** |
| **struct in an OPTION payload, `Some(p) => p.y`** | **0** | **fabricates** |
| **two field reads, let-bound Result then matched** | **0** | **fabricates** |
| `Uncertain<i64>` payload, `Ok(u) => u.value` | **0** | **fabricates** |
| `Uncertain<f64>` payload, `Ok(u) => u.confidence` | **0** | **fabricates** |
| `let p = P {…}` then `p.y` (control) | agrees | fine — let-bindings carry their type |

So it is FIELD ACCESS on a match-arm-bound struct-shaped payload, in both
`Result` and `Option`. The binding lowers fine as a whole value; only the
field access needs the semantic type it does not have.

That the corpus sweep found only 2 programs is therefore not reassurance — the
corpus simply contains few programs of this shape, not few programs at risk.

### Sourcing the "unsupported neighbour" row

The three-row proof needs a shape that is silent now, refuses after B, and
STILL refuses after C — otherwise C could widen the feature surface while
quietly reopening the floor.

That shape should NOT be guessed now. The honest source is B's own corpus
diff: whichever programs B flips to refusal and C does not subsequently fix
are by construction the neighbours. Picking one in advance risks choosing a
shape C happens to cover, which would prove nothing.

`f64_to_i64(u.confidence * 100.0)` is a weaker but already-available candidate:
it refuses TODAY, so it demonstrates "C did not weaken an existing refusal",
though not "B converted a silent case".

### Method note — a comparison harness that agreed on two failures

While mapping the above, one probe reported "agree" for a program whose
interpreter run had exited 2 on a type error. Comparing stdout alone made two
FAILING runs agree trivially. The harness now rejects a run whose output
carries a diagnostic before comparing. Same family as the exit-127 slip
earlier: absence of output is not a result.

### Why B must land first — the fabrication is currently MASKED, not rare

Static count: **12** programs in the corpus contain the at-risk shape (a match
arm binding a payload, whose body reads a field off that binding), 17 sites.
Only 2 were measured as silently fabricating. The other 10 are not safe — they
are MASKED:

| program | build | masked by |
|---|---|---|
| `examples/asi/search_rank.ax` | **exit 0** | nothing — fabricates |
| `tests/fixtures/ai_extract_uncertain.ax` | **exit 0** | nothing — fabricates |
| `examples/asi/classify.ax` | exit 1 | unrelated E0910 (+E0701) |
| `examples/asi/taint_flow.ax` | exit 1 | unrelated E0910 |
| `examples/asi/code_review.ax` | exit 1 | unrelated E0910 (+E0701) |
| `examples/asi/pricing.ax` | exit 1 | unrelated E0910 |
| `examples/asi/summarize.ax` | exit 1 | unrelated E0910 |
| `tests/fixtures/ai_extract_generic.ax` | exit 1 | unrelated E0910 |
| `tests/fixtures/verify_ai_source.ax` | exit 1 | unrelated E0910 |
| `tests/fixtures/phase55_mixed_comprehensive.ax` | exit 1 | unrelated E0910 (+E0701) |

Nine programs traverse the broken path and are saved only by tripping a
DIFFERENT unsupported construct first. That has a consequence for sequencing
that is easy to get backwards:

> **Teaching the backend more constructs makes this defect WORSE before it
> makes it better.** Every E0910 that future feature work removes unmasks a
> program that then builds clean and fabricates.

So the safety net is not merely "nice to have first". Landing feature work
without it actively converts refusals into silent wrong answers. This is the
measured form of the argument for B -> C, and it generalises past this defect:
in a backend that refuses what it cannot lower, a fabrication hole is masked
in proportion to how INCOMPLETE the backend is, and is unmasked by progress.

It also revises the severity reading. "2 of 328 programs" invited the
conclusion that exposure is negligible. The correct statement is that 12 of 328
are exposed and 9 are one feature-commit away from being silently wrong.
## The instrument for the two pending fixes — `scripts/build_outcome_regression.sh`

Both pending fixes make a claim of the shape "exactly these programs move, and
nothing else does". That claim is only checkable against a baseline trusted in
BOTH directions, so the harness is symmetric by construction: it diffs sorted
observed-vs-baseline and classifies every differing row, rather than looping
over one side and looking the other up (the way a one-way check gets written by
accident — it cannot see a row the baseline lacks).

```
scripts/build_outcome_regression.sh              # sweep + differential (~1 min, JOBS=6)
scripts/build_outcome_regression.sh --self-test  # prove the comparators FAIL when they should
scripts/build_outcome_regression.sh --record     # re-baseline, as a deliberate act
```

Committed baselines: `scripts/build_outcome_baseline.tsv` (328 rows: exit
status + path) and `scripts/build_differential_baseline.tsv` (6 declared
interpreter↔native cases). The handed-over 328-program baseline REPRODUCED
exactly — 166 exit-0, 161 exit-1, 1 exit-2, no membership drift.

Pre-fix differential state, measured (stdout, never exit codes — Axon remaps
2..=15 and 101 onto 1, so a status comparison passes on a wrong value):

| case | state |
|---|---|
| `crates/axon-core/tests/fixtures/ai_extract_uncertain.ax` | **diverge** — interp prints `1`, native prints nothing (the fabricated `Result` zero is tag 0 = `Err("")`, so the `Err` arm runs); both exit 0 |
| `examples/asi/search_rank.ax` | **agree** — both stop at the same `@[verify]` failure in `deploy_gate` and print identical stdout, exit 3 both |
| synthetic match-arm field read (the isolated repro) | **diverge** — interp `7`, native `0` |
| synthetic match-arm, NO field read (control) | agree `7` |
| synthetic `let`-bound field read (control) | agree `7` |
| synthetic `sandbox_run` (control) | `native-refused-e0910` — a reported outcome, NOT a pass and NOT a skip |

The `search_rank` row is the one to read carefully: it is one of the two
programs that reach the fabricating branch on a SUCCEEDING build, and the
stdout differential still calls it `agree`. Its row is a change-detector, not a
certificate — a harness is blind wherever the fabricated value does not reach
stdout.

### The class, restated after further isolation

It is not "match-arm bindings". It is:

> Codegen loses the semantic type of a binding introduced by a **match arm** or
> a **closure parameter**. Field access on such a binding fails to lower, and
> the enclosing function body's value is then FABRICATED. `let` bindings and
> named function parameters carry their types correctly.

Closure parameters, isolated by three controls so the boundary is exact:

| shape | native | reading |
|---|---|---|
| closure, SCALAR param — `\|v: i64\| v * 2` | agrees | not "closures are broken" |
| NAMED fn, STRUCT param — `fn take(p: P) -> p.y` | agrees | not "struct params are broken" |
| **closure, STRUCT param, field read — `\|p: P\| p.y`** | **0** | the intersection is the defect |

Also fabricating: nested `Result<Result<P,str>,str>` matched twice then
field-read (interp 9, native 0, clean build).

Two binding kinds lose their type, and both are the kinds introduced by
PATTERN BINDING rather than by declaration. A `let` states its type to the
compiler; a match arm and a closure parameter receive theirs from an enclosing
inference, and that inference is what does not survive into codegen.

This has a direct consequence for the proof matrix: the "unsupported
neighbour" row does not need to be invented. If lane C's fix covers match arms
but not closure parameters, the closure case IS the neighbour — silent today,
refusing after B, still refusing after C. C has been told this explicitly and
asked NOT to expand scope to chase it, only to state clearly which it covers.
An unstated answer is the failure mode here: a green corpus would then be
indistinguishable between "fixed" and "still fabricating and nobody looked".

## Lane C verified (held for merge behind B)

Mechanism confirmed by the agent's own instrumentation and matching the
prediction: `emit_pattern_bindings` wrote `self.locals` but never
`self.local_types`, which is what `sem_type_of_expr(Ident)` reads. The receiver
typed as `None`, no field-access path applied, the body lowered to nothing, and
the non-Unit return path fabricated a zero.

Independently checked by the orchestrator before queueing:

* **Scope** — `codegen/mod.rs` is touched, but NOT the `None if !matches!(ret_sem,
  Type::Unit)` arm. Those 9 lines are an `Expr::Question` case in
  `infer_expr_sem_type`: `let r = f()?` is the SAME binding-type loss in
  implicit form, and is legitimately C's half.
* **Generality** — the string `ai_extract_uncertain` appears **zero** times in
  the source diff. The fix keys on pattern shape (Result/Option/tuple/enum
  variant), not on a builtin name.
* **Corpus, cross-validated by lane D's harness** rather than by C's own
  measurement: 7 programs flipped 1 -> 0, **zero** flipped 0 -> 1, and the named
  set matches C's report exactly. Two lanes that never communicated agree.
* **The floor-prover survives.** A closure struct parameter still fabricates
  after C (interp 8, native 0). C was told not to expand scope to chase it and
  did not. That is the "unsupported neighbour" row, obtained empirically rather
  than guessed.

### A native refusal was masking an interpreter defect

One of the seven, `tests/fixtures/ai_extract_generic.ax`, now builds natively
and prints 5 lines — while the INTERPRETER panics on it, exit 101, *"value of
type ai_extract is not callable"*. Reproduced on main's pre-C binary, so it is
pre-existing: `ai_extract::<T>` turbofish is unimplemented in the interpreter.

Worth stating as its own lesson. The reference engine was broken for this
construct and nothing noticed, because the program ran NOWHERE: the native side
refused it with E0910 and the fixture's own header says "Parse + type-check
only — does NOT execute". A refusal on one engine can hide a defect on the
other, and a fixture that only type-checks hides both. Removing a refusal is
therefore not purely additive — it can surface latent breakage elsewhere, which
is an argument for landing feature work with the measurement harness already in
place, not after.

## The completed two-stage proof

Measured end to end, one shape per row, the same program at each stage:

| shape | before | after safety net | after type propagation |
|---|---|---|---|
| **known valid** — struct in a `Result`, field read in a match arm | interp 9, native **0**, build CLEAN | **E0910 refusal** naming `f` | interp 9, native **9** |
| **unsupported neighbour** — closure with a struct param, field read | interp 8, native **0**, build CLEAN | **E0910 refusal** naming `__lambda_0` | **still refused** |

The second row is the one that had to be earned rather than asserted: it shows
the feature surface widened without the safety floor dropping. Without it,
"the repro now passes" is indistinguishable from "the repro now passes and
three neighbours silently fabricate".

### The neighbour row exposed a hole in the safety net itself

The first attempt at row 2 FAILED. After the named-function safety net landed,
the closure shape still built clean and still printed 0 — because
`emit_lambda` carries its OWN `None => w_ret(const_zero())` in
`codegen/expr.rs`, a file the safety-net lane was explicitly forbidden to touch
so it would not collide with the type-propagation work running in parallel.

So the partitioning that kept two agents from corrupting each other also
guaranteed the invariant would be implemented at one of its two sites. That is
a real cost of parallel isolation and worth naming: a file-scoped exclusion is
a bet that the invariant lives in one file, and nobody checked that bet.

It was caught only by putting the neighbour shape in front of the gate. Reading
the diff would not have shown it — the diff was correct, complete, and covered
every site the lane was allowed to see.

> **A gate is not verified until something it should reject has been shown to
> be rejected.** The same rule as mutation testing, applied to an admission
> gate rather than a test.

### Final corpus state

328 programs: **166/161/1 -> 173/154/1**. Seven moved 1 -> 0, zero moved 0 -> 1,
each of the seven differentially checked against the interpreter rather than
accepted because the build stopped failing. Two differential cases moved
diverge -> agree, exactly the transitions recorded as predictions before either
fix existed.

## The pattern, with every instance found today

One defect class accounts for nearly everything this sweep found:

> **A success signal produced by machinery that never performed the
> underlying check.**

Not "a check that failed silently" — a check that never ran, reporting the
same value it would report on success. The instances, with how each was
detected:

| instance | what reported success | detected by |
|---|---|---|
| native codegen fabricated a return value | clean build, exit 0, no diagnostic | heuristic: documented limitation + literal substitution |
| ...same, in a closure body | clean build | feeding the gate a case it should reject |
| ...same, as an infinite `while let` | clean build | re-running the heuristic after the first two |
| `@[bpf]` `if` returned neither branch | clean build, valid object | zero-coverage sweep |
| BPF objects emitted without IR verification | valid-looking `.o` | asking what the emit path asserts |
| `wasm_aot_stdout_parity` compared nothing | exit 0, "0 match, 0 differ" | coverage audit, demonstrated with `AXON=/bin/false` |
| `tee_sim_run` called a build failure a skip | exit 0 | reading the skip paths |
| two harnesses invoked by nothing | never ran at all | grep for callers |
| `eprint` / `gaussian_sample` "covered" | static query scored them covered | panic-on-call mutation |
| 4 property tests ran nowhere — and FAILED | nothing reported anything | asking what executes them |
| a background gate "exit code 0" | harness notification | reading the log, not the notification |

Eleven instances, one class. The through-line for detection is also singular:
**every one was found by asking what ACTUALLY executed, and none by reading
code or diffs.** The diffs were correct. The code looked right. The reports
said fine.

### The three detectors that did all the work

1. **Mutation** — break the thing, confirm something notices. Finds tests that
   do not test and coverage that does not cover.
2. **Feed the gate a case it must reject** — mutation's rule applied to an
   admission gate rather than a test. Found the closure hole that a correct,
   complete diff could not reveal.
3. **Instrument and sweep the corpus** — put an eprintln on the suspect branch,
   build everything, count. Turns "is this reachable?" from an argument into a
   measurement. Used for the fabricated return (2 of 328 live), the `while let`
   hang (0 of 328, reported as unverified rather than claimed), and the masked
   programs (9 of 12 saved only by an unrelated refusal).

### The counterexample worth keeping

`dict_parity.sh` printed SKIP and its Rust wrapper refused it:

    harness `dict_parity.sh` FAILED (exit Some(1)) — a failure is never a
    skip, whatever it printed

Someone had already been burned by exactly this and built the guard. That is
what the fix looks like when it is done properly: not a repaired instance, but
a wrapper that cannot be lied to.

## Applying the skills, round 2 — what they found

The three skills were crystallised and then immediately run against the repo.

### `feed-the-gate-a-rejectable-case` / the invoked-by-nothing probe

Five pieces of PASSING verification were running nowhere, found in two rounds:

| script | what it verifies | status |
|---|---|---|
| `ebpf_verify.sh` | Axon→eBPF object ACCEPTED by the in-kernel verifier | wired |
| `tee_sim_run.sh` | TEE baseline + type rule | wired |
| `r33_acceptance_gate.sh` | R33 §0 checks | wired |
| `r34_acceptance_gate.sh` | stamp → verify → tamper → BROKEN chain, via the real CLI | wired |
| `r39_slice1_gate.sh` | R39 Slice 1 §0 checks | wired |

Plus `scripts/r34_orig_tmp.sh`: the PRE-FIX copy of the R34 gate, committed as
a backup by the very change that strengthened it. A weaker version of a gate
outlived the fix that removed its weakness. Deleted, and the admission gate now
rejects editing-artifact names generally.

### `documented-limitation-fabricates-value` extended to the other 19 crates

The original sweep only covered `axon-core` and `axon-rt`. Across the remaining
19 crates: **one** candidate, and it is security-relevant.

`crates/axon-guest-kernel/src/enforce.rs` — the guest kernel's effect bitmask:

    /// Default 0xFF matches the open-policy mmds.rs stub so the kernel boots
    /// even if K2 hasn't filled in a real policy yet.
    static mut ALLOWED_EFFECTS: u64 = 0xFF;

A security default chosen for convenience during bring-up and then left. Its
sibling module had ALREADY learned this exact lesson and written it down (T48,
`mmds.rs`): a policy that fails to decode "used to fall through and leave
ALLOWED_EFFECTS at the static default, which was 0xFF. Refuse explicitly and
say so, rather than relying on the static happening to be right." That fix set
a closed policy on the mmds side and left this static open — so two halves of
one concept disagreed about which way to fail.

**Not reachable today, and that is stated rather than glossed:** `enforce::init`
is the only caller that sets the SCE bit enabling SYSCALL, so if it never runs
the handler never runs and the value is never read. The coupling between
"enable the gate" and "install the policy" is the entire safety argument, and
it is one refactor away from not holding.

Changed to deny-all on the same reasoning as the `while let` fallback: zero
behavioural change while the coupling holds (init overwrites it before enabling
the gate), and the difference between a closed door and an open one if it stops
holding.

**STATUS: uncommitted pending the QEMU boot test.** Building in the main repo
while the strict gate runs in another worktree previously broke that gate's
`dict_parity` harness, so the verification is deferred rather than skipped.

## CORRECTION: the orphan probe was broken, and under-reported

The "five pieces of passing verification running nowhere" figure above is
WRONG — too low. The probe that produced it had a systematic blind spot.

It excluded a script's self-reference with `grep -v "^./$s"`, but `grep -rl`
emits paths WITHOUT the `./` prefix. So the exclusion never matched, and any
script whose own header comment names its own filename counted as "referenced
by something" and was silently classified as wired.

`r33`/`r34`/`r39_slice1` surfaced only because their headers happen not to name
themselves. That is the entire reason those three were found and others were
not — a property of comment style, not of whether anything invokes them.

Corrected probe, with `*_parity.sh` excluded because `parity_all.sh` invokes
those by GLOB rather than by name (a second false-positive class, in the
opposite direction):

**16 scripts invoked by nothing executable.** Gate-shaped, i.e. things that
assert a requirement:

    axon_kernel_gate.sh      gfx_wgpu_render_gate.sh   kernel_enforce_test.sh
    r23_acceptance_gate.sh   r30_acceptance_gate.sh    r39_slice3_gate.sh
    r39_slice4_gate.sh       r39_slice5_gate.sh        zephyr_qemu_gate.sh

The rest are demos, benchmarks, probes and one-off measurement tools
(`axon_session_demo.sh`, `perf_bench.sh`, `r1_build_measure.sh`,
`wasm_aot_link_probe.sh`, `browser_webgpu_clear.sh`, `fix_codec.sh`), plus
`build_outcome_regression.sh`, which is deliberately manual and says so.

### The one that matters most

`governance/REQUIREMENTS.md` line 67 records:

    R23 | Proof certificates | ✅ Landed 100% (re-verified 2026-07-18)
       | `scripts/r23_acceptance_gate.sh` PASS

The requirements register cites that gate as the EVIDENCE for a 100%-landed
claim. Nothing executable invokes it. It is referenced only by documentation —
README, three specs, two review documents, and the requirements register that
leans on it.

It is also the only thing in the repo that runs
`cargo test -p axon-certcheck --features smt`, so R23's A5 check
(`acc_a5_certificate_byte_identical`) runs nowhere either. The gate's own smt
stage is `-p axon-core` only.

That is the purest instance of this session's class: **documentation citing
verification as evidence, where the verification never executes.** Not a
failing check — a check whose result is quoted without the check having run.

### Lesson, and it is about my own work

A probe that under-reports produces a number that looks like a finding and is
actually a floor. I published "five" and it was wrong. Both false-positive
directions were present at once: self-reference wrongly counted as a caller,
and glob invocation wrongly counted as no caller. Neither is visible from the
probe's output — only from checking a case whose answer is known
independently, which is how this surfaced (`r23` was known-orphaned from
reading, then found to be absent from the probe's list).

STATUS: the nine gate-shaped orphans are NOT yet verified to pass, and are
NOT yet wired. Several need GPU, QEMU or Zephyr toolchains. Wiring an unproven
gate would be the same error in a new direction.

## The generalisation: which requirement claims rest on gates nobody runs?

Cross-referencing every `scripts/*.sh` cited as EVIDENCE in
`governance/REQUIREMENTS.md` against whether anything executable invokes it.
37 scripts are cited. Accounting for `parity_all.sh`'s glob invocation:

| status | count |
|---|---|
| wired (invoked by a gate, test or aggregator) | 18 |
| glob-invoked by `parity_all.sh` | 12 |
| **invoked by nothing** | **7** |

The seven: `r23_acceptance_gate.sh`, `r30_acceptance_gate.sh`,
`r39_slice3_gate.sh`, `r39_slice4_gate.sh`, `r39_slice5_gate.sh`,
`zephyr_qemu_gate.sh`, `perf_bench.sh`.

Three MORE were in that state until today — `r33`, `r34`, `r39_slice1` — so the
figure before this session's wiring was **ten of thirty-seven**.

That is the finding in its most useful form. The requirements register is the
document that answers "is this done?", its evidence column names a gate, and
for roughly a quarter of the gates it names, nothing runs them. The claim and
the check are both real; the link between them is not.

Two of the seven have a defensible reason and should be labelled rather than
wired: `perf_bench.sh` is a benchmark whose evidence is a recorded measurement,
and `zephyr_qemu_gate.sh` needs a hardware-ish toolchain. The remaining five
are ordinary acceptance gates with every required tool present on this host.

### Why this is worth more than the individual fixes

Every defect this sweep found was a local instance of "a success signal with no
check behind it". This is the same shape at the level of the PROJECT'S OWN
RECORD OF ITSELF. A reader — human or model — consulting REQUIREMENTS.md to
learn what is verified gets an answer that is true about the gate's existence
and silent about whether it has ever run.

The cheap structural fix is not to wire seven scripts. It is to make the
evidence column checkable: a test that reads REQUIREMENTS.md, extracts every
cited `scripts/*.sh`, and fails when one is invoked by nothing — the same
two-directional drift gate the env registry and `AXON_REFERENCE.md` already
have, applied to the register that cites them. That is proposed, not built.

## The invariant, in its final form

Three attempts, each defeated by a false-positive class the next one fixed:

1. *"the gate exists"* — defeated by gates nothing invokes (7 found)
2. *"the gate is NAMED somewhere executable"* — defeated by self-reference, by
   mentions inside comments, and by `[ -f … ]` existence checks. This is what
   my probe checked, and it under-reported by more than half.
3. **the form that survives:**

> **Every requirement row citing executable evidence must have a live,
> TRANSITIVE path from a real execution root to that evidence — unless the row
> explicitly declares a different evidence mode, whose witness obligation is
> itself checked.**

"Transitive" is the word doing the work. Six of the fifteen violations are
gates reachable only through `axon_safety_gate.sh`, which is itself reachable
only from the orphaned `r30_acceptance_gate.sh`. A one-hop check calls all six
wired. They are not: the whole subtree hangs off nothing.

## Merge rule for a checker that reports true violations

> **Do not land a checker while it correctly reports unresolved violations,
> unless those violations are simultaneously resolved or explicitly
> reclassified with evidence.**

The reasoning matters more than the rule. The 15 violations are not a
regression — they are pre-existing drift that the project has only just become
able to SEE. Landing the checker alone would turn `main` red for the crime of
acquiring sight, and the pressure would then be to weaken the checker rather
than fix the drift. That is how a gate gets `#[ignore]`d and joins the set of
checks that exist and never run.

The lane was explicitly told not to reclassify `perf_bench.sh` or
`zephyr_qemu_gate.sh` to get green, and did not. Their classes carry checked
witness obligations — a `recorded_benchmark` needs a repo file that exists AND
names the script; an `external_hardware` gate must contain a real skip guard for
the absent tool. Labels that are checked are evidence; labels that are asserted
are decoration.

## Resolution order — roots before leaves, recompute between

1. wire any ROOT-ish orphan that legitimately activates several downstream
   gates (`axon_safety_gate.sh` covers r27/r28/r29);
2. RECOMPUTE the invocation graph;
3. only then touch leaves that remain unreachable;
4. leave the two special classes alone until their witness obligations hold;
5. merge the checker when the remaining set is zero, or is entirely
   explicitly-supported non-continuous evidence.

Step 2 is not bookkeeping. Without it we would "fix" five leaves independently
that all become reachable the moment one upstream gate is wired — five commits
of motion for one commit of work, and four of them wrong.

## What the checker must SAY, not just decide

A flat list of violations is nearly useless for acting in dependency order. Each
one needs its provenance in the terms the graph already computes:

    R31 -> orphaned
      reason: only mention is a COMMENT in scripts/r33_acceptance_gate.sh:6
    R27 -> manual_operator
      reason: reachable only via axon_safety_gate.sh, itself reachable only
              from r30_acceptance_gate.sh (orphaned); shortest root path: NONE

and, for anything reachable, the actual path from the root. Requested from the
lane while it still holds the graph.
