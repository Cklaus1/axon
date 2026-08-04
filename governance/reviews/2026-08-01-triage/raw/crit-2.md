### F133 — R28 hash-chained audit ledger is truncatable: an audited program can delete its own FS/Exec entries and `axon-os audit verify` reports "✓ ledger intact". Deleting the ledger entirely also verifies clean.
*crates/axon-audit/src/lib.rs (Ledger::verify / Ledger::open); crates/axon-os/src/cli.rs (audit verify)* · kind: audit-ledger

REQUIREMENTS.md R28: "✅ Landed — Ledger::open() now eagerly creates the ledger file ... call_builtin now audits every capability-bearing builtin", evidence `scripts/r28_acceptance_gate.sh PASS`. R29 layers on it ("denied-effect entry trips the R27 kill-switch").

The chain links each entry to the previous `entry_hash`, but nothing anchors the HEAD. Dropping a suffix leaves a perfectly valid 0..k chain.

Reproduction A — plain tail truncation of a real ledger produced by `axon run`:
```
$ head -2 interp.jsonl > trunc.jsonl     # 3-entry ledger -> keep first 2
$ axon-os audit verify --ledger trunc.jsonl
✓ ledger intact (2 entries, chain verified)
verify exit=0
```

Reproduction B — fully in-band, the audited program erases its own trail. `AXON_AUDIT_LEDGER` is readable via `env_var` and the ledger is an ordinary file the program can write:
```
fn wipe(led: str) -> i64 {
    let _w = write_file("./exfil2.txt", "stolen data")
    let script = "(sleep 2; head -c $(head -1 '{led}' | wc -c) '{led}' > '{led}.tmp'; mv '{led}.tmp' '{led}') >/dev/null 2>&1 &"
    let r = exec("/bin/sh", ["-c", script])
    match r { Ok(_s) => { println("cleanup scheduled") 0 }  Err(e) => { println("exec err: {e}") 1 } }
}
fn main() -> i64 { let led = env_var("AXON_AUDIT_LEDGER")  match led { Ok(p) => wipe(p)  Err(e) => { println("no ledger env: {e}") 1 } } }
```
Ledger BEFORE the detached helper fires — all four entries present, including the incriminating ones:
```
{"seq":0,..."effect":"IO","operation":"env_var",...}
{"seq":1,..."effect":"FS","operation":"write_file",...}
{"seq":2,..."effect":"Exec","operation":"exec",...}
{"seq":3,..."effect":"IO","operation":"println",...}
```
AFTER (2s later):
```
{"seq":0,..."operation":"env_var",...}
$ axon-os audit verify --ledger wipe4.jsonl
✓ ledger intact (1 entries, chain verified)
verify exit=0
```
The `write_file` and `exec` records are gone and verification passes.

Reproduction C — a ledger that does not exist at all:
```
$ axon-os audit verify --ledger /path/that/does/not/exist.jsonl
✓ ledger intact (0 entries, chain verified)
verify exit=0
```

What IS covered, showing the gap is precisely tail-drop: `missing_entry_fails_verification` (axon-audit/src/lib.rs:515-532) deletes a MIDDLE line and correctly fails, and a mid-run rewrite trips the seq check (`tamper detected at seq 4: expected seq 1, got 4`, exit 11). Only the suffix case is untested — grep for "truncat" in crates/axon-audit/src/lib.rs hits only `.truncate(true)` and the middle-deletion test's local variable named `truncated`.

**Recommendation:** Anchor the head: write a monotonically-advancing head marker (seq + entry_hash) to a separate append-only/immutable location, or have `Ledger::open()` stamp a run-start entry recording the expected prior head so a re-open detects a shortened predecessor; treat a 0-entry/absent ledger as FAIL, not PASS, when a run-start marker is expected. Add tests: tail truncation, empty file, absent file, and head-rollback. Separately, the ledger path should not be writable through the same capability set the audited program holds.

---

### F109 — `axon fmt` silently deletes `mod` declarations, breaking working multi-file programs in place
*crates/axon-core/src/fmt.rs:139* · kind: tooling-data-loss

The formatter never emits `Item::ModDecl`. In crates/axon-core/src/fmt.rs the header pass at line 77 collects only `Item::UseDecl`, and the body pass at line 139 no-ops both: `Item::UseDecl(_) | Item::ModDecl(_) => {}`. `ModDecl` therefore has no emit path anywhere and is dropped.

Reproduced end to end. Before (runs fine):
```
mod util
use util.{double}
fn main() { println("{to_str(double(21))}") }
```
`AXON_PATH=proj axon run proj/fmttest.ax` -> `42`

After `axon fmt proj/fmttest.ax` (writes in place, reports `formatted: proj/fmttest.ax`, exit 0):
```
use util::{double}

fn main() {
    println("{to_str(double(21))}")
}
```
`AXON_PATH=proj axon run proj/fmttest.ax` -> `{"code":"E0003","message":"module `util` not found"}`

I isolated which of the two edits is fatal by testing all four combinations. The `.`->`::` separator rewrite is harmless (both forms resolve to `42`). The breakage is entirely the dropped `mod`: with `mod` present both separators work; with `mod` absent both fail E0003.

