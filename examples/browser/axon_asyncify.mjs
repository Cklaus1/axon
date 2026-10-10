// axon_asyncify.mjs — the reusable browser-async runtime for Axon (R15 §13 B3).
//
// Runs a `.ax` program in the axon-wasm INTERPRETER, instrumented with
// `wasm-opt --asyncify`, so the program's `host_await(req)` SUSPENDS the wasm
// across an async host operation (a Promise — input box / fetch /
// requestAnimationFrame) and RESUMES at the call. One implementation, two
// callers: the node test driver (scripts/wasm_asyncify_driver.js) and the
// browser demo page (interactive.html) both import this. DOM-free and host-free:
// you supply an async `hostAwait(req) -> string | null` (null = end-of-input).
//
// Asyncify (binaryen): the module exports asyncify_{get_state,start_unwind,
// stop_unwind,start_rewind,stop_rewind}; state 0=normal, 1=unwinding, 2=rewinding.
// The axon_host_await import is hit twice per await — once while unwinding (record
// the request, begin the unwind), once while rewinding (return the awaited reply).

const DATA_SIZE = 256 * 1024; // Asyncify stack-save buffer (header = two i32s)
const enc = new TextEncoder();
const dec = new TextDecoder();

// verifyArtifact(wasmBytes, stampJson) -> throws unless the bytes are the -O2
// build that build-interactive.sh stamped. The browser artifact is GENERATED and
// gitignored, so whatever file sits in examples/browser is whatever the last local
// build left behind. On 2026-09-24 that was a stale UNOPTIMIZED module that exhausts
// host memory on any program. A page must refuse a module it cannot vouch for,
// rather than run it (governance/incidents/2026-09-24-asyncify-linear-memory.md).
export async function verifyArtifact(wasmBytes, stampJson) {
  if (!stampJson || stampJson.schema !== 'axon-asyncify-artifact/1') {
    throw new Error('axon_asyncify: no build stamp for this .wasm — it was not produced by ' +
      'examples/browser/build-interactive.sh. Rebuild it rather than run an unknown module.');
  }
  if (stampJson.opt !== 'O2') {
    throw new Error(`axon_asyncify: artifact was built at -${stampJson.opt}; only -O2 is safe ` +
      '(an unoptimized asyncify module exhausts host memory). Rebuild it.');
  }
  const digest = await crypto.subtle.digest('SHA-256', wasmBytes);
  const hex = [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
  if (hex !== stampJson.sha256) {
    throw new Error('axon_asyncify: .wasm does not match its build stamp (stale or replaced ' +
      'after the build). Rebuild it: examples/browser/build-interactive.sh');
  }
}

// runAxon(wasmBytes, source, hostAwait) -> { exitCode, output }
//   wasmBytes : ArrayBuffer | Uint8Array of the asyncified axon-wasm module
//   source    : the .ax program text
//   hostAwait : async (req: string) => string | null   (null signals EOF)
export async function runAxon(wasmBytes, source, hostAwait) {
  let X, mem;
  let pendingReq = null;
  let pendingReply = null;
  let savedOut = null;

  const imports = {
    env: {
      axon_host_await: (reqPtr, reqLen, outPtr, outCap) => {
        if (X.asyncify_get_state() === 0) {
          // Normal call → capture the request and begin unwinding the module.
          pendingReq = dec.decode(new Uint8Array(mem.buffer, Number(reqPtr), Number(reqLen)));
          savedOut = { outPtr: Number(outPtr), outCap: Number(outCap) };
          X.asyncify_start_unwind(dataAddr);
          return 0n; // ignored — we are unwinding
        }
        // Rewinding → deliver the asynchronously-computed reply.
        X.asyncify_stop_rewind();
        if (pendingReply == null) return -1n; // end-of-input → Axon sees None / ""
        const reply = enc.encode(pendingReply);
        const n = Math.min(reply.length, savedOut.outCap);
        new Uint8Array(mem.buffer, savedOut.outPtr, n).set(reply.subarray(0, n));
        return BigInt(n);
      },
    },
  };

  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
  X = instance.exports;
  mem = X.memory;

  // Asyncify data buffer: header is two i32s {current, end} at dataAddr.
  const dataAddr = X.axon_alloc(DATA_SIZE);
  {
    const v = new Int32Array(mem.buffer);
    v[dataAddr >> 2] = dataAddr + 8;
    v[(dataAddr >> 2) + 1] = dataAddr + DATA_SIZE;
  }

  const srcBytes = enc.encode(source);
  const srcPtr = X.axon_alloc(srcBytes.length);
  new Uint8Array(mem.buffer, srcPtr, srcBytes.length).set(srcBytes);

  // Drive the source with the BORROWING entry point: this loop re-enters the
  // module once per suspend, and the one-shot `axon_eval` frees the buffer it is
  // given — so re-entering it would read and re-free freed memory (a double-free
  // that corrupts the wasm allocator). We keep ownership of srcPtr for the whole
  // evaluation and `axon_free` it once, in a finally, so the error path releases too.
  //
  // REQUIRED ABI. This loop re-enters the module once per suspend, so it needs the
  // BORROWING entry point: the one-shot `axon_eval` frees the buffer it is handed,
  // and re-entering it would read and re-free freed memory. There is deliberately no
  // fallback to the one-shot ABI — it cannot be made correct here. Even giving each
  // re-entry a freshly allocated copy (so no buffer is freed twice) traps with
  // `RuntimeError: unreachable` on the first rewind, because allocating during the
  // rewind perturbs the very allocator state the unwound frames resume against. A
  // module without these exports is simply too old for this driver, and saying so is
  // better than a silent double-free or a confusing wasm trap.
  if (typeof X.axon_eval_borrowed !== 'function' || typeof X.axon_free !== 'function') {
    throw new Error(
      'axon_asyncify: this .wasm predates the axon_eval_borrowed/axon_free ABI and ' +
      'cannot be driven across a suspend (the one-shot axon_eval frees the source ' +
      'buffer this loop must re-enter with). Rebuild it: examples/browser/build-interactive.sh'
    );
  }

  let ret;
  try {
    ret = X.axon_eval_borrowed(srcPtr, srcBytes.length);
    while (X.asyncify_get_state() === 1) {
      // The module unwound at host_await. Stop the unwind, await the host, rewind.
      X.asyncify_stop_unwind();
      pendingReply = await hostAwait(pendingReq);
      X.asyncify_start_rewind(dataAddr);
      ret = X.axon_eval_borrowed(srcPtr, srcBytes.length);
    }

    const output = dec.decode(new Uint8Array(mem.buffer, X.axon_output_ptr(), X.axon_output_len()));
    return { exitCode: ret, output };
  } finally {
    // Exactly once, after the evaluation has reached its terminal state. We hold the
    // buffer for the whole evaluation (axon_eval_borrowed never takes it), and the
    // finally means the error path releases it too.
    X.axon_free(srcPtr, srcBytes.length);
  }
}
