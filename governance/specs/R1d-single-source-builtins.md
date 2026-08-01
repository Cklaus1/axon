# Tech Spec — R1d: Single-Source Builtins

**Status:** 🚧 Implementing (re-verified 2026-07-20) — **Slice 2 out-param-synthesis extension
LANDED 2026-07-20**: `STR_OUT_EXTERNS` (a table separate from `BUILTIN_EXTERNS`, see below) +
`Codegen::synthesize_str_out_wrapper` migrated 7 of the ~10 previously-ineligible str-out-param
candidates — `str_reverse`/`str_to_upper`/`str_to_lower`/`str_digits_only`/`str_trim`/
`str_trim_start`/`str_trim_end` (every one sharing the identical single-`L::Str`-param shape).
Before implementing, verified the out-param convention these rt fns use is a deliberate
cross-target design (wasm32's struct-arg/return ABI differs from native's, per axon-rt's own
`#[cfg(target_arch="wasm32")]` variants) rather than a historical accident, and confirmed
codegen's own LLVM-IR-generation side has NO wasm32-specific branching for any of these
functions — meaning a generic synthesis function reproducing the EXACT hand-written IR shape is
safe on both targets by construction (it emits literally the same IR the hand-written blocks did).
Deliberately a NEW, separate table (not a field added to `ExternSig`) so `BUILTIN_EXTERNS`'s ~32
existing rows and `declare_one_extern`'s already-gate-verified logic are untouched. 3 new drift
tests extend R1d Slice 3's "the tables can't drift" promise to the new table (5 drift tests total).
Verified: `cargo build`/clippy clean (both feature sets); 5/5 drift tests; a manual native==interp
check across 14 hand-picked cases (incl. `straße`→`STRASSE` Unicode-growing case-map, empty
strings) byte-identical; full `scripts/fuzz_parity.sh` (all 7 already had corpus entries) PASS.
**Second batch LANDED same day**: `str_repeat`/`str_slice`/`str_replace`/`str_pad_start`/
`str_pad_end` — the multi-arg candidates, confirmed by reading each one's actual codegen (not
assumed) to share the identical output-side shape as the first batch, needing zero new synthesis
logic — `synthesize_str_out_wrapper` already iterates `params: &'static [L]` generically, so mixed
leading-arg shapes (`[Str, I64]`, `[Str, I64, I64]`, `[Str, Str, Str]`, `[Str, I64, Str]`) all work
unchanged. Verified: `cargo build`/clippy clean; 5/5 drift tests (now covering all 12
`STR_OUT_EXTERNS` rows); a manual native==interp check across 12 more hand-picked cases (incl.
multibyte fill char `★` padding, empty-string edge cases, a byte-vs-char UTF-8 slice boundary)
byte-identical; full `scripts/fuzz_parity.sh` PASS. **`STR_OUT_EXTERNS` is now 12 rows — every
single-`AxonStr`-return out-param candidate identified in the 2026-07-18 scan except `str_split`
(returns `Array<Str>`) and `str_join` (takes an array as input), which remain genuinely different
shapes needing separate, further extension, not assumed to fit this table.** *(Corrected
2026-07-31: the 2026-07-18 scan itself missed a candidate — see the scan correction below;
"every … except" holds only relative to that scan's incomplete enumeration.)* Below this line is
the original **Slice 1 LANDED**
header (`b8f54fb`,
"BUILTIN_EXTERNS registry (R1d slice 1) — collapse 21 declare blocks"):
`crates/axon-core/src/codegen/builtin_externs.rs` exists exactly as designed, 24 registry rows (count corrected 2026-07-31; the spec previously
said 25 — off by one at every stage, see the count correction below),
`declare_one_extern`/`declare_builtin_externs` doing the iterate-and-declare this spec prescribes.
**Slice 2 landed a further batch 2026-07-18**: `sleep_ms`/`now_ms` (`daa01f7`) and `dict_merge`
(`eff8ce2`) were already in the registry; this session added the 7 f64 math intrinsics
`sqrt`/`pow`/`floor`/`ceil`/`exp`/`ln`/`log10` (all `llvm.*.f64` LLVM-intrinsic-backed, previously
a hand-written "Phase 3 math builtins" block in `codegen/builtins.rs` declaring the same functions
one `add_function` call at a time) — collapsed into 7 registry rows, the old block deleted. Verified
before migrating: none of the 7 have bespoke call-site lowering (a single `get_function("sqrt")`
reverse-dependency from `arr_std_f64` and the `sqrt_f64`/`floor_f64`/`ceil_f64` wrapper builtins
still resolves correctly, since `declare_builtin_externs()` always runs first in `declare_builtins`
regardless of which code path declares the underlying function). Re-verified: `cargo build -p
axon-core` clean; `codegen::builtin_externs::drift_tests` both PASS (31 rows then; count corrected 2026-07-31); the full
`scripts/fuzz_parity.sh` PASS including `sqrt`/`floor`/`ceil`/`exp`/`ln`/`log10` and the
`nan_sqrt_neg` edge case; `pow` (not in the fuzz corpus) checked manually via
`axon run`/`axon build` on `pow(2,10)`/`pow(2,0.5)`/`pow(-8,1/3)` — native matches interp exactly
incl. the NaN case. **Resolved 2026-07-18: the str/math family this slice originally named as its
next batch (`str_to_upper`/`str_to_lower`/`str_trim{,_start,_end}`) is confirmed NOT registry-
eligible, not merely unverified.** Read the actual codegen: each is a `str → str` out-param call
(`void __axon_str_to_upper(AxonStr, i64* out_len, i8** out_ptr)`) wrapped in a hand-built shim
function that allocas the two out-slots, calls the rt fn, then reassembles an `AxonStr` return via
`insert_value` — exactly the "bespoke call-site lowering" class `BUILTIN_EXTERNS`'s own top comment
excludes by name ("str_slice's out-param wrapper"). `ExternSig`/`declare_one_extern` only knows how
to declare-and-link a symbol, not synthesize an out-param-unwrapping function body; making this
batch registry-eligible would mean extending the registry's own capability model first (a new,
separate sub-slice), not a batch of ordinary row-adds. Also checked `i64_to_f64`/`f64_to_i64` as
alternative Slice 2 candidates: both fail differently — they're single-LLVM-instruction function
BODIES synthesized in codegen (not declarations of an existing external symbol), so they don't fit
the registry's declare-only model either, and converting them into real `__axon_*` externs would
add a genuine cross-compilation-unit call for what's currently one hardware instruction — a
regression, same class as the bitwise-op rejection above. **Confirmed exhaustively 2026-07-18: no
further simple candidates remain.** Scanned every `"__axon_\w+"` symbol reference in
`codegen/builtins.rs` + `codegen/expr.rs` against the registry (not just a `BUILTINS`-name diff —
the actual linked symbols). Every unregistered `__axon_*` symbol falls into one of the categories
already ruled ineligible: str-returning out-param wrappers (`str_repeat`/`str_slice`/`str_reverse`/
`str_replace`/`str_split`/`str_join`/`str_pad_*`/`str_trim*`/`str_to_upper`/`str_to_lower`/
`str_digits_only`/`str_count`), dict operations (already-established bespoke out-param family),
panic helpers invoked mid-expression rather than as a top-level call (`__axon_*_panic`), or
impure/effectful builtins with complex `Result`/`Option`/`Uncertain` return layouts
(`ai_extract_*`, `goal_run`, `spawn`, `select`, `chan_*`, `parse_*`, `read_file`/`write_file`,
`exec`, `provenance_*`, `register_*`). **Slice 2's simple-batch migration (declare-only externs and
LLVM intrinsics) is COMPLETE for this registry model** — 7 math intrinsics this session plus the
pre-existing 24 rows cover every eligible candidate found. Further Slice 2 progress requires
extending `ExternSig`/`declare_one_extern` to support synthesized out-param-unwrapping wrapper
bodies, which is real, separate structural work (a new sub-slice), not a batch of ordinary
row-adds. **Slice 3 (drift cross-check test) LANDED 2026-07-18**:
`codegen::builtin_externs::drift_tests` — `every_extern_row_matches_a_known_builtin_with_the_same_arity`
asserts every `BUILTIN_EXTERNS` row's `axon_name` (the join key the field comment reserved for
exactly this) resolves to a `BUILTINS` entry with the same param count, and
`no_duplicate_extern_registry_rows` catches accidental double-registration — both PASS on the
then-current 24 rows (count corrected 2026-07-31). This is the one-directional half of the spec's "vice-versa" ask (registry → source
table); the reverse direction (does every eligible `BUILTINS` row have a registry entry) isn't
cleanly automatable without a marker for "which builtins use the straight-extern path vs. a bespoke
call-site special case" — left as a known gap, not silently skipped. **Slice 4 (CLAUDE.md doc
update) LANDED 2026-07-18**: `CLAUDE.md`'s "Adding a New Builtin" section now presents two paths —
the R1d fast path (`builtins.rs` row → axon-rt fn → one `BUILTIN_EXTERNS` row → parity test, 4
steps folding the old codegen-declare + infer.rs-return-type edits into the single registry row)
for a plain scalar/handle extern, and the original 5-step recipe kept verbatim for bespoke
call-site builtins (out-params, dict get/set/remove/keys, `to_str`) that the registry deliberately
excludes. All 4 slices now landed (Slice 2 is the one exception — genuinely partial, not "done";
see above). This header said "Draft" with zero indication that a third of the design already
shipped; same staleness class as R17/R21/R22/R23/R26/R27/R28/R29/R31/R32/R12/R14/R1b/R1c, caught by
the same outer-loop sweep (`EXECUTION_MODEL.md` §2) — genuinely partial, not a clean flip.
**Slice 2 correction 2026-07-20**: sizing "extend `ExternSig`/`declare_one_extern` for out-param
synthesis" (per the "further Slice 2 progress requires..." note above) found the 2026-07-18
"exhaustive `__axon_*` symbol scan" had one real miscategorization: `str_count` was bucketed with
the str-returning out-param wrapper family (`str_replace`/`str_slice`/etc.) purely by name
resemblance — its actual codegen never used the out-param dance at all (`__axon_str_count(AxonStr,
AxonStr) -> i64` was a straight two-str-arg-to-scalar call, the exact shape `str_index_of` already
had a registry row for). Migrated it (32nd row — count corrected 2026-07-31) and deleted the now-redundant hand-written block;
native==interp verified manually (5 cases incl. empty needle, empty haystack, overlapping matches
— all byte-identical). This does NOT mean the "extend the registry for out-param synthesis" work is
done or unnecessary — the other ~10 named candidates (`str_replace`/`str_slice`/`str_reverse`/…)
are genuinely out-param wrappers and still need that real, separate structural extension; this was
just the one candidate that turned out not to need it after all.
**Scan correction 2026-07-31 (adversarial review)**: a mechanical re-scan of every `__axon_*`
symbol in `codegen/builtins.rs` + `codegen/expr.rs` against BOTH registries found the 2026-07-18
"exhaustive" scan's SECOND miss (str_count was the first): `i64_to_str_radix`
(`codegen/builtins.rs:3331`, `BUILTINS` row at `builtins.rs:975`, in the tree since 2026-05-04 —
it predates the scan) is a pure `(i64, i64) -> str` builtin lowered via the EXACT hand-built
out-param shim shape `STR_OUT_EXTERNS` was created to replace (alloca out_len/out_ptr, call
`__axon_i64_to_str_radix`, reassemble `AxonStr`). Its `[I64, I64]` leading params are covered by
the second batch's "mixed leading-arg shapes all work unchanged" synthesis; it falls into none of
the ineligible categories. It is now a named remaining candidate in §6.1 (likely a trivial 13th
`STR_OUT_EXTERNS` row). Borderline sibling: `to_str_f64`/`__axon_f64_to_str`
(`codegen/builtins.rs:~407`) is the identical `[F64] -> Str` shim shape but is DECLARED covered
by the "to_str polymorphic-dispatch" bespoke family (it is the dispatch target `to_str` lowers
to), so it is allowlisted-with-reason, not a migration gap — recorded explicitly so the reverse
drift test (§6.2) can encode that reading. Two independent misses of the same hand-scan mean the
scan itself is no longer trusted as evidence: re-running it mechanically is now part of the §6
exit condition.
**Count correction 2026-07-31 (adversarial review)**: every raw `BUILTIN_EXTERNS` row count in
this spec was off by one — actual counts, verified against git history (`612e4de`/`1884750`/
`e7c9635`), are 24 at Slice-3 landing (spec said 25), 31 after the 7-math batch (spec said 32),
32 after str_count (spec said 33rd row), 32 at HEAD (evidence line said 33; no row was ever
removed). The in-place figures above are corrected; nothing catches count rot because the drift
tests deliberately assert membership/arity, not counts — so prose counts here are descriptive
history, never evidence. The 5-test and 12-`STR_OUT_EXTERNS`-row figures were verified correct.
**Scope correction 2026-07-31 (ASI-trajectory review)**: the title's "single-source" holds for the
ABI half of a builtin and NOT for the containment half. `ExternSig` and `BuiltinFn` carry no effect
row, purity, or capability class; those live in three separate hand-maintained match tables that are
each **fail-open on an unregistered name**, and none appears in the 4-step authoring path Slice 4
wrote into `CLAUDE.md:211-221` — so the cheap, documented path by which new native code enters the
process is the path on which a capability can be introduced silently. Full analysis, verified
sites, and the four-step fail-closed resolution are **§7**; it is a BLOCKING addition to the §6 exit
condition. The same review added three scoped follow-on slices in **§8** — (8.1) `symbol` is
unchecked free text with only an arity check behind it, closable today at zero migration cost since
all 44 rows already follow `__axon_<axon_name>` bar 7 LLVM intrinsics; (8.2) a `coverage` field per
row, with the stated limit that this spec's behavioral evidence is hand-picked manual case sets and
11 of 44 rows have no `fuzz_parity.sh` descriptor; (8.3) generate the differential corpus from the
registry, which already describes exactly the domains `fuzz_parity.sh` transcribes by hand — §5's
double-impl-collapse aside was rewritten as a constraint on R1f-2 rather than an invitation, and the
unresolved strategic questions are parked in **§12** rather than answered.