Blast radius nuance: all 5 in-repo files declaring `mod` happen to contain comments, so the separate comment-refusal guard shields them today — the bug is latent in the repo but live for any user whose multi-file program is comment-free. `axon fmt` is never invoked by scripts/gate.sh or .github/workflows (grep for `axon fmt`/`fmt --check` returns nothing), and neither is the modular/AXON_PATH example, which is why this was never caught.

**Recommendation:** Add an `Item::ModDecl` emit arm in fmt.rs alongside `emit_use`. Until then, make the formatter refuse any file containing a `mod` declaration the same fail-closed way it refuses files with comments — writing in place and destroying code is far worse than declining. Add an `axon fmt --check` + round-trip-still-runs step over examples/modular to gate.sh.

---

### F154 — Principal handles are dense Vec indices — attenuation defeated by `child - 1`
*crates/axon-core/src/kernel.rs:81-215* · kind: forgeable-handle

crates/axon-core/src/kernel.rs:86 stores `principals: Vec<Principal>` and mint/root return `self.principals.len() - 1` (lines 116, 152) as the handle. Every lookup (`get(handle)`, line 135/157/171/189/208) trusts a bare `i64`. The doc comment on `principal_mint` claims "ATTENUATION BY CONSTRUCTION — child cap_X = want_X ∧ parent.X (escalation unrepresentable)", but escalation is one subtraction away.

```
$ axon run h1.ax
root handle = 0   child handle = 1
child authorized for exec? false
forged handle = 0
forged authorized for exec? true
forged budget = 999995
escalated holds exec? true  budget 999995
audit now attributes actions to: root
```
The untrusted component was handed only the attenuated handle `1` (no caps, budget 5). It recovered its parent with `child - 1`, passed `principal_authorize(forged, true, true, true)` for the full cap set, read the root's 999,995-unit budget, minted itself a fresh full-capability principal off the forged parent, and finally called `principal_activate(forged)` so all subsequent audit records are attributed to `root`. Handles are dense and allocation-ordered, so brute-forcing the whole registry is a loop from 0.

**Recommendation:** Make handles unforgeable: either a random 64-bit nonce per principal held in a map (cheap, no type changes), or an affine opaque Handle value like the R13 native-FFI slab wrapper — the codebase already has the pattern. Ban raw arithmetic on principal handles at the type level so `child - 1` does not typecheck. Also gate `principal_activate` so a program cannot re-attribute audit records to a principal it was not handed.

---

### F042 — exec effect evades the declared-effects scanner via one space, so a job approved as "may NOT spawn processes" can spawn processes
*crates/axon-intent/src/synth.rs:97* · kind: bug

Both effect scanners are substring matches on `"exec("` / `"spawn_proc"` (crates/axon-intent/src/synth.rs:97 and the duplicate at crates/axon-os/src/runtime.rs:232). The Axon parser accepts whitespace between callee and arg list: I ran `exec ("echo", ["pwned"])` and it executed the process (exit 0, "out: pwned"), while `src.contains("exec(")` is False. Chain: draft declares exec=false -> grant_infer gives ExecPolicy::None -> derive_risk returns Medium (not Critical) -> legible_bound prints "It may NOT: spawn processes" -> human approves -> axon-os builds ceiling from the GRANT, and any fs axis puts "IO" in the ceiling, and builtin_effect_row("exec") is ["IO"] -> the spawn is permitted at run time. The same scanners also omit `env_var` (IO), `http_sse_post` (Net), and `print/println` (IO), so the declared set diverges from the interpreter's own catalog in both directions.

**Recommendation:** Delete both hand-rolled scanners and derive DeclaredEffects from the compiler: walk the AST (or consume `axon ast review --json` per-fn effect data) and map each called builtin through `builtins::builtin_effect_row`, which is already the single source of truth. At minimum, until that lands, treat an un-analyzable source as DeclaredEffects::unknown() rather than as the empty set.

---

### F014 — Grant path/host allowlists are never enforced anywhere — fs_write=["./out/"] permits writing the whole filesystem
*crates/axon-os/src/runtime.rs:253* · kind: bug

`wrap_in_sandbox` (crates/axon-os/src/runtime.rs:253-275) collapses the grant to three coarse interpreter tags (net→Net,AI; any of fs_read/fs_write/exec→IO). The prefix lists themselves are only used by the static gate for *presence* (gate.rs:51-66 checks booleans) and by `legible_grant`. No component ever compares a runtime path or host against the allowlist.

VERIFIED: manifest `fs_write = ["./out/"]`, everything else empty; program `write_file("/tmp/.../PWNED4.txt", …)`. axon-os printed `✓ completed (value=0)`, exit 0, and the file was created far outside ./out/. Same class applies to `net = ["api.example.com"]` — only the coarse Net tag is passed down, so any host is reachable.

This directly falsifies what `axon-os explain` shows the approving human (cli.rs:63-95 renders "This program MAY: write ./out/"), which is the product's core claim.

**Recommendation:** Push the allowlists down to enforcement: either extend the interpreter's sandbox entry to carry path-prefix and host allowlists checked inside read_file/write_file/http_*/exec dispatch, or run the job under an OS-level confinement (bubblewrap/landlock/seccomp) derived from the grant. Until then, `legible_grant` and the spec must not claim path/host scoping, and a grant containing any prefix narrower than "" should be refused as unenforceable rather than silently widened.

---

