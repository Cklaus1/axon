# authoring-gap - spec (AG-1..AG-4)

**Status: PARTIAL 2026-11-24 on `ag/spec` (AG-1 c2788200, AG-2 277db656 + 99da94d9, PCI note 5d5bd59d; AG-3 done
offline; g01 M5280-M5299 20/20 killed; AG-4 open, not built; the CF-12 filter waits for the freeze).** Build authorized by
the human 2026-11-24. Items 1-3 were authorized as "fix, and benchmark". Language changes are NOT authorized (§7).

**Step 1 review:**
- `AuditAG` (conformance): 17 findings, CF-1..CF-17 (0 blockers, 10 major, 7 minor).
- `DevloopAG` (dev-loop/cost): 7 findings, DL-1..DL-7 (4 major, 3 minor).
- All 24 were accepted. DL-7 is accepted as an optional step. Every disposition appears inline as `[REVISED: CF-#/DL-# ...]`.
- AG-3 results are folded in (§2 AG-3); they add 3 card changes to AG-1.

**Source:** pilot attempt 2 of the authoring eval (`PREREG-ae.md` "Pilot attempt 2"; run `20261111-183515-pilot-k3-a2`,
private). First-try FT-pass for CERE Qwen was T1 0.378 / T2 0.153 / T3 0.055 / TS 0.203. The Python arm scored 0.90 / 0.86 /
0.69 / 0.92 on the same tasks.

Where the 1039 scored round-0 Axon attempts end:

| ending | n |
|---|---|
| refused by `axon check` | 550 |
| check-clean, passed | 203 |
| check-clean, wrong output | 236 `[REVISED: AG-3 filter count; was ~214]` |
| check-clean, sandbox refusal | 53 |
| check-clean, panic | ~17 |

**Out of scope (logged, not dropped):**
- language changes that would accept the habits (§7 NHG-1);
- a `dir_create` grant-root no-op (§7 NHG-2);
- new builtins AG-3 points at: `arr_pop`, an array slice, `str_lines` (§7 NHG-3);
- the model's own type errors (33% of check failures);
- any change to the eval's tasks, tiers or PREREG design;
- plain `axon run` mapping a `main` return of 2-10 to exit 1. This is by design (reserved codes,
  `governance/EXIT_CODES.md`); the intent runner keeps the code. Logged as O-AG-01 `[REVISED: AG-3]`.

## 0. Evidence (round 0, 550 check-refused attempts; reproduce with the classifier in AG-2) `[REVISED: CF-14 550, not 551; CF-11 classifier committed]`

Error classes in the check-refused programs. A program can be in several classes, so shares do not sum to 100%. The
parser stops at the first syntax error, so "sole cause" is an upper bound.

| class | programs | sole cause | help line today | card says it today |
|---|---|---|---|---|
| model type errors (E0102/E0301/E0306/...) | 183 (33%) | 112 | mostly yes | - |
| invented builtins: `arr_new` (34 programs), `arr_pop`, `arr_copy`, `arr_remove`, `is_err`, `unwrap*`, `loop` | 67 (12%) | 37 | no (E0001) | no: the card never shows `[]` |
| `not` / `or` / `and` / `is` | 54 (10%) | 54 | `not`, `or`, `and`: yes (20/20); `is`: no (0/33) | `&&` once; `\|\|` and `!` never |
| `?` in a non-`Result` fn (E0303) | 51 (9%) | 31 | yes | yes (`card.rs:65`) |
| unnamed record literal `{ k: v }` | 35 (6%) | 35 | 1/35 (dict-literal help only) | named form shown (`card.rs:57`) |
| method calls / `unwrap` (E0403) | 33 (6%) | 11 | yes | no |
| Rust forms `[x; n]`, `0i64` | 30 (5%) | 30 | no | no |
| `str(x)` / `i64(x)` conversion | 18 (3%) | 11 | no (E0001) | `to_str` shown |
| unclosed `{` in an interpolated string | 14 (3%) | 14 | no | yes (`card.rs:58`) |
| `fn(...) -> T { }` as a lambda value | 12 (2%) | 12 | no | `\|n\| ...` (`card.rs:138`) |
| `type` inside a fn body; `type X {` without `=` | 18 | - | no | "items live at the top level" (`card.rs:34`) |

