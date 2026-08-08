# R42 — The stdlib gaps measurement found: UTF-8, JSON arrays, dates, filesystem, patterns

**Spec ID:** `R42-stdlib-gaps`
**Status:** Draft — Slice 1 is a SOUNDNESS FIX and is not gated on the rest; Slices 2–6 are additive
**Risk class:** Mixed. Slice 1 is a behaviour change to a shipped builtin (`str_slice`). Slice 4 extends
the `Host` trait, which every host impl must then satisfy. The remainder is additive surface.
**Author / date:** 2026-08-08, from the `tasks_hard` measurement run recorded in
`atlas:spikes/rlm-engine/measurements/`

```spec-meta
id: R42-stdlib-gaps
status-claim: Draft
depends-on: R6-capability-security, R7b-axonhost, R1d-single-source-builtins
blocks: none
blocked-by: none
supersedes: none
related: R41-polyglot-runtime, R21-decimal
reserves: E2200-E2212, confirmed free at spec time (grepped `E22[0-9]{2}` across crates/, governance/
  and spec/ — zero hits; the reserved bands are E1800-E1803, E1810, E1900, E2000-E2003, E2100/E2104,
  E2300-E2302, E3700-E3707, so E22xx is the next contiguous free band)
evidence: measured — tasks_hard 16-task set, three runs per card arm; per-task failure causes read from
  the composed cells, not inferred from the score. Artifacts under atlas measurements/.
```

---

## 1. Why this spec exists, and why it is not "port the Python stdlib"

`ROADMAP.md` §2.6 takes an explicit position:

> If the stdlib is `Vec / HashMap / Iterator`, we shipped Rust. The Axon stdlib must be
> `Goal / Constraint / Budget / Principal / Agent / World / Belief / Plan / Trace / Feedback /
> Reward / Schedule / Tool / Audit`. The stdlib is what cements the paradigm.

That position is right and this spec does not contest it. It was, however, taken **before there was
any measurement of what the conventional gaps cost.** There is one now, and it is specific: a 16-task
set (`tasks_hard`, never used to tune anything) run against Axon three times per configuration, with
the cause of each failure read out of the composed cell rather than guessed from the score.

So the admission test this spec applies to itself — every item below is justified against it, and two
items are deliberately routed to userland because they fail it:

> **An addition is admissible only if (a) a measured failure traces to its absence, AND (b) it either
> cannot be written in userland Axon at acceptable cost, or it is a soundness fix. Anything that can
> be a `.ax` module in `examples/stdlib/` MUST be, because a builtin is permanent TCB surface and a
> `.ax` module is not.**

Applying that test moves dates, sets and paths OUT of the compiler and into userland, and it is the
reason this spec adds fewer builtins than the gap list would suggest.

### 1.1 What measurement actually said

Two things worth stating because they contradict the intuitive gap list:

- **Absence is often survivable.** The `re` task and the `datetime` task both PASSED — the model
  hand-rolled a digit-run scanner and hand-rolled leap-year arithmetic. Neither is evidence the gap
  is harmless; the datetime pass in particular is *luck*, and a task needing month lengths or a
  weekday would not have survived. But it does mean "no regex engine" is not automatically a lost
  task, and this spec ranks by measured cost rather than by how large the gap feels.
- **The costly gaps were not the obvious ones.** The single largest cause of failure in the run was
  not a missing module at all — it was a session defect (a binding named `len` poisoning every later
  cell, fixed separately). The largest *stdlib* cause was JSON array extraction, which is four
  functions of work.

---

## 2. Slice 1 — `str_slice` silently returns `""` across a UTF-8 boundary ⚠ SOUNDNESS

**This is the one item in this spec that is a bug, not a gap, and it should land first and alone.**

Verified in both engines:

```axon
let s = "café"            // 5 BYTES, 4 characters
str_len(s)                // 5   — bytes, not characters
str_slice(s, 0, 5)        // "café"  (byte-aligned, correct)
str_slice(s, 3, 5)        // "é"     (byte-aligned, correct)
str_slice(s, 0, 4)        // ""      ← splits `é`. SILENTLY EMPTY.
char_at(s, 3)             // 195     — the first byte of `é`, 0xC3
```

Interp and native agree (`5, 0, 5, 2`), so this is a uniform bug rather than an I-2 divergence — which
also means a one-sided fix WILL be caught by the parity harness, and both must change together.

**Why it is severe out of proportion to its size:** the language card teaches, as the correct way to
compare one character,

```axon
if str_eq(str_slice(s, i, i + 1), " ") { n = n + 1 }
```

