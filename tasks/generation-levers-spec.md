# generation-levers - DRAFT spec (LV-0..LV-5; `GL-` is the gate-lane spec's)

**Status: AUTHORIZED 2026-11-26 (NHL-4, human "go"): build LV-0..LV-3; LV-4/LV-5 run after AG-5's result is recorded.** Two levers on how a model writes Axon, each measured
as its own advisory A/B on the frozen authoring set: grammar-constrained decoding (LV-1, LV-2, LV-4) and a card sliced
per task (LV-3, LV-5). Neither changes the language, the checker or the interpreter.

**Step 1 review (2026-11-25):**
- `AuditGL` (conformance): 19 findings, GLA-1..GLA-19 (5 blockers, 8 major, 6 minor as listed; its header said 4/8/7).
- `DevloopGL` (dev-loop/cost): 11 findings, GLD-1..GLD-11 (4 major, 7 minor).
- All 30 accepted; GLA-5 and GLD-1 are the same finding. Every disposition is inline as `[REVISED: GLA-#/GLD-# ...]`.

**Source:** human request 2026-11-25 after AG-4 (`PREREG-ae.md` "AG-4 result"): "write spec" for grammar-constrained
decoding ("compile spec/grammar.ebnf to a grammar the server enforces ... check if CERE supports this; if not, should we
write a spec for it for CERE?") and "a card sliced per task: only the builtin sections the task needs ... does CERE
support this?".

**Base commit:** `main` = `6222c16d`. The base card is card 4 (`CARD_VERSION` 4, `crates/axon-core/card-versions.txt`
line 4, 33,593 bytes) unless AG-5 (running, `20261125-211816-ag-card4`) says revert to card 3; LV-4/LV-5 take whichever
card `main` ships when they start ("card N"), and name it in `PREREG-ae.md` before the run.

**Out of scope:**
- Any language, checker, parser or interpreter change. A grammar that disagrees with the parser is a grammar bug.
- Grammar support in the product generator path (`axon-intent --generator ai:<model>`, `axon-ai`): needs-human NHL-1.
- Making the sliced card the default: needs-human NHL-2.
- Any CERE change (other repository). NHL-3 covers the one CERE knob this spec could need.
- Best-of-n, retrieved examples, fine-tuning (the other levers listed 2026-11-25): separate specs if wanted.

## 0. Evidence (observed 2026-11-25, $0 except one live probe)

**CERE supports grammar-constrained decoding today; no CERE spec is needed.**
- The served profile `native-g0-qwen38-flash-next-v58` (`prod-t4`, `prod-t4.toml:45`) lists `ebnf`, `regex`,
  `json_schema` in `structured_outputs` (`ai-cere/configs/deployments/qwen38-flash-next-native-rows16-v58.capabilities.json`),
  with validated combinations `output: "ebnf"` for `sampler_mode` greedy AND sampled, row counts 1-16, context 262,144.
- Request shape: top-level `"ebnf": "<grammar>"`, in `validate.rs` `ALLOWED_FIELDS` (:27), a non-empty string of at
  most `MAX_GRAMMAR_BYTES` = 16 KiB (`ai-cere/crates/cere-runtime/src/constraint.rs:13`); one constraint field per
  request, a later one is refused (`constraint.rs:40-41`) `[REVISED: GLA-16, was the SGLang rule in m28-spec.md:75]`.
  The dialect is xgrammar's (GBNF-like). The grammar sidecar in the served image has xgrammar **0.2.1** (`pip show` in
  `prod-cere-t4`); `grammar_drafts = "on"` (`prod-t4.toml:111`).
- Thinking is off unless requested (sidecar `enable_thinking` default false, `scripts/native/tokenizer_sidecar.py:12-13`);
  the benchmark driver never requests it (`drivers/http-program.py:253-254`), so the grammar applies to the whole reply.
  In reasoning mode the masks are all-ones (`tokenizer_sidecar.py:95`), i.e. the grammar would bind only the answer.
- Live probe (one request, toy grammar `fn main() -> i64 {\n println(" str ")\n 0\n}\n`): `finish_reason` stop, the
  reply was exactly in the grammar. Offline: `xgrammar.testing._is_grammar_accept_string` works in the 0.2.1 image
  without a GPU or tokenizer (`True` / `False` on a 2-string probe).

**CERE needs nothing for a sliced card either:** it is prompt text. What the server does with it:
- AG-4 cand arm (card 3), 2,799 fresh calls: input median 13,686 tokens (p95 17,232), output median 1,792 (p95 3,613),
  call median 7.5 s; 2.87 prompt bytes per token. Prefix-cache hits cover 71.6% of input tokens; round-0 hits are
  mostly exactly 8,192 tokens (1,380 calls), repair rounds 12,288+ (`cached_tokens` in `calls/*.jsonl`).