`[REVISED: CF-15]` Line numbers are `card.rs` source lines, not `axon card` output lines. `[REVISED: CF-14]` The `not/or/and`
(20) and `is` (33) message counts sum to 53. The 54th program uses one of them after an earlier parse error that the
classifier keys first.

What this shows:
- Some rules the card already states are still broken: `?`, `{{`, lambdas, `type` at top level. Prose under a code block
  does not reach a small model. A contrast list ("you may write X; Axon writes Y") is the form AG-1 tests. It is a
  hypothesis, measured by AG-4.
- `parse_help.rs` already maps `not` / `or` / `and` (`parse_help.rs:343-371` `[REVISED: CF; was 347-364]`). The other
  parse-tier habits above get no help line, so the repair loop (Solve@3) does not learn them either.
- Sandbox: 53 attempts call `dir_create("./out")` under the grant `./out/`. The refusal is intended (CBA-2). Both runners
  create every `fs_write` prefix ending in `/` before the run: Python at `gv.py:540-554`, Axon at `genloop.rs:719-730`
  (the loop, and the grade via `grade_axon` -> `lift` -> `axon-intent compile`). So the call is unnecessary
  `[REVISED: CF-17, Axon side verified]`.

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | AG-1 card 2, AG-2 parse-tier help rows, AG-3 wrong-output audit | authorized; AG-1/AG-2 target ~240 check-refused attempts; AG-3 is the largest unexplained gap and is done (§2) |
| P1 | AG-4 paired benchmark | the measurement; it waits on the PREREG freeze |

Repair digest: no file in `REPAIR_SOURCES` (`scorecard.py:89-91`, `axon-cortex` only) changes, so `receipt --check` stays PASS
and needs no paid run.

## 2. Items

### AG-1 (P1) Card 2: a "write it this way" list, `[]`, `||` / `!`, the `./out/` rule, and AG-3's fixes

**Anchors:**
- `crates/axon-core/src/card.rs:25-172` (`CURATED`), `:21` (`CARD_VERSION`);
- `crates/axon-core/card-versions.txt`;
- `spec/stability.md` §4 (version rule, `:48-58`);
- the card tests in `card.rs` (module from `:640`; hash pin `:731`; `every_card_example_type_checks` `:672-705`)
  `[REVISED: CF-15; was "from :729"]`.

**Change:**
1. A new subsection at the end of §1, "Habits from other languages". It is a **list**, one line per class in §0 whose
   right form exists today, each line `- you may write <wrong> -> Axon writes <right>`. It is not a Markdown table: the
   card is plain text to a model, and `|` inside a table cell needs escapes the model would see literally
   `[REVISED: CF-2]`.

   | key | you may write | Axon writes |
   |---|---|---|
   | `logic` | `a or b`, `a and b`, `not a` | `a \|\| b`, `a && b`, `!a` |
   | `result` | `if r is Err`, `r.is_err()`, `r.unwrap()` | `match r { Ok(v) => .., Err(e) => .. }` |
   | `option` | `o.unwrap_or(d)`, `if o is Some` | `match o { Some(v) => .., None => .. }` `[REVISED: CF-4]` |
   | `empty` | `arr_new()`, `Vec::new()` | `let xs: [i64] = []` |
   | `push` | `xs.push(v)` | `xs = arr_push(xs, v)`; builtins are free functions and return a new array |
   | `pop` | `xs.pop()`, `arr_pop(xs)` | `let last = xs[len(xs) - 1]` then `xs = arr_take(xs, len(xs) - 1)` `[REVISED: CF-1, AG-3 S1]` |
   | `remove` | `arr_remove(xs, i)` | `xs = arr_concat(arr_take(xs, i), arr_drop(xs, i + 1))` `[REVISED: CF-1]` |
   | `copy` | `arr_copy(xs)` | `let ys = xs` `[REVISED: CF-1]` |
   | `repeat` | `[0; n]` | `arr_repeat(0, n)` |
   | `suffix` | `0i64` | `0` |
   | `convert` | `str(n)`, `int(s)`, `i64(s)` | `to_str(n)`, `parse_int(s)` (a `Result`) |
   | `record` | `{ name: n }` (an unnamed record) | `type Item = { name: str }` at the top level, then `Item { name: n }` |
   | `lambda` | `fn(a: i64) -> i64 { a + 1 }` as a value | `\|a: i64\| a + 1` |
   | `question` | `?` in a fn returning `i64` / `bool` / `Option` | `match`, or make the fn return `Result` |
   | `type` | `type T = ...` inside a fn | `type` at the top level only |
   | `brace` | `"{"` meaning a brace character | `"{{"` |

   - The table above is this spec's notation. The card holds the same rows as list lines, with no escapes.
   - Rows name forms only. No task vocabulary: benchmark integrity, AG-4.
   - `[REVISED: CF-1]` The pop/remove/copy rows were open in the draft ("AG-1 writes the row from what exists"). No
     `arr_pop`/`arr_remove`/`arr_copy`/`arr_new` exists in `AXON_REFERENCE.md`, the card or any `.rs` file; the right
     forms above use existing builtins and were run (§6).
   - `[REVISED: CF-12]` A row stays only if its class also occurs in round-0 failures on the 107 pool tasks outside the
     frozen draw (seed in PREREG). A row whose class occurs only on drawn tasks is dropped and logged.
   - `[REVISED: build 2026-11-24]` The CF-12 filter was NOT run: the frozen draw does not exist yet. `PREREG-ae.md` is
     `Status: DRAFT`; the draw takes each tier's first n by `sha256("<seed>:<id>")` from the **survivors** of the
     post-pilot second human review, which is not recorded, and no committed code implements the draw. All 16 rows are
     kept. The filter runs at the freeze, before AG-4; a row it drops is a card change (card 3).