For any non-ASCII input that expression silently evaluates `str_eq("", " ")`. The program does not
crash, does not warn, and produces a confidently wrong answer. This is the failure mode the repo
refuses everywhere else (`checked_arith_parity`, the `E0910` refusals, the `Chan` deep-clone refusal),
and it is currently shipped in the one place a model is most likely to hit it.

**Resolution — refuse, do not round.** A byte range that splits a codepoint raises a runtime error at
both sites, sharing the panic-class exit path:

| code | condition |
|---|---|
| `E2200` | `str_slice(s, lo, hi)` where `lo` or `hi` is not a UTF-8 character boundary of `s` |

Rounding to the nearest boundary was considered and rejected: it replaces one silent wrong answer
with another, and a caller that wanted byte semantics cannot tell it happened. Returning `Err` was
considered and rejected because `str_slice` returns `str` today and widening it to `Result<str, str>`
breaks every existing caller for a case that is always a bug.

**Why the existing gates missed it — worth recording, because the reason generalises.**
`str_slice` IS in the differential fuzz corpus, and the corpus IS non-ASCII (`"straße"`,
`"café au lait"` among its inputs). Two independent reasons it still survived:

1. The corpus exercises exactly one index pair, `str_slice(A, 0, str_len(A) / 2)`, and for both
   non-ASCII inputs that midpoint happens to land on a character boundary (`"straße"` is 7 bytes →
   `slice(0,3)` = `"str"`; `"café au lait"` is 13 → `slice(0,6)` = `"café "`). One index pair per
   input cannot probe boundary behaviour.
2. **More fundamentally: `fuzz_parity.sh` compares the interpreter against native codegen, so it
   cannot find a bug the two SHARE.** Both return `""` here, identically, so the harness is green and
   correct to be green — it is an agreement oracle, not a correctness oracle. Every bug in the
   interpreter's own semantics is invisible to it by construction, because the interpreter IS the
   reference oracle (I-2).

That second point is not specific to this slice: the parity harnesses cannot be the only gate on
anything where the interpreter itself might be wrong. It argues for expected-value assertions
alongside agreement assertions, which is a broader gap than this spec closes (§9 Q6).

**Gate:** `utf8_boundary_parity.sh` — asserts EXPECTED VALUES (not merely agreement) for a corpus of
boundary-splitting and boundary-aligned slices, including the card's `str_slice(s, i, i + 1)` loop
over `"café"`, plus interp/native agreement on the new refusal and its exit code.

## 3. Slice 2 — character access that is not byte access

With Slice 1 in place, splitting a character is loud rather than silent. That makes per-character work
*correct*, but not yet *possible*: there is still no way to iterate characters. Today's surface is
byte-indexed throughout (`str_len` → bytes, `char_at` → a byte value as `i64`, `str_slice` → byte
offsets), and Axon has no character type.

**Design: do not add a `Char` type, and do not change `char_at`.** A one-character `str` is already
Axon's character representation everywhere it appears in real code and in the card. Adding `Char`
would fork every string function; changing `char_at`'s return type would break existing programs and
the card's byte-comparison idiom, which is *correct* and worth keeping for ASCII fast paths.

Instead, add a character-indexed layer beside the byte-indexed one, named so the distinction is
impossible to miss:

```
str_chars(s: str) -> [str]                  // one one-character str per CHARACTER
str_len_chars(s: str) -> i64                // character count (str_len stays bytes)
str_char_at(s: str, i: i64) -> str          // the i-th CHARACTER, not the i-th byte
str_char_slice(s: str, lo: i64, hi: i64) -> str   // character-indexed str_slice
char_code(c: str) -> Result<i64, str>       // codepoint of a one-character str
char_from_code(n: i64) -> Result<str, str>  // inverse; Err for a surrogate or > U+10FFFF
char_is_digit(c: str) -> bool
char_is_alpha(c: str) -> bool
char_is_space(c: str) -> bool
```

`str_chars` is the load-bearing one: it turns every per-character task into `arr_*` work over `[str]`,
which the model already writes correctly, and it composes with `arr_group_by` / `dict_inc` for the
tally and grouping tasks.

**"Character" means Unicode scalar value (codepoint), not grapheme cluster.** Stated explicitly
because the two differ (`e` + combining acute is one grapheme, two codepoints) and a spec that leaves
it implicit will get one of each. Grapheme segmentation needs a table and is deferred; §9 Q2 holds it.

**Codegen:** these are pure string functions with no host contact, so they lower natively via
`axon-rt-extern` delegation rather than inline IR. That is not a preference — inline-IR string
builtins have diverged from the interpreter on non-ASCII input four separate times
(`to_upper`/`to_lower`, `trim`, `pad`), always because the IR assumed one byte per character. These
functions are *about* the multi-byte case, so inline IR is prohibited here.

