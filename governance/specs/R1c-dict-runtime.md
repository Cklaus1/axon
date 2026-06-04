# Tech Spec — R1c: `dict_*` Runtime Data Structure (native codegen)

**Status:** 📋 Draft (2026-06-03) — scopes the last large native-codegen gap.
**Requirement:** `../REQUIREMENTS.md` R1 — native pipeline must run the stdlib it
ships. The `dict_*` family (15 builtins, used in **6 example files**) currently
has **no native codegen** and is honestly E0910-gated; `axon run` (interpreter)
handles it today. `arr_group_by` returns a `Dict`, so it is gated on this too.

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
- `dict_set(d, k, v)` → determine v's tag from its LLVM type at the call site
  (i64→Int, f64→Float-bitcast-to-i64, str→Str with the AxonStr handle in
  payload), call the extern. (Same call-site type dispatch as `to_str` /
  `as_i64`.)
- `dict_get(d, k)` → call the extern with out-param slots; build `Option<T>`
  from the found flag + reinterpret payload by tag. v1 narrowing: the *match
  arms* in the examples are monomorphic per call site (a given dict_get is
  matched as str OR int, never both), so codegen can reinterpret the payload to
  the type the surrounding match expects — confirm against
  `examples/asi/llm_cache.ax` + the bandit demos.
- `dict_len`/`dict_has`/`dict_remove` → thin extern calls.

## 3. Slices (one gated commit each)

1. **Runtime core + `dict_new`/`set`/`get`/`has`/`len`** (native). Harness:
   `scripts/dict_parity.sh` — round-trip set→get (int + str values), has,
   len, get-missing→None. Parity native==interp. New `Type::Dict` lowering.
2. **`dict_remove` + `dict_get_or` + `dict_keys`/`dict_values`** (slice-
   returning — runtime mallocs the result).
3. **`dict_merge` + the closure ops** `dict_map_values`/`dict_each`/`dict_filter`
   (lambda fat-pointer callback into the runtime).
4. **wasm32 axon-rt build of the dict externs** (the str-ABI scalar-expansion
   `#[cfg(target_arch="wasm32")]` pattern already established for the str
   builtins applies to `__axon_dict_*` taking AxonStr by value), then
   `arr_group_by` (now its `Dict` return type is buildable). AOT-wasm parity.

## 4. Honesty / scope

Until slice 1 lands, `dict_*` stays **E0910-gated** (compile error naming the
builtin, pointing at `axon run`) — never a silently-wrong binary. The
interpreter path runs all dict programs today (I-2). This spec exists so the
work starts from a plan, not cold; the tagged-value decision is the load-bearing
one and is resolved above.

**Testable gates:** (1) round-trip set/get for int + str + f64 values
native==interp; (2) get-missing → `None`; (3) a mixed-value dict (the real
example shape) round-trips; (4) no value leaks (drop decrefs); (5) AOT-wasm
parity once slice 4 lands.
