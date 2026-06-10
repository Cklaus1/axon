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

The wasm/runtime half is gated headlessly by
`scripts/wasm_browser_io_parity.sh` and the `examples/browser/demo.ax` sweep in
`cli_run` (run under Node, which stands in for the browser glue).

## Interactive Axon — `interactive.html` (suspend/resume)

`index.html` runs one **AOT-compiled** program with output only. `interactive.html`
goes further: it loads the whole **interpreter** (`axon-wasm`), instrumented with
`wasm-opt --asyncify`, so a program can call **`host_await(req)`** to *suspend the
wasm across a browser event* and *resume* exactly where it left off. That is the
coroutine capability behind REPLs, agents that pause for human approval, and frame
loops (R15 §13 B3).

```bash
# 1. Build the Asyncify-instrumented interpreter (emits axon_interp.async.wasm):
bash examples/browser/build-interactive.sh        # needs wasm-opt (binaryen)

# 2. Serve and open interactive.html:
python3 -m http.server -d examples/browser
#    → http://localhost:8000/interactive.html — edit the program, click Run.
```

The default program is a guessing game: each `host_await_opt("your guess> ")`
suspends the module, the page reveals an input box, and your reply resumes it
(type a number and Enter; "End input" sends EOF → `None`).

**How the suspend works.** Asyncify rewrites the module so it can unwind its call
stack back to JS and later rewind it. The state machine — *unwind at `host_await`
→ `await` the host's Promise → rewind to resume* — lives in **`axon_asyncify.mjs`**,
the **same module** the headless node driver (`scripts/wasm_asyncify_driver.js`)
imports. So `scripts/wasm_asyncify_host_await.sh` (gated in `cli_run` as
`wasm_asyncify_host_await_suspends_across_async_r7c`) runs the exact code this page
runs — a 2-turn program, a multi-turn loop, `host_await_opt`, and the deep-nested
guessing game all round-trip across real async (`setTimeout`/Promise) replies.

**Scope:** `host_await` payloads are strings (the serialized host/tool boundary);
full-`Value` payloads are a later R15 slice. A deep suspend point needs more JS
stack than node's ~984 KB default (`node --stack-size=…`); browsers configure
stack per worker, so this is a host knob, not an Asyncify limit.

## The playground — `playground.html` (the wedge, in the browser)

`playground.html` is a paste-and-run REPL that makes Axon's differentiator
visible in the browser: **AI code, sandboxed by the compiler**. It loads the
plain interpreter (`axon-wasm`, no Asyncify) and — crucially — runs the **static
check before running**, exactly like the `axon run` CLI. So a capability
violation surfaces *live*: paste an `@[contained]` agent that tries to escape its
grant, hit Run, and the compiler **refuses it (E1001, exit 2) before it runs** —
right in the page. The default example is exactly that; a one-click "Honest agent"
preset shows the same grant running when the code stays inside it.

```bash
# 1. Build the plain interpreter (emits axon_interp.wasm):
bash examples/browser/build-playground.sh
# 2. Serve and open:
python3 -m http.server -d examples/browser
#    → http://localhost:8000/playground.html — edit, click Run, watch the verdict.
```

The check-first behavior is the same `axon_eval` entry the headless harness
exercises: `scripts/wasm_browser_interp_parity.sh` (gated in `cli_run` as
`wasm_interpreter_evals_identically_to_native_r7c`) runs a compute corpus
through the wasm interpreter *and* asserts an over-reaching `@[contained]` program
is refused in-browser with its E1001 diagnostic — the exact path this page runs.
