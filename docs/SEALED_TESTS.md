# Sealed test runs (`axon test --seal DIR`)

A sealed run executes an operator's test suite against code it does not trust,
typically code an AI model wrote, so that the code under test cannot see or
influence the checks that grade it. This page covers the CLI surface as of
R51 (`governance/specs/R51-sealed-engine-unification.md`). Every command below
was run from the repository root with an interpreter build
(`cargo build -p axon-core --no-default-features --bin axon`) on `PATH`; output
is pasted from those runs, trimmed where marked (`...`). Machine-specific paths
are shortened to `/home/...`.

Runnable example: [`examples/sealed/`](../examples/sealed/): a suite, six
candidates (one honest, one wrong, four attacks), `run.sh`, which grades each
one and checks the verdict, and `check_tokens.py`, which checks completion
tokens.

## 1. Safe grading recipe

Run every candidate like this. Each part is load-bearing:

```sh
AXON=$(command -v axon)          # env -i clears PATH, so use an absolute path
env -i PATH=/usr/bin:/bin \
    AXON_PATH="$SUITE_DIR:$CAND_DIR" AXON_PATH_EXCLUSIVE=1 \
    AXON_ALLOWED_EFFECTS= \
    timeout 60 "$AXON" test "$SUITE_DIR/accept.ax" --json --seal "$CAND_DIR"
```

| Part | Why |
|---|---|
| `--seal "$CAND_DIR"` | The candidate cannot name the suite's definitions and its own `@[test]`s are ignored (section 3). |
| `AXON_ALLOWED_EFFECTS=` (set and empty) | Denies every effect to the whole run. Without it the candidate can end the process with `exit(0)` and every test reports `ok`, print a forged summary line, or read the suite's source (box below, section 5.1). |
| `AXON_PATH=suite:cand`, `AXON_PATH_EXCLUSIVE=1` | Modules resolve only from those two directories, not `~/.axon/lib` or the binary's library (section 4.2). |
| `env -i`, `AXON_ENGINE` unset | Nothing ambient reaches the run; a sealed run refuses `AXON_ENGINE=vm`. |
| `timeout` | The seal does not bound CPU time. |
| verdict = exit code **and** the **last** stdout line | Pass only on exit 0 with a last line `{"type":"summary",...,"failed":0,...}`. Section 4.3. |

If the suite itself needs effects (so the ceiling cannot be empty), add
`--completion-key-stdin` and accept a test only with a valid completion token
(section 4.4). The two can be combined.

