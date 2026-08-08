# R42 — The stdlib gaps measurement found: UTF-8, JSON arrays, dates, filesystem, patterns

**Spec ID:** `R42-stdlib-gaps`
**Status:** Draft — Slice 1 is a SOUNDNESS FIX and is not gated on the rest; Slices 2–6 are additive
**Risk class:** Mixed. Slice 1 is a behaviour change to a shipped builtin (`str_slice`). Slice 4 extends
the `Host` trait, which every host impl must then satisfy. The remainder is additive surface.
**Author / date:** 2026-08-08, from the `tasks_hard` measurement run recorded in
`atlas:spikes/rlm-engine/measurements/`
**Review:** adversarial review folded in 2026-08-08. It corrected four factual claims (§4 array
indexing, §10 `ln`, §3 duplicate of `chr`, §5 the `Host` risk), replaced the regex matching semantics
(§6, leftmost-longest → leftmost-first / Pike VM), found the `&[IoKind]` capability fix insufficient
(§5, B1 — the one item that had to change before implementation), added the missed JSON-construction
gap (§4.1), and answered Q1. One reported issue did not reproduce and is recorded in §8.1.

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
  E2100-E2108 + W2110-W2112 (R16), E2300-E2302, E3700-E3707, so E22xx is the next contiguous
  free band)
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

Interp and native agree exactly — both are `s.get(start..end).unwrap_or("")`
(`crates/axon-core/src/interp/builtins.rs:2279` and `crates/axon-rt/src/lib.rs:2073`), which is
literally where the `""` comes from. So this is a uniform bug rather than an I-2 divergence, which
also means a one-sided fix WILL be caught by the parity harness and both must change together.

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

**The interim window, which must be planned for rather than discovered.** Between Slice 1 and
Slice 2 the card's own taught idiom (`str_slice(s, i, i + 1)`) converts from silently-wrong to a
**panic (exit 101)** on non-ASCII input, and there is no correct per-character alternative in the
language yet. Two consequences:

* **The language card MUST be updated in the same window as Slice 1**, not after Slice 2 — it
  currently teaches an idiom that will crash. The interim advice is the byte comparison
  (`char_at(s, i) == 32`), which stays correct because it never slices.
