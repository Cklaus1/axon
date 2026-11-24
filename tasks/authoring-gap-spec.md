# authoring-gap - spec (AG-1..AG-4)

**Status: DRAFT (before Step 1 review). Build authorized by the human 2026-11-24.** Items 1-3 were authorized as "fix, and
benchmark". Language changes are NOT authorized (§7).

**Source:** pilot attempt 2 of the authoring eval (`PREREG-ae.md` "Pilot attempt 2"; run `20261111-183515-pilot-k3-a2`,
private). First-try FT-pass for CERE Qwen was T1 0.378 / T2 0.153 / T3 0.055 / TS 0.203. The Python arm scored 0.90 / 0.86 /
0.69 / 0.92 on the same tasks.

Where the 1039 scored round-0 Axon attempts end:

| ending | n |
|---|---|
| refused by `axon check` | 550 |
| check-clean, passed | 203 |
| check-clean, wrong output | ~214 |
| check-clean, sandbox refusal | 53 |
| check-clean, panic | ~17 |

**Out of scope (logged, not dropped):**
- language changes that would accept the habits (§7 NHG-1);
- a `dir_create` grant-root no-op (§7 NHG-2);
- the model's own type errors (33% of check failures);
- any change to the eval's tasks, tiers or PREREG design.

## 0. Evidence (round 0, 551 check-refused attempts; reproduce with the classifier in §6 V1)

Error classes in the check-refused programs. A program can be in several classes, so shares do not sum to 100%. The
parser stops at the first syntax error, so "sole cause" is an upper bound.

| class | programs | sole cause | help line today | card says it today |
|---|---|---|---|---|
| model type errors (E0102/E0301/E0306/...) | 183 (33%) | 112 | mostly yes | - |
| invented builtins: `arr_new` (34 programs), `arr_pop`, `arr_copy`, `arr_remove`, `is_err`, `unwrap*`, `loop` | 67 (12%) | 37 | no (E0001) | no: the card never shows `[]` |
| `not` / `or` / `and` / `is` | 54 (10%) | 54 | `not`, `or`, `and`: yes (20/20); `is`: no (0/33) | `&&` once; `\|\|` and `!` never |
| `?` in a non-`Result` fn (E0303) | 51 (9%) | 31 | yes | yes (line 65) |
| unnamed record literal `{ k: v }` | 35 (6%) | 35 | 1/35 (dict-literal help only) | named form shown (line 57) |
| method calls / `unwrap` (E0403) | 33 (6%) | 11 | yes | no |
| Rust forms `[x; n]`, `0i64` | 30 (5%) | 30 | no | no |
| `str(x)` / `i64(x)` conversion | 18 (3%) | 11 | no (E0001) | `to_str` shown |
| unclosed `{` in an interpolated string | 14 (3%) | 14 | no | yes (line 58) |
| `fn(...) -> T { }` as a lambda value | 12 (2%) | 12 | no | `\|n\| ...` (line 138) |
| `type` inside a fn body; `type X {` without `=` | 18 | - | no | "items live at the top level" (line 34) |

What this shows:
- Some rules the card already states are still broken: `?`, `{{`, lambdas, `type` at top level. Prose under a code block
  does not reach a small model. A contrast table ("you may write X; Axon writes Y") is the form AG-1 tests. It is a
  hypothesis, measured by AG-4.
- `parse_help.rs` already maps `not` / `or` / `and` (`parse_help.rs:347-364`). The other parse-tier habits above get no
  help line, so the repair loop (Solve@3) does not learn them either.
- Sandbox: 53 attempts call `dir_create("./out")` under the grant `./out/`. The refusal is intended (CBA-2). The runner
  creates every `fs_write` prefix before the run (`gv.py:540-554`; verify the Axon side in AG-1), so the call is
  unnecessary.

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | AG-1 card 2, AG-2 parse-tier help rows, AG-3 wrong-output audit | authorized; AG-1/AG-2 target ~240 check-refused attempts; AG-3 is the largest unexplained gap |
| P1 | AG-4 paired benchmark | the measurement; it waits on the PREREG freeze |

