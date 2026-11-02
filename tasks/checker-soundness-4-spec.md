# checker-soundness-4 - spec (CS4-1..CS4-5)

**Status: DRAFT (after Step 1 review). Not authorized for build.**

### Step 1 review

Reviews: `agent://AuditCS4` (CS4A-1..8) and `agent://DevloopCS4` (CS4D-1..4), plus integrator rulings 1-6. I checked every finding
before accepting it, either by re-running the reviewer's probe with the reference binary (`/var/tmp/r51-tmp/CS4A/`) or by reading the
cited line. **Accepted 12, rejected 0.**

| finding | severity | disposition | where |
|---|---|---|---|
| CS4A-1: the operator rule silently changes pattern strings (`patswallow` prints `1\|2` today) | blocker | accepted (ruling 1). A string read with a nested slot string becomes a separate token, `Token::SlotStr`. Only the expression-position parse accepts it; every other consumer refuses it (E0000). `patswallow` is pinned as a refusal. A narrower lexer-only rule cannot exist (argued under CS4-2 "Compatibility") | CS4-2 steps 1, 1a; D11; M4712 |
| CS4A-2: fmt's parenthesize rule fails for a string after a keyword (`fmtkw`) | blocker | accepted (ruling 2). fmt escapes slot text (`\` -> `\\`, `"` -> `\"`), the inverse of `unescape`. The parenthesize rule is dropped, D5 is rewritten, and M4711 is re-pointed | CS4-2 step 4; D4; D5 |
| CS4A-3: early returns in the Call arm leak `pending_calls`, which refuses `c_sib` and `c_seq` | blocker | accepted (ruling 3). The pending window becomes a wrapper around a new `infer_call`. `c_seq` is a control row and the trace is corrected | CS4-4; D7; M4718 |
| CS4A-4: constraining an unresolved subject refuses `l1`, which runs today | blocker | accepted (ruling 3). Only a concrete subject is constrained. `l1` is a control row | CS4-3; M4715 |
| CS4A-5: the jump block keeps the `ifbrk_ok` false refusal (its match twin `mbrk_ok` is rc 0) | major | accepted (ruling 3). The jump block skips a branch only when its escaping jump is not its final statement. `ifbrk_ok` is a fix row | CS4-1 step 3; M4703 |
| CS4A-6: the CS4-5 check sits after the wildcard `return` and never runs on `v2q_noall` | major | accepted (ruling 4). The check moves before the return, next to E0405, and the anchors are fixed | CS4-5 |
| CS4A-7: `loopinif` also flips to rc 0 | minor | accepted. It goes into D8 (was NH-1), O-CS4-03 and a pin row | CS4-1 table and tests; D8 |
| CS4A-8: wrong anchors | minor | accepted. `lsp.rs:72`, `parser.rs:3925-3960`, `parser.rs:2485`. The four `opportunities.md` anchors were right at b3a9c56e (`git show b3a9c56e:tasks/opportunities.md`: 3317/3180/3325/3304) and moved down one line on b2/int (the reviewer read 56612031; now f8afd239). Both are given | §0; CS4-2; CS4-5 |
| CS4D-1: `--only M47xx` does not filter | major | accepted: `--only=` (`v022_g01_mutations.py:8784` read) | §3 |
| CS4D-2: b2/ce also edits `spec/axon-for-llms.md` and `lib.rs:71` | major | accepted (`git diff --stat 56612031 b2/ce`: 22 lines in that file, 1 in `lib.rs`) | §6 |
| CS4D-3: `doc_snippets` checks the edited doc blocks | minor | accepted (`doc_snippets.rs:33-37` read) | CS4-2 acceptance; §3 |
| CS4D-4: no critical path | minor | accepted | §5 |

Re-observed with the reference binary (`run.sh`, in `/var/tmp/r51-tmp/CS4A/`):
- `patswallow` rc 0, prints `1|2`. `fmtkw` rc 0, prints `1|1|3`. `fmtkw2` rc 2, E0000:4.
- `c_seq` rc 0, prints `7`. `l1` rc 0, prints `2|3`. `l4` rc 2, E0102:2.
- `ifbrk_ok` rc 2, E0601 at :7 and :10. `mbrk_ok` rc 0, prints `b|ax`.
- `loopinif` rc 2, E0601:5. `mloopinarm` rc 0, prints `aa`.
- `en` rc 0, prints `1|1`, on both the interpreter and `AXON_ENGINE=vm`.

Lines read: `infer.rs:2097`, `:2112-2138` and `:2273` (the early `return`s in the Call arm); `checker.rs:6612`, `:6776-6782` and
`:6812-6828`; `parser.rs:3795`, `:3925-3927`, `:2485` and `:3911-3913` (a pattern accepts `Token::Str` only, never `RawStr`).

