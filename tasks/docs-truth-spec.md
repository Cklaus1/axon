# docs-truth - DRAFT spec

**Status: DRAFT. Not authorized for build until Step 1 review.**

Source: `tasks/opportunities.md` "Gap analysis, 2026-10-29": O-GAP-09 (LLM-facing docs disagree with the binary),
O-GAP-11 (spec status lines lag the code), and the doc part of O-GAP-08 (`mod util` needs `AXON_PATH`). The stamps
DT-5 adds cover O-S1-19, O-S1-20, O-S1-22, O-S1-23, O-S1-24, O-FU-01, O-FU-02, O-FU-05, O-FU-06, O-FU-07, O-S1-21,
O-CS-03, O-CS-04, O-SP-01, O-SP-04, O-SP-07.
Base: `main` 4a6436da, worktree `/home/cklaus/projects/axon-lanes/docs`, branch `docs/spec`. Every probe ran on
`/var/tmp/r51-tmp/main-bin/axon` (built at 0bd2b518; no `crates/` change between 0bd2b518 and 4a6436da). Probe
files are under `/var/tmp/r51-tmp/DocsDraft/` (`snip/` = every doc block extracted, `probe/`, `p2/`, `modp/`,
`closure/` = claims, `verify/` = the replacement blocks below, each checked and run, `post/` = the DT-5 tree that
the ledger check passes on).

This is the third time stale docs and stale closure state have shipped. The doc side was flagged before by
`governance/reviews/2026-07-31-deep-review.md:1423` (stdlib "Planned Phase 4") and
`governance/reviews/2026-08-01-triage/full-185.json:5618, :5696` (stdlib §5, tour §16), and the text is unchanged.
The ledger side is L015 and O-HI-02. So each half gets a mechanism and a doc fix (DT-6, DT-7).

### Step 1 review

Two read-only reviews of draft `67f2e367`: `agent://AuditDocs` (DTA-1..10) and `agent://DevloopDocs` (DTD-1..6). I
re-ran every cited probe (`/var/tmp/r51-tmp/DocsDraft/rev/`, `/var/tmp/r51-tmp/DocsDraft/readme189.ax`) and read
every cited line before deciding. All 16 are accepted; each edit carries `[REVISED: <id>]` at its place.

| review | blocker | major/P1-P2 | minor/P3/nit | accepted | rejected |
|---|---|---|---|---|---|
| DTA | 1 | 3 | 6 | 10 | 0 |
| DTD | 0 | 3 | 3 | 6 | 0 |

| id | disposition (one line) |
|---|---|
| DTA-1 | accepted: `README.md:189-209` checks rc 0 and `axon run` exits 45 (re-run); new marker `axon exits N`, §3a row 32 |
| DTA-2 | accepted: `max_i32(5000000000, 1)` prints `5000000000` (no truncation); stdlib text fixed, O-DT-01 logged |
| DTA-3 | accepted: §1 intro rewritten; the acceptance grep now targets claims that can reach 0 |
| DTA-4 | accepted: CommonMark fence rule (same char, >= length, `~~~`); planted 4-backtick and tilde cases pass |
| DTA-5 | accepted: rule 0, item ids unique across specs (0 collisions today) |
| DTA-6 | accepted: rule 2 reads only the bold span (gate-lane gets its sha 0bd2b518); rule 5 reads the whole Source paragraph |
| DTA-7 | accepted: detect `@[test`; `HOME`/`XDG_CACHE_HOME` = tempdir; stdin null |
| DTA-8 | accepted: "§7, Capability sandboxing" (a bold lead, not a heading) |
| DTA-9 | accepted: A4 names `axon check` and `axon reference`, both verbs |
| DTA-10 | accepted: O-S1-19 evidence is E1103 then E1104, rc 2 |
| DTD-1 | accepted: `builtins.rs` commit gets a pci theme file + `r51-pci-regen.py` + `pci_delta.py --check` (DT-2 step 11a, G1) |
| DTD-2 | accepted: `reference_gate.sh` needs `AXON=<bin>`; the in-gate check is `cli_run.rs:25725` |
| DTD-3 | accepted: CS2's ledger duty under DT-6 written into §6 and sent to `CS2Draft` |
| DTD-4 | accepted: docs-truth merges before CS2; second lander regenerates the reference and pci note |
| DTD-5 | accepted: §5 table redone (clean committed tree for g01, `.md`/`.py` rows need no recompile) |
| DTD-6 | accepted: only M3930 anchors on prose; the other doc rules are pinned by planted tests |

## 1. Out of scope and cross-spec dependencies

| item | where it goes | why |
|---|---|---|
| What `assert_eq_f64` compares (the interpreter allows 1e-9: `interp/builtins.rs:3776-3783`; `stdlib.md:516` says "exact") | `tasks/checker-soundness-2-spec.md` CS2-2 (agent `CS2Draft`) | CS2-2 owns the semantics and the `doc` string at `builtins.rs:247-251`. [REVISED: CS2-2 decided exact IEEE equality in both engines (Main, 2026-10-29)] DT-2 step 3 states that rule in one line and points to `AXON_REFERENCE.md`, which is generated from the `doc` string CS2-2 edits. DT-2 deletes §6, the only other place that describes `assert_eq_f64`. If CS2-2 does not land, the one line is wrong only where the interpreter's 1e-9 tolerance applies; the integrator drops the sentence's first clause in that case. |
| O-GAP-08 language forms (`Ok(())`, `if let`, `loop`, ...) and the misdirecting diagnostics | a separate gap spec | language and diagnostic changes; this spec adds only the AXON_PATH gotcha (DT-1) |
| Searching the entry file's own directory for `mod util` (`gaps-spec.md` NH1) | needs-human, already logged | product change to resolution; DT-1 only documents today's behaviour |
| O-GAP-10 (no offline model-backed ASI test) | separate spec | needs recorded-replay fixtures |
| Rewriting ROADMAP.md (last updated 2026-06-28) | NH-2 | product priorities; DT-5 only adds a pointer line |
| `AXON_KERNEL.md:57`, `VISION_OS.md:309`, `examples/goals/*`: "the compiler proves" about `@[contained]` grants | not changed | true: containment is a check-time refusal (E1001) |
| `crates/axon-cortex/benchmarks/hardset/src/asi/ad_optimizer.ax` and its JSON copies say "compiler-capped" | not changed | frozen benchmark snapshots: `cmp` against `examples/asi/ad_optimizer.ax` already differs, and nothing reads the example's bytes (`hardset/build.py:101-103` takes only the path) |

## 2. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | DT-1, DT-2, DT-7 | A model writing Axon reads these docs first. Today 3 of 10 guide blocks, 19 of 30 stdlib blocks and the CLAUDE.md quick reference fail `axon check`, and the guide's own §7 goal example is refused (E1504). DT-7 keeps it from coming back. |
| P2 | DT-3, DT-4, DT-5, DT-6 | Tour, README and ASI docs are wrong (tour §16 says annotations are "not yet enforced"). Stale ledger state hides what has shipped. DT-6 keeps it from coming back. |

## 3. Items

### DT-1 (P1) `spec/axon-for-llms.md`: three blocks that do not check, plus gotchas - O-GAP-09, O-GAP-08 (doc part)

**Reproduction.** I extracted every ```` ```axon ```` block into `snip/<doc>_<line>.ax` and ran
`axon check` on each (then `axon run` when the check passed and the block has `fn main`). The guide has 10 blocks
and 3 fail:

| lines | `axon check` (first error) | cause |
|---|---|---|
| 38-83 | rc 2, `E0000 line 6: unexpected token: Ident("x"), expected item (fn/type/enum/mod/use/trait/impl/let)` | top-level statements (`x = x + 1`, `while`, `println`) mixed with items |
| 208-211 | rc 2, `E0001 cannot find name `a` in this scope` | a fragment with free names |
| 243-253 | rc 2, `E1504 `tune` is a `@[goal]` function — it must take no parameters`, help: "`tune` is also `@[adaptive]`, and the two are opposite roles ..." | `@[goal("maximize score")]` and `@[adaptive]` on one fn |

The runtime's goal form (`checker.rs:1701-1763` R5 sugar; `interp.rs:6448-6516` `call_fn_goal`): `@[adaptive]` marks
the metric, which takes the params. `@[goal(metric: M, target: T, max_evals: N)]` goes on a separate zero-param fn.
Calling that fn runs the search and then its body, where `goal_met` is 1 when the best score reached `target`.
Appendix A2 is the replacement. `axon check` gives rc 0 (one W2001 vague-goal warning). `axon run` with
`AXON_SEED` = 1, 42 and 7 prints `goal met: 1, best threshold: 40` and `0`, rc 0 each time.

Also `stdlib.md` is called "(every builtin)" at :4 and "Full list in `spec/stdlib.md`" at :125. It covers 63 of
the 344 builtins in `axon reference --json` (DT-2 has the count).

**Fix (mechanical).**
1. Lines 38-83: replace the whole block with Appendix A1, fenced ```` ```axon ````. It moves items to the top and statements into
   `fn main`, defines `n`, and ends the `for` with a real body. `axon check` rc 0 (W0006 unused-variable warnings
   only); `axon run` rc 0.
2. Lines 208-211: the opener becomes ```` ```axon fragment ```` and the body becomes the four lines below.
   Wrapped in `fn main() { ... }` this checks rc 0 and runs rc 0 (`verify/gotcha2.ax`).
   ```
   let a = 1
   let b = 2
   let z = a +
           b          // OK — operator trails
   ```
3. Lines 243-253: replace the block with Appendix A2.
4. Line 4: `` `spec/stdlib.md` (every builtin), `` -> `` `spec/stdlib.md` (a curated guide to the builtins you reach for first), `AXON_REFERENCE.md` (every builtin, generated by `axon reference`), ``.
5. Line 125: ``No imports — all available everywhere. Full list in `spec/stdlib.md`.`` -> ``No imports — all available everywhere. Full list (with signatures): `AXON_REFERENCE.md`, or `axon reference --json`.``
6. Gotcha 4 (:216-218): after "Use `i64_to_f64` / `f64_to_i64` to convert" insert "(or `x as f64` / `x as i64`)".
   `probe/as_cast.ax`: `let n = 3.7 as i64` checks rc 0 and prints `3`.
7. Gotcha 6 (:222-223): append "`str_slice(s, a, b)` takes BYTE indices: an out-of-range bound clamps, but a
   bound inside a multi-byte character panics (E2200); use `str_char_slice` for character indices."
   (`probe/slice_mid.ax`: `str_slice("héllo", 0, 2)` -> `axon: panic: str_slice: E2200 byte range 0..2 splits a
   UTF-8 character ...`, rc 101. `probe/slice_oob.ax`: `str_slice("hello", 2, 99)` prints `llo`, rc 0.)
8. After gotcha 9 (:235) add gotcha 10:
   ```
   10. **A module file beside yours is not found without `AXON_PATH`.** `mod util` searches
      `AXON_PATH`, then `~/.axon/lib`, then `<bindir>/../lib/axon` — never the importing file's own
      directory. With `util.ax` next to `main.ax`, run `AXON_PATH=. axon run main.ax` (E0901 otherwise;
      the E0901 message names the directories it searched).
   ```
   Reproduced: `modp/main.ax` + `modp/util.ax`. `axon check main.ax` gives `E0901 module `util` not found |
   searched: /root/.axon/lib/util.ax (not found) | /var/tmp/r51-tmp/main-bin/../lib/axon/util.ax (not found) | hint:
   AXON_PATH is unset ...`. `AXON_PATH=. axon run main.ax` prints `42`, rc 0.

**Alternatives.** (a) Annotate the broken blocks as "illustrative": rejected, because a model copies them.
(b) Drop §2 and point at the tour: rejected, because §2 is the one-screen summary the guide exists for.

**Fail-first test.** DT-7 `every_axon_block_in_the_llm_facing_docs_checks` fails on today's tree with these 3 rows
listed. **Acceptance:** that test passes. **False-refusal risk:** none (docs only).

### DT-2 (P1) `spec/stdlib.md` becomes a curated guide; the reference is the complete list - O-GAP-09

**Reproduction.**
- Coverage. `axon reference --json` lists 344 builtins. stdlib.md has 17 `####` entries and 71 table rows: 63
  distinct names, all of them real builtins. `axon reference | diff -q - AXON_REFERENCE.md` gives SAME, so the
  checked-in reference matches the binary. `cli_run.rs:25725` `axon_reference_is_in_sync_with_the_binary` keeps
  it that way (`scripts/reference_gate.sh` is the manual equivalent; it is not on the gate path) [REVISED: DTD-2].
