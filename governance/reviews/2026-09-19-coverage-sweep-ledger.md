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
