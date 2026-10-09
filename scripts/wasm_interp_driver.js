// wasm_interp_driver.js — run a .ax file through the axon-wasm interpreter cdylib
// (the in-browser interpreter, wasm32-unknown-unknown) under Node, behaving like
// `axon run`: the program's captured stdout goes to this process's stdout and its
// exit code becomes this process's exit code. Used by wasm_browser_interp_parity.sh
// to diff the wasm interpreter against the native one.
//
// Usage: node wasm_interp_driver.js <axon_wasm.wasm> <program.ax>
//
// R50 §4 Activation: wasm32-unknown-unknown has no environment, so the engine is
// chosen through the `axon_set_engine` export (0 = tree, 1 = vm) before
// `axon_eval`. When AXON_ENGINE is set, this driver forwards it there, so the
// wasm leg runs the same engine as the native leg (which reads AXON_ENGINE).
'use strict';
const fs = require('fs');

const [, , wasmPath, axPath] = process.argv;
if (!wasmPath || !axPath) {
  process.stderr.write('usage: node wasm_interp_driver.js <wasm> <ax>\n');
  process.exit(2);
}

const src = fs.readFileSync(axPath); // Buffer of UTF-8 source bytes
const wasmBytes = fs.readFileSync(wasmPath);

// The module imports `axon_host_await` (referenced by the browser host_await
// binding). Compute programs never call it; provide an EOF stub (returns -1n)
// so instantiation succeeds. A real interactive driver supplies a live host.
const imports = {
  env: {
    axon_host_await: (_reqPtr, _reqLen, _outPtr, _outCap) => -1n,
  },
};

const ENGINE = process.env.AXON_ENGINE;
let engineCode = null;
if (ENGINE !== undefined && ENGINE !== '') {
  if (ENGINE === 'tree') engineCode = 0;
  else if (ENGINE === 'vm') engineCode = 1;
  else {
    // Same message and exit code as the native binary (§3).
    process.stderr.write(`AXON_ENGINE must be "vm" or "tree" (got "${ENGINE}")\n`);
    process.exit(2);
  }
}

WebAssembly.instantiate(wasmBytes, imports)
  .then(({ instance }) => {
    const e = instance.exports;
    if (engineCode !== null) {
      const rc = e.axon_set_engine(engineCode);
      if (rc !== 0) throw new Error(`axon_set_engine(${engineCode}) returned ${rc}`);
    }
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