```spec-meta
id: R1d-single-source-builtins
status-claim: Implementing
depends-on: R1-codegen-build-unblock
blocks: none
blocked-by: none
supersedes: none
related: R1b-str-return-abi, R1c-dict-runtime
conflicts-with: none
reserves: none
evidence: cargo test --lib codegen::builtin_externs -p axon-core (5 tests, 32 BUILTIN_EXTERNS rows + 12 STR_OUT_EXTERNS rows; row count corrected 2026-07-31 — the prior "33, re-verified 2026-07-20" was off by one, see the count correction in the header); scripts/fuzz_parity.sh (all 12 str-out-synthesis candidates + Slice 2 math batch); CLAUDE.md "Adding a New Builtin" (Slice 4); str_count + str-out-synthesis batches native==interp manual verification 2026-07-20 (5 + 14 + 12 cases). Slice 2's simple-batch scope found one more real candidate 2026-07-20 (str_count, a miscategorized non-out-param builtin) after the 2026-07-18 scan missed it; a 2026-07-31 mechanical re-scan found a SECOND miss, i64_to_str_radix (a pure (i64,i64)->str out-param shim — see the scan correction in the header). Remaining out-param candidates: i64_to_str_radix (likely a trivial 13th STR_OUT_EXTERNS row) plus str_split/str_join (genuinely different Array<Str> input/output shapes, confirmed by reading the codegen, not assumed) — separate, further extension. The out-param-synthesis extension landed 2026-07-20 in two batches: 7 single-Str-param candidates then 5 multi-arg candidates (str_repeat/str_slice/str_replace/str_pad_start/str_pad_end), all 12 in STR_OUT_EXTERNS now. Gate wiring corrected 2026-07-31: the 5 drift tests are now executed by scripts/gate.sh --strict (cargo test -p axon-core --lib codegen::builtin_externs, default features) — previously every gate.sh test line compiled them out, so the Slice-3 kill-gate could not fire in the pipeline. Enforcement is strict-gate-only: GitHub CI (.github/workflows/ci.yml) still runs --no-default-features on an LLVM-less runner and compiles the drift tests out (see §5). Exit condition added §6 (i64_to_str_radix + str_split/str_join synthesis + reverse-direction cross-check re-evaluation + mechanical scan re-run). ASI-trajectory review 2026-07-31 added §7 (containment metadata is NOT single-sourced and all three tables fail open on an unregistered name — verified at builtins.rs:2196/:2288, builtins.rs:2130, capabilities.rs:331; BLOCKING, folded in as §6 items alongside new §6.4 CI-reachability and §6.5 machine-checked allowlist obligations), §8 (three scoped follow-on slices: symbol-convention drift test — mechanically verified 44/44 rows conform bar 7 llvm.* intrinsics, zero migration; per-row `coverage` field — 11 of 44 rows have no fuzz_parity.sh descriptor today; registry-generated differential corpus), a rewritten §5 constraint on collapsing the double-impl (it removes the I-2 oracle's independence that R10 G1/E1406 and all 49 parity harnesses depend on — R1f-2 inherits the constraint, not the invitation), and §12 open questions Q1–Q3. NOTE the stated limit recorded in §8.2: this spec's behavioral evidence is hand-picked manual native==interp case sets, i.e. it depends on a human reviewing every generated artifact.
```