## 4. Slice 3 — JSON arrays and typed paths

**The measured failure.** The `json` task asks for the sum of the numbers in `"a"` plus the value at
`"b"."c"` from `{"a": [1, 2, 3], "b": {"c": 4}}`. The existing surface cannot express it:

```
json_parse(s: str) -> Result<str, str>              // validate only; returns its input
json_get_i64(json: str, key: str) -> Result<i64, str>   // TOP-LEVEL scalar only
json_get_str(json: str, key: str) -> Result<str, str>
json_path_str(json: str, path: str) -> Result<str, str> // dot path, STRING leaves only
json_stringify(s: str) -> str
```

There is no array accessor at all, and `json_path_str` cannot reach a numeric leaf. So the task is a
genuine capability hole, not a fluency problem. (An earlier note in this project's history said "JSON
was never the gap, the card just never mentioned it." That is half wrong and is corrected here:
scalars are covered, arrays are not.)

**Design: sub-documents as strings, not a JSON value type.** The existing five functions all take and
return `str`. The minimal addition consistent with that is an accessor returning the sub-document as
a JSON string, which then composes with every existing function:

```
json_get_json(json: str, key: str) -> Result<str, str>      // sub-object/array AS a json str
json_path_json(json: str, path: str) -> Result<str, str>    // same, by dot path
json_path_i64(json: str, path: str) -> Result<i64, str>     // the missing numeric leaf
json_path_f64(json: str, path: str) -> Result<f64, str>
json_len(json: str) -> Result<i64, str>                     // array length / object key count
json_at(json: str, i: i64) -> Result<str, str>              // i-th array element AS a json str
json_keys(json: str) -> Result<[str], str>                   // object keys
json_arr_i64(json: str) -> Result<[i64], str>               // whole array, typed
json_arr_f64(json: str) -> Result<[f64], str>
json_arr_str(json: str) -> Result<[str], str>
```

A `Json` value type would be the textbook answer and is deliberately not proposed: it introduces a
new type to the checker, infer, codegen and the session dump's `value_as_literal` (which would need a
literal form for it), for a surface that is already string-shaped and works. If a future slice wants
one, nothing here blocks it.

Codes: `E2201` malformed JSON where a document was required, `E2202` type mismatch at a path
(`json_arr_i64` on an array of strings).

## 5. Slice 4 — filesystem beyond a single known path ⚠ extends `Host`

Today: `read_file`, `write_file`, `append_file`, `file_size`. You can read and write a path you
already know and nothing else — no existence check, no directory listing, no removal.

```
file_exists(path: str) -> bool
dir_create(path: str) -> Result<(), str>        // creates parents, like mkdir -p
dir_list(path: str) -> Result<[str], str>       // names, not paths; no recursion
file_remove(path: str) -> Result<(), str>
file_copy(from: str, to: str) -> Result<(), str>
file_rename(from: str, to: str) -> Result<(), str>
```

**This is the only slice that touches the TCB, and it needs care in three places:**

1. **`Host` trait.** Every one of these needs a new trait method, and every impl must answer —
   including `BrowserHost`, which has no filesystem and must return `Err` rather than silently
   succeed. `append_file`/`file_size` avoided this by composing existing methods; these cannot.
2. **Capability classification (`capabilities.rs::classify_call`).** Getting this wrong is a sandbox
   escape, so each is stated rather than left to a default:

   | builtin | `IoKind` | why |
   |---|---|---|
   | `file_exists` | `FsRead` | probing existence is an information channel — it leaks whether a path exists outside the allowlist |
   | `dir_list` | `FsRead` | strictly more leakage than a read: it discloses names the caller did not know |
   | `dir_create` | `FsWrite` | |
   | `file_remove` | `FsWrite` | destructive; see §9 Q3 |
   | `file_copy` | `FsRead` **and** `FsWrite` | the ONLY builtin needing two kinds — source read, destination write. `classify_call` returns a single `IoKind` today, so this slice must widen it |
   | `file_rename` | `FsWrite` | both paths must be write-allowed |

   `file_copy` is the interesting one: it is a read/write bridge, and a checker that granted it on
   the strength of the write capability alone would let a `write`-only contained function exfiltrate
   file contents to a path it controls. `classify_call` returning `Option<IoKind>` cannot express
   that, so it becomes `&[IoKind]` as part of this slice — a small refactor with a real reason.