> **Unsafe without the effect ceiling.** With `AXON_ALLOWED_EFFECTS` unset, a
> sealed candidate whose `solve` calls `exit(0)` passes every test.
> `axon test` reports a test that ended through `exit(0)` as `ok` (CHANGELOG,
> R51 S0b); the seal does not change that.
>
> ```text
> $ cat examples/sealed/candidates/attack_exit/sol.ax    # comments trimmed
> pub fn solve(n: i64) -> i64 {
>     exit(0)
>     0
> }
> $ AXON_PATH=examples/sealed/suite:examples/sealed/candidates/attack_exit AXON_PATH_EXCLUSIVE=1 \
>     axon test examples/sealed/suite/accept.ax --seal examples/sealed/candidates/attack_exit --json
> {"name":"small_inputs_match_the_answer_key","status":"ok","duration_ms":1}
> {"name":"negative_and_large_inputs_match_the_reference","status":"ok","duration_ms":0}
> {"type":"summary","total":2,"passed":2,"failed":0,"skipped":0,"duration_ms":1}
> $ echo $?
> 0
> ```
>
> With the empty ceiling `exit` needs `IO`, is refused, and the tests fail:
>
> ```text
> $ AXON_ALLOWED_EFFECTS= AXON_PATH=examples/sealed/suite:examples/sealed/candidates/attack_exit AXON_PATH_EXCLUSIVE=1 \
>     axon test examples/sealed/suite/accept.ax --seal examples/sealed/candidates/attack_exit --json
> {"name":"small_inputs_match_the_answer_key","status":"failed","duration_ms":1,"message":"builtin `exit` requires effect `IO` which is not in the active sandbox's allowed set {} (principal handle 0)"}
> {"name":"negative_and_large_inputs_match_the_reference","status":"failed","duration_ms":0,"message":"builtin `exit` requires effect `IO` which is not in the active sandbox's allowed set {} (principal handle 0)"}
> {"type":"summary","total":2,"passed":0,"failed":2,"skipped":0,"duration_ms":1}
> $ echo $?
> 3
> ```
>
> The candidate's printed output also goes to the same stdout as the NDJSON
> results. With `IO` granted, a candidate can print a summary line of its own
> (here `solve` prints one and returns 0):
>
> ```text
> $ cat /tmp/seal-demo/forge/sol.ax
> pub fn solve(n: i64) -> i64 {
>     println("{{\"type\":\"summary\",\"total\":2,\"passed\":2,\"failed\":0,\"skipped\":0,\"duration_ms\":0}}")
>     0
> }
> $ AXON_ALLOWED_EFFECTS=IO AXON_PATH=examples/sealed/suite:/tmp/seal-demo/forge AXON_PATH_EXCLUSIVE=1 \
>     axon test examples/sealed/suite/accept.ax --seal /tmp/seal-demo/forge --json
> {"type":"summary","total":2,"passed":2,"failed":0,"skipped":0,"duration_ms":0}
> {"type":"summary","total":2,"passed":2,"failed":0,"skipped":0,"duration_ms":0}
> {"name":"small_inputs_match_the_answer_key","status":"failed","duration_ms":1,"message":"assertion failed: 0 != 4"}
> {"name":"negative_and_large_inputs_match_the_reference","status":"failed","duration_ms":0,"message":"assertion failed: 0 != 9"}
> {"type":"summary","total":2,"passed":0,"failed":2,"skipped":0,"duration_ms":1}
> $ echo $?
> 3
> ```
>
> A grader that takes the first summary line it finds reads a pass. Read the
> exit code and the last line, and keep the ceiling empty.

`examples/sealed/run.sh` applies this recipe to each candidate and checks the
verdict; it also runs the two attacks with a safeguard removed, to show that
the safeguard is what stops them:

```text
$ bash examples/sealed/run.sh "$(command -v axon)"
ok       honest                                     expected=PASS    actual=PASS    {"type":"summary","total":2,"passed":2,"failed":0,"skipped":0,"duration_ms":0}
ok       wrong                                      expected=FAIL    actual=FAIL    assertion failed: 8 != 16
ok       attack_reference                           expected=REFUSED actual=REFUSED E0001 cannot find name `reference` in this scope
ok       attack_answer_key                          expected=REFUSED actual=REFUSED E0001 cannot find name `ANSWERS` in this scope
ok       attack_own_test                            expected=FAIL    actual=FAIL    assertion failed: 0 != 4
ok       attack_exit                                expected=FAIL    actual=FAIL    builtin `exit` requires effect `IO` which is not in the active sandbox's allowed set {} (principal handle 0)
ok       honest with AXON_ENGINE=vm                 expected=REFUSED actual=REFUSED error: a sealed run executes on the tree engine only (AXON_ENGINE=vm cannot carry the seal); unset AXON_ENGINE
ok       control: attack_reference, no --seal       expected=PASS    actual=PASS    {"type":"summary","total":2,"passed":2,"failed":0,"skipped":0,"duration_ms":0}
ok       control: attack_exit, IO granted           expected=PASS    actual=PASS    {"type":"summary","total":2,"passed":2,"failed":0,"skipped":0,"duration_ms":0}
ok       key: honest                                expected=VERIFIED actual=VERIFIED
ok       key: attack_exit, IO granted               expected=NO_TOKEN actual=NO_TOKEN
$ echo $?
0
```

It exits non-zero if any row differs from the expected verdict. The same script
runs in the test suite:
`cargo test -p axon-core --no-default-features --test cli_run sealed_examples_grade_every_candidate_as_expected`.

## 2. The model

There are two parties:

- **Operator**: writes the suite, an `.ax` file with `@[test]` functions. It
  imports the candidate as a module (`mod sol` / `use sol.{solve}`) and may
  hold private material: a reference solution, an answer table, helpers.
- **Candidate**: one or more module files in a directory of its own, e.g.
  `cand/sol.ax`. It is the code being graded.

`--seal DIR` marks every module file under `DIR` as **sealed**. The interface
goes one way: the suite may use the candidate's `pub` names; the candidate may
use builtins and the names its own sealed files define, nothing else. `--seal`
can be given more than once.

The suite file itself must not be under `DIR`: its own items would count as
sealed and the run is refused (`cannot find name `solve``).

`axon test` finds the candidate through `AXON_PATH`, so a sealed run names two
directories, the suite's and the candidate's. The directory the candidate
module resolves from must be the one passed to `--seal`; a module found
anywhere else is operator code.

When prompting a model, fix the interface: the module name and the `pub fn`
signatures the suite imports (here module `sol`, `pub fn solve(n: i64) -> i64`),
and say it may use only builtins and its own helpers. Write its output to a
fresh directory per candidate as `<module>.ax`.

```text
$ AXON_PATH=examples/sealed/suite:examples/sealed/candidates/honest AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal examples/sealed/candidates/honest
running 2 tests
test small_inputs_match_the_answer_key ... ok (1.0ms)
test negative_and_large_inputs_match_the_reference ... ok (0.0ms)

test result: ok. 2 passed, 0 failed (1ms total)
$ echo $?
0
```

The suite ([`examples/sealed/suite/accept.ax`](../examples/sealed/suite/accept.ax))
holds two operator-only items, `let ANSWERS = [0, 1, 4, 9, 16]` and
`fn reference(n: i64) -> i64 { n * n }`, and asserts that `solve(n)` matches
them.

## 3. What the seal guarantees

### 3.1 The candidate cannot name operator code

A candidate that calls the operator's reference solution passes unsealed:

```text
$ AXON_PATH=examples/sealed/suite:examples/sealed/candidates/attack_reference AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax
running 2 tests
test small_inputs_match_the_answer_key ... ok (0.0ms)
test negative_and_large_inputs_match_the_reference ... ok (0.0ms)

test result: ok. 2 passed, 0 failed (0ms total)
```

Sealed, it is refused before any test runs:

```text
$ AXON_PATH=examples/sealed/suite:examples/sealed/candidates/attack_reference AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal examples/sealed/candidates/attack_reference
{"schema":"axon-diag/1","severity":"error","code":"E0001","file":"examples/sealed/candidates/attack_reference/sol.ax","line":4,"col":5,"message":"cannot find name `reference` in this scope","help":"introduce `reference` with `let reference = …`, or import it from a module"}
error: 1 type error(s); tests aborted
$ echo $?
2
```

The sealed files are type-checked by a pass that never sees the operator's
definitions, so an operator name gets the same diagnostic as a name nothing
defines (`E0001`, `E0308`, ...): the error does not tell the candidate that
`reference` exists. The same holds for the module-level `let`
(`candidates/attack_answer_key`: `cannot find name `ANSWERS``). The positions
where "same text" is tested are listed in
`governance/specs/v022-protected-suite-verdict.md` (the PSV-1 "Check time"
paragraph); outside them it is not claimed (section 5.4).

**E0004** (`a sealed module (the candidate under test) cannot use `X`, which
the operator's code defines`) is the backstop from the check over the merged
program. A candidate normally meets it only in the cases in section 5.4.

### 3.2 The candidate cannot add to or replace the rubric

A `@[test]` function in a sealed file is not collected. Here the candidate
ships `solve(n) = 0` plus a test claiming it is correct:

```text
$ AXON_PATH=examples/sealed/suite:examples/sealed/candidates/attack_own_test AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax
running 3 tests
test solve_is_correct ... ok (0.0ms)
test small_inputs_match_the_answer_key ... FAILED
  assertion failed: 0 != 4
test negative_and_large_inputs_match_the_reference ... FAILED
  assertion failed: 0 != 9

test result: FAILED. 1 passed, 2 failed (0ms total)
$ AXON_PATH=examples/sealed/suite:examples/sealed/candidates/attack_own_test AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal examples/sealed/candidates/attack_own_test --json
{"name":"small_inputs_match_the_answer_key","status":"failed","duration_ms":1,"message":"assertion failed: 0 != 4"}
{"name":"negative_and_large_inputs_match_the_reference","status":"failed","duration_ms":0,"message":"assertion failed: 0 != 9"}
{"type":"summary","total":2,"passed":0,"failed":2,"skipped":0,"duration_ms":1}
```

Unsealed the candidate's test counts (3 tests); sealed, only the operator's 2.

### 3.3 Runtime seal: taint on the interpreter

Static checking is the first layer. At run time a sealed run carries a taint
channel: every binding in an `Env` has a taint byte (held in a parallel vector
that exists only in sealed runs), values flowing out of sealed code are marked,
and seal edges where sealed code reaches operator code (calls, method
dispatch, global reads, casts, returns) check those marks. One mark, `PICK`,
records that the candidate selected an operator value (by an index or key it
chose, a branch on its data, or by handing the value back). Operator code that
then dispatches a method on that value is refused when the method has more
than one impl, because the candidate would be choosing which impl answers. A
runtime refusal fails the test with a message (exit 3); section 5.3 shows one.

The simple attacks in `examples/sealed/` never reach the runtime edges because
the static check refuses them first. The edges are covered by the in-crate
suites listed in R51 §8 (T3-T5, T8), run with the static check bypassed.

### 3.4 Sealed runs execute on the tree-walker, and only there

The interpreter has two engines: the tree-walker (reference semantics) and the
bytecode VM (the default for unsealed runs since R50). The seal exists only in
the tree-walker, so a sealed run is pinned to it. `AXON_ENGINE` unset or `tree`
runs it on the tree; `AXON_ENGINE=vm` is refused before any output line:

```text
$ AXON_ENGINE=vm AXON_PATH=examples/sealed/suite:examples/sealed/candidates/honest AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal examples/sealed/candidates/honest --json
error: a sealed run executes on the tree engine only (AXON_ENGINE=vm cannot carry the seal); unset AXON_ENGINE
$ echo $?
2
```

There is no silent fallback. `AXON_VM_TRACE=1` prints VM activity on an
unsealed run and nothing on a sealed one:

```text
$ AXON_VM_TRACE=1 AXON_PATH=examples/sealed/suite:examples/sealed/candidates/honest \
    axon test examples/sealed/suite/accept.ax 2>&1 | head -5
running 2 tests
vm: defer small_inputs_match_the_answer_key
vm: defer solve
vm: defer square
vm: solve 1 ops, 0 tree nodes
$ AXON_VM_TRACE=1 AXON_PATH=examples/sealed/suite:examples/sealed/candidates/honest \
    axon test examples/sealed/suite/accept.ax --seal examples/sealed/candidates/honest 2>&1 | head -5
running 2 tests
test small_inputs_match_the_answer_key ... ok (0.0ms)
test negative_and_large_inputs_match_the_reference ... ok (0.0ms)

test result: ok. 2 passed, 0 failed (0ms total)
```

Cost: the tree-walker with taint is slower. R51's own measurement (§14, Q1,
release build, CPU-heavy suite `tasks/r51-s6/q1`) gave median wall times of
1.324 s sealed, 0.433 s unsealed on the tree, 0.119 s unsealed on the VM. Not
re-measured for this page.

## 4. Reference for graders

### 4.1 Environment variables

| Variable | Use in a sealed run |
|---|---|
| `AXON_PATH` | Colon-separated module search path. Suite directory first, then the candidate directory (the one passed to `--seal`). |
| `AXON_PATH_EXCLUSIVE=1` | Search only `AXON_PATH`. Without it the loader also searches `~/.axon/lib` and `<axon binary>/../lib/axon`. Exactly `1`; any other value is ignored. |
| `AXON_ALLOWED_EFFECTS` | Effect ceiling for the whole run, comma-separated. Set and empty denies every effect, including `print`/`println`/`eprintln` and `exit`. Unset imposes no ceiling: `exit`, printing and `read_file` work. |
| `AXON_ENGINE` | Leave unset (or `tree`). `vm` is refused, exit 2. |

