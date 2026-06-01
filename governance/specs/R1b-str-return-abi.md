# Tech Spec — R1b: str-Returning Builtins ABI (str_repeat, str_slice, str_replace, str_reverse)

**Spec ID:** `R1b-str-return-abi` (sub-spec of `R1-codegen-build-unblock.md`)
**Status:** 📝 Draft (2026-06-01)
**Risk class:** Structural
**Author / date:** claude — 2026-06-01

---

### 1. Motivation

Batch 2 of R1 migrated the six **scalar-returning** string builtins (`str_contains`, `str_starts_with`, `str_ends_with`, `str_index_of`, `str_len`, `char_at`) from inline-IR to `axon-rt` extern "C" Rust. Their return types are `i64` or `bool` — no heap allocation, no struct return. The inline-IR block was replaced by a bare `add_function(name, None)` declaration with zero IR cost.

**`str_repeat`, `str_slice`, `str_replace`, `str_reverse`** were explicitly deferred (commit `4b5c9c0` message: "return str (malloc + by-value struct RETURN), a second ABI seam with no axon-rt precedent"). Each of these returns `str` — a malloc'd `{i64 len, i8* ptr}` struct — and all **14 existing str-return externs** in axon-rt (`__axon_read_file`, `__axon_read_line`, `__axon_i64_to_str_radix`, etc.) use **out-params** (`*mut i64 len, *mut *mut u8 ptr`), not by-value struct return.

This decision resolves: how must a str-returning builtin pass its result back to codegen — by-value `repr(C)` struct or out-params — and the trade-offs of each choice for the R1 migration.

**The problem:** Both options require codegen changes. By-value would leave codegen's call-site IR unchanged (the current inline blocks emit `fn_ty = str_ty.fn_type(...)` and `build_ret(result.into())`). Out-params would add alloca/glue code at every call site. The decision is not about "zero codegen change" — it's about **ABI safety + minimal IR cost + forward compatibility with the existing convention.**

---

### 2. Requirement link

`../REQUIREMENTS.md` **R1** (40%). Acceptance quote:

> *A native binary of `examples/*.ax` runs and matches interpreter output + a perf-tier benchmark.*

This spec gates on R1's **ABI contract (I-8/I-9)**: moved builtins must preserve exact observable behavior, and the C-ABI must match between codegen and axon-rt. Acceptance (§9) is framed as parity: migrated builtins' native output must match the interpreter oracle (I-2) on the same `.ax` inputs. This unblocks the "string family (str-return)" migration slice of R1 Batch 2.

Dependencies: **I-2** (interpreter oracle), **I-6** (str layout `{i64 len, ptr}`), **I-8/I-9** (error/Result shapes), the existing 38 extern builtins in axon-rt, the shipped `build_wrappers`.

---

### 3. Surface (what changes — internal, no language surface)

No `.ax` language change. The change is the **C-ABI signature** of 4 axon-rt externs and the corresponding codegen call-site adjustment.

**Chosen surface: out-params (Option B), with a thin glue layer in codegen.**

The axon-rt extern signatures for all 4 builtins:

```rust
// All use the proven out-param convention from 14 existing str-return externs.
// The caller (codegen) allocates an alloca for the str struct and passes pointers.

#[no_mangle]
pub extern "C" fn __axon_str_repeat(
    s: AxonStr,           // input string (len, ptr)
    n: i64,               // repeat count
    out_len: *mut i64,    // output: string byte length
    out_ptr: *mut *mut u8, // output: malloc'd buffer pointer
)

#[no_mangle]
pub extern "C" fn __axon_str_slice(
    s: AxonStr,           // source string
    start: i64,           // byte offset (inclusive)
    end: i64,             // byte offset (exclusive)
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
)

#[no_mangle]
pub extern "C" fn __axon_str_replace(
    s: AxonStr,           // source string
    from: AxonStr,        // substring to replace
    to: AxonStr,          // replacement string
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
)

#[no_mangle]
pub extern "C" fn __axon_str_reverse(
    s: AxonStr,           // input string
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
)
```

**Codegen call-site changes per builtin (the glue):**