3. **Path traversal.** These take paths from the program, so they inherit the existing `..`-component
   refusal (`contained-path-traversal`, E1001). `dir_list` returning names that are then joined by a
   userland `path_join` (§7) is a new way to *construct* a traversing path, which is why §7 carries a
   matching constraint.

**Codegen:** interp-first, native `E0910`-refused, per the precedent set by `append_file`/`file_size`
and every other host-touching builtin. Also must be added to `wasm_parity.sh` `HOST_BUILTINS` — an
omission the `divergent_builtins_are_excluded_from_the_wasm_pure_corpus` test catches, and did.

## 6. Slice 5 — pattern matching, linear time only

The `re` task passed by hand-rolling, so this is not top-ranked by measured cost. It is specced
because hand-rolling does not generalise and because the *shape* of the answer is a security
decision, not just an API decision.

**Requirement: no backtracking, ever.** A regex engine is a denial-of-service surface (ReDoS): with
backtracking, `(a+)+$` against a modest input is exponential. Axon runs model-authored code under
capability sandboxes and per-principal budgets; an unbounded-time builtin would let sandboxed code
burn arbitrary CPU with no capability at all, defeating the containment story rather than a mere
performance goal. So the engine is a Thompson NFA simulation with an O(pattern × input) bound, and
constructs that cannot be simulated in linear time are **refused at construction** rather than
supported slowly:

```
re_is_match(pattern: str, s: str) -> Result<bool, str>
re_find(pattern: str, s: str) -> Result<Option<str>, str>      // leftmost-longest
re_find_all(pattern: str, s: str) -> Result<[str], str>
re_captures(pattern: str, s: str) -> Result<[str], str>        // group 0 first
re_replace_all(pattern: str, s: str, with: str) -> Result<str, str>
re_split(pattern: str, s: str) -> Result<[str], str>
```

Supported: literals, `.`, character classes and negation, `*` `+` `?`, bounded `{n,m}`, alternation,
groups, anchors, and the common escapes. **Refused with `E2203`:** backreferences and lookaround —
both require backtracking, and refusing them is the point rather than a limitation to apologise for.

`Result` on every function because a pattern is data and may be malformed; a panic on a bad pattern
would be the wrong call for something a model composes at runtime.

## 7. Slice 6 — encoding builtins; dates, sets and paths in USERLAND

Split by the §1 admission test. These four are grouped because the interesting content is which side
of the line each falls on.

**Builtins (pure, small, wrong to hand-roll):**

```
base64_encode(s: str) -> str        hex_encode(s: str) -> str
base64_decode(s: str) -> Result<str, str>   hex_decode(s: str) -> Result<str, str>
```

Hand-rolled base64 is a classic source of silent corruption on padding, and it is thirty lines of
Rust. Crypto (`sha256`, `hmac`) is **deliberately excluded** from this slice: R28's audit ledger and
R33's quorum work already need hashing and should own that surface, so adding a second one here would
guarantee two incompatible answers. §9 Q4.

**Userland `.ax` modules — these FAIL admission test (b), so they must not be builtins:**

- **`examples/stdlib/date.ax`.** Civil calendar arithmetic is pure integer math: Hinnant's
  `days_from_civil` / `civil_from_days` are a dozen lines each and need no host beyond the existing
  `now_ms()`. A `Date { y, m, d }` struct with `date_from_ms`, `date_to_days`, `date_add_days`,
  `date_weekday`, `date_is_leap`, `date_diff_days` and an ISO-8601 formatter covers the measured need
  (the `datetime` task) with zero TCB growth. **Timezones and DST are explicitly out** — they need a
  tzdata table and a host clock offset, and pretending otherwise produces wrong answers rather than
  missing ones. `now_ms()` is UTC; the module says so.
- **`examples/stdlib/set.ax`.** A `Set` over the existing `Dict` (unit values), with
  `set_new/add/has/remove/len/union/intersect/difference/to_arr`. Dicts already provide the hashing;
  a builtin would add a type for no capability the language lacks.
- **`examples/stdlib/path.ax`.** `path_join`, `path_basename`, `path_dirname`, `path_ext`,
  `path_normalize` as pure string functions. **Security constraint, not a nicety:** `path_normalize`
  must resolve `..` lexically and `path_join` must never *produce* a path containing a `..`
  component, because `@[contained]`'s fs allowlist refuses `..` paths statically (E1001) and a
  userland joiner that assembles one from `dir_list` output would move traversal past a check that
  already exists. The module carries a test asserting `path_join("./out", "../etc")` is refused
  rather than normalised into an escape.

---

## 8. Cross-cutting requirements