Repair digest: no file in `REPAIR_SOURCES` (`scorecard.py:89-91`, `axon-cortex` only) changes, so `receipt --check` stays PASS
and needs no paid run.

## 2. Items

### AG-1 (P1) Card 2: a "write it this way" table, `[]`, `||` / `!`, and the `./out/` rule

**Anchors:**
- `crates/axon-core/src/card.rs:25-172` (`CURATED`), `:21` (`CARD_VERSION`);
- `crates/axon-core/card-versions.txt`;
- `spec/stability.md` §4 (version rule);
- the card tests in `card.rs` from `:729`.

**Change:**
1. A new subsection at the end of §1, "Habits from other languages". It is a table of `you may write` -> `Axon writes`,
   one row per class in §0 whose right form exists today:

   | you may write | Axon writes |
   |---|---|
   | `a or b`, `a and b`, `not a` | `a \|\| b`, `a && b`, `!a` |
   | `if r is Err`, `r.is_err()`, `r.unwrap()`, `o.unwrap_or(d)` | `match r { Ok(v) => ..., Err(e) => ... }` |
   | `arr_new()`, `Vec::new()` | `let xs: [i64] = []` |
   | `xs.push(v)`, `xs.pop()` | `xs = arr_push(xs, v)`; builtins are free functions |
   | `[0; n]` | `arr_repeat(0, n)` |
   | `0i64` | `0` |
   | `str(n)`, `int(s)`, `i64(s)` | `to_str(n)`, `parse_int(s)` (a `Result`) |
   | `{ name: n }` (an unnamed record) | `type Item = { name: str }` at the top level, then `Item { name: n }` |
   | `fn(a: i64) -> i64 { a + 1 }` as a value | `\|a: i64\| a + 1` |
   | `?` in a fn returning `i64` / `bool` / `Option` | `match`, or make the fn return `Result` |
   | `type T = ...` inside a fn | `type` at the top level only |
   | `"{"` meaning a brace character | `"{{"` |

   - Rows name forms only. No task vocabulary: benchmark integrity, AG-4.
   - `arr_pop` / `arr_remove` / `arr_copy`: AG-1 writes the row from what exists, checked against `AXON_REFERENCE.md`. If no
     builtin exists, the row says how to do it with existing builtins.
2. Use `[]`, `||` and `!` in the §1 example program. A typed empty array is today shown nowhere.
3. §2 Program rules: "A write prefix the task allows (e.g. `./out/`) already exists when the program runs. Write files into
   it; do not call `dir_create` on it (refused)." First verify that the Axon-arm runner pre-creates write prefixes. The
   Python side is `gv.py:540-554`; the Axon side is the axon-intent workspace code, so cite the file:line. If it does not,
   this line is dropped and logged.
4. Bump `CARD_VERSION` to 2, append `2 <sha256>` to `card-versions.txt`, and name `card 2` in the CHANGELOG bullet
   (`release_ledger.py`).

**Size budget:** the card is 31,582 bytes; card 2 adds ≤ 2,000 bytes. The per-call prompt grows by about 500 tokens, which
CERE absorbs at $0.