- The card is ~33.5 KB (~11.7k tokens) of a ~37 KB round-0 prompt (~13k tokens) `[REVISED: GLA-15]`. Every round
  re-sends it (`crates/axon-intent/src/genloop.rs:893-904`).

**Where AG-4's card-3 first tries fail** (1,440 round-0 programs): 564 fail `axon check`; parse errors (E0000) 243 =
16.9% of all first tries (43% of check failures). Top E0000 messages: `Ident(_), expected LBrace` 64 (63 of them
`Ident("is")`, i.e. `x is Err`), `Type, expected expression` 20, unclosed interpolated string 19, `Star` 17,
`If, expected Eq` 11, `Colon` 11, chained comparison 6, integer literal too large 5. Reply shape: 1,079 replies start
with a code fence, 361 are bare source, 0 have prose before a fence.

**Card section use** (card 4 headings; a section is "used" when a final program calls a builtin named in it; 1,440
AG-4 card-3 final programs): file 99.8%, string 99.8%, collection 96.5%, conversion 99.5%, console/time/process 99.8%,
math 18.7%; json, testing, network/database/AI, goals, uncertainty, concurrency/capabilities/storage, systems: **0**.
Those seven sections are 7,675 bytes = 22.8% of the card. Section sizes (bytes incl. heading; sum 33,593): header 269,
§1 syntax 3,698, habits 1,599, §2 rules 1,500, §3 heading 165, file 1,103, string 4,572, collection 6,453,
conversion 2,362, math 3,850, json 964, console 347, testing 191, network 929, goals 1,132, uncertainty 1,346,
concurrency 2,314, systems 799 `[REVISED: GLA-14: five sizes were 2-4 bytes low]`. Every builtin is in global scope
whatever the card shows (card line 4).

**Grammar source:** `spec/grammar.ebnf` is 27,712 bytes, ISO-style EBNF with implicit whitespace ("skipped between every
token"), 68 rules, ~4.3 KB once comments and runs of spaces are removed. It is documentation, not generated from
`crates/axon-core/src/parser.rs` (5,580 lines, hand-written, newline-sensitive: `preceded_by_newline` :520,
`paren_depth` :314 `[REVISED: GLA-18]`). It cannot be sent as is.

## 1. Priority tiers

| item | tier | what | cost |
|---|---|---|---|
| LV-0 | P0 | ae.py lever arms (same binaries, one option differs) | CPU |
| LV-1 | P1 | `axon grammar`: xgrammar-dialect grammar ≤ 16 KiB + conformance tests | CPU |
| LV-2 | P1 | http driver sends the grammar on request | CPU |
| LV-3 | P1 | `axon-intent --card-sections auto`: per-task sliced card | CPU |
| LV-4 | P1 | Run: card N vs card N + grammar | $0, CERE ~8.5-10 h `[REVISED: GLD-1, was ~4.5-9 h]` |
| LV-5 | P2 | Run: card N vs card N sliced | $0, CERE ~7-8 h `[REVISED: GLD-1]` |

Repair digest: LV-1 touches `axon-core/src` (new `grammar.rs`, `main.rs` subcommand) -> PCI note regen; no
`axon-cortex` file is touched, so the cortex receipt digest is unchanged (`scorecard.py:89-99`).

## 2. Items

### LV-0 (P0) Lever arms in `ae.py`

Today an A/B differs only by binaries: `--base-bin`/`--cand-bin`, and `--expect-card-change` refuses byte-equal cards
(`ae.py:563-565`). LV-4/LV-5 compare the SAME binaries with one generation option changed. Equal-binary runs are
allowed today; `bin_pins`, the per-arm `card_sha256` and the `--confirm` pin check compare binaries only.

**Change:**
- `ae.py run --cand-lever ebnf|card-slice` (one lever per run in v1). The lever applies to arm `cand` only. `meta.json`
  records `"lever": <name>`; `history.jsonl`'s row carries it.
- With a lever, `--base-bin` and `--cand-bin` must be byte-equal (refuse otherwise: a lever run isolates the lever), and
  `--expect-card-change` is refused (the full card is equal by construction).
- `[REVISED: GLA-11]` Both binaries run as genviab arm `a` (`ae.py:617` sets `x.arm = "a"`); `driver_env` never sees
  base/cand. ae.py sets `x.ebnf` / `x.card_sections` on the per-binary namespace copy for `b == "cand"`
  (`ae.py:616-617`). `driver_env` (`genviab.py:428-450`) sets `GENVIAB_EBNF=1` from `a.ebnf` (and `HTTP_ENV`
  :423-425 gains `GENVIAB_EBNF`, so it is never inherited); `task_row` adds `--card-sections auto` to the
  `axon-intent compile` argv (`genviab.py:526`) from `a.card_sections`.
- `[REVISED: GLA-4]` `round0_prompt_sha` (`ae.py:125-147`), which runs its own `axon-intent compile ... --max-rounds 0`,
  takes the arm's lever argv, so a card-slice cand gets its own `prompt_set_sha256` (`ae.py:151`).
- `[REVISED: GLA-4]` A lever run is always `paired_mode: "resample"` with no share dir. Today equal prompts give
  `regrade` (`ae.py:570`), which turns on the share dir (`ae.py:607`) that hands one reply to both arms, so an `ebnf`
  run would grade the same unconstrained reply twice (`selftest.py:1562-1563` pins Δ = 0 for that case).
- `[REVISED: GLD-1, GLA-5]` Replay: the cache key includes the driver file's sha256 (`drivers/http-program.py:30-31`,
  :77-79, :108-109), and LV-2 edits the driver, so LV-4 replays nothing from AG-4/AG-5: both arms call CERE fresh.
  LV-5's base arm can replay LV-4's base arm (same binaries, driver and series), but only up to LV-4's first replay
  mismatch: `cache-disabled` stops writes as well as reads (:218-219, :294). Observed base hit rates: AG-4 20% (one
  mismatch), AG-5 43% so far. A CERE redeploy changes the series and also misses.