2. Use `[]`, `||` and `!` in the §1 example program. A typed empty array is today shown nowhere.
3. §2 Program rules: "A write prefix ending in `/` that the task allows (e.g. `./out/`) already exists when the program runs.
   Write files into it; never call `dir_create` on it (refused)." `[REVISED: CF-17; the runner check is done, §0]`
4. Bump `CARD_VERSION` to 2, append `2 <sha256>` to `card-versions.txt`, and name `card 2` in the CHANGELOG bullet
   (`release_ledger.py`).
5. `[REVISED: AG-3 S2]` Fix the card's own file example (`card.rs:148-170`). It splits on `"\n"` and writes every piece plus
   `"\n"`, so `"a\nb\n"` becomes `"A\nB\n\n"`, and an empty file yields one line. The example drops a trailing empty piece:
   `if len(lines) > 0 && lines[len(lines) - 1] == "" { lines = arr_take(lines, len(lines) - 1) }`. Observed: the card
   idiom prints `A B` plus an empty line; the fixed form prints `A B` and gives 0 lines for `""` (§6). The bug sits in
   16 of the 30 sampled wrong-output rows.
6. `[REVISED: AG-3 S3]` §2 Program rules (`card.rs:143`): "On `Err`, report with `eprintln`" becomes "Print what the task
   says to print with `println` (stdout), including error messages it names. Use `eprintln` only for a message the task
   does not specify." In the sample, 7 of 30 rows wrote a required message to stderr.

**Size budget:** the card is 31,582 bytes. Card 2 adds ≤ 2,000 bytes for AG-1 and AG-3 together; the cap 33,600 covers both
`[REVISED: CF-16]`. The per-call prompt grows by about 500 tokens (under 4% of the 13-14.7k input tokens per call), which CERE
absorbs at $0.

**Fail-first test (new, `card.rs` tests):** `every_habit_row_is_true_of_the_checker` `[REVISED: CF-3]`.
- The test holds a const fixture table: (row key, a whole good program, one or more whole bad programs).
- It parses the row keys out of `CURATED` (the habit list lines carry their key) and asserts they **equal** the fixture
  keys exactly.
- Every good program checks with 0 errors and every bad program gets ≥ 1 error, via the `check_pipeline` pattern of
  `card.rs:695-704`.
- It fails before the change because the habit list does not exist (the key set is empty).
- A row that stops being true (a language change) fails the build.
- The existing `every_card_example_type_checks` covers changes 2 and 5.

**g01:** the new test, plus the existing card-version test, kill mutants of the row parser. Rows go in the lane's reserved
block (§5).

**Acceptance:**
- `axon card --card-version` prints `2`;
- `card-versions.txt` ends with `2 <sha of axon card>`;
- the new test passes;
- `release_ledger.py` passes;
- `axon card | wc -c` ≤ 33,600;
- the CF-12 filter result (rows kept, rows dropped) is in the lane report.