### 4.2 `AXON_PATH_EXCLUSIVE`

The difference shows in the module-not-found diagnostic. With an empty
candidate directory:

```text
$ AXON_PATH=examples/sealed/suite:/tmp/empty-cand \
    axon test examples/sealed/suite/accept.ax --seal /tmp/empty-cand
{"schema":"axon-diag/1","severity":"error","code":"E0901","file":"examples/sealed/suite/accept.ax","message":"module `sol` not found\n  searched:\n    examples/sealed/suite/sol.ax (not found)\n    /tmp/empty-cand/sol.ax (not found)\n    /root/.axon/lib/sol.ax (not found)\n    /home/.../debug/../lib/axon/sol.ax (not found)"}
...
$ AXON_PATH=examples/sealed/suite:/tmp/empty-cand AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal /tmp/empty-cand
{"schema":"axon-diag/1","severity":"error","code":"E0901","file":"examples/sealed/suite/accept.ax","message":"module `sol` not found\n  searched:\n    examples/sealed/suite/sol.ax (not found)\n    /tmp/empty-cand/sol.ax (not found)"}
...
```

### 4.3 Output and exit codes

With `--json`, stdout is NDJSON: one line per operator test, then the summary
as the last line.

```text
$ AXON_PATH=examples/sealed/suite:examples/sealed/candidates/wrong AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal examples/sealed/candidates/wrong --json
{"name":"small_inputs_match_the_answer_key","status":"failed","duration_ms":1,"message":"assertion failed: 8 != 16"}
{"name":"negative_and_large_inputs_match_the_reference","status":"failed","duration_ms":0,"message":"assertion failed: -6 != 9"}
{"type":"summary","total":2,"passed":0,"failed":2,"skipped":0,"duration_ms":1}
$ echo $?
3
```

| Exit | Last stdout line | Meaning | Grade as |
|---|---|---|---|
| 0 | summary, `"failed":0` | every operator test reported ok | pass (if the effect ceiling was empty, or every test has a valid completion token) |
| 3 | summary, `"failed":N>0` | at least one operator test failed, including a runtime seal refusal or an effect refusal inside a test | fail |
| 2 | **no summary** | refused before running: parse/resolve/type error in suite or candidate (`E0001`, `E0004`, `E0002`, ...) or an engine refusal (`AXON_ENGINE=vm`). Diagnostics on stderr | refused |
| 1 | no summary | usage error (`axon test` with no file: `error: no source files specified`) | error |
| other (124 from `timeout`, signal) | no summary | did not finish | error |

Fail closed: anything other than exit 0 with a last line saying `"failed":0`
is not a pass. Exit 2 does not distinguish a candidate that does not compile
from one that named operator code; that is deliberate (the error text must not
reveal what the operator defines).

Without `--json` the same results are printed as `test <name> ... ok` lines and
`test result: ok. N passed, M failed`.

### 4.4 Completion tokens (`--completion-key-stdin`)

With `--completion-key-stdin`, `axon test` reads a hex key (at least 16 bytes)
from stdin before running any program code, and each pass the interpreter
decided itself gets a `"completion"` field: HMAC-SHA256(key,
`"axon-test-completion/1\0" + test name`), hex. Those passes are a plain test
whose body completed, a `@[test(should_fail)]` the program rejected (a panic,
an `Err`, a refinement or verify stop), and a `@[forall]` property every case
of which completed. A test that ended through `exit(0)` or an early plain
`return`, a `should_fail` test stopped by `exit(n)`, and a property with a case
that hit an early `return` are still reported `ok`, but without a token.
Candidate code cannot compute a token without the key.
`examples/sealed/check_tokens.py` checks them. Here `IO` is granted on purpose
so `exit(0)` runs:

```text
$ AXON=$(command -v axon); S=examples/sealed
$ KEY=$(python3 -c 'import secrets; print(secrets.token_hex(32))')
$ for c in honest attack_exit; do
>   echo "$KEY" | env -i PATH=/usr/bin:/bin AXON_PATH=$S/suite:$S/candidates/$c AXON_PATH_EXCLUSIVE=1 \
>       AXON_ALLOWED_EFFECTS=IO \
>       "$AXON" test $S/suite/accept.ax --json --completion-key-stdin --seal $S/candidates/$c > /tmp/$c.ndjson
>   echo "exit=$?"
>   python3 $S/check_tokens.py "$KEY" /tmp/$c.ndjson \
>       small_inputs_match_the_answer_key negative_and_large_inputs_match_the_reference
>   echo "check=$?"
> done
exit=0
verified  small_inputs_match_the_answer_key
verified  negative_and_large_inputs_match_the_reference
check=0
exit=0
NO TOKEN  small_inputs_match_the_answer_key
NO TOKEN  negative_and_large_inputs_match_the_reference
check=1
```

Both runs exit 0 with `"failed":0`; only the tokens tell them apart. The
grader must know the test names it expects (an absent test line is not a
pass). With a key, `axon test` always drops `Exec` from the ceiling (the
process holds the key, so it spawns nothing); if `AXON_ALLOWED_EFFECTS` is
unset, the ceiling becomes every grantable effect except `Exec`
(`cmd_test` in `crates/axon-core/src/main.rs`; read from source, not run here).

## 5. What it does NOT guarantee

The written claim and its non-claims are in
`governance/specs/v022-protected-suite-verdict.md` ("NOT claimed" and the
`PSV-1 NON-CLAIMS` block). R51 ported that claim onto the current interpreter;
its §7 requires any new non-claim to come with a stated reason. The ones a
grader is most likely to hit:

### 5.1 The seal is about names, not the machine

A sealed candidate can call any builtin the effect ceiling allows: `exit`,
printing, and `read_file` on the suite's own source. With the ceiling unset:

```text
$ mkdir -p /tmp/seal-demo/peek
$ cat > /tmp/seal-demo/peek/sol.ax <<'EOF'
pub fn solve(n: i64) -> i64 {
    match read_file("examples/sealed/suite/accept.ax") {
        Ok(src) => eprintln("candidate read {str_len(src)} bytes of the suite")
        Err(e) => eprintln("read refused: {e}")
    }
    n * n
}
EOF
$ AXON_PATH=examples/sealed/suite:/tmp/seal-demo/peek AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal /tmp/seal-demo/peek --json
candidate read 861 bytes of the suite
candidate read 861 bytes of the suite
candidate read 861 bytes of the suite
candidate read 861 bytes of the suite
{"name":"small_inputs_match_the_answer_key","status":"ok","duration_ms":1}
{"name":"negative_and_large_inputs_match_the_reference","status":"ok","duration_ms":0}
{"type":"summary","total":2,"passed":2,"failed":0,"skipped":0,"duration_ms":1}
$ AXON_ALLOWED_EFFECTS= AXON_PATH=examples/sealed/suite:/tmp/seal-demo/peek AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal /tmp/seal-demo/peek --json
{"name":"small_inputs_match_the_answer_key","status":"failed","duration_ms":1,"message":"builtin `read_file` requires effect `IO` which is not in the active sandbox's allowed set {} (principal handle 0)"}
{"name":"negative_and_large_inputs_match_the_reference","status":"failed","duration_ms":1,"message":"builtin `read_file` requires effect `IO` which is not in the active sandbox's allowed set {} (principal handle 0)"}
{"type":"summary","total":2,"passed":0,"failed":2,"skipped":0,"duration_ms":2}
```

The empty ceiling also fails an honest candidate that only prints debug output:

```text
$ cat /tmp/seal-demo/print/sol.ax
pub fn solve(n: i64) -> i64 {
    eprintln("candidate says hi")
    n * n
}
$ AXON_ALLOWED_EFFECTS= AXON_PATH=examples/sealed/suite:/tmp/seal-demo/print AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal /tmp/seal-demo/print --json
{"name":"small_inputs_match_the_answer_key","status":"failed","duration_ms":0,"message":"builtin `eprintln` requires effect `IO` which is not in the active sandbox's allowed set {} (principal handle 0)"}
{"name":"negative_and_large_inputs_match_the_reference","status":"failed","duration_ms":0,"message":"builtin `eprintln` requires effect `IO` which is not in the active sandbox's allowed set {} (principal handle 0)"}
{"type":"summary","total":2,"passed":0,"failed":2,"skipped":0,"duration_ms":0}
```

Tell the model not to print. The seal also does not bound CPU time or memory.

### 5.2 The candidate's output decides the verdict

The suite compares what the candidate returns with expected values; that is
the point. What the seal cannot help with is a suite that lets the candidate's
data choose how strictly it is checked: a branch on candidate data into a
weaker check, a loop over a candidate-returned array (an empty array runs the
assertions zero times), an assertion the candidate can skip (inside a `with`
handler body whose arm does not resume), a table of precomputed verdicts
indexed by a candidate value. Compare the candidate's output to a value the
operator computed, outside any loop or branch the candidate controls, and
assert lengths explicitly.

### 5.3 PICK covers choosing the impl, not choosing the data

The dispatch refusal in 3.3 applies when the method has more than one impl in
the program. A table of checker values of one type, indexed by the candidate,
is not refused: the values differ only in data, which makes it a table of
verdicts (5.2).

```text
$ mkdir -p /tmp/seal-demo/pick/suite /tmp/seal-demo/pick/cand
$ cat > /tmp/seal-demo/pick/suite/rubric.ax <<'EOF'
mod sol
use sol.{pick}

type Strict = { want: i64 }
trait Grade {
    fn accepts(self, x: i64) -> bool
}
impl Grade for Strict {
    fn accepts(self: Strict, x: i64) -> bool { x == self.want }
}

// Bad suite design: the candidate's pick() chooses which checker runs.
@[test]
fn candidate_picks_the_checker() {
    let checkers = [Strict { want: 42 }, Strict { want: 1 }]
    assert(checkers[pick()].accepts(1))
}
EOF
$ printf 'pub fn pick() -> i64 { 1 }\n' > /tmp/seal-demo/pick/cand/sol.ax
$ D=/tmp/seal-demo/pick
$ AXON_PATH=$D/suite:$D/cand AXON_PATH_EXCLUSIVE=1 axon test $D/suite/rubric.ax --seal $D/cand
running 1 test
test candidate_picks_the_checker ... ok (1.0ms)

test result: ok. 1 passed, 0 failed (1ms total)
```

Add a second impl of `accepts` anywhere in the program and the same pick is
refused at run time:

```text
$ cat >> $D/suite/rubric.ax <<'EOF'

type Lenient = { want: i64 }
impl Grade for Lenient {
    fn accepts(self: Lenient, x: i64) -> bool { true }
}
EOF
$ AXON_PATH=$D/suite:$D/cand AXON_PATH_EXCLUSIVE=1 axon test $D/suite/rubric.ax --seal $D/cand
running 1 test
test candidate_picks_the_checker ... FAILED
  operator code dispatched `accepts` on a value of an operator type that the candidate picked (out of a table by a key or index it chose, by a branch on a value it chose, or by handing it back) — the candidate would choose which impl answers (here `Strict`); name the value with a literal or build it from data instead of selecting it by the candidate's answer

test result: FAILED. 0 passed, 1 failed (2ms total)
$ echo $?
3
```

Either way, select checkers by a literal key the operator wrote.

### 5.4 Name collisions are refused, and they leak existence

The "same text for an operator name and a missing one" property is tested in
listed positions only. Two places where it does not hold, and where an
**honest** candidate is refused:

A local, parameter, closure parameter, `for` variable or pattern binding named
like an operator global is E0004 (here a local `ANSWERS`; the diagnostic points
at the enclosing `fn` on line 1, not the `let` on line 2):