For each builtin, the inline IR block is replaced by 3 lines:
1. `let out_alloca = w_alloca(str_ty, "out");` — stack slot for the result struct
2. `let out_len_ptr = w_struct_gep(out_alloca, 0, "lenptr");`
3. `let out_ptr_ptr = w_struct_gep(out_alloca, 1, "datptr");`
4. `build_call("__axon_...")` with `(AxonStr(...), args..., out_len_ptr, out_ptr_ptr)`
5. `let result = w_load(str_ty, out_alloca, "result_val");` — the `{i64,ptr}` struct

This is **identical to the existing codegen pattern for the 38 already-extern str-returning builtins** (`i64_to_str_radix`, `read_file`, `read_line`, `ai_extract_*`, `ai_complete`). The glue code is template-able.

---

### 4. Semantics

#### 4.1 Per-builtin semantics (matched exactly to the interpreter oracle)

**`str_repeat(s, n)`** — interp.rs:3364-3367:
```rust
let n = as_int(&args[1])?.max(0) as usize;
ok!(Value::Str(as_str(&args[0])?.repeat(n)));
```
- `n < 0` → empty string `""` (clamped to 0)
- `n == 0` → empty string `""`
- `s == ""` → empty string `""` (any n)
- Result: `s.repeat(n)` — Rust std `str::repeat`, which is byte-dense repetition.
- Memory: `malloc(n * s.len() + 1)` bytes; caller owns buffer.

**`str_slice(s, start, end)`** — interp.rs:3383-3389:
```rust
let start = as_int(&args[1])?.max(0) as usize;
let end = (as_int(&args[2])?.max(0) as usize).min(s.len());
let start = start.min(end);
ok!(Value::Str(s.get(start..end).unwrap_or("").to_string()));
```
- Indices are **byte positions** (not char/Unicode codepoint positions), as `s.len()` and `s.get()` are byte-indexed in Rust.
- `start` clamped to `[0, min(end, s.len())]` (i.e., clamped to `[0, s_len]` then `start.min(end)`).
- `end` clamped to `[start, s.len()]`.
- `s.get(start..end)` returns `Some(&str)` if the range is valid UTF-8 boundaries, `None` otherwise → returns `""`.
- **UTF-8 boundary caveat:** Rust's `s.get(start..end)` is **byte-indexed**, not character-aligned. If `start` or `end` falls in the middle of a UTF-8 codepoint, `get()` returns `None` and the result is `""`. The codegen inline block (line 2739: `s.get(start..end).unwrap_or("")`) matches this exactly.
- Memory: `malloc(end - start + 1)` bytes; caller owns buffer.

**`str_replace(s, from, to)`** — interp.rs:3369-3371:
```rust
ok!(Value::Str(as_str(&args[0])?.replace(as_str(&args[1])?, as_str(&args[2])?)));
```
- `String::replace` semantics: replaces all non-overlapping occurrences of `from` with `to`.
- `from == ""` → Rust `replace` with empty pattern is a special case: it inserts `to` between every byte and at the beginning/end. The interpreter uses Rust's `str::replace("", to)` which interleaves `to` before every char and at boundaries. The codegen inline block (line 3228) handles this with `from_empty` guard: if `from_len == 0`, it skips the count loop and jumps directly to build — effectively copying `s` unchanged (no `from` found → no replacement). **There is a divergence here:** the interpreter's `s.replace("")` and the codegen's `strstr`-loop with `from_empty` guard produce different results for empty `from`. **This divergence already exists in the inline-IR version** — the spec holds behavior constant. The migrated extern must match the **codegen inline-IR behavior** (skip replacement when `from` is empty), which itself is what the native build currently does. The interpreter parity test (§8) must verify against the **native codegen output** for this edge case.
- Memory: `malloc(result_len + 1)` bytes; caller owns buffer.

**`str_reverse(s)`** — interp.rs:3360-3362:
```rust
ok!(Value::Str(as_str(&args[0])?.chars().rev().collect()));
```
- **IMPORTANT:** The interpreter reverses **characters** (Unicode scalar values), not bytes. `s.chars().rev().collect()` operates on `char` boundaries.
- The codegen inline block (line 1720-1769) reverses **bytes**: `buf[i] = s_ptr[s_len - 1 - i]` for each byte position. This is a byte-by-byte reversal, character-aware reversal produces a different result for multi-byte UTF-8.
- **This divergence already exists** — the spec holds behavior constant. The migrated extern must match the **codegen inline-IR behavior** (byte reversal), not the interpreter's character reversal. A parity test must confirm the codegen byte-reversal output is what the native build produces.
- Memory: `malloc(s.len() + 1)` bytes; caller owns buffer.