### AG-2 (P1) Parse-tier help rows for the habits that get none

**Anchors:**
- `crates/axon-core/src/parse_help.rs:200` (`parse_help`) and `:140` (`refused_form_help`);
- the existing dict-literal arm at `parse_help.rs:461-474` and the walrus arm at `:514`, which share `Colon, expected
  expression` `[REVISED: CF-10]`;
- `crates/axon-core/tests/parse_help_probe.rs`;
- the module doc's three-step rule (`parse_help.rs:28-30` `[REVISED: CF; was 27-29]`: arm + test + probe line).

**Rows.** Key each one on a message the compiler is observed to emit, plus a source shape. Probe it first:

| habit | observed message (round 0) | count | shape key | help (draft) |
|---|---|---|---|---|
| `if x is Err` / `is Some(..)` | `unexpected token: Ident("is"), expected LBrace` (also `while .. is`) | 33 | token is `is` | "`is` is not an Axon operator - test a variant with `match x { Err(_) => ..., _ => ... }`" |
| `{ k: v }` unnamed record | `unexpected token: Colon, expected expression` | 35 | after the dict and walrus arms; `!line.contains("{\"") && !line.contains(":=")` `[REVISED: CF-10]` | an unnamed record needs a named type, `type T = { k: ... }` then `T { k: v }`; a map is `dict_from_pairs` |
| `[x; n]` | `unexpected token: Semi, expected RBracket` | 18 | a `[` opens the expression `[REVISED: CF-10]` | "Axon has no `[x; n]` - write `arr_repeat(x, n)`" |
| `0i64` | `unexpected token: Ident("i64"), expected RParen` **or** `expected RBracket` | ≤ 15 | a digit right before the suffix `[REVISED: CF-8]` | "integer literals take no type suffix - write `0`" |
| `fn(..)` as a value | `unexpected token: Fn, expected expression` | 12 | line has `= fn(`, `(fn(` or `, fn(` `[REVISED: CF-5]` | "a lambda is written `\|a: i64\| a + 1`, not `fn(..)`" |
| nested `fn` item | same message | - | `fn` starts the statement `[REVISED: CF-5]` | "fn items live at the top level, outside any fn" |
| `"{"` in a string | ``unclosed `{` in interpolated string`` `[REVISED: CF-9, DL-5; backticks]` | 12 | exact message | "a literal brace in a string is `{{` / `}}`" |
| `type` in a fn body | `unexpected token: Type, expected expression` | 9 | exact message | "`type` declarations live at the top level, outside any fn" |
| `type X { .. }` | `unexpected token: LBrace, expected Eq` | 9 | first word of the line is `type` `[REVISED: CF-7; let x { 1 } shares it]` | "a record type is `type X = { field: T }`" |
| `a ** 2` | `unexpected token: Star, expected expression` | - | line contains `**` `[REVISED: CF-6]` | "Axon has no `**` - multiply, or use a `pow_*` builtin" |
| `*x` deref | same message | 5 | otherwise `[REVISED: CF-6]` | "Axon has no `*` dereference - use the value directly" |

- `[REVISED: CF-8]` `let x = 0i64` gives E0001 "cannot find name `i64`" at the resolve tier, which `parse_help` never sees.
  It is logged as uncovered (O-AG-02), not keyed here.
- Each arm keys on (message, local source shape), as the existing arms do. A key that also fires on an unrelated error with
  the same message gets the narrower source check, or it is dropped; the probe proves the shape.

**Classifier (new, committed):** `crates/axon-intent/benchmarks/genviab/habits.py` `[REVISED: CF-11, DL-6]`.
- It reads a run directory's round-0 transcripts (`rounds[0].report`), re-checks each `source` with `axon check --json`
  (stderr, one JSON line per diagnostic), and prints per habit class (message plus shape, as in the table above): n, and n
  with a help line.
- It holds no task text. Its selftest runs on synthetic programs.
- §0's counts and AG-2's acceptance are both its output.

**Fail-first tests:** one unit test per arm (help present, exact text) and one probe line per arm in
`parse_help_probe.rs`. Each probe fails before the change because the help is `None`. Each narrowed arm also gets a
negative test: the colliding shape (`let x { 1 }`, a nested `fn`, `a ** 2`, `{"k": 1}`, `x := 1`) does not get this arm's
help.

**g01:** one row per arm; the killing test is that arm's unit test.