- 19 of 30 blocks fail `axon check`. The table in §3a lists them.
- "Planned" sections that have shipped. §6 "Planned Phase 4 Additions" (:487, "not present in Phase 1, 2, or 3")
  and §7 "Planned Phase 3" (:589) list `sqrt pow floor ceil assert_eq_str assert_eq_f64 read_line read_file
  write_file sleep_ms now_ms now_ns`. All 12 are in the reference. `chan_clone` is not: `p2/cc.ax` gives `E0001
  cannot find name `chan_clone``. `pow2` (:643) is not a builtin (E0001); `comptime { pow_i64(2, 8) }` checks and
  prints `256` (`verify/sl_comptime.ax`). "Trait-based display" (:624-635) never shipped: `println(5)` gives
  `E0306 argument 0 of `println` has the wrong type (expected str), found i64` (`probe/disp.ax`).
- `str_slice` "clamps" (table :300, `builtins.rs:494` doc string, `AXON_REFERENCE.md:607`). Only an out-of-range
  bound clamps (`probe/slice_oob.ax`: `llo`, `he`, rc 0). A bound inside a UTF-8 character panics
  (`probe/slice_mid.ax`, quoted in DT-1, rc 101; `interp/builtins.rs:3260-3269`).
- `str_cmp` "can be passed as a comparator" (table :296, `builtins.rs:469-470`, `AXON_REFERENCE.md:592`, "Drops
  directly into `arr_sort_by`"). `probe/cmp_sort.ax`, `arr_sort_by(["b","a","c"], str_cmp)`, gives `E0306 `str_cmp`
  is a builtin, which cannot be used as a value — only a non-generic user-defined `fn` can be passed, stored or
  bound by name`, rc 2. The lambda form `|a, b| str_cmp(a, b)` checks, and run prints `a` (`verify/sl_strcmp.ax`).
- §4 "Type Coercions at Call Sites" describes LLVM codegen (`sitofp`, `fptosi`) that the checker never allows.
  `abs_f64(5)` gives `E0306 argument 0 of `abs_f64` has the wrong type (expected f64), found i64`, rc 2.
  `abs_i64(3.7)` gives E0306, rc 2. The one true part: `abs_i32(5000000000)` prints `705032704` (the result is
  truncated to i32).
- §5 "Known Issues and Phase 2 Fixes" (:399-486) is a history of Phase-1 codegen bugs. Its `format` warning
  ("may produce unexpected results") is false today: `println(format("x {1+13}"))` prints `x 14`, rc 0
  (`p2/fmt.ax`).
- `assert_eq` is i64-only, as the doc says: `assert_eq(to_str(1), "1")` gives two E0306. The promised generic form
  never shipped.

**Decision: curated guide (not generated).** The complete list already exists, is generated, and is gated:
`AXON_REFERENCE.md` (CLAUDE.md:247-254, `main.rs:5280-5292`, `cli_run.rs:25725`
`axon_reference_is_in_sync_with_the_binary`). A second generated copy would add nothing. A hand-kept list of 344 rows is what went
stale. The guide's value is the prose: which builtin to reach for, and the surprises. Alternatives: (b) generate
stdlib.md from `BUILTINS`, rejected as a duplicate of AXON_REFERENCE.md; (c) keep the table and add a drift test,
rejected because it re-documents 344 signatures by hand.

**Fix (mechanical), in file order.**
1. :1-4: title `# Axon Standard Library — a curated guide`. Replace :3-4 (`**Version**` / `**Applies to**`) with:
   "This guide covers the builtins a model reaches for first and the ones that surprise. The complete list, with
   every signature and doc string, is `AXON_REFERENCE.md` (generated by `axon reference`; `axon reference --json`
   for tools). A test (`axon_reference_is_in_sync_with_the_binary`) fails when that file and the binary disagree.
   Where this guide and the reference differ, the reference is right." [REVISED: DTD-2 - was "`scripts/reference_gate.sh`
   fails the gate", but that script is not on the gate path]
2. §1 :10-15: replace the paragraph with "Axon's builtins are compiler-known and injected into the global scope
   before your program begins: there is no import to forget. Everything this guide names is available everywhere
   unless it is listed under "Not shipped"." [REVISED: DTA-3 - :10 "every builtin" and :12 "If a function appears
   in this document it is available everywhere" became false once §5 names `chan_clone`; was a :14-15 deletion only]
3. §2 blocks, by opener line:
   - :29, :43, :55, :67, :210 -> opener ```` ```axon fragment ````, body unchanged. Each one, wrapped, checks rc 0 and
     runs rc 0.
   - :83-85 -> ```` ```axon fragment ```` with the body of `verify/sl_assert.ax` (`let result = 6 * 7` /
     `assert(result > 0)`).
   - :95-97 -> Appendix A5 (`sl_asserteq`, a program; `axon test` 1 passed).
   - :112-118 -> Appendix A6 (`sl_asserterr`, a program; `axon test` 1 passed).
   - :194-197 -> ```` ```axon fragment ```` with body `let count = 3` / `println(to_str(42))        // prints: 42` /
     `println("result: " + to_str(count))`.
   - §2 Math, `min_i32` (:246) and `max_i32` (:258): replace "(result truncated to i32 range)" with "(the
     interpreter does not truncate today: `max_i32(5000000000, 1)` is `5000000000`, O-DT-01; prefer `max_i64`)",
     using `min_i64` / "lesser" wording in the `min_i32` entry. [REVISED: DTA-2, `rev/m.ax`: check 0, run prints
     `705032704`, `5000000000`, `5000000000`]
   - Delete the three "Phase 1 note" / "Phase 2 limitation" blockquotes (:99-101, :120-122). Replace the first with
     "`assert_eq` compares `i64` only; use `assert_eq_str` for `str` and `assert_eq_f64` for `f64`.
     `assert_eq_f64` compares with exact IEEE equality (NaN never equals); see `AXON_REFERENCE.md`."
     [REVISED: CS2-2 decided exact IEEE equality in both engines (Main, 2026-10-29); was "see AXON_REFERENCE.md for
     how each compares"]
4. §3 "Builtin Availability Table" (:268-338): delete the whole table. Replace with `## 3. Strings: three surprises`
   holding Appendix A7 (`sl_strcmp`) and these three sentences: "`str_cmp` (like every builtin) is not a value:
   wrap it in a lambda to pass it to `arr_sort_by`. `str_slice` takes byte indices; an out-of-range bound clamps,
   but a bound inside a multi-byte character panics (E2200). `str_char_slice` indexes by character."
5. §4 (:341-389): keep the heading `## 4. Type Coercions at Call Sites`. Replace :343-382 with: "There are no
   implicit numeric coercions. An `i64` where an `f64` is expected (or the reverse) is E0306 at check: convert
   with `as` or `i64_to_f64` / `f64_to_i64`. `abs_i32` takes an `i64` and truncates its result to the i32 range:
   `abs_i32(5000000000)` is `705032704`. Use the `_i64` forms." [REVISED: DTA-2 - `max_i32(5000000000, 1)`
   returns `5000000000` untruncated, so the claim about `min_i32`/`max_i32` was false; the doc-vs-behaviour mismatch
   of those two is logged as a new opportunity in DT-5, not fixed here] Follow it with Appendix A8
   (`sl_coerce`, fragment). Keep :384-395 ("No coercions between unrelated types", "Array arguments are never
   `&mut`") unchanged.
6. §5 (:399-485): delete.
7. §6 and §7 (:487-648): delete. Replace with `## 5. Not shipped` holding: "`chan_clone` (E0001; channels have
   `ch.send(v)` / `ch.recv()`), trait-based display (`println` takes `str` only; `println(5)` is E0306), a
   generic `assert_eq<T>`. `comptime { }` blocks DO ship:" followed by Appendix A9 (`sl_comptime`, fragment).
8. §8 Naming Conventions: renumber to §6, text unchanged. "New builtins checklist" (:687-694): delete step 3 ("Add
   it to the availability table in this document") and step 4 ("Note which phase introduced it"). Add: "3. Run
   `axon reference > AXON_REFERENCE.md` (the gate fails until you do). 4. Add it to this guide only if a model
   would otherwise misuse it." Renumber step 5 to 5.
9. `crates/axon-core/src/builtins.rs:469-470` (`str_cmp` doc): replace "Drops directly into `arr_sort_by`, whose
   comparator is `fn(T, T) -> i64`." with "To sort with it, pass a lambda: `arr_sort_by(xs, |a, b| str_cmp(a, b))`
   (a builtin is not a value, E0306)."
10. `builtins.rs:494` (`str_slice` doc): replace "Clamps indices to valid range." with "An out-of-range index
    clamps; an index inside a multi-byte character panics (E2200) - use `str_char_slice` for character indices."
11. Regenerate: `axon reference > AXON_REFERENCE.md`. The `doc` field feeds only `axon reference`
    (`main.rs:5254`, `:5376`); no checker or interpreter path reads it.
11a. [REVISED: DTD-1] Steps 9-10 are their own commit. `scripts/pci_delta.py` fails S16 (`pci_delta_note.rs`) on a
    commit under `crates/axon-core/src` that no theme classifies. So that commit adds
    `governance/notes/pci-themes/<hash8>.json` = `{"theme": "docs-truth DT-2 (doc strings only)", "text": "doc field
    of str_cmp and str_slice; read only by `axon reference` (main.rs:5254/:5376); no change to what any run accepts,
    refuses or computes"}`, then runs `python3 tasks/r51-pci-regen.py`. G1 adds `python3 scripts/pci_delta.py
    --check` rc 0. The integrator owns the theme file and the regen, the same owner as checker-soundness-2's
    themes (its §4, "integrator-only").
12. Inbound "every builtin" claims: `AGENTS.md:14` "every builtin with signature and semantics" -> "a curated
    builtin guide (the complete list is `AXON_REFERENCE.md`)". `CLAUDE.md:349` "Standard library reference" ->
    "Curated builtin guide (complete list: AXON_REFERENCE.md)".
13. Only if NH-1 = (a) (recommended; not yet decided): the governance checklists stop requiring a stdlib.md entry:
    `DEFINITION_OF_DONE.md:52` -> "- [ ] Public surface documented: a builtin by its `doc` string in `builtins.rs`
    (AXON_REFERENCE.md regenerated, test-checked); syntax or an error code in the relevant `spec/*.md`; add to
    `spec/stdlib.md` or `spec/axon-for-llms.md` only when a model would misuse it without guidance";
    `CODE_REVIEW_RUBRIC.md:57` -> "- [ ] Public surface documented (builtin: `doc` string + AXON_REFERENCE.md
    regenerated; otherwise the relevant spec)."; `BUILD_PROTOCOL.md:121` `` `spec/` or `stdlib.md` `` -> `` `spec/`
    (a builtin: its `doc` string and a regenerated `AXON_REFERENCE.md`) ``.

**Fail-first test.** DT-7 lists the 19 stdlib rows on today's tree. `cli_run.rs:25725`
(`axon_reference_is_in_sync_with_the_binary`) fails if step 11 is skipped. **Acceptance:** both pass;
`grep -n "stdlib.md. (every builtin\|every builtin with signature\|If a function appears in this document" spec/stdlib.md spec/axon-for-llms.md AGENTS.md`
gives no line (today: 3 lines, `stdlib.md:12`, `axon-for-llms.md:4`, `AGENTS.md:14`; the replacement texts: 0).
[REVISED: DTA-3 - was `grep -c "every builtin"`, which cannot reach 0: DT-1's line-4 replacement and step 4's
`str_cmp` sentence legitimately say "every builtin" about the reference and about values.]
**False-refusal risk:** none.
The doc string is not read by check or run, so the cs-sweep is unchanged (§5).

### DT-3 (P2) `spec/language-tour.md`: six blocks, and §16 says annotations are not enforced - O-GAP-09

**Reproduction** (26 blocks, 6 fail):

