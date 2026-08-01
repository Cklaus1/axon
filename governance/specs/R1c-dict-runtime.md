# Tech Spec — R1c: `dict_*` Runtime Data Structure (native codegen)

**Status:** 🚧 Implementing (re-verified 2026-07-18; counts + blockers corrected 2026-07-31) —
the tagged-value runtime this spec chose (option (a)) is real and largely landed: 17 of the
**20** dict-family operations (19 `dict_*` builtins + `arr_group_by`, per the
`crates/axon-core/src/builtins.rs` source of truth, builtins.rs:1480-1630) have native codegen
(`dict_new/set/get/has/len/inc/get_or/remove/keys/values/merge/from_pairs/to_pairs/map_values/
to_str/filter/each`, **17** `__axon_dict_*` extern symbols in axon-rt — 22 `extern fn`
*definitions* counting the 5 wasm twins, an earlier "22 externs total" double-counted those
twins, corrected 2026-07-31 pass 2) with confirmed native==interp parity
(`bash scripts/dict_parity.sh`: PASS on **int-valued** dicts, BTreeMap order preserved — but see
the harness caveat in §4: the harness compares exit codes only and can pass vacuously on SKIPs).
**Three** operations remain honestly interpreter-only, deliberately E0910-refused rather than
silently wrong: `dict_from_str`, `dict_try_from_str` (its strict `Result`-returning sibling,
BUG_HUNT #31 — previously unaccounted for in this spec, corrected 2026-07-31), and
`arr_group_by` (see `emit_dict_nonint_value_guard` in `codegen/expr.rs` — "abort loudly instead
of miscomputing"; earlier cited line numbers 817/7734-7735 had drifted and are dropped in favor
of the function name). Additionally the shipped v1 native dict is **int-valued-read only**: any
str/f64 `dict_get`/`dict_remove`/`dict_get_or` read hits the same guard and aborts at runtime
(exit 101), and the bulk readers `dict_values`/`dict_to_pairs` abort in the *runtime*
(`dict_abort_if_nonint`, axon-rt lib.rs) — the §2 per-call-site narrowing was *not* implemented;
a new slice 5 below schedules it. **OPEN SOUNDNESS BUG (found 2026-07-31 pass 2, confirmed
end-to-end):** the three closure ops (`__axon_dict_map_values`/`__axon_dict_filter`/
`__axon_dict_each`) and `__axon_dict_inc` have **no guard at all** on non-int values — they are
**silently wrong** natively (see §4 and slice 5a).
**SECOND OPEN SOUNDNESS BUG (found 2026-07-31 pass 3, confirmed end-to-end):** the `dict_set`
call-site tag dispatch is a **partial** match over LLVM value kinds whose default arm is
`_ => return None` (`crates/axon-core/src/codegen/expr.rs`, `dict_set` arm) — and `return None`
is exactly what a *successful* void `dict_set` also returns, so any value type the §1
`Tag {Int,Float,Str}` model does not cover compiles to **nothing at all**. The default is
therefore **fail-OPEN**, the precise opposite of the guard's own doc-comment principle ("abort
loudly instead of miscomputing"). Two confirmed I-2 divergences neither §4 nor slice 5a covers:
(1) **bool** rides the Int arm (an `IntValue`, sign-extended) so the tag!=0 guard cannot see it —
`dict_set(d,"approved",true)` reads back `true` under the interpreter and `-1` natively, **both
exit 0**; (2) **struct** values hit the default arm — `dict_set` is a silent **no-op** natively
(`dict_len`=0, `dict_has`=false) while the interpreter stores it, **both exit 0**. Fix scheduled
as **slice 5b**, which ships BEFORE slice 5. The **wasm32 dict externs half of slice 4 is
LANDED** (the **five** AxonStr-by-value externs — `set`/`get`/`has`/`remove`/`inc` — have
`#[cfg(target_arch="wasm32")]` scalar-expansion twins; the remaining 12 are single
target-independent definitions; `scripts/wasm_aot_run_parity.sh` runs the dict pattern on wasm). This header previously said "Draft" with no detail, misleadingly implying
the whole family was ungated; same staleness class as
R17/R21/R22/R23/R26/R27/R28/R29/R31/R32/R12/R14/R1b, caught by the same outer-loop sweep
(`EXECUTION_MODEL.md` §2) — but like R14, the accurate status is partial, not a clean flip.

```spec-meta
id: R1c-dict-runtime
status-claim: Implementing
depends-on: R1-codegen-build-unblock
blocks: none
blocked-by: none
supersedes: none
related: R1b-str-return-abi, R1d-single-source-builtins
conflicts-with: none
reserves: none (E0910 refusal for dict_from_str/dict_try_from_str/arr_group_by)
evidence: scripts/dict_parity.sh (re-verified 2026-07-18, 17/20 ops, int-valued; CAVEAT 2026-07-31: exit-code-only comparison, per-case SKIP still yields PASS — harden per §4 before citing as parity proof; CAVEAT 2026-07-31 pass 3: the vacuous SKIP path FIRED during review — a non-codegen rebuild yields 44 SKIPs and exit 0 as PASS); scripts/wasm_aot_run_parity.sh (dict externs on wasm); scripts/fuzz_parity.sh (PLANNED dict domain — slice 5 gate; dict_* is excluded by name today, ~:29-31)
```
**Requirement:** `../REQUIREMENTS.md` R1 — native pipeline must run the stdlib it
ships. The dict family (20 builtins — 19 `dict_*` + `arr_group_by`, used in **6
example files**) *(corrected 2026-07-31: this paragraph previously said "15
builtins" with "no native codegen"; per the header, 17/20 are now native with
int-valued parity, 3 remain E0910-gated)*; `axon run` (interpreter) handles the
full family today. `arr_group_by` returns a `Dict`, so it is gated on this too —
but its *actual* blocker is the array-valued runtime extension (§3 slice 6), not
the wasm build.

**Decisive fork:** *How does a dynamically-typed `Dict` (the interpreter's
`Rc<RefCell<BTreeMap<String, Value>>>`) become native code, given codegen values
are statically typed?*
- **(a) Tagged-value runtime** — one `__axon_dict_*` extern family in axon-rt
  backed by a `HashMap<String, TaggedVal>` where `TaggedVal` is a tag + 8-byte
  payload (i64 / f64 / AxonStr-handle). Mirrors the interpreter's `Value`
  dynamism; one dict type works for any value.
- **(b) Monomorphized typed dicts** — `Dict<str,i64>`, `Dict<str,str>`, … as
  distinct codegen types. Type-safe but the examples MIX value types in one
  dict (`dict_set(state,"agent",<str>)` and `dict_set(state,"approved",0)`),
  so (b) cannot represent the actual programs.
- **→ Resolve: (a) tagged-value runtime.** It's the only option that matches the
  shipped interpreter semantics (I-2 oracle) and the real example usage.

This mirrors the **channel** precedent (`__axon_chan_*` in `crates/axon-rt/src/
lib.rs`): an opaque `*mut c_void` handle to a heap `Arc<>`-managed structure,
created/mutated/dropped through C-ABI externs the codegen calls by name. NOT
inline IR.

---

## 1. Runtime ABI (axon-rt, native; wasm later)

A dict is an opaque `*mut c_void` → `Arc<Mutex<HashMap<String, TaggedVal>>>`
(Mutex for the same reason channels use one; single-threaded use is fine).

```
#[repr(C)] enum Tag { Int=0, Float=1, Str=2 }      // 1 byte, widened to i64 slot
#[repr(C)] struct TaggedVal { tag: i64, payload: i64 }  // payload: bitcast i64/f64,
                                                        // or a malloc'd AxonStr*
```

**Tag-assignment invariants** *(added 2026-07-31, pass 3 — these are normative, not
descriptive; the shipped code violates both, see the header's second open bug)*:

- **Totality.** The mapping from *source value type* to `Tag` must be **total** over storable
  types. Every value type is either assigned a tag or **E0910-refused at compile time**. There
  is no third outcome: a value type that is neither tagged nor refused is a silent-wrong cell,
  which is a release blocker (§4). Concretely this forbids a partial `match` on the LLVM value
  kind with a fall-through default that is indistinguishable from success.
- **Injectivity.** The tag must **identify the source type**, because it is the *only* dynamic
  information a reader has. Tags are therefore assigned from the **AST-level type**, NOT from
  the LLVM value kind — the LLVM kinds are not injective over source types, and two known
  collisions already exist in the shipped lowering: `bool` (and `i32`) are `IntValue`s and so
  ride `Tag::Int` indistinguishably from `i64`, and an array `[T]` lowers to the *same*
  `{i64 len, ptr data}` struct as `str` and so is stored as `Tag::Str`. Both collisions are
  latent type confusions today and are load-bearing for slice 5 (see §2 and §3 slice 5b).

**Boundary semantics** *(added 2026-07-31, pass 3)*: a `Dict` is the point in the language where
the static type system is **erased** — a value entering a dict is demoted to a raw
`{tag, payload}` scalar. This is a semantic boundary, not merely a codegen representation, and
it matters because every containment mechanism the project offers is type-directed (refinement
types `T where P` and their SMT discharge, effect rows, `Uncertain<T>`, and the info-flow
lattice `Tainted`/`Trusted`/`Secret`/`Public` in `examples/stdlib/tainted.ax`). Therefore:

- **Nothing is preserved across the boundary.** Refinement predicates, taint levels, and
  confidentiality levels are NOT carried in the tag and MUST be re-established on read.
  Refinement predicates already re-check at `let`/param obligation sites (Phase 5) — that is the
  mechanism that re-establishes them, and it only fires if the read is bound at a refined site.
- **Userland lattice wrappers are struct-shaped**, so under the totality rule above they are
  either tagged as structs or E0910-refused — they must never be silently dropped, which is
  exactly the shipped bug. A `Secret` that vanishes from a native dict makes a downstream
  `secret_can_flow_to` read take the None/default path, i.e. the containment check evaluates
  against absent data rather than failing closed.
- Whether lattice-typed values should be *storable in a dict at all*, versus rejected by a
  checker rule, is **unresolved** — see §12 Q2.

Core externs (the high-value slice):
- `__axon_dict_new() -> *mut c_void`
- `__axon_dict_set(d, key: AxonStr, tag: i64, payload: i64)` — string key by
  value (the str-ABI bridge already handles AxonStr; reuse it). Clones the key
  into the map.
- `__axon_dict_get(d, key: AxonStr, out_tag: *mut i64, out_payload: *mut i64)
  -> i1 found` — codegen assembles the `Option<T>` from (found, tag, payload).
- `__axon_dict_has(d, key: AxonStr) -> i1`
- `__axon_dict_len(d) -> i64`
- `__axon_dict_remove(d, key, out_tag, out_payload) -> i1`
- `__axon_dict_drop(d)` — Arc decref.

Follow-on externs: `__axon_dict_keys` / `__axon_dict_values` (return a slice —
must malloc + fill via the runtime, returning the `{len,ptr}` out-params),
`__axon_dict_merge` (new dict from two), and the **closure-taking** ones
(`dict_map_values` / `dict_each` / `dict_filter` — codegen passes the lambda
fat-pointer; the runtime calls back through a `extern "C" fn(env, …)` pointer,
the same indirect-call ABI the arr_* closure ops use).

## 2. Codegen side (`emit_call` in `codegen/expr.rs`)

- `dict_new()` → call `__axon_dict_new`, result is an opaque ptr; the Axon
  `Dict` type lowers to `i8*` (like a channel handle). Add `Type::Dict` →
  `i8_ptr` in `codegen/types.rs`.
- `dict_set(d, k, v)` → determine v's tag at the call site (i64→Int,
  f64→Float-bitcast-to-i64, str→Str with the AxonStr handle in payload), call
  the extern. (Same call-site type dispatch as `to_str` / `as_i64`.)
  *(Corrected 2026-07-31, pass 3: the shipped dispatch reads v's **LLVM** type,
  which violates the §1 injectivity invariant — `bool`/`i32` are `IntValue`s and
  are silently tagged Int, and `[T]` shares `str`'s `{i64,ptr}` struct and is
  silently tagged Str. It is also **partial**: the default arm `_ => return None`
  is byte-identical to a successful void `dict_set`, so an unmodelled value type
  emits no call at all. Per §1 the dispatch must be driven by the AST-level type
  and must be **total** — every unmodelled type E0910-refused by name. Slice 5b.)*
- `dict_get(d, k)` → call the extern with out-param slots; build `Option<T>`
  from the found flag + reinterpret payload by tag. v1 narrowing: the *match
  arms* in the examples are monomorphic per call site (a given dict_get is
  matched as str OR int, never both), so codegen can reinterpret the payload to
  the type the surrounding match expects — confirm against
  `examples/asi/llm_cache.ax` + the bandit demos.
  *(Status note, 2026-07-31; corrected same day, pass 2: the shipped v1
  implemented an int-only READ instead of this narrowing —
  `emit_dict_nonint_value_guard` aborts at runtime (exit 101) on any non-Int tag
  at `dict_get`/`dict_remove`/`dict_get_or`. Str values are runtime-ready **only
  for the single-value readers**: `__axon_dict_get` and `__axon_dict_remove`
  malloc the str copy out. The bulk readers `__axon_dict_values`/
  `__axon_dict_to_pairs` call `dict_abort_if_nonint` and exit 101 on ANY
  non-int-valued dict, and the closure ops/`dict_inc` are silently wrong (§4) —
  an earlier version of this note overclaimed "supported end-to-end in the
  RUNTIME". Slice 5 replaces the guard's int-only condition with an
  expected-tag condition; the abort path itself MUST remain for dynamic tag
  mismatches — the narrowing is by static expectation while the tag is dynamic,
  so dropping the abort would reintroduce the silent-garbage class the guard
  exists to kill. Note the per-call-site monomorphy premise above is still only
  "confirm against examples", not a verified language invariant.)*

  **Monomorphy is an OBSERVATION, not a design premise** *(demoted 2026-07-31,
  pass 3)*. Its entire evidence base is the **6 example files** that use dicts
  (`examples/asi/{safe_bandit,persistent_bandit,corrigible,llm_cache,word_freq}.ax`,
  `examples/stdlib/bandit.ax`, `examples/browser/demo.ax`) — all hand-written, all
  Int- and Str-valued. **Nothing in the language forbids** reading one key at
  several call sites with different expected types, and a generated cache or
  state bag is a natural shape for exactly that. Therefore, normatively: the
  **dynamic tag check is the sole soundness guarantee** for dict reads, and it
  **MUST NEVER be elided on monomorphy grounds** — not as an optimization, not as
  "provably redundant against the corpus". If a *static* guarantee is wanted it
  must be made real (a checker rule with its own E-code rejecting a `dict_get`
  whose match arms demand two different value types), never inferred from the
  current example set. See §12 Q1.

  **Slice 5's planned guard condition is NOT sound as written** *(2026-07-31,
  pass 3)*: "abort on tag != expected" is sound only if the tag identifies the
  source type, and per §1 it does not. An array `[T]` is stored with `Tag::Str`,
  so once a str-expecting call site is allowed to accept tag==2, `dict_set(d,
  "xs", [7,8,9])` followed by a str-typed read reconstructs an `AxonStr` over
  i64 array backing — a type-confusion read of binary data as UTF-8. Today that
  shape survives *by accident only*: the array stores fine and the read aborts
  101 solely because the blunt guard demands Int (and the abort message,
  "non-int-valued dicts (str/float)", already misdescribes what is stored).
  **Slice 5 would make this shape strictly less safe than the status quo.** It is
  therefore blocked on the injectivity fix — see §3 slice 5b.