**Acceptance:**
- every arm's probe passes;
- `habits.py` on the pilot run through the new binary (no model) shows a help line on ≥ 90% of each habit class's
  instances, with n per class shown; a narrowed or dropped arm states its excluded count `[REVISED: DL-6]`;
- existing parse-help tests are unchanged.

### AG-3 (P1) Wrong-output audit (30-sample) and its card lines - DONE 2026-11-24

**Method:**
- Seeded sample of 30 rows: `infra` false, check-clean, not verified, `wrong-output`, ordered by
  `sha256("wo-sample:{id}:{seed}")`. The filter gives 236 rows.
- Each is classified `model-logic` / `axon-surprise` / `card-gap` / `harness` / `other`, with a synthetic repro for every
  surprise. Each round-0 program was re-run offline on its failing examples with the pilot binary.
- Notes live outside the repo (`/var/tmp/r51-tmp/wrong-output/NOTES.md`, repros in `repro/`); task text never enters the
  public repo.

**Result** `[REVISED: AG-3 results replace the PENDING marker]`:

| class | n / 30 | 95% Clopper-Pearson |
|---|---|---|
| model-logic | 27 | 73.5-97.9% |
| axon-surprise | 3 | 2.1-26.5% |
| card-gap, harness, other | 0 each | 0-11.6% each |

Factors across classes (a row can have several):
- Splitting a file into lines: 19/30 (CI 44-80%). 16 rows mishandle a trailing newline, an empty file or a last line with
  no newline. 3 rows try `arr_drop` to remove the trailing empty piece. Python on the same tasks passed 12 of the 14
  trailing-newline tasks checked. The card's own file example teaches the bug (S2).
- A message the task says to print goes to stderr via `eprintln`: 7/30 (CI 10-42%). The card's rule says to use
  `eprintln` on `Err` (S3).
- `arr_drop` used to remove the last element: 5/30 (3 primary, 2 secondary).

| # | behaviour | in card? | intended? | disposition |
|---|---|---|---|---|
| S1 | `arr_drop` drops from the front; the card shows no pop, drop-last or slice | documented | yes | AG-1 `pop` row; a new builtin -> NHG-3 |
| S2 | the card's file example writes an extra empty line and counts `""` as one line | it is the card's example | `str_split` matches Python: yes | AG-1 change 5; a `str_lines` builtin -> NHG-3 |
| S3 | the card rule "On `Err`, report with `eprintln`" | the rule itself | - | AG-1 change 6 |
| S4 | `dir_create("./out")` under grant `./out/` exits 8, not `Err` | partly (`card.rs:144`) | exit 8 by design | AG-1 change 3; matching -> NHG-2 |

These are counted as model-logic, but the cause of S2 and S3 is on the Axon side (the card). There were no harness findings:
expected outputs matched the task texts in all 30.

**Acceptance:** met. Every `card-gap`-like finding maps to an AG-1 change; the builtin questions are in §7.

### AG-4 (P1) The paired benchmark: card 2 + help rows vs card 1

**Precondition:** `PREREG-ae.md` FROZEN.
- The A/A run (seeds 3,4,5) is in progress.
- Then the pilot table and power, then the "fails all 6 trials" ambiguity review (spec AE-3). That review needs human
  verdicts.
- Before the run: the served CERE series equals PREREG's pin, and a CERE deploy hold covers the window. A redeploy after the
  freeze makes `ae.py` refuse (`ae.py:547-549`, "needs a bridge-model run"); that means a bridge run, not a retry
  `[REVISED: DL-3]`.

**Run** `[REVISED: CF-13, DL-1, DL-2]`:
- a one-off frozen-set A/B (the form AE allows for a prompt-content experiment, `authoring-eval-spec.md` O-AE-03 row),
  `ae.py run --advisory --label ag-card2`;
- `--base-bin` = the first parent of the AG merge commit (card 1; its sha8 is named in PREREG before the run),
  `--cand-bin` = the AG merge commit. The draft's "main at freeze" was wrong: if the freeze lands after the AG merge, both
  sides carry card 2 and `--expect-card-change` refuses (`ae.py:561-562`);