My own probes are in `Soundness4/rv/`. `m4716.ax` (the M4717 killer variant; the file keeps the first draft's id) is rc 0 and prints
`7`. `patguard.ax`, the guard spelling the CS4-2 refusal suggests, is rc 0 and prints `1|2`.

Integrator ruling 5 makes NH-1..NH-3 engineering decisions (now D8-D10). NH-4 stays with the human, because the revised items still
refuse programs that run today (§8).

Batch 2. Four entries from `tasks/opportunities.md` (O-AF-07, O-HX3-04, O-LF-01, O-CS3-05), plus one optional P3 item (O-AF-04) taken from
the scan of O-AF-02..08 and O-LF-02..04. Every line anchor was read at `b3a9c56e` (branch `b2/int`, worktree
`/home/cklaus/projects/axon-lanes/b2`). `b3a9c56e` is `ebbb4e02` plus task docs only (`git diff --stat ebbb4e02 b3a9c56e`: 3 `.md`
files), so the reference binary `/var/tmp/r51-tmp/b2-bin/axon` (`axon 0.1.0 (ebbb4e02)`, default features) is the base for every
reproduction. Probe files are in `/var/tmp/r51-tmp/Soundness4/` (`run.sh` prints check rc, the first error code:line, run rc, stdout and
the panic line). No cargo build or test was run. Anything I did not observe is marked `[INFERENCE]`. `[REVISED: Step 1 review]` b2/int
is now `f8afd239`, and `git diff --stat b3a9c56e f8afd239 -- crates/axon-core spec/grammar.ebnf spec/axon-for-llms.md
spec/language-tour.md` is empty, so every source anchor below holds there too. The `tasks/opportunities.md` anchors move down one
line, and both values are given.

Worst-outcome check, per item:
- CS4-1 removes false refusals (E0601) and adds one named refusal.
- CS4-2 removes false refusals (E0000). It fixes a **silent wrong answer in `axon fmt`**, found while checking the round trip. It turns
  one more silent wrong answer, which the first draft would have caused, into a refusal (`patswallow`, CS4A-1).
- CS4-3 and CS4-4 turn programs that check clean and then panic into check refusals.
- CS4-5 turns a silently dead arm into a refusal.

`[REVISED: CS4A-1..4]` The full list of programs that run today and become refusals is in §8 "Refusals of running programs". None
of them is a tracked file.

## 0. Source entries

| item | entry (tasks/opportunities.md) | sev |
|---|---|---|
| CS4-1 | O-AF-07 (`:3317`; `:3318` at f8afd239 `[REVISED: CS4A-8]`) - the `if`/`else` twin of a returning match arm is still E0601 | medium |
| CS4-2 | O-HX3-04 (`:3180`; `:3181` at f8afd239) - `"hi {name + "!"}"` is E0000 against grammar.ebnf §6.12; plus a new finding: `axon fmt` drops `{{`/`}}` escapes and raw-string markers (silent wrong answer, §CS4-2 "fmt") | medium (+ new, high) |
| CS4-3 | O-LF-01 (`:3325`; `:3326` at f8afd239) - tuple patterns are unchecked in `match` and `if let` | medium |
| CS4-4 | O-CS3-05 (`:3095`, unchanged at f8afd239) - a wrapper lambda's call nested in another call's arguments derives no kind | medium |
| CS4-5 (P3, optional) | O-AF-04 (`:3304`; `:3305` at f8afd239) - a qualified pattern of a different enum is accepted and never matches | low |

### Out of scope (logged, not dropped)

| what | why it is not here | where it lives |
|---|---|---|
| O-AF-01 loop-carried moves (`mv3/loopmove.ax`, `mv3/hmcont_if.ax`, both rc 0) | Human decision 2026-11-02: stays open. CS4-1 leaves both rc 0 (keep rows) | O-AF-01 |
| O-AF-02 a move escaping a tail-diverging `if` through `break` (`mv2/br1.ax` rc 0, prints `7`) | asi-fixes decision §6.11. CS4-1 keeps the Moved-union rule that holds this hole exactly as it is, and adds no new path through it (keep rows br1, brrb, ibrk) | O-AF-02 |
| O-AF-03 `return` inside `spawn { }` | Semantics unspecified; not a checker item | O-AF-03 |
| O-AF-05 `cli_run.rs` early-return "skipping" tests | Test-harness work, no checker change | O-AF-05 |
| O-AF-06 pattern-bound `exit` closure: engines disagree | Codegen (native) divergence. CS4-1 follows the interpreter, as AF-2 did | O-AF-06 |
| O-AF-08 E0409 points at the statement, not the arm | Needs a span on `MatchArm` (AST change touching every consumer incl. AX-10 JSON); not small | O-AF-08 |
| O-LF-02 `let (a, b) = p` with a 3-tuple is accepted | Different mechanism: the parser desugars the tuple `let` into `.0`/`.1` field lets (`__tup_N`, `parser.rs:2485`; `[REVISED: CS4A-8]` was `:2350-2387`), not into a pattern. LF-3 documents it. After CS4-3 it is the only tuple-arity site left unchecked; a follow-up could desugar to a checked tuple bind | O-LF-02 |
| O-LF-03 axon-os jobs get no sibling-module search; O-LF-04 no module-not-found line under `AXON_PATH_EXCLUSIVE=1` | Module loading, not checker soundness | O-LF-03, O-LF-04 |
| NEW: a nested block's `let` that shadows a moved name erases the move (`af7c/shb.ax`, `af7c/shm.ax`: `let t = s; { let s = "x" ... }; s + t` checks rc 0). `pop_scope` (`borrow.rs:244-254`) removes the shadowing name instead of restoring the outer state | Fixing it refuses programs that run today (same class as O-AF-01). CS4-1 does NOT copy it into `if` (keep row `shi`) | to log as **O-CS4-01** at integration (D10, was NH-3) |
| NEW: a `match` arm that leaves by `break` with a name moved, then re-binds it, is joined as re-bound (`af7b/mbrk.ax` rc 0; its `if` twin `ibrk` is E0601) | O-AF-02 family inside AF-2's match join; closing it refuses a running program (decision §6.11 class) | to log as **O-CS4-02** (D10, was NH-3) |
| NEW: a loop body that re-binds a moved name is taken as definite, though it may run zero times (`af7b/lz.ax`, `af7b/wz.ax`, `af7b/mrjloop.ax`, `CS4A/mloopinarm.ax` rc 0) | Loop rule (`check_loop_body`, `borrow.rs:569-585`), O-AF-01 family. CS4-1 makes `af7b/rjloop.ax` and `CS4A/loopinif.ax` follow the same rule (D8; `[REVISED: CS4A-7]` loopinif added) | to log as **O-CS4-03** (D8) |
| NEW: literal sub-patterns are not type-checked (`match (1, 2) { ("a", y) => y, (x, _) => x }` `lf/t_lit_ty.ax` rc 0 prints `1`; `Some("a")` on `Option<i64>` `lf/s_some_lit.ax` rc 0) | E0405 is a checker rule for a top-level literal arm on a scalar subject (`checker.rs:6770-6818`); a nested rule is a separate design | to log as **O-CS4-04** |
| CS3-2 handler residual: `let g = \|y\| { println("pre"); f(y) }` (`cs/w17f.ax`) checks rc 0, prints `pre`, panics | An earlier call may be answered by an aborting handler, so `f(y)` may never run; deriving here would refuse `c_sib`-shaped programs. By design of CS3-2 | stays in O-CS3-05 text as the documented residual |

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P0 | CS4-1 | False refusal (E0601) of valid programs that run today; the `match` twin already checks clean |
| P0 | CS4-2 (fmt half) | `axon fmt` changes what a program prints (silent wrong answer) |
| P1 | CS4-2 (lexer half), CS4-3, CS4-4 | CS4-2: false refusal against the grammar. CS4-3/CS4-4: check rc 0 then a runtime panic (a claimed property - "checked programs do not hit type panics" - not enforced) |
| P3 (optional) | CS4-5 | A dead arm accepted silently; low severity, small, same mechanism as CS4-3 (pattern vs subject type) |

## 2. Items

### CS4-1 (P0) The `if` rule joins live branches like the `match` rule (O-AF-07)

**Anchors (b3a9c56e, `crates/axon-core/src/borrow.rs`).**
- `:397-428` `Expr::If`: snapshot, each branch from the snapshot, then `self.bindings = snapshot` (`:413`) and only names a live branch
  left `Moved` are inserted (`:423-427`). A name that was `Moved` before the `if` and is re-bound in every live branch stays `Moved`.
- `:414` `exit_local = self.bindings.contains_key("exit")` - unlike the match rule (`:597`) and the loop rule (`:570`), it ignores
  `self.exit_bound`, so an `exit` bound by an enclosing arm pattern is read as the diverging builtin.
- `:595-644` `check_match_arms` (AF-2): per-name join of live arms with `join_state` (`:156-164`); liveness is
  `!branch_diverges(..) || has_escaping_jump(..)` (`:612-613`).
- `:86-151` `branch_diverges`, `stmts_diverge` (`:104`), `has_escaping_jump` (`:117`), `loop_body_diverges` (`:149`).
- `:244-254` `pop_scope` removes a scope's names (no restore of a shadowed outer state; see O-CS4-01).

**Reproduction** (reference binary; `./run.sh <file>`; probe sources in `af7/`, `af7b/`, `af7c/`).

| probe | shape (after `let t = s` / `let q = p` moved the name) | check rc | run | class |
|---|---|---|---|---|
| `af7/rjif.ax` (= `AFA/rjif.ax`) | `if k == 0 { s = "zero" } else { s = "other" }; s + t` | 2, E0601 at 5:5 "use of moved value `s`" | rc 2 (run checks first) | fix |
| `af7/rjdif.ax` | `if k == 0 { return t } else { s = "other" }` | 2, E0601:4 | - | fix |
| `af7/rjxif.ax` | `if k == 0 { println(t) exit(0) } else { s = "other" }` | 2, E0601:4 | - | fix |
| `af7/rjelif.ax` | `if .. { s = "zero" } else if .. { s = "one" } else { s = "other" }` | 2, E0601:4 | - | fix |
| `af7/rjval.ax` | `let u = if k == 0 { s = "zero"; 1 } else { s = "other"; 2 }` | 2, E0601:4 | - | fix |
| `af7/rjstruct.ax` | struct `p` re-bound in both branches | 2, E0601:4 | - | fix |
| `af7b/rjnest.ax` | nested `if` inside `then`, all paths re-bind | 2, E0601:4 | - | fix |
| `af7b/rjloop.ax` | `for x in xs { if x == 0 { s = "zero" } else { s = "other" } }` | 2, E0601:6 | - | changes to rc 0 (D8, was NH-1) |
| `CS4A/loopinif.ax` `[REVISED: CS4A-7]` | `if k == 0 { for x in xs { s = "z" } } else { s = "o" }`, called with `[]` | 2, E0601:5 | - | changes to rc 0 (D8). Its `match` twin `CS4A/mloopinarm.ax` is rc 0 today, prints `aa` |
| `af7/rjiflet.ax`, `af7/rjdiflet.ax`, `AFA/rj.ax`, `AFA/rjd.ax` | the `if let` / `match` twins | 0 (`zeroa\|othera`, `a\|othera`, ...) | rc 0 | control (unchanged) |
| `af7/rjnoelse.ax`, `af7/rjelif_noelse.ax` | no final `else`: the implicit path keeps `s` moved | 2, E0601:4 | - | keep |
| `af7/rjone.ax` | only `then` re-binds | 2, E0601:4 | - | keep |
| `af7b/rjref.ax`, `af7b/rjtwo.ax` | re-bind `s` in both, but `t`/`r` moved in one branch | 2, E0601:4 (x2) / E0601:5 (x2) | - | keep code+line (one diagnostic instead of two) |
| `af7c/shi.ax` | both branches `let s = ..` (shadow); outer `s` stays moved | 2, E0601:4 | - | keep (do not copy O-CS4-01) |
| `af7b/ibrk.ax` | `while .. { if c { if i == 0 { break } s = "a" } else { s = "b" } }` - `break` leaves with `s` moved | 2, E0601:8 | - | keep (O-AF-02 not worse) |
| `af7/brrb.ax` | `if c { if i == 0 { break } return 1 } else { p = P { x: 9 } }` in a loop | 2, E0601:14 | - | keep (O-AF-02 not worse) |
| `CS4A/ifbrk_ok.ax` `[REVISED: CS4A-5]` | in a `while`: `if i == k { s = "a"; break } else { s = "b" }`, then `println(s)`, and `s + t` after the loop | 2, E0601:7 and E0601:10 | - | **fix**: rc 0, prints `b\|ax`. Its `match` twin `CS4A/mbrk_ok.ax` is rc 0 today with that output |
| `AsiFixes/mv2/br1.ax` | O-AF-02 reproducer | 0, prints `7` | rc 0 | keep rc 0 (decision §6.11) |
| `AsiFixes/mv3/loopmove.ax`, `mv3/hmcont_if.ax` | O-AF-01 | 0 (`14`, `8`) | rc 0 | keep rc 0 (human decision) |
| `af7/exlocif.ax`, `af7/exforif.ax` | `exit` is a local / `for`-bound closure: `if k == 0 { let t = name; exit(3) } else { 0 }`, then `name` | 2, E0601:5 / E0601:6 | - | keep |
| `af7/exarmif.ax` | same, `exit` bound by a match-arm pattern `Some(exit) =>` | **0**, prints `arm 4\|a` (also `AXON_ENGINE=vm`) | rc 0 | **new refusal** E0601 (AF-2 already gives E0601 for the arm twin `ex`; D9, was NH-2) |

**Fix design** (only `Expr::If`, `:397-428`; `check_match_arms` and the loop rule are unchanged).
1. `exit_local = self.exit_bound > 0 || self.bindings.contains_key("exit")` (the match rule's `:597`). This is the `exarmif` refusal.
2. Keep step (1) of today's rule unchanged: every name a syntactically live branch (`!branch_diverges`) ends with `Moved` is `Moved`
   after the `if`. This keeps O-AF-02 exactly as it is (br1 stays rc 0) - a branch is NOT made live by an escaping jump, unlike match.
3. New re-bind join, for each name that is `Moved` in the snapshot:
   - skip it when either branch has an escaping jump (`has_escaping_jump`) **that is not that branch's final statement**: a
     `break`/`continue` reached mid-branch can carry the moved state past the loop (ibrk, brrb). `[REVISED: CS4A-5]` A branch whose
     block's last statement is the `break`/`continue` itself, with no other escaping jump in its earlier statements, does not skip.
     Its end state is the state at that jump, so it joins like any other live branch (ifbrk_ok);
   - skip it when no branch is live;
   - for each live branch take its end state, with a name absent from the end map read as the snapshot state (`Moved`), so a branch
     whose block popped a shadowing `let` never re-binds (shi). A missing `else` is a live branch whose end state is the snapshot;
   - if no live end state is `Moved`, the name's state after the `if` is the `join_state` fold of the live end states.
4. Nothing else changes: names not moved before the `if` follow step 2 as today.

Why this does not make O-AF-02 worse: step 3 only ever lowers a `Moved` name. It does so only when every path out of the `if` ends
with the name re-bound. That covers live branches, and a branch that ends in `break`/`continue` (at that jump the state equals the
branch's end state, so the `break` path also carries the re-bound name). Branches that leave by `return`/`exit`/`?`-style divergence
never reach the code after the `if`. A `break`/`continue` reached before the branch's end, which is what O-AF-02 is about (ibrk,
brrb), still blocks step 3 entirely. Names not moved before the `if` (br1's `p`) never enter step 3.
Why it does not reopen O-AF-01: the loop rule (`:569-585`) is untouched. `rjloop` and `loopinif` change from 2 to 0 because the loop
rule already takes a body's end state as the state after the loop (`lz`, `wz`, `mrjloop`, `mloopinarm` are rc 0 today). That
zero-iteration acceptance is logged as O-CS4-03 (D8, was NH-1).

**Alternatives considered.**
- (a) Reuse `check_match_arms` verbatim (liveness `|| has_escaping_jump`). This refuses `br1`, closing O-AF-02 against decision
  §6.11, and it joins shadow-popped names as absent, which accepts `shi`. Rejected.
- (b) Syntactic liveness only, with no jump block. This accepts `ibrk` and `brrb` (E0601 today, and a real use-after-move on the `break`
  path), widening O-AF-02. Rejected.
- (b') `[REVISED: CS4A-5]` The first draft's block on any escaping jump. This keeps `ifbrk_ok` refused although its `match` twin
  passes. Rejected.
- (c) Desugar `if` to `match` in the borrow walk. Same result as (a). Rejected.

**Fail-first tests** (`crates/axon-core/tests/cli_run.rs`, appended; programs inlined; `--no-default-features --test cli_run`):
- `if_branches_are_alternatives_for_moves` - fix rows: rjif, rjdif, rjxif (on the `exit(0)` path, assert the stdout of the k=1 call
  only), rjelif, rjval, rjstruct, rjnest and `[REVISED: CS4A-5]` ifbrk_ok (`b`, then `ax`). Each checks rc 0 and runs with the
  interpreter's output. Fails on base (E0601). Marker `ATTACK: a re-bind in every if branch was refused`.
- `if_join_keeps_real_moves` - keep rows: rjnoelse, rjelif_noelse, rjone, rjref, shi, ibrk, brrb, exlocif, exforif, exarmif. Each checks
  rc 2 with code E0601 at the line in the table. Passes on base except exarmif, which fails on base (rc 0). Marker
  `ATTACK: an if join dropped a move`.
- `if_join_leaves_o_af_01_and_o_af_02_open` - br1, loopmove and hmcont_if stay check rc 0 with their outputs (`7`, `14`, `8`), which
  pins the human decisions. `[REVISED: CS4A-7]` Also pins rjloop and loopinif at rc 0 (`aa` for loopinif), which records D8's accepted
  hole. Those two rows fail on base (E0601); the other three pass on base and after.

**g01 rows** (§4): M4700-M4704. `[REVISED: CS4A-5]` M4703 is now the final-jump exception. The first draft's `exit_bound` row is
dropped to stay inside the id block; `exarmif` stays in `if_join_keeps_real_moves` (§4).

**Acceptance.** All three tests pass; the sweep (§3) shows no rc 0 -> nonzero file; `match_arms_are_alternatives_for_moves` and
`return_in_arm_or_loop_does_not_move_for_code_after` (`cli_run.rs:45225`, `:45270`) unchanged.

### CS4-2 (P0 fmt / P1 lexer) A string literal inside an interpolation slot; `axon fmt` keeps brace escapes (O-HX3-04 + new)

**Anchors (b3a9c56e).**
- `crates/axon-core/src/token.rs:200-201` `#[token("\"", lex_str)] Str(String)`; `:262-285` `lex_str`: the first unescaped `"` ends the
  token; `:271` returns `unescape(body)` for the whole body; `:289-312` `unescape`.
- `crates/axon-core/src/parser.rs:122-300` `parse_fmt_str_raw`: `{{`/`}}` handling `:155-180`; slot close found by brace depth only
  (`:185-201`, string-unaware); slot re-lexed by `parse_fmt_inner_expr` (`:241-300`, `Lexer::tokenize(inner)` `:252`).
  `:3554-3563` an expression-position `Str` goes through `parse_fmt_str_raw`; `:3565-3570` `RawStr` becomes `Literal::Str` untouched;
  `:3911-3913` a PATTERN-position `Str` becomes `Literal::Str(s)` with no brace processing (so `"{"` is a valid pattern today; a
  pattern accepts `Token::Str` only, never `RawStr`). Other `Token::Str` consumers with no brace processing: `:991` `expect_str`,
  `:1077`, `:2734` (asm template), `:3282`.
- `crates/axon-core/src/lexer.rs:19-55` `tokenize_with_newlines` / `tokenize` wrap the logos lexer; `token.rs:244` is `Token`'s `Display`.
- `spec/grammar.ebnf:364-406` §6.12: "A `{...}` slot holds ANY expression" (`:376`).
- `crates/axon-core/src/fmt.rs:928-950` `Expr::FmtStr`: a literal part's `{` is written as `{` (`:938`) and `}` unchanged; an `Expr`
  part is emitted with `emit_expr`, whose string literals carry bare `"`. `:991-1003` `Literal::Str` is written with `"`/`\\`/`\n`/`\t`
  escapes only, no brace doubling, and a raw string (`Literal::Str` from `RawStr`) is written as a plain string. `:1016`
  `Pattern::Literal` reuses `emit_literal`.
- Source scanners that toggle "in string" on every unescaped `"`: `lib.rs:1883-1925` `empty_block_binding_offsets` (W0008),
  `lib.rs:1930-1960` `floor_division_comment_offsets` (W0007, used at `main.rs:8447-8460`), `main.rs:6539-6560` `source_has_comments`
  (fmt's comment refusal, `:6594`).
- LSP (`lsp.rs:72` `analyse_source`; `[REVISED: CS4A-8]` was `:272`, which is `run_lsp`) and native codegen consume the parsed AST;
  neither re-scans string text (grep of `lsp.rs` for
  `'"'` finds only JSON escaping). No editor grammar is tracked (`git ls-files` has no tmLanguage/tree-sitter/vim file).

**Reproduction (lexer).**

| probe | source line | check | run |
|---|---|---|---|
| `hx/q6a.ax` | `println("hi {name + "!"}")` | rc 2, E0000 3:26 "this string ends at the `"` before `!` - write a `"` inside a string as `\"`; ..." | rc 2 |
| `hx/q6m_esc.ax` | `println("hi {name + \"!\"}")` (escaped workaround) | rc 0 | rc 0, `hi bo!`; native `axon build` + binary: `hi bo!`, rc 0 |
| `hx2/esc_brace.ax` | `println("a{id(\"}\")}b")` | rc 2, E0000 3:28 "`{id("}`: not an expression ... double it: `{{id("}}`" (the slot matcher counts the `}` inside the quoted arg) | rc 2 |
| `hx2/esc_if.ax` | `println("{if n > 1 { \"many\" } else { \"one\" }}")` | rc 0 | `many` |
| `hx/q6o_pat.ax` | `match c { "{" => 1, "}" => 2, _ => 0 }` with `f("{{")`, `f("}")` | rc 0 | `1\|2` (a lone `{` is valid in a pattern) |
| `hx/q6n_lone.ax` | `let s = "{"` | rc 2, E0000 "unclosed `{` in interpolated string" | rc 2 |

**Reproduction (fmt; new finding).** Copies in `/var/tmp/r51-tmp/Soundness4/fmt3/` (originals `fmt2/*.orig.ax`):

| file | before `axon fmt` | `axon fmt` rc | after |
|---|---|---|---|
| `b.ax`: `println("set {{n}}")`, `println(r"raw {n}")`, `n = 1` | run rc 0: `set {n}` / `raw {n}` | 0 | source `println("set {n}")`, `println("raw {n}")`; run rc 0 prints `set 1` / `raw 1` - **silent wrong answer** |
| `e.ax`: `println("hi {name + \"!\"}")` | rc 0 `hi bo!` | 0 | `println("hi {name + "!"}")`: run rc 2, E0000 3:26 |
| `p.ax`: `println(to_str(f("{{")))` | rc 0 `1` | 0 | `f("{")`: run rc 2, E0000 "unclosed `{`" |
| `CS4A/fmtkw.ax` `[REVISED: CS4A-2]`: `"{match s { \"a\" => 1, _ => 2 }}"`, `"{if \"a\" == s { 1 } else { 2 }}"`, `"{match \"a\" { _ => 3 }}"` | rc 0 `1\|1\|3` | 0 | `CS4A/fmtkw2.ax` (bare quotes, multi-line slots): run rc 2, E0000 4:10 |

Corpus exposure (measured): of 700 tracked `.ax` files, 48 contain `{{`, `\"` or a raw string with a brace; `axon fmt` succeeds on 2 of
them (the rest are refused for comments) and changes no brace line in either (`headroom/programs-c2/shifts.ax`, `examples/escapes.ax`:
run output identical). So no tracked file has been damaged, but any user running `axon fmt` on such a file without comments is.

**Fix design.**
1. **One shared string scanner** (new `pub(crate) fn scan_str_body(src: &str) -> Result<(usize /*len incl. closing quote*/, String
   /*value*/), usize /*error offset*/>` in `token.rs`, iterative with an explicit stack, no recursion - compilebench AX-65), used by
   `lex_str`, by the slot matcher in `parse_fmt_str_raw`, and by the three source scanners. Rules:
   - Outside a slot: as today (`\` escapes the next non-newline char; `"` ends the string). `{{` and `}}` are text. A single `{` opens
     a slot (depth 1); a lone `}` is text.
   - Inside a slot: `{` / `}` adjust the depth; depth 0 closes the slot. A `"` **starts a nested string literal** when the slot text
     before it, trimmed, is non-empty and ends with one of `( [ { , ; + - * / % = < > ! & | ? :`; otherwise the `"` ends the
     outer string exactly as today. A nested string is scanned by the same rules (its own slots, its own escapes); `r"` inside a slot
     starts a nested raw string under the same operator condition. `\"` inside a slot keeps today's meaning (an escaped quote).
   - Fallback: if the extended scan reaches EOF without closing the outer string, the token is re-scanned with today's rule (first
     `"` ends it), so every token that lexes today and does not complete under the new rule lexes the same.
1a. `[REVISED: CS4A-1, ruling 1]` **A string read with a nested string is a separate token.** If the scan of a string opened no
   nested string, the lexer emits `Token::Str(value)`, byte-identical to today. If it opened at least one, the lexer emits a new
   `Token::SlotStr(value)`. The builder chooses the mechanism: a logos `extras` flag set by `lex_str`, or the `lexer.rs` wrapper re-running
   `scan_str_body` on the token's span and reading its `nested` result.
   - Only the expression-position parse (`parser.rs:3554-3563`) accepts `SlotStr`; it treats it exactly like `Str`.
   - The pattern parse (`:3911`) gets an explicit `SlotStr` arm. It gives E0000 "this string has an interpolation slot holding a
     string (`{` ... `"`), and a pattern cannot hold one. To match text that starts with a brace, use a guard: `s if s == "{{...}"`".
     That spelling is verified: `Soundness4/rv/patguard.ax` (`c2 if c2 == "{{("`) is rc 0 today and prints `1|2`, and it lexes
     the same after the change because `{{` is text.
   - Every other consumer (`expect_str :991`, `:1077`, asm `:2734`, `:3282`) matches `Token::Str` only, so it rejects `SlotStr` through
     its existing unexpected-token error. This fails closed: a consumer added later cannot take the extended reading silently.
   - `Token`'s `Display` (`token.rs:244`) and `refuse_adjacent_after_string` (`parser.rs:3471`) get a `SlotStr` arm that prints like `Str`.
2. **Value.** The token value is the body with escapes processed in the literal parts and in slot text outside nested strings (today's
   `unescape`), while each nested string's text is copied verbatim (quotes and escapes kept) so that `parse_fmt_inner_expr`'s re-lex
   reads it. `"hi {name + "!"}"` and `"hi {name + \"!\"}"` therefore produce the same value and the same AST.
3. **Slot matcher** (`parser.rs:185-201`): replace the depth loop with the shared scanner's slot-close, which skips nested strings, so
   `"a{id("}")}b"` works and so does the old escaped `"a{id(\"}\")}b"` (whose value holds `id("}")`).
4. **fmt** (`fmt.rs`). `[REVISED: CS4A-2, ruling 2]` The rules:
   - In `FmtPart::Lit`, write `{` as `{{` and `}` as `}}`.
   - In an expression-position `Literal::Str` (a value), write `{`/`}` doubled.
   - A `Pattern::Literal(Literal::Str)` keeps its braces single, because pattern strings get no brace processing (`parser.rs:3911`).
     So `emit_literal` gets a `pattern: bool`.
   - An `Expr` part is emitted into a buffer by `emit_expr` and then written with `\` -> `\\` and `"` -> `\"`. That is exactly the
     inverse of `unescape` (`token.rs:289`) for those two characters, so the written string's value is the slot text `emit_expr`
     produced, and `parse_fmt_inner_expr` re-lexes that text.
   - Other `Token::Str` consumers fmt prints (asm template, `use`/extern paths) are unchanged.

   This output lexes the same before and after the lexer change: it holds no bare `"` inside a slot, and `\"` keeps its meaning (step 1).
   So the fmt fix does not depend on steps 1-3 and may land first. The first draft wrapped a slot whose text starts with `"` in `( )`;
   that is dropped, because it did not cover a string after a keyword (`fmtkw`: `{if "a" == s ...}`, `{match "a" ...}`).
   Newlines that `emit_expr` writes inside a slot stay as they are. A raw newline is accepted in a string body today: `fmtkw2`'s
   multi-line string fails at the quote (4:10), not at the newline.
   A today-valid program never has a quoted brace that unbalances its slot, because today's string-unaware matcher would refuse it
   (E0000). So the escaped output does not rely on step 3 for any program that runs today.
5. **Source scanners** (`lib.rs:1883`, `:1930`, `main.rs:6539`): skip string literals via the shared scanner. Without this a nested
   string holding `//` (e.g. `"{f("a // 2")}"`) is read as code: a false W0007 and a false "contains comments" fmt refusal
   [INFERENCE: derived from the toggle logic, not run - the form does not lex today].
6. **Docs.** `spec/grammar.ebnf` §6.12 says three things:
   - a slot may hold a string literal anywhere an operand follows an operator or opener;
   - a slot cannot START with a string literal (write `{("a" + x)}`);
   - `\"` stays accepted, and `[REVISED: CS4A-1]` a string whose slot holds a string is an expression only, so a pattern cannot be one.

   `spec/axon-for-llms.md:86` and `spec/language-tour.md:56` get one example with a nested string, written in the post-fix form, which
   `doc_snippets` runs (CS4D-3). CHANGELOG lines cover the lexer change (naming the pattern refusal) and the fmt fix.

**Compatibility.** `[REVISED: CS4A-1, ruling 1]` A string that lexes today changes only when two things hold: inside an open `{`, a `"`
follows an operator or opener character, and the extended scan closes. Such a token is `SlotStr`.
- In expression position, every such string is E0000 today: the slot is unclosed, because the today-token ends at that `"` with its
  `{` still open [INFERENCE: argued from `parse_fmt_str_raw`'s brace matcher, not enumerated].
- In every other position, today's program read a plain string with different boundaries. After the change it is **refused** (E0000),
  never re-read as a different string.
- `CS4A/patswallow.ax` (`match c { "{(" => 1, "x" => 2, _ => 0 }`, rc 0 today, prints `1|2`) is one of these: it becomes E0000. Without
  the separate token it would have printed `0|0` (CS4A-1).
- A scan of all 700 tracked `.ax` files with an emulation of rule 1 found **0** strings whose lexing changes, and 0 open-slot strings
  at all (eval in this session, `Soundness4` kernel).
- `q6o_pat` (`"{"` then `=>`) is unchanged, because the slot is empty when the `"` arrives.

**Why no lexer-only rule avoids the refusal.** Any rule that lexes `"{f("a")}"` as one token must open a nested string at the `"` after
`{f(`. The same characters start the pattern string in `match c { "{f(" => 1, "x" => 2, _ => 0 }`, which checks rc 0 today. Under the
rule, the nested string ` => 1, ` closes, and the outer string then closes at the `"` after `x`. The token boundaries move in a running
program, and the lexer cannot see that the token is a pattern. So a pure lexer rule either refuses that program or changes its meaning.

Native: same AST as the escaped form, which builds and runs (`q6m_esc`) [INFERENCE: not built after the fix]. LSP: inherits through
`analyse_source` [INFERENCE].

**Alternatives considered.**
- (a) Make patterns process braces too, then always treat `"` in a slot as nested. This refuses programs shaped like `q6o_pat` that run
  today. Rejected.
- (b) Lexer backtracking only (try nested, fall back on failure). `"{" => 1, "}" => 2` on one line closes under the nested reading and
  silently becomes one string. Rejected; the operator condition is what keeps it.
- (c) Leave the lexer alone and document `\"` only (the O-HX3-04 alternative). This leaves grammar §6.12 false. Rejected.
- (d) `[REVISED: CS4A-1]` The first draft: one `Str` token for both readings. `patswallow` silently prints `0|0`. Rejected.
- (e) `[REVISED: CS4A-1]` Position-aware fallback. The lexer keeps today's tokens; the expression parser, on an unclosed-slot E0000,
  re-scans the source and re-tokenizes the rest of the file. This refuses nothing and changes nothing that parses today. Rejected for
  cost and risk:
  - `parse_source` has no source text for `SourceId::UNKNOWN` (`lib.rs:103`);
  - the token stream after the string would have to be rebuilt with its newline flags and spans (`tokenize_with_newlines`);
  - the tokenize-only consumers (`fmt.rs:1344`, `lsp.rs:810`, the source scanners) would still see today's tokens, so one file would
    have two readings.

  Ruling 1 prefers the refusal of an untracked, rare shape. (e) remains the route to zero refusals if one is wanted later.

**Fail-first tests.**
- cli_run `string_literal_inside_an_interpolation_slot_runs`: q6a (`hi bo!`), `"{join(xs, ", ")}"`-shaped `str_join` case, `"a{id("}")}b"`
  (`a}b`), nested slot `"{f("x{n}y")}"`, nested raw `"{n + r"{"}"`-style case [INFERENCE: exact text chosen by builder], escaped forms
  q6m_esc / esc_brace / esc_if unchanged. Fails on base (E0000). Marker `ATTACK: a string literal in a slot was refused`.
- cli_run `lone_brace_strings_keep_their_meaning`: q6o_pat (`1|2`), q6n_lone stays E0000 "unclosed `{`", `"x{"` stays E0000, and
  `[REVISED: CS4A-1]` patguard (`1|2`, the guard spelling the refusal suggests). Passes on base. Marker `ATTACK: a "{" string changed meaning`.
- cli_run `slot_string_outside_an_expression_is_refused` `[REVISED: CS4A-1, ruling 1]`: patswallow -> rc 2, E0000 at line 2, message
  contains "interpolation slot"; the run never prints `0|0`. Fails on base (rc 0, `1|2`). Marker
  `ATTACK: a slot string outside an expression changed meaning silently`.
- `--lib` `fmt::tests::fmt_keeps_brace_escapes` (via `round_trip`, `fmt.rs:1454`): the b.ax/p.ax shapes print `{{n}}` and `{{`, and a
  raw `r"{n}"` round-trips to a string with the same value. Fails on base. Marker `ATTACK: fmt turned a literal brace into a slot`.
- `--lib` `fmt::tests::fmt_slot_string_round_trips` `[REVISED: CS4A-2]`: the e.ax shape and fmtkw's three slots (`match s { "a" => ..}`,
  `if "a" == s`, `match "a"`) are formatted, re-parsed, and evaluate to the same AST. It passes with the fmt fix alone, without the
  lexer change. Fails on base. Marker `ATTACK: fmt wrote a slot that does not re-parse`.
- `--lib` `fmt::tests::fmt_brace_pattern_stays_single`: `"{" =>` stays `"{"`. Fails on base only if base doubles it; it does not, so it
  passes on base and guards M4710. Marker `ATTACK: fmt changed a brace pattern`.
- `--lib` lexer unit test (`lexer.rs` tests, beside `:360` `string_and_comment_edges`): token values of the nested and escaped forms
  are equal.
- cli_run `floor_division_lint_skips_slot_strings`: `"{f("a // 2")}"` gives no W0007. Fails on base only through E0000 (acceptable
  fail-first: base refuses the program).

**g01 rows**: M4705-M4712. `[REVISED: CS4A-1]` M4712 is now the `SlotStr` pattern refusal. The first draft's W0007 scanner row is
dropped (a warning-only lint); `floor_division_lint_skips_slot_strings` still runs (§4).

**Acceptance.** The tests pass. `axon fmt --check` followed by `axon run` on b/e/p/fmtkw reproduces the pre-fmt output. The sweep shows
no rc 0 -> nonzero file. `lsp::tests` and `program_json_tests` (S12 stage) pass. `[REVISED: CS4D-3]`
`cargo test --locked -p axon-core --no-default-features --test doc_snippets` passes, and any new doc example with a string inside a
slot uses the post-fix form only.

### CS4-3 (P1) Tuple patterns are type-checked (O-LF-01)

**Anchors (b3a9c56e, `crates/axon-core/src/infer.rs`).**
- `:2968-3031` `bind_pattern_with_subj`; `:2985-2997` "Fix #8": `Ok`/`Err`/`Some` constrain the subject (`self.constrain(st, Type::Result(..),
  "Ok pattern")`) and bind the inner pattern with the component var. `:3026-3030` `Pattern::Tuple(pats)`: each sub-pattern is bound with
  `None` - no subject constraint, no component types.
- `:1241` `constrain`; unify `(Type::Tuple(ts1), Type::Tuple(ts2)) if ts1.len() == ts2.len()` (`:3528`), any other pair falls to the
  mismatch path (E0102). `Deferred`/`Unknown` pass without error (unify fallthrough, `:3476-3566`).
- `:2942-2952` `infer_match_arm_with_subj` calls it for every arm; `if let` desugars to `match` (lang-forms LF-2), so it inherits.
  `:2386-2396` `Expr::Match` infers the subject once and passes the same `subj_ty` to every arm.
- `:2488-2495` `Expr::Tuple` infers to `Type::Tuple(..)` directly, and `:1881-1909` `Let` binds the value's type unchanged when there
  is no annotation. So `let t = (1, 2, 3)` gives `t` a concrete `Tuple` at bind time, and an annotated param (`p: P`) is concrete too.
  An untyped lambda param is a fresh `Var`.
- Precedent: `match 5 { Some(x) => x, _ => 0 }` (`lf/s_some_on_int.ax`) is rc 2, E0102 2:5 "type mismatch in Some pattern (expected
  Option<?0>), found i64" - a dead arm of the wrong shape is refused today.

**Reproduction.**

| probe | shape | check | run | after CS4-3 |
|---|---|---|---|---|
| `lf/t_arity3.ax` | `match (1, 2, 3) { (a, b) => a + b }` | 0 | 101 `no match arm matched` | E0102 |
| `lf/t_nontuple.ax` | `match 5 { (a, b) => a + b }` | 0 | 101 `no match arm matched` | E0102 |
| `lf/t_comptype.ax` | `match (1, "s") { (a, b) => to_str(b + 1) }` | 0 | 101 `cannot apply Add to str / i64` | E0102 (from `b + 1`) [INFERENCE: exact code of the Add error] |
| `lf/t_nested_ty.ax` | `(Some("x"), 2)` with `(Some(a), b) => a + 1` | 0 | 101 | refused (same) |
| `lf/t_iflet_ty.ax` | `if let (a, b) = (1, "s") { to_str(b + 1) }` | 0 | 101 | refused |
| `lf/t_iflet.ax` | `if let (a, b) = (1, 2, 3) {..} else { println("no") }` | 0 | **0**, `no` | **new refusal** E0102 (dead branch, D9) |
| `lf/t_mixed_arity.ax` | `match (1, 2) { (a, b) => a + b, (a, b, c) => a }` | 0 | **0**, `3` | **new refusal** E0102 (dead arm, D9) |
| `lf/t_struct_subj.ax` | `match p { (a, b) => 1, _ => 2 }`, `p: P` | 0 | **0**, `2` | **new refusal** E0102 (dead arm, D9) |
| `lf/t_ok.ax`, `t_ok_arr.ax`, `t_ok_iflet.ax`, `t_ok_lambda.ax` (untyped `\|p\| match p {(a, b) => a + b}`), `t_generic.ax` (`swap<A, B>`), `t_tparam.ax` (`fn f<T>(x: T)` matching `(a, b)` and `_`, called with a tuple and with `5`), `t_wild.ax` | valid tuple matches | 0 | 0 (`a!\|3b\|3\|6`, `1a\|2b\|2`, `s\|1`, `3`, `x1`, `1\|2`, `12`) | unchanged (controls) |
| `lf/t_lit_ty.ax` | `("a", y)` on `(i64, i64)` | 0 | 0, `1` | unchanged (O-CS4-04) |
| `CS4A/l1.ax` `[REVISED: CS4A-4]` | `let e = \|p\| match p { (a, b) => 2, (a, b, c) => 3, _ => 0 }`, called with a 2-tuple and a 3-tuple | 0 | 0, `2\|3` (both arms run) | unchanged (control) |
| `CS4A/l4.ax` | the `Some`/`Ok` twin of l1 | 2, E0102:2 | - | unchanged (today's behaviour of the Fix #8 path, shown for contrast) |

**Fix design.** `[REVISED: CS4A-4, ruling 3]` In `Pattern::Tuple(pats)` (`:3026`), constrain the subject only when it is concrete at
bind time:
- Concrete means `subj_ty` is `Some(st)` and `st` is not `Var`, `Deferred`, `Unknown`, `TypeParam` or `DynTrait` at the top level. A
  `Tuple` whose elements are vars is concrete.
- For a concrete subject: create one fresh var per sub-pattern, call `self.constrain(st.clone(), Type::Tuple(vars.clone()),
  "tuple pattern")`, and bind each sub-pattern with `Some(&var_i)`. A `Tuple` of the same length binds its element types. Any other
  concrete type, or a tuple of another length, is the existing E0102 path ("type mismatch in tuple pattern (expected (?1, ?2)), found
  (i64, i64, i64)"), the same code and wording family as the `Some` precedent.
- For any other subject (`None`, an unresolved var, a generic or unknown type), bind each sub-pattern with `None`, as today.

Why concrete only: `l1` is an untyped lambda whose subject is a fresh `Var`. Constraining it in arm 1 fixes `p` to a 2-tuple, and arm
2 then becomes E0102, which refuses a program whose arms both run (CS4A-4). The Fix #8 path does exactly that today for `Some`/`Ok`
(`l4`), but copying it here would add a refusal. Under this rule `t_tparam` (`T` is `Deferred`/`TypeParam`), `t_ok_lambda` and `l1`
(a `Var`) and every nested tuple under `Some(..)` (the inner subject is a fresh var, `:2996`) keep today's behaviour. The dead-arm
refusals (t_arity3, t_nontuple, t_comptype, t_iflet, t_mixed_arity, t_struct_subj) all have a concrete subject (a tuple literal, a
`let` of one, `5`, or a `p: P` param), so they still fail first [INFERENCE: subject types read from `:1881-1909` and `:2488`, not
run]. A tuple pattern on a var subject stays unchecked. That residual is no worse than today, and the builder logs it in the O-LF-01
stamp.

Optional wording (builder's choice, same code): when `st` is already a `Type::Tuple` of a different length at bind time, emit
"tuple pattern has N elements, but the subject has M" instead of the generic mismatch. The test asserts code, line and the substring
"tuple pattern" only.

**Error code decision.** Reuse E0102 (type mismatch). E0405 ("literal pattern's type can't match the match subject") is checker-side and
scalar-only; a tuple arity/shape mismatch is a unification failure like the `Some` precedent. No new code, no `AXON_REFERENCE.md` diff.

**Fail-first tests** (cli_run):
- `tuple_pattern_is_type_checked`: t_arity3, t_nontuple, t_comptype, t_iflet, t_mixed_arity and t_struct_subj each give rc 2, E0102
  at the match line. Fails on base (rc 0). Marker `ATTACK: a tuple pattern of the wrong shape checked clean`.
- `tuple_pattern_controls_stay_legal`: the 7 controls and `[REVISED: CS4A-4]` `l1` (`2|3`), each with its output. Passes on base.
  Marker `ATTACK: a valid tuple match was refused`.
Native: `tuple_patterns_test_every_component_natively` (`integration_fixtures.rs:2855`) must stay green (component types now bound).

**g01 rows**: M4713-M4715 (`[REVISED: CS4A-4]` M4715 is the concrete-subject guard).

**Acceptance.** Tests pass; sweep shows no rc 0 -> nonzero (the corpus's tuple arms, e.g. `examples/tuples.ax:38`,
`crates/axon-cortex/benchmarks/hardset/src/tuples.ax:38`, stay rc 0 - checked by the sweep, not by me after the fix).

### CS4-4 (P1) A wrapper lambda's call nested in another call's arguments derives a kind (O-CS3-05)

**Anchors (b3a9c56e, `crates/axon-core/src/infer.rs`).**
- `:982-994` fields `cond_depth`, `lambda_params`, `lambda_calls` ("the calls started so far in the innermost lambda's body. Only a call
  that is the body's first, with no call in its callee or args, derives kinds"), `all_lambda_params`.
- `:1987-1990` `Expr::Call`: `let calls_before = self.lambda_calls; self.lambda_calls += 1;` BEFORE the callee and args are inferred
  (pre-order). `:2286-2297` `derives_for` is set iff `cond_depth == 0 && calls_before == 0 && self.lambda_calls == 1`.
- `:2303-2310` `Expr::MethodCall`: `self.lambda_calls += 1` before the receiver.
- `[REVISED: CS4A-3]` The Call arm returns early, after inferring the arguments, at `:2097` (`len`), `:2112-2138` (`to_str`) and `:2273`
  (every call to a named fn or builtin, `println` included). Only calls through a value reach `derives_for` at `:2286-2297`.
- `:2780-2803` lambda entry saves/resets `lambda_calls`, `cond_depth` (`may_leave_early`, `:551`), `lambda_params`.
- Interpreter order: `interp/eval.rs:1480` "Evaluate arguments left-to-right" in `eval_call` (`:1451`); the callee is dispatched after
  (`:1558` `dispatch_call`). Lazy exception: `P(...)` (`:1462-1469`).

**Reproduction.**

| probe | body of `g` (`let f = \|x\| str_len(x)`, then `g(5)`) | check | run |
|---|---|---|---|
| `cs/w17b.ax` | `\|y\| println(to_str(f(y)))` | 0 | 101 `expected str, got i64` (also `AXON_ENGINE=vm`) |
| `cs/w17c.ax` | `\|y\| to_str(f(y))` | 0 | 101 same |
| `cs/w17d.ax` | `\|y\| f(y) + 1` | 2, E0102 4:5 "type mismatch in arg 0 of a call through a fn value (expected str), found i64" | - (CS3-2 works when `f(y)` is the first call in pre-order) |
| `cs/w17f.ax` | `\|y\| { println("pre"); f(y) }` | 0 | 101 after `pre` (handler residual, out of scope) |
| `cs/c_sib.ax` | `\|y\| h2(println("x"), f(y))` under `with handler { on IO(p) => 7 }` | 0 | 0, `7` (control: `f` never runs) |
| `cs/c_ok2.ax`, `cs/c_ok.ax` | `\|y\| println(to_str(f(y)))` called with `"abc"` | 0 | 0, `3` (control) |
| `cs/c_cond.ax`, `cs/c_and.ax` | `f(y)` under `if c` / right of `&&` | 0 | 0 (controls) |
| `CS4A/c_seq.ax` `[REVISED: CS4A-3]` | `\|y\| { println("x"); f(y) }` under `with handler { on IO(p) => 7 }` | 0 | 0, `7` (control: the handler answers `println` without `resume`, so `f` never runs) |
| `Soundness4/rv/m4716.ax` | `let f = \|x, z\| str_len(x)`, `\|y\| f(y, println("x"))` under the same handler | 0 | 0, `7` (control: the M4717 killer) |

**Cause.** The count is pre-order: in `println(to_str(f(y)))` the outer `println` is counted first, so `f(y)` has `calls_before == 2`
although it is the first call to RUN (an argument runs before its callee).

**Fix design.** `[REVISED: CS4A-3, ruling 3]` Count in run order. Add `pending_calls: u32` to `InferCtx`, saved and reset with
`lambda_calls` at `:2780-2803`. It counts the enclosing calls whose callee or args are still being inferred: calls that STARTED in
pre-order but cannot have RUN yet.
- Move the body of the `Expr::Call` arm, unchanged apart from the `derives_for` condition, into `fn infer_call(&mut self, callee, args,
  .., calls_before, pending_at_entry) -> Type`. Its early `return`s then return from `infer_call`.
- The arm becomes: `let calls_before = self.lambda_calls; self.lambda_calls += 1; let pending_at_entry = self.pending_calls;
  self.pending_calls += 1; let t = self.infer_call(..); self.pending_calls -= 1; t`. One helper, `fn in_pending_window<T>(&mut self,
  f: impl FnOnce(&mut Self) -> T) -> T`, does the `+= 1 / -= 1` for both Call and MethodCall, so no exit path can skip the decrement.
- `derives_for` holds iff `cond_depth == 0 && calls_before == pending_at_entry && self.lambda_calls == calls_before + 1`. That is: no
  call ran before this one (every call that started earlier is an ancestor still waiting for its arguments), and no call sits in its
  own callee or args.
- `Expr::MethodCall`: the same, with the receiver and args inside the same window.
- The window brackets inference only: it starts after `calls_before` is read and ends before the arm returns. The pending count
  derives no kind of its own, so a call that returns early (`println`, `len`, `to_str`) just leaves the count where it found it.
- `P(...)` (lazy arg): calls inside its argument never derive [INFERENCE: other lazy builtins - the builder lists them from `eval_call`
  `:1451-1560` and treats each the same].

Traces (corrected, `[REVISED: CS4A-3]`):
- `h2(println("x"), f(y))` (c_sib): `h2` starts, so pending is 1 and `lambda_calls` is 1. `println` has `calls_before 1 == pending 1`:
  it is the first call to run, and it is correctly first. Its window closes, so pending is back to 1. `f` has `calls_before 2 !=
  pending 1`, so it does not derive, and `c_sib` stays legal. The first draft leaked `println`'s pending count through the `:2273`
  return, which gave `f` `2 == 2` and refused `c_sib` (CS4A-3).
- `{ println("x"); f(y) }` (c_seq): `println` (`calls_before 0 == pending 0`) is first; `f` has `calls_before 1 != pending 0`, so it
  does not derive. This is the handler residual CS3-2 left alone (w17f), and it stays legal.
- `println(to_str(f(y)))` (w17b): `println` and `to_str` are pending (2), and `f` has `calls_before 2 == pending 2`, so it derives
  `y: str` and `g(5)` is E0102 at check.
- `f(y, println("x"))` (m4716): `f` has `calls_before 0 == pending 0`, but `println` in its args makes `lambda_calls` 2, not 1, so `f`
  does not derive and the program stays legal.

**Alternatives considered.**
- (a) Post-order increment (move `+= 1` after the args). This is equivalent, but every early `return` in the Call arm (special
  builtins) must also increment. Rejected for fragility.
- (b) Derive for every call at cond depth 0. This refuses `c_sib` and `c_seq`. Rejected.
- (c) `[REVISED: CS4A-3]` The first draft: an inline `+= 1 / -= 1` around the arm's inference. Its decrement is skipped on the
  `:2097`, `:2112-2138` and `:2273` returns. Rejected.

**Fail-first tests** (cli_run):
- `wrapper_lambda_nested_call_kind_is_checked`: w17b, w17c, and a method-call receiver case (builder picks a method that exists
  [INFERENCE]), each giving rc 2, E0102, message "arg 0 of a call through a fn value". Fails on base. Marker
  `ATTACK: a wrong-kind argument through a nested wrapper call checked clean`.
- Add `c_sib`, `c_ok2`, `[REVISED: CS4A-3]` `c_seq` and the m4716 shape (each `7` or `3`) to the existing
  `wrapper_lambda_controls_stay_legal` (`cli_run.rs:44328`), keeping its marker.

**g01 rows**: M4716-M4718 (`[REVISED: CS4A-3]` M4718 is the window decrement).

**Acceptance.** Tests pass; `wrapper_lambda_param_kind_is_checked` (`:44249`) and `wrapper_lambda_controls_stay_legal` pass; sweep no
rc 0 -> nonzero.

### CS4-5 (P3, optional) A qualified pattern of a different enum is refused (O-AF-04)

**Anchors.** `[REVISED: CS4A-6, CS4A-8]`
- `parser.rs:3925-3960`: `C::Red` parses to `Pattern::Struct { name: "C::Red", fields: [] }` (`full_name`, `:3925`).
- `checker.rs:6717` `check_match_exhaustiveness`. Inside it:
  - `:6780-6806` the E0405 literal-pattern check (`scalar_kind(subject_ty)`);
  - `:6811-6819` the early `return` when an unguarded `_` or ident arm exists;
  - `:6825-6829` `enum_name` from the subject type (`Type::Enum(n)`, or a sum type that reaches it as `Type::Struct(n)`).
- AF-3's E0409 is NOT in this function; it is in `check_bare_variant_patterns` (`:6612-6647`).
- `error.rs:51` and `:473`: E0405 "literal pattern's type can't match the match subject".

**Reproduction.** `lf/v2q_noall.ax` (`enum C { Red, Green }`, `enum D { Up, Down }`, `match c { D::Up => 2, _ => 1 }` on a `C`): check
rc 0, run rc 0 prints `1`. `lf/v2q_ok.ax` (`C::Red`/`C::Green` arms): rc 0, `6` (control).

**Fix design.** `[REVISED: CS4A-6, ruling 4]` In `check_match_exhaustiveness`, BEFORE the wildcard `return` (`:6817`), next to the
E0405 literal check:
- compute the subject's enum name the way `:6825-6829` does (hoist that `let` above the return; it is used again after);
- when it is `Some(e)` and a top-level arm pattern is `Pattern::Struct { name }` with `name = "X::V"`, `X != e` and `X` a known enum
  (`self.enum_variants`), emit E0405 "this match is on `C`, but `D::Up` is a variant of `D` - it can never match", with help "use a
  `C::` variant";
- inside `Some`/`Ok`/`Err` is optional, as before.

The first draft placed the check after the return. `v2q_noall` has `_ => 1`, so the check never ran on its own reproducer (CS4A-6).

"It can never match" holds even when the two enums share a variant name: `CS4A/en.ax` (`C::Up` and `D::Up`) prints `1|1` on both
the interpreter and `AXON_ENGINE=vm` (re-observed). Broaden the E0405 description (`error.rs:51`, `:473`) to "a pattern's type can't
match the match subject", and regenerate `AXON_REFERENCE.md` (`axon reference`). For a generic enum, compare the enum NAME only (no
type args), so there is no unification risk.

**Fail-first test.** cli_run `foreign_enum_variant_pattern_is_refused`:
- v2q_noall gives E0405. Its `_` arm is present, so the test pins CS4A-6's placement.
- `[REVISED: CS4A-6]` `en` (`D::Up` on a `C` that also has an `Up`) gives E0405. Today it prints `1|1`: the arm is dead.
- v2q_ok stays rc 0.

Marker `ATTACK: a pattern of another enum checked clean`. New refusals of running programs (dead arms): §8, D9.

**g01 row**: M4719.

**Acceptance.** Test passes; sweep no rc 0 -> nonzero; `axon reference` diff is the E0405 line only.

## 3. Shared mechanics

- **Corpus sweep (every item, before/after).** `/var/tmp/r51-tmp/Soundness4/sweep.sh <repo> <bin> <out.tsv>`: `axon check` of every
  `git ls-files '*.ax'` from the file's directory, recording path, rc and sha256 of the diagnostics. Baseline measured at ebbb4e02
  (= b3a9c56e for `.ax` files): **700 files, 609 rc 0, 91 rc 2**, identical with and without
  `AXON_PATH=examples/stdlib:examples/asi:examples/modular:examples/domain` (`base.tsv`, `base-ap.tsv`); two runs byte-identical
  (`base2.tsv`); 1.0 s wall (`xargs -P 12`). Rule: no file goes from rc 0 to nonzero unless this spec names it (it names none among
  tracked files). An rc 2 -> 0 change or a diagnostic hash change is listed in the lane's report. Dev-loop only (untracked script).
- Each behaviour-changing commit touching `crates/axon-core/src` gets a pci theme (`governance/notes/pci-themes/<sha8>.json`
  `{"theme","text"}`, then `python3 tasks/r51-pci-regen.py`, then `python3 scripts/pci_delta.py --check`), and a CHANGELOG line:
  - CS4-1: the if join, and the exarmif refusal;
  - CS4-2: the lexer change, naming the `SlotStr` pattern refusal; the fmt fix;
  - CS4-3: tuple E0102 on a concrete subject;
  - CS4-4: wrapper kinds;
  - CS4-5: E0405.
- g01 rows (`scripts/v022_g01_mutations.py`, format of `:14209-14273`: `(id, title, file, old, new, crate, test args, killer)`) and
  `ATTACK:` markers (`scripts/v022_attack_markers.py`) land in the fix commit; each killing test's assert message carries the marker.
  `[REVISED: CS4D-1]` Verify each with `python3 scripts/v022_g01_mutations.py --only=M47xx` (comma list allowed:
  `--only=M4700,M4701`). The script reads only the `--only=` form (`:8784`); with a space the id filter is never applied.
- `python3 scripts/v022_refusal_coverage.py`: run once per lane. If it lists the CS4-5 push site (checker) or `[REVISED: CS4A-1]` the
  new `SlotStr` refusal in `parse_pattern` (`parser.rs:3911`), add a coverage row, as AF-3 did for E0409. CS4-1, CS4-3 and CS4-4 add no
  new refusal site; they use the existing E0601/E0102 paths.
- Docs: CS4-2 updates `spec/grammar.ebnf` §6.12, `spec/axon-for-llms.md:86` and `spec/language-tour.md:56`. `[REVISED: CS4D-3]`
  Both doc lines sit in code blocks that `crates/axon-core/tests/doc_snippets.rs:33-37` runs, so S4b runs `cargo test --locked -p
  axon-core --no-default-features --test doc_snippets`. CS4-5 regenerates `AXON_REFERENCE.md` with `axon reference` (never by hand).
  Also: `python3 -B scripts/spec_ledger.py` PASS; `cargo fmt --all -- --check`; clippy on `axon-core`.
- Test programs inlined; no test reads `/var/tmp`. Interpreter is the reference; CS4-2/CS4-3 are AST-level, so native follows.

## 4. g01 rows (M4700-M4719)

Target `axon-core --no-default-features --test cli_run` unless named; `--lib` rows use `--lib <filter>`. The builder writes the exact
`old` line after the fix lands.

`[REVISED: CS4A-1, -3, -4, -5]` Twenty-three guards compete for the twenty ids in this block. Three new rows come from the review, so
three first-draft rows are dropped. Their tests still run; only the mutation proof is lost. The dropped rows, each the
lowest-consequence guard in its item:
- the `exit_bound` term in the `If` rule's `exit_local`: losing it accepts `exarmif`, which runs right today;
- the W0007 scanner: losing it gives a false warning, not a refusal;
- the MethodCall receiver window: Call and MethodCall now share one helper, so M4718 covers that helper for both.

Ids are renumbered so each item's rows are contiguous.

| id | item | guard removed by the mutant | file | killer | marker |
|---|---|---|---|---|---|
| M4700 | CS4-1 | the re-bind join (mutant: drop step 3) | borrow.rs | `if_branches_are_alternatives_for_moves` | `ATTACK: a re-bind in every if branch was refused` |
| M4701 | CS4-1 | escaping-jump block (mutant: never skip) | borrow.rs | `if_join_keeps_real_moves` (ibrk, brrb) | `ATTACK: an if join dropped a move` |
| M4702 | CS4-1 | absent name read as the snapshot state (mutant: skip absent names) | borrow.rs | `if_join_keeps_real_moves` (shi) | same |
| M4703 | CS4-1 | `[REVISED: CS4A-5]` a final `break`/`continue` does not block the join (mutant: block on any escaping jump) | borrow.rs | `if_branches_are_alternatives_for_moves` (ifbrk_ok) | `ATTACK: a re-bind in every if branch was refused` |
| M4704 | CS4-1 | a diverging branch is not joined (mutant: `then_live = true`) | borrow.rs | `if_branches_are_alternatives_for_moves` (rjdif, rjxif) | same |
| M4705 | CS4-2 | nested string in a slot (mutant: `"` always ends the outer string) | token.rs | `string_literal_inside_an_interpolation_slot_runs` | `ATTACK: a string literal in a slot was refused` |
| M4706 | CS4-2 | operator condition (mutant: nested also when the slot is empty) | token.rs | `lone_brace_strings_keep_their_meaning` (q6o_pat) | `ATTACK: a "{" string changed meaning` |
| M4707 | CS4-2 | slot matcher skips nested strings (mutant: brace depth only) | parser.rs | `string_literal_inside_an_interpolation_slot_runs` (`a{id("}")}b`) | `ATTACK: a string literal in a slot was refused` |
| M4708 | CS4-2 | nested string copied verbatim (mutant: unescaped with the outer body) | token.rs | `string_literal_inside_an_interpolation_slot_runs` (`{f("a\"b")}` case) | same |
| M4709 | CS4-2 | fmt `FmtPart::Lit` doubles braces (mutant: `'{' => "{"`) | fmt.rs | `fmt::tests::fmt_keeps_brace_escapes` (`--lib`) | `ATTACK: fmt turned a literal brace into a slot` |
| M4710 | CS4-2 | pattern literal keeps single braces (mutant: pattern doubled) | fmt.rs | `fmt::tests::fmt_brace_pattern_stays_single` (`--lib`) | `ATTACK: fmt changed a brace pattern` |
| M4711 | CS4-2 | `[REVISED: CS4A-2]` slot text written with `"` -> `\"` and `\` -> `\\` (mutant: written bare) | fmt.rs | `fmt::tests::fmt_slot_string_round_trips` (`--lib`, the `if "a" == s` case) | `ATTACK: fmt wrote a slot that does not re-parse` |
| M4712 | CS4-2 | `[REVISED: CS4A-1]` the pattern parse refuses `SlotStr` (mutant: accept it like `Str`) | parser.rs | `slot_string_outside_an_expression_is_refused` (patswallow) | `ATTACK: a slot string outside an expression changed meaning silently` |
| M4713 | CS4-3 | `constrain(st, Tuple(vars), "tuple pattern")` on a concrete subject (mutant: dropped) | infer.rs | `tuple_pattern_is_type_checked` | `ATTACK: a tuple pattern of the wrong shape checked clean` |
| M4714 | CS4-3 | components bound with `Some(&var_i)` (mutant: `None`) | infer.rs | `tuple_pattern_is_type_checked` (t_comptype) | same |
| M4715 | CS4-3 | `[REVISED: CS4A-4]` concrete-subject guard (mutant: constrain any subject) | infer.rs | `tuple_pattern_controls_stay_legal` (l1) | `ATTACK: a valid tuple match was refused` |
| M4716 | CS4-4 | `calls_before == pending_at_entry` (mutant: `calls_before == 0`) | infer.rs | `wrapper_lambda_nested_call_kind_is_checked` | `ATTACK: a wrong-kind argument through a nested wrapper call checked clean` |
| M4717 | CS4-4 | `self.lambda_calls == calls_before + 1` (mutant: dropped) | infer.rs | `wrapper_lambda_controls_stay_legal` (the `rv/m4716.ax` shape, `f(y, println("x"))`, rc 0 `7` today) [INFERENCE: refused under the mutant by trace] | `ATTACK: a wrapper-lambda program that runs was refused` |
| M4718 | CS4-4 | `[REVISED: CS4A-3]` the pending window's decrement in the shared helper (mutant: dropped) | infer.rs | `wrapper_lambda_controls_stay_legal` (c_sib) | same |
| M4719 | CS4-5 | foreign-enum check (mutant: `X != e` -> `false`) | checker.rs | `foreign_enum_variant_pattern_is_refused` | `ATTACK: a pattern of another enum checked clean` |

## 5. Gates and cost budget (no paid spend; `AXON_AI_MOCK=1` irrelevant - no AI path touched)

| stage | wall | source |
|---|---|---|
| corpus sweep (dev loop, per item, before/after) | 1.0 s | measured at ebbb4e02 |
| `cargo test -p axon-core --no-default-features --test cli_run --no-run` cold / warm | 26 s / 4.4 s | DevloopCS3 (cs3 §5) |
| one item's cli_run killers (warm, by name) | ~5 s | lang-forms §5 |
| `cargo test -p axon-core --lib fmt::tests` / lexer tests (CS4-2) | not measured | [INFERENCE: < 10 s warm] |
| `--test integration_fixtures tuple_patterns_test_every_component_natively` (CS4-3, LLVM) | not measured | [INFERENCE: a few s once built] |
| S12 serde-json stage (`lsp::tests`, `program_json_tests`, `lsp_multi_file`) for CS4-2 | not measured | no AST type change |
| one g01 row / 20 rows | ~15 s / ~5 min | 15 s per row (cs3 §5) |
| `scripts/gate.sh --lane` | per-lane subset | not the gate |
| `scripts/parity_all.sh` | ≈ 21 min | `gate.sh:1216-1219` |
| `scripts/gate.sh --strict` (integrator only, once after all lanes) | ≈ 85-95 min on this host | integrator brief; budget two runs (one fix-and-retry) ≈ 3 h |

Per-lane cost (edit + tests + sweep + g01, excluding the strict gate) [INFERENCE: estimates]:

| lane | items | code size | wall |
|---|---|---|---|
| S4a | CS4-1 | ~45 lines `borrow.rs` + 3 tests | ~1 h (5 g01 rows ≈ 1.5 min) |
| S4b | CS4-2 | shared scanner ~120 lines `token.rs`, `Token::SlotStr` + its arms (`lexer.rs`, `parser.rs` expr/pattern/Display/adjacency), slot matcher, `fmt.rs` ~30 lines, 3 scanners, docs, 6 tests | ~4 h (8 rows ≈ 2 min + S12 stage + `doc_snippets`) |
| S4c | CS4-3, CS4-4, CS4-5 | ~20 lines + the `infer_call` extraction (a move of the Call arm body, ~300 lines, no logic change) in `infer.rs`, ~30 lines `checker.rs`, `error.rs`, `AXON_REFERENCE.md` regen, 4 tests | ~2.5 h (7 rows + 1 integration_fixtures test) |

`[REVISED: CS4D-4]` Critical path: S4b (~4 h), then integration and one strict gate (≈ 1.5 h, ≈ 3 h with a retry), so ≈ 5.5-7 h wall
from start. S4a and S4c finish inside S4b's window. If S4b is late, S4a and S4c can merge and gate without it: no file is shared.

## 6. Sequencing and lanes

| lane | items | files |
|---|---|---|
| S4a | CS4-1 | `crates/axon-core/src/borrow.rs` (`Expr::If` arm `:397-428` only) |
| S4b | CS4-2 | `token.rs` (`lex_str`, new scanner, `SlotStr`, Display `:244`), `lexer.rs` (wrapper + tests), `parser.rs` (`parse_fmt_str_raw` `:185-201`, expression `Str` `:3554-3563`, pattern `:3911`, `refuse_adjacent_after_string` `:3471`), `fmt.rs` (`:928-1016`), `lib.rs` (`:1883-1960`), `main.rs` (`source_has_comments` `:6539-6560` only), `spec/grammar.ebnf`, `spec/axon-for-llms.md:86`, `spec/language-tour.md:56` |
| S4c | CS4-3, CS4-4, CS4-5 | `infer.rs` (`InferCtx` fields `:982-994`, Call `:1987-2297` -> `infer_call`, MethodCall `:2303-2310`, lambda entry `:2780-2803`, `bind_pattern_with_subj` `:3026`), `checker.rs` (`check_match_exhaustiveness` `:6776-6829`), `error.rs` (`:51`, `:473`), `AXON_REFERENCE.md` (regenerated) |

File overlap: none between S4a, S4b, S4c. Integrator-owned: `cli_run.rs` (blocks appended at end, one per lane), g01/marker files,
`CHANGELOG.md`, pci themes, `tasks/opportunities.md` (stamps for O-AF-07, O-HX3-04, O-LF-01, O-CS3-05, O-AF-04; new O-CS4-01..04).

Cross-batch overlap:
- CeilingsBuild (b2/ce) owns `main.rs` flags, `interp/builtins.rs`, `capabilities.rs`, axon-ai and axon-rt. Its `main.rs` hunks are at
  `:128-540`, `:971-1228` and `:3751`; S4b touches only `source_has_comments` (`:6539`). No textual overlap; message CeilingsBuild
  before landing, and whichever lands second re-reads its anchors.
- `[REVISED: CS4D-2]` b2/ce also edits `spec/axon-for-llms.md` (22 lines, hunks at `:301` and `:317`) and `lib.rs:71` (1 line). S4b
  edits `spec/axon-for-llms.md:86` and `lib.rs:1883-1960`. No hunk overlaps, but both are shared files: S4b rebases on b2/ce if it has
  landed and re-reads both anchors.
- CeilingsBuild's NH-C* error codes may touch `error.rs` (hunks at `:101`, `:510`) and `AXON_REFERENCE.md`; S4c regenerates the
  reference after rebasing.
- GenviabB2 (`crates/axon-intent/benchmarks/genviab/**`) shares nothing.

Order: any. S4c's three items have one owner because CS4-3 and CS4-4 both edit `infer.rs`; land CS4-4's `infer_call` extraction as
its own commit (pure move, no behaviour change) so the CS4-3/CS4-4 diffs stay reviewable.

## 7. Engineering decisions (decided here)

1. **D1 - CS4-1 changes only the `If` rule.** The match join, the loop rule and `branch_diverges` stay as AF-2 left them; the `If` rule
   gains the re-bind join with an escaping-jump block, so O-AF-01 and O-AF-02 stay exactly as decided (pinned by a test).
   `[REVISED: CS4A-5]` A `break`/`continue` that is its branch's final statement does not block the join (ifbrk_ok, like `mbrk_ok`).
2. **D2 - A name a branch's block no longer holds (popped shadow) is read as its pre-`if` state**, so `if` does not inherit O-CS4-01.
3. **D3 - The `If` rule uses `exit_bound` like the match and loop rules.** One rule for "is this `exit` the builtin" across the walker;
   it refuses `exarmif` as AF-2 refuses its match twin `ex` (asi-fixes ruling 2). Listed in D9.
4. **D4 - `[REVISED: CS4A-1]` CS4-2: a `"` in a slot starts a nested string only after an operator or opener.** So a slot cannot
   start with a string literal (write `{("a" + x)}`), and `q6o_pat`-shaped `"{"` patterns lex as today. Measured exposure: 0 tracked
   files change lexing.
5. **D5 - `[REVISED: CS4A-2]` fmt escapes slot text** (`\` -> `\\`, `"` -> `\"`, the inverse of `unescape`) and doubles literal
   braces in values, never in patterns. Its output lexes the same before and after the lexer change, so the fmt fix is independent of
   steps 1-3 and may land first in S4b. It rides with CS4-2 because it is the same surface (string brace escapes).
6. **D6 - CS4-3 reuses E0102**, matching the `Some`-pattern precedent; CS4-5 reuses E0405 with a broadened description.
7. **D7 - `[REVISED: CS4A-3]` CS4-4 counts calls in run order with a pending-ancestor counter kept by one helper** around the extracted
   `infer_call`, so the Call arm's early returns cannot skip the decrement.
8. **D8 (was NH-1, ruling 5) - accept the zero-iteration loop hole for `rjloop` and `loopinif`.** After CS4-1 they check rc 0 (today
   E0601), the same as `lz`, `wz`, `mrjloop` and `mloopinarm`, which are rc 0 today. Their zero-iteration path uses a moved value; it
   runs right, because the interpreter shares values. Closing the hole means the loop rule restores names moved before the loop, which
   refuses those four running programs (O-AF-01 class, left open by the 2026-11-02 decision). Logged as O-CS4-03 and pinned by
   `if_join_leaves_o_af_01_and_o_af_02_open`.
9. **D9 (was NH-2, ruling 5) - refuse, do not warn, these programs that run today:** `af7/exarmif.ax` (E0601, CS4-1),
   `lf/t_iflet.ax`, `lf/t_mixed_arity.ax`, `lf/t_struct_subj.ax` (E0102, CS4-3), and `lf/v2q_noall.ax`, `CS4A/en.ax` (E0405, CS4-5).
   Each is a dead arm or branch, or an `exit` the walker cannot prove is the builtin. Precedents: `match 5 { Some(x) .. }` is E0102
   today, and asi-fixes ruling 2 refused `ex`. A warning would leave `t_arity3`-shaped panics findable only by reading warnings. None is
   a tracked file (sweep).
10. **D10 (was NH-3, ruling 5) - log O-CS4-01 (a shadowing block erases a move: `shb`, `shm`) and O-CS4-02 (a match arm that leaves
    by `break` and then re-binds: `mbrk`) without fixing them.** Both accept a use-after-move in a program that runs today; closing
    either refuses running programs (O-AF-01/02 class). Log as `[medium]` / `[low]`, for a decision with O-AF-01/02 in a later batch.
11. **D11 (ruling 1) - `Token::SlotStr`.** A string whose scan opened a nested string is a separate token that only the
    expression-position parse accepts; every other consumer refuses it. This trades one silent re-reading (`patswallow`, `1|2` ->
    `0|0`) for a refusal (E0000 with the guard spelling as help), and fails closed for any consumer added later.

## 8. Needs human

1. **NH-4 - Is CS4-5 (optional P3) in this batch?** It adds two refusals of programs that run today (`v2q_noall`, `en`: dead arms of
   another enum's variant, D9). Options: (a) yes, in S4c: ~30 lines, one test, one row (M4719); (b) no, and O-AF-04 stays open.
   **Recommend (a)**: `en` is a silent dead arm even when the two enums share a variant name.

For the human's information (decided by ruling 5, not open): the batch refuses these programs that run today, none of them tracked -
`exarmif` (D3/D9), `t_iflet`, `t_mixed_arity`, `t_struct_subj` (D9), `patswallow` (D11), and with CS4-5 `v2q_noall`, `en`. All but
`exarmif` are dead code or a pattern string holding `{` and `"`. The zero-refusal route for `patswallow` is CS4-2 alternative (e).

## 9. Verification log (reference binary ebbb4e02, `/var/tmp/r51-tmp/Soundness4/`)

| probe | result |
|---|---|
| `run.sh af7/*.ax af7b/*.ax af7c/*.ax` + `AsiFixes/mv2/br1.ax`, `AFA/rj.ax`, `AFA/rjd.ax` | as in the CS4-1 table |
| `hx/*.ax`, `hx2/*.ax`; native `axon build hx/q6m_esc.ax` | as in the CS4-2 table; native `hi bo!` rc 0 |
| `fmt3/{b,e,p}.ax` run / `axon fmt` / run | as in the CS4-2 fmt table |
| 48-file fmt corpus probe (`fmtc2/`) | fmt succeeds on 2 files, run output identical |
| rule-1 emulation over 700 tracked `.ax` | 0 strings change lexing |
| `run.sh lf/*.ax lf2/*.ax` | as in the CS4-3 / CS4-5 tables |
| `run.sh cs/*.ax` (+ `AXON_ENGINE=vm` w17b) | as in the CS4-4 table |
| reviewer probes `CS4A/*.ax` re-run (Step 1 review) | as listed under the review table (patswallow, fmtkw, fmtkw2, c_seq, l1, l4, ifbrk_ok, mbrk_ok, loopinif, mloopinarm, en) |
| `rv/m4716.ax`, `rv/patguard.ax` | rc 0, `7`; rc 0, `1\|2` |
| baseline sweep x3 | 700 files, 609 rc 0, 91 rc 2, deterministic |

Not observed: anything marked `[INFERENCE]`; no cargo build or test was run; no fix was built, so every "after" column is the design's
prediction.
