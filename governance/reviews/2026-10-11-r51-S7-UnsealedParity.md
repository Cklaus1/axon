# S7 review - R51 unsealed parity (UnsealedParity)

## Trees
- head: `/home/cklaus/projects/axon-lanes/r51-s7` @ 3921a309
- base: 5864c423 (temporary worktree `/var/tmp/r51-tmp/s7-UnsealedParity/base`, removed after the review)
- reference: v0.22 `/home/cklaus/projects/axon-lanes/v022-ref` @ c9ed2ddd (source only, not built)

## Method
- Release binaries, both `cargo build --release -p axon-core --no-default-features --bin axon`:
  `/var/tmp/r51-tmp/s7-UnsealedParity/axon-head` (3921a309) and `.../axon-base` (5864c423).
- Corpus sweep (`sweep/one.sh`): all 399 `examples/**/*.ax` and `crates/axon-core/tests/fixtures/**/*.ax`
  (42 of them in examples/asi). Each file ran under `axon check`, `axon run` with AXON_ENGINE=tree, and `axon run`
  on the default engine (VM); files with `@[test]` also ran `axon test --json` on tree and on VM. Base and head used the same
  cwd per file, AXON_SEED=42, AXON_AI_MOCK=1 and AXON_CLOCK=0:1. stdout, stderr and rc were compared byte for byte after
  durations were normalised.