- `--expect-card-change` (round-0 prompts differ, so `paired_mode: resample`, trials paired by seed);
- k = PREREG's frozen k (AE-3, `authoring-eval-spec.md:418`). On the pilot rows, FT power at 4 points is about 0.52 at
  k = 3 and about 0.82 at k = 6 (DL-2 simulation, `/var/tmp/r51-tmp/review-AG/devloop/power.py`). The A/A run refines this.
  Plus arm c;
- one CERE daytime window, outside 01:30-08:00. A run cut at 01:30 is INCOMPLETE (`ae.py:31-32`).
- It is not a release-over-release trend row: base and candidate are not releases. The first release that carries card 2
  gives the trend row `[REVISED: CF-13]`.

**Optional interim (4b)** `[REVISED: DL-7, accepted as optional; default: not run unless the freeze is more than 7 days
out]`:
- `ae.py run --pilot`, full pool, base a01db50c, cand the AG merge, seeds 0-2;
- base and arm c replay from the cache, so the cost is about 3.5 h of candidate calls; FT power about 0.68 at 4 points;
- no card or help edits after it; results sealed from the AE-3 ambiguity reviewers; recorded in PREREG.

**Pre-registered prediction (written to PREREG before the run):**
- Round-0 FT-pass Δ in [+0.03, +0.10] overall [INFERENCE]. AG-2 does not change round 0, so its effect shows in Solve@3.
- MDE at 0.8 power: taken from the frozen table. A NO CHANGE result does not refute a Δ below it `[REVISED: DL-2]`.
- The check-refused share falls. The listed parse classes, **pooled**, fall by ≥ 50% `[REVISED: DL-2; per class has 3-25
  events]`.
- The trailing-newline share of wrong-output rows falls (AG-1 change 5).

**Report:**
- Δ FT-pass (headline, AE §3 clustered test), Δ Solve@3, per-tier Δ, Δ check-refused share;
- the per-class shift (`habits.py`, base vs cand);
- AG-3's wrong-output share.

**Integrity:**
- The card rows derive from error classes measured on the pool the frozen set is drawn from. The rows name language forms,
  not task content: a reviewer checks every row against "would this line help on a task never seen".
- `[REVISED: CF-12]` Rows are kept only if their class also occurs outside the frozen draw (AG-1). The contamination note
  and the prediction go into PREREG before the run.
- `[REVISED: CF-13]` The draft's "release-over-release" claim is withdrawn; this is an advisory A/B.

**Cost** `[REVISED: DL-3]`:
- Pilot rates: 2.58 calls per Axon trial, 1.15 per Python trial, 4.5 s per fresh call at `--parallel 2`.
- The cache key holds no binary (`http-program.py:30,108`). Base and arm c replay the pilot's replies (seeds 0-2) and the
  A/A run's (seeds 3-5); only the candidate arm is fresh.
- Fresh: about 2.3 h at k = 3, about 4.7 h at k = 6. Uncached: about 5.7 h / 11 h. $0.
- [INFERENCE] Repair-round prompts replay only where the base binary's diagnostics match a01db50c.

## 3. Sequencing

1. AG-3: done (offline).
2. AG-2 and AG-1 changes 1-6 build in one lane (both in `axon-core`, sharing g01 and the PCI note), with `habits.py`.
3. One card version for the batch: AG-3's card changes (5, 6) are in AG-1 before its version bump.
4. Strict gate, implementation audit, merge.
5. Optional 4b interim (AG-4).
6. AG-4 after the freeze.

## 4. Gates and cost budget

| gate | when | cost | resource |
|---|---|---|---|
| `cargo test -p axon-core --no-default-features` card + parse_help tests | per commit | ~2 min [INFERENCE] | CPU |
| pre-lane: `pci_delta.py --check`; `cargo test -p axon-core --no-default-features --test pci_delta_note`; `release_ledger.py` `[REVISED: DL-4]` | before each lane | < 3 min | CPU |
| `gate.sh --lane` | lane done | ~45-54 min (CS5 and RC lanes selected every unit) | CPU |
| g01 `--only` new rows | lane done | ~11-15 s/row | CPU |
| PCI note (`tasks/r51-pci-regen.py`, `pci_delta.py --check`) | each `axon-core/src` commit, and again after any rebase/squash and the integration merge (theme files are keyed by hash8) `[REVISED: DL-4]` | < 1 min | CPU |
| `gate.sh --strict` | integration | ~51 min | CPU |
| `habits.py` replay (AG-2 acceptance) | lane done | < 5 min, no model (551 sources check in 2.5 s) | CPU |
| AG-4 interim (optional) | after merge | ~3.5 h, $0 | CERE GPU1, daytime |
| AG-4 | after the freeze | ~2.3 h (k = 3) to ~4.7 h (k = 6) cached; ≤ 11 h uncached; $0 | CERE GPU1, daytime |