**Fail-first tests (`selftest.py`):** a lever run with different base/cand binaries is refused; `--expect-card-change`
with a lever is refused; with `--cand-lever ebnf` only the cand driver env has `GENVIAB_EBNF`; with `card-slice` only
the cand argv (task run AND `round0_prompt_sha`) has `--card-sections auto`; a card-slice run's cand
`prompt_set_sha256` differs from base's; an `ebnf` lever run's meta has `paired_mode` `resample` and no share dir;
meta and the history row record the lever.

**Acceptance:** `python3 -B selftest.py` passes; an existing (lever-less) run's argv/env are byte-identical to before.
No g01 rows: a g01 kill must be a cargo test panic with a marker; `selftest.py` cannot provide one `[REVISED: GLD
checked-correct list]`.

### LV-1 (P1) `axon grammar`: the parser's language as an xgrammar grammar

**Change:** `crates/axon-core/src/grammar.rs` holds `GRAMMAR: &str = include_str!("../grammar/axon.gbnf")` and a
production `fn check_grammar_size(&str) -> Result<(), String>` (refuses > 16,384 bytes with
`ATTACK: grammar over MAX_GRAMMAR_BYTES`) that the `axon grammar` CLI path calls before printing `[REVISED: GLD-8]`.
`axon grammar` prints the grammar (no args, exit 0), next to `Command::Card` (`main.rs:1431`). `axon grammar --sha256`
prints its sha256 (as `axon card --sha256`, `genviab.py:172-178`).

The grammar:
- xgrammar dialect, character level. `crates/axon-core/grammar/axon.gbnf` is exactly the text `axon grammar` prints:
  no comments (CERE receives it verbatim).
- `[REVISED: GLA-1, was "a binary operator never starts a line outside (/["]` Whitespace explicit and newline-aware
  exactly as `parser.rs`. Inside a depth-raising `(`/`[` (grouping/tuple, call args, index, array literal,
  `Ok(`/`Err(`/`Some(`, the `ai_extract` turbofish: `parser.rs:2839`, :3160, :3183, :3197, :3504, :3589-3615; listed in
  `grammar.rs`'s doc) with no `{` in between, newlines are free. Every `{` block resets the depth to 0
  (`parse_block`, :1916-1940), and a struct literal's `{ ... }` never raises it. Outside parens (fn/block/struct-literal/
  match body) a newline ends the expression before any binary operator, before a call `(` and an index `[` (:3159,
  :3181, :3195), and after `return` (:2566). `.`, `?` and `as` may follow a newline (:3112-3117). A line may start with
  a unary `-`, `!`, `&`, `*` or a closure `|` as a new statement.
- `[REVISED: GLA-9]` Two whitespace classes: same-line space (spaces, tabs, a `/* ... */` without a newline) and line
  break (`\n`, `// ...\n`, a `/* ... */` containing a newline, which the lexer treats as a newline boundary:
  `lexer.rs:30-36`). Block comments as `"/*" ([^*] | "*" [^/])* "*/"` (a `*` takes its next character along, so
  `/**/` closes and `/***/`, `/* **/`, `/*/` are lex errors: `lexer.rs:382-389`; not the usual C rule) or no block
  comments at all; the build picks one, records which in `grammar.rs`'s doc, and the soundness test reaches it.