| lines | `axon check` | fix |
|---|---|---|
| 376-387 | `E0001 cannot find name `apply`` | add `fn apply(f: fn(i64) -> i64, x: i64) -> i64 { f(x) }` after :378 (`verify/tour_apply.ax`: check 0, run prints 42 x3) |
| 400-405 | `E0000 unexpected token: Ident("print")` | opener -> ```` ```axon fragment ```` (wrapped: check 0, run 0) |
| 408-413 | E0000 (and E0307 when wrapped: the last line is a value) | replace with Appendix A10 (`tour_conv3`). It also fixes two wrong comments: `to_str_f64(3.14)` prints `3.14`, not `"3.140000"`, and the Err text is ``could not parse `bad` as a base-10 integer`` |
| 416-421 | E0000 / E0307 | replace with Appendix A11 (`tour_math`; prints 5, 3.14, 3, 7) |
| 424-426 | E0000 / E0307 | replace with Appendix A12 (`tour_str`; prints 5, 6) |
| 470-486 | `E0102 type mismatch in function body (expected [Task]), found ()`, plus `W0001 unknown attribute `@[verify]`` and E1504 | replace with Appendix A3 (`tour16`) |

§16 heading `## 16. AI Annotations (Deferred)` -> `## 16. AI Annotations`. The sentence at :468 ("These are
recognized by the toolchain but not yet enforced by the Phase 2 compiler") becomes "`@[adaptive]` / `@[goal]`
drive the optimizer, `@[verify]` is a runtime-checked postcondition (a breach exits 3), and `@[contained]` is
enforced at check (`spec/axon-for-llms.md` §7, "Capability sandboxing")." [REVISED: DTA-8 - there is no heading of
that name; it is a bold lead inside §7] The A3 block: `axon check` rc 0 (one
W2001); `AXON_SEED` = 1, 42 and 7 each print `1`, `12`, `9`, rc 0.

**Acceptance:** DT-7 passes. **False-refusal risk:** none.

### DT-4 (P2) README, CLAUDE.md quick reference, and the ASI docs - O-GAP-09

**4a. `README.md`** (11 blocks, 2 fail check, 1 more exits non-zero by design):
- :13-21: `E1001 `read_file("/etc/passwd")` is not permitted: no `fs: [read(...)]` in @[contained]`. This failure
  is the block's whole point, so the opener becomes ```` ```axon fails E1001 ```` (DT-7 then requires the check to
  fail with E1001).
- :232-238: `E0001 cannot find name `add``. Insert `fn add(a: i64, b: i64) -> i64 { a + b }` and a blank line
  above `@[test]` (Appendix A13; `axon test` passes).
- :189-209 (Concurrency): check rc 0; `axon run` exits 45, because `fn main() -> i64` returns `sum` and run's exit
  code is main's return value (`/var/tmp/r51-tmp/DocsDraft/readme189.ax`: check=0, run=45). The block is correct;
  only its opener changes, to ```` ```axon exits 45 ```` (DT-7 marker). [REVISED: DTA-1 - the first draft missed it,
  so DT-7 would have failed after DT-1..DT-4]

**4b. `CLAUDE.md:376-463`** (the "Quick reference" block). `E0000 line 8: unexpected token: Ident("x")`. It also
teaches `@[goal("maximize throughput")] @[adaptive]` and a bare `@[verify]` (W0001) under "AI annotations
(deferred, emit info not errors)". Replace the block body with Appendix A4. That keeps every fact in the old block
(literals, modules, contained, wrapping semantics), moves statements into `fn main`, and replaces the deferred
section with the shipped goal/verify forms. Check rc 0 (W2001 only), run rc 0. `scripts/claims_gate.sh` reads
CLAUDE.md's verb names; A4 names `axon check` and `axon reference`, both real verbs, so it still passes [REVISED:
DTA-9 - was "the block names none"].

**4c. Spend-cap wording** (the bound holds at run time; the wording claims compile time):
- `examples/asi/ad_optimizer.ax:18`: `— a hard SAFETY bound the compiler proves:` -> `— a hard SAFETY bound,
  checked on every call:`.
- `ad_optimizer.ax:64`: `compiler-ENFORCED safety bound` -> `runtime-ENFORCED safety bound` (keep the ─ padding
  to the same width).
- `ad_optimizer.ax:101`: `(compiler-capped at 500)` -> `(capped at 500 by @[verify])`.
- `crates/axon-core/tests/cli_run.rs:5601`: `stdout.contains("compiler-capped at 500")` ->
  `stdout.contains("capped at 500 by @[verify]")`.
- `examples/asi/README.md:17` and `:30` first cell: `compiler-capped at 500` -> `capped at 500 by @[verify]`.
  `:30` third cell: `a compiler-enforced safety bound, not a hope` -> `a runtime-enforced safety bound (a breach
  exits 3), not a hope`.

Evidence that this is the truth: `ad_optimizer.ax:65-69` already says "The runtime gate enforces it ... this one
returns an Uncertain so it uses the runtime gate". `probe/verify_compound.ax`
(`@[verify(value >= 0 && confidence >= 0.9)]` on an `Uncertain<i64>` fn) checks rc 0. `g(5)` prints 5; `g(-1)` gives
`axon: verify failed: verify failed in `g`: composite predicate did not hold (value -1, confidence 0.95, input
-1)`, rc 3.

**4d. Friction list** (`examples/asi/README.md:260-351`). Each entry was re-run (`probe/`, `AXON_SEED=42`). Rule:
prefix a shipped entry's bold title with `[SHIPPED]` and replace its `→ Phase N` line with `→ shipped: <evidence>`.
A partly shipped entry gets `[PARTLY SHIPPED]` and one sentence on what remains. Entries are never deleted, so the
numbering cited elsewhere stays valid.

| # | entry | observed | new tag and `→` line |
|---|---|---|---|
| 1 | `goal_run` only hill-climbs `fn(i64) -> i64` | `multiarg.ax` (`fit(a, b)`): prints `100`, `3,7` (via `goal_best_inputs`), rc 0. `f64adapt.ax` (`fn t(x: f64) -> f64`): `100`, rc 0 | `[SHIPPED]` multi-arg and f64 `@[adaptive]`; `goal_best_inputs` |
| 2 | no deterministic replay | `rec.ax` + `AXON_RECORD=r3.journal`, then `axon replay r3.journal`: "1 host interaction(s)", rc 0; `AXON_REPLAY=r3.journal axon run rec.ax` re-prints the recorded `now_ms` value, rc 0 | `[SHIPPED]` `AXON_RECORD` / `AXON_REPLAY` / `axon replay` |
| 3 | no typed audit log | the provenance record has `"principal"`, `"event":"ai_call"`, `tier`, `model`, `params_hash`, `prompt_hash` (193,150 records with `"principal"`). A link to the triggering goal was not checked | `[PARTLY SHIPPED]` records carry principal, event and model; the goal link is not verified |
| 4 | no budget meter | `bud.ax` with `AXON_AI_MOCK=1 AXON_BUDGET_TOKENS=1`: `axon: ai policy: [E1303] `main` exceeded the run's token budget of 1 ...`, rc 5 | `[SHIPPED]` token budget (`AXON_BUDGET_TOKENS`, E1303). A dollar ceiling was not checked |
| 5 | no sandbox for AI-emitted plans | `eff.ax` with `AXON_ALLOWED_EFFECTS=Pure`: `axon: sandbox violation: builtin `now_ms` requires effect `Time` which is not in the active sandbox's allowed set {"Pure"}`, rc 8 | `[PARTLY SHIPPED]` a runtime effect sandbox exists; there is no tool-call surface for an LLM plan to use [INFERENCE: none found in the reference verbs] |
| 6 | `@[verify]` is `confidence OP K` only | `verify_compound.ax` (4c) | `[SHIPPED]` compound `value` / `confidence` predicates |
| 7 | no digit-only filter | `digits.ax`: `str_digits_only("415-555-0142")` -> `4155550142`, rc 0 | `[SHIPPED]` `str_digits_only` |
| 8 | no `f64` cast operator | `as_cast.ax`: `3.7 as i64` -> `3`, rc 0 | `[SHIPPED]` `as` |
| 9 | no multi-arg `@[adaptive]` | same as 1 | `[SHIPPED]` |
| 10 | no reward-shaping syntax | not probed (no `Reward` in `AXON_REFERENCE.md`) | unchanged |
| 11 | `@[adaptive]` records no inputs | `warm.ax`: `goal_run` then `goal_continue`, and `goal_best_input` -> `20`, rc 0 | `[SHIPPED]` `goal_best_input(s)`, `goal_continue` warm start |
| 12 | no agent abstraction | `examples/stdlib/agent.ax` is a userland Agent module (7 tests per its header); `@[agent]` is a reference attribute | `[PARTLY SHIPPED]` a userland Agent module; composition not verified |
| 13 | no structured-prose surface | `axon goal --help`: "Compile a structured-prose goal file (`*.md`) to AST and run it." | `[SHIPPED]` surface exists (`axon goal`); not exercised by this demo |
| 14 | no human-in-the-loop approval | `axon ast --help`: `approve  Record human approval of a .ax AST (Phase-10 step 3)` | `[SHIPPED]` surface exists (`axon ast approve`); not exercised by this demo |
| 15 | no simulate -> deploy pipeline | `axon deploy --help`: "Run a .ax program through the full safety-gate pipeline, then deploy." | `[PARTLY SHIPPED]` `axon deploy` exists; no simulator or stress stage |

Also: `README.md:226` `## Phase-10 CLI surface (simulated)` -> `## Phase-10 CLI surface`, and :228-229 ->
"`run.sh` predates the shipped verbs; the second column names the verb that now does the job." Column header
`Future name` -> `Shipped verb`. The verb list is `axon reference --json` `.verbs` (27 verbs: ai, ast, build,
cache, caps, check, complexity, deploy, doc, fmt, goal, imports, improve, intent, lock, lsp, parse, redteam,
reference, replay, run, session, target, test, trace, verify, verify-lock). Cell rule: `compile` -> `axon ast
review` (`axon ast --help` lists `review` and `approve`); `run` -> `axon run`; `trace` and `audit` -> `axon
trace`; `improve` -> `axon improve`; `redteam` -> `axon redteam`; `replay` -> `axon replay` (with `AXON_RECORD`);
`log` and `clear` -> `none (run.sh only)`, because there is no `log` verb. Only `ast`, `deploy` and `goal` `--help`
were run; the others are taken from the verb list. Failure mode 4 (:254-255):
`*not yet exercisable*. The English-surface compiler is Phase 10.` -> `not exercised by this demo; `axon goal`
compiles a structured-prose `.md`.`

**4e. `AXON_PATH` for the four stdlib-importing demos.** `bandit_ucb.ax:8`, `persistent_bandit.ax:20`,
`safe_bandit.ax:28` and `safe_self_improve.ax:17` already carry `// AXON_PATH=examples/stdlib axon run ...` in
their headers. The all-demos test sets it (`cli_run.rs:21118-21170`, `.env("AXON_PATH", &path)` at :21153 with
`examples/stdlib`). Two places a reader runs them do not:
- `examples/asi/run.sh`: `AXON=... DEMO=bandit_ucb.ax bash examples/asi/run.sh check` fails with `E0901 module
  `bandit` not found`. `AXON_PATH=examples/stdlib axon check examples/asi/bandit_ucb.ax` gives rc 0. Fix: after
  `run.sh:15` (`REPO=...`) add `export AXON_PATH="${AXON_PATH:-$REPO/examples/stdlib}"`. A caller's own
  AXON_PATH still wins. `cli_run.rs:8948-9030` drive run.sh with `optimize.ax`, which imports nothing, so they are
  unaffected.
- `examples/asi/README.md` "Running" (:193-224): after the `bash` block add "Four demos import
  `examples/stdlib` modules (`bandit_ucb`, `persistent_bandit`, `safe_bandit`, `safe_self_improve`): run them
  with `AXON_PATH=examples/stdlib` from the repo root (`run.sh` sets it for you), or `E0901 module ... not found`."

**Fail-first test.** `cli_run.rs` `ad_optimizer` test (:5577-5602) after the println change; DT-7 for 4a/4b.
Friction-list and README text are prose, so no test (the probes above are the evidence). **Acceptance:** DT-7 and
`cli_run` pass; the run.sh command above gives rc 0. **False-refusal risk:** none.

### DT-5 (P2) Ledger state: Status lines, closure stamps, CHANGELOG, ROADMAP - O-GAP-11