**Requirement:** R1 (native pipeline) — supports the whole stdlib without the
per-builtin "triple-write" tax. Extends `R1-codegen-build-unblock.md` (which
moves inline-IR builtins to `axon-rt` for *build speed*) with the *authoring*
half: make adding/changing a builtin **one edit, not four**.

**Decisive fork:** *How does one builtin stop being defined in 3–4 hand-synced
places?*
- **(a) Proc-macro** `#[axon_builtin]` on each axon-rt fn that emits the sig
  table entry + codegen declaration. Powerful but adds a proc-macro crate,
  compile-time cost, and a second metaprogramming layer to learn.
- **(b) One declarative registry** — a single `const BUILTIN_EXTERNS: &[…]`
  table (axon\_name, extern symbol, LLVM param/ret shape, semantic return Type)
  that codegen *iterates* to declare all externs, paired with the Rust impl in
  axon-rt. Plain data; no macro magic; greppable.
- **→ Resolve: (b) declarative registry.** It matches the codebase's existing
  style (the `BUILTINS` table is already a `const &[BuiltinFn]`), needs no new
  crate, and a human can read the whole builtin surface in one file. The
  load-bearing simplification is collapsing the ~120 hand-written
  `declare_builtins` blocks into table rows — a macro is overkill for that.