- `root ::= fence-open program fence-close | program`, fence `` ```axon\n `` / `` ```\n `` .. `` \n``` ``, then an
  optional final newline. No prose: AG-4 had 0 prose-before-fence replies, and `strip_fences` (`synth.rs:74-87`) only
  strips a reply that starts with a fence.
- `[REVISED: GLA-2: removed `[x; n]` (a habit row, E0000), `struct` (not a keyword), attributes (the card forbids
  them); the card shows no generics or effect rows]` The card-taught subset. Items: `fn` (no generics or effect rows
  unless a card example needs them), `type` (record and sum forms `A {...} | B {...}`), `enum`. Statements and
  expressions: `let`, assignment, `if`/`else` expressions, `for ... in a..b`, `while`, `return`, `match` (incl. struct
  patterns), `?`, `&`/`&mut`, struct literals, tuples, closures `|a: T| e`, `as`, interpolated strings with `{expr}`,
  `{{`, `}}`. Excluded, each with its reason in `grammar.rs`'s module doc: attributes `@[...]`, modules/`use`, traits/
  `impl`, agents, goals, refinement `where`, uncertainty literals, generics and effect rows (unless kept above).
- `[REVISED: GLA-8]` Lexer- and parser-level rejections the grammar must encode: decimal literals ≤ 18 digits, hex
  ≤ 15 digits (no i64 overflow: 5 AG-4 first tries by message prefix, 7 by the auditor's count); comparison is non-associative (`a < b < c` is an error,
  `parser.rs:3009-3012`; 6 first tries); the nested-string-in-slot rule (`token.rs:291-309`, `opens_operand`); keywords
  are not identifiers (`let in = 1` is E0000).
- Size ≤ 16,384 bytes (CERE `MAX_GRAMMAR_BYTES`). If the subset does not fit, shrink the subset, never raise the cap
  here (NHL-3).

**Authoring loop (no cargo) `[REVISED: GLD-9]`:** `scripts/grammar_xcheck.py --grammar <path> [--corpus <dir|files>]`
checks a draft grammar file against the completeness corpus through the served xgrammar (container below);
`cargo test ... --test grammar_conformance -- --ignored dump_samples` writes N soundness samples to a scratch dir, which
the prebuilt `axon parse` checks. Inner loop: draft -> `grammar_xcheck.py`; cargo only before a commit.

**Conformance tests** (`crates/axon-core/tests/grammar_conformance.rs`, `--no-default-features`):
1. A stdlib-only Earley recognizer for the xgrammar subset the file uses (test-only module, ~250 lines). Its own unit
   tests pin xgrammar semantics on the constructs used (char classes, `*`/`+`/`?`, alternation, nested rules).
2. **Completeness:** the card's 4 ```` ```axon ```` example blocks (`card.rs` `every_card_example_type_checks` :868,
   which asserts ≥ 4) `[REVISED: GLA-10, was 2 blocks at :862]` and every `examples/**/*.ax` inside the subset (list in
   the test; each excluded file names the construct that excludes it) are accepted.
3. **Soundness:** 2,000 seeded random derivations from the grammar (depth-bounded) all lex and parse (`axon_core`
   parse API; type errors are fine). Any rejection fails the test with the derivation and the parser message.
   `[REVISED: GLA-8]` Identifiers come from the grammar's own identifier rule plus a targeted set: every keyword, every
   keyword prefix, every keyword with a suffix. The test asserts coverage counts > 0 for: a newline before a binary
   operator inside parens, a newline-led `.`/`?`, a unary-led line, a `{` inside `(` followed by a newline, the longest
   decimal and hex literal, a block comment containing a `*` (when block comments are kept), a comparison, an
   interpolated string with `{{`.
4. **Size:** `check_grammar_size(GRAMMAR)` is Ok and refuses a 16,385-byte fixture.
5. **Strictness record:** how many in-subset `examples/` lines the grammar rejects when reformatted operator-first,
   inside parens and inside `({...})` `[REVISED: GLA-1]` (reported, not asserted).

**Cross-check against the served xgrammar (each grammar change, not a gate stage) `[REVISED: GLD-7]`:** the
completeness corpus plus 200 soundness samples through `xgrammar.testing._is_grammar_accept_string`, in a throwaway
container of the served image, never in `prod-cere-t4` itself: `docker run --rm --network none --entrypoint python3
-v <dir>:/g:ro lovedheart/qwen38-flash-next@sha256:6af86bea9baebe6c15fe91cb23cc7d33c362943d11c01daaf65a12b8218a8c69
/g/check.py` (no `--gpus`; `python3` resolves to `/opt/sglang/bin/python3`, the interpreter CERE's sidecar uses; the
image is local, 63.6 GB, no pull). Results (counts, xgrammar version) go in §6. Any disagreement with the Earley
recognizer is a bug in one of them; fix before LV-4.

**Private corpus (acceptance only, counts reported, no text):** every round whose program passed `axon check` in AG-4
and AG-5 (both arms), fences stripped, must be accepted. `scripts/grammar_xcheck.py --private <runs...>` prints
`accepted/total`; target 100%. Every rejection is a completeness bug; fix the grammar, never add the program anywhere
public.

**Fail-first tests:** the conformance test file exists before `axon.gbnf`; with an empty grammar, 2 fails; with a
16,385-byte fixture, 4's refusal assertion fails until `check_grammar_size` exists; `axon grammar` absent -> the CLI
test fails.