**Reproduction.** I ran the DT-6 prototype (`/var/tmp/r51-tmp/DocsDraft/spec_ledger_proto.py`) on 4a6436da:
`spec_ledger: FAIL (28 problem(s), 17 specs)`, 0.13 s (17 specs counts this one, which adds no problem; the 28th is
gate-lane's Status naming no ship sha, found once rule 2 read only the bold span [REVISED: DTA-6]). It flagged the issues below. I verified each one against
`git log --first-parent` and CHANGELOG before writing a stamp. Every sha below passes
`git merge-base --is-ancestor <sha> 4a6436da`.

| spec | today | evidence | new Status line (inserted as line 3, blank line after) |
|---|---|---|---|
| approval-residue | "DRAFT ... MUST pass Step 1" | 3a4a5508 `merge gaps2/approval: AR1 AR2 AR3`, ef1b27ee `merge gaps2/core: AR4 AR5 AR7`; CHANGELOG "Gaps2" section (:56); AR6 EXCLUDED by human (H3) | `**Status: BUILT 2026-10-17 on `main` 3a4a5508 + ef1b27ee (AR1-AR5, AR7; AR6 EXCLUDED by human as H3). Stamped by tasks/docs-truth-spec.md DT-5.**` |
| killgroup | "DRAFT" | 82a2d51a `merge gaps2/jobs`, 00f84160 `implementation-review fixes`; CHANGELOG "Process-group jobs in one crate (KG1)" | `... BUILT 2026-10-17 on `main` 82a2d51a + 00f84160 (KG1-KG3, KG5; KG4 EXCLUDED by human) ...` |
| lsp-residue | "DRAFT" | c691a51b `merge gaps2/lsp: L1 L2 L3` | `... BUILT 2026-10-17 on `main` c691a51b (L1-L3) ...` |
| harness-hygiene | "DRAFT" | 8afc76ba `merge gaps2/harness: H1-H7`, d2d91d0b `implementation-review fixes` | `... BUILT 2026-10-17 on `main` 8afc76ba + d2d91d0b (H1-H7) ...` |
| gaps | no Status line | 3060b866 `gaps: tracker rows for G1-G5, changelog, spec status`; its "Status (end of run)" table (:205-222) shows G1-G5 done and NH1 not built | `... BUILT 2026-10-15 on `main` 3060b866 (G1-G5, per "Status (end of run)" below; NH1 entry-directory module search was needs-human and is not built, see O-GAP-08) ...` |
| repair | no Status line | 4f9d883e `docs(S1): receipt PASS ... in CHANGELOG and tracker`; CHANGELOG "S1: governed repair episode" (:40) | `... BUILT 2026-10-18 on `main` 4f9d883e (R0-R5, R7, R8; R6 deferred to tasks/r6-ledger-spec.md) ...` |
| followup | `Status: REVIEWED 2026-10-18 ...` (not bold, so no tool reads it) | FU-2...FU-9 merged via `fu/int` (791dee95 `Merge branch 'fu/progs' into fu/int` is an ancestor); FU shas ecf63dfc ac5876df 098786e7 7c5d2136 392acddc 2338ceca 30fb8841 are all ancestors and stamped in opportunities (`[RESOLVED fu/int FU-n, sha]`); FU-1/10/11 went to hx2 (STOPPED at HX-0) | replace `Status: REVIEWED 2026-10-18 (Step 1: ` with `**Status: PARTIAL 2026-10-18 on `main` 791dee95 (FU-2...FU-9 built on `fu/int`; FU-1, FU-10, FU-11 moved to tasks/hx2-spec.md, STOPPED at HX-0, so O-S1-02 stays open). Stamped by tasks/docs-truth-spec.md DT-5.**` + blank line + `Reviewed 2026-10-18 (Step 1: ` |
| small-fixes | `**Status: DRAFT (after Step 1 review). Not authorized for build.**` | CHANGELOG :20-26 names SF-1, SF-2, SF-4, SF-5, SF-6 (changelog commit 1d845fea); SF-1 70d38c0e, SF-2 de6be69e + cb93bf7c, SF-3 d38abb5b, SF-4 19d64dd6, SF-5 2ba68e53, SF-6 43208ed8, all first-parent ancestors of 4a6436da | replace the line with `**Status: BUILT 2026-10-28 on `main` 43208ed8 (SF-1...SF-6; SF-1...SF-4 via `sf/int`, last item SF-6 43208ed8). Stamped by tasks/docs-truth-spec.md DT-5.**` |
| intent-gen | `**Status: Step 1 reviewed (2026-10-18, ...` | not built: CHANGELOG has no `IG-` id and no `intent-gen` mention | `**Status: Step 1 reviewed (2026-10-18, ` -> `**Status: REVIEWED 2026-10-18 (Step 1, ` |
| gate-lane | `**Status: BUILT 2026-10-29 on `main` (GL-1..GL-5 ...` names no ship sha; today it passes rule 2 only because a later sentence cites fix commit `0b29b054` | 0bd2b518 `gate-lane: BUILT status, review revision marker, O-GL-01/O-GL-02` (first-parent, ancestor of 4a6436da) | replace `on `main` (GL-1..GL-5` with `on `main` 0bd2b518 (GL-1..GL-5` [REVISED: DTA-6] |

The stamp format matches existing stamps (`[RESOLVED fu/int FU-2, ecf63dfc]`): a backticked bracket after the
entry's last line. Text below; the build adds the stamp date.

| id | stamp | re-verified 2026-10-29 (`closure/*.ax`) |
|---|---|---|
| O-S1-19 | `[BUILT 2026-10-27 on main 49b576fa: capability-surface CAP-1.]` | `S1_19.ax` `@[verify(len(read_file("x")) > 0)]`: E1103 (ill-typed `len` operand) then `E1104 `@[verify]` predicate of `f` reaches `read_file` (effect `IO`) - a postcondition may only read the clock`, rc 2 [REVISED: DTA-10] |
| O-S1-20 | `[BUILT ... CAP-2; pinned by predicate_capability_routes.rs the_effect_union_carries_the_row_a_predicate_route_implies.]` | not re-run (test name read) |
| O-S1-22 | `[BUILT ... checker-soundness CS-1.]` | `S1_22.ax`: check rc 0; run `refinement violated: field `value` of `Amt` (= -3) ...`, rc 6, no panic |
| O-S1-23 | `[BUILT ... CS-2/CS-3 (NH-1 a: predicates bind the params).]` | `S1_23.ax` (clampv): check 0; run prints 3, rc 0 |
| O-S1-24 | `[BUILT ... CS-5, CS-7, CS-11, CS-12. The param-used-in-two-match-arms sub-item does not reproduce; WITHDRAWN.]` | `S1_24_unit.ax` `type Unit` -> E0005; `S1_24_twoarms.ax` check 0, run prints 3 |
| O-FU-05 | `[BUILT ... CAP-2 + CAP-3; pinned by the_program_wide_surface_agrees_with_the_reach_walker.]` | not re-run |
| O-FU-06 | `[BUILT ... CS-4..CS-8, CS-10 (NH-3 d: str stays a move type, by design).]` | `FU_06.ax` `fn ln(a,b,c,d)` + `ln(1,2,3,4)`: `E0305 function `ln` takes 1 argument but 4 were supplied` + E0306, rc 2 |
| O-SP-01 | `[BUILT ... CAP-1.]` | `SP_01.ax` `| {}` fn whose verify writes a file: E1104 + E1310, rc 2 |
| O-SP-04 | `[BUILT ... CAP-4.]` | `SP_04.ax`: two `E1310 `g` calls `ai_extract::<i64>`, which performs effect `AI``/``Net``, rc 2 |
| O-SP-07 | `[BUILT 2026-10-29 on main 3658ffcb: gate-lane GL-5 (g01_anchors.py and test_harness_skip.sh are full-gate par stages, gate.sh:713-714; vm_parity.sh is stage S22).]` | `gate.sh:713-714` read |
| O-FU-01 | `[BUILT 2026-10-28 on main 70d38c0e: small-fixes SF-1.]` | CHANGELOG :24 |
| O-FU-02 | `[BUILT 2026-10-28 on main de6be69e + cb93bf7c: small-fixes SF-2.]` | CHANGELOG :25 |
| O-FU-07 | `[BUILT 2026-10-28 on main d38abb5b: small-fixes SF-3. The wider leak stays open as O-SP-05.]` | commit read |
| O-S1-21 | `[BUILT 2026-10-28 on main 19d64dd6: small-fixes SF-4.]` | CHANGELOG :26 |
| O-CS-03 | `[BUILT 2026-10-28 on main 43208ed8: small-fixes SF-6.]` | `CS_03.ax` `let f = abs_i64` with a shadowing user fn: E0306, rc 2 |
| O-CS-04 | `[BUILT 2026-10-28 on main 2ba68e53: small-fixes SF-5.]` | `CS_04.ax` `let mut x`: only note I0002, check rc 0 |

O-S1-02 stays open: it went to hx2, which is STOPPED. O-SP-03 and O-SP-05 stay open (logged, not built).

**New opportunity** [REVISED: DTA-2]. Append to `tasks/opportunities.md` after O-GAP-11 (before the "O003
re-assessed" paragraph):
`### O-DT-01 - [low] `min_i32` / `max_i32` do not truncate, though their doc strings say they do` /
`Found drafting docs-truth DT-2 (Step 1, DTA-2). `builtins.rs:142` and `:148` say "(truncated to i32 range)";
`interp/builtins.rs:2896-2900` handles `min_i32` with `min_i64` and `max_i32` with `max_i64`. Reproduced:
`min_i32(5000000000, 6000000000)` -> `5000000000`, `max_i32(5000000000, 1)` -> `5000000000` (check 0, run 0);
`abs_i32(5000000000)` -> `705032704` does truncate. Fix direction: decide the semantics (truncate like `abs_i32`,
or drop the `_i32` names), then make doc, interpreter and axon-rt agree. Not fixed by docs-truth.`
The guide's `min_i32` / `max_i32` entries stop claiming truncation (DT-2 step 3), so the docs state nothing false
meanwhile; the `builtins.rs:142/148` doc strings stay until O-DT-01 decides the semantics.

**CHANGELOG.md**. It has no entry for the CS/CAP batch: `grep -n "E1211\|E1104\|CAP-[1-4]\|(CS-" CHANGELOG.md`
finds one line (:36, the unrelated `builtin-typevar-instantiation` port). It has no `FU-2`..`FU-9` and no `SF-3`
(`grep -n "FU-[0-9]\|SF-3"` finds only the `O-FU-01`/`O-FU-02` ids on :24-25). Add three blocks; the text is in
Appendix C:
- after :26 (the last small-fixes bullet): the SF-3 bullet;
- after that bullet, a blank line and the `## Checker soundness and capability surface ...` section, so it sits
  before :28 `## Requirements-evidence integrity checker`;
- after :39 (end of the section holding the `str_cmp` fuzz bullet at :38), before `## S1:` at :40: the
  `## S1 follow-ups ...` section.

**ROADMAP.md**: after :5 (`spec/compiler-phaseN.md` (per-phase specs).) add `**Build specs**: `tasks/*-spec.md`.
`python3 scripts/spec_ledger.py --list` prints each spec's status and the commit it shipped at; the full gate
fails when a status line and `tasks/opportunities.md` disagree.`

**At the end of the build** (by the integrator): stamp O-GAP-09 and O-GAP-11 `[BUILT <date> on main <sha>:
docs-truth DT-1..DT-7]`; add `[PARTLY: docs-truth DT-1 gotcha 10 documents AXON_PATH; the language forms stay
open]` to O-GAP-08; add a CHANGELOG section for DT-1..DT-7; set this spec's Status to
`**Status: PARTIAL <date> on `main` <sha> (DT-1..DT-7; O-GAP-08 stays open: only its AXON_PATH gotcha is done).**`
(PARTIAL, not BUILT: see the note after Appendix B).

**Acceptance.** `python3 scripts/spec_ledger.py` passes on the built tree. I already got
`spec_ledger: PASS (0 problem(s), 17 specs)` on the scratch tree `/var/tmp/r51-tmp/DocsDraft/post`, which holds
exactly these edits plus this spec (run with `SPEC_LEDGER_GIT` = the worktree). **False-refusal risk:** none.

### DT-6 (P2) Mechanism: `scripts/spec_ledger.py` as a full-gate par stage - O-GAP-11

**Design.** Python 3 stdlib only, offline, no cargo. The only subprocess is `git merge-base --is-ancestor`, once
per BUILT/PARTIAL spec. Measured 0.13 s on 17 specs. Source: Appendix B (the prototype, verified on today's tree
and on the post tree). Rules:
0. An item id (a `##`/`###` heading id such as `SF-3`) is unique across `tasks/*-spec.md`; a duplicate is
   reported as `<spec>: item <ID> also defined in <other>`, so a writer sees the collision at draft time instead of
   as a rule-4 failure later. 0 collisions on today's 17 specs plus checker-soundness-2. [REVISED: DTA-5]
1. Every `tasks/*-spec.md` has `**Status: <WORD>` in its first 15 lines, where WORD is one of DRAFT, REVIEWED,
   AUTHORIZED, BUILT, PARTIAL, STOPPED, SUPERSEDED.
2. BUILT/PARTIAL names a commit (7-12 hex) that is an ancestor of HEAD, inside the bold Status span (up to the
   closing `**`); a sha in later prose does not count. [REVISED: DTA-6 - gate-lane passed only through a fix-commit
   sha in a later sentence; DT-5 gives it its ship sha]
3. BUILT/PARTIAL: CHANGELOG.md names at least one of the spec's item ids (`##`/`###` headings like `SF-3`, `CS-1`,
   `AR4`), or names the spec file.
4. DRAFT/REVIEWED/AUTHORIZED: CHANGELOG.md names none of the spec's item ids. This catches a shipped spec whose
   status never moved: today it flags small-fixes, gaps, approval-residue, and others.
5. BUILT/PARTIAL: every `O-*` id cited in the `Source` paragraph (from a line starting `Source`/`**Source` in the
   first 40 lines to the next blank line, so a wrapped paragraph counts whole) or in an item heading has an entry in
   `tasks/opportunities.md`, and that entry carries a closure stamp (`[RESOLVED|FIXED|DONE|CLOSED|BUILT|WITHDRAWN|
   SUPERSEDED`). Exception: an id that a PARTIAL status line itself names as open. [REVISED: DTA-6 - the first
   draft read only the first physical `Source` line; this spec's own Source wraps over four lines]
6. Reverse: an opportunities entry whose text says `SPECCED ... tasks/X-spec.md`, where X is BUILT/PARTIAL,
   carries a closure stamp.
7. ROADMAP.md contains `scripts/spec_ledger.py` (the index pointer).
`--list` prints `spec | status | rest`, which serves as the index ROADMAP points at. Exit 0 = consistent, 1 = each
problem printed as `spec_ledger: <problem>`.

**Fences.** Every rule reads only prose lines. A fence opens on a line starting (after indentation) with 3+
backticks or 3+ tildes, and closes only on a line holding the same character, at least as many, and nothing else
(CommonMark). So a 4-backtick block that quotes a 3-backtick block, or a `~~~` block, hides its headings.
[REVISED: DTA-4 - the first prototype toggled on any 3-backtick line and ignored `~~~`; planted `## SF-1` in a
4-backtick block and `## SF-2` in a `~~~` block both gave false FAILs, both PASS now]

**A spec authorized for a pilot stage** (e.g. hx3, AUTHORIZED for HX3-0) moves to `**Status: PARTIAL <date> on
`main` <sha> (HX3-0 ...)` in the commit that lands its first stage; otherwise rule 4 fails as soon as CHANGELOG
names `HX3-0`. [REVISED: DTA-5]

**Gate placement.** A full-gate par stage, next to "G01 mutation anchors" (same class: a fast, offline consistency
check):
- `scripts/gate.sh` after :685 add `stage_spec_ledger() {` / `  python3 -B scripts/spec_ledger.py || fail "spec
  ledger (scripts/spec_ledger.py)"` / `}`, and after :714 add `par "spec ledger" stage_spec_ledger`.
- `governance/gate-lanes.json` `par` list: after `"harness skip rules/shapes"` (:28) add `"spec ledger"`, and
  fix the comma on :28. `scripts/gate_lane.py check` fails when a par stage has no row, so both edits land
  together.
- Shallow clones: rule 2 needs history. The gate runs in full worktrees [INFERENCE: no shallow-clone gate path
  found]. With the history missing, the stage fails loud rather than passing, which is the safe direction.

**Alternatives.** (a) A Rust test in axon-core: rejected, because it would tie a docs-ledger check to a cargo build
and the rules are text matching. (b) A pre-commit hook: rejected, because it is not on the gate path and is
skippable. (c) Ask writers to remember: that is what failed three times.

**Fail-first test.** `crates/axon-core/tests/spec_ledger.rs` follows the `pci_delta_note.rs` pattern
(`script_spawn::{repo_root, script, Bins}`, `Bins::NoWorkspaceBinary`, `python3`). Each test plants a minimal
tree in a tempdir (`tasks/`, `CHANGELOG.md`, `ROADMAP.md`, `tasks/opportunities.md`), sets `SPEC_LEDGER_GIT` to
`repo_root()`, and uses `49b576fa` as the ancestor sha and `deadbee` as a non-ancestor:
- `a_consistent_planted_tree_passes` (rc 0, `PASS`);
- `a_spec_without_a_status_line_is_refused`;
- `a_built_spec_citing_an_unstamped_entry_is_refused`;
- `an_entry_specced_in_a_built_spec_without_a_stamp_is_refused`;
- `a_draft_spec_whose_items_shipped_is_refused`;
- `a_built_status_naming_a_non_ancestor_commit_is_refused`;
- `a_roadmap_without_the_ledger_pointer_is_refused`;
- `a_heading_inside_a_fence_is_not_an_item`: a DRAFT spec quoting `## SF-1` inside a 4-backtick block that
  contains 3-backtick lines, and `## SF-2` inside a `~~~` block, with CHANGELOG naming both, passes; this is the
  false refusal the prototype hit on this spec and DTA-4's two planted cases;
- `a_duplicate_item_id_is_refused` (rule 0);
- `a_sha_outside_the_status_span_does_not_count` (rule 2: a BUILT line with no sha, a later line citing an
  ancestor sha, is refused);
- `a_wrapped_source_paragraph_is_read_whole` (rule 5: a BUILT spec whose Source line wraps and cites an unstamped
  `O-GAP-01` on its second line is refused; planted on the post tree it gives `zz-spec.md: status BUILT, but
  O-GAP-01 carries no closure stamp`). [REVISED: DTA-6]

The end-of-build state also checks: the post tree with this spec set to the PARTIAL line of DT-5, O-GAP-09/11
stamped and a CHANGELOG section naming DT-1..DT-7 gives `spec_ledger: PASS (0 problem(s), 17 specs)`.
The `return 0` mutant is killed by every negative test, because each one asserts rc 1. It gets no row of its own.
Each negative test asserts rc 1 and the specific `spec_ledger:` line, with the message `ATTACK: <...> was
accepted` (§4). On today's tree the gate stage fails (28 problems: the 27 of the first draft plus gate-lane's
missing ship sha). After DT-5 it passes. **False-refusal risk:**
a spec writer whose item ids collide with CHANGELOG text. For example, `G1` could match a word: the regex needs
`(?<![\w-])ID(?![\w-])` and ignores fenced headings, and the 17 specs today plus checker-soundness-2 give 0 false
hits (`checker-soundness-2-spec.md` added to the post tree: PASS, 18 specs). The other risk is a spec that cites an O-id it
did not build in its Source line. The fix for that is PARTIAL with the id named, and that is the intended
discipline.

### DT-7 (P1) Mechanism: every ```` ```axon ```` block in the LLM-facing docs checks - O-GAP-09

**Is there a test today?** No. `git grep -n "axon-for-llms\|language-tour\|stdlib\.md\|examples/asi/README" --
crates/*/tests scripts` finds no snippet extraction. `cli_run.rs:6303` `every_goal_example_compiles_and_runs`
covers `examples/goals` only, and `:18143`/`:18256` cover `examples/*.ax`.

**Location.** `crates/axon-core/tests/doc_snippets.rs`, its own test target. Gate stage S16 "axon-core tests" is
`split: per-target` and takes its units from `cargo metadata`, so the new target is a unit with no registry edit.
It uses `Command::new(env!("CARGO_BIN_EXE_axon"))`, as `cli_run.rs:13-14` does (the binary `harness_binaries.rs`
allows).

**Docs.** `spec/axon-for-llms.md`, `spec/language-tour.md`, `spec/stdlib.md`, `README.md`, `CLAUDE.md`,
`examples/asi/README.md`. The ASI README has no axon block today; listing it covers future ones.

**Marker syntax** (the fence info string; GitHub highlights on its first word, so rendering is unchanged):
- ```` ```axon ```` - a whole program. `axon check` must exit 0. If the source has `fn main(`, `axon run` must exit
  0. If it has `@[test` (so `@[test(should_fail)]` counts too), `axon test` must exit 0. [REVISED: DTA-7 - was the
  literal `@[test]`]
- ```` ```axon exits N ```` - a whole program whose `axon run` must exit exactly N. Use it for a `fn main() -> i64`
  example (run's exit code is main's return value) and for a demo of a runtime refusal (verify breach 3,
  refinement 6, panic 101). It combines with `fragment` (```` ```axon fragment exits 3 ````). [REVISED: DTA-1 -
  `README.md:189-209` checks clean and exits 45 by design]
- ```` ```axon fragment ```` - statements. The body is wrapped as `fn main() {\n<body>\n}`, then the same rules
  apply. A fragment whose last line is a bare value is E0307 when wrapped; write it as a `println`.
- ```` ```axon fails E1001 [E....] ```` - meant to be refused. `axon check` must exit non-zero, and every listed
  code must appear on an error-severity line. Otherwise the block is reported.
- Any other info word after `axon` is reported as an unknown marker. A signature listing that is not Axon goes in
  ```` ```text ````.
- A fence may be indented (list items). The opener's indent is stripped from the body.

**Execution.** Each block goes into its own tempdir as `snippet.ax`, with cwd = tempdir and `AXON_PATH` removed.
Env: `AXON_SEED=42`, `AXON_AI_MOCK=1`, `HOME=<tempdir>` and `XDG_CACHE_HOME=<tempdir>` (keeps provenance out of
$HOME, and E0901's `~/.axon/lib` search sees an empty dir, so a `mod` block cannot pass or fail by machine), stdin
= null (a `read_line` block sees EOF instead of hanging). Each child gets a 20 s timeout. [REVISED: DTA-7 - HOME
and stdin] The test collects every failure, then asserts once, listing `doc:line marker: rc, first error code
+ message`, so one run shows the whole list.

**Tests.**
- `every_axon_block_in_the_llm_facing_docs_checks`: the real docs. Message: `ATTACK: a doc snippet that does not
  check or run was accepted`.
- `the_marker_rules_hold_on_planted_blocks`: plants a markdown string with (i) an unmarked block with a top-level
  statement (must be reported), (ii) `fails E1001` on a block that checks clean (reported), (iii) `fails E9999`
  on a block that fails with E1001 (reported), (iv) a `fragment` that is fine (not reported), (v) a program that
  checks but panics at run (`assert(false)` in main, reported), (vi) an unknown marker (reported), and (vii)
  `exits 3` on a program that exits 0 (reported) [REVISED: DTA-1]. Messages
  `ATTACK: a planted <case> was accepted`.

**Runtime.** Measured with the same binary: 100 `axon check` calls on the extracted blocks took 0.23 s wall, and
19 replacement blocks (check + run/test) took 0.10 s. About 78 blocks after DT-1..DT-4 gives under 1 s; the budget
is 5 s per target.

**On today's tree** the test fails with the 32 rows in §3a below. After DT-1..DT-4 it passes. I checked this
row by row on the extracted and replaced files, not on the edited docs [INFERENCE until the build runs it].

### 3a. The full list of failing snippets (78 axon blocks in the six docs; 31 fail `axon check` at 4a6436da, 1 more exits non-zero at run)

| # | doc:lines | check, first error | wrapped as a fragment | disposition |
|---|---|---|---|---|
| 1 | axon-for-llms.md:38-83 | E0000 `Ident("x")` | E0000 `Fn, expected expression` | DT-1 A1 |
| 2 | axon-for-llms.md:208-211 | E0001 `a` | E0001 | DT-1 fragment |
| 3 | axon-for-llms.md:243-253 | E1504 | E0000 | DT-1 A2 |
| 4-7 | stdlib.md:29-33, 43-45, 55-57, 67-69 | E0000 `print`/`println`/`eprint`/`eprintln` | rc 0, run 0 | DT-2 fragment |
| 8 | stdlib.md:83-85 | E0000 `assert` | E0001 `result` | DT-2 sl_assert |
| 9 | stdlib.md:95-97 | E0000 `assert_eq` | E0001 `factorial` | DT-2 A5 |
| 10 | stdlib.md:112-118 | E0000 `Match` | E0001 `safe_div` | DT-2 A6 |
| 11 | stdlib.md:194-197 | E0000 `println` | E0001 `count` | DT-2 fragment |
| 12 | stdlib.md:210-215 | E0000 `Match` | rc 0, run 0 | DT-2 fragment |
| 13 | stdlib.md:379-382 | E0102/E0306 `abs_f64(5)` | E0306 | DT-2 §4 rewrite |
| 14-18 | stdlib.md:493-498, 512-517, 520-523, 532-544, 554-572 | E0000 (signature listings) | E0000 | DT-2 §6 deleted |
| 19-21 | stdlib.md:595-600, 604-608, 614-618 | E0000 (signature listings) | E0000 | DT-2 §7 deleted |
| 22 | stdlib.md:642-644 | E0001 `pow2` | E0001 | DT-2 A9 |
| 23 | language-tour.md:376-387 | E0001 `apply` | - | DT-3 |
| 24 | language-tour.md:400-405 | E0000 `print` | rc 0, run 0 | DT-3 fragment |
| 25 | language-tour.md:408-413 | E0000 `to_str` | E0307 | DT-3 A10 |
| 26 | language-tour.md:416-421 | E0000 `abs_i32` | E0307 | DT-3 A11 |
| 27 | language-tour.md:424-426 | E0000 `len` | E0307 | DT-3 A12 |
| 28 | language-tour.md:470-486 | E0102 (+W0001, E1504) | E0000 | DT-3 A3 |
| 29 | README.md:13-21 | E1001 (intended) | - | DT-4 `fails E1001` |
| 30 | README.md:232-238 | E0001 `add` | - | DT-4 A13 |
| 31 | CLAUDE.md:376-463 | E0000 `Ident("x")` | E0000 | DT-4 A4 |
| 32 | README.md:189-209 | check rc 0; `axon run` exits 45 (`fn main() -> i64` returns `sum`) | - | DT-4 `exits 45` [REVISED: DTA-1] |

Block counts: axon-for-llms 10, stdlib 30, language-tour 26, README 11, CLAUDE 1, examples/asi/README 0. Command:
the extraction in `snip/` plus `axon check <f>`. The wrapped column is `fn main() {\n<body>\n}`. The other 47
blocks check rc 0; 16 of them have `fn main`, and of those 15 run rc 0 and `README.md:189-209` exits 45 by design
(re-run 2026-10-29 with `AXON_SEED=42 AXON_AI_MOCK=1`, stdin null). [REVISED: DTA-1 - was "the 9 of them"]

## 4. g01 rows (M3930-M3939) and attack markers

Format as `scripts/v022_g01_mutations.py` (`(id, desc, file, old, new, pkg, args, test)`); markers in
`scripts/v022_attack_markers.py` as `'M39xx': 'ATTACK: ...'` regexes. Each `old` must occur exactly once in its
file (`scripts/g01_anchors.py`), and I checked the anchors against the post-edit text and the Appendix B
prototype. M3930 is the first g01 row on a `.md` file. The runner reads `ROOT/rel`, checks `count(old) == 1`,
replaces, runs `cargo test` and restores, with no extension filter (`v022_g01_mutations.py:8885-8905`;
`v022_mut_select.judge` handles non-`.rs` paths at :381) [REVISED: DTD-6 checked it; was INFERENCE]. Because the
docs are read at test run time, the edit is seen.

[REVISED: DTD-6] Only one row anchors on prose (M3930); the first draft had three. "G01 mutation anchors" is a
full-gate par stage, so any later edit of an anchored doc line fails the gate. Rule: a doc edit that touches
M3930's anchor (`@[adaptive]\nfn tune(threshold: i64)` in `spec/axon-for-llms.md`) updates the row's `old`/`new`
in the same commit; the integrator owns the registry. The freed ids M3931/M3932 cover DT-6 rules 0 and 2, which
the first draft had no row for. The README `fails` marker and the stdlib `fragment` marker are pinned by
`the_marker_rules_hold_on_planted_blocks` instead.

| id | file | old -> new | test (`axon-core`, `--no-default-features --test ...`) | marker |
|---|---|---|---|---|
| M3930 | spec/axon-for-llms.md | `@[adaptive]\nfn tune(threshold: i64)` -> `@[goal("maximize score")]\n@[adaptive]\nfn tune(threshold: i64)` | doc_snippets / every_axon_block_in_the_llm_facing_docs_checks | `ATTACK: a doc snippet that does not check or run was accepted` |
| M3931 | scripts/spec_ledger.py | `problems.append(f"{rel}: item {i} also defined in {owner[i]}")` -> `None` | spec_ledger / a_duplicate_item_id_is_refused | `ATTACK: a duplicate item id was accepted` |
| M3932 | scripts/spec_ledger.py | `m.group(2).split("**")[0]` -> `m.group(2)` | spec_ledger / a_sha_outside_the_status_span_does_not_count | `ATTACK: a sha outside the status span was accepted` |
| M3933 | scripts/spec_ledger.py | ``problems.append(f"{rel}: no `**Status`` -> ``(f"{rel}: no `**Status`` | spec_ledger / a_spec_without_a_status_line_is_refused | `ATTACK: a spec without a status line was accepted` |
| M3934 | scripts/spec_ledger.py | `elif not CLOSED_RE.search(opp[oid])` -> `elif False and not CLOSED_RE.search(opp[oid])` | spec_ledger / a_built_spec_citing_an_unstamped_entry_is_refused | `ATTACK: a built spec citing an unstamped entry was accepted` |
| M3935 | scripts/spec_ledger.py | `if st in ("BUILT", "PARTIAL") and not CLOSED_RE.search(body)` -> `if False and st in ("BUILT", "PARTIAL") and not CLOSED_RE.search(body)` | spec_ledger / an_entry_specced_in_a_built_spec_without_a_stamp_is_refused | `ATTACK: an entry specced in a built spec without a stamp was accepted` |
| M3936 | scripts/spec_ledger.py | `if status in ("DRAFT", "REVIEWED", "AUTHORIZED") and shipped:` -> `if False:` | spec_ledger / a_draft_spec_whose_items_shipped_is_refused | `ATTACK: a draft spec whose items shipped was accepted` |
| M3937 | scripts/spec_ledger.py | `elif not any(is_ancestor(root, s) for s in shas):` -> `elif False:` | spec_ledger / a_built_status_naming_a_non_ancestor_commit_is_refused | `ATTACK: a built status naming a non-ancestor commit was accepted` |
| M3938 | scripts/spec_ledger.py | `m = FENCE_RE.match(line)` -> `m = None` | spec_ledger / a_heading_inside_a_fence_is_not_an_item | `ATTACK: a fenced heading was read as a shipped item` |
| M3939 | scripts/spec_ledger.py | `if "scripts/spec_ledger.py" not in roadmap:` -> `if False:` | spec_ledger / a_roadmap_without_the_ledger_pointer_is_refused | `ATTACK: a roadmap without the ledger pointer was accepted` |

Each of M3931, M3932 and M3938 was applied to the prototype and run on a planted tree (`rev/`): the unmutated
script gives the expected result, the mutant does not (M3932: the mutant reports "CHANGELOG names none of its
items" instead of "names no commit", so the test must assert the specific line, as all negative tests do).
The harness's own marker logic (fails / fragment / exits / unknown) is pinned by
`the_marker_rules_hold_on_planted_blocks`. No row mutates a test file.

**pci.** No new refusal site in `crates/`, so `v022_refusal_coverage.py` sees none. But the `builtins.rs` doc-string
commit is under `crates/axon-core/src`, so `scripts/pci_delta.py --check` (S16, `pci_delta_note.rs`) needs a theme
file and a note regen: DT-2 step 11a. No g01 row here touches `crates/axon-core/src`, so the pci row count is
unchanged. [REVISED: DTD-1]

## 5. Gates and cost budget

False-refusal sweep: no item changes what `axon check` or `axon run` accepts. The only `crates/` edits are two
`doc:` strings (read only by `axon reference`, `main.rs:5254/:5376`) and one test assertion. So
`/var/tmp/r51-tmp/cs-sweep.sh <repo> <axon-bin> <out>` must be byte-identical to
`/var/tmp/r51-tmp/sf-sweep-base.txt` (290 files, rc0=274). `examples/asi/ad_optimizer.ax`'s outcome stays rc 0
(`W2001`); only a comment and a println string change.

[REVISED: DTD-5 - table replaced; DTD-2 - `reference_gate.sh` needs `AXON=<bin>`, or it refuses with rc 2]

| stage | what | time | paid |
|---|---|---|---|
| G0 | Step 1 review of this spec (2 reviewers, read-only) | ~30 min | $0 |
| G1 | lane A edits + `axon reference > AXON_REFERENCE.md` + `AXON=<bin> bash scripts/reference_gate.sh` + pci theme file + `python3 tasks/r51-pci-regen.py` + `python3 scripts/pci_delta.py --check` | ~60 min edit, <10 s check | $0 |
| G2 | lane B edits + `python3 -B scripts/spec_ledger.py` (0.13 s) + `python3 scripts/test_gate_lane.py && python3 scripts/gate_lane.py check` | ~30 min, <5 s | $0 |
| G3 | `cargo test -p axon-core --no-default-features --test doc_snippets --test spec_ledger --test pci_delta_note` | build 3-6 min warm [INFERENCE], run <30 s | $0 |
| G4 | `cargo test -p axon-core --no-default-features --test cli_run -- ad_optimizer_flagship_demo_runs_the_full_stack axon_reference_is_in_sync_with_the_binary asi_hello_goal_acid_test_loop_runs_end_to_end asi_demo_replay_and_audit_commands_work_end_to_end asi_demo_set_runs_without_crashing` | ~2-5 min [INFERENCE] | $0 |
| G5 | after commit, on a clean tree (the runner refuses an uncommitted one, `v022_g01_mutations.py:8830-8832`): `python3 scripts/g01_anchors.py`; `python3 scripts/v022_g01_mutations.py --only=M3930,M3931,M3932,M3933,M3934,M3935,M3936,M3937,M3938,M3939 <out.json>`. All 10 rows mutate `.md` or `.py`, so no cell recompiles `axon` | <1 s; ~5-10 min incl. one clean build [INFERENCE] | $0 |
| G6 | cs-sweep byte-identical to `sf-sweep-base.txt`; `scripts/gate.sh --strict` rc 0, skips identical to base | ~3 s sweep + the current full-gate time [INFERENCE: no timing log found] | $0 |

Total paid spend: $0. Wall time about 3-4 h including the full gate [INFERENCE].

## 6. Sequencing and lanes

| lane | items | files | notes |
|---|---|---|---|
| A (docs) | DT-1, DT-2, DT-3, DT-4 | spec/axon-for-llms.md, spec/stdlib.md, spec/language-tour.md, README.md, CLAUDE.md, AGENTS.md, examples/asi/{README.md, ad_optimizer.ax, run.sh}, crates/axon-core/src/builtins.rs (:469-470, :494; its own commit), governance/notes/pci-themes/<hash8>.json + the regenerated pci note (integrator), AXON_REFERENCE.md, cli_run.rs:5601; governance/{DEFINITION_OF_DONE,CODE_REVIEW_RUBRIC,BUILD_PROTOCOL}.md only if NH-1 = (a) | `builtins.rs:247-251` (`assert_eq_f64` doc) belongs to CS2-2: different lines, same file; whoever lands second regenerates AXON_REFERENCE.md and re-runs the pci regen |
| B (ledger) | DT-5, DT-6 | tasks/*-spec.md (status lines only, incl. gate-lane's sha), tasks/opportunities.md (stamps + the new O-DT-01 entry), CHANGELOG.md, ROADMAP.md, scripts/spec_ledger.py, scripts/gate.sh, governance/gate-lanes.json, crates/axon-core/tests/spec_ledger.rs | `tasks/opportunities.md` and CHANGELOG are shared with every concurrent spec. Re-run `spec_ledger.py` at integration, since a sibling spec landing BUILT without stamps now fails the gate (by design) |
| C (snippet test) | DT-7 | crates/axon-core/tests/doc_snippets.rs | after lane A (it fails until A lands); g01 rows for B and C go into scripts/v022_g01_mutations.py and v022_attack_markers.py through one owner (the integrator) |

Order: A and B in parallel, then C, then the g01 rows, then G6. Other O-GAP specs drafted in parallel may edit
`spec/axon-for-llms.md` gotchas. DT-7 then holds their blocks to the same rule, and that is intended.

**Against checker-soundness-2 (CS2)** [REVISED: DTD-3, DTD-4]. Shared files: `builtins.rs` (CS2-2 edits the
`assert_eq_f64` doc at :247-251; DT-2 edits :469-470 and :494), `AXON_REFERENCE.md` (CS2 adds an E0607 row and
widens E0304), `CHANGELOG.md`, `tasks/opportunities.md` (adjacent O-GAP entries, so a textual conflict is likely),
and the pci note. g01 ids do not clash (CS2 M3890-M3929, DT M3930-M3939).
- **Merge docs-truth first** (about 2 h of lane work, against CS2's 3-5 h g01 run). CS2's own S16 run then
  includes `doc_snippets`, so a doc block that a new CS2 refusal (E0607, non-enum exhaustiveness, typed HOF
  closures, typed Dict) would break fails in CS2's gate, and CS2's lane fixes the doc. Exposure today
  [INFERENCE: CS2 not built]: every `match` in the doc blocks is on an enum, Option or Result or has `_`; no
  closure writes a captured name; the only HOF lambda is `arr_sort_by(xs, |a, b| str_cmp(a, b))`.
- **The second lander** regenerates `AXON_REFERENCE.md` (`axon reference > AXON_REFERENCE.md`), re-runs the pci
  regen, and carries the other's ledger. Once DT-6 is on main, CS2's integration must set CS2's Status to
  `**Status: BUILT <date> on `main` <sha>` (an ancestor of HEAD, so a follow-up commit after its merge), stamp
  O-GAP-01..07 (or name the open ones in a PARTIAL line), and pass `python3 -B scripts/spec_ledger.py`. If CS2
  lands first, DT-5 adds those stamps. I sent this line to `CS2Draft` for its §6.

## 7. Needs-human

Engineering choices are decided above: curated stdlib, marker syntax, gate placement, friction-list tagging, stamp
wording. Two product choices remain:

- **NH-1 - What the Definition of Done asks for a new builtin.** `governance/DEFINITION_OF_DONE.md:52`,
  `CODE_REVIEW_RUBRIC.md:57` and `BUILD_PROTOCOL.md:121` require documenting public surface "in
  `spec/stdlib.md`". Once stdlib.md is curated, that rule recreates the hand-kept list. Options: (a) "builtin:
  its `doc` string in `builtins.rs` (AXON_REFERENCE.md regenerated, gate-checked); syntax/error code: the
  relevant `spec/*.md`; add to `spec/stdlib.md` or `spec/axon-for-llms.md` only when a model would misuse it
  without guidance"; (b) keep the rule as is. **Recommendation: (a).** It is the only option that does not
  re-grow the stale list, and the reference gate already enforces the doc string.
- **NH-2 - ROADMAP.md is four months stale (2026-06-28).** DT-5 adds only an index pointer. Whether to rewrite
  ROADMAP's priorities is a product call. **Recommendation:** defer to a separate roadmap pass, and keep the
  pointer so the shipped/unshipped state has one generated index.

## Appendix A - verified replacement blocks

Each block below was saved under `/var/tmp/r51-tmp/DocsDraft/verify/`. `axon check` gave rc 0 (warnings only:
W0006 unused, W2001 vague goal), and `axon run` (programs with `fn main`) or `axon test` (with `@[test]`) gave rc 0
with `AXON_SEED=42`. A fragment block is the body only; the harness wraps it.

**A1** (`axon-for-llms.md` §2, ```` ```axon ````; `llms_syntax.ax`)
```
// Items (fn, type) live at the top level; statements live inside a fn body.

// Functions — last expression is the return value (no `return` needed)
fn add(a: i64, b: i64) -> i64 { a + b }
fn inc(x: i64) -> i64 { x + 1 }

// Structs — declared with `type`, structural (no `implements`)
type Point = { x: f64, y: f64 }

// Enums (ADTs) — variants reference with `::`
type Shape = Circle { r: f64 } | Square { side: f64 }

fn main() {
    // Variables — mutable by default, no `let` on reassignment
    let x = 42            // i64 (default integer)
    let y = 3.14          // f64
    let s = "hello"       // str (UTF-8, immutable)
    let b = true          // bool
    x = x + 1             // reassign — no `let`

    // if / while / match are EXPRESSIONS
    let label = if x > 0 { "pos" } else { "non-pos" }
    while x < 10 { x = x + 1 }
    let n = 3
    for i in 0..n { x = x + i }   // i is i64, 0 ≤ i < n

    let p = Point { x: 1.0, y: 2.0 }
    println(to_str_f64(p.x))
    let sh = Shape::Circle { r: 5.0 }

    // Pattern matching — EXHAUSTIVE, `_` is the catch-all
    let area = match sh {
        Shape::Circle { r }   => 3.14159 * r * r
        Shape::Square { side } => side * side
    }

    // String interpolation — `{expr}` evaluated at runtime; `{{`/`}}` for literal braces
    println("hello {s}, x is {to_str(x)}")

    // Lambdas, and named functions as values (type `fn(i64) -> i64`)
    let double = |n| n * 2
    let ops = [inc, double]          // a dispatch table; call with ops[k](x)
    let f = inc                      // apply(inc, 41), arr_map(xs, inc) all work
    // A generic fn or a builtin is NOT a value (E0306): wrap it, `|n| abs_i64(n)`.
    // A call through a value is checked like a direct call: f(1, 2) here is E0305.
}

// Operators:  + - * / %   == != < > <= >=   && ||
//   `/` and `%` on i64 are integer division/modulo.
//   `+` concatenates str ("a" + to_str(n)). `==` works on str.
```

**A2** (`axon-for-llms.md` §7, ```` ```axon ````; `llms_goal.ax`)
```
// Goal-directed optimization. `@[adaptive]` marks the METRIC the optimizer tunes (it
// takes the params); `@[goal]` marks a ZERO-argument entry point that names it. Calling
// the goal fn runs the search first, then its body, where `goal_met` is 1 if the best
// score reached `target` (else 0). Putting both on one fn is E1504.
@[adaptive]
fn tune(threshold: i64) -> i64 { 100 - (threshold - 40) * (threshold - 40) }

@[goal(metric: tune, target: 100, max_evals: 200)]
fn optimize() -> i64 { goal_met }

// Runtime-checked postcondition on the scalar return value. A `@[verify]`
// predicate may only read the clock: any other effect is refused (E1104).
@[verify(value >= 0)]
fn clamp_nonneg(x: i64) -> i64 { max_i64(x, 0) }

fn main() {
    let met = optimize()
    let best = goal_best_input("tune", 100.0)
    println("goal met: {to_str(met)}, best threshold: {to_str(best)}")
    println(to_str(clamp_nonneg(-3)))
}
```

**A3** (`language-tour.md` §16, ```` ```axon ````; `tour16.ax`)
```
// The optimizer tunes an @[adaptive] fn: its params are the search space,
// its return value is the score.
@[adaptive]
fn throughput(workers: i64) -> i64 { 1000 - (workers - 12) * (workers - 12) }

// An @[goal] fn takes no params and names the metric; calling it runs the
// search first. `goal_met` is 1 if the best score reached `target`.
@[goal(metric: throughput, target: 1000, max_evals: 100)]
fn tune_workers() -> i64 { goal_met }

// A runtime-checked postcondition: a breach exits 3.
@[verify(value >= 0.0)]
fn critical_calculation(x: f64) -> f64 { x * x }

fn main() {
    println(to_str(tune_workers()))                       // 1
    println(to_str(goal_best_input("throughput", 1000.0))) // 12
    println(to_str_f64(critical_calculation(3.0)))        // 9
}
```

**A4** (`CLAUDE.md` quick reference, ```` ```axon ````; `claude_qr.ax`)
```
// Items (fn, type, @[test]) live at the top level; statements live in a fn body.
// A top-level line such as `x = x + 1` or `println(..)` is E0000.

// Functions
fn add(a: i64, b: i64) -> i64 { a + b }

// Structs and enums
type Point = { x: f64, y: f64 }
type Shape = Circle { r: f64 } | Square { side: f64 }

// Error handling
fn parse(s: str) -> Result<i64, str> {
    let n = parse_int(s)?
    Ok(n * 2)
}

// Modules — TWO declarations, and both are required:
//   mod scorelib                       // DECLARES the module (registers the symbol)
//   use scorelib.{weighted, approve}   // IMPORTS names from it; `.` separates path segments
// `use X` without a preceding `mod X` is E0003 "module `X` not found". The
// resolver looks up the use-path's first segment in the symbol table, and only
// `mod` puts it there — so the two lines are not alternatives.
//
// Resolution is AMBIENT: `axon_search_dirs` is AXON_PATH, then ~/.axon/lib,
// then <bindir>/../lib/axon. THE ENTRY FILE'S OWN DIRECTORY IS NEVER SEARCHED,
// so a module sitting beside its importer does not resolve without AXON_PATH.
// First match wins and nothing records which file was chosen.

// Goal-directed optimization: `@[adaptive]` marks the METRIC (it takes the
// params); `@[goal]` marks a ZERO-argument entry point that names it. Both on
// one fn is E1504. `goal_met` is 1 if the best score reached `target`.
@[adaptive]
fn tune(t: i64) -> i64 { 100 - (t - 40) * (t - 40) }
@[goal(metric: tune, target: 100, max_evals: 200)]
fn optimize() -> i64 { goal_met }

// Runtime-checked postcondition; a breach exits 3. The predicate may only read the clock (E1104).
@[verify(value >= 0)]
fn clamp_nonneg(x: i64) -> i64 { max_i64(x, 0) }

// Capability sandboxing — @[contained] (enforced by `axon check`, E1001/E1004)
//   fs:    [read("./data/"), write("./out/")]   allowlist of path prefixes
//   net:   ["api.example.com", "*.trusted.io"]  allowlist of hosts (leading * glob)
//   exec:  none | any                           process spawning
//   never: [read("/etc/"), write("/"), net("*"), exec]   hard deny (overrides allowlist)
//   env:   reading env_var(...) is DENIED inside @[contained] (E1001) — env is an
//          ungrantable ambient secret channel; read it OUTSIDE the boundary, pass the value in
@[contained(fs: [write("./out/")], net: ["api.example.com"], exec: none)]
fn scorer() -> i64 { /* compiler refuses any I/O outside the declared caps */ 0 }

// Testing
@[test]
fn test_add() { assert_eq(add(2, 3), 5) }

@[test(should_fail)]
fn test_panic() { assert(false) }

fn main() {
    // Variables
    let x = 42            // i64 — also 0xFF (hex), 0b1010 (binary), 0o755 (octal),
                          //   with `_` separators. A LEADING ZERO stays decimal:
                          //   `017` is 17, not 15 — Axon has no C-style implicit octal.
    let y = 3.14          // f64
    let s = "hello"       // str
    let b = true          // bool
    x = x + 1             // reassignment (no let)

    let p = Point { x: 1.0, y: 2.0 }
    println(to_str_f64(p.x))
    let sh = Shape::Circle { r: 5.0 }

    // Control flow — if / while / match are expressions
    let label = if x > 0 { "pos" } else { "non-pos" }
    let i = 0
    while i < 10 { i = i + 1 }
    let val = parse("21")
    let n = match val { Ok(n) => n  Err(e) => 0 }

    // String interpolation
    let name = "ada"
    let age = 36
    println("hello {name}, age {to_str(age)}")

    println(to_str(optimize()))         // 1: the search reached the target
    println(to_str(clamp_nonneg(-3)))   // 0
}

// Operators: + - * / %   == != < > <= >=   && ||

// Builtins: println print eprint eprintln
//           to_str (polymorphic over scalars: i64/f64/bool) parse_int len
//           to_str_f64 to_str_bool (explicit forms; to_str now covers them)
//           abs_i32 abs_f64 min_i32 max_i32
//           wrapping_add wrapping_sub wrapping_mul wrapping_div wrapping_rem
//             — modular arithmetic, the deliberate opt-out from the checked
//               operators. Axon's semantics are STATED, not inherited:
//               wrapping_div(i64::MIN, -1) = i64::MIN (the wrap of an
//               unrepresentable 2^63), wrapping_rem(i64::MIN, -1) = 0 (that
//               value IS representable; x86 `idiv` traps on the pair, which is
//               a fact about the instruction and not the answer), and ANY
//               divisor of 0 still PANICS in both engines — zero has no
//               wrapped quotient, so returning one would be inventing a result.
//           assert assert_eq assert_err
//           The full list: AXON_REFERENCE.md (`axon reference`).
```

**A5** (`stdlib.md` assert_eq, ```` ```axon ````; `sl_asserteq.ax`)
```
fn factorial(n: i64) -> i64 { if n <= 1 { 1 } else { n * factorial(n - 1) } }

@[test]
fn factorial_of_five() {
    assert_eq(factorial(5), 120)
}
```

