// wasm_asyncify_driver.js — drive the ASYNCIFY-instrumented axon-wasm interpreter
// so host_await can suspend the wasm module ACROSS an async JS operation (R15 §13
// B3). This is the real browser binding: the program calls host_await, the module
// UNWINDS back to JS, JS awaits a Promise for the reply (input box / fetch /
// requestAnimationFrame), then REWINDS the module to resume at host_await.
//
// Asyncify (binaryen): the module exports asyncify_{get_state,start_unwind,
// stop_unwind,start_rewind,stop_rewind}. State 0=normal, 1=unwinding, 2=rewinding.
// The axon_host_await import is called twice per await: once while unwinding
// (records the request, starts the unwind), once while rewinding (returns the
// reply the JS computed asynchronously).
//
// Usage: node wasm_asyncify_driver.js <asyncified.wasm> <ax> <reply1\nreply2\n...>
'use strict';
const fs = require('fs');

const [, , wasmPath, axPath, repliesStr] = process.argv;
const replies = repliesStr && repliesStr.length ? repliesStr.split('\n') : [];
let idx = 0;
const src = fs.readFileSync(axPath);
const wasmBytes = fs.readFileSync(wasmPath);

const DATA_SIZE = 256 * 1024; // Asyncify stack-save buffer
let X, mem, dataAddr;
let pendingReq = null,
  pendingReply = null,
  savedOut = null;

const imports = {
  env: {
    axon_host_await: (reqPtr, reqLen, outPtr, outCap) => {
      if (X.asyncify_get_state() === 0) {
        // Normal call → capture the request and begin unwinding the module.
        pendingReq = Buffer.from(new Uint8Array(mem.buffer, reqPtr, reqLen)).toString('utf8');
        savedOut = { outPtr: Number(outPtr), outCap: Number(outCap) };
        X.asyncify_start_unwind(dataAddr);
        return 0n; // ignored — we're unwinding
      }
      // Rewinding → deliver the asynchronously-computed reply.
      X.asyncify_stop_rewind();
      const reply = Buffer.from(pendingReply == null ? '' : pendingReply, 'utf8');
      if (pendingReply == null) return -1n; // end-of-input
      const n = Math.min(reply.length, savedOut.outCap);
      new Uint8Array(mem.buffer, savedOut.outPtr, n).set(reply.subarray(0, n));
      return BigInt(n);
    },
  },
};

(async () => {
  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
  X = instance.exports;
  mem = X.memory;
  // Asyncify data buffer: header is two i32s {current, end} at dataAddr.
  dataAddr = X.axon_alloc(DATA_SIZE);
  {
    const v = new Int32Array(mem.buffer);
    v[dataAddr >> 2] = dataAddr + 8;
    v[(dataAddr >> 2) + 1] = dataAddr + DATA_SIZE;
  }
  const srcPtr = X.axon_alloc(src.length);
  new Uint8Array(mem.buffer, srcPtr, src.length).set(src);

  let ret = X.axon_eval(srcPtr, src.length);
  while (X.asyncify_get_state() === 1) {
    // The module unwound at host_await. Stop the unwind, do ASYNC work, rewind.
    X.asyncify_stop_unwind();
    process.stderr.write('REQ:' + pendingReq + '\n');
    pendingReply = await new Promise((res) =>
      setTimeout(() => res(idx < replies.length ? replies[idx++] : null), 0)
    );
    X.asyncify_start_rewind(dataAddr);
    ret = X.axon_eval(srcPtr, src.length);
  }
  const out = Buffer.from(new Uint8Array(mem.buffer, X.axon_output_ptr(), X.axon_output_len()));
  process.stdout.write(out);
  process.exit(ret);
})().catch((e) => {
  process.stderr.write('asyncify_driver: ' + (e && e.stack ? e.stack : e) + '\n');
  process.exit(70);
});