---

## 1. The tax this removes

Today a single builtin lives in 3–4 hand-synced sites (measured 2026-06-04):
1. `builtins.rs` `BUILTINS` table (184 rows) — the source-level signature.
2. `interp.rs` match arm (186 arms) — the reference semantics (the I-2 oracle).
3. codegen — **either** ~120 lines of hand-emitted LLVM IR in
   `codegen/builtins.rs` (3787 LOC) / `codegen/expr.rs` (4285 LOC), **or** an
   `axon-rt` extern + a branch in the 388-line `emit_call` `if name == "…"`
   chain.
4. `infer.rs`/`declare_builtins` `fn_return_types` (the return Type), often a
   third manual insert.

A new builtin = 3–4 edits that the parity harnesses (22 of them) catch only
*after* they drift. The two largest codegen files are this boilerplate.

## 2. Target shape (per builtin, post-R1d)

```rust
// crates/axon-rt/src/builtins/<group>.rs  — the ONE canonical impl
#[no_mangle]
pub extern "C" fn __axon_abs_i64(n: i64) -> i64 { n.checked_abs()... }
```

```rust
// crates/axon-core/src/codegen/builtin_externs.rs — ONE registry row
ExternSig {
    axon_name: "abs_i64",
    symbol:    "__axon_abs_i64",
    params:    &[L::I64],          // LLVM param shape
    ret:       L::I64,
    sem_ret:   Type::I64,          // for fn_return_types
}
```

`declare_builtins` becomes: `for e in BUILTIN_EXTERNS { declare_one(e) }` —
where `declare_one` builds the `fn_type` from `e.params`/`e.ret`, does the
get-or-`add_function`, and the two `insert`s. The ~120 hand-written blocks for
already-extern builtins collapse to ~120 table rows + one 15-line loop.

`L` is a tiny LLVM-shape enum (`I64`, `F64`, `I1`, `Str` (= `{i64,i8*}`),
`Slice`, `Ptr`, `OutParam(...)`) → resolved to `BasicMetadataTypeEnum` by
`declare_one`. It covers the by-value-scalar + str-struct + out-param shapes
that 90% of externs already use. Builtins with bespoke call-site lowering
(`to_str` polymorphic dispatch, the arr_*/dict_* inline loops) **stay as
emit_call special-cases** — R1d is for the straight `declare → link` externs,
which are the bulk and the boilerplate.

## 3. Why this is also the build-speed fix (composes with R1)

R1 moves inline-IR builtins into axon-rt to delete their IR-generation cost.
R1d is the *authoring ergonomics* that makes those moves cheap and keeps them
from regressing: once a builtin is a registry row + an axon-rt fn, there is no
hand-emitted IR to drift, and `cargo llvm-lines` for that builtin is ~0. The
two specs are one migration done twice-over: R1 deletes the IR volume, R1d
deletes the duplication that produced it.

## 4. Slices (each gated, native==interp via existing harnesses)

1. **The registry + `declare_one` loop**, seeded with the builtins *already*
   extern (`__axon_abs_i64`, `__axon_str_*`, channels, the new `__axon_dict_*`).
   Net: delete the matching hand-written `declare_builtins` blocks, replace with
   rows. No behavior change — the same `functions`/`fn_return_types` entries,
   proven by the full parity suite + gate. This is the load-bearing slice;
   everything after is volume.