**g01 rows (M5300-M5319 reserved):** M5300 `axon grammar` prints nothing -> CLI test killed; M5301 a recognizer bug
(`?` treated as `*`) -> recognizer unit test killed; M5302 grammar drops the `{{` escape -> completeness killed; M5303
`[REVISED: GLA-1]` grammar lets a binary `+` continue an expression after a newline outside parens or inside a `{`
within parens -> soundness killed (the coverage assertion guarantees the sampler reaches it); M5304
`[REVISED: GLD-8]` `check_grammar_size`'s bound mutated to `usize::MAX` -> test 4 killed by its
`ATTACK: grammar over MAX_GRAMMAR_BYTES` marker. Each row names one (crate, target, test).

**Acceptance:** tests 1-4 pass; xgrammar cross-check 0 disagreements; private corpus 100% accepted; PCI note
regenerated at the lane's final commit; lane gate rc 0.

### LV-2 (P1) The http driver sends the grammar

**Change (`crates/axon-intent/benchmarks/genviab/drivers/http-program.py`):**
- `GENVIAB_EBNF=1`: run `$AXON_BIN grammar` once per process (the driver gets `AXON_BIN` through `gv.intent_env`,
  `gv.py:301-305`), add `"ebnf": <text>` to the request body (:253-254).
- The grammar's sha256 joins the cache key (`cache_key`, :108-109; `GENVIAB_SHARE_DIR` uses the same key, :229-231)
  and the row (`grammar_sha256`; `null` when off). Without this a cached unconstrained reply would replay into a
  constrained arm.