**A6** (`stdlib.md` assert_err, ```` ```axon ````; `sl_asserterr.ax`)
```
fn safe_div(a: i64, b: i64) -> Result<i64, str> {
    if b == 0 { Err("division by zero") } else { Ok(a / b) }
}

@[test]
fn dividing_by_zero_is_an_error() {
    match safe_div(10, 0) {
        Ok(_)  => assert_err(true)    // forces failure
        Err(_) => assert_err(false)   // passes
    }
}
```

**A7** (`stdlib.md` new §3, ```` ```axon fragment ````; `sl_strcmp.ax`)
```
let xs = arr_sort_by(["b", "a", "c"], |a, b| str_cmp(a, b))
println(xs[0])                         // a
println(str_slice("hello", 1, 99))     // ello — an out-of-range bound clamps
println(str_char_slice("héllo", 0, 2)) // hé — character indices
```

**A8** (`stdlib.md` §4, ```` ```axon fragment ````; `sl_coerce.ax`)
```
let n = 7
let r = sqrt(n as f64)          // `sqrt(n)` is E0306: no implicit i64 -> f64
println(to_str_f64(r))
```

**A9** (`stdlib.md` "Not shipped", ```` ```axon fragment ````; `sl_comptime.ax`)
```
let AREA = comptime { pow_i64(2, 8) }   // 256 — folded at compile time
println(to_str(AREA))
```

