# Tech Spec — R7b: `AxonHost` — the interpreter's host-interface seam

**Status:** ✅ Reviewed (2026-06-02)
**Requirement:** `../REQUIREMENTS.md` R7 — *Cross-platform targets.* Extends the R7 Slice-A wasm work (`R7-targets.md` §3.3/§4.3, the "design sketch (not built here)").
**Decisive fork:** *How are the interpreter's ~5 host touchpoints (fs / env / time / sleep) abstracted behind a single seam, such that (a) native behavior is byte-identical to today, and (b) a wasm/browser build can supply a virtual implementation — without threading a `&host` parameter through the whole interpreter?* **→ Resolved below.**

---

## 1. Motivation

R7 Slice A (shipped, `ed7c7bf`) compiles the interpreter to `wasm32-wasip1` and runs pure-compute `.ax` identically to native. The remaining gap is the **host interface**: the interpreter calls `std::fs`/`std::env`/`std::time`/`std::thread` directly in five builtins, so an I/O-using program on `wasm32-unknown-unknown` (the *browser* target — no WASI, no fs/env) would fail to link or trap. The R7 spec (§3.3/§4.3) sketched an `AxonHost` trait but explicitly left it unbuilt.

This spec resolves the trait shape and the *injection mechanism* — the latter is the real fork. Threading a `&dyn AxonHost` parameter through `call_fn`/`call_builtin`/`eval` would be a large, invasive refactor touching hundreds of call sites. The decision below avoids that.

The enumerated touchpoints (grep-verified, `interp.rs`):
| Builtin | Today | Host method |
|---|---|---|
| `read_file(path)` | `std::fs::read_to_string` | `read_file(path) -> Result<String,String>` |
| `write_file(path,data)` | `std::fs::write` | `write_file(path,data) -> Result<(),String>` |
| `env_var(key)` | `std::env::var` | `env_var(key) -> Option<String>` |
| `now_ms()` | `SystemTime::now()` | `now_ms() -> i64` |
| `sleep_ms(ms)` | `std::thread::sleep` | `sleep_ms(ms)` (no-op allowed) |

(`on_deep_stack`'s thread-scope is already `#[cfg(wasm32)]`-handled by Slice A and is *not* part of this trait — it's a stack concern, not an I/O capability.)

## 2. Requirement link