A run above 2× budget is reviewed before it repeats.

## 5. g01 block

Reserved M5280-M5299. Each row names exactly one (crate, target, test).

## 6. Verification log

- V1 classifier (round-0 reports in transcripts, categorised by code and message): counts as in §0, run 2026-11-24 in the
  integrator's kernel. `[REVISED: CF-11]` AG-2 commits it as `habits.py`. DevloopAG reproduced the help counts
  independently: `is` 0/33, colon 1/35, `[x;n]` 0/18, `i64` 0/11, `fn` 0/12, `type` 0/9, `type X {` 0/9, `*` 0/5.
- Right-form probes on the pilot binary:
  - `let xs: [i64] = []`, `arr_push`, `arr_repeat`, `|a: i64| a + 1`, `||`, `!`, `{{` all check clean and run (output
    `5 {lit}`);
  - a named record plus a `parse_int` match ran and printed `13`;
  - `r.is_err()` -> E0403;
  - `[REVISED: CF-1]` pop/remove/copy forms: `last = xs[len(xs)-1]`, `arr_take(xs, len(xs)-1)`,
    `arr_concat(arr_take(xs, 0), arr_drop(xs, 1))` check clean; on `[1,2,3]` they printed `3 2 2`;
  - `[REVISED: AG-3 S2]` the card's file idiom on `"a\nb\n"` prints `A`, `B` and an empty line; with the trailing-piece
    drop it prints `A`, `B`. `len(str_split("", "\n"))` is 1.
  - `[REVISED: AG-3]` plain `axon run` of `main` returning 4 prints the RESERVED-code notice and exits 1.
- Wrong forms on the pilot binary:
  - `if true or false` -> E0000;
  - `if not true` -> E0000;
  - `fn(a: i64) -> i64 { a }` as a value -> E0000;
  - `{ name: "x" }` -> E0000.
- `[REVISED: CF]` AuditAG confirmed every AG-1 right form checks with 0 errors and runs, and every wrong form is refused;
  and observed the colliding shapes in CF-5..CF-10.
- Receipt: `scorecard.py receipt --check` PASS at 52279a84; the digest covers `axon-cortex` only (`scorecard.py:89-99`).
- `[REVISED: build 2026-11-24]` Build lane: `habits.py` on the pilot run, the pre-AG-2 binary then the lane binary,
  own help / n: `is` 0 -> 33/33, record 0 -> 34/34, `[x; n]` 0 -> 18/18, suffix 0 -> 12/12, `fn(..)` value 0 -> 7/7,
  nested `fn` 0 -> 5/5, `"{"` 0 -> 12/12, `type` in fn 0 -> 9/9, `type X {` 0 -> 9/9, `*x` 0 -> 5/5, `**` n = 0;
  excluded 0 per class (550 refused attempts, 1 now checks clean). The record class is 34, §0's colon count 35: the
  35th is a `{"params": ..}` dict literal, which keeps the dict row's help (DevloopAG's 1/35).

## 7. needs-human (excluded from the build)

| Q | Question | Recommendation |
|---|---|---|
| NHG-1 | Accept `and`/`or`/`not`, unnamed record literals, or `str(x)` in the language? | No: a second spelling per construct; the card list and help rows teach the one form |
| NHG-2 | Make `dir_create` on an already-granted write root a no-op instead of a refusal, or match `./out` against `./out/`? | Not now: a sandbox-semantics change (CBA-2) needs a security review; AG-1 change 3 removes the reason models call it |
| NHG-3 | Add builtins AG-3 points at: `arr_pop` / an array slice, `str_lines` (split without the trailing empty piece)? `[REVISED: AG-3]` | Not in this batch. Measure card 2 first (AG-4); if the trailing-newline share of wrong-output rows stays above 25%, propose `str_lines` in its own spec |

Step 2 classification: CF-1..CF-17 and DL-1..DL-7 are engineering decisions, now canon above. NHG-1..NHG-3 are
language/sandbox decisions and stay needs-human; none blocks the build.