**A10** (`language-tour.md` Conversion, ```` ```axon ````; `tour_conv3.ax`)
```
fn main() {
    println(to_str(42))            // 42
    println(to_str_f64(3.14))      // 3.14
    match parse_int("42") {
        Ok(n) => println(to_str(n))   // 42
        Err(e) => println(e)
    }
    match parse_int("bad") {
        Ok(n) => println(to_str(n))
        Err(e) => println(e)          // could not parse `bad` as a base-10 integer
    }
}
```

**A11** (`language-tour.md` Math, ```` ```axon fragment ````; `tour_math.ax` body)
```
println(to_str(abs_i32(-5)))          // 5
println(to_str_f64(abs_f64(-3.14)))   // 3.14
println(to_str(min_i32(3, 7)))        // 3
println(to_str(max_i32(3, 7)))        // 7
```

**A12** (`language-tour.md` String, ```` ```axon fragment ````; `tour_str.ax` body)
```
println(to_str(len("hello")))   // 5
println(to_str(len("héllo")))   // 6 — bytes, not characters
```

**A13** (`README.md` Testing, ```` ```axon ````; `readme_test.ax`)
```
fn add(a: i64, b: i64) -> i64 { a + b }

@[test]
fn test_add() {
    assert_eq(add(2, 3), 5)
    assert_eq(add(-1, 1), 0)
}
```