**Fail-first test (new, `card.rs` tests):** `every_habit_row_is_true_of_the_checker`. For each table row, a small program
using the right form checks clean, and the same program using the wrong form is refused with an error. The test parses the
rows out of `CURATED`, so a row that stops being true (a language change) fails the build. It fails before the change because
the table does not exist (assert the row count is ≥ 10). The existing test that every ```` ```axon ```` block checks clean
covers change 2.

**g01:** the new test, plus the existing card-version test, kill mutants of the row parser. Rows go in the lane's reserved
block (§5).

**Acceptance:**
- `axon card --card-version` prints `2`;
- `card-versions.txt` ends with `2 <sha of axon card>`;
- the new test passes;
- `release_ledger.py` passes;
- `axon card | wc -c` ≤ 33,600.

### AG-2 (P1) Parse-tier help rows for the habits that get none

**Anchors:**
- `crates/axon-core/src/parse_help.rs:200` (`parse_help`) and `:140` (`refused_form_help`);
- `crates/axon-core/tests/parse_help_probe.rs`;
- the module doc's three-step rule (`parse_help.rs:27-29`: arm + test + probe line).

**Rows.** Key each one on a message the compiler is observed to emit, and probe it first:

| habit | observed message (round 0) | count | help (draft) |
|---|---|---|---|
| `if x is Err` / `is Some(..)` | `unexpected token: Ident("is"), expected LBrace` | 33 | "`is` is not an Axon operator - test a variant with `match x { Err(_) => ..., _ => ... }`" |
| `{ k: v }` unnamed record | `unexpected token: Colon, expected expression` | 35 | extend the existing dict-literal help: an unnamed record needs a named type, `type T = { k: ... }` then `T { k: v }`; a map is `dict_from_pairs` |
| `[x; n]` | `unexpected token: Semi, expected RBracket` | 18 | "Axon has no `[x; n]` - write `arr_repeat(x, n)`" |
| `0i64` | `unexpected token: Ident("i64"), expected RParen` (after a digit) | ≤ 15 | "integer literals take no type suffix - write `0`" |
| `fn(..)` as a value | `unexpected token: Fn, expected expression` | 12 | "a lambda is written `\|a: i64\| a + 1`, not `fn(..)`" |
| `"{"` in a string | `unclosed { in interpolated string` | 12 | "a literal brace in a string is `{{` / `}}`" |
| `type` in a fn body | `unexpected token: Type, expected expression` | 9 | "`type` declarations live at the top level, outside any fn" |
| `type X { .. }` | `unexpected token: LBrace, expected Eq` after `type X` | 9 | "a record type is `type X = { field: T }`" |
| `*x` deref | `unexpected token: Star, expected expression` | 5 | "Axon has no `*` dereference - use the value directly" |

Each arm keys on (message, local source shape), as the existing arms do. A key that also fires on an unrelated error with
the same message gets the narrower source check, or it is dropped; the probe proves the shape.

**Fail-first tests:** one unit test per arm (help present, exact text) and one probe line per arm in
`parse_help_probe.rs`. Each probe fails before the change because the help is `None`.

**g01:** one row per arm; the killing test is that arm's unit test.

**Acceptance:**
- every arm's probe passes;
- re-running the §6 V1 classifier on the pilot's round-0 sources through the new binary (`axon check --json`, no model)
  shows a help line on ≥ 90% of each listed message class;
- existing parse-help tests are unchanged.

### AG-3 (P1) Wrong-output audit (30-sample) and its card lines

**Method:**
- Seeded sample of 30 rows: `infra` false, check-clean, not verified, `wrong-output`, ordered by
  `sha256("wo-sample:{id}:{seed}")`.
- Each is classified `model-logic` / `axon-surprise` / `card-gap` / `harness` / `other`, with a synthetic repro for every
  surprise. Runs are offline on the pilot binary.
- Notes live outside the repo (`/var/tmp/r51-tmp/wrong-output/NOTES.md`); task text never enters the public repo.

**Change:**
- a behaviour that is intended but documented nowhere the model sees -> one card line in AG-1's card 2 (same version bump);
- a behaviour that looks wrong -> `opportunities.md` entry, plus §7 needs-human if fixing it changes behaviour;
- a harness finding -> fixed in `genviab` before AG-4, with a selftest.

**Acceptance:** class counts with 95% Clopper-Pearson intervals recorded here; every `card-gap` row maps to a card line or a
logged reason.

`[PENDING: WrongOutput agent results]`

### AG-4 (P1) The paired benchmark: card 2 + help rows vs main

**Precondition:** `PREREG-ae.md` FROZEN.
- The A/A run (seeds 3,4,5) is in progress.
- Then the pilot table and power, then the "fails all 6 trials" ambiguity review (spec AE-3). That review needs human
  verdicts.

**Run:**
- `ae.py run` on the frozen set;
- `--base-bin` = main at freeze, `--cand-bin` = the AG build;
- `--expect-card-change` (round-0 prompts differ, so `paired_mode: resample`, trials paired by seed);
- k = 3, plus arm c;
- one CERE daytime window, outside 01:30-08:00.

**Pre-registered prediction (written to PREREG before the run):**
- Round-0 FT-pass Δ in [+0.03, +0.10] overall [INFERENCE]. AG-2 does not change round 0, so its effect shows in Solve@3.
- The check-refused share falls. The listed parse classes fall by ≥ 50%.

**Report:**
- Δ FT-pass (headline, AE §3 clustered test), Δ Solve@3, per-tier Δ, Δ check-refused share;
- the per-class shift (V1 classifier, base vs cand);
- AG-3's wrong-output share.

**Integrity:**
- The card rows derive from error classes measured on the pool the frozen set is drawn from. The rows name language forms,
  not task content: a reviewer checks every row against "would this line help on a task never seen".
- The trend row is the release-over-release measurement AE exists for; it is not a tuned baseline.

**Cost:** CERE about 5-6 h (pilot attempt 2: 3123 rows in 5 h 20 min; resample doubles the Axon generations of a
240-task set: about 1440 + 720 Python), $0.

## 3. Sequencing

1. AG-3 runs now (offline).
2. AG-2 and AG-1 changes 1-2 build in one lane (both in `axon-core`, sharing g01 and the PCI note).
3. AG-1 takes AG-3's card lines before its version bump. There is one card version for the batch.
4. Strict gate, implementation audit, merge.
5. AG-4 after the freeze.

## 4. Gates and cost budget

| gate | when | cost | resource |
|---|---|---|---|
| `cargo test -p axon-core --no-default-features` card + parse_help tests | per commit | ~2 min | CPU |
| `gate.sh --lane` | lane done | ~45 min (CS5: 43-46 min) | CPU |
| g01 `--only` new rows | lane done | ~15 s/row | CPU |
| PCI note (`tasks/r51-pci-regen.py`, `pci_delta.py --check`) | each `axon-core/src` commit | < 1 min | CPU |
| `gate.sh --strict` | integration | ~51 min | CPU |
| V1 classifier replay (AG-2 acceptance) | lane done | < 5 min, no model | CPU |
| AG-4 | after the freeze | 5-6 h, $0 | CERE GPU1, daytime |

A run above 2× budget is reviewed before it repeats.

## 5. g01 block

Reserved M5280-M5299. Each row names exactly one (crate, target, test).

## 6. Verification log

- V1 classifier (round-0 reports in transcripts, categorised by code and message): counts as in §0, run 2026-11-24 in the
  integrator's kernel.
- Right-form probes on the pilot binary:
  - `let xs: [i64] = []`, `arr_push`, `arr_repeat`, `|a: i64| a + 1`, `||`, `!`, `{{` all check clean and run (output
    `5 {lit}`);
  - a named record plus a `parse_int` match ran and printed `13`;
  - `r.is_err()` -> E0403.
- Wrong forms on the pilot binary:
  - `if true or false` -> E0000;
  - `if not true` -> E0000;
  - `fn(a: i64) -> i64 { a }` as a value -> E0000;
  - `{ name: "x" }` -> E0000.
- Receipt: `scorecard.py receipt --check` PASS at 52279a84; the digest covers `axon-cortex` only (`scorecard.py:89-99`).

## 7. needs-human (excluded from the build)

| Q | Question | Recommendation |
|---|---|---|
| NHG-1 | Accept `and`/`or`/`not`, unnamed record literals, or `str(x)` in the language? | No: a second spelling per construct; the card table and help rows teach the one form |
| NHG-2 | Make `dir_create` on an already-granted write root a no-op instead of a refusal? | Not now: a sandbox-semantics change (CBA-2) needs a security review; AG-1 change 3 removes the reason models call it |
| NHG-3 | Behaviour changes AG-3 may find | listed when AG-3 reports |
