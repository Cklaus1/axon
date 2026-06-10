// wasm_browser_host.js — minimal Node host for an Axon wasm32-unknown-unknown
// (browser-target) module. Provides the one import the browser glue supplies —
// `axon_host_write(ptr, len)` — by reading the module's linear memory and
// appending to stdout. This stands in for the real wasm-bindgen/JS glue so the
// browser I/O path is testable headlessly (R7c). Usage: node wasm_browser_host.js <wasm>
'use strict';
const fs = require('fs');
const path = process.argv[2];
if (!path) { console.error('usage: node wasm_browser_host.js <wasm>'); process.exit(2); }
const buf = fs.readFileSync(path);
const chunks = [];
let mem = null;
const host = {
  axon_host_write: (ptr, len) => {
    const n = typeof len === 'bigint' ? Number(len) : len;
    const bytes = new Uint8Array(mem.buffer, ptr, n);
    chunks.push(Buffer.from(bytes));
  },
};
// rust-lld emits host imports under the `env` module by default.
const imports = { env: host };
WebAssembly.instantiate(buf, imports)
  .then(({ instance }) => {
    mem = instance.exports.memory;
    if (typeof instance.exports.main === 'function') instance.exports.main();
    process.stdout.write(Buffer.concat(chunks).toString('utf8'));
  })
  .catch((e) => { console.error('wasm_browser_host: ' + e); process.exit(1); });