- `dict_len`/`dict_has`/`dict_remove` → thin extern calls.

## 3. Slices (one gated commit each)

1. **Runtime core + `dict_new`/`set`/`get`/`has`/`len`** (native). Harness:
   `scripts/dict_parity.sh` — round-trip set→get (int + str values), has,
   len, get-missing→None. Parity native==interp. New `Type::Dict` lowering.
2. **`dict_remove` + `dict_get_or` + `dict_keys`/`dict_values`** (slice-
   returning — runtime mallocs the result).
3. **`dict_merge` + the closure ops** `dict_map_values`/`dict_each`/`dict_filter`
   (lambda fat-pointer callback into the runtime).
4. ~~**wasm32 axon-rt build of the dict externs**~~ ✅ **LANDED** (corrected
   2026-07-31; mechanism re-corrected same day, pass 2): the **five**
   AxonStr-by-value externs (`set`/`get`/`has`/`remove`/`inc`) have
   `#[cfg(not(target_arch="wasm32"))]`/`#[cfg(target_arch="wasm32")]`
   scalar-expansion twin pairs in `crates/axon-rt/src/lib.rs`; the remaining 12
   (`new/from_pairs/merge/len/keys/values/to_pairs/map_values/to_str/filter/
   each/drop`) are single target-independent definitions needing **no** twin.
   `scripts/wasm_aot_run_parity.sh` exercises the dict pattern on wasm ("The
   ENTIRE dict API now links+runs on wasm" — all 17 symbols link per target).
   The `arr_group_by` tail this slice named is re-scoped to slice 6 — this
   slice's original premise ("now its `Dict` return type is buildable") was
   FALSE: `arr_group_by` returns `Dict[str, [T]]`, an **array-valued** dict,
   and the §1 ABI/runtime has no array variant (`DictVal` = Int|Float|Str
   only). Its blocker is a runtime-ABI extension, not the wasm build.
5. **Str-valued (and f64-valued) `dict_get`/`dict_remove`/`dict_get_or`
   reconstruction** — implement the §2 per-call-site narrowing. This does NOT
   delete `emit_dict_nonint_value_guard`: the guard's condition changes from
   "abort on non-Int" to **"abort on tag != expected"** — the abort path must
   survive, because the narrowing is by static expectation while the stored tag
   is dynamic (a mixed-value dict read at a wrong-typed call site must abort,
   not hand back pointer bits as an i64). Deliverables: `dict_from_str` and
   `dict_try_from_str` go native (both produce str-valued dicts; same
   lowering); str-valued reconstruction for the bulk readers `dict_values`/
   `dict_to_pairs` (whose runtime `dict_abort_if_nonint` guard otherwise still
   aborts — without this, "str-valued dicts work" would quietly exclude the
   bulk readers). **Blocked on slice 5b** — see §2: "abort on tag != expected"
   is unsound while the tag space is non-injective, and landing this slice first
   converts today's accidental array abort into a type-confusion read.
   Gate *(retargeted 2026-07-31, pass 3)*: **add a dict domain to
   `scripts/fuzz_parity.sh`** rather than hand-writing more `dict_parity.sh`
   cases — cross **ops × value types × key sets**, value types at least
   {int, f64, str, bool, array, struct, Option, nested dict} and key sets
   including **non-ASCII** and BTreeMap-ordering-sensitive keys, comparing
   **stdout AND exit code**. `dict_parity.sh` stays as the fast smoke gate.
   Rationale: `R1f-differential-parity-fuzz.md` is LANDED and argues in its own
   problem statement that "a generator that *searches* the input space finds
   these; a human writing check(5, 3) does not" — yet `scripts/fuzz_parity.sh`
   explicitly excludes `dict_*` by name (the "struct-returning constructors"
   exclusion, fuzz_parity.sh ~:29-31), delegating dict back to the very harness
   this spec caveats in §4. Every divergence class found in pass 3 (bool,
   struct-drop, array-mistag) is a value-**type**-space divergence a
   two-dimensional generator finds immediately; the repo has already been bitten
   by this exact shape once, when an ASCII-only fuzz corpus hid the inline-IR
   `str` Unicode divergences. Must additionally include a **deliberate
   tag-mismatch abort case** and a **store-array / read-as-str case that MUST
   abort 101**. This slice is what makes §4 gates (1) and (3) reachable.
   5a. *(added 2026-07-31, pass 2 — soundness fix, ships BEFORE or with any
   further native-dict claims)* **Close the closure-op/`dict_inc` silent-wrong
   hole + harden the parity harness.** (i) Add the `dict_abort_if_nonint`
   runtime guard to the snapshot loops of `__axon_dict_map_values`/
   `__axon_dict_filter`/`__axon_dict_each` (today they pass `s.as_ptr() as
   i64` / `f.to_bits() as i64` straight into the user callback — confirmed:
   interp panics exit 101, native silently prints a raw pointer and exits 0);
   (ii) make `__axon_dict_inc` abort on a non-Int existing value (today `_ =>
   0` silently coerces, vs the interpreter's "existing value at '{k}' is …,
   not i64" panic). (iii) Harden `scripts/dict_parity.sh`: count SKIPs and
   fail when any case SKIPs or when zero cases pass (the current per-case
   "SKIP (native build unavailable)" path lets a total codegen regression
   print 44 SKIPs and still exit 0 — the documented vacuous-pass class), and
   compare stdout in addition to exit codes (today both streams are discarded,
   so same-exit-different-output divergence passes). Gate: str-valued
   `dict_map_values`/`dict_filter`/`dict_each`/`dict_inc` divergence cases in
   the hardened harness, native==interp (both abort 101).
   5b. *(added 2026-07-31, pass 3 — soundness fix; ships BEFORE slice 5 and
   alongside 5a, ahead of any further native-dict work)* **Make the `dict_set`
   tag dispatch TOTAL and the tag space INJECTIVE** (the §1 invariants).
   (i) Replace the default arm `_ => return None` in the `dict_set` lowering
   (`codegen/expr.rs`) with an explicit **E0910 refusal naming `dict_set` and the
   offending value type**, so an unmodelled type is a *compile error* rather than
   a silent no-op — today a struct-valued `dict_set` emits no call at all
   (`dict_len`=0, `dict_has`=false natively vs stored under the interpreter, both
   exit 0). (ii) Give `bool` either its own **`Tag::Bool`** discriminant with a
   matching interpreter round-trip, or an E0910 refusal — it must NOT ride
   `Tag::Int`, where the tag!=0 guard is structurally blind to it and a stored
   `true` reads back as `-1` natively. (iii) **Pull `Tag::Arr` forward from slice
   6**, at minimum as a store-tag with abort-on-read, so `[T]` is distinguishable
   from `str` *before* slice 5 makes str reads legal. (iv) Fix the guard's abort
   message to print the **actual stored tag** rather than the misleading fixed
   string "non-int-valued dicts (str/float)". (v) Drive tag assignment from the
   AST-level type, not the LLVM value kind (§1 injectivity).
   Gate: a **mechanical totality test** over ops × {int, f64, str, bool, array,
   struct, enum, Option, tuple, dict} that **fails on any cell where interp and
   native both exit 0 with differing stdout**. Rationale for the ordering: `bool`
   is the type most likely to carry a safety decision —
   `dict_set(state,"approved", …)` is literally the shape in
   `examples/asi/safe_bandit.ax`, int-valued today only because a human wrote it
   that way.
6. **Array-valued dict extension → `arr_group_by`** — the runtime-ABI extension
   slice 4 was missing *(note 2026-07-31, pass 3: the `Tag::Arr` **discriminant**
   is pulled forward into slice 5b for tag-injectivity reasons — arrays must be
   distinguishable from `str` before str reads become legal. That is a store-tag
   with abort-on-read only; the full read/ownership work below is unchanged and
   still lives here)*: a new `Tag::Arr` variant, an extern signature carrying
   the `{len, ptr}` slice (+ element tag), ownership/drop rules for stored
   slices (the dict owns a deep copy; `__axon_dict_drop` frees it), and codegen
   reconstruction of the `[T]` on `dict_get`/`dict_values`. Alternative
   resolution: explicitly declare `arr_group_by` permanently E0910-gated and
   record that as an accepted narrowing — either outcome must be stated, not
   left implied. Gate: `arr_group_by` parity case in `dict_parity.sh`, or a
   dated E0910-permanent decision in this spec.

## 4. Honesty / scope

*(Corrected 2026-07-31: this section previously said "Until slice 1 lands,
`dict_*` stays E0910-gated" — slices 1-3 and the wasm half of slice 4 are
landed. Re-corrected same day, pass 2: an intermediate rewrite claimed the
enumeration below was complete and concluded "never a silently-wrong binary" —
that conclusion was FALSE; see the open hole.)*

**The posture is an INVARIANT, not a list** *(restated 2026-07-31, pass 3)*:

> **Every (dict op × value type) cell is either statically refused (E0910) or
> dynamically aborted (exit 101). A cell where interp and native both exit 0 with
> differing behaviour is a release blocker, not a documented caveat.**

This wording is deliberate. The enumeration below was originally *derived* from
what the **6 human-authored example files** that use dicts happen to do — all Int-
and Str-valued. An enumeration built that way is a statement about a corpus, not a
guarantee about the language, and it **expires the moment the authorship
distribution shifts** from a handful of curated examples to machine-generated
volume. Pass 3 found two cells the enumeration never listed (bool round-tripping
as `-1`; struct-valued `dict_set` silently dropped — see the header and slice 5b),
both reachable from ordinary source, neither caught by any gate. **Stated limit:**
the enumerative tiers below are only sound under the assumption that a human
reviews every dict-using artifact before it is built natively. That assumption is
recorded here as a *limit*, not relied on as a *control* — the invariant above,
plus the §1 totality/injectivity rules and the slice-5b mechanical totality test,
are what must actually hold.

**Refusal must be at least as loud as the unsound path it replaces**
*(design rule, added 2026-07-31, pass 3)*. The three tiers differ sharply in
**machine visibility**, and that difference is itself a hazard. Tier 1 (E0910) is a
compile error: any generate-compile-retry loop observes it and routes around it
deterministically. Tier 2 (runtime abort) is observable only if the generated
program is actually run on the failing input. Tier 3 (silently wrong) is invisible
— exit 0, plausible output. So the compiler's feedback gradient points a generator
optimizing for "it compiles and the gate is green" **away** from the
refused-but-correct builtins and **toward** the unguarded ones: the enforcement
mechanism actively selects for the unsound surface. This is a property of the tier
*structure*, not of any one bug, and it does not depend on the generator being
careless — an optimizing one finds tier 3 faster. Note also that prose mitigation
in this file is not a control against an optimizing generator: this spec, including
its explicit map of which ops are unguarded, lives in the same repo that is handed
to a generator as context. It helps a careless author and does nothing about an
optimizing one. Consequently: **no dict op may be left in tier 3 between
releases.** If slice 5a/5b cannot land immediately, E0910-refuse
`dict_map_values`/`dict_filter`/`dict_each`/`dict_inc` (and unmodelled `dict_set`
value types) in the interim — a build-time refusal a generator can see and route
around, matching the treatment `dict_from_str` already gets — rather than shipping
a silently-wrong path documented only in prose.

The current (to-be-closed) posture has **three** tiers, not two:
- **E0910-refused** (compile error naming the builtin, pointing at `axon run`):
  `dict_from_str`, `dict_try_from_str`, `arr_group_by`.
- **Runtime abort (exit 101)** on non-int values: the single-value readers
  `dict_get`/`dict_remove`/`dict_get_or` via the codegen-emitted
  `emit_dict_nonint_value_guard`, AND the bulk readers `dict_values`/
  `dict_to_pairs` via the runtime-side `dict_abort_if_nonint` (axon-rt lib.rs —
  this abort fires on all targets, wasm included; an earlier enumeration
  omitted it).
- **OPEN HOLE — silently wrong (I-2 violation), fix scheduled as slice 5a:**
  the closure ops `dict_map_values`/`dict_filter`/`dict_each` snapshot str/f64
  values as raw pointer/bit i64s into the user callback with no guard
  (confirmed end-to-end 2026-07-31: interp panics exit 101, native prints
  pointer arithmetic and exits 0), and `dict_inc` on a non-Int existing value
  silently coerces to 0 where the interpreter panics. **Also (pass 3): `bool`
  values round-trip as `-1` natively vs `true` under the interpreter, and
  struct-valued `dict_set` is a silent native no-op — both exit 0, neither
  reachable by any existing guard (see the header and slice 5b).** Until slices
  5a **and 5b** land, a native dict binary CAN be silently wrong; this spec must
  not claim otherwise.
The interpreter path runs all dict programs today (I-2). The tagged-value
decision is the load-bearing one and is resolved above.

**Harness caveat (2026-07-31, pass 2):** `scripts/dict_parity.sh` — the
primary evidence gate — currently compares **exit codes only** (stdout
discarded on both sides) and treats a per-case native build failure as a
non-fatal SKIP with no skip counter or passed>0 assertion, so a total codegen
regression would still print PASS (the repo's documented vacuous-pass class).
"dict_parity: PASS" is therefore necessary-but-weak evidence until the slice-5a
hardening lands; do not cite it as parity proof without this caveat.
*(Pass 3 — the vacuous path is not theoretical: during the pass-3 review
`target/debug/axon` was rebuilt **without** the codegen feature by a concurrent
build, a state in which the harness prints 44 SKIPs and exits 0 as PASS. The
5a(iii) hardening is accordingly **urgent**, not housekeeping.)* Structurally,
44 hand-authored one-liners over ASCII keys and small int values is the exact
anti-pattern `R1f-differential-parity-fuzz.md` was written to eliminate; the
durable fix is the fuzz domain retargeted into slice 5's gate, not more cases.

**Testable gates:** (1) round-trip set/get for int + str + f64 values
native==interp — *reachable only via slice 5; the shipped v1 `dict_parity.sh` is
int-only by its own header*; (2) get-missing → `None` ✅; (3) a mixed-value dict
(the real example shape) round-trips — *reachable only via slice 5*; (4) no
value leaks (drop decrefs); (5) AOT-wasm parity ✅ (`wasm_aot_run_parity.sh`);
(6) *(added 2026-07-31)* `arr_group_by` either passes a parity case (slice 6) or
carries a dated permanent-E0910 decision in this spec; (7) *(added 2026-07-31,
pass 2)* str-valued `dict_map_values`/`dict_filter`/`dict_each`/`dict_inc`
abort natively (exit 101) matching the interpreter, verified by the hardened
harness (SKIP-fatal, passed>0, stdout compared) — slice 5a; (8) *(added
2026-07-31, pass 3)* the **mechanical totality test** — ops × {int, f64, str,
bool, array, struct, enum, Option, tuple, dict} — reports **zero** cells where
interp and native both exit 0 with differing stdout, and `dict_set` of an
unmodelled value type is an E0910 compile error naming the type — slice 5b;
(9) *(added 2026-07-31, pass 3)* a **store-array / read-as-str** case aborts 101
natively (tag injectivity), gating slice 5 — slice 5b.

## 12. Open questions

Blocking:
- **Q1 (static monomorphy rule?):** §2's per-call-site monomorphy property is
  demoted to an observation over 6 example files, with the dynamic tag check as
  the sole guarantee. Should it *also* become a static checker rule with its own
  E-code — rejecting a `dict_get` whose match arms demand two different value
  types — or does the dynamic abort suffice permanently? Unresolved. Either way
  the dynamic abort **stays**; this question is only about adding a static layer,
  never about removing the runtime one.
- **Q2 (lattice values in dicts):** §1's boundary semantics establish that
  refinement, taint, and confidentiality levels are erased on entry to a dict.
  Two coherent resolutions and no decision yet: (a) allow storage and require
  re-establishment on read (needs a stated re-check obligation, and the read site
  must be a refined binding for Phase-5 machinery to fire), or (b) forbid
  `Tainted`/`Trusted`/`Secret`/`Public`/`Uncertain<T>` values from entering a
  dict at all via a checker rule. Choosing (a) by default — which is what the
  current code does implicitly — leaves the containment machinery without
  purchase at exactly the place generated code is most likely to park state.

Non-blocking:
- **Q3 (`arr_group_by` end state):** unchanged from slice 6 — either an
  array-valued runtime extension or a dated permanent-E0910 decision. Slice 5b
  pulls the `Tag::Arr` *discriminant* forward for injectivity reasons only; that
  does not by itself resolve whether arrays become fully storable/readable.
- **Q4 (dict fuzz cost):** slice 5's retargeted gate assumes a dict domain in
  `scripts/fuzz_parity.sh` is cheap to generate. If the ops × types × keys matrix
  proves too slow for the default gate, the fallback is a nightly-only tier —
  but `dict_parity.sh` alone must NOT be restored as the sole gate.

---

### Review note

**ASI-trajectory review completed 2026-07-31 (pass 3).** Verdict: **undermined** —
the decisive fork (option (a), tagged-value runtime) still holds; nothing about a
shifted authorship distribution changes that it is the only representation matching
the interpreter oracle. What did not hold is the spec's *safety contribution*: §4's
honesty posture was an **enumeration** derived from 6 human-authored example files,
and two silent I-2 divergences outside that enumeration were confirmed end-to-end
on this tree (bool → `-1`; struct-valued `dict_set` → silent no-op, both exit 0),
neither fixed by slice 5a. Both share one structural cause: the `dict_set` tag
dispatch is a partial match whose default arm is indistinguishable from success —
**fail-open**, contradicting the guard's own "abort loudly instead of miscomputing"
principle.

**Fixes applied:** header records the second open soundness bug; §1 gains normative
**totality** and **injectivity** tag invariants plus a **Boundary semantics**
subsection; §2 demotes monomorphy from premise to observation (dynamic check
declared non-elidable) and records that slice 5's planned "tag != expected" guard
is unsound while `[T]` aliases `str`; new **slice 5b** (total dispatch, `Tag::Bool`,
`Tag::Arr` pulled forward, AST-level tagging, honest abort message) ships **before**
slice 5, which is now explicitly blocked on it; slice 5's gate retargeted from
hand-written `dict_parity.sh` cases to a **dict domain in the landed
`scripts/fuzz_parity.sh`** (which excludes `dict_*` by name today); §4 restated as
an **invariant** with the enumeration demoted, adds the *refusal-must-be-as-loud*
design rule (tier visibility steers an optimizing generator toward tier 3) and
records the human-review assumption as a **stated limit, not a control**; gates (8)
and (9) added; §12 opened with Q1–Q4.

**No gate weakened.** Every abort path, E0910 refusal, and existing gate is
preserved or strengthened; the two additions to the tier-1 refusal set (unmodelled
`dict_set` types, interim closure-op refusal) move surface *toward* fail-closed.
Strategic questions (static monomorphy rule, lattice values in dicts) are recorded
in §12 rather than answered here.