`../REQUIREMENTS.md` **R7** (42%). This is the `AxonHost` half of "Remaining: `AxonHost` trait for `unknown-unknown`/browser virtual-FS." It is a **pure refactor** on native (a `DefaultHost` reproduces today's `std::*` behavior exactly) plus the seam a future browser host plugs into. Slice A's wasip1 path keeps working unchanged.

## 3. Surface

No `.ax` language surface change. The trait is Rust-side:

```rust
/// The host capabilities the interpreter needs from its environment. A
/// `DefaultHost` (native) wraps std; a browser/wasm build supplies a virtual
/// impl (fetch-backed FS, a provided env map, performance.now). Every method
/// returns the SAME shape the builtin already returns, so behavior is identical.
pub trait AxonHost {
    fn read_file(&self, path: &str) -> Result<String, String>;
    fn write_file(&self, path: &str, data: &str) -> Result<(), String>;
    fn env_var(&self, key: &str) -> Option<String>;
    fn now_ms(&self) -> i64;
    fn sleep_ms(&self, ms: u64);
}
```

## 4. Semantics

### 4.1 The injection mechanism (the fork resolution)

**Decision: a thread-local `HOST` holding a `Box<dyn AxonHost>`, defaulting to `DefaultHost`, set via a scoped guard — NOT a threaded parameter.**

- A `thread_local! { static HOST: RefCell<Box<dyn AxonHost>> = RefCell::new(Box::new(DefaultHost)); }`.
- The five builtins call `with_host(|h| h.read_file(...))` instead of `std::fs::...` directly.
- `pub fn set_host(h: Box<dyn AxonHost>)` (and a scoped `with_host_override` guard for tests) swaps the active host.
- **Why thread-local, not a parameter:** the interpreter's `call_builtin` is reached through a deep, already-established call chain (`eval` → `eval_call` → `call_builtin`); the `OUTPUT_SINK` (R10) and `current_fn`/provenance state already use this exact pattern (thread-local / interpreter-field), so this is consistent, not novel. A threaded `&dyn AxonHost` would touch every `eval` signature for zero behavioral gain. The interpreter is single-threaded per run (the `on_deep_stack` thread owns the host), so a thread-local is correct.

### 4.2 `DefaultHost` (native — byte-identical to today)

```rust
pub struct DefaultHost;
impl AxonHost for DefaultHost {
    fn read_file(&self, p: &str) -> Result<String,String> { std::fs::read_to_string(p).map_err(|e| e.to_string()) }
    fn write_file(&self, p: &str, d: &str) -> Result<(),String> { std::fs::write(p,d).map_err(|e| e.to_string()) }
    fn env_var(&self, k: &str) -> Option<String> { std::env::var(k).ok() }
    fn now_ms(&self) -> i64 { /* the existing now_ms() body */ }
    fn sleep_ms(&self, ms: u64) { std::thread::sleep(std::time::Duration::from_millis(ms)); }
}
```

The five builtins delegate to the active host; on native the active host is `DefaultHost`, so **every existing test passes unchanged** (that is the acceptance for the refactor half).

### 4.3 The wasm/no-host story

This spec delivers the *seam* + `DefaultHost`, not a browser host. On `wasm32-unknown-unknown` a future `BrowserHost` (fetch FS, env map, `performance.now`, no-op sleep) plugs in via `set_host`. Until one is set, `DefaultHost` is the default — and `std::fs` on `unknown-unknown` returns `Err` (not a trap) for `read_file`/`write_file`, which is the spec's "without a host impl → the builtin returns `Err`, same Result shape" behavior (R7 §4.1). `now_ms`/`sleep_ms` use whatever std provides on the target. So this slice is **forward-compatible** with the browser host without building it.

### 4.4 Determinism / invariants

- **I-2 preserved:** native behavior is identical (DefaultHost == today's std calls); the seam adds an indirection, not a semantic change.
- **I-11 preserved:** `@[contained]` still gates `read_file`/`write_file` at the *static* checker before they run — the host is reached only after the capability check passes. The host is a runtime *backend*, not a capability bypass.
- **Determinism:** `now_ms`/`sleep_ms` are inherently non-deterministic (clock); a test host can stub them for reproducibility, which is an added capability, not a regression.

## 5. Type rules

No type changes — the builtins keep their existing signatures and return types.

## 6. Error codes

None new — the host methods return the same `Result`/`Option` shapes the builtins already surface as `Ok`/`Err`/`None`.

## 7. Test plan

Red test that must fail first: **`host_seam_routes_file_io_through_axonhost`** — install a test `AxonHost` (an in-memory map) via the scoped override, run a program that `write_file`s then `read_file`s a path, and assert the bytes round-trip through the *test* host (not the real fs). Fails today (no seam; the builtin hits `std::fs` directly, so the test host is never consulted).

- [ ] **Unit:** `DefaultHost` read/write/env/now behave like the direct std calls (a file written via the host is readable via std and vice-versa).
- [ ] **Seam:** a test host override intercepts `read_file`/`write_file`/`env_var` — proves the builtins route through the trait, not std.
- [ ] **Native parity:** the full existing suite passes unchanged with `DefaultHost` as the default (the refactor introduced no behavior change).
- [ ] **Scoped restore:** after a `with_host_override` guard drops, the default host is restored (no leakage across tests).

## 8. Acceptance criteria

- [x] `host_seam_routes_file_io_through_axonhost` passes. **DONE** (host.rs unit tests).
- [x] `default_host_matches_std_behavior` passes. **DONE**.
- [x] the full suite is green with the refactor (native behavior unchanged). **DONE** + nested-override regression guard.

R7 may rise 42% → ~50% on this slice (the host seam is the precondition for any browser/`unknown-unknown` build; the browser host itself + js/mobile/AOT remain).

## 9. Scope / non-goals

- **In:** the `AxonHost` trait, `DefaultHost`, the thread-local + `set_host`/scoped-override, routing the 5 builtins through it, tests.
- **Out:** a `BrowserHost` (needs wasm-bindgen + JS glue — a follow-on); changing any builtin signature or `.ax` surface; the `unknown-unknown` target build itself (wasip1 stays the gated target).