- Spec examples: the 3 runnable ```axon blocks in spec/axon-for-llms.md, both trees, both engines.
- Hunk review: interp.rs, eval.rs, builtins.rs, goal.rs, proptest.rs, sym.rs, value.rs, env.rs, resolver.rs, checker.rs and
  main.rs. Four read-only scouts reviewed these. Every candidate they raised was then probed by me on both binaries;
  the outputs quoted below are from those runs.
- Depth: deep recursion probes (direct, mutual, `&mut`, closure, `?`) at the default limit and at AXON_MAX_DEPTH=200000;
  nesting probes (parens and blocks at 3990, a 2000/8000-arm else-if chain, 8000 nested calls); and nested
  expressions at every level of a 5990-deep recursion. Native only.
- VM refusal: sealed `axon test --seal` (tasks/r51-s6/q1) under unset, `vm`, `tree`, `bogus` and empty AXON_ENGINE,
  read against the engine_for/seal_engine_pin/engine_at_build source; every `self.engine` read checked.
- Perf: `scripts/vm_perf_gate.sh` and `--repros` with AXON_BIN=axon-head on CPU 11, compared with the §10 base values;
  `perf stat -e instructions:u` with AXON_ENGINE=tree on fib-recursive and qsort, base vs head.

Probe sources: `/var/tmp/r51-tmp/s7-UnsealedParity/probes/*.ax`; runner `probes/p.sh <verb> <file>` (base and head ×
tree and vm).

## Summary

| id | sev | where (head) | claim broken | evidence |
|---|---|---|---|---|
| S7-UnsealedParity-1 | MAJOR | interp/eval.rs:2262-2267, 2311-2316, 2360-2365 | §3/§7 unsealed unchanged except listed rows | `break` in a handler arm inside a `while`: base `done` rc 0, head panic rc 101 |
| S7-UnsealedParity-2 | MAJOR | interp/eval.rs:2075, 2330 | §3/§7 | nested `with`: base 107, head 7, both engines, rc 0 |
| S7-UnsealedParity-3 | MAJOR | resolver.rs:1160-1271 | §3/§7 (no resolver row) | 7 programs base ran now fail E0002 rc 2 |
| S7-UnsealedParity-4 | MAJOR | interp/sym.rs:449, interp/builtins.rs:3778, interp/proptest.rs:115-127 | §3/§7 | bare `Uncertain` param 101→0.5; `assert_eq_f64(NaN,…)` 0→101; `@[forall]` exit/refine pass→fail |
| S7-UnsealedParity-5 | MINOR | checker.rs:406, 7272 | §3 checker row lists other checker changes | `u.to_string()` on `()` was a runtime 101, now E0403; undefined `DictFoo` type was accepted, now E0308 |
| S7-UnsealedParity-6 | MINOR | resolver.rs:1252-1266 | consistency with checker.rs:406 (exact names) | `type GoalScore = i64 where …` → "defined more than once", though nothing else defines it |
| S7-UnsealedParity-7 | MINOR | resolver.rs:1163 | diagnostic text | message reads "`impl Show for Named(\"P\")`" (Rust Debug output) |
| S7-UnsealedParity-8 | MINOR | interp.rs:3963 | §3 row 2 wording | `return None` test fails, reported as "the test returned `Err`…"; a tail `None` passes |
| S7-UnsealedParity-9 | MINOR | improve.rs:264 | §3 row 2 applied consistently | `run_test_fn(..).is_ok()` ignores `failure_exit`, so `axon improve` passes a test that `axon test` now fails |
| S7-UnsealedParity-10 | MINOR | interp.rs:1736 (stack sizing) vs tree frames | BUG_HUNT #28 "guard trips before a real overflow" | deepnest260-280, AXON_ENGINE=tree: base rc 0, head rc 134 (stack overflow abort) |
| S7-UnsealedParity-11 | NOTE | parser/resolver/main.rs | - | diagnostic location changes, sum-confusion panic text, JSON control-char escaping: improvements |
| S7-UnsealedParity-12 | NOTE | tasks/r51-baseline.md:53 | §14/§10 receipt | head meets §10 (measured); the ledger's last row against 5864c423 is S2.3; tree engine +1.9-2.6% instructions |
| S7-UnsealedParity-13 | NOTE | interp/vm/mod.rs:124-191, 1346 | §3 VM refusal / default | confirmed: sealed+vm refused rc 2; unsealed default VM unchanged; invalid AXON_ENGINE same text both trees |

No BLOCKER. Corpus sweep (399 files, up to 5 modes each): every base/head difference is either an R51 fixture written
to show a §3 row (`tests/fixtures/r51_s0b/{escape_*,sum_confusion,leaf_sum_confusion,test_early_question,
test_err_return,uncertain_source_tag}.ax`) or `examples/mobile/lifecycle.ax`, which times out on both trees (rc 124, an
event-loop program). Spec doc examples: identical on all 4 cells.

## Findings

### S7-UnsealedParity-1 - MAJOR - `break`/`continue` in an effect-handler arm now panics, even when the arm is lexically inside the loop
- Head `crates/axon-core/src/interp/eval.rs:2262-2267` (likewise 2311-2316 and 2360-2365):
  `contain_loop_control(match bound { Ok(_) => self.eval(arm_body, &mut arm_env), Err(e) => Err(e) }, FrameEdge::HandlerArm)`.
  Base (`run_handler_arm`) did `bound.and_then(|_| self.eval(..))` with no containment. The same code is in v0.22
  (`v022-ref/crates/axon-core/src/interp/eval.rs:1807-1811, 1846-1850`), so this is a v0.22 fix carried over.
- Claim broken: §3 lists "a callee's `break`/`continue` escaping its fn". A handler arm is not a fn, and nothing in §3 or §7 covers this.
- Probe `probes/hbreak.ax`:
  ```
  fn main() {
      let i = 0
      while i < 3 {
          with handler { on IO(p) => { break } } {
              println("hi")
          }
          i = i + 1
      }
      eprintln("done")
  }
  ```
  base tree/vm: rc 0, stderr `done`. head tree/vm: rc 101, `axon: panic: \`break\`/\`continue\` outside a loop in an effect-handler arm`.
  The message is false for this program: the arm sits inside the `while`.
- Smallest fix: add a §3 row (decision: an arm's `break` is refused) and reword the message for an arm, for example
  "`break`/`continue` cannot leave an effect-handler arm".

### S7-UnsealedParity-2 - MAJOR - a handler arm's completion now ends its own `with`, not the nearest one (silent value change)
- Head eval.rs:2330 `Ok(v) => Err(Flow::HandlerDone(Box::new((v, idx))))`; eval.rs:2075
  `Err(Flow::HandlerDone(done)) if done.1 == depth => done.0`. Base: `Err(Flow::HandlerDone(v)) => v`, caught by the nearest `with`.
  vm/tests.rs:1759-1763 records the change ("v0.22 HandlerDone(v, depth), M74 … 107 instead of 7").
- Probe `probes/hdone.ax` (the vm/tests.rs `t_callee` program with `eprintln(to_str(v))`): base tree/vm prints `107`, rc 0;
  head tree/vm prints `7`, rc 0. Same exit code, different output.
- Claim broken: §3/§7 (not listed). Smallest fix: a §3 row.

### S7-UnsealedParity-3 - MAJOR - the resolver now refuses duplicate definitions that base accepted (E0002), unsealed
- Head resolver.rs:1160-1206 (duplicate impl key, duplicate method on a dispatch type, duplicate trait, duplicate module `let`,
  duplicate refinement) and 1209-1271 (a refinement may not reuse a type-level name). `collect_top_level` runs on every
  program. Base resolver treated these items as a no-op or as "last wins". The same code is in v0.22 resolver.rs:991-1266.
- Observed (`axon run`, AXON_ENGINE=tree; the error is static, so the engine does not matter):

  | probe | base | head |
  |---|---|---|
  | r1 two `impl Show for P` | rc 0 `second` | rc 2 E0002 "`impl Show for Named(\"P\")` is defined more than once in this program" |
  | r2 traits A and B both give `P.name` | rc 0 `b` | rc 2 E0002 "the method `name` on `P` (a method call cannot tell two impls apart) …" |
  | r3 `trait J` twice | rc 0 `ok` | rc 2 E0002 "the trait `J` …" |
  | r4 `let N = 1` / `let N = 2` | rc 0 `2` | rc 2 E0002 "the constant `N` …" |
  | r5 `type Pos` refinement twice | rc 0 `0` | rc 2 E0002 "the refinement type `Pos` …" |
  | r6a `type GoalScore = i64 where _ >= 0` | rc 0 `5` | rc 2 E0002 "the type name `GoalScore` (a refinement cannot reuse a type's name) …" |
  | r6b `fn first<T>` + `type T = i64 where _ != 0` | rc 0 `1` | rc 2 E0002 |

  (r2g and r2s, two generic or slice impls, were already E0403 on base, so they gain nothing.)
- Claim broken: §3 has no resolver row; S0Merge-3 recorded only the resolver half of break/continue.
- Smallest fix: a §3 row "resolver: duplicate trait/impl/method/const/refinement and refinement-name reuse are E0002 (v0.22 G01/PCI)".

### S7-UnsealedParity-4 - MAJOR - other unsealed runtime changes outside §3
All observed on both engines, base vs head:
- **Bare `Uncertain`/`Temporal` parameter keeps its wrapper.** Head interp/sym.rs:449 `let soft = super::conform::is_soft_decl(&p.ty);`
  (base: only `Generic { base: "Uncertain"|"Temporal" }`). `probes/bareunc.ax` (`fn conf(u: Uncertain) -> f64 { u.confidence }`):
  base rc 101 `axon: panic: field access on non-struct (i64)`; head rc 0 `0.5`.
- **`assert_eq_f64` fails on NaN.** Head interp/builtins.rs:3778 `if !(a == b || (a - b).abs() <= 1e-9)`. `probes/nan.ax`
  (`assert_eq_f64(0.0 / 0.0, 2.0)`): base rc 0 `passed`; head rc 101 `axon: panic: assertion failed: NaN != 2`.
- **`@[forall]` cases fail on exit and halt flows.** Head interp/proptest.rs:115-127 (base `Err(_) => Ok(())`).
  `probes/forall.ax` + `probes/fexit.ax`, `axon test --json`: base all `ok`, rc 0; head `p_exit` "`exit(0)` ended the property case
  before it completed", `p_refine` "the value bound to `x` (= 0) violates the refinement `Positive`…", `p` (exit(3))
  "exited with code 3", rc 3.
- Claim broken: §3/§7. They are bug fixes (v0.22 M76, PCI 11), but §3 is the list of permitted unsealed changes.
  Smallest fix: §3 rows.

### S7-UnsealedParity-5 - MINOR - checker changes not in the §3 checker row
- checker.rs:7272 `Type::Unit => Some("()")` (method on `()` checked statically). `probes/unitmeth.ax`
  (`let u = log("a")` then `u.to_string()`): base rc 101, prints `a`, then `axon: panic: no method \`to_string\` on type \`()\``;
  head rc 2 E0403 at check, nothing runs. `impl Show for ()` still dispatches (`probes/unitimpl.ax`: `unit`, both trees).
- checker.rs:406/7001/7016 `DEFERRED_PREFIXES.contains(..)` (exact names; base `starts_with`). `probes/dictfoo.ax`
  (`fn never(x: DictFoo)`, not defined anywhere): base rc 0 `ok`; head rc 2 E0308 "unknown type 'DictFoo'".
  No corpus file uses a `Goal*`/`Dict*`/`Uncertain*`/`Temporal*` type name that is not exact (grep of *.ax/*.md).
- Fix: add both to the §3 checker row.

### S7-UnsealedParity-6 - MINOR - the refinement-name rule still uses the deferred PREFIX rule that checker.rs dropped
- resolver.rs:1252-1258: `|| ["Uncertain", "Temporal", "Goal", "Dict"].iter().any(|p| n.starts_with(p))`, with the comment
  "The checker accepts any name STARTING with a deferred prefix (`DictTable`, `GoalX`) as a type". At head that is no
  longer true: checker.rs:406 accepts exact names only (amendment 117). So `type GoalScore = i64 where _ >= 0` (r6a) is refused with
  "the type name `GoalScore` … is defined more than once in this program", although no other definition exists.
  v0.22 resolver.rs:1256 has the same prefix rule next to the same exact-name checker (v0.22 checker.rs:406), so the inconsistency is inherited.
- Fix: replace the prefix test with `["Uncertain","Temporal","Goal","Dict"].contains(&n)` (already covered by
  `BUILTIN_GENERICS`, so the clause can simply be deleted), or keep the rule and give it its own message.

### S7-UnsealedParity-7 - MINOR - E0002 for a duplicate impl prints Rust Debug output
- resolver.rs:1163 `format!("\`impl {} for {:?}\`", b.trait_name, b.for_type)`. Observed: "`impl Show for Named(\"P\")` is defined more than once".
- Fix: `crate::interp::type_name_of(&b.for_type)` (already used two lines below for `tn`), or `axon_type_name`.

### S7-UnsealedParity-8 - MINOR - an `@[test]` that does `return None` fails, with a message about `Err`
- interp.rs:3963 `Ok(Value::None) if !self.test_body_finished.get() => Ok(TestEnd::EndedEarly(TEST_ENDED_ON_FAILURE …))`.
  `probes/tnone.ax`: `fn t_return_none() -> Option<i64> { let k = 0  if k == 0 { return None }  Some(1) }`. Base rc 0 `ok`;
  head tree/vm rc 3 `failed` "the test returned \`Err\` (or a \`?\` ended it on \`Err\`/\`None\`): it did not complete".
  A body whose tail value is `None` still passes (`probes/tsf.ax` `t_none_ret`: `ok`). v0.22 (interp.rs:2999) made every early
  `return` EndedEarly and did not single out `None`. §3 row 2 says "returning `Err` or exiting through `?`".
- Probably intended: `return None` cannot be told apart from a `?` on `None`. Fix: put "or `return None`" in §3 row 2, and say so in the message.

### S7-UnsealedParity-9 - MINOR - `axon improve`'s test-flip check does not use the new test verdict
- improve.rs:264 `let ran_ok = crate::interp::run_test_fn(program, name).is_ok();`, where `run_test_fn` = `run_test_fn_outcome(..).map(|_| ())`
  (interp.rs:3888-3890). That drops `TestEnd::failure_exit()`, which main.rs `cmd_test` now treats as a failure. The result:
  a pass that turns a passing test into an `Err` return or a `?` exit is not reported as a flip, although `axon test` fails it.
  [INFERENCE: read from code; not exercised through `axon improve`.]
- Fix: `matches!(run_test_fn_outcome(program, name), Ok(end) if end.failure_exit().is_none())`, or make `run_test_fn` return
  `Err(why)` on `failure_exit()`.

### S7-UnsealedParity-10 - MINOR - less native stack headroom on the tree engine: success → stack-overflow abort
- `probes/deepnest<k>.ax`: `fn down(n) { if n == 0 { 0 } else { (((…down(n - 1) + 0)…) + 1 } }` with k nested parens, `down(5990)`
  (under the default 6000 limit), AXON_ENGINE=tree:

  | k | base | head |
  |---|---|---|
  | 250 | rc 0 | rc 0 |
  | 260, 270, 275, 280 | rc 0 | rc 134 `thread '<unknown>' has overflowed its stack` / `fatal runtime error: stack overflow, aborting` |
  | 285+ | rc 134 | rc 134 |

  The default VM passes at k = 400 on both trees. `probes/deep6.ax` at AXON_MAX_DEPTH=200000 (tree): max RSS 1,072,368 KB on base
  vs 1,192,308 KB on head, about 11% more stack per level. The abort itself predates R51 (base aborts from k = 285 on). BUG_HUNT #28
  says the guard trips before a real overflow, and that already fails when frames hold deeply nested expressions. R51 lowers the
  threshold by about 10%. Every plain recursion probe (direct, mutual, `&mut`, closure, `?`, 6000 deep and 200000 deep) gives the same rc 101
  "recursion limit exceeded" message on both trees and both engines.
- Fix: raise `STACK_BYTES_PER_FRAME` (interp.rs:1736) or `MIN_STACK_BYTES` (interp.rs:1740) by ≥12% to get base's headroom back,
  or record the reduced headroom as accepted.

### S7-UnsealedParity-11 - NOTE - diagnostic and text changes that are improvements
- Top-level `let` initialiser diagnostics now point at the `let` (`probes/r8.ax`: base E0001 line 2 col 5, head line 5 col 1).
- Unexpected EOF is now located (`probes/eof.ax`: base line 1 col 1, head line 2 col 17).
- A `-> Result` fn returning `None`: same rc 101, new text "`solve` is declared to return Result but produced None — a runtime type
  confusion" (base "no match arm matched"); fixture r51_s0b/sum_confusion.ax.
- `axon test --json` failed-message escaping now covers control characters (main.rs cmd_test), where base wrote a raw newline into the JSON line.

### S7-UnsealedParity-12 - NOTE - perf: §10 holds at head, but the ledger does not show it
- Measured on head 3921a309 (`AXON_BIN=axon-head VM_PERF_CPU=11 ./scripts/vm_perf_gate.sh` and `--repros`, release, instructions:u medians of 3; both gates print PASS) against the §10 base values:
  fib-recursive 2,291,906,051 (§10 2,298,930,456, 0.997×), collatz 18,366,337,822 (18,580,211,600), mandelbrot 5,067,021,468
  (5,068,914,361), arr-sum 4,879,409,766 (4,879,384,240, 1.000005×), qsort 11,353,799,850 (11,401,042,655); repros loop 92.2 (92.2),
  call 232.1 (244.1), part 217.1 (218.1), fold 58.0 (58.0), fastcall 232.1 (244.1), mutcall 272.2 (274.2), foldmod 95.0 (95.0),
  swap 257.3 (258.3). All ≤ 1.01×.
- Ledger: `vm_perf_gate.sh`'s PASS is against the CPython budgets, not §10's 1.01 × 5864c423. The S6 T7 row
  (tasks/r51-baseline.md:53) compares r51/s6rows with r51/int, two R51 commits, so it says nothing about §10. The last row against
  5864c423 is S2.3 d7fc0031 (line 28). /var/tmp/r51-tmp/t2-perf.log has no commit id. Fix: add the head row above to the ledger / §14.
- No budget: the unsealed tree engine (AXON_ENGINE=tree) costs more: fib-recursive 9,924,617,831 → 10,178,399,977 (+2.56%), qsort
  76,049,273,664 → 77,518,147,428 (+1.93%).

### S7-UnsealedParity-13 - NOTE - VM refusal is total; unsealed engine selection unchanged
- tasks/r51-s6/q1 under `--seal`: unset → 4/4 passed rc 0; `AXON_ENGINE=vm` → rc 2 "error: a sealed run executes on the tree engine
  only (AXON_ENGINE=vm cannot carry the seal); unset AXON_ENGINE"; `tree` → passed; `bogus` and `` → rc 2 "AXON_ENGINE must be …".
- Unsealed `AXON_ENGINE=bogus` / empty: rc 2 with the same text on base and head.
- Every VM entry point is gated by `seal_pin_vm()` (`self.engine == Engine::Vm`, vm/mod.rs:1346): run_body 1358,
  run_test_body 1407, run_lambda 1483, fold_leaf pure.rs:889, and `compile` asserts (`seal_compile_refusal`). The only
  write to `self.engine` outside build is test code (interp.rs:7973). `engine_for` decides the sealed case before the override or the env var.
