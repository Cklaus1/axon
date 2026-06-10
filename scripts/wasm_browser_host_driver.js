// wasm_browser_host_driver.js — drive the axon-wasm interpreter with a LIVE
// (synchronous) host_await host, simulating the browser binding (R15 §13 B1):
// the program's `host_await(req)` calls the imported `axon_host_await`, JS reads
// the request from linear memory, returns the next scripted reply, and the
// program resumes — all synchronously (the async/Asyncify case is B3).
//
// Unlike the wasip1/native stdio host, the REQUEST does NOT go to stdout here —
// it's handed to the page (logged as `REQ:<req>` on stderr). The program's own
// stdout (println output) goes to this process's stdout. Exit code is the
// program's. This models a real browser host (prompts shown in UI, not stdout).
//
// Usage: node wasm_browser_host_driver.js <wasm> <ax> <reply1\nreply2\n...>
'use strict';
const fs = require('fs');

const [, , wasmPath, axPath, repliesStr] = process.argv;
const replies = repliesStr && repliesStr.length ? repliesStr.split('\n') : [];
let idx = 0;
const src = fs.readFileSync(axPath);
const wasmBytes = fs.readFileSync(wasmPath);

let mem;
const imports = {
  env: {
    axon_host_await: (reqPtr, reqLen, outPtr, outCap) => {
      const req = Buffer.from(new Uint8Array(mem.buffer, reqPtr, reqLen)).toString('utf8');
      process.stderr.write('REQ:' + req + '\n'); // the page would show this
      if (idx >= replies.length) return -1n; // end-of-input
      const reply = Buffer.from(replies[idx++], 'utf8');
      const n = Math.min(reply.length, Number(outCap));
      new Uint8Array(mem.buffer, Number(outPtr), n).set(reply.subarray(0, n));
      return BigInt(n);
    },
  },
};

WebAssembly.instantiate(wasmBytes, imports)
  .then(({ instance }) => {
    const e = instance.exports;
    mem = e.memory;
    const p = e.axon_alloc(src.length);
    new Uint8Array(mem.buffer, p, src.length).set(src);
    const code = e.axon_eval(p, src.length);
    const out = Buffer.from(
      new Uint8Array(mem.buffer, e.axon_output_ptr(), e.axon_output_len())
    );
    process.stdout.write(out);
    process.exit(code);
  })
  .catch((err) => {
    process.stderr.write('wasm_browser_host_driver: ' + err + '\n');
    process.exit(70);
  });
