# Axon in the browser

A compiled Axon program running in a web page via WebAssembly — **no server, no
wasi, no wasm-bindgen**. Just `WebAssembly.instantiate` with a single host
import.

## Run it

```bash
# 1. Build the demo for the browser target (emits demo.linked.wasm here):
axon target build examples/browser/demo.ax --target wasm32-unknown-unknown

# 2. Serve this directory over http (browsers won't fetch wasm from file://):
python3 -m http.server -d examples/browser
#    → open http://localhost:8000/ and click "Run".
```

You should see `demo.ax`'s output in the page:

```
Axon, running in your browser via WebAssembly.
Compiled to wasm32-unknown-unknown — no wasi, no server.
fib(10) = 55
HELLO, WASM
dict answer = 42
pi = 3.14159
```

## How it works

`axon target build --target wasm32-unknown-unknown` produces a **wasi-free**
module (a browser has no wasi). The Axon runtime (`axon-rt`) is compiled with
browser shims (`#[cfg(all(target_arch="wasm32", not(target_os="wasi")))]`):

- `malloc`/`free` → Rust's allocator (dlmalloc on this target)
- `puts` / `println` → an **imported** `axon_host_write(ptr, len)` the page supplies
- number formatting (`to_str`/`to_str_f64`) → `axon-rt` externs, not libc `snprintf`

So the only thing the host (this page, or `scripts/wasm_browser_host.js` for
headless testing) provides is `env.axon_host_write` — it reads the module's
linear memory and writes the bytes to a `<pre>`. That is the seam a real
wasm-bindgen / DOM / canvas integration plugs into next.

**Scope (v1):** compute, strings, dicts, and printing — everything that doesn't
need interactive input or async I/O. A real event loop / `fetch` / animation
frame is gated on Axon's effect-handler `resume` runtime (Phase 6), the same
dependency as the native-FFI UI work.

The wasm/runtime half is gated headlessly by
`scripts/wasm_browser_io_parity.sh` and the `examples/browser/demo.ax` sweep in
`cli_run` (run under Node, which stands in for the browser glue).
