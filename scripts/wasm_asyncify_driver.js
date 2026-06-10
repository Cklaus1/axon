// wasm_asyncify_driver.js — node test driver for the ASYNCIFY-instrumented
// axon-wasm interpreter (R15 §13 B3). Proves host_await can suspend the wasm
// module ACROSS an async JS operation and resume. The Asyncify state-machine
// logic lives in the REUSABLE module examples/browser/axon_asyncify.mjs, which
// the browser demo page imports too — so running this driver verifies the exact
// code the browser runs. Here the async "host" is a setTimeout-resolved Promise
// reading from a scripted reply list; in the browser it is a DOM input box.
//
// Usage: node wasm_asyncify_driver.js <asyncified.wasm> <ax> <reply1\nreply2\n...>
// (a deep suspend point needs more JS stack than node's default — see the harness,
//  which runs this under `node --stack-size=4000`.)
'use strict';
const fs = require('fs');
const path = require('path');

const [, , wasmPath, axPath, repliesStr] = process.argv;
const replies = repliesStr && repliesStr.length ? repliesStr.split('\n') : [];
let idx = 0;
const source = fs.readFileSync(axPath, 'utf8');
const wasmBytes = fs.readFileSync(wasmPath);

// The async host: each request is logged (so the harness can assert what the host
// saw) and answered from the scripted list via a real Promise (null past the end).
const hostAwait = async (req) => {
  process.stderr.write('REQ:' + req + '\n');
  return new Promise((res) => setTimeout(() => res(idx < replies.length ? replies[idx++] : null), 0));
};

(async () => {
  const mod = await import(path.join(__dirname, '..', 'examples', 'browser', 'axon_asyncify.mjs'));
  const { exitCode, output } = await mod.runAxon(
    wasmBytes.buffer.slice(wasmBytes.byteOffset, wasmBytes.byteOffset + wasmBytes.byteLength),
    source,
    hostAwait
  );
  process.stdout.write(output);
  process.exit(exitCode);
})().catch((e) => {
  process.stderr.write('asyncify_driver: ' + (e && e.stack ? e.stack : e) + '\n');
  process.exit(70);
});
