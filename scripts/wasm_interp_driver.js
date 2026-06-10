// wasm_interp_driver.js — run a .ax file through the axon-wasm interpreter cdylib
// (the in-browser interpreter, wasm32-unknown-unknown) under Node, behaving like
// `axon run`: the program's captured stdout goes to this process's stdout and its
// exit code becomes this process's exit code. Used by wasm_browser_interp_parity.sh
// to diff the wasm interpreter against the native one.
//
// Usage: node wasm_interp_driver.js <axon_wasm.wasm> <program.ax>
'use strict';
const fs = require('fs');

const [, , wasmPath, axPath] = process.argv;
if (!wasmPath || !axPath) {
  process.stderr.write('usage: node wasm_interp_driver.js <wasm> <ax>\n');
  process.exit(2);
}

const src = fs.readFileSync(axPath); // Buffer of UTF-8 source bytes
const wasmBytes = fs.readFileSync(wasmPath);

WebAssembly.instantiate(wasmBytes, {})
  .then(({ instance }) => {
    const e = instance.exports;
    const len = src.length;
    const ptr = e.axon_alloc(len);
    new Uint8Array(e.memory.buffer, ptr, len).set(src);
    const code = e.axon_eval(ptr, len);
    const out = Buffer.from(
      new Uint8Array(e.memory.buffer, e.axon_output_ptr(), e.axon_output_len())
    );
    process.stdout.write(out);
    process.exit(code);
  })
  .catch((err) => {
    process.stderr.write('wasm_interp_driver: ' + err + '\n');
    process.exit(70);
  });