## Appendix B - `scripts/spec_ledger.py`

The prototype at `/var/tmp/r51-tmp/DocsDraft/spec_ledger_proto.py`, verbatim below, becomes the script. The build
changes only the docstring (to state the eight rules (0-7) of DT-6 and the exit codes) and sets the executable bit.
Measured on 4a6436da: FAIL, 28 problems, 17 specs, 0.13 s. On the post tree: PASS, 0 problems, 0.13 s.
`prose_lines` drops fenced blocks. Without it, this spec's own Appendix C (a quoted `## S1 follow-ups` heading)
read as an item `S1` that CHANGELOG names, and rule 4 fired on a DRAFT. I found that on the first post-tree run,
and it is the reason for the fence rule.

```python
#!/usr/bin/env python3
"""Prototype of scripts/spec_ledger.py (docs-truth DT-6). Scratch only."""
import glob, os, re, subprocess, sys

STATUS_RE = re.compile(r"^\*\*Status: (DRAFT|REVIEWED|AUTHORIZED|BUILT|PARTIAL|STOPPED|SUPERSEDED)\b(.*)")
CLOSED_RE = re.compile(r"\[(RESOLVED|FIXED|DONE|CLOSED|BUILT|WITHDRAWN|SUPERSEDED)\b")
ID_RE = re.compile(r"\bO-[A-Z][A-Z0-9]*-\d+\b")
ENTRY_RE = re.compile(r"^#{2,3} (O-[A-Z][A-Z0-9]*-\d+)\b")
SPECCED_RE = re.compile(r"(?i)specced\b[^\n]*?tasks/([\w.-]+-spec\.md)")
SHA_RE = re.compile(r"\b[0-9a-f]{7,12}\b")

ITEM_RE = re.compile(r"^#{2,3} ([A-Z][A-Z0-9]*-?\d+[a-z]?)\b")

def entries(path):
    out, cur = {}, None
    for line in open(path, encoding="utf-8"):
        m = ENTRY_RE.match(line)
        if m:
            cur = m.group(1)
            out[cur] = []
            continue
        if line.startswith("#"):
            cur = None
        elif cur:
            out[cur].append(line)
    return {k: "".join(v) for k, v in out.items()}

def is_ancestor(root, sha):
    git = os.environ.get("SPEC_LEDGER_GIT", root)
    r = subprocess.run(["git", "-C", git, "merge-base", "--is-ancestor", sha, "HEAD"],
                       capture_output=True)
    return r.returncode == 0


FENCE_RE = re.compile(r"^\s*(`{3,}|~{3,})")