* **A `tasks_hard` re-run inside that window will score WORSE on non-ASCII string tasks than
  baseline** — a crash where there was a lucky wrong answer. That is the right trade (loud beats
  wrong) but it is a predicted dip, written down here so §11's per-task clause is not misread as a
  regression when it appears.

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
char_is_digit(c: str) -> bool
char_is_alpha(c: str) -> bool
char_is_space(c: str) -> bool
```

**`chr` already exists and is NOT duplicated here.** `chr(n: i64) -> str` (`builtins.rs:778`) is
already the codepoint-to-character direction, so this slice adds only the inverse (`char_code`). An
earlier draft proposed a `char_from_code` returning `Result`; that was a duplicate and is dropped.
`chr` PANICS on an invalid codepoint rather than returning `Err`, which is a live inconsistency with
`char_code`'s `Result` — resolved in favour of leaving `chr` alone (changing a shipped signature for
consistency is not worth a break) and documenting the asymmetry. Note also that `chr` is currently
**E0910-refused by native codegen**, so §3's "these lower natively" applies to the NEW functions;
bringing `chr` along is in scope for this slice since it is the same lowering work.

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

**Corrected during review — the gap is narrower than first written.** `json_path_str` DOES index
arrays: a numeric path component works, verified live, `json_path_str(j, "a.1")` on
`{{"a": ["x","y"]}}` returns `Ok("y")` (implementation at `interp/builtins.rs:2398-2420`, which has
explicit "array index out of bounds" / "array requires numeric index" errors). An earlier draft of
this section said "there is no array accessor at all"; that was wrong.

What is ACTUALLY missing, and what the measured failure traces to:
* **numeric leaves** — `json_path_str(j, "n.1")` on `{{"n": [1,2,3]}}` returns
  `Err("leaf is not a string (found other type)")`, verified. There is no `json_path_i64`.
* **array length** — nothing reports how many elements an array has, so it cannot be looped.
* **whole-array extraction** — no way to get `[1,2,3]` out as an Axon `[i64]`.

So summing `"a"` is impossible: you cannot learn the length, and each element is unreachable as a
number even though it is reachable as a path. That is a real capability hole, not fluency.

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

**Two things an earlier draft got wrong here.**

*E-codes on `Result`-returning builtins.* Every function in this slice returns `Result<_, str>`, so
its failures are **values, not diagnostics** — there is no diagnostic for an E-code to attach to. So
`E2201`/`E2202` are defined as structured PREFIXES inside the `Err` string (`"E2201: ..."`), which is
what makes them greppable in a trace and matchable by a caller, rather than pretending they are
compiler diagnostics. The same correction applies to `E2200` (§2): runtime panics carry no E-code
today (`chr`'s does not), so `E2200` is specified as appearing in the panic message text.

*Re-parsing cost.* `json_at` in a loop re-parses the whole document per call, so walking an n-element
array is O(n²). The typed extractors (`json_arr_i64`/`_f64`/`_str`) exist precisely to avoid that and
are the idiom the language card should teach; `json_at` is for heterogeneous arrays where no typed
extractor applies. Stated because a spec that ships both without ranking them invites the quadratic one.

### 4.1 JSON construction — the gap review found this spec had missed entirely

Slice 3 as first written is read-only. But an RLM engine's programs must also EMIT structured output,
and the only writer today is `json_stringify`, which escapes a single string value. So:

```
json_from_pairs(pairs: [(str, str)]) -> str    // object from pre-encoded JSON values
dict_to_json(d: Dict) -> Result<str, str>      // whole dict; Err on a value with no JSON form
json_arr_from_i64(xs: [i64]) -> str            // and _f64 / _str
```

**One claim from review that did NOT survive checking, recorded so it is not re-litigated:** the
review reported that a JSON literal is effectively unwritable in Axon source, because `{` opens
string interpolation. It is writable — doubled braces are the parser's escape
(`parser.rs:96-101`), the same mechanism the session dump relies on, and this runs today:

```axon
let doc = "{{\"a\": [1, 2, 3], \"b\": {{\"c\": 4}}}}"   // prints {"a": [1, 2, 3], "b": {"c": 4}}
```

The real problem is discoverability, not capability: `{{` is unobvious, undocumented in the language
card, and a model will write `"{\"a\": 1}"` and get a lexer error. That is a card fix, not a builtin,
and it belongs with the §2/§3 card update.

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

1. **`Host` trait — smaller than it looks, IF the methods are default-deny.** `append_file`/
   `file_size` avoided touching the trait by composing existing methods; these cannot. But the trait
   already has the right precedent: `exec` and the `http_*` family are **default methods returning
   `Err`** (`host.rs:36-84`), so a host that cannot do the thing inherits a fail-closed answer and
   needs no code. Every method in this slice MUST follow that pattern rather than being a required
   method. That reduces "every host impl must change" to "only `DefaultHost` implements them", which
   is in fact the only workspace impl of `AxonHost` outside test-local ones — the `--host browser`
   shim (`main.rs:2222`) selects a virtual host and is not a second `AxonHost` impl to update.
   Required (non-default) methods here would be a mistake in both directions: more work, and a host
   that forgets one fails to compile rather than failing closed.
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

   `file_copy` is the interesting one, and the fix is NOT what an earlier draft of this spec said.
   It is a read/write bridge: a checker granting it on the write capability alone would let a
   `write`-only contained function exfiltrate file contents to a path it controls. The earlier draft
   proposed widening `classify_call` from `Option<IoKind>` to `&[IoKind]`. **That is insufficient and
   would leave the hole open.** The static check extracts exactly ONE path — `args.first()`
   (`capabilities.rs:1132`) — and tests it against the one kind. Given a list of kinds it would check
   the SOURCE path against both read and write, and the DESTINATION path against nothing at all,
   which is precisely the exfiltration this paragraph claims to close.

   The requirement is a **per-argument** mapping — `&[(IoKind, arg_index)]` or a bespoke multi-path
   arm — so `file_copy(from, to)` checks arg 0 against `fs:read` and arg 1 against `fs:write`, and
   `file_rename(from, to)` checks BOTH args against `fs:write`. Any design that cannot say *which
   argument* a kind applies to cannot express either builtin safely. This is the one item in the spec
   that must be got right before the builtins land, not alongside them (§9 Q5).
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
performance goal. So the engine is a **Pike VM** — an NFA simulation carrying capture slots — with an
O(pattern × input) bound, and constructs that cannot be simulated in linear time are **refused at
construction** rather than supported slowly:

```
re_is_match(pattern: str, s: str) -> Result<bool, str>
re_find(pattern: str, s: str) -> Result<Option<str>, str>      // leftmost-FIRST (see below)
re_find_all(pattern: str, s: str) -> Result<[str], str>
re_captures(pattern: str, s: str) -> Result<[str], str>        // group 0 first
re_replace_all(pattern: str, s: str, with: str) -> Result<str, str>
re_split(pattern: str, s: str) -> Result<[str], str>
```

Supported: literals, `.`, character classes and negation, `*` `+` `?`, lazy `*?` `+?` `??`, bounded
`{n,m}`, alternation, groups, anchors, and the common escapes. **Refused with `E2203`:**
backreferences and lookaround — both require backtracking, and refusing them is the point rather
than a limitation to apologise for. This matches RE2 and Rust's `regex`, so it does not break
ordinary use.

**Leftmost-first, NOT leftmost-longest — corrected during review.** An earlier draft specified POSIX
leftmost-longest. That is the wrong semantics for this language's authorship model: models write
PCRE-shaped patterns and expect Perl semantics, where `re_find("a|ab", "ab")` is `"a"`, not `"ab"`.
Worse, under leftmost-longest the lazy quantifiers models write constantly (`.*?`) are meaningless.
Leftmost-first is available in linear time — it is what RE2 and Rust's `regex` do — but it requires a
Pike VM rather than a plain Thompson simulation, which is also what `re_captures` needs. Hence the
change above; plain Thompson could not have implemented the capture function this spec already listed.

**Two DoS holes inside the "linear" bound, now closed:**
* `{n,m}` **expands the NFA**, so `a{1,100000}` is a memory and time blow-up that is technically
  linear in the *expanded* pattern. The expanded program size is capped; beyond it, `E2203`. A bound
  advertised as linear in the pattern is worthless if the pattern can be expanded 100,000× by four
  characters of input.
* Pattern compilation is itself work, and these functions take the pattern per call. A pattern cache
  keyed by the pattern string is required, or `re_find_all` in a loop recompiles every iteration.

**Two semantics questions an earlier draft left open, now pinned:** `re_captures -> [str]` cannot
distinguish a group that did not participate from one that matched empty — so a non-participating
group is reported as the empty string and the doc says so (a `[Option<str>]` return was considered
and rejected: it complicates the common case to serve the rare one). And `re_replace_all`'s `with`
argument **does** interpret `$1`..`$9` as capture references, with `$$` for a literal `$`; leaving
that unspecified would have guaranteed two incompatible answers later.

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

**Unstated hole in an earlier draft, now specced: Axon has no bytes type.** `str` must be valid
UTF-8, so `base64_decode` CANNOT represent arbitrary binary — which is most of what base64 exists to
carry. The behaviour is therefore pinned rather than left to discover: a decode whose output is not
valid UTF-8 returns **`Err`** (`E2204`), never lossy replacement characters and never a truncated
prefix. These two functions round-trip TEXT and say so in their doc strings. Silent lossy decoding
would be a smaller sibling of the Slice-1 bug, in a primitive justified on the grounds that
hand-rolling it goes wrong quietly.

Hand-rolled base64 is a classic source of silent corruption on padding, and it is thirty lines of
Rust. Crypto (`sha256`, `hmac`) is **deliberately excluded** from this slice: R28's audit ledger and
R33's quorum work already need hashing and should own that surface, so adding a second one here would
guarantee two incompatible answers. §9 Q4.

**Userland `.ax` modules — these FAIL admission test (b), so they must not be builtins:**

- **`examples/stdlib/date.ax`. Verified feasible, not assumed:** both Hinnant algorithms were run as
  plain Axon before this spec was committed, and produce `366` for the tasks_hard case
  (2020-01-01 → 2021-01-01), round-trip a pre-epoch date (1969-07-20, exercising negative day
  numbers), give the correct weekday for 2026-08-08, and answer `is_leap(2000)=true` /
  `is_leap(1900)=false`. So the routing to userland is demonstrated rather than argued.
  **Caveat found in review:** Axon's `/` and `%` truncate toward zero (Rust semantics), while
  `civil_from_days` and `date_from_ms` need FLOOR division for negative inputs. The probe handles it
  with the explicit `if z >= 0 { z / 146097 } else { (z - 146096) / 146097 }` branch; the module must
  keep that shape and carry a pre-1970 test, or it ships wrong for negative timestamps. A `Date { y, m, d }` struct with `date_from_ms`, `date_to_days`, `date_add_days`,
  `date_weekday`, `date_is_leap`, `date_diff_days` and an ISO-8601 formatter covers the measured need
  (the `datetime` task) with zero TCB growth. **Timezones and DST are explicitly out** — they need a
  tzdata table and a host clock offset, and pretending otherwise produces wrong answers rather than
  missing ones. `now_ms()` is UTC; the module says so.
- **`examples/stdlib/set.ax`.** A `Set` over the existing `Dict` (unit values), with
  `set_new/add/has/remove/len/union/intersect/difference/to_arr`. Dicts already provide the hashing;
  a builtin would add a type for no capability the language lacks. **Dicts are string-keyed**
  (`builtins.rs:1507`), so this is a set of `str`; i64 members go through `to_str`. That changes the
  ergonomics enough to be worth stating in the module's own docs rather than surprising a caller.
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
· `E2203` non-linear regex construct refused · `E2204` decode output is not valid UTF-8 (§7) · `E2205` re_replace_all replacement references a capture group the pattern does not have, or an
ambiguous `$NN` (allocated 2026-08-08, §12.2) · `E2206`–`E2212` unallocated, held for these
· `E2213`+ belong to R43 (bytes) — do not allocate from that range here
slices. Note per §4: on `Result`-returning builtins these are `Err`-string prefixes, and on the
`str_slice` refusal it is panic-message text — not compiler diagnostics.

### 8.1 One reported issue that did NOT reproduce

Review reported that `chr(34) + "abc"` fails type-check with E0102 ("arithmetic operand has
non-numeric type str"), which would have blocked the quote-building idiom the userland modules need.
**It does not reproduce:** `axon check` exits 0 and the program prints `"abc`. The `+`-concat arm
handles `chr`'s return type correctly. Recorded so it is not filed as a bug later on the strength of
the review alone — and as a reminder that a review finding is a hypothesis until it is run.

## 9. Open questions

- **Q1 — ANSWERED: Slice 1 breaks nothing in-repo; land it first.** All 18 `str_slice` call sites
  are in `crates/axon-core/tests/fixtures/phase{18,32,38,48,60}*.ax`, `integration_fixtures.rs`,
  `cli_run.rs` and `parse_help_probe.rs`, and every one slices ASCII literals; the non-ASCII bytes in
  those files are box-drawing characters inside comments. No `examples/*.ax` calls `str_slice` at all.
  The two non-ASCII fuzz rows probe midpoints 3 and 6, both character boundaries (`"str"`,
  `"café "`), so existing fuzz rows will not begin panicking either. Blast radius: zero.
- **Q2 — Codepoints or graphemes?** Slice 2 commits to codepoints. Graphemes need a segmentation
  table (~large) and would make `str_chars("e" + combining-acute")` return one element instead of
  two. Recommendation: ship codepoints, revisit only with a measured failure that graphemes fix.
- **Q3 — Should `file_remove` need more than `FsWrite`?** Deleting is irreversible and R11's risk
  typing already distinguishes irreversibility. Recommendation: `FsWrite` for the capability check
  PLUS an `irreversible` flag feeding `risk_derive`, so a High-risk pipeline gates a program that
  deletes. Needs R11's owner to confirm rather than being decided here.
- **Q4 — Who owns hashing?** R28 (audit ledger) and R33 (quorum) both need `sha256`. This spec
  excludes it to avoid a second answer. Needs an explicit assignment.
- **Q5 — `classify_call` widening, per ARGUMENT.** Slice 4 needs it to carry
  `&[(IoKind, arg_index)]`, not `&[IoKind]` (§5, B1): the static check reads only `args.first()`
  (`capabilities.rs:1132`), so a kind list would check the source path twice and the destination not
  at all. This touches a security-critical function used by `@[contained]`, the sandbox and the effect
  bridge. Recommendation unchanged and now firmer: land the widening as its OWN commit, existing
  capability tests unchanged and green, with a NEW test asserting a `write`-only contained fn cannot
  `file_copy` out of a read-denied path — before any new builtin uses it. A refactor plus a new
  capability in one commit is how a hole gets in.
- **Q6 — Should the parity harnesses gain expected-value rows generally?** §2 showed the
  differential fuzzer is structurally blind to bugs interp and native share, which is every bug in
  the reference semantics. Recommendation: not in this spec (scope), but it deserves its own, and
  this is the second time a shared-semantics bug has reached a shipped builtin.

## 10. Deliberately out of scope

Sockets/TCP/UDP · iterators and lazy sequences · a `Json` value type · CSV/YAML/TOML · a logging
framework · bignum integers · trigonometry (`sin`/`cos`/`tan` and `log2` — absent, with no measured
failure tracing to them; note `ln`, `log10`, `exp`, `sqrt` and `pow` all EXIST, `builtins.rs:208-220`) · timezones/DST · grapheme segmentation · crypto (§9 Q4).

One exclusion worth a note rather than a change: **CSV**. No measured failure traces to it, so the
admission test excludes it — but `str_split` cannot handle quoted fields containing the delimiter, so
the first CSV-shaped task will fail. That is the admission test working as designed (wait for the
measurement), not an oversight, and it is recorded here so the eventual failure is recognised rather
than re-diagnosed.

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

---

## 12. Addendum (2026-08-08) — two post-completion findings

Recorded after the build loop reported DONE. Both are amendments to §11, not new slices.

### 12.1 The stop condition named ONE build configuration, and that was not enough

§11 says "`cargo test --workspace` shows no new failures against the 1808/0 baseline". That was
satisfied — and `scripts/gate.sh` was RED at the same time. The gate's test stage is

```
cargo test -p axon-core --no-default-features      # gate.sh:68
```

and `cli_run.rs`'s `axon()` resolves `CARGO_BIN_EXE_axon`, which is built in the TEST's feature
config. So in that stage the probed binary has no codegen backend, and Slice 2/4's three
`native_refuses_*` tests asserted an `E0910` refusal against a binary whose actual reply is
"requires building axon with the `codegen` feature". They had been failing since T4/T7 landed.

The cause was a stale capability probe — they skipped on `axon build --help` exiting non-zero, but
the `build` verb is registered regardless of the feature, so `--help` always succeeds and the skip
never fired. Fixed by `build_output_or_skip`, which probes the real build's refusal text; verified in
BOTH directions, because a guard that degrades into a permanent silent skip is worse than the bug it
replaced. Three sibling scripts carried the same stale form (`zephyr_qemu_gate.sh` failing rather
than skipping, plus two harmless instances) and were fixed in the same pass.

**§11 is amended:** a stop condition must enumerate every configuration the gate runs, and a green
figure must be reported WITH its configuration. "The suite passes" is not a claim until it names
which suite, built how.

### 12.2 The `str_slice` class had a second member, in the parser

§2 opened this spec on a silent-wrong-answer: `str_slice` returning `""` across a UTF-8 boundary.
The same class was found in string interpolation. `parse_fmt_inner_expr` handed a `{...}` slot to
`Parser::parse_expr` and never checked the sub-parser consumed it, so `"a{2,3}"` compiled to the
string `a2` — the `,3` discarded with no diagnostic. It is now a parse error naming the `{{` escape.

This spec had WORKED AROUND that bug rather than fixed it: §6 and all six `re_*` doc strings tell the
caller to write `a{{1,100000}}`, because every counted regex repetition is exactly the shape that
silently became a different pattern. Documenting a footgun is not closing it, and the note in the
regex fixture now says so.

**The generalisable rule:** wherever a parser runs on a SUBSTRING — format slots, attribute
arguments, embedded DSLs, prose lifting — require it to consume the whole substring. "It parsed" is
not "it parsed all of it". The regex parser added in Slice 5 already checks this
(`compile()` rejects trailing input); the format-slot parser was the outlier.
