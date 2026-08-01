# Adversarial triage — crit-2.md (5 findings)

Method: each cited file opened at the cited lines; where a claim was executable, it was
re-run against `target/debug/axon` rather than trusted. Default posture: refute.

---

## F133 — hash-chained audit ledger is truncatable

**VERDICT: CONFIRMED** · severity **CRITICAL -> HIGH**

The code says what the finding claims. `verify_chain` (crates/axon-audit/src/lib.rs:339-378)
walks `entries` forward from `expected_prev = [0u8; 32]`, checking `seq == i`, `prev_hash`,
and a recomputed `entry_hash` — there is no head/length commitment anywhere, so any prefix
`0..k` of a valid chain is itself a valid chain. `Ledger::open` (lib.rs:212-224) treats a
non-existent path as success: `} else { File::create(path) ... }` then returns an
`entries: Vec::new()` ledger, and cli.rs:661-665 prints `"✓ ledger intact ({} entries,
chain verified)"` with exit 0. The tests confirm the gap is precisely tail-drop:
`missing_entry_fails_verification` (lib.rs:515-532) deletes a *middle* line only.

Downgraded from CRITICAL because the chain is **unkeyed SHA-256** — `compute_entry_hash`
(lib.rs:155-171) mixes no secret and there is no HMAC/signature anywhere in the crate
(`grep -n "hmac\|sign\|key" crates/axon-audit/src/lib.rs` → no hits). Any party who can
truncate the file can equally rewrite it end-to-end into a fully-valid chain of its
choosing, so truncation grants an attacker no capability the threat model doesn't already
concede. The finding is a true instance of an already-unsound integrity story, not a
distinct critical break.

*Fix:* anchor the head — persist `(len, head_hash)` out-of-band (or HMAC the chain with a
key the audited program cannot read) and fail `audit verify` on a 0-entry/absent ledger.

---

## F109 — `axon fmt` silently deletes `mod` declarations

**VERDICT: CONFIRMED** · severity **CRITICAL -> HIGH**

Verified in code and reproduced end-to-end. `Item::ModDecl` has exactly two mentions in
crates/axon-core/src/fmt.rs and both discard it: line 94
`if matches!(item, Item::UseDecl(_) | Item::ModDecl(_)) { continue; }` and line 139
`Item::UseDecl(_) | Item::ModDecl(_) => {}` — there is no `emit_mod` counterpart to
`emit_use` (fmt.rs:104-115). `cmd_fmt` (main.rs:3739-3746) writes the result in place with
`std::fs::write(file, &formatted)` and exits 0. Live repro: a comment-free
`mod util / use util.{double} / fn main …` ran and printed `42`; after
`axon fmt proj/fmttest.ax` (`formatted: …`, exit 0) the `mod` line was gone and the same
run returned `{"code":"E0003","message":"module \`util\` not found"}`, exit 2.

Downgraded from CRITICAL: this is a developer-tool data loss of a single declaration that
fails **loudly and immediately** (E0003, exit 2) rather than corrupting behavior silently;
the `source_has_comments` refusal (main.rs:3670-3691) shields all 5 in-repo `mod` files
today, and `axon fmt` is invoked by neither scripts/gate.sh nor any workflow.

*Fix:* add an `Item::ModDecl` emit arm beside `emit_use`, or fail-closed on any file
containing a `mod` declaration until then.

---

## F154 — principal handles are dense Vec indices, attenuation defeated by `child - 1`

**VERDICT: REFUTED** · severity **CRITICAL -> LOW**

The *mechanism* is accurately described — `principals: Vec<Principal>`
(kernel.rs:86), `root`/`mint` return `self.principals.len() - 1` (kernel.rs:116, 150), and
every accessor takes a bare index (`get`/`budget_remaining`/`authorize`/`can_mint`,
kernel.rs:153-208, guarded only by `h >= 0` at interp/builtins.rs:2775, 2790). But the
claimed *impact* — escalation — does not exist, because `principal_root` is an
**ungated builtin available to any program** (interp/builtins.rs:2674-2687: `want(5)`, take
name/net/fs_write/exec/budget, push, return handle). An attacker never needs to forge a
parent. Verified by running:

```
let fake = principal_root("root", true, true, true, 999999)
→ fake handle = 0 · fake authorized for exec? true · audit attributes to: root
```

i.e. full caps, arbitrary budget, and `root` audit attribution with zero handle
arithmetic. The registry is honest bookkeeping for *cooperating* code (its own doc calls
itself "byte-identical to the userland oracle"), not a mutual-distrust boundary — and no
builtin dispatch consults a principal for capability enforcement (that is
`sandbox_create`'s effect ceiling, which likewise takes its ceiling from the caller).
`child - 1` therefore grants no authority the language already hands out for free.

*Fix (hygiene only):* nonce-keyed handles plus gating `principal_root`/`principal_activate`,
if a within-run trust boundary is ever intended.

---

## F042 — exec effect evades the declared-effects scanner via one space

**VERDICT: CONFIRMED** · severity **CRITICAL -> HIGH**

Both scanners are literal substring matches and both are as quoted:
crates/axon-intent/src/synth.rs:96 and crates/axon-os/src/runtime.rs:232 are the identical
line `let exec = source.contains("exec(") || source.contains("spawn_proc");`, under doc
comments claiming "any ambiguity widens the set, never narrows it" (synth.rs:81) and
"deny-by-default … any ambiguity yields the FULL set" (runtime.rs:215) — both false.
Reproduced: `let r = exec ("/bin/echo", ["pwned"])` printed `out: pwned`, exit 0, while
`grep -c 'exec(' ex.ax` → `0`. The rest of the chain holds: `grant_infer::infer`
(grant_infer.rs:34-44) sets `ExecPolicy::None` when `!declared.row.exec`, `legible_bound`
(approval.rs:167-171) then prints "It may NOT: spawn processes" to the approving human,
and `wrap_in_sandbox` (runtime.rs:259) emits the `IO` tag for any fs axis while
`builtin_effect_row` maps `"exec" => &["IO"]` (builtins.rs:2211) — so the spawn is
permitted at run time.

Downgraded from CRITICAL only because runtime reachability is conditional: the sandbox
ceiling admits `exec` solely when the grant already carries an fs or exec axis (a
net-only/pure grant emits no `IO` tag and the spawn hits SandboxViolation, exit 8). The
human-facing falsification, however, is unconditional.

*Fix:* derive `DeclaredEffects` from the AST via `builtins::builtin_effect_row` (the single
source of truth) and treat un-analyzable source as `DeclaredEffects::unknown()`.

---

## F014 — grant path/host allowlists are never enforced

**VERDICT: CONFIRMED** · severity **CRITICAL -> HIGH**

The allowlists are used only for *presence*, never for comparison. `Grant::effect_set`
(crates/axon-os/src/grant.rs:95-102) is exactly
`fs_read: !self.fs_read.is_empty(), fs_write: !self.fs_write.is_empty(), …` — the `Vec<String>`
prefixes are discarded at that boundary; `gate::admit` (gate.rs:51-77) then compares only
booleans, and `wrap_in_sandbox` (runtime.rs:253-275) collapses further to
`Net`/`AI`/`IO` string tags fed to `sandbox_create(principal, "Net,AI,IO")`. The
interpreter's enforcement point (interp/builtins.rs:182-205) checks a builtin's effect row
against that comma-separated set and nothing else — there is no path or host comparison
anywhere in either crate. Meanwhile cli.rs:63-95 renders `"This program MAY: write ./out/"`
to the approving human, a scope no component ever checks.

Downgraded from CRITICAL on the same basis as F042 (axon-os is un-gated by scripts/gate.sh
and the code comment at runtime.rs:251-252 already concedes the coarseness for the
fs-vs-exec axis, though not for path scoping).

*Fix:* carry the prefix/host allowlists into the interpreter sandbox and check them inside
`read_file`/`write_file`/`http_*`/`exec` dispatch, or refuse any grant narrower than `""`
as unenforceable.