def prose_lines(text):
    # Headings and Source lines inside a fence (quoted CHANGELOG text, code) are not the spec's own.
    # CommonMark: a fence closes only on the same character, at least as long, with nothing after it.
    out, fence = [], None
    for line in text.split("\n"):
        m = FENCE_RE.match(line)
        if fence is None:
            if m:
                fence = m.group(1)
                continue
            out.append(line)
        elif m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence) \
                and line.strip() == m.group(1):
            fence = None
    return out

def main(root, listing=False):
    opp = entries(os.path.join(root, "tasks/opportunities.md"))
    problems, table, status_of = [], [], {}
    changelog = open(os.path.join(root, "CHANGELOG.md"), encoding="utf-8").read()
    specs = sorted(glob.glob(os.path.join(root, "tasks/*-spec.md")))
    owner = {}
    for spec in specs:
        rel = os.path.relpath(spec, root)
        lines = prose_lines(open(spec, encoding="utf-8").read())
        m = next((m for l in lines[:15] if (m := STATUS_RE.match(l))), None)
        if m is None:
            problems.append(f"{rel}: no `**Status: <WORD>` line in its first 15 lines")
            continue
        # Only the bold span is the status claim; later prose may cite unrelated commits.
        status, rest = m.group(1), m.group(2).split("**")[0]
        status_of[os.path.basename(rel)] = (status, rest)
        table.append((rel, status, rest.strip()[:60]))
        items = {mm.group(1) for l in lines if (mm := ITEM_RE.match(l))}
        for i in sorted(items):
            if i in owner:
                problems.append(f"{rel}: item {i} also defined in {owner[i]}")
            else:
                owner[i] = rel
        shipped = sorted(i for i in items if re.search(r"(?<![\w-])" + re.escape(i) + r"(?![\w-])", changelog))
        if status in ("DRAFT", "REVIEWED", "AUTHORIZED") and shipped:
            problems.append(f"{rel}: status {status}, but CHANGELOG.md names its items {shipped}")
        if status in ("BUILT", "PARTIAL") and not shipped and os.path.basename(rel) not in changelog:
            problems.append(f"{rel}: status {status}, but CHANGELOG.md names none of its items")
        if status not in ("BUILT", "PARTIAL"):
            continue
        shas = SHA_RE.findall(rest)
        if not shas:
            problems.append(f"{rel}: status {status} names no commit")
        elif not any(is_ancestor(root, s) for s in shas):
            problems.append(f"{rel}: status {status} names {shas}, none an ancestor of HEAD")
        # The Source paragraph runs from its `Source` line to the next blank line (it may wrap).
        cited, in_source = set(), False
        for l in lines[:40]:
            if l.startswith("Source") or l.startswith("**Source"):
                in_source = True
            elif not l.strip():
                in_source = False
            if in_source:
                cited |= set(ID_RE.findall(l))
        for l in lines:
            if re.match(r"^#{2,4} ", l):
                cited |= set(ID_RE.findall(l))
        for oid in sorted(cited):
            if oid not in opp:
                problems.append(f"{rel}: cites {oid}, which has no entry in tasks/opportunities.md")
            elif not CLOSED_RE.search(opp[oid]) and not (status == "PARTIAL" and oid in rest):
                problems.append(f"{rel}: status {status}, but {oid} carries no closure stamp")
    for oid, body in sorted(opp.items()):
        for name in set(SPECCED_RE.findall(body)):
            st, rest = status_of.get(name, (None, ""))
            if st in ("BUILT", "PARTIAL") and not CLOSED_RE.search(body) \
                    and not (st == "PARTIAL" and oid in rest):
                problems.append(f"{oid}: specced in {name} ({st}) but carries no closure stamp")
    roadmap = open(os.path.join(root, "ROADMAP.md"), encoding="utf-8").read()
    if "scripts/spec_ledger.py" not in roadmap:
        problems.append("ROADMAP.md: does not point at `scripts/spec_ledger.py --list`")
    if listing:
        for r in table:
            print(" | ".join(r))
    for p in problems:
        print("spec_ledger:", p)
    print(f"spec_ledger: {'FAIL' if problems else 'PASS'} ({len(problems)} problem(s), {len(specs)} specs)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else ".", "--list" in sys.argv))
```

Note for the build: this spec's own Source paragraph cites O-GAP-08/09/11 and the DT-5 ids. While it is DRAFT,
rule 5 does not apply to it. When it becomes BUILT, the stamps DT-5 adds plus the end-of-build O-GAP stamps satisfy
rule 5, and O-GAP-08 gets `[PARTLY ...` (not a closure word), so this spec's Status must say `PARTIAL ... O-GAP-08
stays open` (the PARTIAL exemption).

## Appendix C - CHANGELOG text

After :26:
```
- **Conformance workspaces are removed when dropped (SF-3, O-FU-07):** d38abb5b.
```
Then a new section:
```
## Checker soundness and capability surface (checker-soundness CS-1 to CS-13, capability-surface CAP-1 to CAP-4; `main` 49b576fa, 2026-10-27)
- **Refinement predicates are name- and type-checked (CS-1, O-S1-22):** an unbound name or callee in a predicate is E1211, not a run-time panic; a struct-field refinement binds its own field name.
- **`@[verify]` predicates are name- and type-checked and bind the fn's params (CS-2, CS-3, O-S1-23):** an unknown name or an ill-typed operand is E1103; a predicate the run cannot evaluate is refused at check instead of being silently skipped.
- **A call to a builtin-shadowing fn is typed against the builtin it runs (CS-4, O-FU-06):** wrong arity is E0305, wrong argument type E0306.
- **Checker false positives removed (CS-5, CS-6, CS-11, O-S1-24):** `len(x)` with `x` still a type variable is no longer forced to `str`; a move inside an early-`return` branch no longer poisons the fallthrough; `return` inside a match arm or if-branch is no longer typed `()`.
- **Array and `&[T]` params copy on `let`; `str` stays a move type (CS-7, NH-3 d).**
- **A field access followed by a newline and `(` no longer parses as a method call (CS-10).**
- **A user type named like a builtin type is E0005 (CS-12).**
- **The goal generator takes only the score bound as its target (CS-13).**
- **Docs and invariants (CS-8, CS-9):** two gotchas documented; a test pins that a check-accepted fixture never reaches an interpreter-internal panic.
- **A `@[verify]` predicate may only read the clock (CAP-1, O-S1-19, O-SP-01):** any other effect is E1104, and its effects count against the fn's declared row (E1310).
- **Native and turbofish calls reach `effect_union` (CAP-2, O-S1-20);** both capability walkers classify every call through one `Reach::note` and a test pins their agreement (CAP-3, O-FU-05).
- **E1310 sees `ai_extract::<T>` (CAP-4, O-SP-04).**
```
After :39, before `## S1:`:
```
## S1 follow-ups (followup FU-2 to FU-9; `fu/int`, on `main` by 791dee95, 2026-10-18)
- **Capability walkers see predicate routes (FU-2, O-S1-14):** ecf63dfc.
- **A stopped check drain is never complete output (FU-3, O-S1-12):** ac5876df.
- **Restore only bytes this run wrote (FU-4, O-S1-13):** 098786e7.
- **Non-UTF-8 `AXON_ALLOWED_EFFECTS` denies every effect (FU-5, O-S1-09):** 7c5d2136.
- **Relative `cortex --workspace` (FU-6, O-S1-04):** 392acddc.
- **Lexer-aware `locate::fn_body` (FU-7, O-S1-11):** 2338ceca.
- **`run_ab.py` integrity-abort fixture (FU-8, O-S1-10):** 30fb8841.
- **Build-loop skill update (FU-9, O-S1-16).**
- FU-1, FU-10 and FU-11 moved to `tasks/hx2-spec.md` (STOPPED at HX-0); O-S1-02 stays open.
```
The CS/CAP bullet wording paraphrases the two specs' items (`checker-soundness-spec.md` §3,
`capability-surface-spec.md` §3) and was not re-derived from the diff; a Step 1 reviewer should read it against
those item texts.