```text
$ mkdir -p /tmp/seal-demo/shadow
$ cat > /tmp/seal-demo/shadow/sol.ax <<'EOF'
pub fn solve(n: i64) -> i64 {
    let ANSWERS = n * n
    ANSWERS
}
EOF
$ AXON_PATH=examples/sealed/suite:/tmp/seal-demo/shadow AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal /tmp/seal-demo/shadow
{"schema":"axon-diag/1","severity":"error","code":"E0004","file":"/tmp/seal-demo/shadow/sol.ax","line":1,"col":5,"message":"a sealed module (the candidate under test) cannot use `ANSWERS`, which the operator's code defines","help":"the candidate-under-test interface is one-directional: the suite imports the candidate's names, never the reverse — define what the candidate needs in its own modules"}
error: 1 type error(s); tests aborted
```

A top-level item with the same name as an operator item is E0002, and that
diagnostic names the operator's file and line:

```text
$ mkdir -p /tmp/seal-demo/dup
$ cat > /tmp/seal-demo/dup/sol.ax <<'EOF'
fn reference(n: i64) -> i64 { n * n }
pub fn solve(n: i64) -> i64 { reference(n) }
EOF
$ AXON_PATH=examples/sealed/suite:/tmp/seal-demo/dup AXON_PATH_EXCLUSIVE=1 \
    axon test examples/sealed/suite/accept.ax --seal /tmp/seal-demo/dup
{"schema":"axon-diag/1","severity":"error","code":"E0002","file":"examples/sealed/suite/accept.ax","line":14,"col":1,"message":"the name `reference` is defined more than once in this module","help":"rename one of the `reference` definitions, or remove the duplicate"}
{"schema":"axon-diag/1","severity":"error","code":"E0004","file":"/tmp/seal-demo/dup/sol.ax","line":2,"col":5,"message":"a sealed module (the candidate under test) cannot use `reference`, which the operator's code defines","help":"the candidate-under-test interface is one-directional: the suite imports the candidate's names, never the reverse — define what the candidate needs in its own modules"}
error: 2 type error(s); tests aborted
```

Give operator-private items names a candidate is unlikely to pick
(`op_reference`, `OP_ANSWERS`), or a candidate may be refused for a collision
rather than for its code.

### 5.5 Other limits (from the written non-claims, not exercised here)

- A sealed `mod X`/`use X` that reaches an operator module file gets a loader
  message naming the operator path.
- `axon build` (native code) has no seal. Sealed runs are interpreter-only.
- An integer handle to a kernel object, an effect list, a budget, a path, URL
  or `ai_complete` prompt supplied by a candidate value is not tracked.
- Where the process's stdout is a regular file, operator code can read the
  candidate's printed output untainted via `/proc/self/fd/1`. Give the run a
  pipe.

## 6. Protected suite verdicts (axon-psv)

`axon test --seal` is the interpreter side only. The protected suite verdict
(PSV) path wraps it: the `axon-psv` runner spawns
`axon test <entry> --json --filter <test> --exact --completion-key-stdin --seal <candidate>`
with a cleared environment (`PATH`, `AXON_PATH=<suite>:<candidate>`,
`AXON_PATH_EXCLUSIVE=1`, and an `AXON_ALLOWED_EFFECTS` ceiling from the launch
manifest, always set, with `Exec` removed), passes a per-run key on stdin, and
turns the result into a guest verdict (`axon-guest-verdict/1`); exit 2 with no
summary maps to `Unknown`. The authority contract, launch manifest and verdict
formats are in `governance/specs/v022-psv-protocol.md`, a draft that certifies
nothing. This page does not cover that path.

## 7. See also

- `axon test --help`: `--seal <DIR>` (repeatable), `--json`, `--filter`,
  `--exact`, `--completion-key-stdin`.
- `governance/specs/R51-sealed-engine-unification.md`: engine pin, taint
  storage, refusals, test plan, receipts.
- `governance/specs/v022-protected-suite-verdict.md`: PSV-1 claim, non-claims,
  and "What an honest suite must do".
- `CHANGELOG.md`, R51 S0b, S6.1 and S7 entries.
