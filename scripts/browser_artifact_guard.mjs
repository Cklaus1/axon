// browser_artifact_guard.mjs — prove interactive.html's loader refuses a browser
// interpreter artifact it cannot vouch for, and accepts a freshly stamped one.
// Uses the SAME examples/browser/axon_asyncify.mjs the page imports.
//
// Usage: node browser_artifact_guard.mjs <wasm> <stamp.json|-> <expect: accept|refuse>
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
const here = path.dirname(fileURLToPath(import.meta.url));
const { verifyArtifact, runAxon } = await import(path.join(here, '..', 'examples', 'browser', 'axon_asyncify.mjs'));
const [, , wasmPath, stampPath, expect] = process.argv;
const bytes = fs.readFileSync(wasmPath);
const wasm = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
const stamp = stampPath === '-' ? null : JSON.parse(fs.readFileSync(stampPath, 'utf8'));
let refused = null;
try { await verifyArtifact(wasm, stamp); } catch (e) { refused = e.message; }
if (expect === 'refuse') {
  if (refused) { console.log('refused: ' + refused); process.exit(0); }
  console.log('ACCEPTED an artifact that must be refused'); process.exit(1);
}
if (refused) { console.log('refused a good artifact: ' + refused); process.exit(1); }
// Accepted: it must also RUN (a stamp on a broken module would be a false green).
const { exitCode, output } = await runAxon(wasm, 'fn main() -> i64 {\n    println("hello")\n    0\n}\n', async () => null);
if (exitCode !== 0 || output.trim() !== 'hello') { console.log(`accepted but ran badly: exit ${exitCode} out ${JSON.stringify(output)}`); process.exit(1); }
console.log('accepted and ran: hello'); process.exit(0);