**Behavior table:**

| Builtin | Args | Result | malloc? | Caller frees? | Error case? |
|---|---|---|---|---|---|
| `str_repeat` | `str, i64` | `s.repeat(n.max(0))` | yes, `n*s.len()+1` | yes (no GC) | `n<0` → clamped to 0, no error |
| `str_slice` | `str, i64, i64` | `s.get(clamped_start..clamped_end) \|\| ""` | yes, `(end-start)+1` | yes (no GC) | out-of-range → empty str (not error) |
| `str_replace` | `str, str, str` | all `from` replaced with `to` | yes, result_len+1 | yes (no GC) | `from==""` → copy s unchanged |
| `str_reverse` | `str` | byte-reversed buffer | yes, `s.len()+1` | yes (no GC) | none |

#### 4.2 Memory ownership

All 4 builtins `malloc` their result buffers (the codegen inline blocks do this at lines 1724-1730, 2710-2716, 3128-3144, 3186-3266). The returned buffer is **never freed by the runtime**. This is the same pattern as `__axon_read_file` / `__axon_read_line` / `__axon_i64_to_str_radix` in axon-rt (line 401: "The caller owns the buffer and must free it").

**There is a latent memory leak** if the caller does not free the returned string — but this is the same pattern as every other str-returning builtin in the language. Phase 1-10 has no GC. The interpreter "leaks" trivially because it uses Rust `String::to_string()` which is collected when the `Value` is dropped at function return. Codegen's leaked heap buffers accumulate over execution. This is a pre-existing condition, not introduced by the migration.

#### 4.3 Error cases

None of these 4 builtins produce an error in the codegen or interpreter. They all return a valid (possibly empty) string:

- `str_repeat`: negative `n` → clamped to 0 → `""`. No error.
- `str_slice`: out-of-range indices → clamped → possibly empty string. No error.
- `str_replace`: empty `from` → copy `s`. No error.
- `str_reverse`: no error path.

**No error code is needed for these builtins.** They have no failure mode that maps to a negative-length signal (the convention for str-return externs). The negative-len-is-error convention applies to `__axon_read_file` where an I/O failure cannot produce a valid str, but these builtins always produce a valid result.

---

### 5. Type rules

N/A. Builtin signatures (`builtin_sigs`, `fn_return_types`) are unchanged — these builtins already have `str` return type registered in `builtins.rs` (lines 265-268, 582-585, 588-591, 759-762). `infer.rs` / `checker.rs` see no difference (they already treat these as opaque builtins). This is purely a codegen/runtime refactor.

---

### 6. Error codes

**None allocated.** These 4 builtins have no error paths — they always return a valid string (possibly empty). The negative-len-is-error convention from §4.2 does not apply because there is no error to signal. All error-like inputs (negative repeat count, out-of-range slice indices, empty from-string) produce valid empty strings per the interpreter semantics (I-9: documented, intentional sentinel — not a "plausible-looking wrong value").

The only error code this spec inherits from R1 is **E1601** (parity divergence, defined in `R1-codegen-build-unblock.md` §6): if the migrated axon-rt function produces different output from the inline-IR version for a given input, migration is blocked.

---

### 7. Invariants touched