2. **Migrate a batch of pure inline-IR builtins** (the str/math family
   `str_to_upper`/`str_to_lower`/`str_trim`/…, the `min/max_i32` group) from
   hand-emitted IR to axon-rt Rust + registry rows. Each batch: write the Rust
   fn (matching the interpreter's exact semantics — I-2), add the row, delete
   the inline block, gate. The parity harnesses are the safety net (a Rust port
   that drifts from the interp fails them).
3. **Cross-check / single-source the sig table**: derive (or assert at a test)
   that every `BUILTINS` row with an extern has a matching `BUILTIN_EXTERNS`
   row and vice-versa, so the two tables can't drift. Optionally generate the
   `BUILTINS` doc/sig from the registry where they overlap.
4. **Document the authoring path** in `CLAUDE.md` "Adding a New Builtin" (today
   a 5-step list) → "1. write the axon-rt fn, 2. add the registry row, 3. add a
   parity case" — and retire the inline-IR step.

## 5. Scope / honesty

R1d does **not** touch the I-2 oracle (interp stays the reference; codegen links
the same Rust the interp's semantics mirror). It does **not** convert the bespoke
call-site builtins (`to_str`, arr_*/dict_* loops) — those earn their
special-cases. The win is concentrated where the boilerplate is: the ~120
straight externs and their hand-written declaration blocks. **Scope amended
2026-07-31 (adversarial review): R1d's scope DOES include the containment
metadata — effect row, purity, capability class — not only the declare-link ABI.
See §7; that is a blocking addition to the §6 exit condition, not a follow-on.**

**Collapsing the double-impl — constraint, not invitation (rewritten 2026-07-31,
adversarial review).** This section previously carried the aside "in fact a later
step could have the interp *call* the axon-rt fn directly, collapsing the
double-impl entirely, but that's out of scope here." That is not idle:
`governance/specs/R1f-differential-parity-fuzz.md` already declares "the
prerequisite for collapsing the double-impl (R1f-2) is met" and queues R1f-2 to
take it. Framed as a maintenance win (two implementations can drift; one cannot)
— true for *accidental* drift, and false about what the double-impl is for here.
**The double-impl is the I-2 oracle, and it is load-bearing well outside R1d:**
R10's G1 correctness gate is defined as `interp(P(c)) == interp(c)` with the
explicit rule that correctness is "the interpreter oracle, never an AI judgment"
(E1406), and all 49 `*_parity.sh` harnesses — including every gate this spec
cites as its own evidence — are interp-vs-native differentials. If the interp
calls the same axon-rt symbol native links, then `native == interp` becomes a
**tautology** for exactly the builtins R1d migrated; every parity harness
covering them silently degrades from a differential test to a smoke test; and
§8.1 (symbol aliasing) and §8.2 (per-row coverage) lose the only backstop that
would have caught them. It also removes independence exactly where it is worth
most under a stronger generator: a common-mode error — or a deliberate
divergence — in the single shared implementation has no second implementation
left to disagree with it.

So, stated as a constraint R1f-2 inherits: **collapsing the double-impl removes
I-2's independence for the collapsed builtins and therefore requires either (a) a
replacement independent oracle** — per-builtin SMT/spec-level contracts, or a
golden-vector corpus frozen from the pre-collapse interp so the comparison stays
against an independent artifact — **or (b) an explicit invariant/decision record
with a named scope limit**, e.g. collapse permitted only for builtins whose
declared effect row (§7) is empty, never for effectful ones. Cross-reference
R10 §7/E1406 and R1f-2. Which of (a)/(b) is required is open — see §12 Q2.

**Testable gates:** (1) slice 1 is a pure refactor — the entire test suite +
all `scripts/*_parity.sh` harnesses (~~22~~ count corrected 2026-07-31: the
tree now has 49 matching the glob — an earlier same-day correction said 50 by
also counting `parity_all.sh`, the aggregator driver, which does not match the
glob and is not a harness; the criterion is count-free — whatever harnesses
exist must stay green, and `parity_all.sh` globs `scripts/*_parity.sh` itself
so the count-free criterion is genuinely enforced) + `gate.sh --strict` stay green with zero behavior change; (2)
each migration batch keeps its builtins' parity harness green; (3) the
cross-check test (slice 3) fails if the two tables drift. **Gate-(3) wiring
corrected 2026-07-31:** the drift tests live behind `#[cfg(feature =
"codegen")]`, and every `cargo test` line in `scripts/gate.sh` previously
compiled them out (`--no-default-features` in the standard gate) or skipped
`--lib` (`--strict` ran only `--test integration_fixtures`) — so the kill-gate
existed but could not fire in the enforcement pipeline (the documented
gate.sh-coverage-gap class). Fixed: `gate.sh --strict` now runs `cargo test -p
axon-core --lib codegen::builtin_externs` with default features, so a
table-drift regression fails the strict gate. Indexed in
`governance/specs/README.md`. **Reach honestly stated (2026-07-31 adversarial
review):** this enforcement is strict-gate-only (manual / build-loop on an
LLVM-17 box), NOT CI — `.github/workflows/ci.yml` runs `cargo test
--no-default-features -p axon-core` on an LLVM-less runner, so the drift tests
are still compiled out of the one pipeline that runs automatically on every
push/PR; a table-drift regression can merge via a CI-green PR and fires
nothing until a strict-gate run. The tests themselves are pure const-table
assertions needing no LLVM at runtime, but the module's top-level `use
inkwell::…` (`builtin_externs.rs:16`) makes them unbuildable without LLVM as
placed; splitting the tables + drift tests into a non-inkwell module so CI's
existing test line runs them is the known hardening follow-up. Two further
hardening notes, not weaknesses in the current gate: (a) the gate line relies
on a cargo-test name filter, which exits 0 on zero matches — the documented
vacuous-pass class ([[coverage-vacuous-pass-guard]]); it currently matches 5
tests (verified by running it), but should assert non-vacuity (passed>0, the
smt stage's house pattern) so a future module rename cannot silently disarm
it; (b) prose row counts in this spec are not checked by anything (the drift
tests assert membership/arity, not counts) — see the 2026-07-31 count
correction in the header.

## 6. Exit condition (added 2026-07-31; corrected same day — the spec previously never said what flips Implementing → Complete)

*(Correction 2026-07-31, adversarial review: as first written the same day,
§6.1 rested on the 2026-07-18 scan's enumeration — "only str_split/str_join
remain" — which a mechanical re-scan refuted (`i64_to_str_radix`, see the scan
correction in the header). That was the second independent miss of the same
hand-scan, so the exit condition now (a) names the missed candidate, (b)
records the `to_str_f64` allowlist reading explicitly, and (c) makes re-running
the scan part of the exit condition itself rather than trusting any prose
enumeration.)*

R1d flips to ✅ Complete when all of the following are either done or
explicitly re-scoped out with evidence:

1. **Remaining out-param candidates** —
   - **`i64_to_str_radix`** (missed by the 2026-07-18 scan): a pure
     `(i64, i64) -> str` out-param shim (`codegen/builtins.rs:3331`) whose
     `[I64, I64]` leading params the existing synthesis already supports —
     expected to be a trivial 13th `STR_OUT_EXTERNS` row; if not migrated,
     it must be explicitly allowlisted with a reason.
   - **`str_split` / `str_join`** — the two genuinely-different-shape
     candidates (`Array<Str>` output / slice-arg input; hand-written blocks at
     `codegen/builtins.rs:1308` and `:3326`, `emit_call` lowering in
     `codegen/expr.rs`).
   All IN scope as the final synthesis sub-slice (status-claim Implementing
   means work remains), gated the same way as the two STR_OUT batches: drift
   tests extended to any new table/rows + `fuzz_parity.sh` + a manual
   native==interp case set.
   *Allowlist reading recorded:* `to_str_f64`/`__axon_f64_to_str` is the same
   `[F64] -> Str` shim shape but IS covered by the bespoke "`to_str`
   polymorphic dispatch" family (it is the dispatch target `to_str` lowers
   to) — allowlisted-with-reason, not a migration gap; the reverse drift test
   in (2) must encode `to_str` as including its monomorphic dispatch targets.
2. **Slice-3 reverse-direction cross-check** — "does every eligible `BUILTINS`
   row have a registry entry". The 2026-07-18 "no marker exists, not cleanly
   automatable" assessment is **partially stale as of 2026-07-31**: `ExternSig`
   has since grown `fn_key`/`ret_type` fields and `STR_OUT_EXTERNS` covers the
   out-param family, so `BUILTIN_EXTERNS ∪ STR_OUT_EXTERNS` jointly enumerate
   most non-bespoke names — the reverse check now reduces to maintaining an
   explicit allowlist of the known-bespoke families (dict ops, `to_str`
   incl. its monomorphic dispatch targets like `to_str_f64`, panic helpers,
   `ai_*`/effectful) and asserting every other `BUILTINS` name with an
   `__axon_*` symbol appears in one of the two registries. Note this test as
   proposed would already FAIL on `i64_to_str_radix` today — independent
   confirmation that (1) is real. Re-evaluate and either land it as a third
   drift test or record concrete evidence it is still not worth the allowlist
   maintenance.
3. **Mechanical scan re-run as evidence** — re-run the `__axon_*` symbol scan
   (`codegen/builtins.rs` + `codegen/expr.rs` against both registries) at
   completion time and record its output in the evidence line. The hand-scan
   has now missed twice (str_count 2026-07-20, i64_to_str_radix 2026-07-31);
   no prose enumeration of "what remains" is acceptable evidence without a
   fresh scan behind it. Landing (2) as an automated drift test satisfies
   this permanently.

4. **Drift tests reachable from the automatic pipeline** (added 2026-07-31,
   adversarial review; promoted from the §5 "known hardening follow-up"). The 5
   drift tests are the Slice-3 kill-gate, and today only `gate.sh --strict` — a
   human-initiated, single-machine, batch instrument on an LLVM-17 box — runs
   them; CI is the only *automatic* gate and compiles them out. Every
   machine-rate contributor to this repo (the `/build-loop` skill, which runs
   unattended to a full-suite-green stop condition; R10, whose entire purpose is
   to have the compiler propose its own changes) is optimizing against what the
   automatic pipeline checks, so the fraction of changes whose first and possibly
   only gate is CI rises monotonically. That reclassifies the fix from hardening
   to load-bearing: **it decides whether Slice 3 is a kill-gate or
   documentation.** The tests are pure const-table assertions needing no LLVM at
   runtime; only the module's top-level `use inkwell::…`
   (`builtin_externs.rs:16`) blocks them, so the fix is a file split (tables +
   drift tests into a non-inkwell module that CI's existing
   `cargo test --no-default-features -p axon-core` line already compiles), not a
   design change. Also in scope: the non-vacuity assertion (`passed>0`, the smt
   stage's house pattern) for the strict-gate name filter, so a future module
   rename cannot silently disarm the gate ([[coverage-vacuous-pass-guard]]).
5. **The bespoke allowlist carries a machine-checked obligation, not a prose
   one** (added 2026-07-31, adversarial review). §6.1's "explicitly allowlisted
   with a reason" and §6.2's "allowlist of the known-bespoke families" both make
   a plausible sentence the exit criterion for the one test that would otherwise
   mechanically enumerate the unregistered surface — and note what an allowlist
   entry *buys*: exemption from the registry is exemption from every per-row
   invariant §7/§8 attach to rows (declared effects, symbol convention, named
   coverage). Left as prose, the allowlist becomes the standing bypass for every
   mechanical control this spec grows, gated on a justification, and "add it to
   the bespoke allowlist with a reason" is the path of least resistance for a
   hurried human as much as for an acceptance-gate-optimizing generator. So an
   entry must name (a) its bespoke lowering site (`file:symbol`, asserted to
   exist) and (b) its covering parity harness (the same `Coverage` obligation
   rows carry per §8.2) — a reason string may accompany that but is never
   sufficient alone. Additionally assert the allowlist's length against a
   literal in the test, so growth is a visible diff line rather than an
   invisible one. (The `to_str_f64` reading in (1) is a genuine entry and
   survives this rule; the point is that it should have to *pay* for the
   exemption mechanically, as it can.)

Completing (1)+(2)+(3)+(4)+(5) — or closing them as out-of-scope with reasons —
flips the status to Complete (Slices 1/3/4 landed; Slice 2's simple-batch +
out-param-synthesis scope is done through the 12-row `STR_OUT_EXTERNS` table,
minus the (1) candidates above). §7 (containment metadata) is a **blocking**
addition to that list: see §7's own exit line.

## 7. Threat model — what "single-source" does and does NOT cover (added 2026-07-31, adversarial review)

**The word "single-source" in this spec's title is true of the ABI half of a
builtin and false of the containment half, and the two halves fail in opposite
directions.** Verified in the tree 2026-07-31:

- `BuiltinFn` (`builtins.rs:13-22`) carries `name`/`params`/`ret`/`doc`.
- `ExternSig` (`builtin_externs.rs:78-96`) carries `symbol`/`params`/`ret`/
  `fn_key`/`ret_type` — ABI shape plus the semantic return type.
- **Neither carries the builtin's effect row, purity, or capability class.**

Those live in three *independent*, hand-maintained, name-keyed match tables,
every one of which treats an unregistered name as harmless:

| Table | Site | Default for an unknown name |
|---|---|---|
| `builtin_effect_row` | `builtins.rs:2196`, fallback `builtins.rs:2288` | `_ => &[]` — the pure row |
| `is_impure_builtin` | `builtins.rs:2130` | `matches!` allowlist → `false` |
| `classify_call` / `capability_of_builtin` | `capabilities.rs:331` | `None` = pure builtin |

Those three tables are what the containment pillar actually runs on:
`effects.rs:253/:319/:419/:532/:806` (E1310 subsumption, the transitive
anti-laundering walk, handler discharge, the `@[contained]`→row bridge),
`checker.rs:2105` (`@[pure]`, E1207), `codegen/expr.rs:7371` (runtime effect
stamping for the `sandbox_run` ceiling and the audit ledger), and
`capabilities.rs:304` `program_capabilities` — which is the input to R10's G2
capability-monotonicity gate, the firewall the ROADMAP relies on for
"self-modification cannot weaken the TCB".

Now read Slice 4's own deliverable, `CLAUDE.md:211-221`: the R1d fast path is
(1) `BUILTINS` row, (2) axon-rt impl, (3) `ExternSig` row, (4) a parity test.
**Not one of the three security tables appears in either path.**

**Concrete failure.** A new builtin `net_fetch`, added by exactly the documented
fast path with a legitimately capable `__axon_net_fetch` in axon-rt, is
classified pure by all three tables *by omission*. It then passes `@[pure]`, is
invisible to E1310 effect subsumption, is not denied inside `@[contained]`, does
not trip the `sandbox_run` effect ceiling (exit 8), is not tagged in the R28
audit ledger, and contributes nothing to `program_capabilities` — so R10's G2
sees a capability-*preserving* change. The existing guard
`builtin_effect_row_agrees_with_impurity` (`builtins.rs:2656`) does not catch
it: it asserts the two tables *agree*, and empty-row + not-impure agree
perfectly.

**Threat model correction.** This is not a hypothetical adversarial generator: a
merely careless one produces it, because the cheap, documented, one-edit path
*is* the fail-open path. An optimizing one finds it by reading this spec plus
CLAUDE.md — R1d is precisely the route by which new native code enters the
process, and it is the one place a capability can be introduced with no artifact
recording that a capability was introduced. So R1d single-sources the half of a
builtin that is a *compile error* when wrong, and leaves un-sourced the half
that is a *silent containment bypass* when wrong, while driving the marginal
cost of taking that path toward zero.

**Resolution (IN scope, blocking §6 completion — do not close R1d without it):**

1. Add a **required** `effects: &'static [&'static str]` field to `ExternSig`
   and `StrOutSig`. Required, not defaulted: `&[]` then means "somebody typed
   Pure", a declaration a reviewer and a diff can see, not a silence.
2. Have `builtin_effect_row` consult the registries first for registry-backed
   names and fall through to the existing match only for the bespoke families,
   so the row is the source of truth rather than a fourth copy; add a drift test
   asserting the two can never disagree.
3. Add a **fail-closed** drift test: every `BUILTINS` name must appear either in
   a registry row (which now declares its effects) or in an explicit
   `KNOWN_PURE`/bespoke allowlist carrying §6.5's obligations — so a name nobody
   classified fails the gate instead of defaulting to pure. Wire the same shape
   for `classify_call`.
4. Update `CLAUDE.md:211-221` (Slice 4's own deliverable) so **both** paths name
   the effect/purity/capability step explicitly.

§5's scope statement is amended accordingly: R1d's scope includes the
containment metadata, not only the declare-link ABI.

## 8. Follow-on slices (scoped, added 2026-07-31, adversarial review)

Each is a concrete, sized slice — not aspiration — and each converts a
human-rate instrument into a mechanical one.

### 8.1 `symbol` is unchecked free text — make it a derived value

`ExternSig.symbol` (`builtin_externs.rs:84`) is the symbol codegen links, and
**nothing asserts it**. Reading all five drift tests
(`builtin_externs.rs:661-751`): the two `*_matches_a_known_builtin_with_the_same_arity`
tests check `axon_name` → `BUILTINS` and `params.len()` equality; the two
`no_duplicate_*` tests and `str_out_and_builtin_externs_never_name_the_same_builtin`
check *name* uniqueness. `symbol` is checked for neither uniqueness,
correspondence to `axon_name`, nor existence.

Two consequences:

- **Arity-only means type drift is invisible.** A row declaring
  `params: &[L::I64, L::I64]` for an axon-rt fn taking `(AxonStr, AxonStr)`
  passes every drift test and links cleanly — a silent ABI mismatch at native
  runtime with no diagnostic. This is the shape-vs-content failure class the
  repo has already eaten once (the golden-IR `@[packed]` i64-store memory
  corruption that shipped for a month, [[golden-ir-shape-vs-content-gap]]).
- **Name/implementation aliasing.** One row can bind a harmless `axon_name` to a
  powerful `symbol`, and *every* downstream security decision keys on the
  source-level name (`builtin_effect_row`, `is_impure_builtin`, `classify_call`
  all match on it, per §7) — so the call inherits the harmless name's pure
  classification while executing the powerful symbol. The only thing between
  that row and production is a differential parity case for that specific
  builtin, which §8.2 shows is manual and incomplete.

Cheap to close, because the convention is already perfectly regular: a
mechanical check of all 44 rows (2026-07-31) found **every** non-intrinsic row's
symbol is exactly `"__axon_" + axon_name`, with exactly 7 exceptions, all LLVM
math intrinsics (`sqrt`→`llvm.sqrt.f64`, `pow`, `floor`, `ceil`, `exp`,
`log10`, and the one name-divergent case `ln`→`llvm.log.f64`). **Zero
mismatches today**, so the invariant can be asserted immediately with no
migration.

**Slice:** (a) a sixth drift test asserting `symbol == format!("__axon_{axon_name}")`
OR `symbol` starts with `llvm.` and is in a small explicit `LLVM_INTRINSIC_ROWS`
allowlist (7 entries today); (b) symbol uniqueness asserted across **both**
tables; (c) extend the arity check to compare each `L` shape against the
`BUILTINS` row's source-level param type strings (`I64`/`F64`/`Str`/`Bool` are
mechanically mappable), so a wrong *shape* fails the gate rather than corrupting
memory at runtime.

### 8.2 Per-row coverage as a declared, checked field

**Stated limit (this is the assumption that expires).** This spec's load-bearing
behavioral evidence is human-rate throughout: "a manual native==interp check
across 14 hand-picked cases", "12 more hand-picked cases", str_count's "5 cases
incl. empty needle, empty haystack, overlapping matches", and for `pow`
explicitly "(not in the fuzz corpus) checked manually via `axon run`/`axon
build`". §5 gate (1) is "the entire suite + all harnesses stay green" — but
green-ness of 49 harnesses is evidence of no behavior change *only for the
builtins those harnesses exercise*, and nothing in the tree establishes which
rows those are.

Measured 2026-07-31: of the 44 registry rows, **11 have no `fuzz_parity.sh`
descriptor at all** — `dict_has`, `dict_inc`, `dict_len`, `dict_merge`,
`dict_new`, `now_ms`, `pow`, `sleep_ms`, `str_count`, `str_pad_start`,
`str_pad_end`. Several are genuinely covered elsewhere (`dict_*` by
`dict_parity.sh`, the pads by `str_utf8_parity.sh`, `str_count` by
`str_count_parity.sh`) and two are legitimately unfuzzable (`now_ms`,
`sleep_ms`) — but **that mapping exists only in a reviewer's head**; there is no
artifact linking a row to the harness covering it, and `pow` appears to be
covered by nothing but a one-time manual session. This is the vacuous-pass class
this spec already cites for the gate.sh name filter, applied one level up to the
migration gate itself.

The assumption that expires is *not* "models get better". It is that the
marginal cost of **adding** a row (which R1d deliberately drove to zero, and
which CLAUDE.md now advertises as a 4-step recipe an agent can follow) stays
coupled to the marginal cost of **verifying** one (a human hand-picking 12
Unicode edge cases per batch). R1d decoupled exactly those two costs, in the
direction that makes the unverified row the default outcome at volume. **This
spec depends on a human reviewing every generated artifact, and that is stated
here as a limit, not assumed.**

**Slice:** add a `coverage` field to `ExternSig`/`StrOutSig` naming the harness
or fuzz descriptor that exercises the row — `Coverage::Fuzz("abs_i64")`,
`Coverage::Harness("dict_parity.sh")`, `Coverage::Exempt("nondeterministic —
now_ms")` — plus a drift test asserting the named artifact actually exists (grep
`scripts/fuzz_parity.sh` for the descriptor; stat the harness path). That
converts `CLAUDE.md` step 4 from documentation into a gate and makes the 11
currently-unmapped rows visible today. **Restate §5 gate (1) as "every row's
named coverage is green"** rather than "the suite is green", since only the
former means what the spec claims.

### 8.3 Generate the differential corpus from the registry

`scripts/fuzz_parity.sh` is a hand-maintained list of ~30 `fuzz NAME domain
arity 'EXPR' [ret]` lines whose domains are exactly `i64`/`pos`/`f64`/`str`
(R1f slice 2). `ExternSig` already carries `params: &[L]` and `ret: L` with
`L ∈ {I64, I32, F64, I1, Str, Ptr, Void}` — i.e. **the registry is already a
machine-readable description of precisely the input domains R1f's author has
been transcribing by hand, one line at a time, and omitting 11 times out of
44.**

§4 slice 3 already gestures this way ("optionally generate the `BUILTINS`
doc/sig from the registry where they overlap") but points it at documentation,
the least valuable thing derivable from the table. The direction that matters:
emit a default differential-parity case for every row whose `params`/`ret` are
all scalar/str shapes, so a new row **arrives with coverage** instead of
arriving with a TODO. Cheaper now than when this spec was written — the mapping
is mechanical, and the awkward cases (bounded exponents for `pow`, NaN/overflow
modes) already have named descriptor forms in R1f slice 2b (`nan_case`,
`expect_overflow`) that a row could select via an enum.

The gate-incentive payoff is the point: today a generator optimizing for "the
gate goes green" is rewarded for adding a row and no test, because the gate
cannot tell. With generated coverage, adding a row **creates the adversarial
test against itself**, and the cheapest path to green becomes a correct
implementation.

**Slice:** a generator (build-script or a `#[test]` that writes/checks a
generated block in `fuzz_parity.sh`; better, a Rust-side differential test
walking `BUILTIN_EXTERNS` and driving interp-vs-native per row) covering every
row whose param/ret shapes are all scalar/str. Rows outside that envelope
declare `Coverage::Harness(...)`/`Coverage::Exempt(reason)` per §8.2. This
subsumes §5 gate (2) and permanently discharges §6.3's "no prose enumeration is
acceptable evidence without a fresh scan behind it" for the *behavioral* half,
the way §6.2's reverse drift test does for the membership half.

## 12. Open questions

- **Q1 — Does §7's `effects` field belong on `ExternSig`, or does the whole
  containment triple belong in a fourth table keyed by `BUILTINS` name?** The
  §7 resolution puts `effects` on the registry rows, which single-sources it for
  the 44+12 registry-backed builtins but leaves the ~140 bespoke `BUILTINS`
  names still classified by three separate match tables (the fail-closed
  allowlist in §7.3 makes their omission *visible*, not *sourced*). The
  alternative — one `SecuritySig { name, effects, capability }` table that every
  builtin, registry-backed or bespoke, must appear in — single-sources all of
  them but duplicates the join key and is a larger migration. Not resolved here;
  §7's four steps are correct under either answer and should land first.
- **Q2 — What is the replacement independent oracle if R1f-2 collapses the
  double-impl?** §5 records the constraint (a replacement oracle or a scoped
  invariant is required) but does not choose between the two candidates named
  there — per-builtin SMT/spec-level contracts, or a golden-vector corpus frozen
  from the pre-collapse interp. The corpus is far cheaper and buys independence
  against *accidental* common-mode error; contracts are the only option that
  buys it against a deliberate divergence in the shared implementation. Which is
  required is a TCB question, not an R1d question — flagged for R1f-2 rather
  than answered here.
- **Q3 — Should the bespoke allowlist (§6.5) be capped in size, or only made
  visible?** §6.5 requires machine-checked obligations plus a length literal so
  growth shows in the diff. It does not set a ceiling. A hard cap would be a
  real forcing function against "allowlist everything awkward", but nothing in
  the current tree indicates where the legitimate steady-state size is (the
  known-bespoke families — dict, `to_str` + dispatch targets, panic helpers,
  `ai_*`/effectful — are not yet enumerated to a number). Revisit once §6.2's
  reverse drift test lands and produces the real count.
