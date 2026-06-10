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

  let ret = X.axon_eval(srcPtr, srcBytes.length);
  while (X.asyncify_get_state() === 1) {
    // The module unwound at host_await. Stop the unwind, await the host, rewind.
    X.asyncify_stop_unwind();
    pendingReply = await hostAwait(pendingReq);
    X.asyncify_start_rewind(dataAddr);
    ret = X.axon_eval(srcPtr, srcBytes.length);
  }

  const output = dec.decode(new Uint8Array(mem.buffer, X.axon_output_ptr(), X.axon_output_len()));
  return { exitCode: ret, output };
}