- `[REVISED: GLA-7]` A 400 from CERE is already an infrastructure row (`HTTPError` -> `session_errors` -> "request
  failed", :168-169, :265-266; `genviab.py:460-461`); the run's 5% infra rule then voids a run whose grammar CERE
  refuses. A regression test pins it for `ebnf`.
- `[REVISED: GLA-6]` In a lever run (both arms), `finish_reason` `repetition` is a scored model failure: today any reason
  other than stop/length is "finish_reason ..." exit 1 (:310-311), outside `MODEL_FAILURES` (`genviab.py:456-461`),
  so infra, dropped from the headline (`ae.py:32`, :75, :286). Left as is, loops the grammar causes would vanish from
  the constrained arm and bias Δ upward. Lever-less runs keep today's rule (trend comparability). AG-4 had 3
  `repetition` and 4 `length`. LV-4 reports length and repetition rates per arm.

**Fail-first tests (`selftest.py`, fake server) `[REVISED: GLD-2]`:** the selftest's `AXON_BIN` for these cases is a
fake script that prints a fixed grammar (so lane A passes without LV-1's `axon grammar`); with `GENVIAB_EBNF=1` the body
has `ebnf` equal to that output and the row has its sha; without it neither; the cache key differs between the two; in
a lever run a `repetition` finish is a model failure; a 400 on `ebnf` is an infra row (regression test).

**Acceptance:** selftest passes; after integration (real `axon grammar`), one live request through the driver with a
trivial intent returns `finish_reason` stop and a program the grammar accepts (recorded in §6, $0).

### LV-3 (P1) Card sliced per task: `axon-intent compile --card-sections auto|full`

**Change (`crates/axon-intent/src/genloop.rs`, `cli.rs:202`):**
- Default `full` (today's behaviour, byte-identical prompts). `auto` slices the card text `load_card` returned
  (`genloop.rs:601-620` `[REVISED: GLA-18]`) at its `### ` headings under `## 3. Builtins`; everything before `## 3.`
  (header, §1 incl. `### Habits from other languages`, §2) and the `## 3.` heading line always stay. Kept sections keep
  the card's order, so tasks with the same section set share one prompt prefix (CERE's prefix cache, §0).
- `[REVISED: GLA-17]` `SLICE_RULES = 1` keys on the card's exact `### ` heading text, and a test asserts every `### `
  heading under `## 3.` has a rule (a new card section fails the test until it gets one). Headings: `file`, `string`,
  `collection`, `conversion`, `math`, `json`, `console, time and process`, `testing`, `network, database and AI`,
  `goals`, ``uncertainty, probability and time-validity (`E`, `Var`, `P`: refinement predicates only)``,
  `concurrency, capabilities, supervision and storage`, `systems and bare metal`.
- Selection is deterministic from the intent only:
  - always: file, string, collection, conversion, math, console/time/process (math is kept: used by 18.7%, and a missing
    builtin costs more than 3.8 KB `[INFERENCE]`);
  - network/database/AI: the intent's `## Allowed` `net` list is non-empty (`intent.rs` `parse_allowed`: `net: none`
    gives an empty list), or the text says http, https, url, database, sql or llm;
  - json: the text says json;
  - testing: the text says `@[test]` or "unit test";
  - goals: `@[goal]`, optimise/optimize, maximise/maximize, minimise/minimize;
  - uncertainty: probability, uncertain;
  - concurrency/capabilities/storage: concurrent, parallel, thread, spawn, channel, supervise;
  - systems: mmio, interrupt, bare metal.
  `[REVISED: GLA-19]` Case-insensitive, matched at word starts with `_` as a word boundary (so `from_json` and
  `json_get_str` match json). `[REVISED: GLA-13]` The draft's words `store`, `storage`, `register`, `goal`, `confidence`,
  `distribution`, `model call` matched common English in the frozen set (counts only: concurrency 15, goals 4, systems
  3, uncertainty 3); they were dropped at Step 1. With the rules above, 236 of 240 frozen tasks get exactly the always-set
  (network 1, json 1, systems 1, goals 1; counts only). This tightening is the only change made after looking at
  frozen-set intents; after LV-3 merges the rules change only to fix a bug, never after looking at frozen-set outcomes.
- One index line replaces the omitted sections: `Other builtin sections (not shown): <names>.` so the model knows they
  exist. LV-5 reports how many cand programs call a builtin of an omitted section (the line's cost, if any).
- `[REVISED: GLA-3: the draft's E0001 trigger could never fire: every builtin is in global scope, so a real builtin
  used without its section is not E0001]` **Safety net:** the next round's card adds an omitted section (and keeps it
  for the remaining rounds) when the round's check has (a) an E0001 whose name or `did you mean` suggestion is a builtin
  of that section, or (b) an error on a line that calls a builtin of that section. genloop keeps the raw `axon-diag/1`
  lines (`genloop.rs:502`, :646-682), so message, help and line are available.
- `[REVISED: GLA-12]` Each round records `card_sections` (the kept list) and `card_sha256` of the text actually sent;
  the transcript-level `card_sha256` (`genloop.rs:216`, :336; copied to the row at `genviab.py:540`) stays the full
  card's.

**Fail-first tests (`crates/axon-intent`, unit):** `full` returns the card byte-identically; `auto` on an intent with
`net: none` and no keywords keeps exactly the always-set and drops the seven; `net: api.example.com` keeps network;
a kept section is byte-identical to the full card's; the safety net adds `json` after the real diagnostic for
`json_prase("1")` (E0001, help "did you mean `json_parse`?") and after an E0102 on a line calling `json_parse`; kept
order equals card order; every `### ` heading under `## 3.` has a rule; per-round `card_sha256` changes when the net
fires.

**Offline check before LV-5 ($0):** `ae.py` dry listing of the frozen set's section sets under `SLICE_RULES = 1`:
counts per set, the share of tasks with exactly the always-set, and mean card bytes, recorded in `PREREG-ae.md` before
the run (counts only, no task text).

**g01 rows:** M5305 `auto` keeps every section -> "drops the seven" killed; M5306 the safety net never fires -> the
`json_prase` test killed; M5307 `full` emits the index line -> byte-identity killed; M5308 kept order reversed -> order
test killed.

**Acceptance:** unit tests pass; `--card-sections full` prompts on a sample of 5 frozen tasks are byte-identical to the
current binary's (prompt shas equal); lane gate rc 0.

### LV-4 (P1) Run: card N vs card N + grammar

`ae.py run --advisory --label gl-ebnf --cand-lever ebnf --seeds 0,1,2,3,4,5 --set-dir <private>/ae-1/frozen
--base-bin B --cand-bin B` (B = the merge commit's binaries). PREREG entry appended before the run: binaries, grammar
sha, lever, prediction, headline (paired M1 FT-pass Δ, family-clustered `SignFlip`, two-sided, k = 6).

Reported besides the headline: E0000 first tries per arm (expected ~0 on cand), E0001/E0102 per arm (a parse fix can
move failures there), `finish_reason` length/repetition rate per arm, mean output tokens and call time per arm,
Solve@3, CCP soundness (M2s) per arm.

**Prediction `[INFERENCE]`, to be fixed in PREREG before the run:** FT-pass up by 3-8 points: E0000 was 16.9% of first
tries, but a program forced to parse can still fail on names, types or output (about 67% of check-passing card-3
programs passed FT, so the ceiling is near 11 points). A drop is possible if forcing a form the model did not want
degrades the rest of the program; the length/repetition rate is the early sign. `[REVISED: GLD-10]` Power at k = 6 is
0.57 at 3 points, 0.81 at 4, 0.94 at 5 (`PREREG-ae.md:136-139`), simulated at base FT-pass 0.21; at today's ~0.41 it is
likely lower `[INFERENCE]`. The low end of the band may come out NO CHANGE.

**Time `[REVISED: GLD-1]`:** all fresh (no replay, LV-0): about 5,800-6,500 calls at the observed 635-708 fresh calls/h
(AG-4, AG-5) = ~8.5-10 h, before any extra decode time from the grammar. Starts by ~15:30 to finish before 01:30, or
runs under a human `--ignore-blackout`.

### LV-5 (P2) Run: card N vs card N sliced

As LV-4 with `--cand-lever card-slice`, label `gl-slice`. Reported: headline, E0001 per arm, safety-net fire count,
programs calling an omitted-section builtin, mean prompt tokens and `cached_tokens` share per arm, call time.

**Prediction `[INFERENCE]`:** |Δ| < 2 points (NO CHANGE): on this set 236/240 tasks get the same 7.7 KB cut (§2 LV-3),
~2.7k fewer input tokens per call. `[REVISED: GLD-10]` A NO CHANGE result does not show |Δ| < 2; it shows only that no
change of ≥ ~4-5 points occurred (power figures as LV-4). The run is still worth its $0: a drop would show the full
card's unused sections carry rules the model uses.

**Time `[REVISED: GLD-1]`:** base arm partly replays LV-4's base (20-43% observed hit rates): ~4,600-5,200 fresh calls,
~7-8 h.

## 3. Sequencing

1. AG-5 finishes and its result is recorded (it uses CERE; one CERE run at a time).
2. `[REVISED: GLD-2, GLD-3]` Two lanes, two worktrees, two target dirs:
   - lane A: LV-0, LV-2 (selftest with a fake `AXON_BIN` grammar), LV-3 (`axon-intent`, Python harness);
   - lane B: LV-1 (`axon-core`, PCI note).
   Shared files: `scripts/v022_g01_mutations.py`, `scripts/v022_attack_markers.py`, `CHANGELOG.md`. Lane B owns them;
   lane A hands its M5305-M5308 rows and markers to the integrator as a patch, applied on the integration branch; one
   CHANGELOG Batch entry is written at integration.
3. LV-1's xgrammar cross-check and private-corpus check; after integration, LV-2's live single request against the real
   `axon grammar`.
4. Strict gate on the integration merge, implementation audit, merge, push.
5. LV-4 (~8.5-10 h, no replay), then LV-5 (~7-8 h) `[REVISED: GLD-1]`; one CERE run at a time; the 01:30-08:00 CERE
   window applies unless the human says `--ignore-blackout`.

## 4. Gates and cost budget

| gate | when | cost | resource |
|---|---|---|---|
| `scripts/grammar_xcheck.py --grammar <draft>` | each grammar draft | seconds per corpus once Python starts [INFERENCE] | CPU (container) |
| `cargo test -p axon-core --no-default-features --test grammar_conformance` | before each LV-1 commit | 2-5 min (lib rebuild through `include_str!` + debug-mode Earley over examples) [INFERENCE]; record the first measured time in §6 `[REVISED: GLD-5]` | CPU |
| `cargo test -p axon-intent --lib` / `--test genloop` | per LV-3 commit | ~1 min (S20 ag5-lane2: compile 4.6 s, tests ~35 s; the crate has no default features) `[REVISED: GLD-5]` | CPU |
| `python3 -B selftest.py` (genviab) | per LV-0/LV-2 commit | ~70 s incl. the axon/axon-intent build (S23: 68.6 s ag5-lane2, 69.6 s ag5-strict, 67.8 s ag-strict3) `[REVISED: GLD-4]` | CPU |
| PCI note (`tasks/r51-pci-regen.py`, `pci_delta.py --check`) | lane B's final commit, again after any rebase/merge (the first AG-5 lane failed S06 on this) `[REVISED: GLD-6]` | < 1 min | CPU |
| `gate.sh --lane` | each lane | lane B ~46 min (ag5-lane2 wall 46:12); lane A expected well below [INFERENCE: check with `gate.sh --lane --plan`] `[REVISED: GLD-6]` | CPU |
| g01 `--only` M5300-M5308 | lane done | axon-core rows rebuild the lib per mutation, ~1-3 min/row [INFERENCE]; record the measured time in §6 `[REVISED: GLD-11]` | CPU |
| `gate.sh --strict` | integration | 45-51 min (ag-strict3 44:59, ag5-strict 50:54) | CPU |
| LV-2 live request | after integration | 1 call, $0 | CERE |
| LV-4 | after merge | ~8.5-10 h, $0 | CERE GPU1 |
| LV-5 | after LV-4 | ~7-8 h, $0 | CERE GPU1 |

The new `axon-core` test target needs no `gate.sh` registration: `gate_lane.py:888-890` selects a target unseen by the
audit, and S16 is split per target; `axon-intent` tests run through S20. A run above 2× budget is reviewed before it
repeats.

## 5. g01 block

Reserved M5300-M5319. Used M5300-M5308. Each row names exactly one (crate, target, test).

## 6. Verification log

- §0 numbers: computed 2026-11-25 in the integrator's kernel from `20261125-080201-ag-card2` (`cand/s*/transcripts`,
  `cand/s*/calls`) and `axon card` of the card-4 binary (`.doc-build/ag5-cand/debug/axon`, 33,593 bytes). `AuditGL`
  recomputed them; five section sizes were corrected (GLA-14), everything else matched.
- CERE: capabilities json read; `constraint.rs:13`, :40-41; `validate.rs:27`; `pip show xgrammar` in `prod-cere-t4`
  -> 0.2.1; offline accept probe True/False; one live `ebnf` request -> stop, reply in grammar.
- Step 1 probes (`AuditGL`, card-4 binary `axon check`): `let y = f({\n 1\n + 2\n })` -> E0000; `P { x: 1\n + 2 }` ->
  E0000; `(1\n + 2)` ok; `let y = 1\n -2` ok (two statements); `"a"\n .len()` parses; `g()\n ?` ok; `[0; 3]` E0000;
  `struct P {..}` E0000; `let in = 1` E0000; `/* **/` E0000; `json_parse("1")` no error; `json_prase("1")` E0001 with
  "did you mean `json_parse`?".
- Frozen-set keyword counts (GLA-13 and the revised rules): counts only, from the 240 `intent.md` files.

## 7. needs-human (excluded from the build)

| Q | Question | Recommendation |
|---|---|---|
| NHL-1 | Send the grammar from the product generator (`axon-intent --generator ai:`, `axon-ai`) when the server supports it? | Decide after LV-4. Only if LV-4 is IMPROVED with no rise in length/repetition |
| NHL-2 | Make `--card-sections auto` the default? | Decide after LV-5. Default stays `full` unless LV-5 shows no drop and omitted-section builtins are rare in cand programs |
| NHL-3 | If the card-taught subset cannot fit in 16 KiB: raise CERE's `MAX_GRAMMAR_BYTES` (CERE repo, its own spec) or keep the smaller subset? | Keep the smaller subset and list what it excludes: under the grammar the model cannot write an excluded construct at all, so the list is the price |
| NHL-4 | Authorize the build (LV-0..LV-3) and the two $0 CERE runs (LV-4, LV-5)? | `[DECIDED by human 2026-11-26: "go" - build now; runs after AG-5's result is recorded, one CERE run at a time]` |

Step 2 classification: GLA-1..GLA-19 and GLD-1..GLD-11 are engineering decisions, now canon above. NHL-1..NHL-4 are
product/run decisions and stay needs-human; NHL-4 gates the build.

## 8. Build plan (build-loop, 2026-11-26)

**Baseline:** `main` `880ff337`. Last full gate: strict at `b2d22ab3` (8,329 passed, 0 failed, 17 ignored, 50:54). Since
then only `ae.py`/`selftest.py`/PREREG/spec files changed (genviab selftest rc 0 at `6222c16d`), so that result is the
baseline; known-failure list: empty.

**Plan options** (active: Fastest at 90 %, the default; not chosen by the user):
- Fastest: two lanes in parallel, each runs `gate.sh --lane` + its g01 `--only` rows; one strict gate on the
  integration merge. Build ~4-6 h to merge [INFERENCE]; ~1 in 10 chance a problem slips to the strict gate or audit,
  costing ~1 h. In words: done by breakfast; out of 10 builds like this, about 1 lets a mistake through to the last
  check, and fixing it takes about an hour.
- Most reliable: strict gate per lane and again on the merge: +~1.5 h. In words: done by mid-morning; out of 100
  builds, maybe 1 lets a mistake through.

**DAG** (critical path: LV-1 -> LV-2 live check -> LV-4; extra proof for LV-1: the xgrammar cross-check and the
private-corpus acceptance, both 0 disagreements / 100%):

| node | lane | depends on | files |
|---|---|---|---|
| LV-0 | A | - | `genviab/ae.py`, `genviab/genviab.py`, `genviab/selftest.py` |
| LV-2 | A | LV-0 (env plumbing); live check after integration needs LV-1 | `genviab/drivers/http-program.py`, `selftest.py` |
| LV-3 | A | - | `crates/axon-intent/src/{genloop.rs,cli.rs}` (+ its tests) |
| LV-1 | B | - | `crates/axon-core/{grammar/axon.gbnf,src/grammar.rs,src/lib.rs,src/main.rs,tests/grammar_conformance.rs}`, `scripts/grammar_xcheck.py` |
| integrate | main agent | LV-0..LV-3 | g01 rows/markers, `CHANGELOG.md` Batch 11, PCI note, this spec |
| LV-4 | CERE | integrate, AG-5 done | runs only |
| LV-5 | CERE | LV-4 | runs only |

Smoke (after integration, temp dir): `axon grammar | wc -c` ≤ 16,384 and `--sha256` stable; a capturing-stub generator
run of `axon-intent compile` on 5 frozen tasks with `--card-sections full` (prompts byte-equal to base) and `auto`
(only the expected sections dropped); one live `GENVIAB_EBNF=1` request (LV-2 acceptance).
