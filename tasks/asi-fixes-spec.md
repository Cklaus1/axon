# asi-fixes - DRAFT spec (AF-1..AF-6)

**Status: DRAFT (after Step 1 review). Not authorized for build.**

### Step 1 review

Reviews: `agent://AuditAF` (AFA#1-6) and `agent://DevloopAF` (AFD-1..9), plus integrator rulings 1-10. I verified each finding by
reading the cited line or the reviewer's probe text (`/var/tmp/r51-tmp/AFA/`); I ran one probe of my own (`AsiFixes/rv/sh.ax`, AFA#6).
All 15 findings are accepted; none rejected.

| finding | severity | disposition | where |
|---|---|---|---|
| AFA#1 join keeps pre-match moves (false refusal of rj, rj1, rjd) | blocker | accepted (ruling 1): per-name join over live arms; rj, rj1, rjd added as "fix" rows; M4625 | AF-2 table, step 1, §6.12 |
| AFA#2 `exit` bound by an arm pattern read as diverging (ex accepted after the fix) | major | accepted (ruling 2): `exit_local` also counts arm/loop pattern bindings; ex added as "keep" row; engine divergence logged as O-AF-06; M4624 | AF-2 step 1, step 4, O-AF-06 |
| AFA#3 guard moves lost | minor | accepted (ruling 3): guards walk on a running "tried" state; gm becomes E0601; M4616 re-pointed | AF-2 step 1, behaviour change (b) |
| AFA#4 `ra` cannot be accepted soundly | major | accepted (ruling 4): refused, as a decision, with the real-use-after-move reason | AF-2 behaviour change (a), §6.10 |
| AFA#5 "refuses nothing that runs today" false for dead code | minor | accepted (ruling 8): replacement text adopted | AF-1 step 2 |
| AFA#6 shorthand field `E::V { f }` would get E0409 | minor | accepted (ruling 7): shorthand fields skipped; probe sh.ax observed rc 0 today | AF-3 design and tests |
| AFD-1 native parity test in cli_run.rs runs in no codegen gate | major | accepted: moved to `integration_fixtures.rs` under `cfg(feature = "codegen")`; gated by `--strict`; M4603 args | AF-1 parity, g01 |
| AFD-2 `cfg(asi-runtime)` pin runs nowhere | major | accepted: literal-pin alternative, no new gate line (ruling 9) | AF-4 step 2, §6.7 |
| AFD-3 M4623 mutation in cli_run.rs cannot be killed | major | accepted: M4623 now mutates `examples/asi/run.sh` | g01 |
| AFD-4 fake binary passes the test; only step 1 rc checked | major | accepted: every step's rc checked; record rc = replay rc = 3; replay-only crash fake added | AF-5 |
| AFD-5 "dead code" half right | minor | accepted: reworded (unreachable because `harness()` sets AXON) | AF-5 reproduction |
| AFD-6 g01 crate/args columns missing | minor | accepted: columns added | g01 |
| AFD-7 `main.rs` overlap with CE-2/CE-5, LF-4 not listed | minor | accepted | §5 |
| AFD-8 AF-2 sweep must run after LF-2; if-let twins | minor | accepted: sweep after wave 3; if-let twins of h2/hp1/hmbr; combined batch order | AF-2 gate step and tests, §5 |
| AFD-9 strict cost row missing | minor | accepted | §4 |
| former NH-1..NH-4 | - | decided by rulings 5, 4, 6, 7; moved to §6 (decisions 9, 10, 11, 6). The options were: NH-1 (a) E1300 / (b) panic / (c) `Err`; NH-2 (a) refuse `ra` / (b) keep; NH-3 (a) close O-AF-01/02 / (b) leave open; NH-4 (a) refuse last arm / (b) exempt | §6, §7 |

Base: `asi/next` = main `a5310878`. Reference binary: `/var/tmp/r51-tmp/asi-bin/axon` (`axon 0.1.0 (a5310878)`, default features, so
no `asi-runtime`). Scratch and every probe file named below: `/var/tmp/r51-tmp/AsiFixes/` (`mv/`, `mv2/`, `mv3/`, `mv4/` for AF-2,
`bv/` for AF-3, the top level for AF-1/AF-4/AF-5). The full AF-2 matrix, with program text and output, is in
`/var/tmp/r51-tmp/AsiFixes/af2-matrix.txt`. The probe programs that matter are inlined below. Anything not observed by running or
reading code is marked `[INFERENCE]`.

## 0. Source entries

| item | entry (tasks/opportunities.md) | sev |
|---|---|---|
| AF-1 | O-SLV-01 (`:3235`), O-NP-02 (`:3001` area, "fixtures/ai_extract_generic.ax: interpreter exit 101, native exit 0") | high / low |
| AF-2 | O-HX3-02 (`:3084`), O-HX3-03 (`:3111`) | high / high |
| AF-3 | O-CS3-01 (bare variant name in a pattern binds instead of comparing; W0004 only) | - |
| AF-4 | O-RP-03 ("extracts leave no audit record": no `ai_call` record, no cost; live path stores tokens 0) | low |
| AF-5 | Hello Goal acid test passes vacuously: `crates/axon-core/tests/cli_run.rs:9046-9072` | - |
| AF-6 | O-HI-03 (`:1694`), plus stale-open entries O008, O009, O016-O020, O-RLM-02/04/06, O-SESS-01, O-RLM2-03, wrapping_*, RecordingHost/ReplayHost, replay-diff, `axon replay` verb | - |

### Out of scope (logged, not dropped)

| what | why it is not here | where it lives |
|---|---|---|
| Native no-key semantics of `__axon_ai_complete` and the five `__axon_ai_extract_*` bridges (key error returned as `Err`, rc 0) | Owned by Ceilings CE-8 / NH-C10 (`tasks/ceilings-spec.md:660-700`) | ceilings-spec |
| R3c budget and `AXON_BUDGET_TOKENS` hooks for extracts | Ceilings CE-4/CE-6 hook the shared pre-dispatch gate once AF-4 routes extracts through it (`ceilings-spec.md:78,406,527`). AF-4 only provides the routing | ceilings-spec |
| Loop-carried moves are not detected: `mv3/loopmove.ax` (`for x in xs { s = s + p.x; let t = p }`) and `mv3/hmcont_if.ax` both check rc 0 and run (`14`, `8`) | Pre-existing hole in `borrow.rs` loop handling (`:452-487`: the body is walked once). Closing it refuses programs that run today and print the right answer | new entry **O-AF-01** (left open: decision §6.11) `[REVISED: ruling 6]` |
| A move escapes a tail-diverging `if` branch through `break`: `mv2/br1.ax` checks rc 0 and prints `7` | Pre-existing hole in the CS-6 `If` rule (`borrow.rs:344-363`, which uses only syntactic `branch_diverges`). AF-2 does NOT copy this hole into `match` (see AF-2 hmbr) | new entry **O-AF-02** (left open: decision §6.11) `[REVISED: ruling 6]` |
| A pattern-bound closure named `exit`: the interpreter calls the closure, native calls the builtin (`AFA/ex2.ax`: interp rc 0, native rc 3) | Pre-existing engine divergence; AF-2 follows the interpreter | new entry **O-AF-06** `[REVISED: AFA#2, ruling 2]` |
| `return` inside `spawn { }` returns from the enclosing fn in the interpreter: `mv2/sr.ax` (`let h = spawn { return 5 }; 9`) prints `5`; `mv/sp1.ax` (`spawn { return name }` then `name`) is E0601 | The semantics of `return` in `spawn` are unspecified. AF-2 does not treat `Spawn` as diverging | new entry **O-AF-03** |
| A qualified pattern of a DIFFERENT enum is accepted and never matches: `bv/v2q.ax` (`match c { D::Up => 2, _ => 1 }`, c: C) check rc 0, prints `1` | A pattern/subject type mismatch, not the bare-name class | new entry **O-AF-04** |
| Other `cli_run.rs` tests that early-return "skipping" when a script or toolchain is missing (survey below, AF-5) | Not milestone gates. They already go through `note_harness_skip` or are toolchain-optional | AF-5 table |
| AF-6 PARTIAL entries (O008, O009, O-RLM-06, `axon replay` verb) | Only the fixed half is stamped; the open half stays open | AF-6 |

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P0 | AF-1, AF-2 | AF-1: a checked program panics in the reference engine, and native silently drops the call for out-of-set T (silent wrong answer). AF-2: false refusals of valid programs (worst outcome class) |
| P1 | AF-3, AF-4, AF-5 | AF-3: silent wrong answer on both engines. AF-4: an audit gap on the "fully audited" contract. AF-5: a milestone gate that cannot fail |
| P2 | AF-6 | Docs and tracker truth |

## 2. Items

### AF-1 (P0) `ai_extract::<T>(prompt)` checks clean, then the interpreter panics

**Anchors (read at a5310878).**
- Parse: `parser.rs:2685-2704` `parse_ai_extract_turbofish` lowers to `Call { callee: StructLit { name: "ai_extract::<T>", fields: [] }, args: [prompt] }`.
- Type: `infer.rs:2019-2060` types it as `Result<T, str>`. The comment at `:2021-2025` says "v1 T set is enforced downstream in codegen". **Nothing enforces it at check time.**
- Effects and capabilities: `effects.rs:994-1006` `effect_row_name` maps every `ai_extract::<..>` to `ai_extract_i64`. `capabilities.rs:165-168` (`classify_call` gives Net for the prefix) and `:188-195` `ai_builtin_host` (prefix counted).
- Interpreter: `interp/eval.rs:634-705` handles a `StructLit` callee only for `chan::<T>` / `Chan::new`. Otherwise the callee evaluates to a value and `interp.rs:2783` `not_callable` panics. The VM sends `StructLit` callees to the tree (`interp/vm/compile.rs:1729` `Some("struct-lit")`), so it fails the same way.
- Native: `codegen/expr.rs:9912-9945` maps `i64`/`f64`/`bool` to the `__axon_ai_extract_{i64,f64,bool}` bridges and the `Uncertain` forms to the uncertain bridges `[INFERENCE: exact Uncertain mapping lines not re-read]`. An out-of-set T falls past `:9964` (a `StructLit` is excluded from the closure path) and the call is **dropped silently**.
- Interpreter extract arms: `interp/builtins.rs:6261-6325` (`ai_extract_uncertain_i64` and `_f64`: replay, then mock, then live under `#[cfg(feature = "asi-runtime")]`, else `panic("... requires --features asi-runtime (or set AXON_AI_MOCK=1)")`).
- `ai_complete` arm: `interp/builtins.rs:6018-6260`. It resolves the tier (`:6029-6031`), estimates cost, runs the R3c budget (`:6044-6062`), appends to the audit ledger (`:6132-6134`), then replay (`:6135-6157`), mock, live, and the E1300 fallback.
- Flat names: `builtins.rs:2589` (`is_impure_builtin`) and `:2699-2702` (`builtin_effect_row` = `["AI","Net"]`) list `ai_extract_i64/f64/str/bool`. They are **not** in `BUILTINS`, so `ai_extract_i64("x")` is E0001 (observed, `t_flat.ax`).
- axon-ai: the mock values are `i64` 1, `f64` 1.0, `bool` true, confidence 0.9 (`axon-ai/src/lib.rs:845-850`). `complete_typed_{i64,f64,bool}` are at `:999-1022` and `complete_structured_inner` at `:829`. The native refusal of a live call is `refuse_unaudited_native_live_ai` (`:410`, commit 105444e6).

**Reproduction.** `g.ax` = `crates/axon-core/tests/fixtures/ai_extract_generic.ax` verbatim (five fns, one per T; `main` prints each `Ok`).
```
$ axon check g.ax                         rc=0
$ AXON_AI_MOCK=1 axon run g.ax            axon: panic: value of type ai_extract is not callable   rc=101
$ env -u ANTHROPIC_API_KEY axon run g.ax  same panic                                               rc=101
$ AXON_ENGINE=vm AXON_AI_MOCK=1 axon run g.ax   same panic                                         rc=101
$ axon build g.ax -o g.bin                rc=0
$ AXON_AI_MOCK=1 ./g.bin                  1 / 1 / yes / 1 / 1  rc=0   [stdout lines; f64 prints via to_str_f64]
$ env -u ANTHROPIC_API_KEY ./g.bin        "ANTHROPIC_API_KEY (or AXON_AI_API_KEY) is not set" x5 on stderr, rc=0
$ AXON_AI_MOCK= ANTHROPIC_API_KEY=sk-fake ANTHROPIC_BASE_URL=http://127.0.0.1:9 ./g.bin
    `ai_extract_i64` reached a LIVE model call, but this is a NATIVELY BUILT binary ... Refusing to run   rc=2
```
Out-of-set T (`t_str.ax`: `fn main() { match ai_extract::<str>("name?") { Ok(s) => println(s) Err(e) => println(e) } }`): check rc 0,
run panics rc 101, build rc 0, and the native mock run prints **nothing**, rc 0. `t_str3.ax` (with `println("before")`/`"after"` around
the match) natively prints only the surrounding lines. `t_rec.ax` (`ai_extract::<R>` for a record `R`) behaves the same. `t_str2.ax`
(`let r = ai_extract::<str>(..)`) and `t_dead.ax` get native E0910. Mock parity of the legacy names (`uf.ax`, the two uncertain
builtins): interpreter and native both print `1 0.9` / `1 0.9`.

**Fix design.**
1. **One mapping, shared.** Add `pub(crate) fn ai_extract_target(t: &str) -> Option<&'static str>` in `builtins.rs`:
   `i64`→`ai_extract_i64`, `f64`→`ai_extract_f64`, `bool`→`ai_extract_bool`, `Uncertain<i64>`→`ai_extract_uncertain_i64`,
   `Uncertain<f64>`→`ai_extract_uncertain_f64`. It is keyed on `axon_type_to_str` of the parsed T, the spelling `parser.rs:2695` writes.
   Use it in infer (validation), interp (dispatch), `effects.rs:999` `effect_row_name` (replaces the hardcoded `ai_extract_i64`),
   and `codegen/expr.rs` (replaces its local match). After this, codegen and interp cannot disagree on the T set.
2. **Check-time refusal of out-of-set T: E1304** (released by Ceilings, `ceilings-spec.md:870-871`, and claimed here). In
   `infer.rs:2026-2060`, when `ai_extract_target` is `None`, push
   `E1304 "ai_extract::<{T}> is not supported: T must be one of i64, f64, bool, Uncertain<i64>, Uncertain<f64>"` with the help
   `"for free text use ai_complete(prompt); for a record, extract each field with its own ai_extract::<T>"`. This refuses only
   programs whose out-of-set call is never reached: `t_dead.ax` (an uncalled fn using `ai_extract::<str>`) runs rc 0 today in the
   interpreter, and native already gives E0910. 0 tracked programs (the only tracked `.ax` is the fixture, all in-set).
   `[REVISED: AFA#5 - the draft said "refuses nothing that runs today", which is false for dead code]`
3. **Interpreter dispatch.** In `interp/eval.rs` `Expr::Call` (the `StructLit` branch at `:636`, beside `chan::<T>`), for a name with
   prefix `ai_extract::<`: evaluate the single argument with `eval_t::<T>` (as `:698-700` do), map it with `ai_extract_target`, and
   dispatch through the **same builtin block `dispatch_named` uses** (`eval.rs:1687-1712`: `t_builtin_in`, pc, `call_builtin`,
   `t_builtin_out`, `t_builtin_pick`). Extract that block into one private helper called from both places, so the source keeps exactly
   the `call_builtin(` sites that `taint_tests.rs:935-962` `builtins_are_dispatched_only_where_the_taint_is_routed` pins. No local
   closure, no user fn and no sealed-local lookup can capture the turbofish, because it has no name binding.
4. **Three hidden runtime arms** `ai_extract_i64/f64/bool` in `interp/builtins.rs`, next to `:6261-6325`, with the same order of paths
   as the uncertain arms: replay lookup keyed by `(prompt, builtin name)` (the convention at `:6264-6276`) → mock (`Int(1)`, `Float(1.0)`,
   `Bool(true)`, wrapped in `Ok`; byte parity with `axon-ai/src/lib.rs:845-850`) → live under `asi-runtime`
   (`axon_ai::complete_typed_{i64,f64,bool}`) → no runtime and no mock. The shared prologue (provenance, cost, ledger) is AF-4.
   The names stay out of `BUILTINS`: no new user surface.
5. **Tables a hidden name must be in.** `builtin_effect_row` and `is_impure_builtin` list them already. `audit_effect_kind`
   (`interp/builtins.rs:337`) derives `AI` from the row (the 92c9e639 fix), so the ledger kind is AI. **Add** the three names to
   `capabilities.rs:158-164` `classify_call` (Net) and `:188-195` `ai_builtin_host`: the runtime sandbox net check
   (`interp/builtins.rs:187-215`) looks hosts up by the canonical name, and without this entry it skips the check. **Taint:**
   `interp/taint.rs:711-738` `info()` is built from `BUILTINS` only, so a hidden name has no class and `t_builtin_in` does nothing
   (`taint.rs:1388-1395`, `None => {}`). Add a hidden-target list to the `info()` map with `Class::World` (as
   `ai_extract_uncertain_*` have, `taint.rs:236-238`). Do not add them to `WORLD_BUILTINS`: the drift test (`taint_tests.rs:856`)
   requires every entry there to be a builtin.
6. **No runtime, no mock: E1300, rc 5** for all five extract targets, matching `ai_complete`, with an extract-specific text (a
   `fallback:` is a `str` and cannot stand in for `T`, so the extract arms do not consult it). rc 5 maps to
   `Denied{axis:"ai-policy"}` in axon-os (`axon-os/src/runtime.rs:1360-1369`); 101 is a crash. Today the two uncertain arms
   panic (rc 101), so this changes only programs that already fail. Alternatives rejected: keep the panic (a crash is not a
   refusal); return `Err` (diverges from `ai_complete`). `[REVISED: integrator ruling 5 - former NH-1 is now a decision]`
7. **Native.** Unchanged except that it uses the shared mapping. Out-of-set T can no longer reach codegen (E1304 at check).

**Alternatives considered.** Desugar in the parser to `Call { callee: Ident("ai_extract_i64") }`: this makes the flat names a user
surface (E0001 today) and breaks codegen's `StructLit` path. Rejected. Add the flat names to `BUILTINS`: new surface, docs and
`axon reference` rows. Rejected. Dispatch `ai_extract::<i64>` as `ai_extract_uncertain_i64` and drop the confidence: `bool` has no
uncertain builtin, and the live schema differs from native's flat bridge (`complete_typed_flat_inner`). Rejected.

**Interp/native parity under mock.** The native-vs-interpreter test goes in `crates/axon-core/tests/integration_fixtures.rs` under
`#[cfg(feature = "codegen")]`, appended as an integrator block (the convention of ceilings CED-5 and lang-forms :703). It builds
`ai_extract_generic.ax`, unchanged, and compares stdout with `axon run` under `AXON_AI_MOCK=1`. Expected on both engines:
`1\n1\nyes\n1\n1\n`. The only gate that runs it is `gate.sh --strict` (`--test integration_fixtures`, `gate.sh:1196`), plus CI's
`codegen` job (`cargo test -p axon-core`, `ci.yml:230-234`). S16 is `--no-default-features` (`gate.sh:1069`), and
`all_examples_parity.sh` builds only `examples/*.ax` and `examples/stdlib/*.ax`. Without LLVM, the interpreter half runs everywhere,
and `axon-ai` unit tests call the bridges directly under `AXON_AI_MOCK=1` (no LLVM; the pattern of `lib.rs:1366`
`mock_reaches_the_structured_output_path`) and assert the same values. They are gated through S20 (the release manifest requires
`cargo test --locked -p axon-ai`). `[REVISED: AFD-1 - the draft put this in cli_run.rs, where no gate stage runs it with codegen,
and claimed CI runs gate.sh, which ci.yml:104 says it does not]`

**Fail-first tests** (`cli_run.rs`, appended block):
- `ai_extract_generic_runs_under_mock_on_every_engine`: `vm_same_both_engines` (`:37963`) on the fixture text with `AXON_AI_MOCK=1` →
  rc 0, stdout `1\n1\nyes\n1\n1\n`, on `tree`, `vm`, `vm-default`. Fails today (rc 101).
- `ai_extract_generic_native_matches_interp_under_mock` (in `integration_fixtures.rs`, `cfg(feature = "codegen")`): native vs interpreter stdout equal. `[REVISED: AFD-1]`
- `ai_extract_out_of_set_type_is_e1304`: `ai_extract::<str>` and `ai_extract::<R>` → check rc 2, E1304 at the turbofish line. Fails today (rc 0).
- `ai_extract_turbofish_audits_as_ai`: with `AXON_AUDIT_LEDGER`, the ledger has an `AI` row whose operation is `ai_extract_i64` (the pattern of `every_model_call_audits_as_ai`, `:4572`).
- `ai_extract_without_runtime_or_mock_is_e1300`: builds without `asi-runtime` (every gate build), `AXON_AI_MOCK` unset, no replay; `ai_extract_uncertain_i64` and `ai_extract::<bool>` → rc 5, stderr `E1300`. Fails today (rc 101 panic). `[REVISED: integrator ruling 5]`
- `ai_extract_turbofish_is_net_scoped_in_a_sandbox`: a sandbox whose net scope excludes `api.anthropic.com` denies `ai_extract::<bool>` the same way it denies `ai_extract_uncertain_i64` `[INFERENCE: harness for sandboxed interp runs to be chosen from the existing ai_builtin_host tests]`.
- Taint unit test in `taint_tests.rs`: `info("ai_extract_i64")` is `Class::World`.

**Docs.** `VISION.md:123` claims `ai_extract<T>` shipped. After AF-1 that holds for the five T. Reword the row to name the T set.
`spec/axon-for-llms.md` and `AXON_REFERENCE.md` (regenerated): the T set and E1304. Update the fixture header comment ("does NOT execute").

**Acceptance.** The fixture runs under mock on tree, VM and native with identical stdout. Out-of-set T is E1304 at check on both
engines. The ledger kind is AI, the sandbox host check covers the three hidden names, and the hidden names are World in taint.

### AF-2 (P0) The move checker treats match arms as sequential, and a `return` in an arm or loop marks the value moved

**Cause (read, `crates/axon-core/src/borrow.rs`).**
- `:500-507` `Expr::Match`: arms are walked one after another on ONE `bindings` map, with only `push_scope`/`pop_scope`. A move in arm 1
  is seen by arm 2 (O-HX3-02), and every arm's moves flow out of the match, including the moves of an arm that returns (O-HX3-03, first half).
- `:333-364` `Expr::If` does it right: a snapshot, each branch from the snapshot, then a union of the branches that do not diverge
  (`branch_diverges`, `:86-105`).
- `:415-417` `Expr::Return(Some(v))` calls `check_move_expr(v)`. `move_binding` (`:203-229`) reports **E0602** when `v` is `Borrowed`.
  The collection `for` desugars to `ref __forarr_N = xs` (`parser.rs:2128-2144`), so `xs` is Borrowed in the loop body and
  `return xs` is E0602 (O-HX3-03, second half).
- `:452-487` `While`/`WhileLet`/`For`: the body's moves always flow out of the loop, even when the body ends in `return`.

**Reproduction** (the `axon check` code and line; full text in `af2-matrix.txt` and `mv4/`). "fix" = must pass after AF-2; "keep" = must
stay refused (the hole probes).

| probe | shape | today | after |
|---|---|---|---|
| mv/h2 (O-HX3-02 inlined) | `let t = name` in arm 0 and in arm 1, `r` after | E0601 :8 | **fix** (if-twin `h2twin` rc 0, prints `a0`,`b1`) |
| mv/h3a (O-HX3-03 inlined) | `None => return p`, then `p.x` | E0601 :7 | **fix** (if-twin `h3atwin` rc 0, `12`) |
| mv/h3b (O-HX3-03 inlined) | `for x in xs { if x < 0 { return xs } }` | E0602 :3 | **fix** (index-loop twin `h3btwin` rc 0, `2`,`0`) |
| mv/hp4 | arm 0 `let t = name; return t`, then `r + name` | E0601 :9 | **fix** |
| mv/hp7 | arm 0 `let t = name ... exit(3)`, then `name` | E0601 :10 | **fix** (if-twin `hp7if` rc 0, `b`,`a`) |
| mv4/gd | guarded arm `0 if len(name) > 0 => return name`, then `name` | E0601 :6 | **fix** |
| mv4/nest | inner match arm `0 => return name` inside outer arm `Some(v)`, then `name` | E0601 :11 | **fix** |
| mv2/lp1 | `for x in xs { return name }`, then `name` | E0601 :5 | **fix** |
| mv4/wl | `while i < n { return name }`, then `name` | E0601 :6 | **fix** |
| mv/hp9 | `ref r = s; if len(s) > 0 { return s }` | E0602 :3 | **fix** (a return ends every borrow of the frame) |
| mv/hp6 | both arms move `name`, then `r + name` | E0601 :8 and :12 | **keep** :12 only (the :8 cross-arm error goes away) |
| mv/hp1 | arm 0 moves, arm `_` does not, then `r + name` | E0601 :9 | **keep** |
| mv/hp2 | `let t = name; t + name` in one arm | E0601 :5 | **keep** |
| mv/hp3 | arm 0 moves (live), arm 1 `return "one"`, then `r + name` | E0601 :10 | **keep** |
| mv/hp5 | `let t = name` before the match, `0 => return name` | E0601 :3 | **keep** |
| mv/hp8 | arm `{ let t = name; break }` in a `for`, then `out + name` | E0601 :12 | **keep** (break moves flow out) |
| mv3/hmbr | arm ends in `return 1` but contains `if x == 0 { let t = p; break }`; `p.x` after the loop | E0601 :15 | **keep** (see "escaping jump") |
| mv2/br2 | `if c { let t = p; break }` in a `while`, then `p.x` | E0601 :13 | **keep** (unchanged code path) |
| mv4/hp9b | `ref r = s; let t = s` | E0602 :3 | **keep** |
| mv3/hminner | arm `{ let t = p; for y in xs { break }; return t.x }`, then `p.x` | E0601 :11 | **fix** (the `break` is bound by the inner `for`, so it does not escape) |
| mv3/hmcont | arm 0 `if s == 0 { let t = p; s = 1; continue }; return 1`, arm `_ => s = s + p.x` | E0601 :14 | **becomes rc 0** - see "behaviour changes" |
| mv4/ra | arm 0 `let t = s`, arm `_ => s = "new"`, then `s` | rc 0, prints `new`,`a` | **becomes E0601** - a real use-after-move (see "behaviour changes") |
| AFA/rj | `let t = s`; `match k { 0 => { s = "zero" } _ => { s = "other" } }`; `s + t` | rc 0, prints `othera`,`zeroa` | **fix (stays rc 0)**: every live arm re-binds `s` `[REVISED: AFA#1]` |
| AFA/rj1 | as rj with the single arm `_ => { s = "other" }` | rc 0, prints `othera` | **fix (stays rc 0)** `[REVISED: AFA#1]` |
| AFA/rjd | `let t = s`; `0 => { return t }`, `_ => { s = "other" }`; `s + t` | E0601 :8 | **fix** (the returning arm is discarded and the live arm re-binds `s`) `[REVISED: AFA#1]` |
| AFA/ex | `Some(exit) => { let t = name; exit(3) }` (`exit` is the arm's closure binding), then `name` | E0601 :7 | **keep**: the arm does not diverge, because `exit` is the bound closure `[REVISED: AFA#2]` |
| AFA/gm | `0 if { let t = name; false } => return "x"`, `_ => {}`, then `name` | rc 0, prints `a` | **becomes E0601**: the guard ran and moved `name` (see "behaviour changes") `[REVISED: AFA#3]` |

**Fix design.**
1. `Expr::Match` (`:500-507`): `check_read(subject)`; `snapshot = bindings.clone()`; `tried = snapshot.clone()`. For each arm k, in
   order: `bindings = tried`, push scope, walk the guard if there is one (guards are not walked today), then set
   `tried = bindings` (a guard runs whenever its arm is tried, even if it fails, so its moves reach every later arm and the code
   after). Then walk the body, pop scope, and record the arm's end state. An arm is discarded (not live) iff
   `branch_diverges(&arm.body, exit_local)` **and** `!has_escaping_jump(&arm.body)`. **Join:** if no arm is live,
   `bindings = tried`. Otherwise each name's state is the join of the live arms' end states and `tried`'s moves: `Moved` if it is
   `Moved` in any live arm or was moved by a guard (in `tried` but not in `snapshot`), else the state the live arms share (`Owned`
   if every live arm ends `Owned`). Names introduced inside an arm are popped with its scope.
   `[REVISED: AFA#1 - the draft kept the pre-match state and only added moves, so a name re-bound in every live arm stayed Moved
   (false refusal of rj, rj1, rjd). AFA#3 - guards walk on a running "tried" state]`
   **`exit_local` per arm** = (the scope has a local `exit`, as at `:350`) **or** (the arm's pattern binds `exit`, via
   `resolver::collect_pattern_binding_names`). The checker never adds pattern bindings to `bindings` (`:500-507` only pushes a
   scope), so without the second half `Some(exit) => { ..; exit(3) }` would count as diverging. `[REVISED: AFA#2]`
2. New `fn has_escaping_jump(e: &Expr) -> bool`: true if `e` contains a `Break` or `Continue` that is not inside a nested
   `While`/`WhileLet`/`For` body. It does not descend into `Lambda` bodies (they are checked separately, `:488-491`). It DOES descend
   into `Spawn` and `Select` (conservative: counting a jump as escaping keeps moves, which is the refusing direction).
3. `Expr::Return(Some(v))` (`:415-417`): if `v` is an `Ident` whose state is `Borrowed`, do not call `move_binding` (a return ends the
   frame, so every `ref` of it is dead). A `Moved` ident is still E0601 (hp5). Any other expression keeps `check_move_expr`.
   Sound because only a local `RefBind` sets Borrowed (`:288-293`) and parameters start Owned (`:169-171`), so a borrowed
   parameter cannot be returned by move (AFA, checked).
4. Loops (`:452-487`): `snapshot` before the body. After the body, if its last statement diverges (`branch_diverges` on
   `Block(body)`, with `exit_local` also true when the `for`/`while let` pattern binds `exit` `[REVISED: AFA#2]`) and
   `!has_escaping_jump(body)`, restore `snapshot`: every iteration that starts ends in `return`/`exit`, so no body move reaches the
   code after the loop, and the zero-iteration path equals the snapshot. Otherwise keep today's behaviour (body moves flow out).
5. `If` is **unchanged**: it keeps the br1 hole (O-AF-02, a decision in §6).

**Behaviour changes, stated.** (a) `ra.ax` runs today (prints `new`,`a`) and becomes E0601. This is decided, not open: `f(0, "a")`
takes the `0 => { let t = s }` path and then returns `s`, a real use after move that prints `a` only because the interpreter shares
values. Any rule that accepts `ra` also accepts its arm-swapped twin `rb.ax` and the if-twin `rif.ax`, both E0601 today.
`[REVISED: AFA#4, integrator ruling 4 - former NH-2]` (b) `gm.ax` runs today (prints `a`) and becomes E0601, for the same reason:
the guard moves `name` on every call that tries arm 0. `[REVISED: AFA#3]` (c) `hmcont` goes from E0601 to rc 0. Its refusal today
came only from the sequential-arms bug; the remaining use-after-move is loop-carried, and the if-twin `hmcont_if` already passes
(O-AF-01). **Gate step:** before merging, run `axon check` on every tracked `.ax` file (`git ls-files '*.ax'`, 700 files) and on
every fixture with the old and the new binary. **The sweep runs on the tree where LangForms LF-2 (its lane B) has also landed**,
because `if let` then desugars to `match` and follows this rule `[REVISED: AFD-8]`. Any file that goes from rc 0 to nonzero blocks
the item and goes to a human.

**New logged entry O-AF-06** `[REVISED: AFA#2, integrator ruling 2]`: when a pattern binds a closure named `exit`, the engines
disagree. In `AFA/ex2.ax` (`Some(exit) => { println("arm"); exit(3) }`), the interpreter calls the closure and falls through
(prints `arm`, `after`, rc 0), while the native build calls the builtin `exit` (rc 3) (AuditAF, observed). This is pre-existing and
not fixed here; AF-2 takes the interpreter's reading (the reference semantics), so such an arm is live.

**Alternatives considered.** A full flow analysis (a dead flag after return/break, with break states joined at the loop exit). It is
precise, but applied to `If` it refuses br1, which runs today. Applied only to `match`/loops it would be a second convention next to
`If`'s. Rejected for this batch. Discarding diverging arms with the syntactic rule alone (as `If` does): this accepts hmbr and copies
the br1 hole into `match`. Rejected.

**Native/VM.** The borrow check is front-end only (`main.rs:7754`, `lib.rs:1438`), so the engines need no change.

**Fail-first tests** (`cli_run.rs` block, `axon check` on `tmp_ax` files). `match_arms_are_alternatives_for_moves` (h2, hp4, hp7, gd,
nest, hminner, rjd → rc 0; rj, rj1 stay rc 0; also run under mock and compare stdout with the if-twins). After LF-2 lands, the same
test also covers `if let` twins of h2, hp1 and hmbr, with the same verdicts `[REVISED: AFD-8]`.
`return_in_arm_or_loop_does_not_move_for_code_after` (h3a, h3b, lp1, wl, hp9 → rc 0). `move_hole_probes_stay_refused` (hp1, hp2,
hp3, hp5, hp6 at line 12 only, hp8, hmbr, br2, hp9b, ex → same code and line as in the table). `use_after_move_on_one_arm_is_refused`
(ra, gm → E0601). `[REVISED: AFA#1-#4, ruling 4 - ra is no longer conditional]`

**Acceptance.** Every "fix" row passes and every "keep" row keeps its code and line. The corpus sweep reports no rc 0 → nonzero file,
or each one is approved by a human.

### AF-3 (P1) A bare variant name in a pattern binds instead of comparing

**Anchors.** `parser.rs:3795-3801`: `C::Red` parses to `Pattern::Struct { name, fields: [] }`, and a bare ident to `Pattern::Ident`.
`checker.rs:4745-4760` Match: the subject type is `resolve_expr_type` (`:4750`) and `check_match_exhaustiveness` runs at `:4751`.
`checker.rs:6606-6660` W0004 (a later arm after a binding catch-all). `checker.rs:6703-6709` resolves the subject's enum (`Type::Enum`,
or `Type::Struct` registered in `enum_variants`). `enum_variants` is at `:521`, and `enum_variant_fields` / `_field_types` at
`:534-538` (filled at `:889-900`). While-let: `checker.rs:5035` `[INFERENCE: the check_expr WhileLet arm; pattern subject typing to
be confirmed by the implementer]`.

**Reproduction** (`bv/`; check result, interpreter output, native output):

| file | pattern | expected | check | run | native |
|---|---|---|---|---|---|
| v1 | `match c { Green => 2, Red => 1, Blue => 3 }`, calls Red, Blue | `1`,`3` | rc 0, W0004 x2 | `2`,`2` | `2`,`2` |
| v11 | `match c { Green => 2 }`, call Red | non-exhaustive | rc 0, **no diagnostic** | `2` | - |
| v5 | `Some(Green) => 2, Some(Red) => 1, ...`, call Some(C::Red) | `1` | rc 0, W0004 x2 | `2` | `2` |
| v6 | `Green if k > 0 => 2, C::Red => 1, _ => 3`, call (Red,1) | `1` | rc 0 | `2` | `2` |
| v9 | `(Green, n) => n + 2, (Red, n) => n + 1`, call (Red,10) | `11` | rc 0 | `12` | `12` |
| v10 | `while let Some(Green) = xs[i]` over `[Some(C::Red), None]` | `0` | rc 0 | `1` | `1` |
| v7 | sum type `S = A{v} \| B{w}`: `B => 2, S::A { v } => v`, call A{7} | `7` | rc 0, W0004 | `2` | `2` |
| v8 | `match c { Red => 1 }`, call Green | non-exhaustive | rc 0 | `1` | `1` |
| v2 | `Up => 2, _ => 1` (Up is a variant of a different enum D) | - | rc 0, W0004 | `2` | `2` |
| v4 | subject `i64`: `Red => Red + 1` | `5` (a binding) | rc 0 | `5` | `5` |
| v3 | `C::Green => 2, other => 1` (lowercase binding) | `1` | rc 0 | `1` | `1` |

**Fix design.** A new check `check_bare_variant_patterns(pat, ty)`, called for each match arm at `checker.rs:4753` and for the
while-let pattern. It walks the pattern together with its expected type: `Some`/`None` against `Option<T>`, `Ok`/`Err` against
`Result`, `Tuple` element-wise, and `Struct` fields through `enum_variant_field_types` or the struct's field types. It stops when the
type is unknown. At a `Pattern::Ident(n)` whose expected type is enum `E` (resolved as at `:6703-6709`) and where `n` is a variant of
`E`, it emits:
- **E0409** (free at a5310878: `error.rs` defines E0401-E0407. E0408 is avoided because `spec/compiler-phase2.md:389` uses it.
  Sent to LangForms by message): ``"`{n}` in a pattern binds a new variable that matches every `{E}`; it does not compare against
  the variant `{E}::{n}`"``. Help for a unit variant: ``"write `{E}::{n}`"``. For a variant with fields: ``"write `{E}::{n} { f1, f2 }`"``
  (names from `enum_variant_fields`).
- **A name that is a variant of a different enum** (v2) or a subject that is not an enum (v4): **no new diagnostic**. Decided. v4 is a
  legal binding with the right answer. For v2, the qualified form `D::Up` is itself accepted silently (`v2q`, O-AF-04), so the fix
  belongs to the pattern-type check, not here.
- Irrefutable binding positions (`let`, and LangForms' `for (a, b) in`, including LF-3's synthetic `let a = __forpat_N.0`): not checked. A name there can only bind.
- LangForms LF-2 desugars `if let P = e { A } else { B }` to `match e { P => A, __iflet_N => B }` (agreed with LangForms by message).
  So the Match check covers `if let` with no extra code. `if let Green = c` (c: C) gets the same E0409 as the `match` form. The check
  skips any `Pattern::Ident` whose name starts with `__`: the parser's synthetic catch-alls cannot be variant names, but the
  exemption keeps them out of the check by construction.
- **Shorthand struct fields are skipped.** `E::V { f }` parses to the field pattern `Pattern::Ident(f)` (`parser.rs:3776-3781`), so
  the walker does not check a field whose pattern is `Pattern::Ident(fname)` with `fname` equal to the field name: that is a pure
  binding, even when `f` equals a variant name of the field's enum. `[REVISED: AFA#6]`
- **Every arm position, including the last arm.** E0409 also fires on a bare variant in the last arm, where only one variant
  remains and the program gives the intended answer today (e.g. `C::Red => 1, C::Green => 2, Blue => 3`). Reasons: one rule; Rust's analogue
  (E0170, bindings named the same as variants) is deny-by-default; the fix is the mechanical `E::V`; 0 tracked hits. The message
  names the qualified form. `[REVISED: integrator ruling 7 - former NH-4 is now a decision]`

**Corpus impact (refusal of a program that runs today).** Scan at a5310878 (Python over `git ls-files`): for every `.ax` file (700)
and every `.rs` under `src/` or `tests/` (510, for test programs embedded in strings), collect the variant names declared in that
file (`enum X { .. }`, `type X = A | B`). Then search for a bare variant name in a pattern position (`Name =>`, `Name if`,
`Some(Name)`, `(Name, ..) =>`). Result: **0 hits in .ax**; 4 hits in `.rs`, all Rust source code, not Axon programs
(`interp.rs:773,945,1153`, `vm/pure.rs:284`). A second pass that used the union of all `.ax` variant names (to catch imported enums)
found 0, and so did the docs (`spec/`, `docs/`, AXON_REFERENCE, README, CLAUDE.md: 643 files). Earlier the reference binary was run
over every tracked `.ax` looking for the binding-catch-all W0004, also with 0 hits. So AF-3 refuses no tracked program. Every refused
shape in the table above gives a wrong answer on both engines, except the last-arm case (decided above). AuditAF re-ran the scan
independently (700 `.ax`, 61 variant names across files, arm/guard/`Some(..)`/tuple/`if let`/`while let` positions): 0 hits.

**Fail-first tests.** `bare_variant_in_pattern_is_e0409` (v1, v5, v6, v7, v8, v9, v10, v11, and the last-arm form
`C::Red => 1, C::Green => 2, Blue => 3` → E0409 with the `C::Green`-style help, at the arm's line).
`bare_name_of_other_enum_or_non_enum_subject_is_not_e0409` (v2, v4, v3, and the shorthand field `enum K { x, y }`,
`type S = A { x: K } | B { w: i64 }`, `match s { S::A { x } => 1, S::B { w } => w }` → rc 0 as today; observed
`AsiFixes/rv/sh.ax`: check rc 0, prints `1`) `[REVISED: AFA#6, ruling 7]`. After LF-2 lands:
`bare_variant_in_if_let_is_e0409` (`if let Green = c { 2 } else { 1 }` → E0409; `if let C::Green = c` → rc 0; the desugared
`__iflet_N` arm raises no E0409).

**Acceptance.** As above, plus a refusal-coverage row for the new site (`scripts/v022_refusal_coverage.py`), the E0409 entry in
`error.rs` (`:754` `error_codes_are_unique` and the explain table near `:471`), and docs: language tour (patterns) and
`spec/axon-for-llms.md`.

### AF-4 (P1) Extracts write no `ai_call` record, charge no cost, and store tokens 0

**Reproduction** (`p1.ax`: `ai_complete("hi")`, `ai_extract_uncertain_i64("how many?")`, `ai_extract_uncertain_f64("how hot?")`, then
`println(to_str(ai_cost_spent()))`), with `AXON_AI_MOCK=1 AXON_AUDIT_LEDGER=led6.jsonl XDG_CACHE_HOME=x6 AXON_AI_REPLAY=rep6.cache`, rc 0:
- provenance (`x6/axon/provenance.jsonl`): `run_start`, then **one** `ai_call` (fn `main`, mode `mock`, cost 3e-06, model
  `anthropic:claude-sonnet`). **The two extracts have no record.**
- ledger: `AI ai_complete:sha256:..`, `AI ai_extract_uncertain_i64`, `AI ai_extract_uncertain_f64`, plus the IO rows. The ledger is
  already right (92c9e639).
- replay cache: the extracts are stored. Their live path stores tokens `0` (`interp/builtins.rs:6283-6284`, `:6311-6313`:
  `ai_replay_store(.., 0)`).
- Budget: `b1.ax` (`@[ai(policy(budget: 1))]` fn with two extracts) rc 0, prints `2`. Its `ai_complete` twin `b2.ax` is E1301 rc 5.
  `AXON_BUDGET_TOKENS=0` with one extract (`u1.ax`) rc 0; with `ai_complete` (`c1.ax`) E1303 rc 5. (Not fixed here; Ceilings CE-4/CE-6.)

**Fix design.**
1. Factor the `ai_complete` prologue (`interp/builtins.rs:6029-6134`: tier, `est_tokens`, cost, budget gate, goal, principal, ledger
   append) into one `fn ai_call_prologue(&self, name, prompt, tier) -> Result<AiCallCtx, Flow>`. `ai_complete` and all five extract
   targets call it. Ceilings CE-4/CE-6 then hook this one gate (agreed by their text, `ceilings-spec.md:78`).
2. **Tier and model for extracts.** Extracts are routed by `axon_ai::decision_model()` (`axon-ai/src/lib.rs:497-502`: CHEAP env, else
   BALANCED env, else `DECISION_MODEL_DEFAULT`), not by the fn's tier. axon-core cannot call axon-ai without `asi-runtime`, so add
   `ai_routing::decision_tier() -> Tier` with the same precedence (CHEAP set → Cheap; else BALANCED set → Balanced; else Cheap). The
   record names `decision_tier().model()` and is costed at that tier's rate (`ai_routing.rs:54-66`). **Pin both sides to the literal**
   (no `asi-runtime` test: no gate stage builds that feature, AFD-2): (i) an `axon-ai` lib test that
   `DECISION_MODEL_DEFAULT == "claude-haiku-5-5"` (`lib.rs:483`) and that `decision_model()` follows CHEAP → BALANCED → default
   (`:497-502`), gated through S20; (ii) an axon-core default-build test that `decision_tier()` follows the same precedence, and
   `decision_tier().api_model()` equals the CHEAP or BALANCED env value when set. The no-env literal `Tier::Cheap.api_model() ==
   "claude-haiku-5-5"` is already pinned at `ai_routing.rs:265`. Under any env, `decision_tier().api_model()` reads the same variable
   `decision_model()` reads, so the two agree. `[REVISED: AFD-2 - the draft's cfg(asi-runtime) pin ran in no gate; integrator chose
   the literal-pin alternative, with no new gate line]`
3. **Records.** Every path (replay, mock, live) calls `append_ai_call_jsonl` (`interp/provenance.rs:196`) under
   `provenance_write_permitted()`, as `ai_complete` does at `:6140-6155`, with fn = caller, mode, cost and `effect_row` `"AI"`, and
   adds the cost to `ai_cost_micro`. Mock cost uses `est_tokens`, the same rule as `ai_complete`.
4. **Live tokens.** `axon-ai`: `complete_structured_inner` (`:829`) also returns the response's usage total, parsed like
   `ai_complete_inner_model_usage` (`:584-612`: Anthropic `input+output`, OpenAI `total_tokens`, else the length estimate). The six
   `complete_typed_*` fns return `(.., tokens)`. The C bridges (`:1060-1290`) drop the token count. Interp stores the real tokens in
   the replay cache and charges them. Update the `axon-ai` tests at `:1379-1389`.
5. `ai_cost_spent()` then counts extracts. **Behaviour change:** `p1.ax` prints a larger number under mock, and `axon trace --ai`
   counts extracts (`main.rs:4689-4693`). Tests that pin call counts: `cli_run.rs:8735` (`"3 ai_complete call(s)"`, no extracts in its
   program `[INFERENCE: re-check]`), `:9141` (Hello Goal: `"ai_complete call(s)"`, substring only), and the replay demos test
   `:21700-21845` (asserts every `ai_call` record is `mode:"replay"`; extracts then add rows, which must also be replay; examples
   `optimize.ax` uses no extract `[INFERENCE]`).

**Fail-first tests.** `extracts_write_ai_call_provenance_on_mock_and_replay` (p1: three `ai_call` rows, extract rows with the decision
model, mode `mock`, cost > 0; a second run with `AXON_AI_REPLAY` and no mock gives mode `replay`). `ai_cost_spent_counts_extracts`.
`axon-ai` unit test: `complete_structured_inner` on a canned response body returns the usage total (no network: factor the body parse
into a pure fn). The AF-1 generic form is covered by the same test using `ai_extract::<i64>`.

**Acceptance.** Every extract execution appends exactly one `ai_call` record with a nonzero cost, on every path. Live tokens come from usage.

### AF-5 (P1) The Hello Goal acid test passes vacuously

**Anchor.** `crates/axon-core/tests/cli_run.rs:9048` `asi_hello_goal_acid_test_loop_runs_end_to_end`: at `:9056-9059`, if
`examples/asi/run.sh` is missing it does `eprintln!("asi/run.sh not found - skipping"); return;`. At `:9068-9072`, if the output
contains `axon binary not found` it does `eprintln!("run.sh could not locate the axon binary - skipping"); return;`. Neither goes
through `note_harness_skip` (`:214-233`), so `AXON_HARNESS_STRICT=1` does not catch them.

**Reproduction.** With a fake `axon` that panics (`/var/tmp/r51-tmp/AsiFixes/fake-axon`), `AXON=$F AXON_AI_MOCK=1 bash
examples/asi/run.sh hello-goal` exits **0** (`hg-fake3.out`). run.sh swallows the failures: each step is `"$bin" run ... || rc=$?`, and
a failed run prints `-> DEPLOY GATE FIRED (exit 101)` as if it were the safety catch. With `AXON=/nonexistent` run.sh prints
`ERROR: AXON=... is not executable` (`hg-bad2.out`). The test's `axon binary not found` branch cannot be reached from the test:
`harness()` always sets `AXON` (`cli_run.rs:44-52`, `:49`), and run.sh:36 prints that string only when `AXON` is unset and the cargo
build fails. `[REVISED: AFD-5]` The real binary prints `DEPLOY GATE FIRED (exit 3)` (VERIFY_FAILED_EXIT_CODE, `interp.rs:1251`).
**The fake binary passes the test as written** (DevloopAF, observed): `hg-fake3.out` contains "DEPLOY GATE FIRED (exit 101) ...
SAFETY CATCH" and "✓ reproducible", and the test's checks (`cli_run.rs:9073-9100`) are "GATE FIRED", "SAFETY CATCH",
"reproducible" and "loop complete". Step 5 (`run.sh:150-158`) compares two identical crash outputs, so it reports "reproducible";
step 4 is `|| true`. `[REVISED: AFD-4 - replaces the draft's "not run" note]`

**Fix design.** (1) Replace both early returns with `panic!` naming the missing prerequisite. `run.sh` is tracked, and the binary is
`CARGO_BIN_EXE_axon` passed by the harness (`harness()`, `:40-56`), so neither can be missing in a correct checkout. (2) Assert the
safety catch precisely: the hello-goal output must contain `DEPLOY GATE FIRED (exit 3)`. A crash (101) must not read as the catch.
(3) `run.sh hello-goal` (`examples/asi/run.sh:125-172`): **every** `$bin run` step (1, 4, the record and the replay) must exit 0 or
3, else run.sh exits nonzero (step 4 loses its `|| true` for any other rc). Step 5 also requires the replay rc to equal the record rc
and both to be 3. `[REVISED: AFD-4, integrator ruling 9]`

**Survey of the same pattern** (an early `return` after an `eprintln!("... skipping")` without `note_harness_skip`, found by scanning
`cli_run.rs`: 52 sites). Milestone/acid tests in scope: `:9048` Hello Goal (the two sites above); `:9101`
`asi_demo_replay_and_audit_commands_work_end_to_end` (same run.sh guard `[INFERENCE: same shape; line from the survey]`);
`:18531` `wasm_aot_runs_and_matches_interp_on_pure_int` (R7 milestone: a "script not found" return; its codegen-unavailable path
already uses `note_harness_skip`). Acid Test 2 (`:3216`, `:3506`) and `:27562` do not have the pattern. Out of scope (toolchain- or
script-optional parity tests, list at `:298, :812, :849, :888, :924, :961, :998, :19321, :19365, :19444, :22183, :34914, :35001`, and
the rest of the 52): log as **O-AF-05**, routing them through `note_harness_skip` so strict mode sees them.

**Fail-first tests** (`cli_run.rs`). `run_sh_hello_goal_fails_on_crashing_binary`: `harness()`-style call of `run.sh hello-goal` with
`AXON` set to a fake binary that panics (rc 101) → run.sh exits nonzero. A second fake that crashes only when `AXON_AI_REPLAY` is
set (the record step succeeds, the replay step panics) → run.sh exits nonzero. Both tests fail today, because run.sh exits 0 in
both cases (the first observed as `hg-fake3.out`). The fakes are small shell scripts that the test writes to a temp dir. `[REVISED: AFD-4]`

**Acceptance.** The milestone tests cannot pass when their prerequisites are missing or the binary crashes.

### AF-6 (P2) Docs truth

**(a) O-HI-03 docs line - the premise is FALSE at a5310878.** A native binary does NOT make live AI calls. Observed with `c1.ax`
(`ai_complete("hi")`) and `g.ax`, both built with the reference binary (default features, no `asi-runtime`):
- `AXON_AI_MOCK=1 ./c1n`: the mock stub, rc 0 (the same text as the interpreter).
- a fake key, no mock: `` `ai_complete` reached a LIVE model call, but this is a NATIVELY BUILT binary ... Refusing to run `` **rc 2**
  (`refuse_unaudited_native_live_ai`, `axon-ai/src/lib.rs:410`, commit 105444e6; all five extract bridges call it at `:443, :1078,
  :1129, :1187, :1232, :1282`).
- no key, no mock: `Err("ANTHROPIC_API_KEY (or AXON_AI_API_KEY) is not set")` returned into the program, rc 0 (Ceilings NH-C10 may change this).

`axon build --help` has no AI line (observed). The doc line to add, in `axon build --help` (the long about) and `spec/axon-for-llms.md`
(AI section): *"A natively built binary never calls a model: under `AXON_AI_MOCK=1` it returns the deterministic stub; with an API key
it refuses with exit 2 (native builds write no `ai_call` audit record - run with `axon run`); with neither, the AI builtin returns
`Err` with the missing-key message."* The last clause is owned by Ceilings CE-8: whichever lands second rewrites it. Stamp O-HI-03:
`[REVISED a5310878: native no longer dials a model - live calls refuse with exit 2 (105444e6, axon-ai/src/lib.rs:410); docs line per
tasks/asi-fixes-spec.md AF-6(a)]`.

**(b) Stale-open entries.** Verified by reading the anchor and checking it with grep at a5310878, plus `git log -S` for a hash where it
was run. Probes on the binary were NOT run for these `[INFERENCE: behaviour inferred from code]`.

| entry (line) | verdict | stamp text |
|---|---|---|
| O008 (234) | PARTIAL | `[REVISED a5310878: fixed - explicit operator re-pin: axon-vm attest --pin-baseline [--repin] (crates/axon-vm/src/main.rs:278-295,891-903). Open - no doc says a kernel rebuild needs a re-pin, and scripts/axon_kernel_gate.sh:114 re-pins on its own]` `[INFERENCE: anchors from a read-only pass, not re-grepped]` |
| O009 (251) | PARTIAL | `[REVISED a5310878: drift caught by a test (crates/axon-core/tests/cli_run.rs:3381-3432; scripts/wasm_parity.sh:105-106). Open - HOST_BUILTINS still hand-written]` `[INFERENCE: not re-grepped]` |
| O016 (520) | FIXED | `[FIXED 35ede229 (AUDIT T24) + e622e1c9 (T44): verdict by exit code + completion marker, never Completed by fallthrough (crates/axon-os/src/runtime.rs:1370-1458); test tests/acceptance.rs:966]` |
| O017 (560) | FIXED | `[FIXED 16d5d48b (AUDIT T23): AXON_* controls forwarded after env_clear (crates/axon-os/src/runtime.rs:903-944)]` |
| O018 (599) | FIXED | `[FIXED 16d5d48b (T23) + T24: AXON_AI_MOCK/REPLAY forwarded (runtime.rs:933-935); exit 5 -> Denied ai-policy (runtime.rs:1368,1435); test tests/sandbox_capture.rs:293-316]` |
| O019 (642) | FIXED | `[FIXED d5afa6c5 (T25, drain child pipes concurrently), bounded by e1127c42: spawn_drain/join_drains (crates/axon-os/src/runtime.rs:408-450); test runtime.rs:1822]` `[INFERENCE: d5afa6c5 is from an earlier log listing, not re-run]` |
| O020 (683) | FIXED | `[FIXED 3aef7eb0: one shared <run>.kill for --killable/--monitor (crates/axon-os/src/cli.rs:433-439); test tests/r27_acceptance.rs:91]` |
| O-RLM-02 (1446) | FIXED | `[FIXED a5310878: const/var foreign_keyword_help at the resolve tier (crates/axon-core/src/resolver.rs:496-497); test tests/parse_help_probe.rs:133]` |
| O-RLM-04 (1470) | FIXED | `[FIXED a5310878: six crates in runtime-crate clippy + S15 coverage stage (scripts/gate.sh:988-1015)]` |
| O-RLM-06 (1492) | PARTIAL | `[REVISED a5310878: wildcard ACAO removed (crates/axon-web/src/server.rs:71-93). Open - no auth/Origin/CSRF; a cross-origin simple POST still executes]` `[INFERENCE: not probed]` |
| O-SESS-01 (1734) | FIXED | `[FIXED a5310878: arr_push is [T], T -> [T] (crates/axon-core/src/builtins.rs:614-616); native any element type AX-44 (CHANGELOG.md:472)]` |
| O-RLM2-03 (1797) | FIXED | `[FIXED a5310878: + concatenates str and the card says so (crates/axon-core/src/card.rs:123); in-place s = s + t AX-31 (CHANGELOG.md:513)]` (8/8 not re-measured) |
| no wrapping_* (2132) | FIXED | `[FIXED a5310878: wrapping_add/sub/mul/div/rem - BUILTINS (builtins.rs:559+), interp (interp/builtins.rs:1775), ExternSig (codegen/builtin_externs.rs:105), __axon_wrapping_* (axon-rt/src/lib.rs:1275)]` |
| RecordingHost/ReplayHost (1919) | FIXED | `[FIXED a5310878: RecordingHost/ReplayHost (crates/axon-core/src/replay.rs:245,493); a miss refuses]` |
| replay-diff (1961) | FIXED | `[FIXED a5310878: diff_journals (replay.rs:971) via axon replay --diff (main.rs:4909)]` |
| `axon replay` verb (1974) | PARTIAL | `[REVISED a5310878: axon replay <journal> [--diff] [--show-values] (main.rs:432,1154). Open - no --verify; no web/deploy integration]` |

None was dropped: every listed entry is at least partly fixed in code. The first commits that introduced each anchor (`git log -S
--reverse -- crates scripts`, observed): O-RLM-02 8363eb97, O-RLM-04 5e6e8f1c (S15 stage), wrapping_* cac9cdf2, replay-diff f3348d48,
RecordingHost/ReplayHost 6741b51c. When stamping, the integrator replaces "a5310878" with these hashes. O-SESS-01 and O-RLM2-03 stay "a5310878".

## 3. Shared mechanics

- Each behaviour change gets a pci theme (`governance/notes/pci-themes/<sha8>.json`, then `python3 tasks/r51-pci-regen.py`, then
  `scripts/pci_delta.py --check`) and a CHANGELOG line. That covers AF-1 (E1304; extracts run; 101→E1300 rc 5 with no runtime and
  no mock), AF-2 (move rules, including the `ra`/`gm` refusals), AF-3 (E0409), AF-4 (records/cost) and AF-5 (run.sh exit codes).
  `[REVISED: rulings 4, 5 - no longer conditional on NH answers]`
- g01 rows and `ATTACK:` markers (`scripts/v022_g01_mutations.py`, `scripts/v022_attack_markers.py`) land in the same commit as the fix.
  Each killing test's assert message carries the marker text.
- Refusal-coverage rows (`scripts/v022_refusal_coverage.py`) for the new refusal sites: E1304 (infer), E0409 (checker), and E1300
  in the extract arms.
- `AXON_REFERENCE.md` is regenerated with `axon reference`. Expected diff: E0409, E1304.
- Test programs are inlined in tests. No test reads `/var/tmp`.

### g01 rows (M4600-M4639; ids used: M4600-M4625, rest unused)

Format as in `v022_g01_mutations.py` (`(id, guard description, file, find, replace, crate, test args, test name)`). Find strings are
the post-fix code the implementer writes, so they are given as intent `[INFERENCE: exact text fixed at build time]`. The crate and
test-args columns follow the registry (`v022_g01_mutations.py:385-434`). `cli` = `--no-default-features --test cli_run`.
`[REVISED: AFD-6 - columns added; AFD-1 M4603; AFD-3 M4623; AFA#1/#2/#3 M4609, M4616, M4624, M4625]`

| id | guard | file | mutation | crate | args | killing test | ATTACK marker |
|---|---|---|---|---|---|---|---|
| M4600 | E1304 out-of-set T | infer.rs | drop the `ai_extract_target(..).is_none()` push | axon-core | cli | ai_extract_out_of_set_type_is_e1304 | `ATTACK: ai_extract::<str> passed check and was dropped natively` |
| M4601 | interp turbofish dispatch | interp/eval.rs | remove the `ai_extract::<` branch | axon-core | cli | ai_extract_generic_runs_under_mock_on_every_engine | `ATTACK: ai_extract::<T> panicked not callable` |
| M4602 | mapping bool | builtins.rs | `bool`→`ai_extract_i64` | axon-core | cli | ai_extract_generic_runs_under_mock_on_every_engine | `ATTACK: ai_extract::<bool> dispatched to the i64 target` |
| M4603 | mock bool value | interp/builtins.rs | `Bool(true)`→`Bool(false)` | axon-core | `--test integration_fixtures` (default features) | ai_extract_generic_native_matches_interp_under_mock | `ATTACK: interp mock differs from native stub` |
| M4604 | net host for hidden names | capabilities.rs | remove the three names from `ai_builtin_host` | axon-core | cli | ai_extract_turbofish_is_net_scoped_in_a_sandbox | `ATTACK: a sandboxed ai_extract::<T> skipped the host check` |
| M4605 | taint class | interp/taint.rs | hidden targets → Unclassified/none | axon-core | `--no-default-features --lib` | taint unit test | `ATTACK: an AI answer carried no world taint` |
| M4606 | ledger kind | interp/builtins.rs | `audit_effect_kind` returns Net for `ai_extract_` | axon-core | cli | ai_extract_turbofish_audits_as_ai | `ATTACK: a model call audited outside the AI class` |
| M4607 | effect_row_name uses mapping | effects.rs | revert to the constant | axon-core | cli | E1310 propagation test (existing M3807 neighbourhood) | `ATTACK: closed-row caller inherited nothing from ai_extract::<T>` |
| M4608 | match snapshot | borrow.rs | arms share state (no reset) | axon-core | cli | match_arms_are_alternatives_for_moves | `ATTACK: cross-arm move refused a valid program` |
| M4609 | live-arm join (Moved wins) | borrow.rs | drop live-arm moves from the join | axon-core | cli | move_hole_probes_stay_refused (hp1) | `ATTACK: a move in a live arm escaped the match` |
| M4610 | escaping jump | borrow.rs | `has_escaping_jump` → false | axon-core | cli | move_hole_probes_stay_refused (hmbr) | `ATTACK: a break-escaping move discarded with its arm` |
| M4611 | nested loop binds break | borrow.rs | count a nested-loop break as escaping | axon-core | cli | match_arms_are_alternatives_for_moves (hminner) | `ATTACK: inner-loop break kept a returning arm live` |
| M4612 | return of borrowed | borrow.rs | drop the Borrowed exemption | axon-core | cli | return_in_arm_or_loop_does_not_move_for_code_after (h3b) | `ATTACK: return xs in for x in xs refused` |
| M4613 | return of moved | borrow.rs | exempt Moved too | axon-core | cli | move_hole_probes_stay_refused (hp5) | `ATTACK: returned a moved value` |
| M4614 | loop body diverges | borrow.rs | never restore the snapshot | axon-core | cli | return_in_arm_or_loop_does_not_move_for_code_after (lp1) | `ATTACK: loop return marked value moved after loop` |
| M4615 | loop restore only on divergence | borrow.rs | always restore | axon-core | cli | move_hole_probes_stay_refused (hp8/br2) | `ATTACK: break move in loop body forgotten` |
| M4616 | guard on the running "tried" state | borrow.rs | walk the guard on a per-arm copy | axon-core | cli | use_after_move_on_one_arm_is_refused (gm) | `ATTACK: guard read a moved value` |
| M4617 | E0409 emit | checker.rs | remove the push | axon-core | cli | bare_variant_in_pattern_is_e0409 | `ATTACK: bare variant name bound as catch-all` |
| M4618 | E0409 nested walk | checker.rs | no recursion into Some/tuple | axon-core | cli | bare_variant_in_pattern_is_e0409 (v5, v9) | `ATTACK: Some(Green) bound instead of compared` |
| M4619 | E0409 subject-enum only | checker.rs | fire for any enum's variant | axon-core | cli | bare_name_of_other_enum_or_non_enum_subject_is_not_e0409 (v4) | `ATTACK: legal binding refused` |
| M4620 | extract provenance | interp/builtins.rs | skip `append_ai_call_jsonl` in extract mock | axon-core | cli | extracts_write_ai_call_provenance_on_mock_and_replay | `ATTACK: extract left no ai_call record` |
| M4621 | extract cost | interp/builtins.rs | no `ai_cost_micro` add | axon-core | cli | ai_cost_spent_counts_extracts | `ATTACK: extract cost uncounted` |
| M4622 | usage tokens | axon-ai/src/lib.rs | return 0 tokens | axon-ai | `--lib` | axon-ai usage parse test | `ATTACK: live extract tokens stored as 0` |
| M4623 | acid gate fails closed | examples/asi/run.sh | drop the new `rc ∉ {0,3} → exit 1` check | axon-core | cli | run_sh_hello_goal_fails_on_crashing_binary | `ATTACK: acid test passed without running` |
| M4624 | `exit` bound by an arm pattern | borrow.rs | drop the pattern-binds-`exit` half of `exit_local` | axon-core | cli | move_hole_probes_stay_refused (ex) | `ATTACK: a closure named exit read as a diverging call` |
| M4625 | join keeps a re-bound name Owned | borrow.rs | start the join from `snapshot` (the draft rule) | axon-core | cli | match_arms_are_alternatives_for_moves (rj, rjd) | `ATTACK: a name re-bound in every arm refused` |

## 4. Gates and cost budget

How the repo measures it: `scripts/gate.sh` runs numbered stages; `gate.sh --lane` selects the stages a change feeds, and its wall
time comes from `tasks/gate-lane-spec.md:255-300` (stage inventory: the parallel block "21 par stages ... wall ≈ 15.3 s"; S03
`cargo test` of the loop/fabric crates 374.1 s + 26.2 s compile). Per-stage numbers below are `[INFERENCE]` from that table and from
the shape of this change. Nothing here was run.

| stage | for | est. wall |
|---|---|---|
| `cargo test -p axon-core --test cli_run <new block>` | AF-1..AF-5 | ~2-4 min incl. compile `[INFERENCE]` |
| `cargo test -p axon-core --lib` (taint_tests, borrow, error codes) | AF-1, AF-2, AF-3 | ~2 min `[INFERENCE]` |
| `cargo test -p axon-ai` | AF-4 | <1 min `[INFERENCE]` |
| corpus check sweep, old vs new binary, 700 `.ax` + fixtures | AF-2, AF-3 | ~2 min (an earlier full `axon check` sweep finished within a 900 s timeout) `[INFERENCE]` |
| `scripts/all_examples_parity.sh` (native) | AF-1 | as in gate `[INFERENCE]` |
| g01 shard for M4600-M4625 | all | 26 rows x one targeted test each; ~10-20 min `[INFERENCE]` |
| full `gate.sh` once at integration | all | per gate-lane-spec |
| full `gate.sh --strict` (runs `--test integration_fixtures`, `gate.sh:1196`, so M4603's killer) | all | ≈ 95 min on this host (integrator figure); 43m24s recorded (`gate.sh:1217`); `parity_all` ≈ 22 min of it. Budget: 2 strict runs (one after the last wave, one retry), about 3.2 h; 3 if the AF fixes must ship before Ceilings wave 4 `[REVISED: AFD-9]` |

No paid calls: every AI path is tested under `AXON_AI_MOCK=1`, replay, or a canned response body.

## 5. Sequencing and lanes

| lane | items | files |
|---|---|---|
| A | AF-1 + AF-4 (one owner: same arms) | `parser.rs` (comment only), `infer.rs:2019-2060`, `effects.rs:994-1006`, `capabilities.rs:155-195`, `builtins.rs` (mapping fn), `interp/eval.rs:634-705,1687-1712`, `interp/builtins.rs:6018-6325`, `interp/taint.rs:711-738`, `ai_routing.rs`, `axon-ai/src/lib.rs:584-612,829-1022,1060-1290`, `codegen/expr.rs:9912-9945` |
| B | AF-2 | `borrow.rs` only |
| C | AF-3 | `checker.rs` (Match `:4745-4760`, while-let), `error.rs` |
| D | AF-5, AF-6 | `tests/cli_run.rs` (one block), `examples/asi/run.sh`, `main.rs` (build long-about), `spec/axon-for-llms.md`, `tasks/opportunities.md` |

The integrator owns `cli_run.rs`, `integration_fixtures.rs`, the g01 registry, CHANGELOG and pci themes (blocks appended at the end).

File-overlap graph: A-B none. A-C none (`infer.rs` vs `checker.rs`). B-C none. D touches only `cli_run.rs`, which the integrator owns
(blocks appended at the end). **Siblings:** Ceilings CE-4/CE-6/`ai_calls` edit `interp/builtins.rs:6036-6215` and CE-8 edits
`axon-ai/src/lib.rs:410-430`. Lane A must land **before** CE-4/CE-6 (they hook the prologue AF-4 creates; their text says so) and
coordinates on `axon-ai` with CE-8 (one owner recommended, as `ceilings-spec.md:964` proposes). LangForms LF-2 edits
`checker.rs:6624-6633` (W0004) and `infer.rs:2356-2365`, and LF-1/3 edit `parser.rs` and `infer.rs:2039-2063` (the `len` special case,
**adjacent to AF-1's `:2019-2060`**). Lane A re-reads `infer.rs` anchors if LangForms lands first. Lane C's Match edit is at
`:4753`, not in LF-2's range. Error codes: E1304 (AF-1, released by Ceilings), E0409 (AF-3; LangForms adds no codes). AF-3's
`bare_variant_in_if_let_is_e0409` needs LangForms LF-2 (its lane B) merged first; the other AF-3 tests do not. LangForms recorded the
same agreement in `tasks/lang-forms-spec.md:800-803`. **`main.rs`:** lane D's long-about edit is also touched by Ceilings CE-2/CE-5
and LangForms LF-4 (its lane C); whichever lands second re-reads the anchors. LF-1 edits `interp/eval.rs:2920`, a function disjoint
from lane A's `:634-705,1687-1712`. `[REVISED: AFD-7]`

**Combined batch order** (DevloopAF's plan, adopted) `[REVISED: AFD-8, integrator ruling 9]`:
1. Wave 1, in parallel with no shared windows: AF lane B (`borrow.rs`), AF lane D (AF-5/6, run.sh), LF lane C (LF-4), Ceilings batch 0 (CE-1, CE-7).
2. Wave 2: LF lane A, then LF lane B (`parser.rs`, `fmt.rs`, `parse_help.rs`, in that order).
3. Wave 3: AF lane C (AF-3; its if-let test needs LF-2) in parallel with AF lane A (AF-1 + AF-4). One owner also does CE-8,
   because both edit `axon-ai`. Lane A re-reads the `infer.rs` anchors after LF lane A.
4. Wave 4, one owner, in series: CE-2, CE-10, CE-3, then CE-4 and CE-6 + `ai_calls` (on AF-4's prologue), then CE-5.

Per lane: the item's `cli_run` killers, `gate.sh --lane`, its `integration_fixtures` test by name (LLVM), and its g01 rows. The
AF-2/AF-3 corpus sweep and the `if let` twins run after wave 3 (LF-2 merged). One `gate.sh --strict` runs after wave 4, plus one
budgeted retry.

## 6. Engineering decisions (decided)

1. The T set is enforced at check (E1304), not in codegen. One mapping fn serves infer, interp, effects and codegen.
2. The flat extract targets stay hidden (not in `BUILTINS`). Tables that need them get explicit entries; taint gives them World.
3. The turbofish dispatch reuses `dispatch_named`'s builtin block through an extracted helper. The `call_builtin(` site count is unchanged.
4. Match arms are alternatives. An arm is discarded only if it diverges syntactically and has no escaping `break`/`continue`. `If` is untouched.
5. A `return` of a Borrowed owner is not a move error.
6. E0409 fires only for a variant of the subject's own enum, in refutable positions, in every arm position including the last
   arm; shorthand struct fields and `__`-prefixed synthetic names are skipped. (Former NH-4: Rust's E0170 is deny-by-default, the
   fix is the mechanical `E::V`, 0 tracked hits.) `[REVISED: ruling 7, AFA#6]`
7. Extract records name the decision tier, which `ai_routing::decision_tier()` computes with the same precedence as
   `axon_ai::decision_model()`. Both sides are pinned to the literal `claude-haiku-5-5` default by tests in the default build. `[REVISED: AFD-2]`
8. The acid gate fails closed. `run.sh hello-goal` checks every step's rc, and the record rc must equal the replay rc and both
   be 3. `[REVISED: AFD-4]`
9. **Extracts with no AI runtime and no mock give E1300, rc 5**, matching `ai_complete`. A panic (101) is a crash, not a refusal,
   and axon-os maps rc 5 to the ai-policy axis. (Former NH-1.) `[REVISED: ruling 5]`
10. **`mv4/ra.ax` is refused (E0601).** It is a real use-after-move that only passes because the interpreter shares values; no sound
    rule accepts it without also accepting `rb`/`rif`. The same holds for `gm.ax` (a guard's move). (Former NH-2.) `[REVISED: ruling 4, AFA#3, AFA#4]`
11. **O-AF-01 (loop-carried moves) and O-AF-02 (a break escaping a tail-diverging `if`) stay open and documented.** Closing them
    refuses `br1`, `loopmove` and `hmcont_if`, which run today with right answers. (Former NH-3.) `[REVISED: ruling 6]`
12. The match join is per name over the live arms (Moved wins; otherwise the shared state). `exit_local` also counts an `exit`
    bound by the arm or loop pattern. Guards walk on a running "tried" state. `[REVISED: AFA#1, #2, #3]`

## 7. Needs-human

None. `[REVISED: integrator ruling 10 - former NH-1..NH-4 are decisions 9, 10, 11 and 6 above, with their reasons; their options
and recommendations are in the Step 1 review table]`