- **I-2 (interpreter is reference):** Preserved + strengthened. The migrated builtins' axon-rt Rust code will mirror the interpreter's logic (for `str_repeat`, `str_replace`, `str_slice`), collapsing two implementations toward one. For `str_reverse`, the codegen inline block does byte-reversal while the interpreter does character-reversal — the extern must match the **codegen inline-IR output** (which is what the native build produces). The parity test will confirm match-to-codegen, and flag the codegen/interp divergence as a known drift (#33/#36/#37 class).
- **I-6 (canonical IR layouts):** Preserved. The `{i64 len, ptr}` str layout is unchanged. Out-params write to an alloca allocated by codegen using `str_ty` — same layout.
- **I-8/I-9 (success signal):** Preserved. All 4 builtins always succeed (valid string output). No error exit code needed.
- **I-11 (capability boundary):** Preserved. These are pure compute builtins; no I/O paths.
- **I-14 (stable codes):** Preserved. No new error codes; E1601 from R1 parent applies.

---

### 8. Test plan

**Red test that must fail first:** `str_repeat_mismatch` — port `str_repeat` to `axon-rt` as `__axon_str_repeat` with out-params, call it from codegen, and assert the returned `{len, ptr}` struct produces different output than the interpreter on a known input. Fails today: there is no `__axon_str_repeat` in axon-rt; it is inline IR.

- **[ ] Unit (axon-rt):** Each migrated builtin tested directly against value/string sweeps.
  - `str_repeat`: `"" , 0` → `""`; `"a", 0` → `""`; `"a", -5` → `""`; `"ab", 3` → `"ababab"`; `i64::MAX` → overflow guard.
  - `str_slice`: `start > end` → `""`; `start < 0` → clamped to 0; `end > len` → clamped; mid-UTF-8 byte boundaries (e.g. `"héllo".get(1..4)`).
  - `str_replace`: `from == ""` → copy; `from == to` → no-op; `to == ""` → delete all; no-occurrence → copy.
  - `str_reverse`: `""` → `""`; `"a"` → `"a"`; `"abcd"` → `"dcba"`; `"héllo"` → byte-reversal (not char-reversal).

- **[ ] Differential (the core):** For each builtin, assert `axon_rt_fn(x) == inline_ir_output(x)` over generated inputs — the **codegen inline-IR version is the oracle** for migrated builtins (since that is what the native build currently does). This runs **now**, no slow build needed.

- **[ ] Parity (interp↔codegen):** After migration, `interp` and `codegen` may diverge on `str_reverse` (interp: char-reverse; codegen: byte-reverse). This is a **pre-existing drift** that the spec documents but does not fix. The test confirms that native output matches **what codegen currently does** for each builtin. The interp↔codegen drift for `str_reverse` is noted as a separate issue.

- **[ ] ABI (link):** `cargo build -p axon-rt` produces `__axon_str_repeat`, `__axon_str_slice`, `__axon_str_replace`, `__axon_str_reverse` symbols; a check asserts every codegen `add_function(extern)` has a matching exported symbol (E1602 guard).

- **[ ] Regression:** The existing 532-test suite (interpreter path) is untouched and stays green throughout — migration must not change interpreter behavior.

- **[ ] Adversarial:**
  - Empty string: `""` as any argument
  - `n = 0`, `n = -1`, `n = i64::MAX` (repeat count boundaries)
  - Out-of-range slice: `start > s.len()`, `end > s.len()`, `start > end`
  - UTF-8 boundary in `str_slice`: `start` or `end` in the middle of a multibyte character (e.g. `0xe9` in `"héllo"`)
  - `str_replace` with `from == to`, `to == ""` (delete), `from` not in `s`
  - `str_reverse` on multibyte input (`"héllo"`, `"こんにちは"`)

---

### 9. Acceptance criteria

The migration passes in 4 slices, each independently revertible:

- **[ ] `str_repeat_native_matches_inline_ir`:** Native binary output for `str_repeat("ab", 3)` = `"ababab"` matches the inline-IR output. Symbol `__axon_str_repeat` resolves in `libaxon_rt.a`.
- **[ ] `str_slice_native_matches_inline_ir:`** Native binary output for `str_slice("hello", 1, 4)` = `"ell"` matches inline-IR output. Symbol `__axon_str_slice` resolves.
- **[ ] `str_replace_native_matches_inline_ir`:** Native binary output for `str_replace("abcabc", "a", "x")` = `"xbcxbc"` matches inline-IR output. Symbol `__axon_str_replace` resolves.
- **[ ] `str_reverse_native_matches_inline_ir`:** Native binary output for `str_reverse("abcd")` = `"dcba"` matches inline-IR output. Symbol `__axon_str_reverse` resolves.
- **[ ] All 4 symbols pass `nm -D libaxon_rt.a` verification (E1602 clean).
- **[ ] `cargo test -p axon-core --no-default-features` stays green (interpreter parity test unchanged).

---

### 10. Performance budget

**IR cost delta is net-positive.** Each migrated builtin deletes ~100–300 lines of hand-emitted inkwell IR (see lines: str_reverse 1711–1774 = 63 lines; str_slice 2682–2748 = 66 lines; str_repeat 3113–3176 = 63 lines; str_replace 3184–3331 = 147 lines; total ≈ 339 lines of inline IR deleted). The call-site glue adds ~5–7 LLVM IR instructions per call site (alloca, 2 struct gep, call, load), which is trivial compared to the ~80–150 block inline bodies deleted per builtin.

The net IR cost reduction per builtin is **strongly positive**: ~100–150 IR blocks deleted, replaced by ~1 basic block with ~10 instructions. The `declare_string_builtins` function (≈557 `build_wrappers::` calls) shrinks by ~400 calls when these 4 are removed.

---

### 11. Rollout & rollback

- **Incremental, per-builtin, parity-gated.** Each builtin moves in its own commit: write the axon-rt fn, replace the inline block with the out-param glue + extern declaration, parity test green, commit. If parity fails, `git revert` that one builtin — the inline IR is restored, nothing else touched.

- **Order:** Any order is fine; they are independent. Recommended: `str_reverse` (simplest, 1 arg) → `str_slice` (2 args) → `str_repeat` (2 args, loop logic) → `str_replace` (complexity canary).

- **Call-site changes:** The glue code is a 7-line template per builtin, identical to the pattern used for `i64_to_str_radix`, `read_file`, `read_line`, and all `ai_extract_*` builtins. It is not new codegen logic — it is applying an existing pattern.

- **Interaction with existing AxonStr:** The `#[repr(C)] AxonStr { len, ptr }` type (from Batch 2, commit `4b5c9c0`) is used for the **input** `str` parameters. The output is via out-params. No new AxonStr-by-value-return path is introduced, so the zero-precedent gap remains cleanly bounded.

- **Blast radius:** zero to the interpreter (untouched; 532 tests stay green). Native-only risk, gated by the differential parity oracle. The 38 already-extern str-returning builtins are the existence proof that the out-param link path is sound.

---

### 12. Open questions

**Q1 (confirmed — the by-value struct return ABI cannot be proven safe without the native build):** Option A (by-value `repr(C)` AxonStr return) would require that Rust's LLVM codegen's `{i64, *const u8}` struct return convention matches LLVM's `struct { i64, i8* }` return convention **exactly** — in registers (System V AMD64: `rax:rdx`; aarch64: `x0:x1`). Rust's LLVM backend does this on x86-64 and aarch64 Linux, but: (a) the existing codegen type is `struct_type(&[i64_ty, i8_ptr], false)` — **not** `struct_type(&[i64_ty, i8_ptr], true)` (packed), and Rust `repr(C)` struct layout matches this on the targets, but (b) **there is zero axon-rt precedent for by-value struct returns**. All 14 existing str-return externs use out-params precisely because this was unknown. The 38 already-extern builtins confirm the out-param path is safe. **Choose Option B (out-params) because its ABI is proven by 14 existing externs, accepting the small call-site glue cost rather than betting the first str-return migration on an unproven by-value struct return.**

**Q2 (str_reverse byte-vs-char divergence):** The interpreter reverses characters (Unicode scalar values); the codegen inline block reverses bytes. The migrated extern must match codegen output (byte reversal) to preserve native-build behavior. The interp↔codegen drift is a known issue (#33/#36/#37 class) — this spec documents it but does not fix it. A follow-up spec should decide: which is correct, and should the fix go to interp or codegen?

**Q3 (empty `from` in `str_replace`):** The codegen inline block (line 3228) skips replacement when `from` is empty (via `from_empty` guard jumping to build phase). The interpreter uses Rust's `str::replace("", to)` which interleaves `to` between every character. The migrated extern must match codegen behavior (skip replacement). This divergence is documented but not fixed — a follow-up spec should resolve which semantics are correct.

**Q4 (malloc ownership):** The caller owns returned buffers. There is no mechanism in Axon Phase 1–10 to free these (no `free` builtin for heap strings). This is a pre-existing condition, not introduced by migration. A future spec should address string lifetime management.