**I-2 (native agrees or refuses).** Pure functions (Slices 2, 3, 5, 6-builtins) lower natively via
`axon-rt-extern` delegation; NO inline IR for anything string-shaped (§3). Host-touching functions
(Slice 4) are interp-only with `E0910` refusal. No item may land with native computing a *different*
answer, and each slice adds rows to the differential fuzz corpus. Those rows must
include MULTIPLE index/argument choices per input, not one: the corpus already carried non-ASCII
strings and still missed §2 because it probed a single midpoint (§2). And per §2, agreement rows are
not sufficient on their own — anything where the interpreter's own semantics are in question needs an
expected-value assertion too.

**Effect rows.** Slice 4 is `{IO}`; everything else is pure and must NOT acquire an effect row, or
`@[pure]` functions lose the ability to parse JSON and split strings.

**Session dump.** No new value types are introduced, so `value_as_literal` needs no new arms —
deliberately (§4). Slice 6's userland `Set` is a `Dict` underneath and inherits the dict literal form
and its aliasing refusal for free.

**Reserved codes.** `E2200` UTF-8 boundary · `E2201` malformed JSON · `E2202` JSON path type mismatch
· `E2203` non-linear regex construct refused · `E2204`–`E2212` unallocated, held for these slices.

## 9. Open questions

- **Q1 — Does Slice 1 break real code?** `str_slice` silently returning `""` may have accreted
  callers that depend on it. Must be answered by grepping `examples/` and the test corpus for slices
  over non-ASCII data BEFORE the change lands, not after. Recommendation: fix regardless, since every
  such caller is already producing a wrong answer; but the blast radius must be *known*.
- **Q2 — Codepoints or graphemes?** Slice 2 commits to codepoints. Graphemes need a segmentation
  table (~large) and would make `str_chars("e" + combining-acute")` return one element instead of
  two. Recommendation: ship codepoints, revisit only with a measured failure that graphemes fix.
- **Q3 — Should `file_remove` need more than `FsWrite`?** Deleting is irreversible and R11's risk
  typing already distinguishes irreversibility. Recommendation: `FsWrite` for the capability check
  PLUS an `irreversible` flag feeding `risk_derive`, so a High-risk pipeline gates a program that
  deletes. Needs R11's owner to confirm rather than being decided here.
- **Q4 — Who owns hashing?** R28 (audit ledger) and R33 (quorum) both need `sha256`. This spec
  excludes it to avoid a second answer. Needs an explicit assignment.
- **Q5 — `classify_call` widening.** Slice 4 changes it from `Option<IoKind>` to a slice for
  `file_copy`. That touches a security-critical function used by `@[contained]`, the sandbox and the
  effect bridge. Recommendation: land the widening as its OWN commit with the existing capability
  tests unchanged and green, before any new builtin uses it — a refactor and a new capability in one
  commit is how a hole gets in.
- **Q6 — Should the parity harnesses gain expected-value rows generally?** §2 showed the
  differential fuzzer is structurally blind to bugs interp and native share, which is every bug in
  the reference semantics. Recommendation: not in this spec (scope), but it deserves its own, and
  this is the second time a shared-semantics bug has reached a shipped builtin.

## 10. Deliberately out of scope

Sockets/TCP/UDP · iterators and lazy sequences · a `Json` value type · CSV/YAML/TOML · a logging
framework · bignum integers · trigonometry (`sin`/`cos`/`tan`, `ln`, `log2` — absent, but no measured
failure traces to them) · timezones/DST · grapheme segmentation · crypto (§9 Q4).

**Do not build:** a general-purpose `Iterator` protocol, a `Char` primitive type, or a regex engine
with backtracking. The first two contradict ROADMAP §2.6; the third is a capability-escape surface
(§6).

## 11. Stop condition

```
DONE = Slice 1 landed, with interp+native byte-identical and the non-ASCII fuzz rows added
   AND every slice's builtins registered at ALL FOUR sites (BUILTINS, interp, capability
       classification where applicable, wasm_parity HOST_BUILTINS) — the registration checklist
       in CLAUDE.md, not a subset
   AND every new builtin either lowers natively or E0910-refuses; none diverges
   AND userland modules (date/set/path) ship as .ax with @[test]s, NOT as builtins
   AND cargo test --workspace shows no new failures against the 1808/0 baseline
   AND tasks_hard re-run: the json task passes, and the measurement is reported per-task rather
       than as a total, since a total cannot show whether THIS spec's items were the ones fixed
```

The last clause is the one that matters. Every previous round of this work produced a number that
moved for reasons other than the change being tested — a splitter bug read as a capability result, a
card edit read as a compiler improvement. Per-task attribution or the measurement does not count.
